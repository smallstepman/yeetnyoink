use anyhow::{Context, Result};

use crate::engine::contract::{AppAdapter, TopologyHandler};
use crate::engine::runtime::ProcessId;
use crate::engine::topology::Direction;
use crate::engine::window_manager::{ConfiguredWindowManager, WindowRecord};
use crate::engine::chain_resolver::resolve_app_chain;
use crate::logging;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DirectionalProbeFocusMode {
    RestoreSource,
    KeepTarget,
}

pub(crate) fn focused_window_record(wm: &mut ConfiguredWindowManager) -> Result<WindowRecord> {
    let window = wm.focused_window()?;
    Ok(WindowRecord {
        id: window.id,
        app_id: window.app_id,
        title: window.title,
        pid: window.pid,
        is_focused: true,
        original_tile_index: window.original_tile_index,
    })
}

pub(crate) fn resolve_adapter_for_window(
    adapter_name: &str,
    window: &WindowRecord,
) -> Option<Box<dyn AppAdapter>> {
    let owner_pid = window.pid.map(ProcessId::get).unwrap_or(0);
    resolve_app_chain(
        window.app_id.as_deref().unwrap_or_default(),
        owner_pid,
        window.title.as_deref().unwrap_or_default(),
    )
    .into_iter()
    .find(|adapter| adapter.adapter_name() == adapter_name)
}

fn window_matches_adapter(adapter_name: &str, window: &WindowRecord) -> bool {
    resolve_adapter_for_window(adapter_name, window).is_some()
}

pub(crate) fn probe_directional_target(
    wm: &mut ConfiguredWindowManager,
    dir: Direction,
    source_window_id: u64,
    focus_mode: DirectionalProbeFocusMode,
) -> Result<Option<WindowRecord>> {
    if let Err(err) = wm.focus_direction(dir) {
        logging::debug(format!(
            "orchestrator: directional target probe failed dir={} err={:#}",
            dir, err
        ));
        return Ok(None);
    }

    let target = match focused_window_record(wm) {
        Ok(window) => window,
        Err(err) => {
            let _ = wm.focus_window_by_id(source_window_id);
            return Err(err.context("failed to read target window during directional probe"));
        }
    };

    if target.id == source_window_id {
        return Ok(None);
    }

    if matches!(focus_mode, DirectionalProbeFocusMode::RestoreSource) {
        wm.focus_window_by_id(source_window_id).with_context(|| {
            format!("failed to restore focus to window {}", source_window_id)
        })?;
    }
    Ok(Some(target))
}

pub(crate) fn probe_directional_target_for_adapter(
    wm: &mut ConfiguredWindowManager,
    dir: Direction,
    source_window_id: u64,
    adapter_name: &str,
    focus_mode: DirectionalProbeFocusMode,
) -> Result<Option<WindowRecord>> {
    let Some(target_window) =
        probe_directional_target(wm, dir, source_window_id, focus_mode)?
    else {
        return Ok(None);
    };
    if window_matches_adapter(adapter_name, &target_window) {
        return Ok(Some(target_window));
    }
    if matches!(focus_mode, DirectionalProbeFocusMode::KeepTarget) {
        let _ = wm.focus_window_by_id(source_window_id);
    }
    Ok(None)
}

pub(crate) fn probe_in_place_target_for_adapter(
    wm: &mut ConfiguredWindowManager,
    outer_chain: &[Box<dyn AppAdapter>],
    dir: Direction,
    source_window_id: u64,
    owner_pid: u32,
    app_id: &str,
    title: &str,
    adapter_name: &str,
) -> Result<Option<Box<dyn AppAdapter>>> {
    for outer in outer_chain {
        if !outer.capabilities().focus
            || !TopologyHandler::can_focus(outer.as_ref(), dir, owner_pid)?
        {
            continue;
        }
        TopologyHandler::focus(outer.as_ref(), dir, owner_pid)?;
        let focused_window_id = wm.focused_window()?.id;
        if focused_window_id != source_window_id {
            let _ = wm.focus_window_by_id(source_window_id);
            continue;
        }
        let target_app =
            resolve_app_chain(app_id, owner_pid, title)
                .into_iter()
                .find(|candidate| candidate.adapter_name() == adapter_name);
        if target_app.is_some() {
            return Ok(target_app);
        }
        let _ = TopologyHandler::focus(outer.as_ref(), dir.opposite(), owner_pid);
    }
    Ok(None)
}

pub(crate) fn restore_in_place_target_focus(
    outer_chain: &[Box<dyn AppAdapter>],
    dir: Direction,
    owner_pid: u32,
) {
    for outer in outer_chain {
        if outer.capabilities().focus
            && TopologyHandler::can_focus(outer.as_ref(), dir.opposite(), owner_pid)
                .unwrap_or(false)
        {
            let _ = TopologyHandler::focus(outer.as_ref(), dir.opposite(), owner_pid);
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use anyhow::{anyhow, Result};

    use super::{focused_window_record, probe_directional_target, DirectionalProbeFocusMode};
    use crate::engine::runtime::ProcessId;
    use crate::engine::topology::Direction;
    use crate::engine::window_manager::{
        ConfiguredWindowManager, FocusedWindowRecord, ResizeIntent, WindowManagerCapabilities,
        WindowManagerFeatures, WindowManagerSession, WindowRecord,
    };

    struct FakeSession {
        focused_id: u64,
        focus_direction_ok: bool,
    }

    impl WindowManagerSession for FakeSession {
        fn adapter_name(&self) -> &'static str {
            "fake"
        }

        fn capabilities(&self) -> WindowManagerCapabilities {
            WindowManagerCapabilities::none()
        }

        fn focused_window(&mut self) -> Result<FocusedWindowRecord> {
            Ok(FocusedWindowRecord {
                id: self.focused_id,
                app_id: Some("com.test.app".into()),
                title: Some("Test Window".into()),
                pid: ProcessId::new(1234),
                original_tile_index: 2,
            })
        }

        fn windows(&mut self) -> Result<Vec<WindowRecord>> {
            Ok(Vec::new())
        }

        fn focus_direction(&mut self, _direction: Direction) -> Result<()> {
            if self.focus_direction_ok {
                Ok(())
            } else {
                Err(anyhow!("focus direction unavailable"))
            }
        }

        fn move_direction(&mut self, _direction: Direction) -> Result<()> {
            Ok(())
        }

        fn resize_with_intent(&mut self, _intent: ResizeIntent) -> Result<()> {
            Ok(())
        }

        fn spawn(&mut self, _command: Vec<String>) -> Result<()> {
            Ok(())
        }

        fn focus_window_by_id(&mut self, id: u64) -> Result<()> {
            self.focused_id = id;
            Ok(())
        }

        fn close_window_by_id(&mut self, _id: u64) -> Result<()> {
            Ok(())
        }
    }

    fn fake_wm(focused_id: u64, focus_direction_ok: bool) -> ConfiguredWindowManager {
        ConfiguredWindowManager::new(
            Box::new(FakeSession {
                focused_id,
                focus_direction_ok,
            }),
            WindowManagerFeatures::default(),
        )
    }

    #[test]
    fn focused_window_record_maps_all_fields_from_focused_window() {
        let mut wm = fake_wm(42, true);
        let record = focused_window_record(&mut wm).expect("focused_window_record should succeed");
        assert_eq!(record.id, 42);
        assert_eq!(record.app_id.as_deref(), Some("com.test.app"));
        assert_eq!(record.title.as_deref(), Some("Test Window"));
        assert_eq!(record.pid, ProcessId::new(1234));
        assert_eq!(record.original_tile_index, 2);
        assert!(record.is_focused, "focused_window_record should always mark is_focused=true");
    }

    #[test]
    fn probe_directional_target_returns_none_when_focus_direction_fails() {
        let mut wm = fake_wm(10, false); // focus_direction returns Err
        let result = probe_directional_target(
            &mut wm,
            Direction::East,
            10,
            DirectionalProbeFocusMode::RestoreSource,
        );
        // focus failure is treated as no target (returns Ok(None), not Err)
        assert!(result.is_ok(), "should not propagate focus error");
        assert_eq!(result.unwrap(), None);
    }

    #[test]
    fn probe_directional_target_returns_none_when_target_equals_source_window() {
        // focus_direction succeeds but focused_window still returns the source id
        let mut wm = fake_wm(99, true); // after focus_direction, focused_id stays 99
        let result = probe_directional_target(
            &mut wm,
            Direction::East,
            99, // source_window_id matches the focused id
            DirectionalProbeFocusMode::RestoreSource,
        );
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None, "no target when source == target");
    }
}
