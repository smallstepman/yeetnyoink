#![allow(dead_code)]

use std::ffi::{CStr, c_char};

pub(crate) const MWM_STATUS_OK: i32 = 0;
pub(crate) const MWM_STATUS_INVALID_ARGUMENT: i32 = 1;
pub(crate) const MWM_STATUS_UNAVAILABLE: i32 = 2;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
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
#[derive(Debug, Clone, Copy, Default)]
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
#[derive(Debug, Clone, Copy, Default)]
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
