use anyhow::{Context, Result};

use super::AppContext;
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
    let Some(ctx) = AppContext::from_focused(wm)? else {
        return Ok(false);
    };
    let owner_pid = ctx.pid.get();

    for app in ctx.resolve_chain() {
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
}
