#![allow(dead_code)]

use std::{
    ffi::{CString, c_void},
    ptr::NonNull,
};

use crate::{
    NativeBounds, NativeDesktopSnapshot, NativeDirection, NativeFastFocusContext, SpaceKind,
    error::{
        MacosNativeBridgeError, MacosNativeConnectError, MacosNativeOperationError,
        MacosNativeProbeError,
    },
    ffi,
    transport::{
        MWM_STATUS_CONNECT_MISSING_ACCESSIBILITY_PERMISSION,
        MWM_STATUS_CONNECT_MISSING_REQUIRED_SYMBOL,
        MWM_STATUS_CONNECT_MISSING_TOPOLOGY_PRECONDITION, MWM_STATUS_OK,
        MWM_STATUS_PROBE_MISSING_TOPOLOGY, MwmDesktopSnapshotAbi, MwmFastFocusContextAbi,
        MwmRectAbi, MwmSpaceAbi, MwmStatus, MwmWindowAbi,
    },
};

const MWM_STATUS_OPERATION_MISSING_SPACE: i32 = 30;
const MWM_STATUS_OPERATION_MISSING_WINDOW: i32 = 31;
const MWM_STATUS_OPERATION_MISSING_WINDOW_FRAME: i32 = 32;
const MWM_STATUS_OPERATION_MISSING_WINDOW_PID: i32 = 33;
const MWM_STATUS_OPERATION_UNSUPPORTED_STAGE_MANAGER_SPACE: i32 = 34;
const MWM_STATUS_OPERATION_NO_DIRECTIONAL_FOCUS_TARGET: i32 = 35;
const MWM_STATUS_OPERATION_NO_DIRECTIONAL_MOVE_TARGET: i32 = 36;
const MWM_STATUS_OPERATION_CALL_FAILED: i32 = 37;

pub(crate) struct SwiftBackendShim {
    raw: NonNull<c_void>,
}

// SAFETY: `SwiftBackendShim` owns a retained opaque Swift backend handle and never dereferences the
// raw pointer directly from Rust. Moving the wrapper between threads only transfers handle
// ownership; all backend interactions remain behind FFI calls on `&self`, and Rust never aliases
// mutable access to the underlying Swift object.
unsafe impl Send for SwiftBackendShim {}

pub(crate) struct OwnedDesktopSnapshot {
    raw: MwmDesktopSnapshotAbi,
}

struct OwnedFastFocusContext {
    raw: MwmFastFocusContextAbi,
}

struct OwnedDesktopSnapshotInput {
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
        self.snapshot_via(ffi::backend_desktop_snapshot)
    }

    pub(crate) fn topology_snapshot(&self) -> Result<OwnedDesktopSnapshot, MacosNativeBridgeError> {
        self.snapshot_via(ffi::backend_topology_snapshot)
    }

    pub(crate) fn prepare_fast_focus_context(
        &self,
    ) -> Result<NativeFastFocusContext, MacosNativeBridgeError> {
        let mut context = MwmFastFocusContextAbi::empty();
        let mut status = MwmStatus::ok();
        let code = unsafe {
            ffi::backend_prepare_fast_focus_context(self.raw.as_ptr(), &mut context, &mut status)
        };
        status.code = code;

        if code != MWM_STATUS_OK {
            return Err(status_error(&mut status));
        }

        let context = OwnedFastFocusContext::new(context);
        unsafe { ffi::status_release(&mut status) };
        context.validate()?.into_native_context()
    }

    fn snapshot_via(
        &self,
        fetch: unsafe fn(*mut c_void, *mut MwmDesktopSnapshotAbi, *mut MwmStatus) -> i32,
    ) -> Result<OwnedDesktopSnapshot, MacosNativeBridgeError> {
        let mut snapshot = MwmDesktopSnapshotAbi::empty();
        let mut status = MwmStatus::ok();
        let code = unsafe { fetch(self.raw.as_ptr(), &mut snapshot, &mut status) };
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

    pub(crate) fn desktop_snapshot_native(
        &self,
    ) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
        self.desktop_snapshot()
            .map_err(bridge_probe_error)?
            .into_native_snapshot()
    }

    pub(crate) fn topology_snapshot_native(
        &self,
    ) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
        self.topology_snapshot()
            .map_err(bridge_probe_error)?
            .into_native_snapshot()
    }

    pub(crate) fn switch_space(&self, space_id: u64) -> Result<(), MacosNativeOperationError> {
        let mut status = MwmStatus::ok();
        let code = unsafe { ffi::backend_switch_space(self.raw.as_ptr(), space_id, &mut status) };
        status.code = code;
        status_result(&mut status)
    }

    pub(crate) fn switch_adjacent_space(
        &self,
        direction: NativeDirection,
        space_id: u64,
    ) -> Result<(), MacosNativeOperationError> {
        let mut status = MwmStatus::ok();
        let code = unsafe {
            ffi::backend_switch_adjacent_space(
                self.raw.as_ptr(),
                direction_to_ffi(Some(direction)),
                space_id,
                &mut status,
            )
        };
        status.code = code;
        status_result(&mut status)
    }

    pub(crate) fn switch_space_in_snapshot(
        &self,
        snapshot: &NativeDesktopSnapshot,
        space_id: u64,
        adjacent_direction: Option<NativeDirection>,
    ) -> Result<(), MacosNativeOperationError> {
        let snapshot = OwnedDesktopSnapshotInput::from_native(snapshot);
        let mut status = MwmStatus::ok();
        let code = unsafe {
            ffi::backend_switch_space_in_snapshot(
                self.raw.as_ptr(),
                snapshot.as_ref(),
                space_id,
                direction_to_ffi(adjacent_direction),
                &mut status,
            )
        };
        status.code = code;
        status_result(&mut status)
    }

    pub(crate) fn switch_space_and_refresh(
        &self,
        snapshot: &NativeDesktopSnapshot,
        space_id: u64,
        adjacent_direction: Option<NativeDirection>,
    ) -> Result<NativeDesktopSnapshot, MacosNativeOperationError> {
        let snapshot = OwnedDesktopSnapshotInput::from_native(snapshot);
        let mut refreshed = MwmDesktopSnapshotAbi::empty();
        let mut status = MwmStatus::ok();
        let code = unsafe {
            ffi::backend_switch_space_and_refresh(
                self.raw.as_ptr(),
                snapshot.as_ref(),
                space_id,
                direction_to_ffi(adjacent_direction),
                &mut refreshed,
                &mut status,
            )
        };
        status.code = code;

        if code != MWM_STATUS_OK {
            return Err(operation_error(&mut status));
        }

        let refreshed = OwnedDesktopSnapshot::new(refreshed)
            .validate()
            .map_err(|_| MacosNativeOperationError::CallFailed("swift macOS backend transport"))?;
        unsafe { ffi::status_release(&mut status) };
        refreshed.into_native_snapshot().map_err(MacosNativeOperationError::from)
    }

    pub(crate) fn focus_window(&self, window_id: u64) -> Result<(), MacosNativeOperationError> {
        let mut status = MwmStatus::ok();
        let code = unsafe { ffi::backend_focus_window(self.raw.as_ptr(), window_id, &mut status) };
        status.code = code;
        status_result(&mut status)
    }

    pub(crate) fn focus_window_with_known_pid(
        &self,
        window_id: u64,
        pid: u32,
    ) -> Result<(), MacosNativeOperationError> {
        let mut status = MwmStatus::ok();
        let code = unsafe {
            ffi::backend_focus_window_with_known_pid(self.raw.as_ptr(), window_id, pid, &mut status)
        };
        status.code = code;
        status_result(&mut status)
    }

    pub(crate) fn focus_window_in_active_space_with_known_pid(
        &self,
        window_id: u64,
        pid: u32,
        target_hint: Option<(u64, NativeBounds)>,
    ) -> Result<(), MacosNativeOperationError> {
        let mut status = MwmStatus::ok();
        let (
            has_target_hint,
            target_hint_space_id,
            target_hint_x,
            target_hint_y,
            target_hint_width,
            target_hint_height,
        ) = target_hint.map_or((0, 0, 0, 0, 0, 0), |(space_id, bounds)| {
            (1, space_id, bounds.x, bounds.y, bounds.width, bounds.height)
        });
        let code = unsafe {
            ffi::backend_focus_window_in_active_space_with_known_pid(
                self.raw.as_ptr(),
                window_id,
                pid,
                has_target_hint,
                target_hint_space_id,
                target_hint_x,
                target_hint_y,
                target_hint_width,
                target_hint_height,
                &mut status,
            )
        };
        status.code = code;
        status_result(&mut status)
    }

    pub(crate) fn focus_same_space_target_in_snapshot(
        &self,
        snapshot: &NativeDesktopSnapshot,
        direction: NativeDirection,
        target_window_id: u64,
    ) -> Result<(), MacosNativeOperationError> {
        let snapshot = OwnedDesktopSnapshotInput::from_native(snapshot);
        let mut status = MwmStatus::ok();
        let code = unsafe {
            ffi::backend_focus_same_space_target_in_snapshot(
                self.raw.as_ptr(),
                snapshot.as_ref(),
                direction_to_ffi(Some(direction)),
                target_window_id,
                &mut status,
            )
        };
        status.code = code;
        status_result(&mut status)
    }

    pub(crate) fn move_window_to_space(
        &self,
        window_id: u64,
        space_id: u64,
    ) -> Result<(), MacosNativeOperationError> {
        let mut status = MwmStatus::ok();
        let code = unsafe {
            ffi::backend_move_window_to_space(self.raw.as_ptr(), window_id, space_id, &mut status)
        };
        status.code = code;
        status_result(&mut status)
    }

    pub(crate) fn swap_window_frames(
        &self,
        source_window_id: u64,
        source_frame: NativeBounds,
        target_window_id: u64,
        target_frame: NativeBounds,
    ) -> Result<(), MacosNativeOperationError> {
        let mut status = MwmStatus::ok();
        let code = unsafe {
            ffi::backend_swap_window_frames(
                self.raw.as_ptr(),
                source_window_id,
                source_frame.x,
                source_frame.y,
                source_frame.width,
                source_frame.height,
                target_window_id,
                target_frame.x,
                target_frame.y,
                target_frame.width,
                target_frame.height,
                &mut status,
            )
        };
        status.code = code;
        status_result(&mut status)
    }

    pub(crate) fn as_ptr(&self) -> *mut c_void {
        self.raw.as_ptr()
    }

    #[cfg(test)]
    pub(crate) fn dangling_for_test() -> Self {
        Self {
            raw: NonNull::<c_void>::dangling(),
        }
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

impl OwnedFastFocusContext {
    fn new(raw: MwmFastFocusContextAbi) -> Self {
        Self { raw }
    }

    fn validate(self) -> Result<Self, MacosNativeBridgeError> {
        self.raw
            .validate()
            .map_err(MacosNativeBridgeError::InvalidFastFocusContextTransport)?;
        Ok(self)
    }

    fn into_native_context(self) -> Result<NativeFastFocusContext, MacosNativeBridgeError> {
        Ok(unsafe { self.raw.to_native_context() })
    }
}

impl Drop for OwnedDesktopSnapshot {
    fn drop(&mut self) {
        unsafe { ffi::desktop_snapshot_release(&mut self.raw) };
    }
}

impl Drop for OwnedFastFocusContext {
    fn drop(&mut self) {
        unsafe { ffi::desktop_snapshot_release(&mut self.raw.snapshot) };
    }
}

impl OwnedDesktopSnapshotInput {
    fn from_native(snapshot: &NativeDesktopSnapshot) -> Self {
        let spaces = snapshot
            .spaces
            .iter()
            .map(|space| MwmSpaceAbi {
                id: space.id,
                display_index: space.display_index,
                active: u8::from(space.active),
                kind: space_kind_to_ffi(space.kind),
            })
            .collect::<Vec<_>>();
        let windows = snapshot
            .windows
            .iter()
            .map(|window| MwmWindowAbi {
                id: window.id,
                pid: window.pid.unwrap_or_default(),
                has_pid: u8::from(window.pid.is_some()),
                app_id_ptr: window
                    .app_id
                    .as_ref()
                    .map(|value| CString::new(value.as_str()).unwrap().into_raw())
                    .unwrap_or(std::ptr::null_mut()),
                title_ptr: window
                    .title
                    .as_ref()
                    .map(|value| CString::new(value.as_str()).unwrap().into_raw())
                    .unwrap_or(std::ptr::null_mut()),
                frame: window
                    .bounds
                    .map_or_else(MwmRectAbi::default, |bounds| MwmRectAbi {
                        x: bounds.x,
                        y: bounds.y,
                        width: bounds.width,
                        height: bounds.height,
                    }),
                has_frame: u8::from(window.bounds.is_some()),
                level: window.level,
                space_id: window.space_id,
                order_index: window.order_index.unwrap_or_default(),
                has_order_index: u8::from(window.order_index.is_some()),
            })
            .collect::<Vec<_>>();

        let spaces_len = spaces.len();
        let windows_len = windows.len();
        let spaces_ptr = if spaces.is_empty() {
            std::ptr::null_mut()
        } else {
            Box::into_raw(spaces.into_boxed_slice()) as *mut MwmSpaceAbi
        };
        let windows_ptr = if windows.is_empty() {
            std::ptr::null_mut()
        } else {
            Box::into_raw(windows.into_boxed_slice()) as *mut MwmWindowAbi
        };

        Self {
            raw: MwmDesktopSnapshotAbi {
                spaces_ptr,
                spaces_len,
                windows_ptr,
                windows_len,
                focused_window_id: snapshot.focused_window_id.unwrap_or_default(),
            },
        }
    }

    fn as_ref(&self) -> *const MwmDesktopSnapshotAbi {
        &self.raw
    }
}

impl Drop for OwnedDesktopSnapshotInput {
    fn drop(&mut self) {
        unsafe {
            if !self.raw.windows_ptr.is_null() {
                let windows = Vec::from_raw_parts(
                    self.raw.windows_ptr,
                    self.raw.windows_len,
                    self.raw.windows_len,
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
            if !self.raw.spaces_ptr.is_null() {
                drop(Vec::from_raw_parts(
                    self.raw.spaces_ptr,
                    self.raw.spaces_len,
                    self.raw.spaces_len,
                ));
            }
        }
        self.raw = MwmDesktopSnapshotAbi::empty();
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
            MWM_STATUS_PROBE_MISSING_TOPOLOGY => {
                MacosNativeProbeError::MissingTopology(static_message(message))
            }
            _ => MacosNativeProbeError::MissingTopology("swift macOS backend"),
        },
        MacosNativeBridgeError::InvalidDesktopSnapshotTransport(_) => {
            MacosNativeProbeError::MissingTopology("swift macOS backend transport")
        }
        MacosNativeBridgeError::InvalidFastFocusContextTransport(_) => {
            MacosNativeProbeError::MissingTopology("swift macOS backend transport")
        }
        MacosNativeBridgeError::NullBackendHandle => {
            MacosNativeProbeError::MissingTopology("swift macOS backend")
        }
    }
}

pub(crate) fn static_message(message: Option<String>) -> &'static str {
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

fn status_result(status: &mut MwmStatus) -> Result<(), MacosNativeOperationError> {
    if status.code == MWM_STATUS_OK {
        unsafe { ffi::status_release(status) };
        return Ok(());
    }

    Err(operation_error(status))
}

fn operation_error(status: &mut MwmStatus) -> MacosNativeOperationError {
    let code = status.code;
    let message = unsafe { status.message() };
    unsafe { ffi::status_release(status) };

    match code {
        MWM_STATUS_OPERATION_MISSING_SPACE
        | MWM_STATUS_OPERATION_MISSING_WINDOW
        | MWM_STATUS_OPERATION_MISSING_WINDOW_FRAME
        | MWM_STATUS_OPERATION_MISSING_WINDOW_PID
        | MWM_STATUS_OPERATION_UNSUPPORTED_STAGE_MANAGER_SPACE
        | MWM_STATUS_OPERATION_NO_DIRECTIONAL_FOCUS_TARGET
        | MWM_STATUS_OPERATION_NO_DIRECTIONAL_MOVE_TARGET
        | MWM_STATUS_OPERATION_CALL_FAILED => {
            MacosNativeOperationError::from_swift_status(code, message.as_deref())
        }
        _ => MacosNativeOperationError::CallFailed("swift macOS backend"),
    }
}

fn direction_to_ffi(direction: Option<NativeDirection>) -> i32 {
    match direction {
        Some(NativeDirection::West) => 0,
        Some(NativeDirection::East) => 1,
        Some(NativeDirection::North) => 2,
        Some(NativeDirection::South) => 3,
        None => -1,
    }
}

fn space_kind_to_ffi(kind: SpaceKind) -> i32 {
    match kind {
        SpaceKind::Desktop => 0,
        SpaceKind::Fullscreen => 1,
        SpaceKind::SplitView => 2,
        SpaceKind::System => 3,
        SpaceKind::StageManagerOpaque => 4,
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
pub(crate) fn ffi_test_guard() -> std::sync::MutexGuard<'static, ()> {
    use std::sync::{Mutex, OnceLock};

    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}

#[cfg(test)]
pub(crate) fn test_switch_space_error_from_ffi() -> MacosNativeOperationError {
    use std::mem::ManuallyDrop;

    let _guard = ffi_test_guard();
    ffi::test_support::reset();
    ffi::test_support::set_switch_space_in_snapshot_response(ffi::TestOperationResponse {
        code: MWM_STATUS_OPERATION_MISSING_WINDOW,
        status: MwmStatus {
            code: MWM_STATUS_OPERATION_MISSING_WINDOW,
            message_ptr: CString::new("77").unwrap().into_raw(),
        },
    });

    let shim = ManuallyDrop::new(SwiftBackendShim {
        raw: NonNull::<c_void>::dangling(),
    });
    let snapshot = test_snapshot_from_ffi();
    shim.switch_space_in_snapshot(&snapshot, 102, Some(NativeDirection::West))
        .unwrap_err()
}

#[cfg(test)]
#[test]
fn switch_space_in_snapshot_maps_swift_operation_error() {
    let err = test_switch_space_error_from_ffi();
    assert!(matches!(
        err,
        crate::MacosNativeOperationError::MissingWindow(_)
    ));
}

#[cfg(test)]
#[test]
fn switch_space_and_refresh_maps_swift_operation_error() {
    use std::mem::ManuallyDrop;

    let _guard = ffi_test_guard();
    ffi::test_support::reset();
    ffi::test_support::set_switch_space_and_refresh_response(ffi::TestDesktopSnapshotResponse {
        code: MWM_STATUS_OPERATION_MISSING_WINDOW,
        snapshot: MwmDesktopSnapshotAbi::empty(),
        status: MwmStatus {
            code: MWM_STATUS_OPERATION_MISSING_WINDOW,
            message_ptr: CString::new("77").unwrap().into_raw(),
        },
    });

    let shim = ManuallyDrop::new(SwiftBackendShim {
        raw: NonNull::<c_void>::dangling(),
    });
    let snapshot = test_snapshot_from_ffi();
    let err = shim
        .switch_space_and_refresh(&snapshot, 102, Some(NativeDirection::West))
        .unwrap_err();

    assert!(matches!(
        err,
        crate::MacosNativeOperationError::MissingWindow(_)
    ));
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
fn sample_fast_focus_context_abi() -> MwmFastFocusContextAbi {
    MwmFastFocusContextAbi {
        snapshot: sample_snapshot_abi(),
        environment: 0,
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
        let _guard = super::ffi_test_guard();
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

    #[test]
    fn topology_snapshot_wrapper_uses_topology_transport() {
        let _guard = super::ffi_test_guard();
        ffi::test_support::reset();
        ffi::test_support::set_topology_snapshot_response(TestDesktopSnapshotResponse {
            code: MWM_STATUS_OK,
            snapshot: super::sample_snapshot_abi(),
            status: MwmStatus::ok(),
        });

        let shim = ManuallyDrop::new(SwiftBackendShim {
            raw: NonNull::<c_void>::dangling(),
        });

        let snapshot = shim.topology_snapshot_native().unwrap();

        assert_eq!(snapshot.spaces.len(), 2);
        assert_eq!(snapshot.windows.len(), 3);
        assert_eq!(snapshot.focused_window_id, Some(9003));
        assert_eq!(ffi::test_support::desktop_snapshot_release_calls(), 1);
    }

    #[test]
    fn fast_focus_context_maps_transport() {
        let _guard = super::ffi_test_guard();
        ffi::test_support::reset();
        ffi::test_support::set_fast_focus_context_response(ffi::TestFastFocusContextResponse {
            code: MWM_STATUS_OK,
            context: super::sample_fast_focus_context_abi(),
            status: MwmStatus::ok(),
        });

        let shim = ManuallyDrop::new(SwiftBackendShim {
            raw: NonNull::<c_void>::dangling(),
        });

        let context = shim.prepare_fast_focus_context().unwrap();

        assert_eq!(
            context.environment,
            crate::NativeFastFocusEnvironment::Validated
        );
        assert_eq!(context.desktop_snapshot.focused_window_id, Some(9003));
        assert_eq!(context.desktop_snapshot.windows.len(), 3);
        assert_eq!(ffi::test_support::desktop_snapshot_release_calls(), 1);
    }

    #[test]
    fn fast_focus_context_releases_transport_when_validation_fails() {
        let _guard = super::ffi_test_guard();
        ffi::test_support::reset();
        ffi::test_support::set_fast_focus_context_response(ffi::TestFastFocusContextResponse {
            code: MWM_STATUS_OK,
            context: super::MwmFastFocusContextAbi {
                snapshot: MwmDesktopSnapshotAbi {
                    windows_len: 1,
                    ..MwmDesktopSnapshotAbi::empty()
                },
                environment: 0,
            },
            status: MwmStatus::ok(),
        });

        let shim = ManuallyDrop::new(SwiftBackendShim {
            raw: NonNull::<c_void>::dangling(),
        });

        let error = match shim.prepare_fast_focus_context() {
            Ok(_) => panic!("prepare_fast_focus_context should fail validation"),
            Err(error) => error,
        };

        assert_eq!(
            error,
            MacosNativeBridgeError::InvalidFastFocusContextTransport(
                "windows_ptr was null for a non-empty snapshot"
            )
        );
        assert_eq!(ffi::test_support::desktop_snapshot_release_calls(), 1);
    }
}
