use anyhow::{Context, Result};

use super::context::with_focused_app_session;
use crate::engine::contract::TopologyHandler;
use crate::engine::topology::Direction;
use crate::engine::window_manager::ConfiguredWindowManager;
use crate::logging;

pub(crate) fn attempt_focused_app_resize(
    wm: &mut ConfiguredWindowManager,
    dir: Direction,
    grow: bool,
    step: i32,
) -> Result<bool> {
    let _span = tracing::debug_span!("attempt_focused_app_resize", dir = ?dir).entered();
    let result = with_focused_app_session(wm, |session| {
        let owner_pid = session.pid.get();
        for app in session.chain {
            if !app.capabilities().resize_internal {
                continue;
            }
            let adapter_name = app.adapter_name();
            if TopologyHandler::can_resize(app.as_ref(), dir, grow, owner_pid)
                .with_context(|| format!("{adapter_name} can_resize failed"))?
            {
                TopologyHandler::resize_internal(app.as_ref(), dir, grow, step, owner_pid)
                    .with_context(|| format!("{adapter_name} resize_internal failed"))?;
                logging::debug(format!(
                    "actions::resize: app resize handled by {adapter_name}"
                ));
                return Ok(true);
            }
        }
        Ok(false)
    })?;
    Ok(result.unwrap_or(false))
}
