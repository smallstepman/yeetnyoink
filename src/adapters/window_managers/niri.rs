use anyhow::{anyhow, bail, Context, Result};
use niri_ipc::socket::Socket;
use niri_ipc::{Action, Request, Response, SizeChange, Window, Workspace, WorkspaceReferenceArg};
use std::any::TypeId;

use crate::adapters::window_managers::{
    NiriAdapter, WindowManagerExecution, WindowManagerIntrospection,
};
use crate::engine::domain::PaneState;
use crate::engine::domain::{decode_native_window_ref, encode_native_window_ref};
use crate::engine::domain::{
    DomainLeafSnapshot, DomainSnapshot, ErasedDomain, TilingDomain, TopologyModifierImpl,
    TopologyProvider,
};
use crate::engine::topology::Direction;
use crate::engine::topology::{DomainId, LeafId, Rect};
use crate::logging;

pub struct Niri {
    socket: Socket,
}

impl Niri {
    pub fn connect() -> Result<Self> {
        let _span = tracing::debug_span!("niri.connect").entered();
        logging::debug("niri: connecting to IPC socket");
        let socket = Socket::connect().context("failed to connect to niri IPC socket")?;
        logging::debug("niri: IPC socket connected");
        Ok(Self { socket })
    }

    fn send_action(&mut self, action: Action) -> Result<()> {
        let _span = tracing::debug_span!("niri.send_action", action = ?action).entered();
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
        let _span = tracing::debug_span!("niri.focused_window").entered();
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
        let _span = tracing::debug_span!("niri.focus_direction", ?dir).entered();
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

    pub fn close_window_by_id(&mut self, id: u64) -> Result<()> {
        self.send_action(Action::CloseWindow { id: Some(id) })
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

pub struct NiriDomainPlugin {
    domain_id: DomainId,
    inner: NiriAdapter,
}

impl NiriDomainPlugin {
    pub fn connect(domain_id: DomainId) -> Result<Self> {
        Ok(Self {
            domain_id,
            inner: NiriAdapter::connect()?,
        })
    }

    fn snapshot_leaves(&mut self) -> Result<Vec<DomainLeafSnapshot>> {
        let windows = self.inner.windows()?;
        Ok(windows
            .iter()
            .enumerate()
            .map(|(index, window)| {
                let x = (index as i32) * 1000;
                DomainLeafSnapshot {
                    id: (index as LeafId) + 1,
                    native_id: encode_native_window_ref(window.id, window.pid),
                    rect: Rect {
                        x,
                        y: 0,
                        w: 900,
                        h: 900,
                    },
                    focused: window.is_focused,
                }
            })
            .collect())
    }
}

impl TopologyProvider for NiriDomainPlugin {
    type NativeId = Vec<u8>;
    type Error = anyhow::Error;

    fn domain_name(&self) -> &'static str {
        "niri"
    }

    fn rect(&self) -> Rect {
        Rect {
            x: 0,
            y: 0,
            w: 10000,
            h: 10000,
        }
    }

    fn fetch_layout(&mut self) -> Result<(), Self::Error> {
        let _ = self.inner.windows()?;
        Ok(())
    }
}

impl TopologyModifierImpl for NiriDomainPlugin {
    fn focus_impl(&mut self, native_id: &Self::NativeId) -> Result<(), Self::Error> {
        let target = decode_native_window_ref(native_id).context("invalid niri native id")?;
        self.inner.focus_window_by_id(target.window_id)
    }

    fn move_impl(&mut self, native_id: &Self::NativeId, dir: Direction) -> Result<(), Self::Error> {
        let target = decode_native_window_ref(native_id).context("invalid niri native id")?;
        self.inner.focus_window_by_id(target.window_id)?;
        self.inner.move_direction(dir)
    }

    fn tear_off_impl(&mut self, _id: &Self::NativeId) -> Result<Box<dyn PaneState>, Self::Error> {
        Err(anyhow!("niri domain does not support payload tear-off"))
    }

    fn merge_in_impl(
        &mut self,
        _target: &Self::NativeId,
        _dir: Direction,
        _payload: Box<dyn PaneState>,
    ) -> Result<Self::NativeId, Self::Error> {
        Err(anyhow!("niri domain does not support payload merge-in"))
    }
}

impl TilingDomain for NiriDomainPlugin {
    fn supported_payload_types(&self) -> &'static [TypeId] {
        &[]
    }
}

impl ErasedDomain for NiriDomainPlugin {
    fn domain_id(&self) -> DomainId {
        self.domain_id
    }

    fn domain_name(&self) -> &'static str {
        "niri"
    }

    fn rect(&self) -> Rect {
        TopologyProvider::rect(self)
    }

    fn fetch_snapshot(&mut self) -> Result<DomainSnapshot> {
        Ok(DomainSnapshot {
            domain_id: self.domain_id,
            rect: TopologyProvider::rect(self),
            leaves: self.snapshot_leaves()?,
        })
    }

    fn supported_payload_types(&self) -> Vec<TypeId> {
        vec![]
    }

    fn tear_off(&mut self, native_id: &[u8]) -> Result<Box<dyn PaneState>> {
        self.tear_off_impl(&native_id.to_vec())
    }

    fn merge_in(
        &mut self,
        target_native_id: &[u8],
        dir: Direction,
        payload: Box<dyn PaneState>,
    ) -> Result<Vec<u8>> {
        self.merge_in_impl(&target_native_id.to_vec(), dir, payload)
    }
}
