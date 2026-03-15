use anyhow::Result;

use crate::engine::actions::orchestrator::ActionKind;
use crate::engine::topology::Direction;
use crate::logging;

pub fn run(dir: Direction) -> Result<()> {
    logging::debug(format!("move: dir={dir}"));
    super::run_action(ActionKind::Move, dir)
}
