use anyhow::{bail, Context, Result};
use niri_ipc::socket::Socket;
use niri_ipc::{Action, Request, Response, SizeChange, Window, Workspace, WorkspaceReferenceArg};

use crate::engine::direction::Direction;
use crate::logging;

pub struct Niri {
    socket: Socket,
}

impl Niri {
    pub fn connect() -> Result<Self> {
        logging::debug("niri: connecting to IPC socket");
        let socket = Socket::connect().context("failed to connect to niri IPC socket")?;
        logging::debug("niri: IPC socket connected");
        Ok(Self { socket })
    }

    fn send_action(&mut self, action: Action) -> Result<()> {
        logging::debug(format!("niri: action request = {:?}", action));
        let reply = self
            .socket
            .send(Request::Action(action))
            .context("failed to send niri action")?;
        match reply {
            Ok(Response::Handled) => {
                logging::debug("niri: action handled");
                Ok(())
            }
            Ok(other) => bail!("unexpected response: {:?}", other),
            Err(e) => bail!("niri error: {e}"),
        }
    }

    pub fn focused_window(&mut self) -> Result<Window> {
        logging::debug("niri: requesting focused window");
        let reply = self
            .socket
            .send(Request::FocusedWindow)
            .context("failed to send FocusedWindow request")?;
        match reply {
            Ok(Response::FocusedWindow(Some(w))) => {
                logging::debug(format!(
                    "niri: focused window id={} app_id={:?} pid={:?}",
                    w.id, w.app_id, w.pid
                ));
                Ok(w)
            }
            Ok(Response::FocusedWindow(None)) => bail!("no focused window"),
            Ok(other) => bail!("unexpected response: {:?}", other),
            Err(e) => bail!("niri error: {e}"),
        }
    }

    pub fn windows(&mut self) -> Result<Vec<Window>> {
        let reply = self
            .socket
            .send(Request::Windows)
            .context("failed to send Windows request")?;
        match reply {
            Ok(Response::Windows(windows)) => Ok(windows),
            Ok(other) => bail!("unexpected response: {:?}", other),
            Err(e) => bail!("niri error: {e}"),
        }
    }

    pub fn workspaces(&mut self) -> Result<Vec<Workspace>> {
        let reply = self
            .socket
            .send(Request::Workspaces)
            .context("failed to send Workspaces request")?;
        match reply {
            Ok(Response::Workspaces(workspaces)) => Ok(workspaces),
            Ok(other) => bail!("unexpected response: {:?}", other),
            Err(e) => bail!("niri error: {e}"),
        }
    }

    pub fn focus_direction(&mut self, dir: Direction) -> Result<()> {
        let action = match dir {
            Direction::West => Action::FocusColumnLeft {},
            Direction::East => Action::FocusColumnRight {},
            Direction::North => Action::FocusWindowOrWorkspaceUp {},
            Direction::South => Action::FocusWindowOrWorkspaceDown {},
        };
        self.send_action(action)
    }

    pub fn move_column(&mut self, dir: Direction) -> Result<()> {
        let action = match dir {
            Direction::West => Action::MoveColumnLeft {},
            Direction::East => Action::MoveColumnRight {},
            _ => return Ok(()),
        };
        self.send_action(action)
    }

    pub fn move_direction(&mut self, dir: Direction) -> Result<()> {
        let action = match dir {
            Direction::West => Action::ConsumeOrExpelWindowLeft { id: None },
            Direction::East => Action::ConsumeOrExpelWindowRight { id: None },
            Direction::North => Action::MoveWindowUpOrToWorkspaceUp {},
            Direction::South => Action::MoveWindowDownOrToWorkspaceDown {},
        };
        self.send_action(action)
    }

    pub fn resize_window(&mut self, dir: Direction, grow: bool, step: i32) -> Result<()> {
        let magnitude = step.abs().max(1);
        let directional_delta = match dir {
            Direction::East | Direction::South => magnitude,
            Direction::West | Direction::North => -magnitude,
        };
        let delta = if grow {
            directional_delta
        } else {
            -directional_delta
        };
        let change = SizeChange::AdjustFixed(delta);
        let action = match dir {
            Direction::West | Direction::East => Action::SetWindowWidth { id: None, change },
            Direction::North | Direction::South => Action::SetWindowHeight { id: None, change },
        };
        self.send_action(action)
    }

    pub fn spawn(&mut self, command: Vec<String>) -> Result<()> {
        self.send_action(Action::Spawn { command })
    }

    pub fn spawn_sh(&mut self, command: String) -> Result<()> {
        self.send_action(Action::SpawnSh { command })
    }

    pub fn focus_window_by_id(&mut self, id: u64) -> Result<()> {
        self.send_action(Action::FocusWindow { id })
    }

    pub fn move_window_to_workspace(
        &mut self,
        window_id: u64,
        reference: WorkspaceReferenceArg,
        focus: bool,
    ) -> Result<()> {
        self.send_action(Action::MoveWindowToWorkspace {
            window_id: Some(window_id),
            reference,
            focus,
        })
    }

    pub fn move_window_to_monitor(&mut self, id: u64, output: String) -> Result<()> {
        self.send_action(Action::MoveWindowToMonitor {
            id: Some(id),
            output,
        })
    }

    /// After a new tile is created (and focused) to the right of the original,
    /// consume it into the original's column and position it right next to
    /// the original tile (below for south, above for north).
    ///
    /// `original_tile_idx` is the 1-based tile index of the original window
    /// in its column (from `layout.pos_in_scrolling_layout`).
    pub fn consume_into_column_and_move(
        &mut self,
        dir: Direction,
        original_tile_idx: usize,
    ) -> Result<()> {
        // The new tile is in its own column to the right of the original.
        // Consume it leftward into the original's column (goes to bottom).
        self.send_action(Action::ConsumeOrExpelWindowLeft { id: None })?;

        // B is now at the bottom of the column. Query its position.
        let new_window = self.focused_window()?;
        let new_tile_idx = new_window
            .layout
            .pos_in_scrolling_layout
            .map(|(_, t)| t)
            .unwrap_or(1);

        // Target position: right below the original for south, at the
        // original's position (pushing it down) for north.
        let target_idx = match dir {
            Direction::South => original_tile_idx + 1,
            Direction::North => original_tile_idx,
            _ => new_tile_idx, // no-op
        };

        // Move up from bottom to target.
        for _ in 0..new_tile_idx.saturating_sub(target_idx) {
            self.send_action(Action::MoveWindowUp {})?;
        }
        Ok(())
    }
}
