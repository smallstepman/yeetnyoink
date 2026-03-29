#![allow(dead_code)]

use std::ffi::c_void;

use crate::transport::{MwmDesktopSnapshotAbi, MwmStatus};

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn mwm_backend_new(out_backend: *mut *mut c_void, out_status: *mut c_void) -> i32;
    fn mwm_backend_free(backend: *mut c_void);
    fn mwm_backend_desktop_snapshot(
        backend: *mut c_void,
        out_snapshot: *mut c_void,
        out_status: *mut c_void,
    ) -> i32;
    fn mwm_status_release(status: *mut c_void);
    fn mwm_desktop_snapshot_release(snapshot: *mut c_void);
}

#[cfg(target_os = "macos")]
pub(crate) unsafe fn backend_new(out_backend: *mut *mut c_void, out_status: *mut MwmStatus) -> i32 {
    unsafe { mwm_backend_new(out_backend, out_status.cast()) }
}

#[cfg(not(target_os = "macos"))]
pub(crate) unsafe fn backend_new(out_backend: *mut *mut c_void, out_status: *mut MwmStatus) -> i32 {
    if !out_backend.is_null() {
        unsafe {
            *out_backend = std::ptr::null_mut();
        }
    }

    if !out_status.is_null() {
        unsafe {
            *out_status = MwmStatus::unavailable();
        }
    }

    crate::transport::MWM_STATUS_UNAVAILABLE
}

#[cfg(target_os = "macos")]
pub(crate) unsafe fn backend_free(backend: *mut c_void) {
    unsafe { mwm_backend_free(backend) }
}

#[cfg(not(target_os = "macos"))]
pub(crate) unsafe fn backend_free(_backend: *mut c_void) {}

#[cfg(target_os = "macos")]
pub(crate) unsafe fn backend_desktop_snapshot(
    backend: *mut c_void,
    out_snapshot: *mut MwmDesktopSnapshotAbi,
    out_status: *mut MwmStatus,
) -> i32 {
    unsafe { mwm_backend_desktop_snapshot(backend, out_snapshot.cast(), out_status.cast()) }
}

#[cfg(not(target_os = "macos"))]
pub(crate) unsafe fn backend_desktop_snapshot(
    _backend: *mut c_void,
    out_snapshot: *mut MwmDesktopSnapshotAbi,
    out_status: *mut MwmStatus,
) -> i32 {
    if !out_snapshot.is_null() {
        unsafe {
            *out_snapshot = MwmDesktopSnapshotAbi::empty();
        }
    }

    if !out_status.is_null() {
        unsafe {
            *out_status = MwmStatus::unavailable();
        }
    }

    crate::transport::MWM_STATUS_UNAVAILABLE
}

#[cfg(target_os = "macos")]
pub(crate) unsafe fn status_release(status: *mut MwmStatus) {
    unsafe { mwm_status_release(status.cast()) }
}

#[cfg(not(target_os = "macos"))]
pub(crate) unsafe fn status_release(status: *mut MwmStatus) {
    if !status.is_null() {
        unsafe {
            *status = MwmStatus::default();
        }
    }
}

#[cfg(target_os = "macos")]
pub(crate) unsafe fn desktop_snapshot_release(snapshot: *mut MwmDesktopSnapshotAbi) {
    unsafe { mwm_desktop_snapshot_release(snapshot.cast()) }
}

#[cfg(not(target_os = "macos"))]
pub(crate) unsafe fn desktop_snapshot_release(snapshot: *mut MwmDesktopSnapshotAbi) {
    if !snapshot.is_null() {
        unsafe {
            *snapshot = MwmDesktopSnapshotAbi::default();
        }
    }
}
