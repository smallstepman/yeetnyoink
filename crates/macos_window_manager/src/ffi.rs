#![allow(dead_code)]

use std::ffi::c_void;

use crate::transport::{MwmDesktopSnapshotAbi, MwmFastFocusContextAbi, MwmStatus};

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
#[derive(Debug, Clone)]
pub(crate) struct TestFastFocusContextResponse {
    pub code: i32,
    pub context: MwmFastFocusContextAbi,
    pub status: MwmStatus,
}

#[cfg(test)]
unsafe impl Send for TestFastFocusContextResponse {}

#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct TestOperationResponse {
    pub code: i32,
    pub status: MwmStatus,
}

#[cfg(test)]
unsafe impl Send for TestOperationResponse {}

#[cfg(test)]
#[derive(Default)]
struct TestState {
    desktop_snapshot_response: Option<TestDesktopSnapshotResponse>,
    topology_snapshot_response: Option<TestDesktopSnapshotResponse>,
    fast_focus_context_response: Option<TestFastFocusContextResponse>,
    switch_space_in_snapshot_response: Option<TestOperationResponse>,
    desktop_snapshot_release_calls: usize,
}

#[cfg(test)]
fn test_state() -> &'static Mutex<TestState> {
    static TEST_STATE: OnceLock<Mutex<TestState>> = OnceLock::new();
    TEST_STATE.get_or_init(|| Mutex::new(TestState::default()))
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::{
        TestDesktopSnapshotResponse, TestFastFocusContextResponse, TestOperationResponse,
        test_state,
    };

    pub(crate) fn reset() {
        *test_state().lock().unwrap() = Default::default();
    }

    pub(crate) fn set_desktop_snapshot_response(response: TestDesktopSnapshotResponse) {
        test_state().lock().unwrap().desktop_snapshot_response = Some(response);
    }

    pub(crate) fn set_topology_snapshot_response(response: TestDesktopSnapshotResponse) {
        test_state().lock().unwrap().topology_snapshot_response = Some(response);
    }

    pub(crate) fn set_fast_focus_context_response(response: TestFastFocusContextResponse) {
        test_state().lock().unwrap().fast_focus_context_response = Some(response);
    }

    pub(crate) fn set_switch_space_in_snapshot_response(response: TestOperationResponse) {
        test_state()
            .lock()
            .unwrap()
            .switch_space_in_snapshot_response = Some(response);
    }

    pub(crate) fn desktop_snapshot_release_calls() -> usize {
        test_state().lock().unwrap().desktop_snapshot_release_calls
    }
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn mwm_backend_new(out_backend: *mut *mut c_void, out_status: *mut c_void) -> i32;
    fn mwm_backend_validate_environment(backend: *mut c_void, out_status: *mut c_void) -> i32;
    fn mwm_backend_free(backend: *mut c_void);
    fn mwm_backend_desktop_snapshot(
        backend: *mut c_void,
        out_snapshot: *mut c_void,
        out_status: *mut c_void,
    ) -> i32;
    fn mwm_backend_topology_snapshot(
        backend: *mut c_void,
        out_snapshot: *mut c_void,
        out_status: *mut c_void,
    ) -> i32;
    fn mwm_backend_prepare_fast_focus_context(
        backend: *mut c_void,
        out_context: *mut c_void,
        out_status: *mut c_void,
    ) -> i32;
    fn mwm_backend_switch_space(
        backend: *mut c_void,
        space_id: u64,
        out_status: *mut c_void,
    ) -> i32;
    fn mwm_backend_switch_adjacent_space(
        backend: *mut c_void,
        direction: i32,
        space_id: u64,
        out_status: *mut c_void,
    ) -> i32;
    fn mwm_backend_switch_space_in_snapshot(
        backend: *mut c_void,
        snapshot: *const c_void,
        space_id: u64,
        adjacent_direction: i32,
        out_status: *mut c_void,
    ) -> i32;
    fn mwm_backend_focus_window(
        backend: *mut c_void,
        window_id: u64,
        out_status: *mut c_void,
    ) -> i32;
    fn mwm_backend_focus_window_with_known_pid(
        backend: *mut c_void,
        window_id: u64,
        pid: u32,
        out_status: *mut c_void,
    ) -> i32;
    fn mwm_backend_focus_window_in_active_space_with_known_pid(
        backend: *mut c_void,
        window_id: u64,
        pid: u32,
        has_target_hint: u8,
        target_hint_space_id: u64,
        target_hint_x: i32,
        target_hint_y: i32,
        target_hint_width: i32,
        target_hint_height: i32,
        out_status: *mut c_void,
    ) -> i32;
    fn mwm_backend_focus_same_space_target_in_snapshot(
        backend: *mut c_void,
        snapshot: *const c_void,
        direction: i32,
        target_window_id: u64,
        out_status: *mut c_void,
    ) -> i32;
    fn mwm_backend_move_window_to_space(
        backend: *mut c_void,
        window_id: u64,
        space_id: u64,
        out_status: *mut c_void,
    ) -> i32;
    fn mwm_backend_swap_window_frames(
        backend: *mut c_void,
        source_window_id: u64,
        source_x: i32,
        source_y: i32,
        source_width: i32,
        source_height: i32,
        target_window_id: u64,
        target_x: i32,
        target_y: i32,
        target_width: i32,
        target_height: i32,
        out_status: *mut c_void,
    ) -> i32;
    fn mwm_status_release(status: *mut c_void);
    fn mwm_desktop_snapshot_release(snapshot: *mut c_void);
}

#[cfg(target_os = "macos")]
pub(crate) unsafe fn backend_new(out_backend: *mut *mut c_void, out_status: *mut MwmStatus) -> i32 {
    unsafe { mwm_backend_new(out_backend, out_status.cast()) }
}

#[cfg(target_os = "macos")]
pub(crate) unsafe fn backend_validate_environment(
    backend: *mut c_void,
    out_status: *mut MwmStatus,
) -> i32 {
    unsafe { mwm_backend_validate_environment(backend, out_status.cast()) }
}

#[cfg(target_os = "macos")]
pub(crate) unsafe fn backend_free(backend: *mut c_void) {
    unsafe { mwm_backend_free(backend) }
}

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

#[cfg(target_os = "macos")]
pub(crate) unsafe fn backend_topology_snapshot(
    backend: *mut c_void,
    out_snapshot: *mut MwmDesktopSnapshotAbi,
    out_status: *mut MwmStatus,
) -> i32 {
    #[cfg(test)]
    if let Some(response) = test_state()
        .lock()
        .unwrap()
        .topology_snapshot_response
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

    unsafe { mwm_backend_topology_snapshot(backend, out_snapshot.cast(), out_status.cast()) }
}

#[cfg(target_os = "macos")]
pub(crate) unsafe fn backend_prepare_fast_focus_context(
    backend: *mut c_void,
    out_context: *mut MwmFastFocusContextAbi,
    out_status: *mut MwmStatus,
) -> i32 {
    #[cfg(test)]
    if let Some(response) = test_state()
        .lock()
        .unwrap()
        .fast_focus_context_response
        .take()
    {
        if !out_context.is_null() {
            unsafe {
                *out_context = response.context;
            }
        }

        if !out_status.is_null() {
            unsafe {
                *out_status = response.status;
            }
        }

        return response.code;
    }

    unsafe {
        mwm_backend_prepare_fast_focus_context(backend, out_context.cast(), out_status.cast())
    }
}

#[cfg(target_os = "macos")]
pub(crate) unsafe fn backend_switch_space(
    backend: *mut c_void,
    space_id: u64,
    out_status: *mut MwmStatus,
) -> i32 {
    unsafe { mwm_backend_switch_space(backend, space_id, out_status.cast()) }
}

#[cfg(target_os = "macos")]
pub(crate) unsafe fn backend_switch_adjacent_space(
    backend: *mut c_void,
    direction: i32,
    space_id: u64,
    out_status: *mut MwmStatus,
) -> i32 {
    unsafe { mwm_backend_switch_adjacent_space(backend, direction, space_id, out_status.cast()) }
}

#[cfg(target_os = "macos")]
pub(crate) unsafe fn backend_switch_space_in_snapshot(
    backend: *mut c_void,
    snapshot: *const MwmDesktopSnapshotAbi,
    space_id: u64,
    adjacent_direction: i32,
    out_status: *mut MwmStatus,
) -> i32 {
    #[cfg(test)]
    if let Some(response) = test_state()
        .lock()
        .unwrap()
        .switch_space_in_snapshot_response
        .take()
    {
        if !out_status.is_null() {
            unsafe { *out_status = response.status };
        }
        return response.code;
    }

    unsafe {
        mwm_backend_switch_space_in_snapshot(
            backend,
            snapshot.cast(),
            space_id,
            adjacent_direction,
            out_status.cast(),
        )
    }
}

#[cfg(target_os = "macos")]
pub(crate) unsafe fn backend_focus_window(
    backend: *mut c_void,
    window_id: u64,
    out_status: *mut MwmStatus,
) -> i32 {
    unsafe { mwm_backend_focus_window(backend, window_id, out_status.cast()) }
}

#[cfg(target_os = "macos")]
pub(crate) unsafe fn backend_focus_window_with_known_pid(
    backend: *mut c_void,
    window_id: u64,
    pid: u32,
    out_status: *mut MwmStatus,
) -> i32 {
    unsafe { mwm_backend_focus_window_with_known_pid(backend, window_id, pid, out_status.cast()) }
}

#[cfg(target_os = "macos")]
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn backend_focus_window_in_active_space_with_known_pid(
    backend: *mut c_void,
    window_id: u64,
    pid: u32,
    has_target_hint: u8,
    target_hint_space_id: u64,
    target_hint_x: i32,
    target_hint_y: i32,
    target_hint_width: i32,
    target_hint_height: i32,
    out_status: *mut MwmStatus,
) -> i32 {
    unsafe {
        mwm_backend_focus_window_in_active_space_with_known_pid(
            backend,
            window_id,
            pid,
            has_target_hint,
            target_hint_space_id,
            target_hint_x,
            target_hint_y,
            target_hint_width,
            target_hint_height,
            out_status.cast(),
        )
    }
}

#[cfg(target_os = "macos")]
pub(crate) unsafe fn backend_focus_same_space_target_in_snapshot(
    backend: *mut c_void,
    snapshot: *const MwmDesktopSnapshotAbi,
    direction: i32,
    target_window_id: u64,
    out_status: *mut MwmStatus,
) -> i32 {
    unsafe {
        mwm_backend_focus_same_space_target_in_snapshot(
            backend,
            snapshot.cast(),
            direction,
            target_window_id,
            out_status.cast(),
        )
    }
}

#[cfg(target_os = "macos")]
pub(crate) unsafe fn backend_move_window_to_space(
    backend: *mut c_void,
    window_id: u64,
    space_id: u64,
    out_status: *mut MwmStatus,
) -> i32 {
    unsafe { mwm_backend_move_window_to_space(backend, window_id, space_id, out_status.cast()) }
}

#[cfg(target_os = "macos")]
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn backend_swap_window_frames(
    backend: *mut c_void,
    source_window_id: u64,
    source_x: i32,
    source_y: i32,
    source_width: i32,
    source_height: i32,
    target_window_id: u64,
    target_x: i32,
    target_y: i32,
    target_width: i32,
    target_height: i32,
    out_status: *mut MwmStatus,
) -> i32 {
    unsafe {
        mwm_backend_swap_window_frames(
            backend,
            source_window_id,
            source_x,
            source_y,
            source_width,
            source_height,
            target_window_id,
            target_x,
            target_y,
            target_width,
            target_height,
            out_status.cast(),
        )
    }
}

#[cfg(target_os = "macos")]
pub(crate) unsafe fn status_release(status: *mut MwmStatus) {
    unsafe { mwm_status_release(status.cast()) }
}

#[cfg(target_os = "macos")]
pub(crate) unsafe fn desktop_snapshot_release(snapshot: *mut MwmDesktopSnapshotAbi) {
    #[cfg(test)]
    {
        test_state().lock().unwrap().desktop_snapshot_release_calls += 1;
    }

    unsafe { mwm_desktop_snapshot_release(snapshot.cast()) }
}
