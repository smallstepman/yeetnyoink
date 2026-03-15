use anyhow::{Context, Result};

use super::with_focused_app_session;
use crate::engine::contract::TopologyHandler;
use crate::engine::topology::Direction;
use crate::engine::window_manager::ConfiguredWindowManager;
use crate::logging;

pub(crate) fn attempt_focused_app_focus(
    wm: &mut ConfiguredWindowManager,
    dir: Direction,
) -> Result<bool> {
    let _span =
        tracing::debug_span!("actions::focus.attempt_focused_app_focus", ?dir).entered();
    let result = with_focused_app_session(wm, |session| {
        let owner_pid = session.pid.get();
        for app in session.chain {
            if !app.capabilities().focus {
                continue;
            }
            let adapter_name = app.adapter_name();
            if TopologyHandler::can_focus(app.as_ref(), dir, owner_pid)
                .with_context(|| format!("{adapter_name} can_focus failed"))?
            {
                TopologyHandler::focus(app.as_ref(), dir, owner_pid)
                    .with_context(|| format!("{adapter_name} focus failed"))?;
                logging::debug(format!(
                    "actions::focus: app focus handled by {adapter_name}"
                ));
                return Ok(true);
            }
        }
        Ok(false)
    })?;
    Ok(result.unwrap_or(false))
}
