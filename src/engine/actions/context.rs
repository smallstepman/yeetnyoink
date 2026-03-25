use anyhow::{Error, Result};

use crate::engine::contracts::adapter::AppAdapter;
use crate::engine::runtime::ProcessId;
use crate::engine::wm::configured::ConfiguredWindowManager;

// ── FocusedAppSession ────────────────────────────────────────────────────────

/// A fully-resolved snapshot of the currently-focused window together with its
/// adapter chain.  Shared across focus, move, and resize actions so that each
/// action does not repeat the same focused-window / chain-resolution preamble.
pub(crate) struct FocusedAppSession {
    pub(crate) source_window_id: u64,
    pub(crate) source_tile_index: usize,
    pub(crate) pid: ProcessId,
    pub(crate) app_id: String,
    pub(crate) title: String,
    pub(crate) chain: Vec<Box<dyn AppAdapter>>,
}

/// Resolve the focused window and adapter chain, then call `f` with the
/// resulting [`FocusedAppSession`].
///
/// Returns `Ok(None)` when the focused window has no PID (same early-exit
/// condition used across focus, move, and resize actions).
pub(crate) fn with_focused_app_session<T>(
    wm: &mut ConfiguredWindowManager,
    f: impl FnOnce(FocusedAppSession) -> Result<T>,
) -> Result<Option<T>> {
    let focused = {
        let _span = tracing::debug_span!("actions.context.focused_window").entered();
        wm.focused_window()?
    };
    let Some(pid) = focused.pid else {
        return Ok(None);
    };
    let app_id = focused.app_id.unwrap_or_default();
    let title = focused.title.unwrap_or_default();
    let chain = {
        let _span = tracing::debug_span!(
            "actions.context.resolve_chain",
            app_id = app_id.as_str(),
            pid = pid.get()
        )
        .entered();
        crate::engine::resolution::resolve_app_chain(&app_id, pid.get(), &title)
    };
    Ok(Some(f(FocusedAppSession {
        source_window_id: focused.id,
        source_tile_index: focused.original_tile_index,
        pid,
        app_id,
        title,
        chain,
    })?))
}

pub(crate) fn is_no_focused_window_error(err: &Error) -> bool {
    err.chain()
        .any(|cause| cause.to_string() == "no focused window")
}

// ── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::topology::Direction;
    use crate::engine::wm::{
        ConfiguredWindowManager, FocusedWindowRecord, ResizeIntent, WindowManagerCapabilities,
        WindowManagerFeatures, WindowManagerSession, WindowRecord,
    };

    struct NoPidSession;

    impl WindowManagerSession for NoPidSession {
        fn adapter_name(&self) -> &'static str {
            "no-pid"
        }
        fn capabilities(&self) -> WindowManagerCapabilities {
            WindowManagerCapabilities::none()
        }
        fn focused_window(&mut self) -> anyhow::Result<FocusedWindowRecord> {
            Ok(FocusedWindowRecord {
                id: 1,
                app_id: Some("test-app".into()),
                title: Some("test-title".into()),
                pid: None,
                original_tile_index: 0,
            })
        }
        fn windows(&mut self) -> anyhow::Result<Vec<WindowRecord>> {
            Ok(Vec::new())
        }
        fn focus_direction(&mut self, _: Direction) -> anyhow::Result<()> {
            Ok(())
        }
        fn move_direction(&mut self, _: Direction) -> anyhow::Result<()> {
            Ok(())
        }
        fn resize_with_intent(&mut self, _: ResizeIntent) -> anyhow::Result<()> {
            Ok(())
        }
        fn spawn(&mut self, _: Vec<String>) -> anyhow::Result<()> {
            Ok(())
        }
        fn focus_window_by_id(&mut self, _: u64) -> anyhow::Result<()> {
            Ok(())
        }
        fn close_window_by_id(&mut self, _: u64) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn with_focused_app_session_returns_none_when_focused_window_has_no_pid() {
        let mut wm =
            ConfiguredWindowManager::new(Box::new(NoPidSession), WindowManagerFeatures::default());
        let result = with_focused_app_session(&mut wm, |_session| Ok(42u32));
        assert_eq!(result.unwrap(), None);
    }

    struct NoFocusedWindowSession;

    impl WindowManagerSession for NoFocusedWindowSession {
        fn adapter_name(&self) -> &'static str {
            "no-focused-window"
        }
        fn capabilities(&self) -> WindowManagerCapabilities {
            WindowManagerCapabilities::none()
        }
        fn focused_window(&mut self) -> anyhow::Result<FocusedWindowRecord> {
            Err(anyhow::anyhow!("no focused window"))
        }
        fn windows(&mut self) -> anyhow::Result<Vec<WindowRecord>> {
            Ok(Vec::new())
        }
        fn focus_direction(&mut self, _: Direction) -> anyhow::Result<()> {
            Ok(())
        }
        fn move_direction(&mut self, _: Direction) -> anyhow::Result<()> {
            Ok(())
        }
        fn resize_with_intent(&mut self, _: ResizeIntent) -> anyhow::Result<()> {
            Ok(())
        }
        fn spawn(&mut self, _: Vec<String>) -> anyhow::Result<()> {
            Ok(())
        }
        fn focus_window_by_id(&mut self, _: u64) -> anyhow::Result<()> {
            Ok(())
        }
        fn close_window_by_id(&mut self, _: u64) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn detects_no_focused_window_error_across_contexts() {
        let mut wm = ConfiguredWindowManager::new(
            Box::new(NoFocusedWindowSession),
            WindowManagerFeatures::default(),
        );
        let err = with_focused_app_session(&mut wm, |_session| Ok(42u32))
            .expect_err("missing focused window should error");
        assert!(is_no_focused_window_error(&err));
    }
}
