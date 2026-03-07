use std::process::Command;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::engine::contract::{
    AdapterCapabilities, MergeExecutionMode, MergePreparation, SourcePaneMerge, TearResult,
    TerminalMultiplexerProvider, TerminalPaneSnapshot, TopologyHandler,
};
use crate::engine::runtime::{self, ProcessId};
use crate::engine::topology::{Direction, DirectionalNeighbors};
use crate::logging;

#[derive(Debug, Deserialize)]
struct KittyOsWindow {
    id: u64,
    #[serde(default)]
    is_focused: bool,
    #[serde(default)]
    tabs: Vec<KittyTab>,
}

#[derive(Debug, Deserialize)]
struct KittyTab {
    id: u64,
    #[serde(default)]
    is_focused: bool,
    #[serde(default)]
    windows: Vec<KittyPane>,
}

#[derive(Debug, Deserialize)]
struct KittyPane {
    id: u64,
    #[serde(default)]
    is_focused: bool,
    #[serde(default)]
    is_active: bool,
    #[serde(default)]
    cmdline: Vec<String>,
    #[serde(default)]
    foreground_processes: Vec<KittyForegroundProcess>,
}

#[derive(Debug, Deserialize)]
struct KittyForegroundProcess {
    #[serde(default)]
    cmdline: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct KittyMux;

pub(crate) static KITTY_MUX_PROVIDER: KittyMux = KittyMux;

impl KittyPane {
    fn foreground_process_name(&self) -> Option<String> {
        let command = self
            .foreground_processes
            .iter()
            .find_map(|process| process.cmdline.first())
            .map(String::as_str)
            .or_else(|| self.cmdline.first().map(String::as_str))?;
        let normalized = runtime::normalize_process_name(command);
        (!normalized.is_empty()).then_some(normalized)
    }
}

impl KittyMux {
    fn no_match(&self, stderr: &str) -> bool {
        let value = stderr.to_ascii_lowercase();
        value.contains("no matching window")
            || value.contains("no matching windows")
            || value.contains("no matching tabs")
            || value.contains("matches no windows")
    }

    fn missing_tty_remote_control(&self, stderr: &str) -> bool {
        let value = stderr.to_ascii_lowercase();
        value.contains("/dev/tty") || value.contains("controlling terminal")
    }

    fn detached_remote_control_help(&self, pid: u32, stderr: &str) -> String {
        format!(
            "kitty native mux requires kitty remote control to be reachable from outside kitty \
             for detached invocations such as WM keybindings. No KITTY_LISTEN_ON socket was \
             found for pid {pid}, so kitty fell back to the controlling tty and failed. \
             Add this to kitty.conf, restart kitty, and try again:\n\
\n\
allow_remote_control socket-only\n\
listen_on unix:@kitty-{{kitty_pid}}\n\
\n\
Original error: {stderr}"
        )
    }

    fn status_error(&self, pid: u32, args: &[&str], stderr: &str) -> anyhow::Error {
        if self.missing_tty_remote_control(stderr) && self.socket_for_pid(pid).is_none() {
            anyhow::anyhow!(self.detached_remote_control_help(pid, stderr))
        } else {
            anyhow::anyhow!(
                "terminal multiplexer command {:?} failed: {}",
                args,
                stderr.trim()
            )
        }
    }

    fn action_spec(&self, action: &str, dir: Direction) -> String {
        format!("{action} {}", dir.egocentric())
    }

    fn move_window_action(&self, dir: Direction) -> String {
        self.action_spec("move_window", dir)
    }

    fn move_to_screen_edge_action(&self, dir: Direction) -> String {
        format!("layout_action move_to_screen_edge {}", dir.positional())
    }

    fn target_merge_layout(&self, dir: Direction) -> &'static str {
        match dir {
            Direction::West | Direction::East => "splits:split_axis=horizontal",
            Direction::North | Direction::South => "splits:split_axis=vertical",
        }
    }

    fn socket_for_pid(&self, pid: u32) -> Option<String> {
        for candidate in runtime::process_tree_pids(pid) {
            if let Some(socket) = runtime::process_environ_var(candidate, "KITTY_LISTEN_ON") {
                return Some(socket);
            }
        }
        None
    }

    fn active_tab_panes(&self, pid: u32) -> Result<Vec<TerminalPaneSnapshot>> {
        let windows: Vec<KittyOsWindow> = self.cli_json_for_pid(
            pid,
            &["ls", "--output-format", "json"],
            "failed to parse kitty ls json",
        )?;
        let window = windows
            .iter()
            .find(|window| window.is_focused)
            .or_else(|| {
                windows.iter().find(|window| {
                    window.tabs.iter().any(|tab| {
                        tab.is_focused
                            || tab
                                .windows
                                .iter()
                                .any(|pane| pane.is_focused || pane.is_active)
                    })
                })
            })
            .or_else(|| windows.first())
            .context("kitty did not report any windows")?;
        let tab = window
            .tabs
            .iter()
            .find(|tab| tab.is_focused)
            .or_else(|| {
                window.tabs.iter().find(|tab| {
                    tab.windows
                        .iter()
                        .any(|pane| pane.is_focused || pane.is_active)
                })
            })
            .or_else(|| window.tabs.first())
            .context("kitty focused window has no tabs")?;
        Ok(tab
            .windows
            .iter()
            .map(|pane| TerminalPaneSnapshot {
                pane_id: pane.id,
                tab_id: Some(tab.id),
                window_id: Some(window.id),
                is_active: pane.is_focused || pane.is_active,
                foreground_process_name: pane.foreground_process_name(),
            })
            .collect())
    }

    fn active_tab_id_for_pid(&self, pid: u32) -> Result<u64> {
        self.active_tab_panes(pid)?
            .first()
            .and_then(|pane| pane.tab_id)
            .context("kitty active tab is missing an id")
    }

    fn focus_pane_by_id(&self, pid: u32, pane_id: u64) -> Result<()> {
        let matcher = format!("id:{pane_id}");
        self.cli_stdout_for_pid(pid, &["focus-window", "--match", &matcher])?;
        Ok(())
    }

    fn run_action_for_pane(&self, pid: u32, pane_id: u64, action: &str) -> Result<()> {
        let matcher = format!("id:{pane_id}");
        self.cli_stdout_for_pid(pid, &["action", "--match", &matcher, action])?;
        Ok(())
    }

    fn set_tab_layout(&self, pid: u32, tab_id: u64, layout: &str) -> Result<()> {
        let matcher = format!("id:{tab_id}");
        self.cli_stdout_for_pid(pid, &["goto-layout", "--match", &matcher, layout])?;
        Ok(())
    }

    fn resize_increment(dir: Direction, grow: bool, step: i32) -> i32 {
        let magnitude = step.abs().max(1);
        let directional_delta = dir.sign() * magnitude;
        if grow {
            directional_delta
        } else {
            -directional_delta
        }
    }

    fn try_focus_neighbor(&self, pid: u32, dir: Direction) -> Result<bool> {
        let matcher = format!("neighbor:{}", dir.positional());
        let output = self.cli_output_for_pid(pid, &["focus-window", "--match", &matcher])?;
        if output.status.success() {
            return Ok(true);
        }
        let stderr = runtime::stderr_text(&output);
        if self.no_match(&stderr) {
            return Ok(false);
        }
        Err(self.status_error(pid, &["focus-window", "--match", &matcher], &stderr))
    }
}

impl TerminalMultiplexerProvider for KittyMux {
    fn command_error_for_pid(&self, pid: u32, args: &[&str], stderr: &str) -> anyhow::Error {
        self.status_error(pid, args, stderr)
    }

    fn cli_output_for_pid(&self, pid: u32, args: &[&str]) -> Result<std::process::Output> {
        let mut command = Command::new("kitty");
        command.arg("@");
        if let Some(socket) = self.socket_for_pid(pid) {
            command.args(["--to", &socket]);
        } else {
            logging::debug(format!(
                "kitty: no KITTY_LISTEN_ON discovered for pid {}; falling back to controlling tty",
                pid
            ));
        }
        command.args(args);
        command
            .output()
            .context("failed to run kitty remote-control command")
    }

    fn list_panes_for_pid(&self, pid: u32) -> Result<Vec<TerminalPaneSnapshot>> {
        self.active_tab_panes(pid)
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities::terminal_mux_defaults()
            .with_resize_internal(true)
            .with_rearrange(true)
    }

    fn focused_pane_for_pid(&self, pid: u32) -> Result<u64> {
        self.focused_pane_from_snapshots(
            &self.active_tab_panes(pid)?,
            "unable to determine focused kitty pane",
        )
    }

    fn pane_in_direction_for_pid(
        &self,
        pid: u32,
        pane_id: u64,
        dir: Direction,
    ) -> Result<Option<u64>> {
        let original_focus = self.focused_pane_for_pid(pid).ok();
        if original_focus != Some(pane_id) {
            self.focus_pane_by_id(pid, pane_id)?;
        }
        let result = (|| -> Result<Option<u64>> {
            if !self.try_focus_neighbor(pid, dir)? {
                return Ok(None);
            }
            let focused = self.focused_pane_for_pid(pid)?;
            Ok((focused != pane_id).then_some(focused))
        })();
        if let Some(original_focus) = original_focus {
            let _ = self.focus_pane_by_id(pid, original_focus);
        }
        result
    }

    fn send_text_to_pane(&self, pid: u32, pane_id: u64, text: &str) -> Result<()> {
        let matcher = format!("id:{pane_id}");
        self.cli_status_for_pid(pid, &["send-text", "--match", &matcher, "--", text])?;
        Ok(())
    }

    fn mux_attach_args(&self, _target: String) -> Option<Vec<String>> {
        None
    }

    fn merge_source_pane_into_focused_target(
        &self,
        source_pid: u32,
        source_pane_id: u64,
        target_pid: u32,
        _target_window_id: Option<u64>,
        dir: Direction,
    ) -> Result<()> {
        if let (Some(source_socket), Some(target_socket)) = (
            self.socket_for_pid(source_pid),
            self.socket_for_pid(target_pid),
        ) {
            if source_socket != target_socket {
                bail!(
                    "source and target kitty instances differ ({} != {})",
                    source_socket,
                    target_socket
                );
            }
        }

        let target_pane_id = self.focused_pane_for_pid(target_pid)?;
        if target_pane_id == source_pane_id {
            bail!("source and target kitty panes are the same");
        }

        let target_tab_pane_count = self.active_tab_panes(target_pid)?.len();
        let target_tab_id = self.active_tab_id_for_pid(target_pid)?;
        if target_tab_pane_count == 1 {
            self.set_tab_layout(target_pid, target_tab_id, self.target_merge_layout(dir))?;
        }
        let source_match = format!("id:{source_pane_id}");
        let target_tab_match = format!("id:{target_tab_id}");
        self.cli_stdout_for_pid(
            target_pid,
            &[
                "detach-window",
                "--match",
                &source_match,
                "--target-tab",
                &target_tab_match,
            ],
        )?;

        let target_side = dir.opposite();
        if let Err(err) = self.run_action_for_pane(
            target_pid,
            source_pane_id,
            &self.move_to_screen_edge_action(target_side),
        ) {
            logging::debug(format!(
                "kitty: merge post-placement move_to_screen_edge failed source_pane_id={} target_pid={} dir={} err={:#}",
                source_pane_id,
                target_pid,
                target_side.positional(),
                err
            ));
            if let Err(edge_err) = self.run_action_for_pane(
                target_pid,
                source_pane_id,
                &self.move_window_action(target_side),
            ) {
                logging::debug(format!(
                    "kitty: merge post-placement move_window failed source_pane_id={} target_pid={} dir={} err={:#}",
                    source_pane_id,
                    target_pid,
                    target_side.positional(),
                    edge_err
                ));
            }
        }
        Ok(())
    }
}

impl TopologyHandler for KittyMux {
    fn directional_neighbors(&self, pid: u32) -> Result<DirectionalNeighbors> {
        self.directional_neighbors_from_pane_lookup(pid)
    }

    fn window_count(&self, pid: u32) -> Result<u32> {
        self.active_scope_pane_count_for_pid(pid)
    }

    fn supports_rearrange_decision(&self) -> bool {
        // Kitty can rearrange panes, but without a split tree its perpendicular-neighbor
        // heuristic is too eager and blocks expected edge tear-outs from WM keybindings.
        false
    }

    fn can_focus(&self, dir: Direction, pid: u32) -> Result<bool> {
        self.can_focus_from_pane_lookup(dir, pid)
    }

    fn can_resize(&self, dir: Direction, _grow: bool, pid: u32) -> Result<bool> {
        if self.window_count(pid)? <= 1 {
            return Ok(false);
        }
        self.axis_neighbors_exist_from_pane_lookup(pid, dir)
    }

    fn move_decision(
        &self,
        dir: Direction,
        pid: u32,
    ) -> Result<crate::engine::contract::MoveDecision> {
        self.move_decision_from_pane_lookup(dir, pid, false)
    }

    fn focus(&self, dir: Direction, pid: u32) -> Result<()> {
        if !self.try_focus_neighbor(pid, dir)? {
            bail!("no kitty pane exists in requested direction");
        }
        Ok(())
    }

    fn move_internal(&self, dir: Direction, pid: u32) -> Result<()> {
        let pane_id = self.focused_pane_for_pid(pid)?;
        self.run_action_for_pane(pid, pane_id, &self.move_window_action(dir))?;
        Ok(())
    }

    fn resize_internal(&self, dir: Direction, grow: bool, step: i32, pid: u32) -> Result<()> {
        let pane_id = self.focused_pane_for_pid(pid)?;
        let matcher = format!("id:{pane_id}");
        let increment = Self::resize_increment(dir, grow, step).to_string();
        self.cli_stdout_for_pid(
            pid,
            &[
                "resize-window",
                "--match",
                &matcher,
                "--axis",
                dir.axis_name(),
                "--increment",
                &increment,
            ],
        )?;
        Ok(())
    }

    fn rearrange(&self, dir: Direction, pid: u32) -> Result<()> {
        let pane_id = self.focused_pane_for_pid(pid)?;
        self.run_action_for_pane(pid, pane_id, &self.move_to_screen_edge_action(dir))?;
        Ok(())
    }

    fn move_out(&self, _dir: Direction, pid: u32) -> Result<TearResult> {
        let pane_id = self.focused_pane_for_pid(pid)?;
        let matcher = format!("id:{pane_id}");
        self.cli_stdout_for_pid(pid, &["detach-window", "--match", &matcher])?;
        Ok(TearResult {
            spawn_command: None,
        })
    }

    fn merge_execution_mode(&self) -> MergeExecutionMode {
        MergeExecutionMode::TargetFocused
    }

    fn prepare_merge(&self, source_pid: Option<ProcessId>) -> Result<MergePreparation> {
        self.prepare_merge_payload(source_pid, "source kitty merge missing pid", |source_pid| {
            Ok(SourcePaneMerge::new(
                self.focused_pane_for_pid(source_pid)?,
                (),
            ))
        })
    }

    fn merge_into_target(
        &self,
        dir: Direction,
        source_pid: Option<ProcessId>,
        target_pid: Option<ProcessId>,
        preparation: MergePreparation,
    ) -> Result<()> {
        let (source_pid, target_pid, preparation) = self
            .resolve_target_focused_merge::<SourcePaneMerge<()>>(
                source_pid,
                target_pid,
                preparation,
                "source kitty merge missing pid",
                "target kitty merge missing pid",
                "source kitty merge missing pane id",
            )?;
        self.merge_source_pane_into_focused_target(
            source_pid,
            preparation.pane_id,
            target_pid,
            None,
            dir,
        )
        .context("kitty merge failed")
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::KittyMux;
    use crate::engine::contract::{TerminalMultiplexerProvider, TopologyHandler};
    use crate::engine::topology::Direction;

    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    const TEST_RESPONSES_ENV: &str = "KITTY_TEST_RESPONSES_DIR";
    const TEST_LOG_ENV: &str = "KITTY_TEST_LOG";

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::utils::env_guard()
    }

    struct KittyHarness {
        base: PathBuf,
        responses_dir: PathBuf,
        log_file: PathBuf,
        old_path: Option<OsString>,
        old_responses_dir: Option<OsString>,
        old_log_file: Option<OsString>,
    }

    impl KittyHarness {
        fn new() -> Self {
            let unique = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let base = std::env::temp_dir().join(format!(
                "yeet-and-yoink-kitty-mux-test-{}-{unique}",
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
            let old_responses_dir = std::env::var_os(TEST_RESPONSES_ENV);
            let old_log_file = std::env::var_os(TEST_LOG_ENV);

            let mut path_entries = vec![bin_dir];
            if let Some(ref old) = old_path {
                path_entries.extend(std::env::split_paths(old));
            }
            let path = std::env::join_paths(path_entries).expect("failed to compose PATH");
            std::env::set_var("PATH", path);
            std::env::set_var(TEST_RESPONSES_ENV, &responses_dir);
            std::env::set_var(TEST_LOG_ENV, &log_file);

            Self {
                base,
                responses_dir,
                log_file,
                old_path,
                old_responses_dir,
                old_log_file,
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
                std::env::set_var(TEST_RESPONSES_ENV, value);
            } else {
                std::env::remove_var(TEST_RESPONSES_ENV);
            }
            if let Some(value) = &self.old_log_file {
                std::env::set_var(TEST_LOG_ENV, value);
            } else {
                std::env::remove_var(TEST_LOG_ENV);
            }
            let _ = fs::remove_dir_all(&self.base);
        }
    }

    #[test]
    fn focused_pane_and_window_count_come_from_active_tab() {
        let _guard = env_guard();
        let harness = KittyHarness::new();
        harness.set_response(
            "ls --output-format json",
            0,
            r#"[
              {"id": 1, "is_focused": true, "tabs": [
                {"id": 10, "is_focused": true, "windows": [
                  {"id": 100, "is_focused": true, "foreground_processes": [{"cmdline": ["zsh"]}]},
                  {"id": 101, "is_focused": false, "foreground_processes": [{"cmdline": ["nvim"]}]}
                ]}
              ]}
            ]"#,
            "",
        );

        let provider = KittyMux;
        let focused = provider.focused_pane_for_pid(0).expect("focused pane");
        let count = provider.window_count(0).expect("window_count");
        assert_eq!(focused, 100);
        assert_eq!(count, 2);
    }

    #[test]
    fn move_internal_uses_action_move_window_command() {
        let _guard = env_guard();
        let harness = KittyHarness::new();
        harness.set_response(
            "ls --output-format json",
            0,
            r#"[
              {"id": 1, "is_focused": true, "tabs": [
                {"id": 10, "is_focused": true, "windows": [
                  {"id": 100, "is_focused": true, "foreground_processes": [{"cmdline": ["zsh"]}]}
                ]}
              ]}
             ]"#,
            "",
        );
        harness.set_response("action --match id:100 move_window left", 0, "", "");

        let provider = KittyMux;
        provider
            .move_internal(Direction::West, 0)
            .expect("move_internal should succeed");
        assert!(harness
            .command_log()
            .contains("action --match id:100 move_window left"));
    }

    #[test]
    fn rearrange_uses_layout_action_move_to_screen_edge() {
        let _guard = env_guard();
        let harness = KittyHarness::new();
        harness.set_response(
            "ls --output-format json",
            0,
            r#"[
              {"id": 1, "is_focused": true, "tabs": [
                {"id": 10, "is_focused": true, "windows": [
                  {"id": 100, "is_focused": true, "foreground_processes": [{"cmdline": ["zsh"]}]}
                ]}
              ]}
            ]"#,
            "",
        );
        harness.set_response(
            "action --match id:100 layout_action move_to_screen_edge top",
            0,
            "",
            "",
        );

        let provider = KittyMux;
        provider
            .rearrange(Direction::North, 0)
            .expect("rearrange should succeed");
        assert!(harness
            .command_log()
            .contains("action --match id:100 layout_action move_to_screen_edge top"));
    }

    #[test]
    fn resize_internal_uses_resize_window_command() {
        let _guard = env_guard();
        let harness = KittyHarness::new();
        harness.set_response(
            "ls --output-format json",
            0,
            r#"[
              {"id": 1, "is_focused": true, "tabs": [
                {"id": 10, "is_focused": true, "windows": [
                  {"id": 100, "is_focused": true, "foreground_processes": [{"cmdline": ["zsh"]}]},
                  {"id": 101, "is_focused": false, "foreground_processes": [{"cmdline": ["nvim"]}]}
                ]}
              ]}
            ]"#,
            "",
        );
        harness.set_response(
            "resize-window --match id:100 --axis horizontal --increment -3",
            0,
            "",
            "",
        );

        let provider = KittyMux;
        provider
            .resize_internal(Direction::West, true, 3, 0)
            .expect("resize_internal should succeed");
        assert!(harness
            .command_log()
            .contains("resize-window --match id:100 --axis horizontal --increment -3"));
    }

    #[test]
    fn move_policy_prefers_tearout_over_auto_rearrange() {
        let provider = KittyMux;
        assert!(!TopologyHandler::supports_rearrange_decision(&provider));
    }

    #[test]
    fn merge_moves_source_window_into_target_tab_and_repositions() {
        let _guard = env_guard();
        let harness = KittyHarness::new();
        harness.set_response(
            "ls --output-format json",
            0,
            r#"[
              {"id": 1, "is_focused": true, "tabs": [
                {"id": 10, "is_focused": true, "windows": [
                  {"id": 100, "is_focused": true, "foreground_processes": [{"cmdline": ["zsh"]}]}
                ]}
              ]}
            ]"#,
            "",
        );
        harness.set_response(
            "goto-layout --match id:10 splits:split_axis=horizontal",
            0,
            "",
            "",
        );
        harness.set_response("detach-window --match id:200 --target-tab id:10", 0, "", "");
        harness.set_response(
            "action --match id:200 layout_action move_to_screen_edge right",
            0,
            "",
            "",
        );

        let provider = KittyMux;
        provider
            .merge_source_pane_into_focused_target(0, 200, 0, None, Direction::West)
            .expect("merge should succeed");
        let log = harness.command_log();
        assert!(log.contains("goto-layout --match id:10 splits:split_axis=horizontal"));
        assert!(log.contains("detach-window --match id:200 --target-tab id:10"));
        assert!(log.contains("action --match id:200 layout_action move_to_screen_edge right"));
    }

    #[test]
    fn missing_socket_error_explains_kitty_conf_requirement() {
        let _guard = env_guard();
        let harness = KittyHarness::new();
        harness.set_response(
            "ls --output-format json",
            1,
            "",
            "Error: open /dev/tty: no such device or address",
        );

        let provider = KittyMux;
        let err = provider
            .focused_pane_for_pid(0)
            .expect_err("focused_pane_for_pid should fail without tty/socket");
        let message = format!("{err:#}");
        assert!(message.contains("allow_remote_control socket-only"));
        assert!(message.contains("listen_on unix:@kitty-{kitty_pid}"));
    }

    #[test]
    fn move_out_uses_detach_window_command() {
        let _guard = env_guard();
        let harness = KittyHarness::new();
        harness.set_response(
            "ls --output-format json",
            0,
            r#"[
              {"id": 1, "is_focused": true, "tabs": [
                {"id": 10, "is_focused": true, "windows": [
                  {"id": 100, "is_focused": true, "foreground_processes": [{"cmdline": ["zsh"]}]}
                ]}
              ]}
            ]"#,
            "",
        );
        harness.set_response("detach-window --match id:100", 0, "", "");

        let provider = KittyMux;
        provider
            .move_out(Direction::East, 0)
            .expect("move_out should succeed");
        assert!(harness
            .command_log()
            .contains("detach-window --match id:100"));
    }
}
