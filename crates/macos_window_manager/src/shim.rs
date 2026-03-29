#![allow(dead_code)]

use std::{ffi::c_void, ops::Deref, ptr::NonNull};

use crate::{
    error::MacosNativeBridgeError,
    ffi,
    transport::{MWM_STATUS_OK, MwmDesktopSnapshotAbi, MwmStatus},
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

        snapshot
            .validate()
            .map_err(MacosNativeBridgeError::InvalidDesktopSnapshotTransport)?;

        unsafe { ffi::status_release(&mut status) };
        Ok(OwnedDesktopSnapshot { raw: snapshot })
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
    pub(crate) fn raw(&self) -> &MwmDesktopSnapshotAbi {
        &self.raw
    }
}

impl Deref for OwnedDesktopSnapshot {
    type Target = MwmDesktopSnapshotAbi;

    fn deref(&self) -> &Self::Target {
        self.raw()
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
