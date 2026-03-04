use anyhow::{anyhow, Result};

use crate::engine::contract::{
    AdapterCapabilities, MoveDecision, TearResult, TerminalMuxProvider, TopologyHandler,
};
use crate::engine::topology::Direction;

#[derive(Debug, Clone, Copy, Default)]
pub struct KittyMuxProvider;

pub(crate) static KITTY_MUX_PROVIDER: KittyMuxProvider = KittyMuxProvider;

impl TerminalMuxProvider for KittyMuxProvider {
    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            probe: true,
            focus: false,
            move_internal: false,
            resize_internal: false,
            rearrange: false,
            tear_out: false,
            merge: false,
        }
    }

    fn focused_pane_for_pid(&self, _pid: u32) -> Result<u64> {
        Err(unsupported_kitty_backend())
    }

    fn pane_neighbor_for_pid(&self, _pid: u32, _pane_id: u64, _dir: Direction) -> Result<u64> {
        Err(unsupported_kitty_backend())
    }

    fn send_text_to_pane(&self, _pid: u32, _pane_id: u64, _text: &str) -> Result<()> {
        Err(unsupported_kitty_backend())
    }

    fn mux_attach_args(&self, target: String) -> Option<Vec<String>> {
        Some(vec!["kitty".into(), target])
    }

    fn merge_source_pane_into_focused_target(
        &self,
        _source_pid: u32,
        _source_pane_id: u64,
        _target_pid: u32,
        _target_window_id: Option<u64>,
        _dir: Direction,
    ) -> Result<()> {
        Err(unsupported_kitty_backend())
    }

    fn active_foreground_process(&self, _pid: u32) -> Option<String> {
        None
    }
}

impl TopologyHandler for KittyMuxProvider {
    fn can_focus(&self, _dir: Direction, _pid: u32) -> Result<bool> {
        Err(unsupported_kitty_backend())
    }

    fn move_decision(&self, _dir: Direction, _pid: u32) -> Result<MoveDecision> {
        Err(unsupported_kitty_backend())
    }

    fn focus(&self, _dir: Direction, _pid: u32) -> Result<()> {
        Err(unsupported_kitty_backend())
    }

    fn move_internal(&self, _dir: Direction, _pid: u32) -> Result<()> {
        Err(unsupported_kitty_backend())
    }

    fn move_out(&self, _dir: Direction, _pid: u32) -> Result<TearResult> {
        Err(unsupported_kitty_backend())
    }
}

fn unsupported_kitty_backend() -> anyhow::Error {
    anyhow!("kitty mux backend is kitty-terminal specific and not implemented here")
}
