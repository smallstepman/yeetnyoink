use anyhow::{bail, Result};

use crate::config;
use crate::engine::contracts::{MoveDecision, TerminalMultiplexerProvider};
use crate::engine::topology::Direction;

pub(crate) trait TerminalHostTabController {
    fn can_focus_host_tab(&self, pid: u32, dir: Direction) -> Result<bool>;
    fn focus_host_tab(&self, pid: u32, dir: Direction) -> Result<()>;
    fn can_move_to_host_tab(&self, pid: u32, dir: Direction) -> Result<bool>;
    fn move_to_host_tab(&self, pid: u32, dir: Direction) -> Result<()>;
}

fn host_tab_direction_supported(dir: Direction) -> bool {
    matches!(dir, Direction::West | Direction::East)
}

fn focus_host_tabs_enabled(aliases: &[&str], dir: Direction) -> bool {
    host_tab_direction_supported(dir) && config::terminal_focus_host_tabs_for(aliases)
}

fn move_host_tabs_enabled(aliases: &[&str], dir: Direction) -> bool {
    host_tab_direction_supported(dir) && config::terminal_move_host_tabs_for(aliases)
}

pub(crate) fn can_focus(
    aliases: &[&str],
    mux: &dyn TerminalMultiplexerProvider,
    host: &impl TerminalHostTabController,
    dir: Direction,
    pid: u32,
) -> Result<bool> {
    if mux.can_focus(dir, pid)? {
        return Ok(true);
    }
    if !focus_host_tabs_enabled(aliases, dir) {
        return Ok(false);
    }
    host.can_focus_host_tab(pid, dir)
}

pub(crate) fn focus(
    aliases: &[&str],
    mux: &dyn TerminalMultiplexerProvider,
    host: &impl TerminalHostTabController,
    dir: Direction,
    pid: u32,
) -> Result<()> {
    if mux.can_focus(dir, pid)? {
        return mux.focus(dir, pid);
    }
    if focus_host_tabs_enabled(aliases, dir) && host.can_focus_host_tab(pid, dir)? {
        return host.focus_host_tab(pid, dir);
    }
    mux.focus(dir, pid)
}

pub(crate) fn move_decision(
    aliases: &[&str],
    mux: &dyn TerminalMultiplexerProvider,
    host: &impl TerminalHostTabController,
    dir: Direction,
    pid: u32,
) -> Result<MoveDecision> {
    let inner = mux.move_decision(dir, pid)?;
    if matches!(inner, MoveDecision::Internal | MoveDecision::Rearrange) {
        return Ok(inner);
    }
    if !move_host_tabs_enabled(aliases, dir) {
        return Ok(inner);
    }
    if host.can_move_to_host_tab(pid, dir)? {
        return Ok(MoveDecision::Internal);
    }
    Ok(inner)
}

pub(crate) fn move_internal(
    aliases: &[&str],
    mux: &dyn TerminalMultiplexerProvider,
    host: &impl TerminalHostTabController,
    dir: Direction,
    pid: u32,
) -> Result<()> {
    if matches!(mux.move_decision(dir, pid)?, MoveDecision::Internal) {
        return mux.move_internal(dir, pid);
    }
    if move_host_tabs_enabled(aliases, dir) && host.can_move_to_host_tab(pid, dir)? {
        return host.move_to_host_tab(pid, dir);
    }
    bail!("no terminal host tab exists in requested move direction")
}
