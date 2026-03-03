use anyhow::Result;
use clap::ValueEnum;

use crate::adapters::window_managers::connect_selected;
use crate::engine::topology::Direction;
use crate::engine::domain::runtime_domains_for_window_manager;
use crate::engine::orchestrator::{ActionKind, ActionRequest, Orchestrator};
use crate::logging;

const DEFAULT_RESIZE_STEP: i32 = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ResizeMode {
    Grow,
    Shrink,
}

pub fn run(dir: Direction, mode: ResizeMode) -> Result<()> {
    let grow = matches!(mode, ResizeMode::Grow);
    logging::debug(format!("resize: dir={} mode={:?}", dir, mode));
    let mut wm = connect_selected()?;
    let mut orchestrator = Orchestrator::default();
    for domain in runtime_domains_for_window_manager(&mut wm)? {
        orchestrator.register_domain(domain);
    }
    orchestrator.execute(
        &mut wm,
        ActionRequest {
            kind: ActionKind::Resize {
                grow,
                step: DEFAULT_RESIZE_STEP,
            },
            direction: dir,
        },
    )
}
