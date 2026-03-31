use std::path::Path;

use anyhow::Result;

use crate::engine::actions::orchestrator::ActionKind;
use crate::engine::topology::Direction;
use crate::logging;

pub fn run(dir: Direction) -> Result<()> {
    let helper_socket = super::warm_helper::focus_forward_socket_path()?;
    run_with_strategy(
        dir,
        helper_socket.as_deref(),
        |direction, socket| {
            logging::debug(format!(
                "focus: forwarding via warm helper socket={}",
                socket.display()
            ));
            super::warm_helper::forward_focus_request(socket, direction)
        },
        run_local,
    )
}

pub(crate) fn run_local(dir: Direction) -> Result<()> {
    logging::debug(format!("focus: dir={dir}"));
    super::run_action(ActionKind::Focus, dir)
}

fn run_with_strategy<F, L>(
    dir: Direction,
    helper_socket: Option<&Path>,
    forward_focus: F,
    run_local: L,
) -> Result<()>
where
    F: FnOnce(Direction, &Path) -> Result<()>,
    L: FnOnce(Direction) -> Result<()>,
{
    match helper_socket {
        Some(socket) => forward_focus(dir, socket),
        None => run_local(dir),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::path::Path;

    use super::run_with_strategy;
    use crate::commands::warm_helper;
    use crate::engine::topology::Direction;

    #[test]
    fn focus_uses_local_path_when_helper_socket_env_is_absent() {
        let guard = warm_helper::tests::warm_helper_socket_env_guard();
        guard.remove();

        let local_called = Cell::new(false);
        let forward_called = Cell::new(false);
        let helper_socket = warm_helper::focus_forward_socket_path()
            .expect("missing helper env should preserve local focus");

        run_with_strategy(
            Direction::West,
            helper_socket.as_deref(),
            |_dir, _socket| {
                forward_called.set(true);
                Ok(())
            },
            |dir| {
                local_called.set(true);
                assert_eq!(dir, Direction::West);
                Ok(())
            },
        )
        .expect("focus without helper env should use local path");

        assert!(local_called.get());
        assert!(!forward_called.get());
    }

    #[test]
    fn focus_forwards_when_helper_socket_env_is_set() {
        let guard = warm_helper::tests::warm_helper_socket_env_guard();
        guard.set("/tmp/yny-focus-forward.sock");

        let local_called = Cell::new(false);
        let forward_called = Cell::new(false);
        let helper_socket = warm_helper::focus_forward_socket_path()
            .expect("configured helper env should be accepted");

        run_with_strategy(
            Direction::East,
            helper_socket.as_deref(),
            |dir, socket| {
                forward_called.set(true);
                assert_eq!(dir, Direction::East);
                assert_eq!(socket, Path::new("/tmp/yny-focus-forward.sock"));
                Ok(())
            },
            |_dir| {
                local_called.set(true);
                Ok(())
            },
        )
        .expect("focus with helper env should forward");

        assert!(forward_called.get());
        assert!(!local_called.get());
    }

    #[test]
    fn focus_does_not_fall_back_to_local_path_when_forwarding_fails() {
        let guard = warm_helper::tests::warm_helper_socket_env_guard();
        guard.set("/tmp/yny-focus-forward.sock");

        let local_called = Cell::new(false);
        let forward_called = Cell::new(false);
        let helper_socket = warm_helper::focus_forward_socket_path()
            .expect("configured helper env should be accepted");

        let err = run_with_strategy(
            Direction::North,
            helper_socket.as_deref(),
            |dir, socket| {
                forward_called.set(true);
                assert_eq!(dir, Direction::North);
                assert_eq!(socket, Path::new("/tmp/yny-focus-forward.sock"));
                Err(anyhow::anyhow!("helper forwarding failed closed"))
            },
            |_dir| {
                local_called.set(true);
                Ok(())
            },
        )
        .expect_err("forwarding failure should not fall back to local focus path");

        assert!(forward_called.get());
        assert!(!local_called.get());
        assert!(
            err.to_string().contains("failed closed"),
            "unexpected error: {err:#}"
        );
    }
}
