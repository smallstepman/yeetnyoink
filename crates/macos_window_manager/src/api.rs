use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use crate::desktop_topology_snapshot::{
    RawSpaceRecord, RawTopologySnapshot, RawWindow, WindowSnapshot,
};
use crate::error::{MacosNativeConnectError, MacosNativeOperationError, MacosNativeProbeError};
use crate::{
    active_space_ax_backed_same_pid_target, active_space_focus_target_hint_from_topology,
    active_window_pid_from_topology, desktop_topology_snapshot, ensure_supported_target_space,
    focus_same_space_target_in_snapshot, focused_window_from_active_space_windows,
    space_id_for_window, space_transition_window_ids, switch_space_in_snapshot,
    validate_environment_with_api, wait_for_space_presentation,
};

pub type NativeSpaceId = u64;
pub type NativeWindowId = u64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeDesktopSnapshot {
    pub spaces: Vec<NativeSpaceSnapshot>,
    pub active_space_ids: HashSet<NativeSpaceId>,
    pub windows: Vec<NativeWindowSnapshot>,
    pub focused_window_id: Option<NativeWindowId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeSpaceSnapshot {
    pub id: NativeSpaceId,
    pub display_index: usize,
    pub active: bool,
    pub kind: desktop_topology_snapshot::SpaceKind,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeWindowSnapshot {
    pub id: NativeWindowId,
    pub pid: Option<u32>,
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub bounds: Option<NativeBounds>,
    pub level: i32,
    pub space_id: NativeSpaceId,
    pub order_index: Option<usize>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeBounds {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeDirection {
    West,
    East,
    North,
    South,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveSpaceFocusTargetHint {
    pub space_id: NativeSpaceId,
    pub bounds: NativeBounds,
}

impl std::fmt::Display for NativeDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::West => f.write_str("west"),
            Self::East => f.write_str("east"),
            Self::North => f.write_str("north"),
            Self::South => f.write_str("south"),
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MissionControlModifiers {
    pub control: bool,
    pub option: bool,
    pub command: bool,
    pub shift: bool,
    pub function: bool,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MissionControlHotkey {
    pub key_code: u16,
    pub mission_control: MissionControlModifiers,
}

#[allow(dead_code)]
pub struct NativeBackendOptions {
    pub west_space_hotkey: MissionControlHotkey,
    pub east_space_hotkey: MissionControlHotkey,
    pub diagnostics: Option<Arc<dyn NativeDiagnostics>>,
}

#[allow(dead_code)]
pub trait NativeDiagnostics: Send + Sync {
    fn debug(&self, message: &str);
}

pub trait MacosNativeApi {
    fn has_symbol(&self, symbol: &'static str) -> bool;
    fn ax_is_trusted(&self) -> bool;
    fn minimal_topology_ready(&self) -> bool;
    fn debug(&self, _message: &str) {}
    fn validate_environment(&self) -> Result<(), MacosNativeConnectError> {
        validate_environment_with_api(self)
    }
    #[allow(dead_code)]
    fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError>;
    fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError>;
    fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError>;
    fn active_space_windows(&self, space_id: u64) -> Result<Vec<RawWindow>, MacosNativeProbeError>;
    fn inactive_space_window_ids(&self) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError>;
    fn focused_window_id(&self) -> Result<Option<NativeWindowId>, MacosNativeProbeError> {
        Ok(None)
    }
    fn focused_window_snapshot(&self) -> Result<WindowSnapshot, MacosNativeProbeError> {
        let active_space_ids = self.active_space_ids()?;
        let active_space_windows = active_space_ids
            .into_iter()
            .map(|space_id| {
                self.active_space_windows(space_id)
                    .map(|windows| (space_id, windows))
            })
            .collect::<Result<HashMap<_, _>, _>>()?;
        focused_window_from_active_space_windows(&active_space_windows, self.focused_window_id()?)
    }
    #[allow(dead_code)]
    fn ax_window_ids_for_pid(&self, _pid: u32) -> Result<Vec<u64>, MacosNativeOperationError> {
        Ok(Vec::new())
    }
    fn onscreen_window_ids(&self) -> Result<HashSet<NativeWindowId>, MacosNativeProbeError> {
        Ok(HashSet::new())
    }
    fn switch_space(&self, space_id: u64) -> Result<(), MacosNativeOperationError>;
    fn switch_adjacent_space(
        &self,
        _direction: NativeDirection,
        space_id: u64,
    ) -> Result<(), MacosNativeOperationError> {
        self.switch_space(space_id)
    }
    fn focus_window(&self, window_id: u64) -> Result<(), MacosNativeOperationError>;
    fn focus_window_with_known_pid(
        &self,
        window_id: u64,
        _pid: u32,
    ) -> Result<(), MacosNativeOperationError> {
        self.focus_window(window_id)
    }
    fn focus_window_in_active_space_with_known_pid(
        &self,
        window_id: u64,
        pid: u32,
        target_hint: Option<ActiveSpaceFocusTargetHint>,
    ) -> Result<(), MacosNativeOperationError> {
        match self.focus_window_with_known_pid(window_id, pid) {
            Err(MacosNativeOperationError::MissingWindow(missing_window_id))
                if missing_window_id == window_id =>
            {
                if let Some(remapped_target_id) = active_space_ax_backed_same_pid_target(
                    self,
                    &self.desktop_snapshot()?,
                    window_id,
                    pid,
                    target_hint,
                )? {
                    self.debug(&format!(
                        "macos_native: active-space focus remapped stale same-pid target {} to {}",
                        window_id, remapped_target_id
                    ));
                    return self.focus_window_with_known_pid(remapped_target_id, pid);
                }
                Err(MacosNativeOperationError::MissingWindow(window_id))
            }
            other => other,
        }
    }
    fn switch_space_in_snapshot(
        &self,
        snapshot: &NativeDesktopSnapshot,
        space_id: u64,
        adjacent_direction: Option<NativeDirection>,
    ) -> Result<(), MacosNativeOperationError> {
        switch_space_in_snapshot(self, snapshot, space_id, adjacent_direction)
    }
    fn focus_same_space_target_in_snapshot(
        &self,
        snapshot: &NativeDesktopSnapshot,
        direction: NativeDirection,
        target_window_id: u64,
    ) -> Result<(), MacosNativeOperationError> {
        focus_same_space_target_in_snapshot(self, snapshot, direction, target_window_id)
    }
    fn focus_window_by_id(&self, window_id: u64) -> Result<(), MacosNativeOperationError> {
        let topology = self.topology_snapshot()?;
        let target_space_id = space_id_for_window(&topology, window_id)
            .ok_or(MacosNativeOperationError::MissingWindow(window_id))?;
        ensure_supported_target_space(&topology, target_space_id)?;

        let mut refreshed_topology = None;
        if !topology.active_space_ids.contains(&target_space_id) {
            let (source_focus_window_id, target_window_ids) =
                space_transition_window_ids(&topology, target_space_id);
            self.debug(&format!(
                "macos_native: switching to space {target_space_id} source_focus={:?} target_windows={}",
                source_focus_window_id,
                target_window_ids.len()
            ));
            self.switch_space(target_space_id)?;
            wait_for_space_presentation(
                self,
                target_space_id,
                source_focus_window_id,
                &target_window_ids,
            )?;
            refreshed_topology = Some(self.topology_snapshot()?);
        }

        let focus_topology = refreshed_topology.as_ref().unwrap_or(&topology);
        if let Some(pid) = active_window_pid_from_topology(focus_topology, window_id) {
            if refreshed_topology.is_some() {
                let target_hint =
                    active_space_focus_target_hint_from_topology(focus_topology, window_id);
                self.debug(&format!(
                    "macos_native: focusing window {window_id} in active space via known pid {pid}"
                ));
                self.focus_window_in_active_space_with_known_pid(window_id, pid, target_hint)
            } else {
                self.debug(&format!(
                    "macos_native: focusing window {window_id} via known pid {pid}"
                ));
                self.focus_window_with_known_pid(window_id, pid)
            }
        } else {
            self.debug(&format!(
                "macos_native: focusing window {window_id} via description lookup"
            ));
            self.focus_window(window_id)
        }
    }
    fn move_window_to_space(
        &self,
        window_id: u64,
        space_id: u64,
    ) -> Result<(), MacosNativeOperationError>;
    fn swap_window_frames(
        &self,
        source_window_id: u64,
        source_frame: NativeBounds,
        target_window_id: u64,
        target_frame: NativeBounds,
    ) -> Result<(), MacosNativeOperationError>;

    fn topology_snapshot(&self) -> Result<RawTopologySnapshot, MacosNativeProbeError> {
        let spaces = self.managed_spaces()?;
        let active_space_ids = self.active_space_ids()?;
        let active_space_windows = active_space_ids
            .iter()
            .copied()
            .map(|space_id| {
                self.active_space_windows(space_id)
                    .map(|windows| (space_id, windows))
            })
            .collect::<Result<HashMap<_, _>, _>>()?;
        let inactive_space_window_ids = self.inactive_space_window_ids()?;

        Ok(RawTopologySnapshot {
            spaces,
            active_space_ids,
            active_space_windows,
            inactive_space_window_ids,
            focused_window_id: self.focused_window_id()?,
        })
    }

    fn topology_snapshot_without_focus(
        &self,
    ) -> Result<RawTopologySnapshot, MacosNativeProbeError> {
        let mut topology = self.topology_snapshot()?;
        topology.focused_window_id = None;
        Ok(topology)
    }
}
