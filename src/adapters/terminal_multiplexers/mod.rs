use anyhow::Result;

use crate::config::TerminalMuxBackend;
use crate::engine::contracts::TerminalMultiplexerProvider;

pub use crate::engine::resolution::command::{
    prepend_terminal_launch_prefix, spawn_attach_command,
};

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
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::utils::env_guard()
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "yeetnyoink-terminal-mux-{prefix}-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("temp dir should be created");
        path
    }

    fn load_config(path: &Path) -> crate::config::Config {
        let old = crate::config::snapshot();
        crate::config::prepare_with_path(Some(path)).expect("config should load");
        old
    }

    fn restore_config(old: crate::config::Config) {
        crate::config::install(old);
    }

    #[test]
    fn active_mux_provider_exposes_default_wezterm_capabilities() {
        let _guard = env_guard();
        let root = unique_temp_dir("wezterm-default-provider");
        let config = root.join("config.toml");
        fs::write(&config, "").expect("config file should be writable");
        let old_config = load_config(&config);
        let provider = super::active_mux_provider(super::WEZTERM_HOST_ALIASES);
        let caps = provider.capabilities();
        assert!(caps.focus);
        assert!(caps.resize_internal);
        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn spawn_attach_command_is_none_for_wezterm_mux_default() {
        let _guard = env_guard();
        let root = unique_temp_dir("wezterm-default-attach");
        let config = root.join("config.toml");
        fs::write(&config, "").expect("config file should be writable");
        let old_config = load_config(&config);
        let command = super::spawn_attach_command(
            super::WEZTERM_HOST_ALIASES,
            &["wezterm", "-e"],
            "dev".to_string(),
        );
        assert_eq!(command, None);
        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }
}
