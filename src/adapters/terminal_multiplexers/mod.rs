use anyhow::Result;

use crate::config::TerminalMuxBackend;
use crate::engine::contract::TerminalMultiplexerProvider;

pub mod kitty;
pub mod tmux;
pub mod wezterm;
pub mod zellij;

pub const WEZTERM_HOST_ALIASES: &[&str] = &["wezterm", "terminal"];

pub fn active_mux_provider(aliases: &[&str]) -> &'static dyn TerminalMultiplexerProvider {
    match crate::config::mux_policy_for(aliases).backend {
        TerminalMuxBackend::Wezterm => &wezterm::WEZTERM_MUX_PROVIDER,
        TerminalMuxBackend::Tmux => &tmux::TMUX_MUX_PROVIDER,
        TerminalMuxBackend::Zellij => &zellij::ZELLIJ_MUX_PROVIDER,
        TerminalMuxBackend::Kitty => &kitty::KITTY_MUX_PROVIDER,
    }
}

pub fn spawn_attach_command(
    aliases: &[&str],
    terminal_launch_prefix: &[&str],
    target: String,
) -> Option<Vec<String>> {
    let mux_args = active_mux_provider(aliases).mux_attach_args(target)?;
    let mut command: Vec<String> = terminal_launch_prefix
        .iter()
        .map(|segment| segment.to_string())
        .collect();
    command.extend(mux_args);
    Some(command)
}

pub fn active_foreground_process(aliases: &[&str], pid: u32) -> Option<String> {
    active_mux_provider(aliases).active_foreground_process(pid)
}

pub fn pane_neighbor_for_pid(
    aliases: &[&str],
    pid: u32,
    pane_id: u64,
    dir: crate::engine::topology::Direction,
) -> Result<u64> {
    active_mux_provider(aliases).pane_neighbor_for_pid(pid, pane_id, dir)
}

pub fn send_text_to_pane(aliases: &[&str], pid: u32, pane_id: u64, text: &str) -> Result<()> {
    active_mux_provider(aliases).send_text_to_pane(pid, pane_id, text)
}

#[cfg(test)]
mod tests {
    #[test]
    fn active_mux_provider_exposes_default_wezterm_capabilities() {
        crate::config::prepare().expect("config should load");
        let provider = super::active_mux_provider(super::WEZTERM_HOST_ALIASES);
        let caps = provider.capabilities();
        assert!(caps.focus);
        assert!(caps.resize_internal);
    }

    #[test]
    fn spawn_attach_command_is_none_for_wezterm_mux_default() {
        crate::config::prepare().expect("config should load");
        let command = super::spawn_attach_command(
            super::WEZTERM_HOST_ALIASES,
            &["wezterm", "-e"],
            "dev".to_string(),
        );
        assert_eq!(command, None);
    }
}
