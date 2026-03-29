use crate::foundation::{
    CFArrayRef, CFRetain, CFTypeRef, CfOwned, OSStatus, cf_array_iter, cf_string,
};
use crate::window_server;
use crate::{
    MacosNativeApi, MacosNativeOperationError, MacosNativeProbeError, NativeBounds, RealNativeApi,
};
use std::{
    ffi::{c_int, c_void},
    ptr,
};

pub(crate) type AXUIElementRef = *const c_void;
pub(crate) type AXValueType = u32;

pub(crate) const K_AX_VALUE_TYPE_CGPOINT: AXValueType = 1;
pub(crate) const K_AX_VALUE_TYPE_CGSIZE: AXValueType = 2;

type AxIsProcessTrustedFn = unsafe extern "C" fn() -> u8;
type AxUiElementGetWindowFn = unsafe extern "C" fn(AXUIElementRef, *mut u32) -> OSStatus;

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct CGPoint {
    pub(crate) x: f64,
    pub(crate) y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct CGSize {
    pub(crate) width: f64,
    pub(crate) height: f64,
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    pub(crate) fn AXUIElementCreateApplication(pid: c_int) -> AXUIElementRef;
    pub(crate) fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    pub(crate) fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: *const c_void,
        value: *mut CFTypeRef,
    ) -> OSStatus;
    pub(crate) fn AXUIElementPerformAction(
        element: AXUIElementRef,
        action: *const c_void,
    ) -> OSStatus;
    pub(crate) fn AXUIElementSetAttributeValue(
        element: AXUIElementRef,
        attribute: *const c_void,
        value: CFTypeRef,
    ) -> OSStatus;
    #[allow(dead_code)]
    pub(crate) fn AXUIElementGetPid(element: AXUIElementRef, pid: *mut c_int) -> OSStatus;
    pub(crate) fn AXValueCreate(value_type: AXValueType, value_ptr: *const c_void) -> CFTypeRef;
}

pub(crate) fn is_process_trusted(api: &RealNativeApi) -> bool {
    let Some(symbol) = api.resolve_symbol("AXIsProcessTrusted") else {
        return false;
    };

    let ax_is_process_trusted: AxIsProcessTrustedFn = unsafe { std::mem::transmute(symbol) };
    unsafe { ax_is_process_trusted() != 0 }
}

pub(crate) fn focused_window_id<App, Window, FocusedApplication, FocusedWindow, WindowId>(
    mut focused_application: FocusedApplication,
    mut focused_window: FocusedWindow,
    mut window_id: WindowId,
) -> Result<Option<u64>, MacosNativeProbeError>
where
    FocusedApplication: FnMut() -> Result<Option<App>, MacosNativeProbeError>,
    FocusedWindow: FnMut(&App) -> Result<Option<Window>, MacosNativeProbeError>,
    WindowId: FnMut(&Window) -> Result<u64, MacosNativeProbeError>,
{
    let Some(application) = focused_application()? else {
        return Ok(None);
    };
    let Some(window) = focused_window(&application)? else {
        return Ok(None);
    };
    window_id(&window).map(Some)
}

pub(crate) fn copy_system_wide_ax_element(
    _api: &RealNativeApi,
) -> Result<CfOwned, MacosNativeProbeError> {
    let _span = tracing::debug_span!("macos_native.ax.system_wide_element").entered();
    unsafe { CfOwned::from_create_rule(AXUIElementCreateSystemWide() as CFTypeRef) }.ok_or(
        MacosNativeProbeError::MissingTopology("AXUIElementCreateSystemWide"),
    )
}

pub(crate) fn copy_ax_attribute_value(
    _api: &RealNativeApi,
    element: AXUIElementRef,
    attribute_name: &str,
) -> Result<Option<CfOwned>, MacosNativeProbeError> {
    let _span = tracing::debug_span!(
        "macos_native.ax.copy_attribute_value",
        attribute = attribute_name
    )
    .entered();
    let attribute = cf_string(attribute_name)?;
    let mut value = ptr::null();
    let status =
        unsafe { AXUIElementCopyAttributeValue(element, attribute.as_type_ref(), &mut value) };

    if status != 0 {
        return Ok(None);
    }

    Ok(unsafe { CfOwned::from_create_rule(value) })
}

pub(crate) fn copy_focused_application_ax(
    api: &RealNativeApi,
) -> Result<Option<CfOwned>, MacosNativeProbeError> {
    let system = copy_system_wide_ax_element(api)?;
    copy_ax_attribute_value(
        api,
        system.as_type_ref() as AXUIElementRef,
        "AXFocusedApplication",
    )
}

pub(crate) fn copy_focused_window_ax(
    api: &RealNativeApi,
    application: &CfOwned,
) -> Result<Option<CfOwned>, MacosNativeProbeError> {
    copy_ax_attribute_value(
        api,
        application.as_type_ref() as AXUIElementRef,
        "AXFocusedWindow",
    )
}

#[allow(dead_code)]
pub(crate) fn ax_pid(
    _api: &RealNativeApi,
    element: &CfOwned,
) -> Result<u32, MacosNativeProbeError> {
    let mut pid = 0;
    let status = unsafe { AXUIElementGetPid(element.as_type_ref() as AXUIElementRef, &mut pid) };
    if status != 0 || pid <= 0 {
        return Err(MacosNativeProbeError::MissingFocusedWindow);
    }
    Ok(pid as u32)
}

pub(crate) fn ax_window_id(
    api: &RealNativeApi,
    element: &CfOwned,
) -> Result<u64, MacosNativeProbeError> {
    let Some(symbol) = api.resolve_symbol("_AXUIElementGetWindow") else {
        return Err(MacosNativeProbeError::MissingTopology(
            "_AXUIElementGetWindow",
        ));
    };
    let ax_ui_element_get_window: AxUiElementGetWindowFn = unsafe { std::mem::transmute(symbol) };
    let mut window_id = 0u32;
    let status = unsafe {
        ax_ui_element_get_window(element.as_type_ref() as AXUIElementRef, &mut window_id)
    };

    if status != 0 || window_id == 0 {
        return Err(MacosNativeProbeError::MissingFocusedWindow);
    }

    Ok(window_id as u64)
}

pub(crate) fn probe_focused_window_id(
    api: &RealNativeApi,
) -> Result<Option<u64>, MacosNativeProbeError> {
    focused_window_id(
        || {
            let _span = tracing::debug_span!("macos_native.ax.focused_application").entered();
            copy_focused_application_ax(api)
        },
        |application| {
            let _span = tracing::debug_span!("macos_native.ax.focused_window").entered();
            copy_focused_window_ax(api, application)
        },
        |window| {
            let _span = tracing::debug_span!("macos_native.ax.window_id").entered();
            ax_window_id(api, window)
        },
    )
}

pub(crate) fn copy_application_ax_element(
    _api: &RealNativeApi,
    pid: u32,
) -> Result<CfOwned, MacosNativeOperationError> {
    unsafe { CfOwned::from_create_rule(AXUIElementCreateApplication(pid as c_int) as CFTypeRef) }
        .ok_or(MacosNativeOperationError::CallFailed(
            "AXUIElementCreateApplication",
        ))
}

pub(crate) fn copy_window_ax_for_id(
    api: &RealNativeApi,
    pid: u32,
    window_id: u64,
) -> Result<CfOwned, MacosNativeOperationError> {
    let application = copy_application_ax_element(api, pid)?;
    let windows = copy_ax_attribute_value(
        api,
        application.as_type_ref() as AXUIElementRef,
        "AXWindows",
    )
    .map_err(MacosNativeOperationError::from)?
    .ok_or(MacosNativeOperationError::MissingWindow(window_id))?;
    let windows = windows.as_type_ref() as CFArrayRef;

    for candidate in cf_array_iter(windows) {
        let Some(candidate) = (unsafe { CfOwned::from_create_rule(CFRetain(candidate)) }) else {
            continue;
        };
        if ax_window_id(api, &candidate).ok() == Some(window_id) {
            return Ok(candidate);
        }
    }

    let ax_window_ids = cf_array_iter(windows)
        .filter_map(|candidate| {
            let candidate = unsafe { CfOwned::from_create_rule(CFRetain(candidate)) }?;
            ax_window_id(api, &candidate).ok()
        })
        .collect::<Vec<_>>();
    api.debug(&format!(
        "macos_native: target window {window_id} missing from AXWindows for pid {pid}; ax_window_ids={ax_window_ids:?} focused_window_id={:?}",
        MacosNativeApi::focused_window_id(api).ok().flatten()
    ));
    Err(MacosNativeOperationError::MissingWindow(window_id))
}

pub(crate) fn ax_window_ids_for_pid(
    api: &RealNativeApi,
    pid: u32,
) -> Result<Vec<u64>, MacosNativeOperationError> {
    let application = copy_application_ax_element(api, pid)?;
    let Some(windows) = copy_ax_attribute_value(
        api,
        application.as_type_ref() as AXUIElementRef,
        "AXWindows",
    )
    .map_err(MacosNativeOperationError::from)?
    else {
        return Ok(Vec::new());
    };
    let windows = windows.as_type_ref() as CFArrayRef;

    Ok(cf_array_iter(windows)
        .filter_map(|candidate| {
            let candidate = unsafe { CfOwned::from_create_rule(CFRetain(candidate)) }?;
            ax_window_id(api, &candidate).ok()
        })
        .collect())
}

pub(crate) fn raise_window_via_ax(
    api: &RealNativeApi,
    window_id: u64,
    pid: u32,
) -> Result<(), MacosNativeOperationError> {
    let window = copy_window_ax_for_id(api, pid, window_id)?;
    let action = cf_string("AXRaise").map_err(MacosNativeOperationError::from)?;
    let status = unsafe {
        AXUIElementPerformAction(window.as_type_ref() as AXUIElementRef, action.as_type_ref())
    };

    (status == 0)
        .then_some(())
        .ok_or(MacosNativeOperationError::CallFailed(
            "AXUIElementPerformAction",
        ))
}

pub(crate) fn set_window_frame_via_ax(
    api: &RealNativeApi,
    window_id: u64,
    pid: u32,
    frame: NativeBounds,
) -> Result<(), MacosNativeOperationError> {
    let window = copy_window_ax_for_id(api, pid, window_id)?;
    let position_attr = cf_string("AXPosition").map_err(MacosNativeOperationError::from)?;
    let size_attr = cf_string("AXSize").map_err(MacosNativeOperationError::from)?;
    let position = CGPoint {
        x: f64::from(frame.x),
        y: f64::from(frame.y),
    };
    let position_value = unsafe {
        CfOwned::from_create_rule(AXValueCreate(
            K_AX_VALUE_TYPE_CGPOINT,
            (&raw const position).cast(),
        ))
    }
    .ok_or(MacosNativeOperationError::CallFailed("AXValueCreate"))?;
    let position_status = unsafe {
        AXUIElementSetAttributeValue(
            window.as_type_ref() as AXUIElementRef,
            position_attr.as_type_ref(),
            position_value.as_type_ref(),
        )
    };

    if position_status != 0 {
        return Err(MacosNativeOperationError::CallFailed(
            "AXUIElementSetAttributeValue",
        ));
    }

    let size = CGSize {
        width: f64::from(frame.width),
        height: f64::from(frame.height),
    };
    let size_value = unsafe {
        CfOwned::from_create_rule(AXValueCreate(
            K_AX_VALUE_TYPE_CGSIZE,
            (&raw const size).cast(),
        ))
    }
    .ok_or(MacosNativeOperationError::CallFailed("AXValueCreate"))?;
    let size_status = unsafe {
        AXUIElementSetAttributeValue(
            window.as_type_ref() as AXUIElementRef,
            size_attr.as_type_ref(),
            size_value.as_type_ref(),
        )
    };

    (size_status == 0)
        .then_some(())
        .ok_or(MacosNativeOperationError::CallFailed(
            "AXUIElementSetAttributeValue",
        ))
}

pub(crate) fn swap_window_frames(
    api: &RealNativeApi,
    source_window_id: u64,
    source_frame: NativeBounds,
    target_window_id: u64,
    target_frame: NativeBounds,
) -> Result<(), MacosNativeOperationError> {
    let source = window_server::window_description(api, source_window_id)?;
    let source_pid = source
        .pid
        .ok_or(MacosNativeOperationError::MissingWindowPid(
            source_window_id,
        ))?;
    let target = window_server::window_description(api, target_window_id)?;
    let target_pid = target
        .pid
        .ok_or(MacosNativeOperationError::MissingWindowPid(
            target_window_id,
        ))?;

    set_window_frame_via_ax(api, source_window_id, source_pid, target_frame)?;
    set_window_frame_via_ax(api, target_window_id, target_pid, source_frame)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focused_window_id_via_ax_queries_focused_app_then_window() {
        let focused_window_id = focused_window_id(
            || Ok(Some("app")),
            |application| {
                assert_eq!(*application, "app");
                Ok(Some("window"))
            },
            |element| {
                assert_eq!(*element, "window");
                Ok(77)
            },
        )
        .unwrap();

        assert_eq!(focused_window_id, Some(77));
    }
}
