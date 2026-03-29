#![allow(dead_code)]

use std::ffi::c_void;

use crate::transport::{MwmDesktopSnapshotAbi, MwmStatus};

#[cfg(test)]
use std::sync::{Mutex, OnceLock};

#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct TestDesktopSnapshotResponse {
    pub code: i32,
    pub snapshot: MwmDesktopSnapshotAbi,
    pub status: MwmStatus,
}

#[cfg(test)]
unsafe impl Send for TestDesktopSnapshotResponse {}

#[cfg(test)]
#[derive(Default)]
struct TestState {
    desktop_snapshot_response: Option<TestDesktopSnapshotResponse>,
    desktop_snapshot_release_calls: usize,
}

#[cfg(test)]
fn test_state() -> &'static Mutex<TestState> {
    static TEST_STATE: OnceLock<Mutex<TestState>> = OnceLock::new();
    TEST_STATE.get_or_init(|| Mutex::new(TestState::default()))
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::{TestDesktopSnapshotResponse, test_state};

    pub(crate) fn reset() {
        *test_state().lock().unwrap() = Default::default();
    }

    pub(crate) fn set_desktop_snapshot_response(response: TestDesktopSnapshotResponse) {
        test_state().lock().unwrap().desktop_snapshot_response = Some(response);
    }

    pub(crate) fn desktop_snapshot_release_calls() -> usize {
        test_state().lock().unwrap().desktop_snapshot_release_calls
    }
}

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
    #[cfg(test)]
    if let Some(response) = test_state()
        .lock()
        .unwrap()
        .desktop_snapshot_response
        .take()
    {
        if !out_snapshot.is_null() {
            unsafe {
                *out_snapshot = response.snapshot;
            }
        }

        if !out_status.is_null() {
            unsafe {
                *out_status = response.status;
            }
        }

        return response.code;
    }

    unsafe { mwm_backend_desktop_snapshot(backend, out_snapshot.cast(), out_status.cast()) }
}

#[cfg(not(target_os = "macos"))]
pub(crate) unsafe fn backend_desktop_snapshot(
    _backend: *mut c_void,
    out_snapshot: *mut MwmDesktopSnapshotAbi,
    out_status: *mut MwmStatus,
) -> i32 {
    #[cfg(test)]
    if let Some(response) = test_state()
        .lock()
        .unwrap()
        .desktop_snapshot_response
        .take()
    {
        if !out_snapshot.is_null() {
            unsafe {
                *out_snapshot = response.snapshot;
            }
        }

        if !out_status.is_null() {
            unsafe {
                *out_status = response.status;
            }
        }

        return response.code;
    }

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
    #[cfg(test)]
    {
        test_state().lock().unwrap().desktop_snapshot_release_calls += 1;
    }

    unsafe { mwm_desktop_snapshot_release(snapshot.cast()) }
}

#[cfg(not(target_os = "macos"))]
pub(crate) unsafe fn desktop_snapshot_release(snapshot: *mut MwmDesktopSnapshotAbi) {
    #[cfg(test)]
    {
        test_state().lock().unwrap().desktop_snapshot_release_calls += 1;
    }

    if !snapshot.is_null() {
        unsafe {
            *snapshot = MwmDesktopSnapshotAbi::default();
        }
    }
}
