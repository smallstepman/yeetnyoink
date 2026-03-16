pub mod browser_host;
pub mod focus;
#[cfg(any(test, target_os = "linux"))]
pub mod focus_or_cycle;
pub mod move_win;
pub mod resize;
pub mod setup;

use anyhow::Result;

use crate::engine::actions::orchestrator::{ActionKind, ActionRequest, Orchestrator};
use crate::engine::topology::Direction;
use crate::engine::transfer::bridge::runtime_domains_for_window_manager;
use crate::engine::transfer::ErasedDomain;
use crate::engine::wm::connect_selected;
use crate::logging;

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

/// Shared runner for simple action commands (focus, move).
pub(crate) fn run_action(kind: ActionKind, dir: Direction) -> Result<()> {
    let _span = tracing::debug_span!("commands.run_action", ?kind, ?dir).entered();
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

#[cfg(test)]
mod tests {
    use anyhow::Result;

    use super::load_runtime_domains_for_action;
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
}
