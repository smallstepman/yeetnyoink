use std::{collections::HashSet, time::Instant};

use crate::api::{MacosNativeApi, NativeDesktopSnapshot, NativeDirection};
use crate::desktop_topology_snapshot::SpaceKind;
use crate::error::MacosNativeOperationError;

#[cfg(target_os = "macos")]
const SPACE_SWITCH_SETTLE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(300);
#[cfg(target_os = "macos")]
const SPACE_SWITCH_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(10);
#[cfg(target_os = "macos")]
const SPACE_SWITCH_STABLE_TARGET_POLLS: usize = 3;

pub(crate) fn wait_for_space_presentation<A: MacosNativeApi + ?Sized>(
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
        let target_visible =
            target_window_ids.is_empty() || !target_window_ids.is_disjoint(&onscreen_window_ids);
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

pub(crate) fn switch_space_in_snapshot<A: MacosNativeApi + ?Sized>(
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
        match wait_for_space_presentation(api, space_id, source_focus_window_id, &target_window_ids)
        {
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
