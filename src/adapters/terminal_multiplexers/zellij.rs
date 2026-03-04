use anyhow::{anyhow, Result};

use crate::engine::contract::{
    AdapterCapabilities, MoveDecision, TearResult, TerminalMultiplexerProvider, TopologyHandler,
};
use crate::engine::topology::Direction;

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ZellijMuxProvider;

pub(crate) static ZELLIJ_MUX_PROVIDER: ZellijMuxProvider = ZellijMuxProvider;

impl TerminalMultiplexerProvider for ZellijMuxProvider {
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
        Err(unsupported_zellij_backend())
    }

    fn pane_neighbor_for_pid(&self, _pid: u32, _pane_id: u64, _dir: Direction) -> Result<u64> {
        Err(unsupported_zellij_backend())
    }

    fn send_text_to_pane(&self, _pid: u32, _pane_id: u64, _text: &str) -> Result<()> {
        Err(unsupported_zellij_backend())
    }

    fn mux_attach_args(&self, target: String) -> Option<Vec<String>> {
        Some(vec!["zellij".into(), "attach".into(), target])
    }

    fn merge_source_pane_into_focused_target(
        &self,
        _source_pid: u32,
        _source_pane_id: u64,
        _target_pid: u32,
        _target_window_id: Option<u64>,
        _dir: Direction,
    ) -> Result<()> {
        Err(unsupported_zellij_backend())
    }

    fn active_foreground_process(&self, _pid: u32) -> Option<String> {
        None
    }
}

impl TopologyHandler for ZellijMuxProvider {
    fn can_focus(&self, _dir: Direction, _pid: u32) -> Result<bool> {
        Err(unsupported_zellij_backend())
    }

    fn move_decision(&self, _dir: Direction, _pid: u32) -> Result<MoveDecision> {
        Err(unsupported_zellij_backend())
    }

    fn focus(&self, _dir: Direction, _pid: u32) -> Result<()> {
        Err(unsupported_zellij_backend())
    }

    fn move_internal(&self, _dir: Direction, _pid: u32) -> Result<()> {
        Err(unsupported_zellij_backend())
    }

    fn move_out(&self, _dir: Direction, _pid: u32) -> Result<TearResult> {
        Err(unsupported_zellij_backend())
    }
}

fn unsupported_zellij_backend() -> anyhow::Error {
    anyhow!("zellij mux backend is not implemented yet")
}
