use anyhow::{Context, Result};

use super::context::{is_no_focused_window_error, with_focused_app_session};
use crate::engine::contracts::TopologyHandler;
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
    if TopologyHandler::can_focus(app, dir, owner_pid)
        .with_context(|| format!("{adapter_name} can_focus failed"))?
    {
        TopologyHandler::focus(app, dir, owner_pid)
            .with_context(|| format!("{adapter_name} focus failed"))?;
        logging::debug(format!(
            "actions::focus: app focus handled by {adapter_name}"
        ));
        return Ok(true);
    }
    Ok(false)
}

pub(crate) fn attempt_focused_app_focus(
    wm: &mut ConfiguredWindowManager,
    dir: Direction,
) -> Result<bool> {
    let _span = tracing::debug_span!("actions::focus.attempt_focused_app_focus", ?dir).entered();
    let result = match with_focused_app_session(wm, |session| {
        let owner_pid = session.pid.get();
        for app in session.chain {
            if attempt_focus_adapter(app.as_ref(), owner_pid, dir)? {
                return Ok(true);
            }
        }
        Ok(false)
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
    use anyhow::Result;

    use super::attempt_focused_app_focus;
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
}
