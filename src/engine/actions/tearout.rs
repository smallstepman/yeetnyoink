use std::collections::BTreeSet;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::engine::actions::context::FocusedAppSession;
use crate::engine::contracts::{AppAdapter, TopologyHandler};
use crate::engine::runtime::ProcessId;
use crate::engine::topology::Direction;
use crate::engine::wm::{
    plan_tear_out, CapabilitySupport, ConfiguredWindowManager, WindowRecord,
};
use crate::logging;

// ── TearOutRequest ────────────────────────────────────────────────────────────

/// Groups the parameters for a single tear-out operation into a named context
/// so that call-sites in `movement.rs` remain readable.
pub(crate) struct TearOutRequest<'a> {
    pub(crate) app: &'a dyn AppAdapter,
    pub(crate) session: &'a FocusedAppSession,
    pub(crate) dir: Direction,
    pub(crate) decision_label: &'a str,
}

impl<'a> TearOutRequest<'a> {
    pub(crate) fn run(self, wm: &mut ConfiguredWindowManager) -> Result<()> {
        execute_app_tear_out(
            wm,
            self.app,
            self.dir,
            self.session.pid.get(),
            self.session.source_window_id,
            self.session.source_tile_index,
            Some(self.session.pid),
            &self.session.app_id,
            self.decision_label,
        )
    }
}

pub(crate) fn execute_app_tear_out(
    wm: &mut ConfiguredWindowManager,
    app: &dyn AppAdapter,
    dir: Direction,
    owner_pid: u32,
    source_window_id: u64,
    source_tile_index: usize,
    source_pid: Option<ProcessId>,
    app_id: &str,
    decision_label: &str,
) -> Result<()> {
    let adapter_name = app.adapter_name();
    let pre_window_ids: BTreeSet<u64> = match wm.windows() {
        Ok(windows) => windows.into_iter().map(|window| window.id).collect(),
        Err(err) => {
            logging::debug(format!(
                "actions::tearout: unable to snapshot pre-tearout windows err={:#}",
                err
            ));
            BTreeSet::new()
        }
    };
    let tear = TopologyHandler::move_out(app, dir, owner_pid)
        .with_context(|| format!("{adapter_name} move_out failed"))?;
    if let Some(command) = tear.spawn_command {
        wm.spawn(command)
            .with_context(|| format!("{adapter_name} tear-out spawn via wm failed"))?;
    }
    let tearout_window_id = match focus_tearout_window(
        wm,
        &pre_window_ids,
        source_window_id,
        source_pid,
        app_id,
    ) {
        Ok(window_id) => window_id,
        Err(err) => {
            logging::debug(format!(
                "actions::tearout: unable to focus tear-out window adapter={} err={:#}",
                adapter_name, err
            ));
            None
        }
    };
    if let Err(err) = place_tearout_window(
        wm,
        dir,
        source_window_id,
        source_tile_index,
        tearout_window_id,
    ) {
        logging::debug(format!(
            "actions::tearout: tear-out placement fallback failed adapter={} err={:#}",
            adapter_name, err
        ));
    }
    logging::debug(format!(
        "actions::tearout: app move handled by {adapter_name} decision={decision_label}"
    ));
    Ok(())
}

pub(crate) fn focus_tearout_window(
    wm: &mut ConfiguredWindowManager,
    pre_window_ids: &BTreeSet<u64>,
    source_window_id: u64,
    source_pid: Option<ProcessId>,
    source_app_id: &str,
) -> Result<Option<u64>> {
    let target_window_id = wait_for_tearout_window_id(
        wm,
        pre_window_ids,
        source_window_id,
        source_pid,
        source_app_id,
    )?;
    if let Some(target_window_id) = target_window_id {
        if target_window_id != source_window_id {
            wm.focus_window_by_id(target_window_id)?;
            return Ok(Some(target_window_id));
        }
    }
    Ok(None)
}

pub(crate) fn wait_for_tearout_window_id(
    wm: &mut ConfiguredWindowManager,
    pre_window_ids: &BTreeSet<u64>,
    source_window_id: u64,
    source_pid: Option<ProcessId>,
    source_app_id: &str,
) -> Result<Option<u64>> {
    const ATTEMPTS: usize = 25;
    const DELAY: Duration = Duration::from_millis(40);

    for attempt in 0..ATTEMPTS {
        match wm.windows() {
            Ok(windows) => {
                if let Some(target_window_id) = select_tearout_window_id(
                    pre_window_ids,
                    &windows,
                    source_window_id,
                    source_pid,
                    source_app_id,
                ) {
                    if target_window_id != source_window_id {
                        return Ok(Some(target_window_id));
                    }
                }
            }
            Err(err) => {
                logging::debug(format!(
                    "actions::tearout: tear-out post-window snapshot failed attempt={} err={:#}",
                    attempt + 1,
                    err
                ));
            }
        }

        if attempt + 1 < ATTEMPTS {
            std::thread::sleep(DELAY);
        }
    }

    Ok(None)
}

pub(crate) fn select_tearout_window_id(
    pre_window_ids: &BTreeSet<u64>,
    windows: &[WindowRecord],
    source_window_id: u64,
    source_pid: Option<ProcessId>,
    source_app_id: &str,
) -> Option<u64> {
    let mut new_windows: Vec<&WindowRecord> = windows
        .iter()
        .filter(|window| !pre_window_ids.contains(&window.id))
        .collect();
    if new_windows.is_empty() {
        return windows
            .iter()
            .find(|window| window.is_focused && window.id != source_window_id)
            .map(|window| window.id);
    }
    new_windows.sort_by_key(|window| window.id);

    new_windows
        .iter()
        .find(|window| {
            window.pid == source_pid && window.app_id.as_deref() == Some(source_app_id)
        })
        .map(|window| window.id)
        .or_else(|| {
            new_windows
                .iter()
                .find(|window| window.pid == source_pid)
                .map(|window| window.id)
        })
        .or_else(|| {
            new_windows
                .iter()
                .find(|window| window.app_id.as_deref() == Some(source_app_id))
                .map(|window| window.id)
        })
        .or_else(|| {
            new_windows
                .iter()
                .find(|window| window.is_focused)
                .map(|window| window.id)
        })
        .or_else(|| new_windows.first().map(|window| window.id))
}

pub(crate) fn place_tearout_window(
    wm: &mut ConfiguredWindowManager,
    dir: Direction,
    source_window_id: u64,
    source_tile_index: usize,
    target_window_id: Option<u64>,
) -> Result<()> {
    if let Some(target_window_id) = target_window_id.filter(|id| *id != source_window_id) {
        wm.focus_window_by_id(target_window_id)?;
    }

    let focused_window_id = wm.focused_window()?.id;
    if focused_window_id == source_window_id {
        return Ok(());
    }

    match plan_tear_out(wm.capabilities(), dir) {
        CapabilitySupport::Native => wm.move_direction(dir),
        CapabilitySupport::Unsupported => Ok(()),
        CapabilitySupport::Composed => {
            let adapter_name = wm.adapter_name();
            wm.tear_out_composer_mut()
                .with_context(|| {
                    format!(
                        "configured wm '{}' is missing a tear-out composer for {dir}",
                        adapter_name
                    )
                })?
                .compose_tear_out(dir, source_tile_index)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{focus_tearout_window, select_tearout_window_id, TearOutRequest};
    use crate::engine::runtime::ProcessId;
    use crate::engine::wm::{
        ConfiguredWindowManager, FocusedWindowRecord, ResizeIntent, WindowManagerCapabilities,
        WindowManagerFeatures, WindowManagerSession, WindowRecord,
    };
    use crate::engine::topology::Direction;

    fn window(id: u64, pid: u32, app_id: &str, focused: bool) -> WindowRecord {
        WindowRecord {
            id,
            app_id: Some(app_id.into()),
            title: None,
            pid: ProcessId::new(pid),
            is_focused: focused,
            original_tile_index: 0,
        }
    }

    #[test]
    fn select_tearout_window_id_returns_none_when_windows_empty() {
        let pre = BTreeSet::new();
        let result = select_tearout_window_id(&pre, &[], 10, ProcessId::new(1), "app");
        assert_eq!(result, None);
    }

    #[test]
    fn select_tearout_window_id_returns_none_when_source_only_and_not_focused_elsewhere() {
        let mut pre = BTreeSet::new();
        pre.insert(10);
        let windows = vec![window(10, 1, "app", false)];
        let result = select_tearout_window_id(&pre, &windows, 10, ProcessId::new(1), "app");
        assert_eq!(result, None);
    }

    #[test]
    fn select_tearout_window_id_prefers_pid_only_match_over_app_id_only() {
        let mut pre = BTreeSet::new();
        pre.insert(10);
        let source_pid = ProcessId::new(42);
        let windows = vec![
            window(10, 42, "source-app", false),
            window(11, 99, "source-app", false), // same app_id, different pid
            window(12, 42, "other-app", false),  // same pid, different app_id
        ];
        // window 12 has matching pid so should be preferred over window 11 (app_id only)
        let result = select_tearout_window_id(&pre, &windows, 10, source_pid, "source-app");
        // Both 11 and 12 are new; 12 matches pid so pid-only wins over app_id-only
        // Actually the function first checks pid+app_id, then pid-only, then app_id-only
        // window 12 matches pid-only, window 11 matches app_id-only → window 12 wins
        assert_eq!(result, Some(12));
    }

    #[test]
    fn select_tearout_window_id_falls_back_to_app_id_only_when_no_pid_match() {
        let mut pre = BTreeSet::new();
        pre.insert(10);
        let source_pid = ProcessId::new(42);
        let windows = vec![
            window(10, 42, "source-app", false),
            window(11, 99, "source-app", false), // app_id matches, pid doesn't
            window(12, 77, "other-app", true),   // neither matches, but focused
        ];
        // No pid match → falls back to app_id match (window 11)
        let result = select_tearout_window_id(&pre, &windows, 10, source_pid, "source-app");
        assert_eq!(result, Some(11));
    }

    #[test]
    fn select_tearout_window_id_returns_first_new_window_when_no_preference_matches() {
        let mut pre = BTreeSet::new();
        pre.insert(10);
        let source_pid = ProcessId::new(42);
        let windows = vec![
            window(10, 42, "source-app", false),
            window(15, 77, "other-app", false), // no pid/app_id match, not focused
            window(11, 99, "another-app", false), // no pid/app_id match, not focused
        ];
        // No pid, app_id, or focused match → returns first by id (11 < 15)
        let result = select_tearout_window_id(&pre, &windows, 10, source_pid, "source-app");
        assert_eq!(result, Some(11));
    }

    // ── minimal fake WM for wait/focus tests ─────────────────────────────────

    struct SnapshotWM {
        /// Snapshots returned in order on successive `windows()` calls.
        /// When the list is exhausted, the last entry is repeated.
        snapshots: Vec<Vec<WindowRecord>>,
        call_count: usize,
        focused_id: u64,
    }

    impl SnapshotWM {
        fn new(focused_id: u64, snapshots: Vec<Vec<WindowRecord>>) -> Self {
            Self { snapshots, call_count: 0, focused_id }
        }
    }

    impl WindowManagerSession for SnapshotWM {
        fn adapter_name(&self) -> &'static str { "snapshot-fake" }
        fn capabilities(&self) -> WindowManagerCapabilities { WindowManagerCapabilities::none() }
        fn focused_window(&mut self) -> anyhow::Result<FocusedWindowRecord> {
            Ok(FocusedWindowRecord {
                id: self.focused_id,
                app_id: Some("com.mitchellh.ghostty".into()),
                title: None,
                pid: ProcessId::new(10),
                original_tile_index: 0,
            })
        }
        fn windows(&mut self) -> anyhow::Result<Vec<WindowRecord>> {
            let idx = self.call_count.min(self.snapshots.len().saturating_sub(1));
            self.call_count += 1;
            Ok(self.snapshots.get(idx).cloned().unwrap_or_default())
        }
        fn focus_direction(&mut self, _: Direction) -> anyhow::Result<()> { Ok(()) }
        fn move_direction(&mut self, _: Direction) -> anyhow::Result<()> { Ok(()) }
        fn resize_with_intent(&mut self, _: ResizeIntent) -> anyhow::Result<()> { Ok(()) }
        fn spawn(&mut self, _: Vec<String>) -> anyhow::Result<()> { Ok(()) }
        fn focus_window_by_id(&mut self, id: u64) -> anyhow::Result<()> {
            self.focused_id = id;
            Ok(())
        }
        fn close_window_by_id(&mut self, _: u64) -> anyhow::Result<()> { Ok(()) }
    }

    fn snapshot_wm(focused_id: u64, snapshots: Vec<Vec<WindowRecord>>) -> ConfiguredWindowManager {
        ConfiguredWindowManager::new(
            Box::new(SnapshotWM::new(focused_id, snapshots)),
            WindowManagerFeatures::default(),
        )
    }

    /// Verifies that `focus_tearout_window` resolves a new window that does not
    /// appear in the first snapshot poll but becomes visible on the third attempt,
    /// and that focus is actually moved to the late-appearing window.
    ///
    /// Compile-fails until `TearOutRequest` is defined in this module.
    #[test]
    fn tear_out_wait_and_focus_returns_new_window_when_it_appears_late() {
        // Structural compile guard — fails until TearOutRequest is defined.
        fn _assert_exists<'a>(_: std::marker::PhantomData<TearOutRequest<'a>>) {}

        let source_pid = ProcessId::new(10);
        let app_id = "com.mitchellh.ghostty";
        let source_window_id = 1u64;

        let source = WindowRecord {
            id: source_window_id,
            app_id: Some(app_id.into()),
            title: None,
            pid: source_pid,
            is_focused: true,
            original_tile_index: 0,
        };
        let new_window = WindowRecord {
            id: 2,
            app_id: Some(app_id.into()),
            title: None,
            pid: source_pid,
            is_focused: false,
            original_tile_index: 0,
        };

        let pre_ids: BTreeSet<u64> = [source_window_id].into_iter().collect();

        // Three snapshot rounds: source only, source only, then new_window appears.
        // The WM starts focused on the source window.
        let mut wm = snapshot_wm(source_window_id, vec![
            vec![source.clone()],
            vec![source.clone()],
            vec![source.clone(), new_window.clone()],
        ]);

        // Exercise focus_tearout_window (not just wait_for_tearout_window_id) to
        // verify that the wait-and-focus path actually moves focus to the new window.
        let result = focus_tearout_window(
            &mut wm,
            &pre_ids,
            source_window_id,
            source_pid,
            app_id,
        )
        .expect("focus_tearout_window should not error");

        assert_eq!(
            result,
            Some(2),
            "new window should be returned even when it appears on the third poll attempt"
        );

        // The key behavioral assertion: focus must have moved to the new window.
        let focused_after = wm
            .focused_window()
            .expect("focused_window should not fail after focus_tearout_window")
            .id;
        assert_eq!(
            focused_after,
            2,
            "focus should have moved to the late-appearing window"
        );
    }
}
