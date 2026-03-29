mod api;
#[cfg(target_os = "macos")]
mod ax;
mod desktop_topology_snapshot;
mod environment;
mod error;
mod ffi;
#[cfg(target_os = "macos")]
mod foundation;
mod navigation;
mod real_api;
mod shim;
#[cfg(target_os = "macos")]
mod skylight;
mod transport;
#[cfg(target_os = "macos")]
mod window_server;

#[cfg(target_os = "macos")]
use std::{
    collections::{HashMap, HashSet},
    time::Instant,
};

#[cfg(not(target_os = "macos"))]
const REQUIRED_PRIVATE_SYMBOLS: &[&str] = &[];
#[cfg(not(target_os = "macos"))]
const SPACE_SWITCH_SETTLE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(300);
#[cfg(not(target_os = "macos"))]
const SPACE_SWITCH_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(10);
#[cfg(not(target_os = "macos"))]
const SPACE_SWITCH_STABLE_TARGET_POLLS: usize = 3;
#[cfg(not(target_os = "macos"))]
const UNSUPPORTED_PLATFORM_MESSAGE: &str = "macos_window_manager requires macOS";

fn native_window(
    snapshot: &NativeDesktopSnapshot,
    window_id: u64,
) -> Option<&NativeWindowSnapshot> {
    snapshot
        .windows
        .iter()
        .find(|window| window.id == window_id)
}

fn native_space(snapshot: &NativeDesktopSnapshot, space_id: u64) -> Option<&NativeSpaceSnapshot> {
    snapshot.spaces.iter().find(|space| space.id == space_id)
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
        .max_by(|left, right| compare_native_windows_for_target_match(target_bounds, left, right))
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
    candidates
        .sort_by(|left, right| compare_native_windows_for_edge(left, right, direction).reverse());

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
        api.debug("macos_native: split-view peer remap found no same-app directional candidates");
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
        if let Some((peer_target_id, peer_pid)) = focusable_same_app_split_view_peer_from_source(
            api,
            &refreshed_snapshot,
            original_focused.id,
            direction,
            refreshed_target_id,
        )? {
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
    let Some(pid) = native_window(snapshot, focus_target_id).and_then(|window| window.pid) else {
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
                refreshed_split_view_focus_target(api, snapshot, direction, focus_target_id, pid)?
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

#[cfg(target_os = "macos")]
#[cfg(target_os = "macos")]
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
    FrontProcessWindow: FnMut(&ProcessSerialNumber, u64) -> Result<(), MacosNativeOperationError>,
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

#[cfg(target_os = "macos")]
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
#[cfg(target_os = "macos")]
use foundation::*;

pub use api::{
    ActiveSpaceFocusTargetHint, MacosNativeApi, MissionControlHotkey, MissionControlModifiers,
    NativeBackendOptions, NativeBounds, NativeDesktopSnapshot, NativeDiagnostics, NativeDirection,
    NativeSpaceId, NativeSpaceSnapshot, NativeWindowId, NativeWindowSnapshot,
};
pub use desktop_topology_snapshot::SpaceKind;
pub use desktop_topology_snapshot::{
    RawSpaceRecord, RawTopologySnapshot, RawWindow, WindowSnapshot,
};
pub use error::{
    MacosNativeBridgeError, MacosNativeConnectError, MacosNativeOperationError,
    MacosNativeProbeError,
};
pub use real_api::RealNativeApi;

#[cfg(test)]
#[allow(dead_code)]
mod tests;
