#![allow(dead_code)]

use crate::{
    ActiveSpaceFocusTargetHint, MacosNativeOperationError, MacosNativeProbeError,
    NativeBounds, NativeDesktopSnapshot, NativeDirection, NativeSpaceSnapshot,
    NativeWindowSnapshot,
};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceKind {
    Desktop,
    Fullscreen,
    SplitView,
    System,
    StageManagerOpaque,
}

pub(crate) const DESKTOP_SPACE_TYPE: i32 = 0;
pub(crate) const FULLSCREEN_SPACE_TYPE: i32 = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawSpaceRecord {
    pub managed_space_id: u64,
    pub display_index: usize,
    pub space_type: i32,
    pub tile_spaces: Vec<u64>,
    pub has_tile_layout_manager: bool,
    pub stage_manager_managed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowSnapshot {
    pub id: u64,
    pub pid: Option<u32>,
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub space_id: u64,
    pub order_index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawWindow {
    pub id: u64,
    pub pid: Option<u32>,
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub level: i32,
    pub visible_index: Option<usize>,
    pub frame: Option<NativeBounds>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawTopologySnapshot {
    pub spaces: Vec<RawSpaceRecord>,
    pub active_space_ids: HashSet<u64>,
    pub active_space_windows: HashMap<u64, Vec<RawWindow>>,
    pub inactive_space_window_ids: HashMap<u64, Vec<u64>>,
    pub focused_window_id: Option<u64>,
}

pub(crate) fn classify_space(raw_space: &RawSpaceRecord) -> SpaceKind {
    if raw_space.stage_manager_managed {
        SpaceKind::StageManagerOpaque
    } else if raw_space.has_tile_layout_manager || !raw_space.tile_spaces.is_empty() {
        SpaceKind::SplitView
    } else if raw_space.space_type == FULLSCREEN_SPACE_TYPE {
        SpaceKind::Fullscreen
    } else if raw_space.space_type == DESKTOP_SPACE_TYPE {
        SpaceKind::Desktop
    } else {
        SpaceKind::System
    }
}

pub(crate) fn stable_app_id_from_real_window(
    pid: Option<u32>,
    _owner_name: Option<&str>,
) -> Option<String> {
    pid.and_then(stable_app_id_from_pid)
}

pub(crate) fn enrich_real_window_app_ids(windows: Vec<RawWindow>) -> Vec<RawWindow> {
    enrich_real_window_app_ids_with(windows, stable_app_id_from_pid)
}

pub(crate) fn enrich_real_window_app_ids_with<F>(
    windows: Vec<RawWindow>,
    mut resolve_app_id: F,
) -> Vec<RawWindow>
where
    F: FnMut(u32) -> Option<String>,
{
    let mut app_ids_by_pid = HashMap::<u32, Option<String>>::new();
    windows
        .into_iter()
        .map(|mut window| {
            if window.app_id.is_none() {
                window.app_id = window.pid.and_then(|pid| {
                    app_ids_by_pid
                        .entry(pid)
                        .or_insert_with(|| resolve_app_id(pid))
                        .clone()
                });
            }
            window
        })
        .collect()
}

pub(crate) fn stable_app_id_from_pid(pid: u32) -> Option<String> {
    let _span = tracing::debug_span!("macos_native.app_id_from_pid", pid).entered();
    let lsappinfo_output = lsappinfo_bundle_identifier_output(pid)?;
    parse_lsappinfo_bundle_identifier(&lsappinfo_output)
}

fn lsappinfo_bundle_identifier_output(pid: u32) -> Option<String> {
    let _span =
        tracing::debug_span!("macos_native.app_id_from_pid.lsappinfo", pid).entered();
    let application_specifier = format!("#{pid}");
    let output = std::process::Command::new("lsappinfo")
        .args(["info", "-only", "bundleid", application_specifier.as_str()])
        .output()
        .ok()?;

    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(crate) fn parse_lsappinfo_bundle_identifier(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        line.strip_prefix("\"CFBundleIdentifier\"=")
            .and_then(|value| {
                let bundle_identifier = value.trim().trim_matches('"');
                (!bundle_identifier.is_empty()).then(|| bundle_identifier.to_string())
            })
    })
}

pub(crate) fn compare_active_windows(
    left: &RawWindow,
    right: &RawWindow,
) -> std::cmp::Ordering {
    match (left.visible_index, right.visible_index) {
        (Some(left_index), Some(right_index)) => left_index.cmp(&right_index),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
    .then_with(|| right.level.cmp(&left.level))
    .then_with(|| left.id.cmp(&right.id))
}

pub(crate) fn order_active_space_windows(windows: &[RawWindow]) -> Vec<RawWindow> {
    let mut ordered = windows.to_vec();
    ordered.sort_by(compare_active_windows);
    ordered
}

fn snapshots_for_active_space(space_id: u64, windows: &[RawWindow]) -> Vec<WindowSnapshot> {
    order_active_space_windows(windows)
        .into_iter()
        .enumerate()
        .map(|(index, window)| WindowSnapshot {
            id: window.id,
            pid: window.pid,
            app_id: window.app_id,
            title: window.title,
            space_id,
            order_index: Some(index),
        })
        .collect()
}

pub(crate) fn active_window_snapshot(
    space_id: u64,
    windows: &[RawWindow],
    window_id: u64,
) -> Option<WindowSnapshot> {
    order_active_space_windows(windows)
        .into_iter()
        .enumerate()
        .find_map(|(index, window)| {
            (window.id == window_id).then_some(WindowSnapshot {
                id: window.id,
                pid: window.pid,
                app_id: window.app_id,
                title: window.title,
                space_id,
                order_index: Some(index),
            })
        })
}

pub(crate) fn snapshots_for_inactive_space(
    space_id: u64,
    window_ids: &[u64],
) -> Vec<WindowSnapshot> {
    window_ids
        .iter()
        .map(|id| WindowSnapshot {
            id: *id,
            pid: None,
            app_id: None,
            title: None,
            space_id,
            order_index: None,
        })
        .collect()
}

#[allow(dead_code)]
pub(crate) fn native_desktop_snapshot_from_topology(
    topology: &RawTopologySnapshot,
) -> NativeDesktopSnapshot {
    let spaces = topology
        .spaces
        .iter()
        .map(|space| NativeSpaceSnapshot {
            id: space.managed_space_id,
            display_index: space.display_index,
            active: topology.active_space_ids.contains(&space.managed_space_id),
            kind: classify_space(space),
        })
        .collect();
    let mut windows = Vec::new();

    for space in &topology.spaces {
        if topology.active_space_ids.contains(&space.managed_space_id) {
            windows.extend(
                order_active_space_windows(
                    topology
                        .active_space_windows
                        .get(&space.managed_space_id)
                        .map(Vec::as_slice)
                        .unwrap_or(&[]),
                )
                .into_iter()
                .enumerate()
                .map(|(index, window)| NativeWindowSnapshot {
                    id: window.id,
                    pid: window.pid,
                    app_id: window.app_id,
                    title: window.title,
                    bounds: window.frame,
                    level: window.level,
                    space_id: space.managed_space_id,
                    order_index: Some(index),
                }),
            );
        } else {
            windows.extend(
                topology
                    .inactive_space_window_ids
                    .get(&space.managed_space_id)
                    .map(Vec::as_slice)
                    .unwrap_or(&[])
                    .iter()
                    .copied()
                    .map(|window_id| NativeWindowSnapshot {
                        id: window_id,
                        pid: None,
                        app_id: None,
                        title: None,
                        bounds: None,
                        level: 0,
                        space_id: space.managed_space_id,
                        order_index: None,
                    }),
            );
        }
    }

    NativeDesktopSnapshot {
        spaces,
        active_space_ids: topology.active_space_ids.clone(),
        windows,
        focused_window_id: topology.focused_window_id,
    }
}

pub(crate) fn window_snapshots_from_topology(
    topology: &RawTopologySnapshot,
) -> Vec<WindowSnapshot> {
    let mut snapshots = Vec::new();

    for space in &topology.spaces {
        if topology.active_space_ids.contains(&space.managed_space_id) {
            snapshots.extend(snapshots_for_active_space(
                space.managed_space_id,
                topology
                    .active_space_windows
                    .get(&space.managed_space_id)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]),
            ));
        } else {
            let window_ids = topology
                .inactive_space_window_ids
                .get(&space.managed_space_id)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            snapshots.extend(snapshots_for_inactive_space(
                space.managed_space_id,
                window_ids,
            ));
        }
    }

    snapshots
}

pub(crate) fn focused_window_from_active_space_windows(
    active_space_windows: &HashMap<u64, Vec<RawWindow>>,
    focused_window_id: Option<u64>,
) -> Result<WindowSnapshot, MacosNativeProbeError> {
    if let Some(target_window_id) = focused_window_id {
        if let Some(snapshot) =
            active_space_windows.iter().find_map(|(space_id, windows)| {
                active_window_snapshot(*space_id, windows, target_window_id)
            })
        {
            return Ok(snapshot);
        }
    }

    active_space_windows
        .iter()
        .flat_map(|(space_id, windows)| {
            windows
                .iter()
                .cloned()
                .map(move |window| (*space_id, window))
        })
        .min_by(|(_, left), (_, right)| compare_active_windows(left, right))
        .and_then(|(space_id, window)| {
            active_window_snapshot(
                space_id,
                active_space_windows.get(&space_id)?,
                window.id,
            )
        })
        .ok_or(MacosNativeProbeError::MissingFocusedWindow)
}

pub(crate) fn space_id_for_window(
    topology: &RawTopologySnapshot,
    window_id: u64,
) -> Option<u64> {
    topology
        .active_space_windows
        .iter()
        .find_map(|(space_id, windows)| {
            windows
                .iter()
                .any(|window| window.id == window_id)
                .then_some(*space_id)
        })
        .or_else(|| {
            topology
                .inactive_space_window_ids
                .iter()
                .find_map(|(space_id, windows)| {
                    windows.contains(&window_id).then_some(*space_id)
                })
        })
}

pub(crate) fn display_index_for_space(
    topology: &RawTopologySnapshot,
    space_id: u64,
) -> Option<usize> {
    topology
        .spaces
        .iter()
        .find(|space| space.managed_space_id == space_id)
        .map(|space| space.display_index)
}

pub(crate) fn active_space_on_display(
    topology: &RawTopologySnapshot,
    display_index: usize,
) -> Option<u64> {
    topology.active_space_ids.iter().copied().find(|space_id| {
        display_index_for_space(topology, *space_id) == Some(display_index)
    })
}

pub(crate) fn window_ids_for_space(
    topology: &RawTopologySnapshot,
    space_id: u64,
) -> HashSet<u64> {
    if topology.active_space_ids.contains(&space_id) {
        return topology
            .active_space_windows
            .get(&space_id)
            .into_iter()
            .flat_map(|windows| windows.iter().map(|window| window.id))
            .collect();
    }

    topology
        .inactive_space_window_ids
        .get(&space_id)
        .into_iter()
        .flat_map(|window_ids| window_ids.iter().copied())
        .collect()
}

pub(crate) fn best_window_id_from_windows(
    direction: NativeDirection,
    windows: &[RawWindow],
) -> Option<u64> {
    let focusable_windows = windows
        .iter()
        .filter(|window| is_directional_focus_window(window))
        .cloned()
        .collect::<Vec<_>>();
    edge_window_id_in_direction(&focusable_windows, direction).or_else(|| {
        focusable_windows
            .iter()
            .min_by(|left, right| compare_active_windows(left, right))
            .map(|window| window.id)
    })
}

pub(crate) fn is_directional_focus_window(window: &RawWindow) -> bool {
    window.level == 0
}

pub(crate) fn edge_window_id_in_direction(
    windows: &[RawWindow],
    direction: NativeDirection,
) -> Option<u64> {
    windows
        .iter()
        .filter(|window| window.frame.is_some())
        .max_by(|left, right| compare_windows_for_edge(left, right, direction))
        .map(|window| window.id)
}

pub(crate) fn compare_windows_for_edge(
    left: &RawWindow,
    right: &RawWindow,
    direction: NativeDirection,
) -> std::cmp::Ordering {
    let left_frame = left.frame.expect("frame should be present");
    let right_frame = right.frame.expect("frame should be present");

    match direction {
        NativeDirection::East => {
            (left_frame.x + left_frame.width).cmp(&(right_frame.x + right_frame.width))
        }
        NativeDirection::West => right_frame.x.cmp(&left_frame.x),
        NativeDirection::North => right_frame.y.cmp(&left_frame.y),
        NativeDirection::South => {
            (left_frame.y + left_frame.height).cmp(&(right_frame.y + right_frame.height))
        }
    }
    .then_with(|| compare_active_windows(right, left))
}

pub(crate) fn space_transition_window_ids(
    topology: &RawTopologySnapshot,
    target_space_id: u64,
) -> (Option<u64>, HashSet<u64>) {
    let source_space_id = display_index_for_space(topology, target_space_id)
        .and_then(|display_index| active_space_on_display(topology, display_index))
        .filter(|source_space_id| *source_space_id != target_space_id);
    let source_focus_window_id = topology
        .focused_window_id
        .filter(|window_id| space_id_for_window(topology, *window_id) == source_space_id);
    let target_window_ids = window_ids_for_space(topology, target_space_id);

    (source_focus_window_id, target_window_ids)
}

pub(crate) fn ensure_supported_target_space(
    topology: &RawTopologySnapshot,
    space_id: u64,
) -> Result<(), MacosNativeOperationError> {
    let Some(space) = topology
        .spaces
        .iter()
        .find(|space| space.managed_space_id == space_id)
    else {
        return Err(MacosNativeOperationError::MissingSpace(space_id));
    };

    (classify_space(space) != SpaceKind::StageManagerOpaque)
        .then_some(())
        .ok_or(MacosNativeOperationError::UnsupportedStageManagerSpace(
            space_id,
        ))
}

pub(crate) fn active_window_pid_from_topology(
    topology: &RawTopologySnapshot,
    window_id: u64,
) -> Option<u32> {
    topology
        .active_space_windows
        .values()
        .flat_map(|windows| windows.iter())
        .find(|window| window.id == window_id)
        .and_then(|window| window.pid)
}

pub(crate) fn active_space_focus_target_hint_from_topology(
    topology: &RawTopologySnapshot,
    window_id: u64,
) -> Option<ActiveSpaceFocusTargetHint> {
    let space_id = space_id_for_window(topology, window_id)?;
    let bounds = topology
        .active_space_windows
        .get(&space_id)?
        .iter()
        .find(|window| window.id == window_id)?
        .frame?;
    Some(ActiveSpaceFocusTargetHint { space_id, bounds })
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct SpaceSnapshot {
        pub(crate) id: u64,
        pub(crate) kind: SpaceKind,
        pub(crate) is_active: bool,
        pub(crate) ordered_window_ids: Option<Vec<u64>>,
    }

    pub(crate) fn space_snapshots_from_topology(
        topology: &RawTopologySnapshot,
    ) -> Vec<SpaceSnapshot> {
        topology
            .spaces
            .iter()
            .map(|space| {
                let is_active = topology.active_space_ids.contains(&space.managed_space_id);
                let ordered_window_ids = is_active.then(|| {
                    snapshots_for_active_space(
                        space.managed_space_id,
                        topology
                            .active_space_windows
                            .get(&space.managed_space_id)
                            .map(Vec::as_slice)
                            .unwrap_or(&[]),
                    )
                    .into_iter()
                    .map(|window| window.id)
                    .collect::<Vec<_>>()
                });

                SpaceSnapshot {
                    id: space.managed_space_id,
                    kind: classify_space(space),
                    is_active,
                    ordered_window_ids,
                }
            })
            .collect()
    }
}

pub(crate) fn focused_window_from_topology(
    topology: &RawTopologySnapshot,
) -> Result<WindowSnapshot, MacosNativeProbeError> {
    focused_window_from_active_space_windows(
        &topology.active_space_windows,
        topology.focused_window_id,
    )
}
