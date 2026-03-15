use anyhow::Result;

use crate::engine::contracts::adapter::AppAdapter;
use crate::engine::runtime::ProcessId;
use crate::engine::wm::configured::ConfiguredWindowManager;

/// Captures the focused-window state extracted by the preamble shared across
/// `attempt_focused_app_focus`, `attempt_focused_app_move`, and
/// `attempt_focused_app_resize` in `orchestrator.rs`.
pub(crate) struct AppContext {
    pub(crate) source_window_id: u64,
    pub(crate) source_tile_index: usize,
    pub(crate) pid: ProcessId,
    pub(crate) app_id: String,
    pub(crate) title: String,
}

impl AppContext {
    /// Build an `AppContext` from the currently-focused window.
    /// Returns `Ok(None)` when there is no focused process ID (same condition
    /// the orchestrator methods use to bail early).
    pub(crate) fn from_focused(wm: &mut ConfiguredWindowManager) -> Result<Option<Self>> {
        let focused = wm.focused_window()?;
        let source_window_id = focused.id;
        let source_tile_index = focused.original_tile_index;
        let app_id = focused.app_id.unwrap_or_default();
        let title = focused.title.unwrap_or_default();
        let Some(pid) = focused.pid else {
            return Ok(None);
        };

        Ok(Some(Self {
            source_window_id,
            source_tile_index,
            pid,
            app_id,
            title,
        }))
    }

    /// Resolve the ordered chain of app adapters for this context.
    pub(crate) fn resolve_chain(&self) -> Vec<Box<dyn AppAdapter>> {
        crate::engine::chain_resolver::resolve_app_chain(
            &self.app_id,
            self.pid.get(),
            &self.title,
        )
    }
}

// ── FocusedAppSession ────────────────────────────────────────────────────────

/// A fully-resolved snapshot of the currently-focused window together with its
/// adapter chain.  Shared across focus, resize, and (future) move actions so
/// that each action does not repeat the same focused-window / chain-resolution
/// preamble.
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
/// condition used by `AppContext::from_focused`).
pub(crate) fn with_focused_app_session<T>(
    wm: &mut ConfiguredWindowManager,
    f: impl FnOnce(FocusedAppSession) -> Result<T>,
) -> Result<Option<T>> {
    let focused = wm.focused_window()?;
    let Some(pid) = focused.pid else {
        return Ok(None);
    };
    let app_id = focused.app_id.unwrap_or_default();
    let title = focused.title.unwrap_or_default();
    let chain = crate::engine::chain_resolver::resolve_app_chain(&app_id, pid.get(), &title);
    Ok(Some(f(FocusedAppSession {
        source_window_id: focused.id,
        source_tile_index: focused.original_tile_index,
        pid,
        app_id,
        title,
        chain,
    })?))
}

// ── chain-walking helpers ────────────────────────────────────────────────────

/// Walk an adapter chain, calling `f` for each adapter and returning the first
/// `Some` value.  Returns `Ok(None)` when no adapter produces a result.
pub(crate) fn walk_chain<T, F>(chain: &[Box<dyn AppAdapter>], mut f: F) -> Result<Option<T>>
where
    F: FnMut(&dyn AppAdapter) -> Result<Option<T>>,
{
    walk_chain_iter(chain.len(), |i| f(chain[i].as_ref()))
}

/// Testable inner implementation of the chain-walk loop.
/// `count` is the number of items; `f` receives the index and returns
/// `Ok(Some(v))` to stop, `Ok(None)` to continue, or `Err(e)` to propagate.
#[allow(dead_code)]
pub(crate) fn walk_chain_iter<T, F>(count: usize, mut f: F) -> Result<Option<T>>
where
    F: FnMut(usize) -> Result<Option<T>>,
{
    for i in 0..count {
        if let Some(v) = f(i)? {
            return Ok(Some(v));
        }
    }
    Ok(None)
}

// ── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::window_manager::{
        ConfiguredWindowManager, FocusedWindowRecord, ResizeIntent, WindowManagerCapabilities,
        WindowManagerFeatures, WindowManagerSession, WindowRecord,
    };
    use crate::engine::topology::Direction;

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
        let mut wm = ConfiguredWindowManager::new(
            Box::new(NoPidSession),
            WindowManagerFeatures::default(),
        );
        let result = with_focused_app_session(&mut wm, |_session| Ok(42u32));
        assert_eq!(result.unwrap(), None);
    }

    #[test]
    fn walk_chain_returns_first_some() {
        // Simulates [None, Some(42), Some(99)] — should stop at index 1.
        let values: &[Option<i32>] = &[None, Some(42), Some(99)];
        let result = walk_chain_iter(values.len(), |i| Ok(values[i])).unwrap();
        assert_eq!(result, Some(42));
    }

    #[test]
    fn walk_chain_returns_none_if_all_none() {
        let values: &[Option<i32>] = &[None, None, None];
        let result = walk_chain_iter(values.len(), |i| Ok(values[i])).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn walk_chain_stops_and_propagates_error() {
        let result = walk_chain_iter::<u32, _>(3, |i| {
            if i == 1 { Err(anyhow::anyhow!("adapter failed")) }
            else { Ok(None) }
        });
        assert!(result.is_err());
    }
}
