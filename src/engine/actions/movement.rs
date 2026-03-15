use anyhow::{Context, Result};

use super::context::{with_focused_app_session, FocusedAppSession};
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
pub(crate) struct MoveExecution<'a> {
    pub(crate) wm: &'a mut ConfiguredWindowManager,
    pub(crate) session: &'a FocusedAppSession,
    pub(crate) dir: Direction,
}

impl MoveExecution<'_> {
    pub(crate) fn run(self) -> Result<bool> {
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
    let session_opt = with_focused_app_session(wm, |s| Ok(s))?;
    let Some(session) = session_opt else {
        return Ok(false);
    };
    MoveExecution { wm, session: &session, dir }.run()
}

// ── structural regression tests ───────────────────────────────────────────────

#[cfg(test)]
mod tests {
    /// The move entrypoint must delegate focused-session setup to
    /// `with_focused_app_session` rather than duplicating the preamble inline.
    ///
    /// The assertion is scoped to the *body* of `attempt_focused_app_move` by
    /// splitting on the function signature.  This ensures the check cannot be
    /// satisfied by the `use` import at the top of the file, which also
    /// contains the string `with_focused_app_session` and would allow a
    /// regression to slip through undetected.
    #[test]
    fn attempt_focused_app_move_uses_shared_session_helper() {
        let source = include_str!("movement.rs");
        let impl_src = source
            .split_once("#[cfg(test)]")
            .map(|(impl_part, _)| impl_part)
            .expect("movement.rs must contain a test module");

        // Isolate everything from the function signature onward, excluding
        // the import section where `with_focused_app_session` also appears.
        let fn_body = impl_src
            .split_once("fn attempt_focused_app_move(")
            .map(|(_, rest)| rest)
            .expect("attempt_focused_app_move must be defined in movement.rs");

        assert!(
            fn_body.contains("with_focused_app_session("),
            "attempt_focused_app_move must call with_focused_app_session(...) \
             in its body, not merely import it"
        );
        assert!(
            !fn_body.contains("focused_window("),
            "attempt_focused_app_move must not duplicate focused_window() preamble; \
             use with_focused_app_session instead"
        );
        assert!(
            !fn_body.contains("resolve_app_chain("),
            "attempt_focused_app_move must not duplicate resolve_app_chain() preamble; \
             use with_focused_app_session instead"
        );
    }

    /// `AppContext` and `walk_chain` are dead context scaffolding that must be
    /// removed once the move path uses the shared session helper.
    #[test]
    fn obsolete_context_scaffolding_is_removed() {
        let source = include_str!("context.rs");
        let impl_src = source
            .split_once("#[cfg(test)]")
            .map(|(impl_part, _)| impl_part)
            .expect("context.rs must contain a test module");

        assert!(
            !impl_src.contains("struct AppContext"),
            "AppContext must be removed from context.rs"
        );
        assert!(
            !impl_src.contains("fn walk_chain"),
            "walk_chain must be removed from context.rs"
        );

        let mod_source = include_str!("mod.rs");
        assert!(
            !mod_source.contains("AppContext"),
            "mod.rs must not re-export the obsolete AppContext"
        );
        assert!(
            !mod_source.contains("walk_chain"),
            "mod.rs must not re-export the obsolete walk_chain"
        );
    }
}
