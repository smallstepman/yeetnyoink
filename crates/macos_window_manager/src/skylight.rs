use crate::foundation::{
    CFArrayRef, CFDictionaryRef, CFStringRef, CfOwned, SlsCopyManagedDisplayForSpaceFn,
    SlsCopyManagedDisplaySpacesFn, SlsCopyWindowsWithOptionsAndTagsFn, SlsMainConnectionIdFn,
    SlsManagedDisplayGetCurrentSpaceFn, SlsManagedDisplaySetCurrentSpaceFn, array_from_type_refs,
    cf_array_iter, cf_as_dictionary, cf_dictionary_array, cf_dictionary_dictionary,
    cf_dictionary_i32, cf_dictionary_string, cf_dictionary_u64, cf_number_from_u64,
    cf_number_to_u64, cf_string,
};
use crate::{MacosNativeOperationError, MacosNativeProbeError, RawSpaceRecord, RealNativeApi};
use std::collections::HashSet;

pub(crate) fn main_connection_id(api: &RealNativeApi) -> Result<u32, MacosNativeProbeError> {
    let Some(symbol) = api.resolve_symbol("SLSMainConnectionID") else {
        return Err(MacosNativeProbeError::MissingTopology(
            "SLSMainConnectionID",
        ));
    };

    let main_connection_id: SlsMainConnectionIdFn = unsafe { std::mem::transmute(symbol) };
    let connection_id = unsafe { main_connection_id() };

    (connection_id != 0)
        .then_some(connection_id)
        .ok_or(MacosNativeProbeError::MissingTopology(
            "SLSMainConnectionID",
        ))
}

pub(crate) fn copy_managed_display_spaces_raw(
    api: &RealNativeApi,
) -> Result<CfOwned, MacosNativeProbeError> {
    let Some(symbol) = api.resolve_symbol("SLSCopyManagedDisplaySpaces") else {
        return Err(MacosNativeProbeError::MissingTopology(
            "SLSCopyManagedDisplaySpaces",
        ));
    };

    let copy_managed_display_spaces: SlsCopyManagedDisplaySpacesFn =
        unsafe { std::mem::transmute(symbol) };
    let connection_id = main_connection_id(api)?;
    let payload = unsafe { CfOwned::from_create_rule(copy_managed_display_spaces(connection_id)) }
        .ok_or(MacosNativeProbeError::MissingTopology(
            "SLSCopyManagedDisplaySpaces",
        ))?;

    Ok(payload)
}

pub(crate) fn current_space_for_display(
    api: &RealNativeApi,
    display_identifier: &str,
) -> Result<u64, MacosNativeProbeError> {
    let Some(symbol) = api.resolve_symbol("SLSManagedDisplayGetCurrentSpace") else {
        return Err(MacosNativeProbeError::MissingTopology(
            "SLSManagedDisplayGetCurrentSpace",
        ));
    };

    let current_space_for_display: SlsManagedDisplayGetCurrentSpaceFn =
        unsafe { std::mem::transmute(symbol) };
    let connection_id = main_connection_id(api)?;
    let display_identifier = cf_string(display_identifier)?;
    let space_id =
        unsafe { current_space_for_display(connection_id, display_identifier.as_type_ref()) };

    (space_id != 0)
        .then_some(space_id)
        .ok_or(MacosNativeProbeError::MissingTopology(
            "SLSManagedDisplayGetCurrentSpace",
        ))
}

pub(crate) fn copy_windows_for_space_raw(
    api: &RealNativeApi,
    space_id: u64,
) -> Result<CfOwned, MacosNativeProbeError> {
    let Some(symbol) = api.resolve_symbol("SLSCopyWindowsWithOptionsAndTags") else {
        return Err(MacosNativeProbeError::MissingTopology(
            "SLSCopyWindowsWithOptionsAndTags",
        ));
    };

    let copy_windows_with_options_and_tags: SlsCopyWindowsWithOptionsAndTagsFn =
        unsafe { std::mem::transmute(symbol) };
    let connection_id = main_connection_id(api)?;
    let space_number = cf_number_from_u64(space_id)?;
    let space_list = CfOwned::from_servo(array_from_type_refs(&[space_number.as_type_ref()]));
    let mut set_tags = 0i64;
    let mut clear_tags = 0i64;
    let payload = unsafe {
        copy_windows_with_options_and_tags(
            connection_id,
            0,
            space_list.as_type_ref() as CFArrayRef,
            0x2,
            &mut set_tags,
            &mut clear_tags,
        )
    };
    let payload = unsafe { CfOwned::from_create_rule(payload) }.ok_or(
        MacosNativeProbeError::MissingTopology("SLSCopyWindowsWithOptionsAndTags"),
    )?;

    Ok(payload)
}

pub(crate) fn copy_managed_display_for_space_raw(
    api: &RealNativeApi,
    space_id: u64,
) -> Result<CfOwned, MacosNativeOperationError> {
    let Some(symbol) = api.resolve_symbol("SLSCopyManagedDisplayForSpace") else {
        return Err(MacosNativeOperationError::CallFailed(
            "SLSCopyManagedDisplayForSpace",
        ));
    };

    let copy_managed_display_for_space: SlsCopyManagedDisplayForSpaceFn =
        unsafe { std::mem::transmute(symbol) };
    let connection_id = main_connection_id(api)?;
    let payload = unsafe {
        CfOwned::from_create_rule(copy_managed_display_for_space(connection_id, space_id))
    }
    .ok_or(MacosNativeOperationError::CallFailed(
        "SLSCopyManagedDisplayForSpace",
    ))?;

    Ok(payload)
}

pub(crate) fn switch_space(
    api: &RealNativeApi,
    space_id: u64,
) -> Result<(), MacosNativeOperationError> {
    let Some(symbol) = api.resolve_symbol("SLSManagedDisplaySetCurrentSpace") else {
        return Err(MacosNativeOperationError::CallFailed(
            "SLSManagedDisplaySetCurrentSpace",
        ));
    };

    let set_current_space: SlsManagedDisplaySetCurrentSpaceFn =
        unsafe { std::mem::transmute(symbol) };
    let connection_id = main_connection_id(api)?;
    let display_identifier = copy_managed_display_for_space_raw(api, space_id)?;

    unsafe {
        set_current_space(
            connection_id,
            display_identifier.as_type_ref() as CFStringRef,
            space_id,
        );
    }

    Ok(())
}

pub(crate) fn move_window_to_space(
    api: &RealNativeApi,
    window_id: u64,
    space_id: u64,
) -> Result<(), MacosNativeOperationError> {
    let Some(symbol) = api.resolve_symbol("SLSMoveWindowsToManagedSpace") else {
        return Err(MacosNativeOperationError::CallFailed(
            "SLSMoveWindowsToManagedSpace",
        ));
    };

    let move_windows_to_managed_space: unsafe extern "C" fn(u32, CFArrayRef, u64) =
        unsafe { std::mem::transmute(symbol) };
    let connection_id = main_connection_id(api)?;
    let window_number = cf_number_from_u64(window_id).map_err(MacosNativeOperationError::from)?;
    let window_list = CfOwned::from_servo(array_from_type_refs(&[window_number.as_type_ref()]));

    unsafe {
        move_windows_to_managed_space(
            connection_id,
            window_list.as_type_ref() as CFArrayRef,
            space_id,
        );
    }

    Ok(())
}

pub(crate) fn parse_display_identifiers(
    payload: CFArrayRef,
) -> Result<Vec<String>, MacosNativeProbeError> {
    let display_identifier_key = cf_string("Display Identifier")?;

    cf_array_iter(payload)
        .map(|display| {
            let display = cf_as_dictionary(display).ok_or(
                MacosNativeProbeError::MissingTopology("SLSCopyManagedDisplaySpaces"),
            )?;
            cf_dictionary_string(display, display_identifier_key.as_type_ref()).ok_or(
                MacosNativeProbeError::MissingTopology("SLSCopyManagedDisplaySpaces"),
            )
        })
        .collect()
}

pub(crate) fn parse_active_space_ids(
    payload: CFArrayRef,
) -> Result<HashSet<u64>, MacosNativeProbeError> {
    let current_space_key = cf_string("Current Space")?;
    let current_space_id_key = cf_string("Current Space ID")?;
    let current_managed_space_id_key = cf_string("CurrentManagedSpaceID")?;
    let managed_space_id_key = cf_string("ManagedSpaceID")?;
    let id64_key = cf_string("id64")?;
    let active_space_ids = cf_array_iter(payload)
        .map(|display| {
            let display = cf_as_dictionary(display).ok_or(
                MacosNativeProbeError::MissingTopology("SLSCopyManagedDisplaySpaces"),
            )?;

            cf_dictionary_u64(display, current_space_id_key.as_type_ref())
                .or_else(|| cf_dictionary_u64(display, current_managed_space_id_key.as_type_ref()))
                .or_else(|| {
                    cf_dictionary_dictionary(display, current_space_key.as_type_ref()).and_then(
                        |current_space| {
                            cf_dictionary_u64(current_space, managed_space_id_key.as_type_ref())
                                .or_else(|| {
                                    cf_dictionary_u64(current_space, id64_key.as_type_ref())
                                })
                        },
                    )
                })
                .ok_or(MacosNativeProbeError::MissingTopology(
                    "SLSCopyManagedDisplaySpaces",
                ))
        })
        .collect::<Result<HashSet<_>, _>>()?;

    (!active_space_ids.is_empty())
        .then_some(active_space_ids)
        .ok_or(MacosNativeProbeError::MissingTopology(
            "SLSCopyManagedDisplaySpaces",
        ))
}

pub(crate) fn parse_managed_spaces(
    payload: CFArrayRef,
) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
    let spaces_key = cf_string("Spaces")?;
    let mut spaces = Vec::new();

    for (display_index, display) in cf_array_iter(payload).enumerate() {
        let display = cf_as_dictionary(display).ok_or(MacosNativeProbeError::MissingTopology(
            "SLSCopyManagedDisplaySpaces",
        ))?;
        let display_spaces = cf_dictionary_array(display, spaces_key.as_type_ref() as CFStringRef)
            .ok_or(MacosNativeProbeError::MissingTopology(
                "SLSCopyManagedDisplaySpaces",
            ))?;

        for space in cf_array_iter(display_spaces) {
            let space = cf_as_dictionary(space).ok_or(MacosNativeProbeError::MissingTopology(
                "SLSCopyManagedDisplaySpaces",
            ))?;
            spaces.push(parse_raw_space_record(space, display_index)?);
        }
    }

    Ok(spaces)
}

pub(crate) fn parse_raw_space_record(
    space: CFDictionaryRef,
    display_index: usize,
) -> Result<RawSpaceRecord, MacosNativeProbeError> {
    let managed_space_id_key = cf_string("ManagedSpaceID")?;
    let space_type_key = cf_string("type")?;
    let tile_layout_manager_key = cf_string("TileLayoutManager")?;
    let tile_spaces_key = cf_string("TileSpaces")?;
    let id64_key = cf_string("id64")?;

    let managed_space_id = cf_dictionary_u64(space, managed_space_id_key.as_type_ref()).ok_or(
        MacosNativeProbeError::MissingTopology("SLSCopyManagedDisplaySpaces"),
    )?;
    let space_type = cf_dictionary_i32(space, space_type_key.as_type_ref()).ok_or(
        MacosNativeProbeError::MissingTopology("SLSCopyManagedDisplaySpaces"),
    )?;
    let tile_layout_manager =
        cf_dictionary_dictionary(space, tile_layout_manager_key.as_type_ref());
    let has_tile_layout_manager = tile_layout_manager.is_some();
    let tile_spaces = tile_layout_manager
        .and_then(|manager| cf_dictionary_array(manager, tile_spaces_key.as_type_ref()))
        .map(|tile_spaces| {
            cf_array_iter(tile_spaces)
                .filter_map(|tile_space| {
                    cf_as_dictionary(tile_space).and_then(|tile_space| {
                        cf_dictionary_u64(tile_space, managed_space_id_key.as_type_ref())
                            .or_else(|| cf_dictionary_u64(tile_space, id64_key.as_type_ref()))
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(RawSpaceRecord {
        managed_space_id,
        display_index,
        space_type,
        tile_spaces,
        has_tile_layout_manager,
        stage_manager_managed: stage_manager_managed(space),
    })
}

fn stage_manager_managed(dictionary: CFDictionaryRef) -> bool {
    [
        "StageManagerManaged",
        "StageManagerSpace",
        "isStageManager",
        "StageManager",
    ]
    .into_iter()
    .any(|key| {
        cf_string(key)
            .ok()
            .and_then(|key| cf_dictionary_u64(dictionary, key.as_type_ref() as CFStringRef))
            .is_some()
    })
}

pub(crate) fn parse_window_ids(payload: CFArrayRef) -> Result<Vec<u64>, MacosNativeProbeError> {
    cf_array_iter(payload)
        .map(|window_id| {
            cf_number_to_u64(window_id).ok_or(MacosNativeProbeError::MissingTopology(
                "SLSCopyWindowsWithOptionsAndTags",
            ))
        })
        .collect()
}
