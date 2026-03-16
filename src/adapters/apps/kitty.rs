use anyhow::{bail, Result};

use crate::adapters::apps::terminal_host_tabs::{self, TerminalHostTabController};
use crate::adapters::terminal_multiplexers;
use crate::config::{self, TerminalMuxBackend};
use crate::engine::contracts::{MergeExecutionMode, MergePreparation, TearResult, TopologyHandler};
use crate::engine::runtime::ProcessId;
use crate::engine::topology::Direction;

pub struct KittyBackend;
pub const ADAPTER_NAME: &str = "terminal";
pub const ADAPTER_ALIASES: &[&str] = &["kitty", "terminal"];
pub const APP_IDS: &[&str] = &["kitty", "org.kovidgoyal.kitty", "net.kovidgoyal.kitty"];
pub const TERMINAL_LAUNCH_PREFIX: &[&str] = &["kitty"];

crate::adapters::apps::impl_terminal_host_app_adapter!(KittyBackend, TERMINAL_LAUNCH_PREFIX);

impl KittyBackend {
    fn host_mux() -> &'static terminal_multiplexers::kitty::KittyMux {
        &terminal_multiplexers::kitty::KITTY_MUX_PROVIDER
    }

    fn host_tab_moves_are_native() -> bool {
        matches!(
            config::mux_policy_for(ADAPTER_ALIASES).backend,
            TerminalMuxBackend::Kitty
        )
    }
}

impl TerminalHostTabController for KittyBackend {
    fn can_focus_host_tab(&self, pid: u32, dir: Direction) -> Result<bool> {
        Self::host_mux().can_focus_host_tab(pid, dir)
    }

    fn focus_host_tab(&self, pid: u32, dir: Direction) -> Result<()> {
        Self::host_mux().focus_host_tab(pid, dir)
    }

    fn can_move_to_host_tab(&self, pid: u32, dir: Direction) -> Result<bool> {
        if !Self::host_tab_moves_are_native() {
            return Ok(false);
        }
        Self::host_mux().can_move_to_host_tab(pid, dir)
    }

    fn move_to_host_tab(&self, pid: u32, dir: Direction) -> Result<()> {
        if !Self::host_tab_moves_are_native() {
            bail!("kitty host-tab move requires mux_backend = \"kitty\"");
        }
        Self::host_mux().move_pane_to_host_tab(pid, dir)
    }
}

impl TopologyHandler for KittyBackend {
    fn can_focus(&self, dir: Direction, pid: u32) -> Result<bool> {
        terminal_host_tabs::can_focus(ADAPTER_ALIASES, Self::mux_provider(), self, dir, pid)
    }

    fn move_decision(
        &self,
        dir: Direction,
        pid: u32,
    ) -> Result<crate::engine::contracts::MoveDecision> {
        terminal_host_tabs::move_decision(ADAPTER_ALIASES, Self::mux_provider(), self, dir, pid)
    }

    fn can_resize(&self, dir: Direction, grow: bool, pid: u32) -> Result<bool> {
        Self::mux_provider().can_resize(dir, grow, pid)
    }

    fn focus(&self, dir: Direction, pid: u32) -> Result<()> {
        terminal_host_tabs::focus(ADAPTER_ALIASES, Self::mux_provider(), self, dir, pid)
    }

    fn move_internal(&self, dir: Direction, pid: u32) -> Result<()> {
        terminal_host_tabs::move_internal(ADAPTER_ALIASES, Self::mux_provider(), self, dir, pid)
    }

    fn resize_internal(&self, dir: Direction, grow: bool, step: i32, pid: u32) -> Result<()> {
        Self::mux_provider().resize_internal(dir, grow, step, pid)
    }

    fn rearrange(&self, dir: Direction, pid: u32) -> Result<()> {
        Self::mux_provider().rearrange(dir, pid)
    }

    fn move_out(&self, dir: Direction, pid: u32) -> Result<TearResult> {
        Ok(terminal_multiplexers::prepend_terminal_launch_prefix(
            TERMINAL_LAUNCH_PREFIX,
            Self::mux_provider().move_out(dir, pid)?,
        ))
    }

    fn merge_execution_mode(&self) -> MergeExecutionMode {
        Self::mux_provider().merge_execution_mode()
    }

    fn prepare_merge(&self, source_pid: Option<ProcessId>) -> Result<MergePreparation> {
        Self::mux_provider().prepare_merge(source_pid)
    }

    fn augment_merge_preparation_for_target(
        &self,
        preparation: MergePreparation,
        target_window_id: Option<u64>,
    ) -> MergePreparation {
        Self::mux_provider().augment_merge_preparation_for_target(preparation, target_window_id)
    }

    fn merge_into_target(
        &self,
        dir: Direction,
        source_pid: Option<ProcessId>,
        target_pid: Option<ProcessId>,
        preparation: MergePreparation,
    ) -> Result<()> {
        Self::mux_provider().merge_into_target(dir, source_pid, target_pid, preparation)
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::KittyBackend;
    use crate::engine::contracts::{AppAdapter, MoveDecision, TopologyHandler};

    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::utils::env_guard()
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "yeetnyoink-kitty-config-{prefix}-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("temp dir should be created");
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

    struct KittyHarness {
        base: PathBuf,
        responses_dir: PathBuf,
        log_file: PathBuf,
        old_path: Option<OsString>,
        old_responses_dir: Option<OsString>,
        old_log_file: Option<OsString>,
        old_config: crate::config::Config,
    }

    impl KittyHarness {
        fn new(config_toml: &str) -> Self {
            let unique = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let base = std::env::temp_dir().join(format!(
                "yeetnyoink-kitty-app-test-{}-{unique}",
                std::process::id()
            ));
            let bin_dir = base.join("bin");
            let responses_dir = base.join("responses");
            let log_file = base.join("commands.log");
            fs::create_dir_all(&bin_dir).expect("failed to create fake bin dir");
            fs::create_dir_all(&responses_dir).expect("failed to create fake responses dir");

            let fake_kitty = bin_dir.join("kitty");
            fs::write(
                &fake_kitty,
                r#"#!/bin/sh
set -eu
if [ "$#" -ge 1 ] && [ "$1" = "@" ]; then
  shift
fi
if [ "$#" -ge 2 ] && [ "$1" = "--to" ]; then
  shift 2
fi
key="$*"
printf '%s\n' "$key" >> "${KITTY_TEST_LOG}"
safe_key="$(printf '%s' "$key" | tr -c 'A-Za-z0-9._-' '_')"
status_file="${KITTY_TEST_RESPONSES_DIR}/${safe_key}.status"
stdout_file="${KITTY_TEST_RESPONSES_DIR}/${safe_key}.stdout"
stderr_file="${KITTY_TEST_RESPONSES_DIR}/${safe_key}.stderr"
status=0
if [ -f "$status_file" ]; then
  status="$(cat "$status_file")"
fi
if [ -f "$stdout_file" ]; then
  cat "$stdout_file"
fi
if [ -f "$stderr_file" ]; then
  cat "$stderr_file" >&2
fi
exit "$status"
"#,
            )
            .expect("failed to write fake kitty script");
            let mut permissions = fs::metadata(&fake_kitty)
                .expect("failed to stat fake kitty script")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&fake_kitty, permissions)
                .expect("failed to chmod fake kitty script");

            let old_path = std::env::var_os("PATH");
            let old_responses_dir = std::env::var_os("KITTY_TEST_RESPONSES_DIR");
            let old_log_file = std::env::var_os("KITTY_TEST_LOG");
            let old_config = crate::config::snapshot();

            let mut path_entries = vec![bin_dir];
            if let Some(ref old) = old_path {
                path_entries.extend(std::env::split_paths(old));
            }
            let path = std::env::join_paths(path_entries).expect("failed to compose PATH");
            std::env::set_var("PATH", path);
            std::env::set_var("KITTY_TEST_RESPONSES_DIR", &responses_dir);
            std::env::set_var("KITTY_TEST_LOG", &log_file);

            let config_dir = base.join("config");
            fs::create_dir_all(&config_dir).expect("config dir should be created");
            let config_path = config_dir.join("config.toml");
            fs::write(&config_path, config_toml).expect("config file should be writable");
            crate::config::prepare_with_path(Some(&config_path)).expect("config should load");

            Self {
                base,
                responses_dir,
                log_file,
                old_path,
                old_responses_dir,
                old_log_file,
                old_config,
            }
        }

        fn set_response(&self, key: &str, status: i32, stdout: &str, stderr: &str) {
            let safe_key: String = key
                .chars()
                .map(|ch| {
                    if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                        ch
                    } else {
                        '_'
                    }
                })
                .collect();
            fs::write(
                self.responses_dir.join(format!("{safe_key}.status")),
                status.to_string(),
            )
            .expect("failed to write fake status");
            fs::write(
                self.responses_dir.join(format!("{safe_key}.stdout")),
                stdout,
            )
            .expect("failed to write fake stdout");
            fs::write(
                self.responses_dir.join(format!("{safe_key}.stderr")),
                stderr,
            )
            .expect("failed to write fake stderr");
        }

        fn command_log(&self) -> String {
            fs::read_to_string(&self.log_file).unwrap_or_default()
        }
    }

    impl Drop for KittyHarness {
        fn drop(&mut self) {
            if let Some(value) = &self.old_path {
                std::env::set_var("PATH", value);
            } else {
                std::env::remove_var("PATH");
            }
            if let Some(value) = &self.old_responses_dir {
                std::env::set_var("KITTY_TEST_RESPONSES_DIR", value);
            } else {
                std::env::remove_var("KITTY_TEST_RESPONSES_DIR");
            }
            if let Some(value) = &self.old_log_file {
                std::env::set_var("KITTY_TEST_LOG", value);
            } else {
                std::env::remove_var("KITTY_TEST_LOG");
            }
            crate::config::install(self.old_config.clone());
            let _ = fs::remove_dir_all(&self.base);
        }
    }

    #[test]
    fn declares_explicit_capability_contract() {
        let _guard = env_guard();
        let root = unique_temp_dir("capabilities");
        let config_dir = root.join("yeetnyoink");
        fs::create_dir_all(&config_dir).expect("config dir should be created");
        fs::write(
            config_dir.join("config.toml"),
            r#"
[app.terminal.kitty]
enabled = true
mux_backend = "kitty"
"#,
        )
        .expect("config file should be writable");
        let old_config = load_config(&config_dir.join("config.toml"));

        let app = KittyBackend;
        let caps = AppAdapter::capabilities(&app);
        assert!(caps.probe);
        assert!(caps.focus);
        assert!(caps.move_internal);
        assert!(caps.resize_internal);
        assert!(caps.rearrange);
        assert!(caps.tear_out);
        assert!(caps.merge);

        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn advertises_config_aliases_for_policy_binding() {
        let app = KittyBackend;
        assert_eq!(app.config_aliases(), Some(super::ADAPTER_ALIASES));
    }

    #[test]
    fn host_tabs_focus_switches_tabs_at_edge() {
        let _guard = env_guard();
        let harness = KittyHarness::new(
            r#"
[app.terminal.kitty]
enabled = true
mux_backend = "kitty"
host_tabs = "focus"
"#,
        );
        harness.set_response(
            "ls --output-format json",
            0,
            r#"[
              {"id": 1, "is_focused": true, "tabs": [
                {"id": 10, "is_focused": true, "windows": [
                  {"id": 100, "is_focused": true, "foreground_processes": [{"cmdline": ["zsh"]}]}
                ]},
                {"id": 20, "is_focused": false, "windows": [
                  {"id": 200, "is_active": true, "foreground_processes": [{"cmdline": ["zsh"]}]}
                ]}
              ]}
            ]"#,
            "",
        );
        harness.set_response(
            "focus-window --match neighbor:right",
            1,
            "",
            "No matching window",
        );
        harness.set_response("focus-tab --match id:20", 0, "", "");

        let app = KittyBackend;
        assert!(app
            .can_focus(crate::engine::topology::Direction::East, 0)
            .expect("host tab focus should be available"));
        app.focus(crate::engine::topology::Direction::East, 0)
            .expect("host tab focus should succeed");

        assert!(harness.command_log().contains("focus-tab --match id:20"));
    }

    #[test]
    fn host_tabs_native_full_moves_into_adjacent_tab() {
        let _guard = env_guard();
        let harness = KittyHarness::new(
            r#"
[app.terminal.kitty]
enabled = true
mux_backend = "kitty"
host_tabs = "native_full"
"#,
        );
        harness.set_response(
            "ls --output-format json",
            0,
            r#"[
              {"id": 1, "is_focused": true, "tabs": [
                {"id": 10, "is_focused": true, "windows": [
                  {"id": 100, "is_focused": true, "foreground_processes": [{"cmdline": ["zsh"]}]}
                ]},
                {"id": 20, "is_focused": false, "windows": [
                  {"id": 200, "is_active": true, "foreground_processes": [{"cmdline": ["zsh"]}]}
                ]}
              ]}
            ]"#,
            "",
        );
        harness.set_response(
            "goto-layout --match id:20 splits:split_axis=horizontal",
            0,
            "",
            "",
        );
        harness.set_response("detach-window --match id:100 --target-tab id:20", 0, "", "");
        harness.set_response("focus-tab --match id:20", 0, "", "");
        harness.set_response(
            "action --match id:100 layout_action move_to_screen_edge left",
            0,
            "",
            "",
        );

        let app = KittyBackend;
        let decision = app
            .move_decision(crate::engine::topology::Direction::East, 0)
            .expect("move_decision should succeed");
        assert!(matches!(decision, MoveDecision::Internal));

        app.move_internal(crate::engine::topology::Direction::East, 0)
            .expect("host-tab move should succeed");

        let log = harness.command_log();
        assert!(log.contains("goto-layout --match id:20 splits:split_axis=horizontal"));
        assert!(log.contains("detach-window --match id:100 --target-tab id:20"));
        assert!(log.contains("focus-tab --match id:20"));
        assert!(log.contains("action --match id:100 layout_action move_to_screen_edge left"));
    }

    #[test]
    fn zellij_backend_selects_attach_command_with_kitty_prefix() {
        let _guard = env_guard();
        let root = unique_temp_dir("zellij-attach");
        let config_dir = root.join("yeetnyoink");
        fs::create_dir_all(&config_dir).expect("config dir should be created");
        fs::write(
            config_dir.join("config.toml"),
            r#"
[app.terminal.kitty]
enabled = true
mux_backend = "zellij"
"#,
        )
        .expect("config file should be writable");
        let old_config = load_config(&config_dir.join("config.toml"));

        let command = KittyBackend::spawn_attach_command("dev".to_string());
        assert_eq!(
            command,
            Some(vec![
                "kitty".to_string(),
                "zellij".to_string(),
                "attach".to_string(),
                "dev".to_string(),
            ])
        );

        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn kitty_mux_backend_has_no_attach_spawn_command() {
        let _guard = env_guard();
        let root = unique_temp_dir("kitty-attach-none");
        let config_dir = root.join("yeetnyoink");
        fs::create_dir_all(&config_dir).expect("config dir should be created");
        fs::write(
            config_dir.join("config.toml"),
            r#"
[app.terminal.kitty]
enabled = true
mux_backend = "kitty"
"#,
        )
        .expect("config file should be writable");
        let old_config = load_config(&config_dir.join("config.toml"));

        let command = KittyBackend::spawn_attach_command("dev".to_string());
        assert_eq!(command, None);

        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }
}
