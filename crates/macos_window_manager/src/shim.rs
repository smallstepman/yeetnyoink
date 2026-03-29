#![allow(dead_code)]

use std::{ffi::c_void, ptr::NonNull};

use crate::{
    error::MacosNativeBridgeError,
    ffi,
    transport::{MWM_STATUS_OK, MwmDesktopSnapshotAbi, MwmStatus},
};

pub(crate) struct SwiftBackendShim {
    raw: NonNull<c_void>,
}

impl SwiftBackendShim {
    pub(crate) fn new() -> Result<Self, MacosNativeBridgeError> {
        let mut raw = std::ptr::null_mut();
        let mut status = MwmStatus::ok();
        let code = unsafe { ffi::backend_new(&mut raw, &mut status) };
        status.code = code;

        if code != MWM_STATUS_OK {
            return Err(status_error(&status));
        }

        let raw = NonNull::new(raw).ok_or(MacosNativeBridgeError::NullBackendHandle)?;
        Ok(Self { raw })
    }

    pub(crate) fn desktop_snapshot(&self) -> Result<MwmDesktopSnapshotAbi, MacosNativeBridgeError> {
        let mut snapshot = MwmDesktopSnapshotAbi::empty();
        let mut status = MwmStatus::ok();
        let code =
            unsafe { ffi::backend_desktop_snapshot(self.raw.as_ptr(), &mut snapshot, &mut status) };
        status.code = code;

        if code != MWM_STATUS_OK {
            return Err(status_error(&status));
        }

        snapshot
            .validate()
            .map_err(MacosNativeBridgeError::InvalidDesktopSnapshotTransport)?;

        Ok(snapshot)
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

fn status_error(status: &MwmStatus) -> MacosNativeBridgeError {
    MacosNativeBridgeError::BackendStatus {
        code: status.code,
        message: unsafe { status.message() },
    }
}
