use crate::ax;
use crate::foundation::{
    CFArrayRef, CFDictionaryRef, CFStringRef, CGEventCreateKeyboardEvent, CGEventFlags,
    CGEventPost, CGEventSetFlags, CGKeyCode, CGWindowID, CGWindowListCreateDescriptionFromArray,
    CPS_USER_GENERATED, CfOwned, GetProcessForPidFn, K_CG_HID_EVENT_TAP, K_CG_NULL_WINDOW_ID,
    K_CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS, K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY,
    ProcessSerialNumber, SlpsPostEventRecordToFn, SlpsSetFrontProcessWithOptionsFn,
    array_from_type_refs, cf_array_count, cf_array_iter, cf_as_dictionary,
    cf_dictionary_dictionary, cf_dictionary_i32, cf_dictionary_string, cf_dictionary_u32,
    cf_dictionary_u64, cf_number_from_u64, cf_string,
};
use crate::skylight;
use crate::{
    MacosNativeOperationError, MacosNativeProbeError, NativeBounds, RawWindow, RealNativeApi,
    enrich_real_window_app_ids, focus_window_via_process_and_raise,
};
use std::{
    collections::{HashMap, HashSet},
    ffi::{c_int, c_void},
    ptr,
};

pub(crate) fn process_serial_number_for_pid(
    api: &RealNativeApi,
    pid: u32,
) -> Result<ProcessSerialNumber, MacosNativeOperationError> {
    let Some(get_process_for_pid_symbol) = api.resolve_symbol("GetProcessForPID") else {
        return Err(MacosNativeOperationError::CallFailed("GetProcessForPID"));
    };
    let get_process_for_pid: GetProcessForPidFn =
        unsafe { std::mem::transmute(get_process_for_pid_symbol) };
    let mut psn = ProcessSerialNumber::default();
    let status = unsafe { get_process_for_pid(pid as c_int, &mut psn) };

    (status == 0)
        .then_some(psn)
        .ok_or(MacosNativeOperationError::CallFailed("GetProcessForPID"))
}

pub(crate) fn front_process_window(
    api: &RealNativeApi,
    psn: &ProcessSerialNumber,
    window_id: u64,
) -> Result<(), MacosNativeOperationError> {
    let Some(front_process_symbol) = api.resolve_symbol("_SLPSSetFrontProcessWithOptions") else {
        return Err(MacosNativeOperationError::CallFailed(
            "_SLPSSetFrontProcessWithOptions",
        ));
    };
    let front_process_with_options: SlpsSetFrontProcessWithOptionsFn =
        unsafe { std::mem::transmute(front_process_symbol) };
    let status =
        unsafe { front_process_with_options(psn, window_id as CGWindowID, CPS_USER_GENERATED) };

    (status == 0)
        .then_some(())
        .ok_or(MacosNativeOperationError::CallFailed(
            "_SLPSSetFrontProcessWithOptions",
        ))
}

pub(crate) fn make_key_window(
    api: &RealNativeApi,
    psn: &ProcessSerialNumber,
    window_id: u64,
) -> Result<(), MacosNativeOperationError> {
    let Some(post_event_symbol) = api.resolve_symbol("SLPSPostEventRecordTo") else {
        return Err(MacosNativeOperationError::CallFailed(
            "SLPSPostEventRecordTo",
        ));
    };
    let post_event_record_to: SlpsPostEventRecordToFn =
        unsafe { std::mem::transmute(post_event_symbol) };
    let window_id = u32::try_from(window_id)
        .map_err(|_| MacosNativeOperationError::MissingWindow(window_id))?;
    let mut event_bytes = [0u8; 0xf8];
    event_bytes[0x04] = 0xf8;
    event_bytes[0x3a] = 0x10;
    event_bytes[0x3c..0x40].copy_from_slice(&window_id.to_ne_bytes());
    event_bytes[0x20..0x30].fill(0xff);

    event_bytes[0x08] = 0x01;
    let press_status = unsafe { post_event_record_to(psn, event_bytes.as_ptr().cast::<c_void>()) };
    if press_status != 0 {
        return Err(MacosNativeOperationError::CallFailed(
            "SLPSPostEventRecordTo",
        ));
    }

    event_bytes[0x08] = 0x02;
    let release_status =
        unsafe { post_event_record_to(psn, event_bytes.as_ptr().cast::<c_void>()) };
    if release_status != 0 {
        return Err(MacosNativeOperationError::CallFailed(
            "SLPSPostEventRecordTo",
        ));
    }

    Ok(())
}

pub(crate) fn post_keyboard_event(
    _api: &RealNativeApi,
    key_code: CGKeyCode,
    key_down: bool,
    flags: CGEventFlags,
) -> Result<(), MacosNativeOperationError> {
    let event = unsafe {
        CfOwned::from_create_rule(CGEventCreateKeyboardEvent(
            ptr::null(),
            key_code,
            if key_down { 1 } else { 0 },
        ))
    }
    .ok_or(MacosNativeOperationError::CallFailed(
        "CGEventCreateKeyboardEvent",
    ))?;

    unsafe {
        CGEventSetFlags(event.as_type_ref(), flags);
        CGEventPost(K_CG_HID_EVENT_TAP, event.as_type_ref());
    }

    Ok(())
}

pub(crate) fn copy_window_descriptions_raw(
    api: &RealNativeApi,
    window_ids: CFArrayRef,
) -> Result<CfOwned, MacosNativeProbeError> {
    // Keep this raw for now: current callers build CFNumber-object arrays for this flow,
    // while Servo models create_description_from_array as CFArray<CGWindowID> copyables.
    let descriptions =
        unsafe { CfOwned::from_create_rule(CGWindowListCreateDescriptionFromArray(window_ids)) }
            .ok_or(MacosNativeProbeError::MissingTopology(
                "CGWindowListCreateDescriptionFromArray",
            ))?;

    if cf_array_count(descriptions.as_type_ref() as CFArrayRef) > 0 {
        return Ok(descriptions);
    }

    let target_window_ids = skylight::parse_window_ids(window_ids)?;
    let fallback = copy_matching_onscreen_window_descriptions_raw(&target_window_ids)?;
    api.debug(&format!(
        "macos_native: falling back to onscreen descriptions requested_ids={} fallback_descriptions={}",
        target_window_ids.len(),
        cf_array_count(fallback.as_type_ref() as CFArrayRef)
    ));
    Ok(fallback)
}

pub(crate) fn active_space_windows_without_app_ids(
    api: &RealNativeApi,
    space_id: u64,
) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
    let payload = skylight::copy_windows_for_space_raw(api, space_id)?;
    let raw_window_ids = skylight::parse_window_ids(payload.as_type_ref() as CFArrayRef)?;
    let visible_order = query_visible_window_order(&raw_window_ids)?;
    let descriptions = copy_window_descriptions_raw(api, payload.as_type_ref() as CFArrayRef)?;
    parse_window_descriptions(descriptions.as_type_ref() as CFArrayRef, &visible_order)
}

pub(crate) fn focus_window(
    api: &RealNativeApi,
    window_id: u64,
) -> Result<(), MacosNativeOperationError> {
    focus_window_via_process_and_raise(
        window_id,
        |target_window_id| {
            let window = window_description(api, target_window_id)?;
            window
                .pid
                .ok_or(MacosNativeOperationError::MissingWindowPid(
                    target_window_id,
                ))
        },
        |pid| process_serial_number_for_pid(api, pid),
        |psn, target_window_id| front_process_window(api, psn, target_window_id),
        |psn, target_window_id| make_key_window(api, psn, target_window_id),
        |target_window_id, pid| ax::raise_window_via_ax(api, target_window_id, pid),
    )
}

pub(crate) fn window_description(
    api: &RealNativeApi,
    window_id: u64,
) -> Result<RawWindow, MacosNativeOperationError> {
    let window_number = cf_number_from_u64(window_id).map_err(MacosNativeOperationError::from)?;
    let window_list = CfOwned::from_servo(array_from_type_refs(&[window_number.as_type_ref()]));
    let descriptions = copy_window_descriptions_raw(api, window_list.as_type_ref() as CFArrayRef)?;
    let visible_order = HashMap::new();

    parse_window_descriptions(descriptions.as_type_ref() as CFArrayRef, &visible_order)?
        .into_iter()
        .find(|window| window.id == window_id)
        .ok_or(MacosNativeOperationError::MissingWindow(window_id))
}

pub(crate) fn query_visible_window_order(
    target_window_ids: &[u64],
) -> Result<HashMap<u64, usize>, MacosNativeProbeError> {
    let onscreen_descriptions = copy_onscreen_window_descriptions_raw()?;
    let target_window_ids = target_window_ids.iter().copied().collect::<HashSet<_>>();
    let mut visible_order = HashMap::new();
    let window_number_key = cg_window_number_key();

    for (index, window) in
        cf_array_iter(onscreen_descriptions.as_type_ref() as CFArrayRef).enumerate()
    {
        let Some(window) = cf_as_dictionary(window) else {
            continue;
        };
        let Some(window_id) = cf_dictionary_u64(window, window_number_key) else {
            continue;
        };

        if target_window_ids.contains(&window_id) {
            visible_order.insert(window_id, index);
        }
    }

    Ok(visible_order)
}

pub(crate) fn copy_onscreen_window_descriptions_raw() -> Result<CfOwned, MacosNativeProbeError> {
    core_graphics::window::copy_window_info(
        K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY | K_CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS,
        K_CG_NULL_WINDOW_ID,
    )
    .map(CfOwned::from_servo)
    .ok_or(MacosNativeProbeError::MissingTopology(
        "CGWindowListCopyWindowInfo",
    ))
}

pub(crate) fn onscreen_window_ids_from_descriptions(
    payload: CFArrayRef,
) -> Result<HashSet<u64>, MacosNativeProbeError> {
    let window_number_key = cg_window_number_key();
    Ok(cf_array_iter(payload)
        .filter_map(|description| {
            let description = cf_as_dictionary(description)?;
            cf_dictionary_u64(description, window_number_key)
        })
        .collect())
}

pub(crate) fn filter_window_descriptions_raw(
    payload: CFArrayRef,
    target_window_ids: &[u64],
) -> Result<CfOwned, MacosNativeProbeError> {
    let target_window_ids = target_window_ids.iter().copied().collect::<HashSet<_>>();
    let window_number_key = cg_window_number_key();
    let matching = cf_array_iter(payload)
        .filter(|description| {
            cf_as_dictionary(*description)
                .and_then(|description| cf_dictionary_u64(description, window_number_key))
                .is_some_and(|window_id| target_window_ids.contains(&window_id))
        })
        .collect::<Vec<_>>();

    Ok(CfOwned::from_servo(array_from_type_refs(&matching)))
}

fn copy_matching_onscreen_window_descriptions_raw(
    target_window_ids: &[u64],
) -> Result<CfOwned, MacosNativeProbeError> {
    let onscreen_descriptions = copy_onscreen_window_descriptions_raw()?;
    filter_window_descriptions_raw(
        onscreen_descriptions.as_type_ref() as CFArrayRef,
        target_window_ids,
    )
}

pub(crate) fn parse_window_descriptions(
    payload: CFArrayRef,
    visible_order: &HashMap<u64, usize>,
) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
    let mut windows = Vec::new();
    let window_number_key = cg_window_number_key();
    let window_owner_pid_key = cg_window_owner_pid_key();
    let window_name_key = cg_window_name_key();
    let window_layer_key = cg_window_layer_key();

    for description in cf_array_iter(payload) {
        let description = cf_as_dictionary(description).ok_or(
            MacosNativeProbeError::MissingTopology("CGWindowListCreateDescriptionFromArray"),
        )?;
        let id = cf_dictionary_u64(description, window_number_key).ok_or(
            MacosNativeProbeError::MissingTopology("CGWindowListCreateDescriptionFromArray"),
        )?;
        let pid = cf_dictionary_u32(description, window_owner_pid_key);

        windows.push(RawWindow {
            id,
            pid,
            app_id: None,
            title: cf_dictionary_string(description, window_name_key),
            level: cf_dictionary_i32(description, window_layer_key).unwrap_or_default(),
            visible_index: visible_order.get(&id).copied(),
            frame: cg_window_bounds(description),
        });
    }

    Ok(windows)
}

pub(crate) fn assemble_real_active_space_windows(
    payload: CFArrayRef,
    visible_order: &HashMap<u64, usize>,
) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
    parse_window_descriptions(payload, visible_order).map(enrich_real_window_app_ids)
}

pub(crate) fn cg_window_number_key() -> CFStringRef {
    unsafe { core_graphics::window::kCGWindowNumber as CFStringRef }
}

pub(crate) fn cg_window_owner_pid_key() -> CFStringRef {
    unsafe { core_graphics::window::kCGWindowOwnerPID as CFStringRef }
}

pub(crate) fn cg_window_name_key() -> CFStringRef {
    unsafe { core_graphics::window::kCGWindowName as CFStringRef }
}

pub(crate) fn cg_window_layer_key() -> CFStringRef {
    unsafe { core_graphics::window::kCGWindowLayer as CFStringRef }
}

pub(crate) fn cg_window_bounds_key() -> CFStringRef {
    unsafe { core_graphics::window::kCGWindowBounds as CFStringRef }
}

fn cg_window_bounds(description: CFDictionaryRef) -> Option<NativeBounds> {
    let bounds = cf_dictionary_dictionary(description, cg_window_bounds_key())?;
    let x_key = cf_string("X").ok()?;
    let y_key = cf_string("Y").ok()?;
    let width_key = cf_string("Width").ok()?;
    let height_key = cf_string("Height").ok()?;

    Some(NativeBounds {
        x: cf_dictionary_i32(bounds, x_key.as_type_ref() as CFStringRef)?,
        y: cf_dictionary_i32(bounds, y_key.as_type_ref() as CFStringRef)?,
        width: cf_dictionary_i32(bounds, width_key.as_type_ref() as CFStringRef)?,
        height: cf_dictionary_i32(bounds, height_key.as_type_ref() as CFStringRef)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_foundation::base::CFTypeRef;

    fn cf_test_dictionary(entries: &[(CFTypeRef, CFTypeRef)]) -> CfOwned {
        CfOwned::from_servo(crate::tests::dictionary_from_type_refs(entries))
    }

    fn cf_test_array(values: &[CFTypeRef]) -> CfOwned {
        CfOwned::from_servo(array_from_type_refs(values))
    }

    #[test]
    fn matching_onscreen_window_descriptions_preserve_target_window_metadata() {
        let window_number_key = cg_window_number_key();
        let window_owner_pid_key = cg_window_owner_pid_key();
        let window_name_key = cg_window_name_key();
        let window_layer_key = cg_window_layer_key();
        let window_bounds_key = cg_window_bounds_key();
        let x_key = cf_string("X").unwrap();
        let y_key = cf_string("Y").unwrap();
        let width_key = cf_string("Width").unwrap();
        let height_key = cf_string("Height").unwrap();
        let id_11 = cf_number_from_u64(11).unwrap();
        let pid_101 = cf_number_from_u64(101).unwrap();
        let level_5 = cf_number_from_u64(5).unwrap();
        let x_10 = cf_number_from_u64(10).unwrap();
        let y_20 = cf_number_from_u64(20).unwrap();
        let width_300 = cf_number_from_u64(300).unwrap();
        let height_400 = cf_number_from_u64(400).unwrap();
        let title_alpha = cf_string("alpha").unwrap();
        let id_22 = cf_number_from_u64(22).unwrap();
        let pid_202 = cf_number_from_u64(202).unwrap();
        let level_7 = cf_number_from_u64(7).unwrap();
        let title_beta = cf_string("beta").unwrap();
        let first_bounds = cf_test_dictionary(&[
            (x_key.as_type_ref(), x_10.as_type_ref()),
            (y_key.as_type_ref(), y_20.as_type_ref()),
            (width_key.as_type_ref(), width_300.as_type_ref()),
            (height_key.as_type_ref(), height_400.as_type_ref()),
        ]);
        let first_window = cf_test_dictionary(&[
            (window_number_key as CFTypeRef, id_11.as_type_ref()),
            (window_owner_pid_key as CFTypeRef, pid_101.as_type_ref()),
            (window_name_key as CFTypeRef, title_alpha.as_type_ref()),
            (window_layer_key as CFTypeRef, level_5.as_type_ref()),
            (window_bounds_key as CFTypeRef, first_bounds.as_type_ref()),
        ]);
        let second_window = cf_test_dictionary(&[
            (window_number_key as CFTypeRef, id_22.as_type_ref()),
            (window_owner_pid_key as CFTypeRef, pid_202.as_type_ref()),
            (window_name_key as CFTypeRef, title_beta.as_type_ref()),
            (window_layer_key as CFTypeRef, level_7.as_type_ref()),
        ]);
        let onscreen_descriptions =
            cf_test_array(&[first_window.as_type_ref(), second_window.as_type_ref()]);

        let filtered = filter_window_descriptions_raw(
            onscreen_descriptions.as_type_ref() as CFArrayRef,
            &[11],
        )
        .unwrap();
        let parsed = parse_window_descriptions(
            filtered.as_type_ref() as CFArrayRef,
            &HashMap::from([(11, 0usize)]),
        )
        .unwrap();

        assert_eq!(
            parsed,
            vec![RawWindow {
                id: 11,
                pid: Some(101),
                app_id: None,
                title: Some("alpha".to_string()),
                level: 5,
                visible_index: Some(0),
                frame: Some(NativeBounds {
                    x: 10,
                    y: 20,
                    width: 300,
                    height: 400,
                }),
            }]
        );
    }
}
