use anyhow::{Context, Result};

use super::context::{is_no_focused_window_error, with_focused_app_session};
use crate::engine::contracts::TopologyHandler;
use crate::engine::resolution::chain::resolve_root_adapter;
use crate::engine::wm::FocusedAppRecord;
use crate::engine::topology::Direction;
use crate::engine::wm::ConfiguredWindowManager;
use crate::logging;

fn attempt_focus_adapter(
    app: &dyn crate::engine::contracts::AppAdapter,
    owner_pid: u32,
    dir: Direction,
) -> Result<bool> {
    if !app.capabilities().focus {
        return Ok(false);
    }
    let adapter_name = app.adapter_name();
    let _span = tracing::debug_span!(
        "actions.focus.adapter",
        adapter = adapter_name,
        pid = owner_pid,
        ?dir
    )
    .entered();
    if TopologyHandler::focus_if_possible(app, dir, owner_pid)
        .with_context(|| format!("{adapter_name} focus failed"))?
    {
        logging::debug(format!(
            "actions::focus: app focus handled by {adapter_name}"
        ));
        return Ok(true);
    }
    Ok(false)
}

fn attempt_focus_with_chain(
    chain: Vec<Box<dyn crate::engine::contracts::AppAdapter>>,
    owner_pid: u32,
    dir: Direction,
) -> Result<bool> {
    for app in chain {
        if attempt_focus_adapter(app.as_ref(), owner_pid, dir)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn attempt_focus_with_root_and_chain(
    root_adapter: Option<Box<dyn crate::engine::contracts::AppAdapter>>,
    resolve_chain: impl FnOnce() -> Vec<Box<dyn crate::engine::contracts::AppAdapter>>,
    owner_pid: u32,
    dir: Direction,
) -> Result<bool> {
    let root_adapter_name = match root_adapter {
        Some(root_adapter) => {
            let name = root_adapter.adapter_name();
            if attempt_focus_adapter(root_adapter.as_ref(), owner_pid, dir)? {
                return Ok(true);
            }
            Some(name)
        }
        None => None,
    };

    let mut chain = resolve_chain();
    if let Some(root_name) = root_adapter_name {
        if chain
            .first()
            .is_some_and(|app| app.adapter_name() == root_name)
        {
            chain.remove(0);
        }
    }
    attempt_focus_with_chain(chain, owner_pid, dir)
}

pub(crate) fn attempt_focused_app_focus_from_record(
    focused: FocusedAppRecord,
    dir: Direction,
) -> Result<bool> {
    let owner_pid = focused.pid.get();
    let root_adapter = resolve_root_adapter(&focused.app_id);
    attempt_focus_with_root_and_chain(
        root_adapter,
        || {
            let _span = tracing::debug_span!(
                "actions.context.resolve_chain",
                app_id = focused.app_id.as_str(),
                pid = owner_pid
            )
            .entered();
            crate::engine::resolution::resolve_app_chain(&focused.app_id, owner_pid, &focused.title)
        },
        owner_pid,
        dir,
    )
}

pub(crate) fn attempt_focused_app_focus(
    wm: &mut ConfiguredWindowManager,
    dir: Direction,
) -> Result<bool> {
    let _span = tracing::debug_span!("actions::focus.attempt_focused_app_focus", ?dir).entered();
    let result = match with_focused_app_session(wm, |session| {
        attempt_focus_with_chain(session.chain, session.pid.get(), dir)
    }) {
        Ok(result) => result,
        Err(err) if is_no_focused_window_error(&err) => {
            logging::debug(
                "actions::focus: no focused window available; skipping app focus and falling back to WM",
            );
            return Ok(false);
        }
        Err(err) => return Err(err),
    };
    Ok(result.unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use anyhow::Result;

    use super::{attempt_focus_with_root_and_chain, attempt_focused_app_focus};
    use crate::engine::contracts::{AppAdapter, AppCapabilities, AppKind, TopologyHandler};
    use crate::engine::topology::Direction;
    use crate::engine::wm::{
        ConfiguredWindowManager, FocusedWindowRecord, ResizeIntent, WindowManagerCapabilities,
        WindowManagerFeatures, WindowManagerSession, WindowRecord,
    };

    struct NoFocusedWindowSession;

    impl WindowManagerSession for NoFocusedWindowSession {
        fn adapter_name(&self) -> &'static str {
            "no-focused-window"
        }

        fn capabilities(&self) -> WindowManagerCapabilities {
            WindowManagerCapabilities::none()
        }

        fn focused_window(&mut self) -> Result<FocusedWindowRecord> {
            Err(anyhow::anyhow!("no focused window"))
        }

        fn windows(&mut self) -> Result<Vec<WindowRecord>> {
            Ok(Vec::new())
        }

        fn focus_direction(&mut self, _direction: Direction) -> Result<()> {
            Ok(())
        }

        fn move_direction(&mut self, _direction: Direction) -> Result<()> {
            Ok(())
        }

        fn resize_with_intent(&mut self, _intent: ResizeIntent) -> Result<()> {
            Ok(())
        }

        fn spawn(&mut self, _command: Vec<String>) -> Result<()> {
            Ok(())
        }

        fn focus_window_by_id(&mut self, _id: u64) -> Result<()> {
            Ok(())
        }

        fn close_window_by_id(&mut self, _id: u64) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn attempt_focused_app_focus_returns_false_when_wm_has_no_focused_window() {
        let mut wm = ConfiguredWindowManager::new(
            Box::new(NoFocusedWindowSession),
            WindowManagerFeatures::default(),
        );
        let handled = attempt_focused_app_focus(&mut wm, Direction::North)
            .expect("focus attempt should not error");
        assert!(!handled);
    }

    struct FocusHandlingAdapter;

    impl AppAdapter for FocusHandlingAdapter {
        fn adapter_name(&self) -> &'static str {
            "focus-handling"
        }

        fn kind(&self) -> AppKind {
            AppKind::Terminal
        }

        fn capabilities(&self) -> AppCapabilities {
            AppCapabilities {
                probe: false,
                focus: true,
                move_internal: false,
                resize_internal: false,
                rearrange: false,
                tear_out: false,
                merge: false,
            }
        }
    }

    impl TopologyHandler for FocusHandlingAdapter {
        fn can_focus(&self, _dir: Direction, _pid: u32) -> Result<bool> {
            Ok(true)
        }

        fn focus(&self, _dir: Direction, _pid: u32) -> Result<()> {
            Ok(())
        }

        fn move_internal(&self, _dir: Direction, _pid: u32) -> Result<()> {
            Ok(())
        }

        fn move_out(
            &self,
            _dir: Direction,
            _pid: u32,
        ) -> Result<crate::engine::contracts::TearResult> {
            Ok(crate::engine::contracts::TearResult { spawn_command: None })
        }
    }

    #[test]
    fn root_focus_short_circuits_before_resolving_chain() {
        let chain_resolved = Cell::new(false);

        let handled = attempt_focus_with_root_and_chain(
            Some(Box::new(FocusHandlingAdapter)),
            || {
                chain_resolved.set(true);
                Vec::new()
            },
            42,
            Direction::East,
        )
        .expect("root focus should succeed");

        assert!(handled);
        assert!(!chain_resolved.get());
    }
}
