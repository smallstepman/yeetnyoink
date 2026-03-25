pub mod browser_host;
pub mod focus;
#[cfg(any(test, target_os = "linux"))]
pub mod focus_or_cycle;
pub mod move_win;
pub mod resize;
pub mod setup;

use anyhow::Result;

use crate::adapters::window_managers::spec_for_backend;
use crate::config::selected_wm_backend;
use crate::engine::actions::focus::attempt_focused_app_focus_from_record;
use crate::engine::actions::orchestrator::{ActionKind, ActionRequest, Orchestrator};
use crate::engine::topology::Direction;
use crate::engine::transfer::bridge::runtime_domains_for_window_manager;
use crate::engine::transfer::ErasedDomain;
use crate::engine::wm::connect_selected;
use crate::logging;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FastFocusPath {
    Handled,
    NotHandled,
    Unavailable,
}

fn load_runtime_domains_for_action(
    kind: ActionKind,
    wm: &mut crate::engine::wm::ConfiguredWindowManager,
) -> Result<Vec<Box<dyn ErasedDomain>>> {
    if matches!(kind, ActionKind::Focus) {
        logging::debug("commands: focus action skips runtime domain loading");
        return Ok(Vec::new());
    }
    runtime_domains_for_window_manager(wm)
}

fn execute_connected_action(kind: ActionKind, dir: Direction) -> Result<()> {
    let mut wm = connect_selected()?;
    let mut orchestrator = Orchestrator::default();
    let domains = {
        let _span = tracing::debug_span!("commands.load_domains").entered();
        load_runtime_domains_for_action(kind, &mut wm)?
    };
    for domain in domains {
        orchestrator.register_domain(domain);
    }
    {
        let _span = tracing::debug_span!("commands.execute_action").entered();
        orchestrator.execute(&mut wm, ActionRequest::new(kind, dir))
    }
}

fn try_fast_focus_path(dir: Direction) -> Result<FastFocusPath> {
    let backend = selected_wm_backend();
    let spec = spec_for_backend(backend);
    match spec.focused_app_record() {
        Ok(Some(focused)) => {
            if attempt_focused_app_focus_from_record(focused, dir)? {
                Ok(FastFocusPath::Handled)
            } else {
                Ok(FastFocusPath::NotHandled)
            }
        }
        Ok(None) => Ok(FastFocusPath::Unavailable),
        Err(err) => {
            logging::debug(format!(
                "commands: fast focus path failed; falling back to WM connect: {err:#}"
            ));
            Ok(FastFocusPath::Unavailable)
        }
    }
}

fn execute_focus_wm_only(dir: Direction) -> Result<()> {
    let mut wm = connect_selected()?;
    let _span = tracing::debug_span!("commands.execute_action").entered();
    wm.focus_direction(dir)
}

fn execute_focus_with_fast_path<F, W, C>(
    dir: Direction,
    fast_focus: F,
    wm_only_fallback: W,
    full_fallback: C,
) -> Result<()>
where
    F: FnOnce(Direction) -> Result<FastFocusPath>,
    W: FnOnce() -> Result<()>,
    C: FnOnce() -> Result<()>,
{
    match fast_focus(dir)? {
        FastFocusPath::Handled => Ok(()),
        FastFocusPath::NotHandled => wm_only_fallback(),
        FastFocusPath::Unavailable => full_fallback(),
    }
}

/// Shared runner for simple action commands (focus, move).
pub(crate) fn run_action(kind: ActionKind, dir: Direction) -> Result<()> {
    let _span = tracing::debug_span!("commands.run_action", ?kind, ?dir).entered();
    match kind {
        ActionKind::Focus => execute_focus_with_fast_path(
            dir,
            try_fast_focus_path,
            || execute_focus_wm_only(dir),
            || execute_connected_action(kind, dir),
        ),
        _ => execute_connected_action(kind, dir),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use anyhow::Result;

    use super::{execute_focus_with_fast_path, load_runtime_domains_for_action, FastFocusPath};
    use crate::engine::actions::context::is_no_focused_window_error;
    use crate::engine::actions::orchestrator::ActionKind;
    use crate::engine::topology::Direction;
    use crate::engine::wm::{
        ConfiguredWindowManager, FocusedWindowRecord, ResizeIntent, WindowManagerCapabilities,
        WindowManagerFeatures, WindowManagerSession, WindowRecord,
    };

    struct NoFocusedWindowSession;

    impl WindowManagerSession for NoFocusedWindowSession {
        fn adapter_name(&self) -> &'static str {
            "no-focused-window"
        }

        fn capabilities(&self) -> WindowManagerCapabilities {
            WindowManagerCapabilities::none()
        }

        fn focused_window(&mut self) -> Result<FocusedWindowRecord> {
            Err(anyhow::anyhow!("no focused window"))
        }

        fn windows(&mut self) -> Result<Vec<WindowRecord>> {
            Ok(Vec::new())
        }

        fn focus_direction(&mut self, _direction: Direction) -> Result<()> {
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

        fn focus_window_by_id(&mut self, _id: u64) -> Result<()> {
            Ok(())
        }

        fn close_window_by_id(&mut self, _id: u64) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn focus_action_domain_loading_does_not_require_focused_window() {
        let mut wm = ConfiguredWindowManager::new(
            Box::new(NoFocusedWindowSession),
            WindowManagerFeatures::default(),
        );
        let domains = load_runtime_domains_for_action(ActionKind::Focus, &mut wm)
            .expect("focus domain loading should skip missing focused window");
        assert!(domains.is_empty());
    }

    #[test]
    fn move_action_domain_loading_still_requires_focused_window() {
        let mut wm = ConfiguredWindowManager::new(
            Box::new(NoFocusedWindowSession),
            WindowManagerFeatures::default(),
        );
        let err = match load_runtime_domains_for_action(ActionKind::Move, &mut wm) {
            Ok(_) => panic!("move domain loading should still require a focused window"),
            Err(err) => err,
        };
        assert!(is_no_focused_window_error(&err));
    }

    #[test]
    fn focus_action_skips_wm_connect_when_fast_path_handles_focus() {
        let connect_called = Cell::new(false);

        execute_focus_with_fast_path(
            Direction::West,
            |_dir| Ok(FastFocusPath::Handled),
            || {
                connect_called.set(true);
                Err(anyhow::anyhow!("wm-only fallback should not run"))
            },
            || {
                connect_called.set(true);
                Err(anyhow::anyhow!("full fallback should not run"))
            },
        )
        .expect("fast focus path should short-circuit successfully");

        assert!(!connect_called.get());
    }

    #[test]
    fn focus_action_uses_wm_only_fallback_when_fast_path_proves_app_unhandled() {
        let wm_only_called = Cell::new(false);
        let full_fallback_called = Cell::new(false);

        execute_focus_with_fast_path(
            Direction::East,
            |_dir| Ok(FastFocusPath::NotHandled),
            || {
                wm_only_called.set(true);
                Ok(())
            },
            || {
                full_fallback_called.set(true);
                Ok(())
            },
        )
        .expect("wm-only fallback should run");

        assert!(wm_only_called.get());
        assert!(!full_fallback_called.get());
    }
}
