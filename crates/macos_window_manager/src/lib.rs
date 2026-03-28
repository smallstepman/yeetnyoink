mod ax;
mod desktop_topology_snapshot;
mod error;
mod foundation;
mod skylight;
mod window_server;

use std::{
    collections::{HashMap, HashSet},
    ffi::{CString, c_void},
    sync::Arc,
    time::Instant,
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



fn validate_environment_with_api<A: MacosNativeApi + ?Sized>(
    api: &A,
) -> Result<(), MacosNativeConnectError> {
    for symbol in REQUIRED_PRIVATE_SYMBOLS {
        if !api.has_symbol(symbol) {
            return Err(MacosNativeConnectError::MissingRequiredSymbol(symbol));
        }
    }

    if !api.ax_is_trusted() {
        return Err(MacosNativeConnectError::MissingAccessibilityPermission);
    }

    if !api.minimal_topology_ready() {
        return Err(MacosNativeConnectError::MissingTopologyPrecondition(
            "main SkyLight connection",
        ));
    }

    Ok(())
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
    fn active_space_windows(
        &self,
        space_id: u64,
    ) -> Result<Vec<RawWindow>, MacosNativeProbeError>;
    fn inactive_space_window_ids(
        &self,
    ) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError>;
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
        focused_window_from_active_space_windows(
            &active_space_windows,
            self.focused_window_id()?,
        )
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

fn wait_for_space_presentation<A: MacosNativeApi + ?Sized>(
    api: &A,
    space_id: u64,
    source_focus_window_id: Option<u64>,
    target_window_ids: &HashSet<u64>,
) -> Result<(), MacosNativeOperationError> {
    let deadline = Instant::now() + SPACE_SWITCH_SETTLE_TIMEOUT;
    let mut polls = 0usize;
    let mut stable_target_polls = 0usize;

    loop {
        polls += 1;
        let active_space_ids = api.active_space_ids()?;
        let onscreen_window_ids = api.onscreen_window_ids()?;
        let target_active = active_space_ids.contains(&space_id);
        let source_focus_hidden = source_focus_window_id
            .is_none_or(|window_id| !onscreen_window_ids.contains(&window_id));
        let target_visible = target_window_ids.is_empty()
            || !target_window_ids.is_disjoint(&onscreen_window_ids);
        if target_active && target_visible {
            stable_target_polls += 1;
        } else {
            stable_target_polls = 0;
        }

        if target_active
            && target_visible
            && (source_focus_hidden || stable_target_polls >= SPACE_SWITCH_STABLE_TARGET_POLLS)
        {
            api.debug(&format!(
                "macos_native: space {space_id} presentation settled after {polls} poll(s)"
            ));
            return Ok(());
        }

        if Instant::now() >= deadline {
            api.debug(&format!(
                "macos_native: space {space_id} did not settle after {polls} poll(s) target_active={target_active} source_focus_hidden={source_focus_hidden} target_visible={target_visible}"
            ));
            return Err(MacosNativeOperationError::CallFailed(
                "wait_for_active_space",
            ));
        }

        std::thread::sleep(SPACE_SWITCH_POLL_INTERVAL);
    }
}

fn switch_space_in_snapshot<A: MacosNativeApi + ?Sized>(
    api: &A,
    snapshot: &NativeDesktopSnapshot,
    space_id: u64,
    adjacent_direction: Option<NativeDirection>,
) -> Result<(), MacosNativeOperationError> {
    let Some(target_space) = snapshot.spaces.iter().find(|space| space.id == space_id) else {
        return Err(MacosNativeOperationError::MissingSpace(space_id));
    };
    if target_space.kind == SpaceKind::StageManagerOpaque {
        return Err(MacosNativeOperationError::UnsupportedStageManagerSpace(
            space_id,
        ));
    }
    if snapshot.active_space_ids.contains(&space_id) {
        return Ok(());
    }

    let (source_focus_window_id, target_window_ids) =
        outer_space_transition_window_ids(snapshot, space_id);
    api.debug(&format!(
        "macos_native: switching to space {space_id} source_focus={:?} target_windows={}",
        source_focus_window_id,
        target_window_ids.len()
    ));
    if let Some(direction) = adjacent_direction {
        if target_window_ids.is_empty() {
            api.debug(&format!(
                "macos_native: using exact space switch for empty adjacent space {space_id}"
            ));
            api.switch_space(space_id)?;
            return wait_for_space_presentation(
                api,
                space_id,
                source_focus_window_id,
                &target_window_ids,
            );
        }

        api.switch_adjacent_space(direction, space_id)?;
        match wait_for_space_presentation(
            api,
            space_id,
            source_focus_window_id,
            &target_window_ids,
        ) {
            Ok(()) => Ok(()),
            Err(err) => {
                let target_still_inactive = match api.active_space_ids() {
                    Ok(active_space_ids) => !active_space_ids.contains(&space_id),
                    Err(probe_err) => {
                        api.debug(&format!(
                            "macos_native: failed to re-check active spaces after adjacent hotkey switch failure for space {space_id} ({probe_err}); retrying exact space switch"
                        ));
                        true
                    }
                };

                if !target_still_inactive {
                    return Err(err);
                }

                let retry_target_window_ids = match api.onscreen_window_ids() {
                    Ok(onscreen_window_ids)
                        if !target_window_ids.is_empty()
                            && !target_window_ids.is_disjoint(&onscreen_window_ids) =>
                    {
                        api.debug(&format!(
                            "macos_native: adjacent hotkey left target-space window ids visible while target space {space_id} is still inactive; treating target ids as unreliable for exact-switch retry"
                        ));
                        HashSet::new()
                    }
                    Ok(_) => target_window_ids.clone(),
                    Err(probe_err) => {
                        api.debug(&format!(
                            "macos_native: failed to inspect onscreen windows after adjacent hotkey switch failure for space {space_id} ({probe_err}); preserving target ids for exact-switch retry"
                        ));
                        target_window_ids.clone()
                    }
                };

                api.debug(&format!(
                    "macos_native: adjacent hotkey did not activate target space {space_id}; retrying exact space switch"
                ));
                api.switch_space(space_id)?;
                wait_for_space_presentation(
                    api,
                    space_id,
                    source_focus_window_id,
                    &retry_target_window_ids,
                )
            }
        }
    } else {
        api.switch_space(space_id)?;
        wait_for_space_presentation(api, space_id, source_focus_window_id, &target_window_ids)
    }
}

fn native_window(
    snapshot: &NativeDesktopSnapshot,
    window_id: u64,
) -> Option<&NativeWindowSnapshot> {
    snapshot
        .windows
        .iter()
        .find(|window| window.id == window_id)
}

fn native_space(
    snapshot: &NativeDesktopSnapshot,
    space_id: u64,
) -> Option<&NativeSpaceSnapshot> {
    snapshot.spaces.iter().find(|space| space.id == space_id)
}

fn outer_space_transition_window_ids(
    snapshot: &NativeDesktopSnapshot,
    target_space_id: u64,
) -> (Option<u64>, HashSet<u64>) {
    let target_display_index = snapshot
        .spaces
        .iter()
        .find(|space| space.id == target_space_id)
        .map(|space| space.display_index);
    let source_space_id = target_display_index.and_then(|display_index| {
        snapshot
            .spaces
            .iter()
            .find(|space| {
                space.active && space.display_index == display_index && space.id != target_space_id
            })
            .map(|space| space.id)
    });
    let source_focus_window_id = snapshot.focused_window_id.filter(|window_id| {
        snapshot
            .windows
            .iter()
            .find(|window| window.id == *window_id)
            .map(|window| window.space_id)
            == source_space_id
    });
    let target_window_ids = snapshot
        .windows
        .iter()
        .filter(|window| window.space_id == target_space_id)
        .map(|window| window.id)
        .collect();

    (source_focus_window_id, target_window_ids)
}

fn compare_native_active_windows(
    left: &NativeWindowSnapshot,
    right: &NativeWindowSnapshot,
) -> std::cmp::Ordering {
    match (left.order_index, right.order_index) {
        (Some(left_index), Some(right_index)) => left_index.cmp(&right_index),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
    .then_with(|| left.id.cmp(&right.id))
}

fn resolved_focused_native_window(
    snapshot: &NativeDesktopSnapshot,
) -> Result<&NativeWindowSnapshot, MacosNativeProbeError> {
    let is_active_window =
        |window: &&NativeWindowSnapshot| snapshot.active_space_ids.contains(&window.space_id);

    if let Some(focused_window_id) = snapshot.focused_window_id {
        if let Some(window) = snapshot
            .windows
            .iter()
            .find(|window| window.id == focused_window_id)
        {
            return Ok(window);
        }
    }

    snapshot
        .windows
        .iter()
        .filter(is_active_window)
        .min_by(|left, right| compare_native_active_windows(left, right))
        .ok_or(MacosNativeProbeError::MissingFocusedWindow)
}

fn native_candidate_extends_in_direction(
    source: NativeBounds,
    candidate: NativeBounds,
    direction: NativeDirection,
) -> bool {
    match direction {
        NativeDirection::West => candidate.x < source.x,
        NativeDirection::East => candidate.x + candidate.width > source.x + source.width,
        NativeDirection::North => candidate.y < source.y,
        NativeDirection::South => candidate.y + candidate.height > source.y + source.height,
    }
}

fn is_directional_focus_window(window: &NativeWindowSnapshot) -> bool {
    window.level == 0
}

fn native_overlap_len(start_a: i32, len_a: i32, start_b: i32, len_b: i32) -> i64 {
    let end_a = start_a + len_a;
    let end_b = start_b + len_b;
    i64::from((end_a.min(end_b) - start_a.max(start_b)).max(0))
}

fn native_overlap_area(left: NativeBounds, right: NativeBounds) -> i64 {
    native_overlap_len(left.x, left.width, right.x, right.width)
        * native_overlap_len(left.y, left.height, right.y, right.height)
}

fn native_center_distance_sq(left: NativeBounds, right: NativeBounds) -> i128 {
    let left_center_x = left.x as i128 + left.width as i128 / 2;
    let left_center_y = left.y as i128 + left.height as i128 / 2;
    let right_center_x = right.x as i128 + right.width as i128 / 2;
    let right_center_y = right.y as i128 + right.height as i128 / 2;
    let delta_x = left_center_x - right_center_x;
    let delta_y = left_center_y - right_center_y;
    delta_x * delta_x + delta_y * delta_y
}

fn compare_native_windows_for_target_match(
    target_bounds: NativeBounds,
    left: &NativeWindowSnapshot,
    right: &NativeWindowSnapshot,
) -> std::cmp::Ordering {
    let left_bounds = left.bounds.expect("bounds should be present");
    let right_bounds = right.bounds.expect("bounds should be present");
    let left_overlap = native_overlap_area(target_bounds, left_bounds);
    let right_overlap = native_overlap_area(target_bounds, right_bounds);
    let left_distance = native_center_distance_sq(target_bounds, left_bounds);
    let right_distance = native_center_distance_sq(target_bounds, right_bounds);

    left_overlap
        .cmp(&right_overlap)
        .then_with(|| right_distance.cmp(&left_distance))
        .then_with(|| compare_native_active_windows(right, left))
}

fn compare_native_windows_for_edge(
    left: &NativeWindowSnapshot,
    right: &NativeWindowSnapshot,
    direction: NativeDirection,
) -> std::cmp::Ordering {
    let left_bounds = left.bounds.expect("bounds should be present");
    let right_bounds = right.bounds.expect("bounds should be present");

    match direction {
        NativeDirection::East => {
            (left_bounds.x + left_bounds.width).cmp(&(right_bounds.x + right_bounds.width))
        }
        NativeDirection::West => right_bounds.x.cmp(&left_bounds.x),
        NativeDirection::North => right_bounds.y.cmp(&left_bounds.y),
        NativeDirection::South => {
            (left_bounds.y + left_bounds.height).cmp(&(right_bounds.y + right_bounds.height))
        }
    }
    .then_with(|| compare_native_active_windows(right, left))
}

fn native_ax_backed_same_pid_target(
    snapshot: &NativeDesktopSnapshot,
    direction: NativeDirection,
    pid: u32,
    ax_window_ids: &HashSet<u64>,
) -> Option<u64> {
    let focused = resolved_focused_native_window(snapshot).ok()?;
    let focused_space = native_space(snapshot, focused.space_id)?;
    if focused.pid != Some(pid) || focused_space.kind != SpaceKind::SplitView {
        return None;
    }

    let source_bounds = focused.bounds?;
    snapshot
        .windows
        .iter()
        .filter(|window| window.id != focused.id)
        .filter(|window| window.space_id == focused.space_id)
        .filter(|window| is_directional_focus_window(window))
        .filter(|window| window.pid == Some(pid))
        .filter(|window| ax_window_ids.contains(&window.id))
        .filter(|window| {
            window.bounds.is_some_and(|bounds| {
                native_candidate_extends_in_direction(source_bounds, bounds, direction)
            })
        })
        .max_by(|left, right| compare_native_windows_for_edge(left, right, direction))
        .map(|window| window.id)
}

fn active_space_ax_backed_same_pid_target<A: MacosNativeApi + ?Sized>(
    api: &A,
    snapshot: &NativeDesktopSnapshot,
    target_window_id: u64,
    pid: u32,
    target_hint: Option<ActiveSpaceFocusTargetHint>,
) -> Result<Option<u64>, MacosNativeOperationError> {
    let target = native_window(snapshot, target_window_id);
    let Some(target_bounds) = target
        .and_then(|window| window.bounds)
        .or(target_hint.map(|hint| hint.bounds))
    else {
        api.debug(&format!(
            "macos_native: active-space stale-target remap skipped; target window {target_window_id} has no bounds"
        ));
        return Ok(None);
    };
    let Some(target_space_id) = target
        .map(|window| window.space_id)
        .or(target_hint.map(|hint| hint.space_id))
    else {
        api.debug(&format!(
            "macos_native: active-space stale-target remap skipped; target window {target_window_id} missing from snapshot"
        ));
        return Ok(None);
    };
    let Some(target_space) = native_space(snapshot, target_space_id) else {
        api.debug(&format!(
            "macos_native: active-space stale-target remap skipped; target space {} missing from snapshot",
            target_space_id
        ));
        return Ok(None);
    };
    if target_space.kind != SpaceKind::SplitView
        || target.is_some_and(|window| window.pid != Some(pid))
    {
        return Ok(None);
    }

    let ax_window_ids = api
        .ax_window_ids_for_pid(pid)?
        .into_iter()
        .collect::<HashSet<_>>();
    let candidates = snapshot
        .windows
        .iter()
        .filter(|window| window.id != target_window_id)
        .filter(|window| window.space_id == target_space_id)
        .filter(|window| window.pid == Some(pid))
        .filter(|window| is_directional_focus_window(window))
        .filter(|window| window.bounds.is_some())
        .filter(|window| ax_window_ids.contains(&window.id))
        .collect::<Vec<_>>();

    api.debug(&format!(
        "macos_native: active-space stale-target remap target={} pid={} candidates={:?}",
        target_window_id,
        pid,
        candidates
            .iter()
            .map(|candidate| candidate.id)
            .collect::<Vec<_>>()
    ));

    Ok(candidates
        .into_iter()
        .max_by(|left, right| {
            compare_native_windows_for_target_match(target_bounds, left, right)
        })
        .map(|window| window.id))
}

fn split_view_same_space_focus_target_from_source(
    snapshot: &NativeDesktopSnapshot,
    source_window_id: u64,
    direction: NativeDirection,
) -> Option<u64> {
    let focused = native_window(snapshot, source_window_id)?;
    let focused_space = native_space(snapshot, focused.space_id)?;
    if focused_space.kind != SpaceKind::SplitView {
        return None;
    }

    let source_bounds = focused.bounds?;
    snapshot
        .windows
        .iter()
        .filter(|window| window.id != focused.id)
        .filter(|window| window.space_id == focused.space_id)
        .filter(|window| is_directional_focus_window(window))
        .filter(|window| {
            window.bounds.is_some_and(|bounds| {
                native_candidate_extends_in_direction(source_bounds, bounds, direction)
            })
        })
        .max_by(|left, right| compare_native_windows_for_edge(left, right, direction))
        .map(|window| window.id)
}

fn split_view_same_space_focus_target(
    snapshot: &NativeDesktopSnapshot,
    direction: NativeDirection,
) -> Option<u64> {
    let focused = resolved_focused_native_window(snapshot).ok()?;
    split_view_same_space_focus_target_from_source(snapshot, focused.id, direction)
}

fn focusable_same_app_split_view_peer_from_source<A: MacosNativeApi + ?Sized>(
    api: &A,
    snapshot: &NativeDesktopSnapshot,
    source_window_id: u64,
    direction: NativeDirection,
    target_window_id: u64,
) -> Result<Option<(u64, u32)>, MacosNativeOperationError> {
    let Some(focused) = native_window(snapshot, source_window_id) else {
        api.debug(&format!(
            "macos_native: split-view peer remap skipped; source window {source_window_id} missing from snapshot"
        ));
        return Ok(None);
    };
    let Some(source_bounds) = focused.bounds else {
        api.debug(&format!(
            "macos_native: split-view peer remap skipped; source window {source_window_id} has no bounds"
        ));
        return Ok(None);
    };
    let Some(target) = native_window(snapshot, target_window_id) else {
        api.debug(&format!(
            "macos_native: split-view peer remap skipped; target window {target_window_id} missing from snapshot"
        ));
        return Ok(None);
    };
    let Some(target_app_id) = target.app_id.as_deref() else {
        api.debug(&format!(
            "macos_native: split-view peer remap skipped; target window {target_window_id} has no app_id"
        ));
        return Ok(None);
    };
    let mut candidates = snapshot
        .windows
        .iter()
        .filter(|window| window.id != focused.id && window.id != target_window_id)
        .filter(|window| window.space_id == focused.space_id)
        .filter(|window| is_directional_focus_window(window))
        .filter(|window| window.app_id.as_deref() == Some(target_app_id))
        .filter(|window| window.pid.is_some())
        .filter(|window| {
            window.bounds.is_some_and(|bounds| {
                native_candidate_extends_in_direction(source_bounds, bounds, direction)
            })
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        compare_native_windows_for_edge(left, right, direction).reverse()
    });

    api.debug(&format!(
        "macos_native: split-view peer remap source={} target={} target_pid={:?} target_app_id={} candidates={:?}",
        source_window_id,
        target_window_id,
        target.pid,
        target_app_id,
        candidates
            .iter()
            .map(|candidate| (candidate.id, candidate.pid))
            .collect::<Vec<_>>()
    ));

    let fallback_candidate = candidates
        .first()
        .and_then(|candidate| candidate.pid.map(|pid| (candidate.id, pid)));

    let mut ax_window_ids_by_pid = HashMap::<u32, HashSet<u64>>::new();
    for candidate in candidates {
        let Some(pid) = candidate.pid else {
            continue;
        };
        let ax_window_ids = match ax_window_ids_by_pid.entry(pid) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => entry.insert(
                api.ax_window_ids_for_pid(pid)?
                    .into_iter()
                    .collect::<HashSet<_>>(),
            ),
        };
        api.debug(&format!(
            "macos_native: split-view peer remap candidate={} pid={} ax_window_ids={:?}",
            candidate.id, pid, ax_window_ids
        ));
        if ax_window_ids.contains(&candidate.id) {
            api.debug(&format!(
                "macos_native: split-view peer remap chose AX-backed candidate={} pid={}",
                candidate.id, pid
            ));
            return Ok(Some((candidate.id, pid)));
        }
    }

    if let Some((candidate_id, candidate_pid)) = fallback_candidate {
        api.debug(&format!(
            "macos_native: split-view peer remap falling back to directional candidate={} pid={} despite empty AX preflight",
            candidate_id, candidate_pid
        ));
    } else {
        api.debug(
            "macos_native: split-view peer remap found no same-app directional candidates",
        );
    }

    Ok(fallback_candidate)
}

fn focusable_same_app_split_view_peer<A: MacosNativeApi + ?Sized>(
    api: &A,
    snapshot: &NativeDesktopSnapshot,
    direction: NativeDirection,
    target_window_id: u64,
) -> Result<Option<(u64, u32)>, MacosNativeOperationError> {
    let Some(focused) = resolved_focused_native_window(snapshot).ok() else {
        return Ok(None);
    };
    focusable_same_app_split_view_peer_from_source(
        api,
        snapshot,
        focused.id,
        direction,
        target_window_id,
    )
}

fn refreshed_split_view_focus_target<A: MacosNativeApi + ?Sized>(
    api: &A,
    snapshot: &NativeDesktopSnapshot,
    direction: NativeDirection,
    target_window_id: u64,
    pid: u32,
) -> Result<Option<(u64, Option<u32>)>, MacosNativeOperationError> {
    let Some(original_focused) = resolved_focused_native_window(snapshot).ok() else {
        api.debug(
            "macos_native: refreshed split-view retarget skipped; no focused source window in planning snapshot",
        );
        return Ok(None);
    };
    let refreshed_snapshot = api.desktop_snapshot()?;
    let Some(refreshed_target_id) = split_view_same_space_focus_target_from_source(
        &refreshed_snapshot,
        original_focused.id,
        direction,
    ) else {
        api.debug(&format!(
            "macos_native: refreshed split-view retarget found no directional target from source {}",
            original_focused.id
        ));
        return Ok(None);
    };
    let refreshed_pid =
        native_window(&refreshed_snapshot, refreshed_target_id).and_then(|window| window.pid);
    api.debug(&format!(
        "macos_native: refreshed split-view retarget source={} stale_target={} stale_pid={} refreshed_target={} refreshed_pid={:?}",
        original_focused.id, target_window_id, pid, refreshed_target_id, refreshed_pid
    ));
    if refreshed_target_id == target_window_id && refreshed_pid == Some(pid) {
        if let Some((peer_target_id, peer_pid)) =
            focusable_same_app_split_view_peer_from_source(
                api,
                &refreshed_snapshot,
                original_focused.id,
                direction,
                refreshed_target_id,
            )?
        {
            api.debug(&format!(
                "macos_native: refreshed split-view retarget remapped stale target {} to peer {} pid={}",
                refreshed_target_id, peer_target_id, peer_pid
            ));
            return Ok(Some((peer_target_id, Some(peer_pid))));
        }
        api.debug(&format!(
            "macos_native: refreshed split-view retarget still stale after peer probing target={} pid={}",
            refreshed_target_id, pid
        ));
        return Ok(None);
    }
    Ok(Some((refreshed_target_id, refreshed_pid)))
}

fn focus_same_space_target_in_snapshot<A: MacosNativeApi + ?Sized>(
    api: &A,
    snapshot: &NativeDesktopSnapshot,
    direction: NativeDirection,
    target_window_id: u64,
) -> Result<(), MacosNativeOperationError> {
    let focus_target_id =
        split_view_same_space_focus_target(snapshot, direction).unwrap_or(target_window_id);
    let Some(pid) = native_window(snapshot, focus_target_id).and_then(|window| window.pid)
    else {
        return api.focus_window(focus_target_id);
    };

    focus_same_space_target_with_known_pid(api, snapshot, direction, focus_target_id, pid)
}

fn focus_same_space_target_with_known_pid<A: MacosNativeApi + ?Sized>(
    api: &A,
    snapshot: &NativeDesktopSnapshot,
    direction: NativeDirection,
    target_window_id: u64,
    pid: u32,
) -> Result<(), MacosNativeOperationError> {
    let focused = resolved_focused_native_window(snapshot)
        .ok()
        .filter(|focused| focused.pid == Some(pid));
    let same_pid_split_view = focused
        .and_then(|focused| native_space(snapshot, focused.space_id))
        .is_some_and(|space| space.kind == SpaceKind::SplitView);
    let mut ax_window_ids = None;
    let mut focus_target_id = target_window_id;

    api.debug(&format!(
        "macos_native: split-view focus target preflight target={} pid={} same_pid_split_view={} focused_same_pid={:?}",
        target_window_id,
        pid,
        same_pid_split_view,
        focused.as_ref().map(|window| window.id)
    ));

    if same_pid_split_view {
        let ids = api
            .ax_window_ids_for_pid(pid)?
            .into_iter()
            .collect::<HashSet<_>>();
        if !ids.contains(&target_window_id) {
            if let Some(remapped_target_id) =
                native_ax_backed_same_pid_target(snapshot, direction, pid, &ids)
                    .filter(|candidate| *candidate != target_window_id)
            {
                api.debug(&format!(
                    "macos_native: split-view focus remapped same-pid stale target {} to {}",
                    target_window_id, remapped_target_id
                ));
                focus_target_id = remapped_target_id;
            }
        }
        ax_window_ids = Some(ids);
    }

    match api.focus_window_with_known_pid(focus_target_id, pid) {
        Err(MacosNativeOperationError::MissingWindow(missing_window_id))
            if missing_window_id == focus_target_id =>
        {
            if same_pid_split_view {
                let ax_window_ids = match ax_window_ids {
                    Some(ids) => ids,
                    None => api
                        .ax_window_ids_for_pid(pid)?
                        .into_iter()
                        .collect::<HashSet<_>>(),
                };
                if let Some(remapped_target_id) =
                    native_ax_backed_same_pid_target(snapshot, direction, pid, &ax_window_ids)
                        .filter(|candidate| *candidate != focus_target_id)
                {
                    api.debug(&format!(
                        "macos_native: split-view focus retry remapped same-pid stale target {} to {}",
                        focus_target_id, remapped_target_id
                    ));
                    return api.focus_window_with_known_pid(remapped_target_id, pid);
                }
            }

            if let Some((remapped_target_id, remapped_pid)) =
                focusable_same_app_split_view_peer(api, snapshot, direction, focus_target_id)?
            {
                api.debug(&format!(
                    "macos_native: split-view focus remapped stale target {} to same-app peer {} pid={}",
                    focus_target_id, remapped_target_id, remapped_pid
                ));
                return api.focus_window_with_known_pid(remapped_target_id, remapped_pid);
            }

            if let Some((refreshed_target_id, refreshed_pid)) =
                refreshed_split_view_focus_target(
                    api,
                    snapshot,
                    direction,
                    focus_target_id,
                    pid,
                )?
            {
                if let Some(refreshed_pid) = refreshed_pid {
                    api.debug(&format!(
                        "macos_native: split-view focus retrying with refreshed target {} pid={}",
                        refreshed_target_id, refreshed_pid
                    ));
                    return api.focus_window_with_known_pid(refreshed_target_id, refreshed_pid);
                }
                api.debug(&format!(
                    "macos_native: split-view focus retrying with refreshed target {} via generic focus",
                    refreshed_target_id
                ));
                return api.focus_window(refreshed_target_id);
            }

            if !same_pid_split_view {
                api.debug(&format!(
                    "macos_native: split-view focus falling back to generic focus for stale target {}",
                    focus_target_id
                ));
                return api.focus_window(focus_target_id);
            }

            Err(MacosNativeOperationError::MissingWindow(focus_target_id))
        }
        other => other,
    }
}

pub struct RealNativeApi {
    skylight: Option<DylibHandle>,
    hiservices: Option<DylibHandle>,
    options: NativeBackendOptions,
}

impl RealNativeApi {
    pub fn new(options: NativeBackendOptions) -> Self {
        Self {
            skylight: DylibHandle::open(SKYLIGHT_FRAMEWORK_PATH),
            hiservices: DylibHandle::open(HISERVICES_FRAMEWORK_PATH),
            options,
        }
    }

    fn resolve_symbol(&self, symbol: &'static str) -> Option<*mut c_void> {
        let symbol =
            CString::new(symbol).expect("required symbol names should not contain NULs");

        self.skylight
            .as_ref()
            .and_then(|handle| handle.resolve(symbol.as_c_str()))
            .or_else(|| {
                self.hiservices
                    .as_ref()
                    .and_then(|handle| handle.resolve(symbol.as_c_str()))
            })
    }

    fn debug(&self, message: impl AsRef<str>) {
        if let Some(diagnostics) = self.options.diagnostics.as_ref() {
            diagnostics.debug(message.as_ref());
        }
    }
}

impl MacosNativeApi for RealNativeApi {
    fn has_symbol(&self, symbol: &'static str) -> bool {
        self.resolve_symbol(symbol).is_some()
    }

    fn debug(&self, message: &str) {
        RealNativeApi::debug(self, message);
    }

    fn ax_is_trusted(&self) -> bool {
        ax::is_process_trusted(self)
    }

    fn minimal_topology_ready(&self) -> bool {
        let Some(symbol) = self.resolve_symbol("SLSMainConnectionID") else {
            return false;
        };

        let main_connection_id: SlsMainConnectionIdFn = unsafe { std::mem::transmute(symbol) };
        unsafe { main_connection_id() != 0 }
    }

    fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
        Ok(native_desktop_snapshot_from_topology(
            &self.topology_snapshot()?,
        ))
    }

    fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
        let payload = skylight::copy_managed_display_spaces_raw(self)?;
        parse_managed_spaces(payload.as_type_ref() as CFArrayRef)
    }

    fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
        let payload = skylight::copy_managed_display_spaces_raw(self)?;
        let display_identifiers =
            parse_display_identifiers(payload.as_type_ref() as CFArrayRef)?;
        let active_space_ids = display_identifiers
            .into_iter()
            .map(|display_identifier| {
                skylight::current_space_for_display(self, &display_identifier)
            })
            .collect::<Result<HashSet<_>, _>>()?;

        (!active_space_ids.is_empty())
            .then_some(active_space_ids)
            .ok_or(MacosNativeProbeError::MissingTopology(
                "SLSManagedDisplayGetCurrentSpace",
            ))
    }

    fn active_space_windows(
        &self,
        space_id: u64,
    ) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
        let payload = skylight::copy_windows_for_space_raw(self, space_id)?;
        let visible_order = query_visible_window_order(&parse_window_ids(
            payload.as_type_ref() as CFArrayRef,
        )?)?;
        let descriptions = window_server::copy_window_descriptions_raw(
            self,
            payload.as_type_ref() as CFArrayRef,
        )?;

        assemble_real_active_space_windows(
            descriptions.as_type_ref() as CFArrayRef,
            &visible_order,
        )
    }

    fn inactive_space_window_ids(
        &self,
    ) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
        let spaces = self.managed_spaces()?;
        let active_space_ids = self.active_space_ids()?;
        let mut inactive_space_window_ids = HashMap::new();

        for space in spaces {
            if active_space_ids.contains(&space.managed_space_id) {
                continue;
            }

            let payload = skylight::copy_windows_for_space_raw(self, space.managed_space_id)?;
            inactive_space_window_ids.insert(
                space.managed_space_id,
                parse_window_ids(payload.as_type_ref() as CFArrayRef)?,
            );
        }

        Ok(inactive_space_window_ids)
    }

    fn onscreen_window_ids(&self) -> Result<HashSet<NativeWindowId>, MacosNativeProbeError> {
        let descriptions = copy_onscreen_window_descriptions_raw()?;
        onscreen_window_ids_from_descriptions(descriptions.as_type_ref() as CFArrayRef)
    }

    fn switch_space(&self, space_id: u64) -> Result<(), MacosNativeOperationError> {
        skylight::switch_space(self, space_id)
    }

    fn switch_adjacent_space(
        &self,
        direction: NativeDirection,
        _space_id: u64,
    ) -> Result<(), MacosNativeOperationError> {
        self.debug(&format!(
            "macos_native: switching adjacent space via mission-control hotkey direction={direction}"
        ));
        switch_adjacent_space_via_hotkey(
            &self.options,
            direction,
            |key_code, key_down, flags| {
                window_server::post_keyboard_event(self, key_code, key_down, flags)
            },
        )
    }

    fn focus_window(&self, window_id: u64) -> Result<(), MacosNativeOperationError> {
        window_server::focus_window(self, window_id)
    }

    fn focus_window_with_known_pid(
        &self,
        window_id: u64,
        pid: u32,
    ) -> Result<(), MacosNativeOperationError> {
        match focus_window_via_process_and_raise(
            window_id,
            |_| Ok(pid),
            |resolved_pid| window_server::process_serial_number_for_pid(self, resolved_pid),
            |psn, target_window_id| {
                window_server::front_process_window(self, psn, target_window_id)
            },
            |psn, target_window_id| window_server::make_key_window(self, psn, target_window_id),
            |target_window_id, resolved_pid| {
                ax::raise_window_via_ax(self, target_window_id, resolved_pid)
            },
        ) {
            Err(MacosNativeOperationError::MissingWindow(missing_window_id))
                if missing_window_id == window_id =>
            {
                let deadline = Instant::now() + AX_RAISE_SETTLE_TIMEOUT;
                loop {
                    if self.focused_window_id().ok() == Some(Some(window_id)) {
                        self.debug(&format!(
                            "macos_native: treating missing AX raise target {window_id} as success after focus confirmation"
                        ));
                        return Ok(());
                    }
                    if Instant::now() >= deadline {
                        break;
                    }
                    std::thread::sleep(AX_RAISE_RETRY_INTERVAL);
                }
                self.debug(&format!(
                    "macos_native: AX raise still missing target {window_id} after retries; focused_window_id={:?}",
                    self.focused_window_id().ok().flatten()
                ));
                Err(MacosNativeOperationError::MissingWindow(window_id))
            }
            other => other,
        }
    }

    fn ax_window_ids_for_pid(&self, pid: u32) -> Result<Vec<u64>, MacosNativeOperationError> {
        ax::ax_window_ids_for_pid(self, pid)
    }

    fn focus_window_in_active_space_with_known_pid(
        &self,
        window_id: u64,
        pid: u32,
        target_hint: Option<ActiveSpaceFocusTargetHint>,
    ) -> Result<(), MacosNativeOperationError> {
        match focus_window_via_make_key_and_raise(
            window_id,
            |_| Ok(pid),
            |resolved_pid| window_server::process_serial_number_for_pid(self, resolved_pid),
            |psn, target_window_id| window_server::make_key_window(self, psn, target_window_id),
            |target_window_id, resolved_pid| {
                ax::raise_window_via_ax(self, target_window_id, resolved_pid)
            },
        ) {
            Err(MacosNativeOperationError::MissingWindow(missing_window_id))
                if missing_window_id == window_id =>
            {
                let deadline = Instant::now() + AX_RAISE_SETTLE_TIMEOUT;
                loop {
                    if self.focused_window_id().ok() == Some(Some(window_id)) {
                        self.debug(&format!(
                            "macos_native: treating missing active-space AX raise target {window_id} as success after focus confirmation"
                        ));
                        return Ok(());
                    }
                    if Instant::now() >= deadline {
                        break;
                    }
                    std::thread::sleep(AX_RAISE_RETRY_INTERVAL);
                }
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
                self.debug(&format!(
                    "macos_native: active-space AX raise still missing target {window_id} after retries; focused_window_id={:?}",
                    self.focused_window_id().ok().flatten()
                ));
                Err(MacosNativeOperationError::MissingWindow(window_id))
            }
            other => other,
        }
    }

    fn move_window_to_space(
        &self,
        window_id: u64,
        space_id: u64,
    ) -> Result<(), MacosNativeOperationError> {
        skylight::move_window_to_space(self, window_id, space_id)
    }

    fn swap_window_frames(
        &self,
        source_window_id: u64,
        source_frame: NativeBounds,
        target_window_id: u64,
        target_frame: NativeBounds,
    ) -> Result<(), MacosNativeOperationError> {
        ax::swap_window_frames(
            self,
            source_window_id,
            source_frame,
            target_window_id,
            target_frame,
        )
    }

    fn focused_window_id(&self) -> Result<Option<NativeWindowId>, MacosNativeProbeError> {
        ax::probe_focused_window_id(self)
    }

    fn focused_window_snapshot(&self) -> Result<WindowSnapshot, MacosNativeProbeError> {
        let focused_window_id = ax::probe_focused_window_id(self)?;
        let active_space_ids = self.active_space_ids()?;
        let mut active_space_windows = HashMap::new();

        for space_id in active_space_ids {
            let windows = window_server::active_space_windows_without_app_ids(self, space_id)?;
            if let Some(target_window_id) = focused_window_id {
                if let Some(mut snapshot) =
                    active_window_snapshot(space_id, &windows, target_window_id)
                {
                    snapshot.app_id = snapshot
                        .app_id
                        .or_else(|| stable_app_id_from_real_window(snapshot.pid, None));
                    return Ok(snapshot);
                }
            }
            active_space_windows.insert(space_id, windows);
        }

        let mut snapshot =
            focused_window_from_active_space_windows(&active_space_windows, focused_window_id)?;
        snapshot.app_id = snapshot
            .app_id
            .or_else(|| stable_app_id_from_real_window(snapshot.pid, None));
        Ok(snapshot)
    }

    fn topology_snapshot(&self) -> Result<RawTopologySnapshot, MacosNativeProbeError> {
        let mut topology = self.topology_snapshot_without_focus()?;
        topology.focused_window_id = self.focused_window_id()?;
        Ok(topology)
    }

    fn topology_snapshot_without_focus(
        &self,
    ) -> Result<RawTopologySnapshot, MacosNativeProbeError> {
        let payload = skylight::copy_managed_display_spaces_raw(self)?;
        let payload = payload.as_type_ref() as CFArrayRef;
        let spaces = parse_managed_spaces(payload)?;
        let active_space_ids = parse_active_space_ids(payload)?;
        let mut active_space_windows = HashMap::new();
        let mut inactive_space_window_ids = HashMap::new();

        for space in &spaces {
            let payload = skylight::copy_windows_for_space_raw(self, space.managed_space_id)?;
            let raw_window_ids = parse_window_ids(payload.as_type_ref() as CFArrayRef)?;

            if active_space_ids.contains(&space.managed_space_id) {
                let visible_order = query_visible_window_order(&raw_window_ids)?;
                let descriptions = window_server::copy_window_descriptions_raw(
                    self,
                    payload.as_type_ref() as CFArrayRef,
                )?;
                let windows = assemble_real_active_space_windows(
                    descriptions.as_type_ref() as CFArrayRef,
                    &visible_order,
                )?;

                active_space_windows.insert(space.managed_space_id, windows);
            } else {
                inactive_space_window_ids.insert(space.managed_space_id, raw_window_ids);
            }
        }

        Ok(RawTopologySnapshot {
            spaces,
            active_space_ids,
            active_space_windows,
            inactive_space_window_ids,
            focused_window_id: None,
        })
    }
}

pub(crate) fn focus_window_via_process_and_raise<
    WindowPid,
    ProcessSerial,
    FrontProcessWindow,
    MakeKeyWindow,
    RaiseWindow,
>(
    window_id: u64,
    mut window_pid: WindowPid,
    mut process_serial_number: ProcessSerial,
    mut front_process_window: FrontProcessWindow,
    mut make_key_window: MakeKeyWindow,
    mut raise_window: RaiseWindow,
) -> Result<(), MacosNativeOperationError>
where
    WindowPid: FnMut(u64) -> Result<u32, MacosNativeOperationError>,
    ProcessSerial: FnMut(u32) -> Result<ProcessSerialNumber, MacosNativeOperationError>,
    FrontProcessWindow:
        FnMut(&ProcessSerialNumber, u64) -> Result<(), MacosNativeOperationError>,
    MakeKeyWindow: FnMut(&ProcessSerialNumber, u64) -> Result<(), MacosNativeOperationError>,
    RaiseWindow: FnMut(u64, u32) -> Result<(), MacosNativeOperationError>,
{
    let pid = window_pid(window_id)?;
    let psn = process_serial_number(pid)?;
    front_process_window(&psn, window_id)?;
    make_key_window(&psn, window_id)?;
    let deadline = Instant::now() + AX_RAISE_SETTLE_TIMEOUT;
    loop {
        match raise_window(window_id, pid) {
            Err(MacosNativeOperationError::MissingWindow(missing_window_id))
                if missing_window_id == window_id && Instant::now() < deadline =>
            {
                std::thread::sleep(AX_RAISE_RETRY_INTERVAL);
            }
            result => return result,
        }
    }
}

pub(crate) fn focus_window_via_make_key_and_raise<
    WindowPid,
    ProcessSerial,
    MakeKeyWindow,
    RaiseWindow,
>(
    window_id: u64,
    mut window_pid: WindowPid,
    mut process_serial_number: ProcessSerial,
    mut make_key_window: MakeKeyWindow,
    mut raise_window: RaiseWindow,
) -> Result<(), MacosNativeOperationError>
where
    WindowPid: FnMut(u64) -> Result<u32, MacosNativeOperationError>,
    ProcessSerial: FnMut(u32) -> Result<ProcessSerialNumber, MacosNativeOperationError>,
    MakeKeyWindow: FnMut(&ProcessSerialNumber, u64) -> Result<(), MacosNativeOperationError>,
    RaiseWindow: FnMut(u64, u32) -> Result<(), MacosNativeOperationError>,
{
    let pid = window_pid(window_id)?;
    let psn = process_serial_number(pid)?;
    make_key_window(&psn, window_id)?;
    let deadline = Instant::now() + AX_RAISE_SETTLE_TIMEOUT;
    loop {
        match raise_window(window_id, pid) {
            Err(MacosNativeOperationError::MissingWindow(missing_window_id))
                if missing_window_id == window_id && Instant::now() < deadline =>
            {
                std::thread::sleep(AX_RAISE_RETRY_INTERVAL);
            }
            result => return result,
        }
    }
}

use desktop_topology_snapshot::*;
use foundation::*;
use skylight::*;
use window_server::*;

pub use desktop_topology_snapshot::SpaceKind;
pub use desktop_topology_snapshot::{
    RawSpaceRecord, RawTopologySnapshot, RawWindow, WindowSnapshot,
};
pub use error::{
    MacosNativeConnectError, MacosNativeOperationError, MacosNativeProbeError,
};

#[cfg(test)]
#[allow(dead_code)]
pub(crate) mod tests {
    #[allow(unused_imports)]
    pub(crate) use super::desktop_topology_snapshot::tests::{
        SpaceSnapshot, space_snapshots_from_topology,
    };
    #[allow(unused_imports)]
    pub(crate) use super::foundation::tests::dictionary_from_type_refs;
    use super::*;

    pub(crate) fn focused_window_id_via_ax<
        App,
        Window,
        FocusedApplication,
        FocusedWindow,
        WindowId,
    >(
        focused_application: FocusedApplication,
        focused_window: FocusedWindow,
        window_id: WindowId,
    ) -> Result<Option<u64>, MacosNativeProbeError>
    where
        FocusedApplication: FnMut() -> Result<Option<App>, MacosNativeProbeError>,
        FocusedWindow: FnMut(&App) -> Result<Option<Window>, MacosNativeProbeError>,
        WindowId: FnMut(&Window) -> Result<u64, MacosNativeProbeError>,
    {
        ax::focused_window_id(focused_application, focused_window, window_id)
    }
}
