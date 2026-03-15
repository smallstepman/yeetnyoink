use anyhow::Result;

use crate::engine::actions::orchestrator::ActionKind;
use crate::engine::topology::Direction;
use crate::logging;

pub fn run(dir: Direction) -> Result<()> {
    logging::debug(format!("focus: dir={dir}"));
    super::run_action(ActionKind::Focus, dir)
}
