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

#[cfg(test)]
mod tests {
    use std::{ffi::c_void, mem::ManuallyDrop, ptr::NonNull};

    use super::SwiftBackendShim;
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
}
