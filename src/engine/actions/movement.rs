use anyhow::{Context, Result};

use super::AppContext;
use super::{
    attempt_passthrough_merge, execute_app_tear_out, probe_directional_target_for_adapter,
    DirectionalProbeFocusMode,
};
use crate::engine::contract::{AppKind, MoveDecision, TopologyHandler};
use crate::engine::topology::Direction;
use crate::engine::window_manager::ConfiguredWindowManager;
use crate::logging;

pub(crate) fn attempt_focused_app_move(
    wm: &mut ConfiguredWindowManager,
    dir: Direction,
) -> Result<bool> {
    let _span = tracing::debug_span!("attempt_focused_app_move", dir = ?dir).entered();
    let Some(ctx) = AppContext::from_focused(wm)? else {
        return Ok(false);
    };
    let owner_pid = ctx.pid.get();
    let source_window_id = ctx.source_window_id;
    let source_tile_index = ctx.source_tile_index;
    let source_pid = Some(ctx.pid);
    let app_id = ctx.app_id.clone();
    let title = ctx.title.clone();

    let chain = ctx.resolve_chain();
    for (index, app) in chain.iter().enumerate() {
        let adapter_name = app.adapter_name();
        let decision = TopologyHandler::move_decision(app.as_ref(), dir, owner_pid)
            .with_context(|| format!("{adapter_name} move_decision failed"))?;
        match decision {
            MoveDecision::Passthrough => {
                if attempt_passthrough_merge(
                    wm,
                    app.as_ref(),
                    &chain[index + 1..],
                    &app_id,
                    &title,
                    dir,
                    source_window_id,
                    source_pid,
                )? {
                    return Ok(true);
                }
                if matches!(app.kind(), AppKind::Terminal)
                    && app.capabilities().tear_out
                    && probe_directional_target_for_adapter(
                        wm,
                        dir,
                        source_window_id,
                        adapter_name,
                        DirectionalProbeFocusMode::RestoreSource,
                    )?
                    .is_none()
                {
                    execute_app_tear_out(
                        wm,
                        app.as_ref(),
                        dir,
                        owner_pid,
                        source_window_id,
                        source_tile_index,
                        source_pid,
                        &app_id,
                        "PassthroughTearOut",
                    )?;
                    return Ok(true);
                }
            }
            MoveDecision::Internal => {
                TopologyHandler::move_internal(app.as_ref(), dir, owner_pid)
                    .with_context(|| format!("{adapter_name} move_internal failed"))?;
                logging::debug(format!(
                    "actions::move: app move handled by {adapter_name} decision=Internal"
                ));
                return Ok(true);
            }
            MoveDecision::Rearrange => {
                TopologyHandler::rearrange(app.as_ref(), dir, owner_pid)
                    .with_context(|| format!("{adapter_name} rearrange failed"))?;
                logging::debug(format!(
                    "actions::move: app move handled by {adapter_name} decision=Rearrange"
                ));
                return Ok(true);
            }
            MoveDecision::TearOut => {
                execute_app_tear_out(
                    wm,
                    app.as_ref(),
                    dir,
                    owner_pid,
                    source_window_id,
                    source_tile_index,
                    source_pid,
                    &app_id,
                    "TearOut",
                )?;
                return Ok(true);
            }
        }
    }

    Ok(false)
}
