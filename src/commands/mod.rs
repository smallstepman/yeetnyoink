pub mod focus;
#[cfg(any(test, target_os = "linux"))]
pub mod focus_or_cycle;
pub mod move_win;
pub mod resize;

use anyhow::Result;

use crate::engine::actions::orchestrator::{ActionKind, ActionRequest, Orchestrator};
use crate::engine::topology::Direction;
use crate::engine::transfer::bridge::runtime_domains_for_window_manager;
use crate::engine::wm::connect_selected;

/// Shared runner for simple action commands (focus, move).
pub(crate) fn run_action(kind: ActionKind, dir: Direction) -> Result<()> {
    let _span = tracing::debug_span!("commands.run_action", ?kind, ?dir).entered();
    let mut wm = connect_selected()?;
    let mut orchestrator = Orchestrator::default();
    let domains = {
        let _span = tracing::debug_span!("commands.load_domains").entered();
        runtime_domains_for_window_manager(&mut wm)?
    };
    for domain in domains {
        orchestrator.register_domain(domain);
    }
    {
        let _span = tracing::debug_span!("commands.execute_action").entered();
        orchestrator.execute(&mut wm, ActionRequest::new(kind, dir))
    }
}
