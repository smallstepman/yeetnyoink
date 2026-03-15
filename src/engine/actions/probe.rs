use anyhow::{Context, Result};

use crate::engine::contracts::{AppAdapter, TopologyHandler};
use crate::engine::runtime::ProcessId;
use crate::engine::topology::Direction;
use crate::engine::wm::{ConfiguredWindowManager, WindowRecord};
use crate::engine::resolution::resolve_app_chain;
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

/// A scoped helper that owns directional focus mutation and restoration for a
/// single source window.  Callers in movement, merge, and the orchestrator
/// should build on this rather than invoking the free `probe_directional_*`
/// functions directly.
pub(crate) struct DirectionalWindowProbe<'a> {
    wm: &'a mut ConfiguredWindowManager,
    source_window_id: u64,
}

impl<'a> DirectionalWindowProbe<'a> {
    pub(crate) fn new(wm: &'a mut ConfiguredWindowManager, source_window_id: u64) -> Self {
        Self {
            wm,
            source_window_id,
        }
    }

    /// Move focus in `dir`, record the target window, and then apply
    /// `focus_mode` (restore focus to source, or leave it on the target).
    ///
    /// Returns `None` when there is no window in that direction or when the
    /// focus_direction call fails.
    pub(crate) fn window(
        &mut self,
        dir: Direction,
        focus_mode: DirectionalProbeFocusMode,
    ) -> Result<Option<WindowRecord>> {
        if let Err(err) = self.wm.focus_direction(dir) {
            logging::debug(format!(
                "actions::probe: directional target probe failed dir={} err={:#}",
                dir, err
            ));
            return Ok(None);
        }

        let target = match focused_window_record(self.wm) {
            Ok(window) => window,
            Err(err) => {
                let _ = self.wm.focus_window_by_id(self.source_window_id);
                return Err(err.context("failed to read target window during directional probe"));
            }
        };

        if target.id == self.source_window_id {
            return Ok(None);
        }

        if matches!(focus_mode, DirectionalProbeFocusMode::RestoreSource) {
            self.wm
                .focus_window_by_id(self.source_window_id)
                .with_context(|| {
                    format!(
                        "failed to restore focus to window {}",
                        self.source_window_id
                    )
                })?;
        }
        Ok(Some(target))
    }

    /// Like [`window`] but also filters by adapter name — returns `None` when
    /// the target window does not match the adapter.
    pub(crate) fn window_matching_adapter(
        &mut self,
        dir: Direction,
        adapter_name: &str,
        focus_mode: DirectionalProbeFocusMode,
    ) -> Result<Option<WindowRecord>> {
        let Some(target_window) = self.window(dir, focus_mode)? else {
            return Ok(None);
        };
        if window_matches_adapter(adapter_name, &target_window) {
            return Ok(Some(target_window));
        }
        if matches!(focus_mode, DirectionalProbeFocusMode::KeepTarget) {
            let _ = self.wm.focus_window_by_id(self.source_window_id);
        }
        Ok(None)
    }
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

    use super::{focused_window_record, DirectionalProbeFocusMode, DirectionalWindowProbe};
    use crate::engine::runtime::ProcessId;
    use crate::engine::topology::Direction;
    use crate::engine::wm::{
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
    fn directional_probe_returns_none_when_focus_direction_fails() {
        let mut wm = fake_wm(10, false); // focus_direction returns Err
        let mut probe = DirectionalWindowProbe::new(&mut wm, 10);
        let result = probe.window(Direction::East, DirectionalProbeFocusMode::RestoreSource);
        // focus failure is treated as no target (returns Ok(None), not Err)
        assert!(result.is_ok(), "should not propagate focus error");
        assert_eq!(result.unwrap(), None);
    }

    #[test]
    fn directional_probe_returns_none_when_target_equals_source_window() {
        // focus_direction succeeds but focused_window still returns the source id
        let mut wm = fake_wm(99, true); // after focus_direction, focused_id stays 99
        let mut probe = DirectionalWindowProbe::new(&mut wm, 99);
        let result = probe.window(Direction::East, DirectionalProbeFocusMode::RestoreSource);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None, "no target when source == target");
    }

    // --- DirectionalWindowProbe tests ---

    /// A FakeSession variant that switches focused_id to target_id when
    /// focus_direction is called, simulating an actual directional focus move.
    struct DirectionalFakeSession {
        focused_id: u64,
        target_id: u64,
    }

    impl WindowManagerSession for DirectionalFakeSession {
        fn adapter_name(&self) -> &'static str {
            "directional-fake"
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
                original_tile_index: 0,
            })
        }

        fn windows(&mut self) -> Result<Vec<WindowRecord>> {
            Ok(Vec::new())
        }

        fn focus_direction(&mut self, _direction: Direction) -> Result<()> {
            self.focused_id = self.target_id;
            Ok(())
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

    fn directional_fake_wm(source_id: u64, target_id: u64) -> ConfiguredWindowManager {
        ConfiguredWindowManager::new(
            Box::new(DirectionalFakeSession {
                focused_id: source_id,
                target_id,
            }),
            WindowManagerFeatures::default(),
        )
    }

    #[test]
    fn directional_probe_restore_source_returns_focus_to_source_window() {
        let source_id = 10;
        let target_id = 20;
        let mut wm = directional_fake_wm(source_id, target_id);
        let mut probe = DirectionalWindowProbe::new(&mut wm, source_id);
        let result = probe
            .window(Direction::East, DirectionalProbeFocusMode::RestoreSource)
            .expect("probe should succeed");
        assert!(result.is_some(), "should find a target window");
        assert_eq!(result.unwrap().id, target_id);
        // RestoreSource: focus must be restored to source after the probe
        let focused = wm.focused_window().expect("focused_window should succeed");
        assert_eq!(focused.id, source_id, "focus should be restored to source");
    }

    #[test]
    fn directional_probe_keep_target_leaves_focus_on_target_window() {
        let source_id = 10;
        let target_id = 20;
        let mut wm = directional_fake_wm(source_id, target_id);
        let mut probe = DirectionalWindowProbe::new(&mut wm, source_id);
        let result = probe
            .window(Direction::East, DirectionalProbeFocusMode::KeepTarget)
            .expect("probe should succeed");
        assert!(result.is_some(), "should find a target window");
        assert_eq!(result.unwrap().id, target_id);
        // KeepTarget: focus must remain on the target window
        let focused = wm.focused_window().expect("focused_window should succeed");
        assert_eq!(focused.id, target_id, "focus should remain on target");
    }

    /// `window_matching_adapter` with `KeepTarget` and a non-matching adapter:
    /// the target window is found, focus was left on it by `window()`, but the
    /// adapter check fails so `window_matching_adapter` must restore focus to
    /// the source before returning `None`.
    #[test]
    fn window_matching_adapter_keep_target_mismatch_restores_focus_to_source() {
        let source_id = 10;
        let target_id = 20;
        let mut wm = directional_fake_wm(source_id, target_id);
        let mut probe = DirectionalWindowProbe::new(&mut wm, source_id);
        // "nonexistent-adapter" will never match the fake window's app chain
        let result = probe
            .window_matching_adapter(
                Direction::East,
                "nonexistent-adapter",
                DirectionalProbeFocusMode::KeepTarget,
            )
            .expect("window_matching_adapter should not error");
        assert_eq!(result, None, "mismatched adapter should yield None");
        // Focus must have been restored to source, not left on the target
        let focused = wm.focused_window().expect("focused_window should succeed");
        assert_eq!(
            focused.id, source_id,
            "KeepTarget+mismatch must restore focus to source"
        );
    }

    /// `window_matching_adapter` with `RestoreSource` and a non-matching adapter:
    /// `window()` already restored focus to source; the adapter check fails but
    /// focus is already correct.  Returns `None` with focus on source.
    #[test]
    fn window_matching_adapter_restore_source_mismatch_leaves_focus_on_source() {
        let source_id = 10;
        let target_id = 20;
        let mut wm = directional_fake_wm(source_id, target_id);
        let mut probe = DirectionalWindowProbe::new(&mut wm, source_id);
        let result = probe
            .window_matching_adapter(
                Direction::East,
                "nonexistent-adapter",
                DirectionalProbeFocusMode::RestoreSource,
            )
            .expect("window_matching_adapter should not error");
        assert_eq!(result, None, "mismatched adapter should yield None");
        // Focus was already restored to source by window(); must still be there
        let focused = wm.focused_window().expect("focused_window should succeed");
        assert_eq!(
            focused.id, source_id,
            "RestoreSource+mismatch must leave focus on source"
        );
    }
}
