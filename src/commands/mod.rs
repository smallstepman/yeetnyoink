pub mod focus;
pub mod focus_or_cycle;
pub mod move_win;
pub mod resize;

use anyhow::Result;

use crate::adapters::window_managers::connect_selected;
use crate::engine::topology::Direction;
use crate::engine::domain::runtime_domains_for_window_manager;
use crate::engine::orchestrator::{ActionKind, ActionRequest, Orchestrator};

/// Shared runner for simple action commands (focus, move).
pub(crate) fn run_action(kind: ActionKind, dir: Direction) -> Result<()> {
    let mut wm = connect_selected()?;
    let mut orchestrator = Orchestrator::default();
    for domain in runtime_domains_for_window_manager(&mut wm)? {
        orchestrator.register_domain(domain);
    }
    orchestrator.execute(
        &mut wm,
        ActionRequest {
            kind,
            direction: dir,
        },
    )
}
