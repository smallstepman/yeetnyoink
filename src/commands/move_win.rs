use anyhow::Result;

use crate::engine::direction::Direction;
use crate::engine::orchestrator::ActionKind;
use crate::logging;

pub fn run(dir: Direction) -> Result<()> {
    logging::debug(format!("move: dir={dir}"));
    super::run_action(ActionKind::Move, dir)
}
