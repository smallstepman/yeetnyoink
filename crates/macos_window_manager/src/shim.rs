#![allow(dead_code)]

use std::{ffi::c_void, ptr::NonNull};

use crate::{
    NativeDesktopSnapshot,
    error::{MacosNativeBridgeError, MacosNativeConnectError, MacosNativeProbeError},
    ffi,
    transport::{
        MWM_STATUS_CONNECT_MISSING_ACCESSIBILITY_PERMISSION,
        MWM_STATUS_CONNECT_MISSING_REQUIRED_SYMBOL,
        MWM_STATUS_CONNECT_MISSING_TOPOLOGY_PRECONDITION, MWM_STATUS_OK,
        MWM_STATUS_PROBE_MISSING_TOPOLOGY, MwmDesktopSnapshotAbi, MwmStatus,
    },
};

pub(crate) struct SwiftBackendShim {
    raw: NonNull<c_void>,
}

pub(crate) struct OwnedDesktopSnapshot {
    raw: MwmDesktopSnapshotAbi,
}

impl SwiftBackendShim {
    pub(crate) fn new() -> Result<Self, MacosNativeBridgeError> {
        let mut raw = std::ptr::null_mut();
        let mut status = MwmStatus::ok();
        let code = unsafe { ffi::backend_new(&mut raw, &mut status) };
        status.code = code;

        if code != MWM_STATUS_OK {
            return Err(status_error(&mut status));
        }

        unsafe { ffi::status_release(&mut status) };
        let raw = NonNull::new(raw).ok_or(MacosNativeBridgeError::NullBackendHandle)?;
        Ok(Self { raw })
    }

    pub(crate) fn desktop_snapshot(&self) -> Result<OwnedDesktopSnapshot, MacosNativeBridgeError> {
        let mut snapshot = MwmDesktopSnapshotAbi::empty();
        let mut status = MwmStatus::ok();
        let code =
            unsafe { ffi::backend_desktop_snapshot(self.raw.as_ptr(), &mut snapshot, &mut status) };
        status.code = code;

        if code != MWM_STATUS_OK {
            return Err(status_error(&mut status));
        }

        let snapshot = OwnedDesktopSnapshot::new(snapshot);
        unsafe { ffi::status_release(&mut status) };
        snapshot.validate()
    }

    pub(crate) fn validate_environment(&self) -> Result<(), MacosNativeConnectError> {
        let mut status = MwmStatus::ok();
        let code = unsafe { ffi::backend_validate_environment(self.raw.as_ptr(), &mut status) };
        status.code = code;

        if code == MWM_STATUS_OK {
            unsafe { ffi::status_release(&mut status) };
            return Ok(());
        }

        Err(connect_error(&mut status))
    }

    pub(crate) fn desktop_snapshot_native(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
        self.desktop_snapshot()
            .map_err(bridge_probe_error)?
            .into_native_snapshot()
    }

    pub(crate) fn as_ptr(&self) -> *mut c_void {
        self.raw.as_ptr()
    }
}

impl Drop for SwiftBackendShim {
    fn drop(&mut self) {
        unsafe { ffi::backend_free(self.raw.as_ptr()) };
    }
}

impl OwnedDesktopSnapshot {
    fn new(raw: MwmDesktopSnapshotAbi) -> Self {
        Self { raw }
    }

    fn validate(self) -> Result<Self, MacosNativeBridgeError> {
        self.raw
            .validate()
            .map_err(MacosNativeBridgeError::InvalidDesktopSnapshotTransport)?;
        Ok(self)
    }

    fn into_native_snapshot(self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
        let snapshot = unsafe { self.raw.to_native_snapshot() };
        Ok(snapshot)
    }
}

impl Drop for OwnedDesktopSnapshot {
    fn drop(&mut self) {
        unsafe { ffi::desktop_snapshot_release(&mut self.raw) };
    }
}

fn status_error(status: &mut MwmStatus) -> MacosNativeBridgeError {
    let code = status.code;
    let message = unsafe { status.message() };
    unsafe { ffi::status_release(status) };

    MacosNativeBridgeError::BackendStatus { code, message }
}

fn connect_error(status: &mut MwmStatus) -> MacosNativeConnectError {
    let code = status.code;
    let message = unsafe { status.message() };
    unsafe { ffi::status_release(status) };

    match code {
        MWM_STATUS_CONNECT_MISSING_REQUIRED_SYMBOL => {
            MacosNativeConnectError::MissingRequiredSymbol(static_message(message))
        }
        MWM_STATUS_CONNECT_MISSING_ACCESSIBILITY_PERMISSION => {
            MacosNativeConnectError::MissingAccessibilityPermission
        }
        MWM_STATUS_CONNECT_MISSING_TOPOLOGY_PRECONDITION => {
            MacosNativeConnectError::MissingTopologyPrecondition(static_message(message))
        }
        _ => MacosNativeConnectError::MissingTopologyPrecondition("swift macOS backend"),
    }
}

fn bridge_probe_error(error: MacosNativeBridgeError) -> MacosNativeProbeError {
    match error {
        MacosNativeBridgeError::BackendStatus { code, message } => match code {
            MWM_STATUS_PROBE_MISSING_TOPOLOGY => MacosNativeProbeError::MissingTopology(
                static_message(message),
            ),
            _ => MacosNativeProbeError::MissingTopology("swift macOS backend"),
        },
        MacosNativeBridgeError::InvalidDesktopSnapshotTransport(_) => {
            MacosNativeProbeError::MissingTopology("swift macOS backend transport")
        }
        MacosNativeBridgeError::NullBackendHandle => {
            MacosNativeProbeError::MissingTopology("swift macOS backend")
        }
    }
}

fn static_message(message: Option<String>) -> &'static str {
    match message.as_deref() {
        Some("SLSMainConnectionID") => "SLSMainConnectionID",
        Some("SLSCopyManagedDisplaySpaces") => "SLSCopyManagedDisplaySpaces",
        Some("SLSManagedDisplayGetCurrentSpace") => "SLSManagedDisplayGetCurrentSpace",
        Some("SLSManagedDisplaySetCurrentSpace") => "SLSManagedDisplaySetCurrentSpace",
        Some("SLSCopyManagedDisplayForSpace") => "SLSCopyManagedDisplayForSpace",
        Some("SLSCopyWindowsWithOptionsAndTags") => "SLSCopyWindowsWithOptionsAndTags",
        Some("SLSMoveWindowsToManagedSpace") => "SLSMoveWindowsToManagedSpace",
        Some("AXIsProcessTrusted") => "AXIsProcessTrusted",
        Some("_AXUIElementGetWindow") => "_AXUIElementGetWindow",
        Some("_SLPSSetFrontProcessWithOptions") => "_SLPSSetFrontProcessWithOptions",
        Some("GetProcessForPID") => "GetProcessForPID",
        Some("CGWindowListCopyWindowInfo") => "CGWindowListCopyWindowInfo",
        Some("CGWindowListCreateDescriptionFromArray") => "CGWindowListCreateDescriptionFromArray",
        Some("AXUIElementCopyAttributeValue") => "AXUIElementCopyAttributeValue",
        Some("main SkyLight connection") => "main SkyLight connection",
        _ => "swift macOS backend",
    }
}

#[cfg(test)]
pub(crate) fn test_snapshot_from_ffi() -> crate::NativeDesktopSnapshot {
    let mut abi = sample_snapshot_abi();
    let snapshot = unsafe { abi.to_native_snapshot() };
    release_test_snapshot(&mut abi);
    snapshot
}

#[cfg(test)]
fn sample_snapshot_abi() -> MwmDesktopSnapshotAbi {
    use std::ffi::CString;

    use crate::transport::{MwmRectAbi, MwmSpaceAbi, MwmWindowAbi};

    let spaces = vec![
        MwmSpaceAbi {
            id: 101,
            display_index: 0,
            active: 1,
            kind: 0,
        },
        MwmSpaceAbi {
            id: 102,
            display_index: 0,
            active: 0,
            kind: 2,
        },
    ];
    let windows = vec![
        MwmWindowAbi {
            id: 9001,
            pid: 41,
            has_pid: 1,
            app_id_ptr: CString::new("com.apple.Terminal").unwrap().into_raw(),
            title_ptr: CString::new("Terminal").unwrap().into_raw(),
            frame: MwmRectAbi {
                x: 0,
                y: 0,
                width: 900,
                height: 700,
            },
            has_frame: 1,
            level: 0,
            space_id: 101,
            order_index: 0,
            has_order_index: 1,
        },
        MwmWindowAbi {
            id: 9002,
            pid: 42,
            has_pid: 1,
            app_id_ptr: CString::new("com.apple.Safari").unwrap().into_raw(),
            title_ptr: CString::new("Safari").unwrap().into_raw(),
            frame: MwmRectAbi {
                x: 20,
                y: 10,
                width: 600,
                height: 500,
            },
            has_frame: 1,
            level: 3,
            space_id: 101,
            order_index: 1,
            has_order_index: 1,
        },
        MwmWindowAbi {
            id: 9003,
            pid: 0,
            has_pid: 0,
            app_id_ptr: std::ptr::null_mut(),
            title_ptr: std::ptr::null_mut(),
            frame: MwmRectAbi::default(),
            has_frame: 0,
            level: 0,
            space_id: 102,
            order_index: 0,
            has_order_index: 0,
        },
    ];

    let spaces = Box::into_raw(spaces.into_boxed_slice()) as *mut MwmSpaceAbi;
    let windows = Box::into_raw(windows.into_boxed_slice()) as *mut MwmWindowAbi;
    MwmDesktopSnapshotAbi {
        spaces_ptr: spaces,
        spaces_len: 2,
        windows_ptr: windows,
        windows_len: 3,
        focused_window_id: 9003,
    }
}

#[cfg(test)]
fn release_test_snapshot(snapshot: &mut MwmDesktopSnapshotAbi) {
    use std::ffi::CString;

    unsafe {
        if !snapshot.windows_ptr.is_null() {
            let windows = Vec::from_raw_parts(
                snapshot.windows_ptr,
                snapshot.windows_len,
                snapshot.windows_len,
            );
            for window in windows {
                if !window.app_id_ptr.is_null() {
                    drop(CString::from_raw(window.app_id_ptr));
                }
                if !window.title_ptr.is_null() {
                    drop(CString::from_raw(window.title_ptr));
                }
            }
        }
        if !snapshot.spaces_ptr.is_null() {
            drop(Vec::from_raw_parts(
                snapshot.spaces_ptr,
                snapshot.spaces_len,
                snapshot.spaces_len,
            ));
        }
    }
    *snapshot = MwmDesktopSnapshotAbi::empty();
}

#[cfg(test)]
mod tests {
    use std::{ffi::c_void, mem::ManuallyDrop, ptr::NonNull};

    use super::{SwiftBackendShim, test_snapshot_from_ffi};
    use crate::{
        error::MacosNativeBridgeError,
        ffi::{self, TestDesktopSnapshotResponse},
        transport::{MWM_STATUS_OK, MwmDesktopSnapshotAbi, MwmStatus},
    };

    #[test]
    fn desktop_snapshot_releases_transport_when_validation_fails() {
        ffi::test_support::reset();
        ffi::test_support::set_desktop_snapshot_response(TestDesktopSnapshotResponse {
            code: MWM_STATUS_OK,
            snapshot: MwmDesktopSnapshotAbi {
                windows_len: 1,
                ..MwmDesktopSnapshotAbi::empty()
            },
            status: MwmStatus::ok(),
        });

        let shim = ManuallyDrop::new(SwiftBackendShim {
            raw: NonNull::<c_void>::dangling(),
        });

        let error = match shim.desktop_snapshot() {
            Ok(_) => panic!("desktop_snapshot should fail validation"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            MacosNativeBridgeError::InvalidDesktopSnapshotTransport(
                "windows_ptr was null for a non-empty snapshot"
            )
        );
        assert_eq!(ffi::test_support::desktop_snapshot_release_calls(), 1);
    }

    #[test]
    fn snapshot_wrapper_converts_ffi_snapshot() {
        let snapshot = test_snapshot_from_ffi();

        assert_eq!(snapshot.spaces.len(), 2);
        assert_eq!(snapshot.windows.len(), 3);
        assert_eq!(snapshot.focused_window_id, Some(9003));
    }
}
