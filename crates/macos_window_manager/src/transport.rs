#![allow(dead_code)]

use std::ffi::{c_char, CStr};

use crate::{
    desktop_topology_snapshot::SpaceKind, NativeBounds, NativeDesktopSnapshot, NativeSpaceSnapshot,
    NativeWindowSnapshot,
};

pub(crate) const MWM_STATUS_OK: i32 = 0;
pub(crate) const MWM_STATUS_INVALID_ARGUMENT: i32 = 1;
pub(crate) const MWM_STATUS_UNAVAILABLE: i32 = 2;
pub(crate) const MWM_STATUS_CONNECT_MISSING_REQUIRED_SYMBOL: i32 = 10;
pub(crate) const MWM_STATUS_CONNECT_MISSING_ACCESSIBILITY_PERMISSION: i32 = 11;
pub(crate) const MWM_STATUS_CONNECT_MISSING_TOPOLOGY_PRECONDITION: i32 = 12;
pub(crate) const MWM_STATUS_PROBE_MISSING_TOPOLOGY: i32 = 20;

/// ABI layout assertions for the Swift transport contract.
///
/// The Swift definitions in `swift/Sources/MacosWindowManagerFFI/Transport.swift`
/// must keep these exact sizes, alignments, and field offsets on macOS 64-bit.
/// Pointer payloads are owned by the Swift FFI layer when non-null and must be
/// released via `mwm_status_release` or `mwm_desktop_snapshot_release` after the
/// Rust side has copied any needed data out of them.
#[cfg(not(target_pointer_width = "64"))]
compile_error!("macos_window_manager FFI transport requires a 64-bit target");
#[cfg(target_pointer_width = "64")]
const _: [(); 16] = [(); std::mem::size_of::<MwmStatus>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 8] = [(); std::mem::align_of::<MwmStatus>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 0] = [(); std::mem::offset_of!(MwmStatus, code)];
#[cfg(target_pointer_width = "64")]
const _: [(); 8] = [(); std::mem::offset_of!(MwmStatus, message_ptr)];
#[cfg(target_pointer_width = "64")]
const _: [(); 16] = [(); std::mem::size_of::<MwmRectAbi>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 4] = [(); std::mem::align_of::<MwmRectAbi>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 0] = [(); std::mem::offset_of!(MwmRectAbi, x)];
#[cfg(target_pointer_width = "64")]
const _: [(); 4] = [(); std::mem::offset_of!(MwmRectAbi, y)];
#[cfg(target_pointer_width = "64")]
const _: [(); 8] = [(); std::mem::offset_of!(MwmRectAbi, width)];
#[cfg(target_pointer_width = "64")]
const _: [(); 12] = [(); std::mem::offset_of!(MwmRectAbi, height)];
#[cfg(target_pointer_width = "64")]
const _: [(); 24] = [(); std::mem::size_of::<MwmSpaceAbi>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 8] = [(); std::mem::align_of::<MwmSpaceAbi>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 0] = [(); std::mem::offset_of!(MwmSpaceAbi, id)];
#[cfg(target_pointer_width = "64")]
const _: [(); 8] = [(); std::mem::offset_of!(MwmSpaceAbi, display_index)];
#[cfg(target_pointer_width = "64")]
const _: [(); 16] = [(); std::mem::offset_of!(MwmSpaceAbi, active)];
#[cfg(target_pointer_width = "64")]
const _: [(); 20] = [(); std::mem::offset_of!(MwmSpaceAbi, kind)];
#[cfg(target_pointer_width = "64")]
const _: [(); 80] = [(); std::mem::size_of::<MwmWindowAbi>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 8] = [(); std::mem::align_of::<MwmWindowAbi>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 0] = [(); std::mem::offset_of!(MwmWindowAbi, id)];
#[cfg(target_pointer_width = "64")]
const _: [(); 8] = [(); std::mem::offset_of!(MwmWindowAbi, pid)];
#[cfg(target_pointer_width = "64")]
const _: [(); 12] = [(); std::mem::offset_of!(MwmWindowAbi, has_pid)];
#[cfg(target_pointer_width = "64")]
const _: [(); 16] = [(); std::mem::offset_of!(MwmWindowAbi, app_id_ptr)];
#[cfg(target_pointer_width = "64")]
const _: [(); 24] = [(); std::mem::offset_of!(MwmWindowAbi, title_ptr)];
#[cfg(target_pointer_width = "64")]
const _: [(); 32] = [(); std::mem::offset_of!(MwmWindowAbi, frame)];
#[cfg(target_pointer_width = "64")]
const _: [(); 48] = [(); std::mem::offset_of!(MwmWindowAbi, has_frame)];
#[cfg(target_pointer_width = "64")]
const _: [(); 52] = [(); std::mem::offset_of!(MwmWindowAbi, level)];
#[cfg(target_pointer_width = "64")]
const _: [(); 56] = [(); std::mem::offset_of!(MwmWindowAbi, space_id)];
#[cfg(target_pointer_width = "64")]
const _: [(); 64] = [(); std::mem::offset_of!(MwmWindowAbi, order_index)];
#[cfg(target_pointer_width = "64")]
const _: [(); 72] = [(); std::mem::offset_of!(MwmWindowAbi, has_order_index)];
#[cfg(target_pointer_width = "64")]
const _: [(); 40] = [(); std::mem::size_of::<MwmDesktopSnapshotAbi>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 8] = [(); std::mem::align_of::<MwmDesktopSnapshotAbi>()];
#[cfg(target_pointer_width = "64")]
const _: [(); 0] = [(); std::mem::offset_of!(MwmDesktopSnapshotAbi, spaces_ptr)];
#[cfg(target_pointer_width = "64")]
const _: [(); 8] = [(); std::mem::offset_of!(MwmDesktopSnapshotAbi, spaces_len)];
#[cfg(target_pointer_width = "64")]
const _: [(); 16] = [(); std::mem::offset_of!(MwmDesktopSnapshotAbi, windows_ptr)];
#[cfg(target_pointer_width = "64")]
const _: [(); 24] = [(); std::mem::offset_of!(MwmDesktopSnapshotAbi, windows_len)];
#[cfg(target_pointer_width = "64")]
const _: [(); 32] = [(); std::mem::offset_of!(MwmDesktopSnapshotAbi, focused_window_id)];

#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct MwmStatus {
    pub code: i32,
    pub message_ptr: *mut c_char,
}

impl MwmStatus {
    pub(crate) fn ok() -> Self {
        Self::default()
    }

    pub(crate) fn unavailable() -> Self {
        Self {
            code: MWM_STATUS_UNAVAILABLE,
            message_ptr: std::ptr::null_mut(),
        }
    }

    pub(crate) unsafe fn message(&self) -> Option<String> {
        (!self.message_ptr.is_null()).then(|| unsafe {
            CStr::from_ptr(self.message_ptr)
                .to_string_lossy()
                .into_owned()
        })
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct MwmRectAbi {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct MwmSpaceAbi {
    pub id: u64,
    pub display_index: usize,
    pub active: u8,
    pub kind: i32,
}

#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct MwmWindowAbi {
    pub id: u64,
    pub pid: u32,
    pub has_pid: u8,
    pub app_id_ptr: *mut c_char,
    pub title_ptr: *mut c_char,
    pub frame: MwmRectAbi,
    pub has_frame: u8,
    pub level: i32,
    pub space_id: u64,
    pub order_index: usize,
    pub has_order_index: u8,
}

#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct MwmDesktopSnapshotAbi {
    pub spaces_ptr: *mut MwmSpaceAbi,
    pub spaces_len: usize,
    pub windows_ptr: *mut MwmWindowAbi,
    pub windows_len: usize,
    pub focused_window_id: u64,
}

impl MwmDesktopSnapshotAbi {
    pub(crate) fn empty() -> Self {
        Self::default()
    }

    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if self.spaces_len > 0 && self.spaces_ptr.is_null() {
            return Err("spaces_ptr was null for a non-empty snapshot");
        }

        if self.windows_len > 0 && self.windows_ptr.is_null() {
            return Err("windows_ptr was null for a non-empty snapshot");
        }

        Ok(())
    }
}

impl MwmRectAbi {
    fn to_native_bounds(self) -> NativeBounds {
        NativeBounds {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
        }
    }
}

impl MwmSpaceAbi {
    fn kind(self) -> SpaceKind {
        match self.kind {
            0 => SpaceKind::Desktop,
            1 => SpaceKind::Fullscreen,
            2 => SpaceKind::SplitView,
            3 => SpaceKind::System,
            4 => SpaceKind::StageManagerOpaque,
            _ => SpaceKind::System,
        }
    }
}

impl MwmDesktopSnapshotAbi {
    pub(crate) unsafe fn to_native_snapshot(&self) -> NativeDesktopSnapshot {
        let spaces = if self.spaces_len == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(self.spaces_ptr, self.spaces_len) }
        }
        .iter()
        .copied()
        .map(|space| NativeSpaceSnapshot {
            id: space.id,
            display_index: space.display_index,
            active: space.active != 0,
            kind: space.kind(),
        })
        .collect::<Vec<_>>();

        let windows = if self.windows_len == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(self.windows_ptr, self.windows_len) }
        }
        .iter()
        .map(|window| NativeWindowSnapshot {
            id: window.id,
            pid: (window.has_pid != 0).then_some(window.pid),
            app_id: unsafe { owned_string(window.app_id_ptr) },
            title: unsafe { owned_string(window.title_ptr) },
            bounds: (window.has_frame != 0).then_some(window.frame.to_native_bounds()),
            level: window.level,
            space_id: window.space_id,
            order_index: (window.has_order_index != 0).then_some(window.order_index),
        })
        .collect::<Vec<_>>();

        NativeDesktopSnapshot {
            active_space_ids: spaces
                .iter()
                .filter(|space| space.active)
                .map(|space| space.id)
                .collect(),
            spaces,
            windows,
            focused_window_id: (self.focused_window_id != 0).then_some(self.focused_window_id),
        }
    }
}

unsafe fn owned_string(ptr: *mut c_char) -> Option<String> {
    (!ptr.is_null()).then(|| unsafe { CStr::from_ptr(ptr).to_string_lossy().into_owned() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ffi_transport_layout_matches_swift_contract() {
        assert_eq!(std::mem::size_of::<MwmStatus>(), 16);
        assert_eq!(std::mem::align_of::<MwmStatus>(), 8);
        assert_eq!(std::mem::offset_of!(MwmStatus, code), 0);
        assert_eq!(std::mem::offset_of!(MwmStatus, message_ptr), 8);

        assert_eq!(std::mem::size_of::<MwmRectAbi>(), 16);
        assert_eq!(std::mem::align_of::<MwmRectAbi>(), 4);
        assert_eq!(std::mem::offset_of!(MwmRectAbi, x), 0);
        assert_eq!(std::mem::offset_of!(MwmRectAbi, y), 4);
        assert_eq!(std::mem::offset_of!(MwmRectAbi, width), 8);
        assert_eq!(std::mem::offset_of!(MwmRectAbi, height), 12);

        assert_eq!(std::mem::size_of::<MwmSpaceAbi>(), 24);
        assert_eq!(std::mem::align_of::<MwmSpaceAbi>(), 8);
        assert_eq!(std::mem::offset_of!(MwmSpaceAbi, id), 0);
        assert_eq!(std::mem::offset_of!(MwmSpaceAbi, display_index), 8);
        assert_eq!(std::mem::offset_of!(MwmSpaceAbi, active), 16);
        assert_eq!(std::mem::offset_of!(MwmSpaceAbi, kind), 20);

        assert_eq!(std::mem::size_of::<MwmWindowAbi>(), 80);
        assert_eq!(std::mem::align_of::<MwmWindowAbi>(), 8);
        assert_eq!(std::mem::offset_of!(MwmWindowAbi, id), 0);
        assert_eq!(std::mem::offset_of!(MwmWindowAbi, pid), 8);
        assert_eq!(std::mem::offset_of!(MwmWindowAbi, has_pid), 12);
        assert_eq!(std::mem::offset_of!(MwmWindowAbi, app_id_ptr), 16);
        assert_eq!(std::mem::offset_of!(MwmWindowAbi, title_ptr), 24);
        assert_eq!(std::mem::offset_of!(MwmWindowAbi, frame), 32);
        assert_eq!(std::mem::offset_of!(MwmWindowAbi, has_frame), 48);
        assert_eq!(std::mem::offset_of!(MwmWindowAbi, level), 52);
        assert_eq!(std::mem::offset_of!(MwmWindowAbi, space_id), 56);
        assert_eq!(std::mem::offset_of!(MwmWindowAbi, order_index), 64);
        assert_eq!(std::mem::offset_of!(MwmWindowAbi, has_order_index), 72);

        assert_eq!(std::mem::size_of::<MwmDesktopSnapshotAbi>(), 40);
        assert_eq!(std::mem::align_of::<MwmDesktopSnapshotAbi>(), 8);
        assert_eq!(std::mem::offset_of!(MwmDesktopSnapshotAbi, spaces_ptr), 0);
        assert_eq!(std::mem::offset_of!(MwmDesktopSnapshotAbi, spaces_len), 8);
        assert_eq!(std::mem::offset_of!(MwmDesktopSnapshotAbi, windows_ptr), 16);
        assert_eq!(std::mem::offset_of!(MwmDesktopSnapshotAbi, windows_len), 24);
        assert_eq!(
            std::mem::offset_of!(MwmDesktopSnapshotAbi, focused_window_id),
            32
        );
    }
}
