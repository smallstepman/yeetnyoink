use anyhow::{Context, Result};

use super::context::FocusedAppSession;
use super::merge::PassthroughMergeContext;
use super::tearout::TearOutRequest;
use super::DirectionalProbeFocusMode;
use super::probe::DirectionalWindowProbe;
use crate::engine::contract::{AppAdapter, AppKind, MoveDecision, TopologyHandler};
use crate::engine::topology::Direction;
use crate::engine::window_manager::ConfiguredWindowManager;
use crate::logging;

// ── MoveExecution ─────────────────────────────────────────────────────────────

/// Owns a single move session: the already-resolved focused-app snapshot and
/// the window manager reference.  `run` iterates the adapter chain and
/// delegates detailed mechanics to [`PassthroughMergeContext`] and
/// [`TearOutRequest`], keeping this file readable as a policy loop.
struct MoveExecution<'a> {
    wm: &'a mut ConfiguredWindowManager,
    session: &'a FocusedAppSession,
    dir: Direction,
}

impl MoveExecution<'_> {
    fn run(self) -> Result<bool> {
        let Self { wm, session, dir } = self;
        for (index, app) in session.chain.iter().enumerate() {
            if handle_move_decision(wm, session, index, app.as_ref(), dir)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

/// Per-adapter routing for a single step in the move chain.  Returns `true`
/// when the move was handled and the loop should stop.
fn handle_move_decision(
    wm: &mut ConfiguredWindowManager,
    session: &FocusedAppSession,
    index: usize,
    app: &dyn AppAdapter,
    dir: Direction,
) -> Result<bool> {
    let adapter_name = app.adapter_name();
    let owner_pid = session.pid.get();
    let decision = TopologyHandler::move_decision(app, dir, owner_pid)
        .with_context(|| format!("{adapter_name} move_decision failed"))?;

    match decision {
        MoveDecision::Passthrough => {
            if (PassthroughMergeContext {
                app,
                session,
                outer_chain: &session.chain[index + 1..],
                dir,
            })
            .run(wm)?
            {
                return Ok(true);
            }
            if matches!(app.kind(), AppKind::Terminal)
                && app.capabilities().tear_out
                && DirectionalWindowProbe::new(wm, session.source_window_id)
                    .window_matching_adapter(
                        dir,
                        adapter_name,
                        DirectionalProbeFocusMode::RestoreSource,
                    )?
                    .is_none()
            {
                TearOutRequest {
                    app,
                    session,
                    dir,
                    decision_label: "PassthroughTearOut",
                }
                .run(wm)?;
                return Ok(true);
            }
            Ok(false)
        }
        MoveDecision::Internal => {
            TopologyHandler::move_internal(app, dir, owner_pid)
                .with_context(|| format!("{adapter_name} move_internal failed"))?;
            logging::debug(format!(
                "actions::move: app move handled by {adapter_name} decision=Internal"
            ));
            Ok(true)
        }
        MoveDecision::Rearrange => {
            TopologyHandler::rearrange(app, dir, owner_pid)
                .with_context(|| format!("{adapter_name} rearrange failed"))?;
            logging::debug(format!(
                "actions::move: app move handled by {adapter_name} decision=Rearrange"
            ));
            Ok(true)
        }
        MoveDecision::TearOut => {
            TearOutRequest {
                app,
                session,
                dir,
                decision_label: "TearOut",
            }
            .run(wm)?;
            Ok(true)
        }
    }
}

pub(crate) fn attempt_focused_app_move(
    wm: &mut ConfiguredWindowManager,
    dir: Direction,
) -> Result<bool> {
    let _span = tracing::debug_span!("attempt_focused_app_move", dir = ?dir).entered();
    let focused = wm.focused_window()?;
    let Some(pid) = focused.pid else {
        return Ok(false);
    };
    let app_id = focused.app_id.unwrap_or_default();
    let title = focused.title.unwrap_or_default();
    let chain = crate::engine::chain_resolver::resolve_app_chain(&app_id, pid.get(), &title);
    let session = FocusedAppSession {
        source_window_id: focused.id,
        source_tile_index: focused.original_tile_index,
        pid,
        app_id,
        title,
        chain,
    };
    MoveExecution { wm, session: &session, dir }.run()
}
