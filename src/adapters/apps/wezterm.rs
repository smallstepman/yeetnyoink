//! # WezTerm integration capability map (for yeet-and-yoink)
//!
//! This module implements the WezTerm side of directional focus/move semantics used by
//! `yeet-and-yoink`, including:
//!
//! - pane-local directional focus,
//! - in-window pane rearrange,
//! - tear-out to new window,
//! - merge back into a neighboring WezTerm window through CLI target selection.
//!
//! The goal of this comment is to document the *full relevant WezTerm capability surface*
//! for this problem (including APIs not currently used directly), and to point to the
//! canonical upstream docs.
//!
//! ## Mental model: WezTerm has multiple control planes
//!
//! 1. **CLI plane** (`wezterm cli ...`)
//!    - Strong for external orchestration from Rust.
//!    - Can target a specific pane/window and perform atomic pane operations.
//! 2. **Lua/mux plane** (`wezterm.mux`, `pane:*`, `MuxTab`, `MuxWindow`, etc.)
//!    - Strong for in-process policy, startup orchestration, and GUI/mux mapping.
//! 3. **GUI plane** (`window`, `wezterm.gui.*`)
//!    - Represents visible windows and focus state, and can be mapped to mux objects.
//!
//! yeet-and-yoink currently uses explicit CLI/socket control. Lua/mux APIs remain useful
//! background context, but the current integration does not require a WezTerm plugin.
//!
//! ## CLI capabilities relevant to this module
//!
//! - `wezterm cli list --format json`:
//!   Enumerates panes with `window_id`, `tab_id`, `pane_id`, workspace, etc.
//!   This is our primary topology snapshot.
//! - `wezterm cli list-clients --format json`:
//!   Gives per-client session state (`pid`, `focused_pane_id`, workspace, idle/connected
//!   timing). This is used to bias toward the pane focused by the active GUI client.
//! - `wezterm cli get-pane-direction <Dir> --pane-id <id>`:
//!   Finds directional neighbors for internal move/focus decisions.
//! - `wezterm cli split-pane --pane-id <target> <side> --move-pane-id <source>`:
//!   Core merge/rearrange primitive: split relative to target and move an existing pane.
//! - `wezterm cli move-pane-to-new-tab --new-window --pane-id <source>`:
//!   Core tear-out primitive used to create a new window and move a pane there.
//!
//! Targeting behavior that matters for correctness:
//!
//! - `$WEZTERM_UNIX_SOCKET` can force CLI commands to a specific running instance.
//! - If `--pane-id` is omitted, CLI uses `$WEZTERM_PANE` or most-recent client focus
//!   heuristics, which is often too implicit for cross-window orchestration.
//! - `wezterm cli` can prefer the mux server with `--prefer-mux`; yeet-and-yoink currently
//!   relies primarily on explicit pane IDs and instance targeting instead.
//!
//! ## Lua/mux capabilities relevant to this problem
//!
//! - `wezterm.mux` module:
//!   Multiplexer API over panes/tabs/windows/workspaces and domains; suitable for
//!   validating pane identity, spawning windows, and workspace/domain logic.
//! - `wezterm.mux.get_pane(id)`:
//!   Validates/returns a Pane object for an ID from external sources.
//! - `MuxTab:get_pane_direction(direction)`:
//!   Directional neighbor lookup in mux space, analogous to CLI direction queries.
//! - `pane:split{ ... }`:
//!   Creates new splits and spawns processes, but does **not** expose a direct equivalent
//!   of CLI `--move-pane-id` for moving an *existing* pane into the split.
//! - `pane:move_to_new_tab()` / `pane:move_to_new_window([workspace])`:
//!   Useful tear-out style operations from Lua callbacks.
//!
//! This split between CLI and Lua capabilities is why the current integration stays
//! CLI-driven: the CLI exposes `split-pane --move-pane-id`, while Lua exposes richer
//! mux state but not the same "move an existing pane into a new split" primitive.
//!
//! ## GUI <-> mux mapping capabilities
//!
//! - `window:mux_window()` converts GUI window -> mux window.
//! - `mux_window:gui_window()` converts mux window -> GUI window (when visible/active).
//! - `wezterm.gui.gui_window_for_mux_window(window_id)` resolves mux window id to GUI
//!   window object, when such mapping exists in the active workspace.
//! - `wezterm.gui.gui_windows()` lists GUI windows in stable order.
//! - `window:is_focused()` can still be useful for future in-process policy layers that
//!   need to validate GUI focus before acting.
//!
//! ## Events and lifecycle hooks worth knowing
//!
//! - `update-status` is the current periodic status event.
//! - `gui-startup` / `mux-startup` are the correct places for startup window/tab/pane
//!   creation; upstream explicitly warns against spawning splits/tabs/windows at config
//!   file scope because config can be evaluated multiple times.
//!
//! ## Domain/workspace capabilities (not directly used today, but relevant)
//!
//! - `MuxDomain` (`attach`, `detach`, `state`, `is_spawnable`) can model remote domains
//!   and whether they can create panes.
//! - Workspace operations (`wezterm.mux.set_active_workspace`, etc.) can affect whether a
//!   mux window has a GUI representation at a given moment.
//!
//! These matter if yeet-and-yoink is later extended to cross-domain or workspace-aware routing.
//!
//! ## Practical edge cases this module must handle
//!
//! - Multiple GUI clients may exist; `list-clients` can transiently report focus that
//!   doesn't yet reflect niri focus hops.
//! - Pane `is_active` signals can be ambiguous across windows; deterministic pane IDs and
//!   explicit window filtering are safer.
//! - Focus updates are asynchronous; short polling/retry windows are often necessary before
//!   committing merge targets.
//!
//! ## Canonical references (URLs)
//!
//! Core CLI:
//! - https://wezterm.org/cli/cli/index.html
//! - https://wezterm.org/cli/cli/list.html
//! - https://wezterm.org/cli/cli/list-clients.html
//! - https://wezterm.org/cli/cli/get-pane-direction.html
//! - https://wezterm.org/cli/cli/split-pane.html
//! - https://wezterm.org/cli/cli/move-pane-to-new-tab.html
//!
//! Core mux/Lua:
//! - https://wezterm.org/config/lua/wezterm.mux/index.html
//! - https://wezterm.org/config/lua/wezterm.mux/get_pane.html
//! - https://wezterm.org/config/lua/MuxDomain/index.html
//! - https://wezterm.org/config/lua/MuxTab/index.html
//! - https://wezterm.org/config/lua/MuxTab/get_pane_direction.html
//! - https://wezterm.org/config/lua/mux-window/index.html
//! - https://wezterm.org/config/lua/pane/split.html
//! - https://wezterm.org/config/lua/pane/move_to_new_tab.html
//! - https://wezterm.org/config/lua/pane/move_to_new_window.html
//!
//! GUI mapping and events:
//! - https://wezterm.org/config/lua/window/mux_window.html
//! - https://wezterm.org/config/lua/mux-window/gui_window.html
//! - https://wezterm.org/config/lua/wezterm.gui/gui_window_for_mux_window.html
//! - https://wezterm.org/config/lua/wezterm.gui/gui_windows.html
//! - https://wezterm.org/config/lua/window/is_focused.html
//! - https://wezterm.org/config/lua/window-events/update-status.html
//! - https://wezterm.org/config/lua/gui-events/gui-startup.html
//! - https://wezterm.org/config/lua/mux-events/mux-startup.html
//!
//! Extra domain details:
//! - https://wezterm.org/config/lua/MuxDomain/attach.html
//! - https://wezterm.org/config/lua/MuxDomain/detach.html
//! - https://wezterm.org/config/lua/MuxDomain/state.html
//! - https://wezterm.org/config/lua/MuxDomain/is_spawnable.html
//!
//! Keep this comment aligned with upstream semantics whenever WezTerm changes CLI/mux APIs.
//!
use crate::adapters::terminal_multiplexers;

/// Terminal app adapter surface (domain identity + integration helpers).
pub struct WeztermBackend;
pub const ADAPTER_NAME: &str = "terminal";
pub const ADAPTER_ALIASES: &[&str] = terminal_multiplexers::WEZTERM_HOST_ALIASES;
pub const APP_IDS: &[&str] = &["org.wezfurlong.wezterm"];

/// Terminal launch prefix for composing spawn commands (e.g. `["wezterm", "-e"]`).
pub const TERMINAL_LAUNCH_PREFIX: &[&str] = &["wezterm", "-e"];

crate::adapters::apps::impl_terminal_host_backend!(WeztermBackend, TERMINAL_LAUNCH_PREFIX);

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::WeztermBackend;
    use crate::adapters::terminal_multiplexers::tmux::TmuxMuxProvider;
    use crate::adapters::terminal_multiplexers::wezterm::WeztermMux;
    use crate::adapters::terminal_multiplexers::zellij::ZellijMuxProvider;
    use crate::engine::contracts::{
        AppAdapter, MoveDecision, TerminalMultiplexerProvider, TopologyHandler,
    };
    use crate::engine::topology::Direction;

    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::utils::env_guard()
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "yeet-and-yoink-wezterm-config-{prefix}-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("temp dir should be created");
        path
    }

    fn load_config(path: &std::path::Path) -> crate::config::Config {
        let old = crate::config::snapshot();
        crate::config::prepare_with_path(Some(path)).expect("config should load");
        old
    }

    fn restore_config(old: crate::config::Config) {
        crate::config::install(old);
    }

    #[test]
    fn declares_explicit_capability_contract() {
        let _guard = env_guard();
        let root = unique_temp_dir("capabilities");
        let config = root.join("config.toml");
        fs::write(&config, "").expect("config file should be writable");
        let old_config = load_config(&config);
        let app = WeztermBackend;
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
        let app = WeztermBackend;
        assert_eq!(app.config_aliases(), Some(super::ADAPTER_ALIASES));
    }

    #[test]
    fn mux_providers_implement_expected_traits() {
        fn assert_mux_trait<T: TerminalMultiplexerProvider>() {}
        assert_mux_trait::<WeztermMux>();
        assert_mux_trait::<TmuxMuxProvider>();
        assert_mux_trait::<ZellijMuxProvider>();
    }

    #[test]
    fn capabilities_follow_tmux_mux_backend() {
        let _guard = env_guard();
        let root = unique_temp_dir("caps-tmux");
        let config_dir = root.join("yeet-and-yoink");
        fs::create_dir_all(&config_dir).expect("config dir should be created");
        fs::write(
            config_dir.join("config.toml"),
            r#"
[app.terminal.wezterm]
enabled = true
mux_backend = "tmux"
"#,
        )
        .expect("config file should be writable");
        let old_config = load_config(&config_dir.join("config.toml"));

        let app = WeztermBackend;
        let caps = AppAdapter::capabilities(&app);
        assert!(caps.probe);
        assert!(caps.focus);
        assert!(caps.move_internal);
        assert!(!caps.resize_internal);
        assert!(!caps.rearrange);
        assert!(caps.tear_out);

        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn zellij_backend_selects_zellij_attach_command() {
        let _guard = env_guard();
        let root = unique_temp_dir("zellij-attach");
        let config_dir = root.join("yeet-and-yoink");
        fs::create_dir_all(&config_dir).expect("config dir should be created");
        fs::write(
            config_dir.join("config.toml"),
            r#"
[app.terminal.wezterm]
enabled = true
mux_backend = "zellij"
"#,
        )
        .expect("config file should be writable");
        let old_config = load_config(&config_dir.join("config.toml"));

        let command = WeztermBackend::spawn_attach_command("dev".to_string());
        assert_eq!(
            command,
            Some(vec![
                "wezterm".to_string(),
                "-e".to_string(),
                "zellij".to_string(),
                "attach".to_string(),
                "dev".to_string(),
            ])
        );

        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn wezterm_mux_backend_has_no_attach_spawn_command() {
        let _guard = env_guard();
        let root = unique_temp_dir("wezterm-attach-none");
        let config_dir = root.join("yeet-and-yoink");
        fs::create_dir_all(&config_dir).expect("config dir should be created");
        fs::write(
            config_dir.join("config.toml"),
            r#"
[app.terminal.wezterm]
enabled = true
mux_backend = "wezterm"
"#,
        )
        .expect("config file should be writable");
        let old_config = load_config(&config_dir.join("config.toml"));

        let command = WeztermBackend::spawn_attach_command("dev".to_string());
        assert_eq!(command, None);

        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }

    struct WeztermHarness {
        base: PathBuf,
        responses_dir: PathBuf,
        log_file: PathBuf,
        old_path: Option<OsString>,
        old_runtime_dir: Option<OsString>,
        old_responses_dir: Option<OsString>,
        old_log_file: Option<OsString>,
        old_config: crate::config::Config,
    }

    impl WeztermHarness {
        fn new(pid: u32) -> Self {
            let unique = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let base =
                std::env::temp_dir().join(format!("yeet-and-yoink-wezterm-test-{pid}-{unique}"));
            let bin_dir = base.join("bin");
            let runtime_dir = base.join("runtime");
            let responses_dir = base.join("responses");
            let log_file = base.join("commands.log");

            fs::create_dir_all(&bin_dir).expect("failed to create fake bin dir");
            fs::create_dir_all(runtime_dir.join("wezterm"))
                .expect("failed to create fake runtime dir");
            fs::create_dir_all(&responses_dir).expect("failed to create responses dir");
            fs::write(
                runtime_dir.join("wezterm").join(format!("gui-sock-{pid}")),
                "",
            )
            .expect("failed to create fake wezterm socket");

            let fake_wezterm = bin_dir.join("wezterm");
            fs::write(
                &fake_wezterm,
                r#"#!/bin/sh
set -eu

mode=""
if [ "$#" -ge 1 ] && [ "$1" = "cli" ]; then
  shift
  if [ "$#" -ge 2 ] && [ "$1" = "--unix-socket" ]; then
    mode="cli-socket"
    shift 2
  elif [ -n "${WEZTERM_UNIX_SOCKET:-}" ]; then
    mode="env-socket"
  else
    echo "missing unix socket context for wezterm cli" >&2
    exit 2
  fi
elif [ "$#" -ge 3 ] && [ "$1" = "--unix-socket" ] && [ "$3" = "cli" ]; then
  mode="cli-socket"
  shift 3
else
  echo "expected wezterm cli invocation with unix socket" >&2
  exit 2
fi

key="$*"
printf '%s\n' "$key" >> "${WEZTERM_TEST_LOG}"

safe_key="$(printf '%s' "$key" | tr -c 'A-Za-z0-9._-' '_')"
status_file="${WEZTERM_TEST_RESPONSES_DIR}/${safe_key}.status"
stdout_file="${WEZTERM_TEST_RESPONSES_DIR}/${safe_key}.stdout"
stderr_file="${WEZTERM_TEST_RESPONSES_DIR}/${safe_key}.stderr"

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
            .expect("failed to write fake wezterm script");

            let mut perms = fs::metadata(&fake_wezterm)
                .expect("failed to stat fake wezterm")
                .permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&fake_wezterm, perms)
                .expect("failed to mark fake wezterm executable");

            let old_path = std::env::var_os("PATH");
            let old_runtime_dir = std::env::var_os("XDG_RUNTIME_DIR");
            let old_responses_dir = std::env::var_os("WEZTERM_TEST_RESPONSES_DIR");
            let old_log_file = std::env::var_os("WEZTERM_TEST_LOG");
            let old_config = crate::config::snapshot();

            let mut path_entries = vec![bin_dir];
            if let Some(ref old) = old_path {
                path_entries.extend(std::env::split_paths(old));
            }
            let path = std::env::join_paths(path_entries).expect("failed to join PATH entries");

            std::env::set_var("PATH", path);
            std::env::set_var("XDG_RUNTIME_DIR", &runtime_dir);
            std::env::set_var("WEZTERM_TEST_RESPONSES_DIR", &responses_dir);
            std::env::set_var("WEZTERM_TEST_LOG", &log_file);

            let config_dir = base.join("config");
            fs::create_dir_all(&config_dir).expect("config dir should be creatable");
            let config_path = config_dir.join("config.toml");
            fs::write(
                &config_path,
                "[app.terminal.wezterm]\nenabled = true\nmux_backend = \"wezterm\"\n",
            )
            .expect("config file should be writable");
            crate::config::prepare_with_path(Some(&config_path)).expect("config should load");

            Self {
                base,
                responses_dir,
                log_file,
                old_path,
                old_runtime_dir,
                old_responses_dir,
                old_log_file,
                old_config,
            }
        }

        fn set_response(&self, key: &str, status: i32, stdout: &str, stderr: &str) {
            let safe_key: String = key
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                        c
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

    impl Drop for WeztermHarness {
        fn drop(&mut self) {
            if let Some(value) = &self.old_path {
                std::env::set_var("PATH", value);
            } else {
                std::env::remove_var("PATH");
            }

            if let Some(value) = &self.old_runtime_dir {
                std::env::set_var("XDG_RUNTIME_DIR", value);
            } else {
                std::env::remove_var("XDG_RUNTIME_DIR");
            }

            if let Some(value) = &self.old_responses_dir {
                std::env::set_var("WEZTERM_TEST_RESPONSES_DIR", value);
            } else {
                std::env::remove_var("WEZTERM_TEST_RESPONSES_DIR");
            }

            if let Some(value) = &self.old_log_file {
                std::env::set_var("WEZTERM_TEST_LOG", value);
            } else {
                std::env::remove_var("WEZTERM_TEST_LOG");
            }

            crate::config::install(self.old_config.clone());

            let _ = fs::remove_dir_all(&self.base);
        }
    }

    #[test]
    fn active_foreground_process_prefers_focused_client_pane() {
        let _env_guard = env_guard();
        let pid = 4242;
        let harness = WeztermHarness::new(pid);

        harness.set_response(
            "list --format json",
            0,
            r#"
            [
              {"pane_id":11,"tab_id":1,"is_active":true,"foreground_process_name":"bash"},
              {"pane_id":42,"tab_id":2,"is_active":true,"foreground_process_name":"tmux"}
            ]
            "#,
            "",
        );
        harness.set_response(
            "list-clients --format json",
            0,
            r#"[{"pid":9999,"focused_pane_id":11},{"pid":4242,"focused_pane_id":42}]"#,
            "",
        );

        let fg = WeztermBackend::mux_provider().active_foreground_process(pid);
        assert_eq!(fg.as_deref(), Some("tmux"));
    }

    #[test]
    fn can_focus_works_when_list_clients_is_unavailable() {
        let _env_guard = env_guard();
        let pid = 5151;
        let harness = WeztermHarness::new(pid);

        harness.set_response(
            "list --format json",
            0,
            r#"[{"pane_id":7,"tab_id":1,"is_active":true,"foreground_process_name":"zsh"}]"#,
            "",
        );
        harness.set_response(
            "list-clients --format json",
            1,
            "",
            "unknown subcommand: list-clients",
        );
        harness.set_response("get-pane-direction Left --pane-id 7", 1, "", "no pane");

        let app = WeztermBackend;
        let can_focus = app
            .can_focus(Direction::West, pid)
            .expect("can_focus should gracefully fall back");
        assert!(!can_focus);
    }

    #[test]
    fn move_decision_tears_out_when_no_neighbor_in_direction() {
        let _env_guard = env_guard();
        let pid = 6262;
        let harness = WeztermHarness::new(pid);

        harness.set_response(
            "list --format json",
            0,
            r#"
            [
              {"pane_id":1,"tab_id":9,"is_active":false,"foreground_process_name":"zsh"},
              {"pane_id":2,"tab_id":9,"is_active":true,"foreground_process_name":"zsh"}
            ]
            "#,
            "",
        );
        harness.set_response(
            "list-clients --format json",
            0,
            r#"[{"pid":6262,"focused_pane_id":2}]"#,
            "",
        );
        harness.set_response("get-pane-direction Right --pane-id 2", 1, "", "no pane");
        harness.set_response("get-pane-direction Up --pane-id 2", 1, "", "no pane");
        harness.set_response("get-pane-direction Down --pane-id 2", 1, "", "no pane");

        let app = WeztermBackend;
        let decision = app
            .move_decision(Direction::East, pid)
            .expect("move_decision should succeed");
        assert!(matches!(decision, MoveDecision::TearOut));
    }

    #[test]
    fn move_decision_rearranges_when_perpendicular_neighbor_exists() {
        let _env_guard = env_guard();
        let pid = 6272;
        let harness = WeztermHarness::new(pid);

        harness.set_response(
            "list --format json",
            0,
            r#"
            [
              {"pane_id":1,"tab_id":9,"is_active":false,"foreground_process_name":"zsh"},
              {"pane_id":2,"tab_id":9,"is_active":true,"foreground_process_name":"zsh"}
            ]
            "#,
            "",
        );
        harness.set_response(
            "list-clients --format json",
            0,
            r#"[{"pid":6272,"focused_pane_id":2}]"#,
            "",
        );
        harness.set_response("get-pane-direction Up --pane-id 2", 1, "", "no pane");
        harness.set_response("get-pane-direction Left --pane-id 2", 0, "1\n", "");

        let app = WeztermBackend;
        let decision = app
            .move_decision(Direction::North, pid)
            .expect("move_decision should succeed");
        assert!(matches!(decision, MoveDecision::Rearrange));
    }

    #[test]
    fn move_internal_uses_neighbor_pane_as_split_anchor() {
        let _env_guard = env_guard();
        let pid = 7373;
        let harness = WeztermHarness::new(pid);

        harness.set_response(
            "list --format json",
            0,
            r#"[{"pane_id":10,"tab_id":3,"is_active":true,"foreground_process_name":"zsh"},{"pane_id":9,"tab_id":3,"is_active":false,"foreground_process_name":"zsh"}]"#,
            "",
        );
        harness.set_response(
            "list-clients --format json",
            0,
            r#"[{"pid":7373,"focused_pane_id":10}]"#,
            "",
        );
        harness.set_response("get-pane-direction Left --pane-id 10", 0, "9\n", "");
        harness.set_response("split-pane --pane-id 9 --left --move-pane-id 10", 0, "", "");

        let app = WeztermBackend;
        app.move_internal(Direction::West, pid)
            .expect("move_internal should succeed");

        let log = harness.command_log();
        assert!(log.contains("split-pane --pane-id 9 --left --move-pane-id 10"));
    }

    #[test]
    fn rearrange_falls_back_to_tab_peer_when_direction_probe_is_empty() {
        let _env_guard = env_guard();
        let pid = 7474;
        let harness = WeztermHarness::new(pid);

        harness.set_response(
            "list-clients --format json",
            0,
            r#"[{"pid":7474,"focused_pane_id":70}]"#,
            "",
        );
        harness.set_response("get-pane-direction Left --pane-id 70", 0, "", "");
        harness.set_response("get-pane-direction Right --pane-id 70", 0, "", "");
        harness.set_response(
            "list --format json",
            0,
            r#"[
              {"window_id":60,"pane_id":70,"tab_id":104,"is_active":true,"foreground_process_name":"zsh"},
              {"window_id":60,"pane_id":68,"tab_id":104,"is_active":false,"foreground_process_name":"zsh"}
            ]"#,
            "",
        );
        harness.set_response("split-pane --pane-id 68 --top --move-pane-id 70", 0, "", "");

        let app = WeztermBackend;
        app.rearrange(Direction::North, pid)
            .expect("rearrange should fallback to tab peer");

        let log = harness.command_log();
        assert!(log.contains("split-pane --pane-id 68 --top --move-pane-id 70"));
    }

    #[test]
    fn move_out_uses_move_pane_to_new_window() {
        let _env_guard = env_guard();
        let pid = 8484;
        let harness = WeztermHarness::new(pid);

        harness.set_response(
            "list --format json",
            0,
            r#"[{"pane_id":77,"tab_id":4,"is_active":true,"foreground_process_name":"zsh"}]"#,
            "",
        );
        harness.set_response(
            "list-clients --format json",
            0,
            r#"[{"pid":8484,"focused_pane_id":77}]"#,
            "",
        );
        harness.set_response("move-pane-to-new-tab --new-window --pane-id 77", 0, "", "");

        let app = WeztermBackend;
        let tear = app
            .move_out(Direction::East, pid)
            .expect("move_out should succeed");
        assert!(tear.spawn_command.is_none());
    }

    #[test]
    fn resize_internal_uses_adjust_pane_size_command() {
        let _env_guard = env_guard();
        let pid = 8585;
        let harness = WeztermHarness::new(pid);

        harness.set_response(
            "list --format json",
            0,
            r#"[{"pane_id":10,"tab_id":3,"is_active":true,"foreground_process_name":"zsh"},{"pane_id":9,"tab_id":3,"is_active":false,"foreground_process_name":"zsh"}]"#,
            "",
        );
        harness.set_response(
            "list-clients --format json",
            0,
            r#"[{"pid":8585,"focused_pane_id":10}]"#,
            "",
        );
        harness.set_response("adjust-pane-size --pane-id 10 --amount 40 Right", 0, "", "");

        let app = WeztermBackend;
        app.resize_internal(Direction::East, true, 40, pid)
            .expect("resize_internal should succeed");

        let log = harness.command_log();
        assert!(log.contains("adjust-pane-size --pane-id 10 --amount 40 Right"));
    }

    #[test]
    fn merge_source_pane_uses_opposite_split_side_on_target() {
        let _env_guard = env_guard();
        let pid = 9595;
        let harness = WeztermHarness::new(pid);

        harness.set_response(
            "list --format json",
            0,
            r#"[{"pane_id":9,"tab_id":5,"is_active":true,"foreground_process_name":"zsh"}]"#,
            "",
        );
        harness.set_response(
            "list-clients --format json",
            0,
            r#"[{"pid":9595,"focused_pane_id":9}]"#,
            "",
        );
        harness.set_response(
            "split-pane --pane-id 9 --right --move-pane-id 10",
            0,
            "",
            "",
        );

        WeztermBackend::mux_provider()
            .merge_source_pane_into_focused_target(pid, 10, pid, None, Direction::West)
            .expect("merge should succeed");
        let log = harness.command_log();
        assert!(log.contains("split-pane --pane-id 9 --right --move-pane-id 10"));
    }

    #[test]
    fn merge_source_pane_with_target_hint_uses_direct_cli() {
        let _env_guard = env_guard();
        let pid = 9711;
        let harness = WeztermHarness::new(pid);

        harness.set_response(
            "list --format json",
            0,
            r#"[
              {"window_id":1,"pane_id":10,"tab_id":5,"is_active":true,"foreground_process_name":"zsh"},
              {"window_id":2,"pane_id":20,"tab_id":6,"is_active":true,"foreground_process_name":"zsh"}
            ]"#,
            "",
        );
        harness.set_response(
            "split-pane --pane-id 20 --right --move-pane-id 10",
            0,
            "",
            "",
        );

        WeztermBackend::mux_provider()
            .merge_source_pane_into_focused_target(pid, 10, pid, Some(2), Direction::West)
            .expect("merge with explicit target should use direct cli");

        let log = harness.command_log();
        assert!(log.contains("split-pane --pane-id 20 --right --move-pane-id 10"));
    }

    #[test]
    fn merge_source_pane_uses_direct_cli_by_default() {
        let _env_guard = env_guard();
        let pid = 9717;
        let harness = WeztermHarness::new(pid);

        harness.set_response(
            "list --format json",
            0,
            r#"[{"pane_id":9,"tab_id":5,"is_active":true,"foreground_process_name":"zsh"}]"#,
            "",
        );
        harness.set_response(
            "list-clients --format json",
            0,
            r#"[{"pid":9717,"focused_pane_id":9}]"#,
            "",
        );
        harness.set_response(
            "split-pane --pane-id 9 --right --move-pane-id 10",
            0,
            "",
            "",
        );

        WeztermBackend::mux_provider()
            .merge_source_pane_into_focused_target(pid, 10, pid, None, Direction::West)
            .expect("direct merge should succeed");

        let log = harness.command_log();
        assert!(log.contains("split-pane --pane-id 9 --right --move-pane-id 10"));
    }

    #[test]
    fn merge_source_pane_resolves_target_from_other_window_when_client_focus_is_source() {
        let _env_guard = env_guard();
        let pid = 9797;
        let harness = WeztermHarness::new(pid);

        harness.set_response(
            "list --format json",
            0,
            r#"[
              {"window_id":1,"pane_id":1,"tab_id":1,"is_active":true,"foreground_process_name":"zsh"},
              {"window_id":0,"pane_id":0,"tab_id":0,"is_active":true,"foreground_process_name":"zsh"}
            ]"#,
            "",
        );
        harness.set_response(
            "list-clients --format json",
            0,
            r#"[{"pid":9797,"focused_pane_id":1}]"#,
            "",
        );
        harness.set_response("split-pane --pane-id 0 --right --move-pane-id 1", 0, "", "");

        WeztermBackend::mux_provider()
            .merge_source_pane_into_focused_target(pid, 1, pid, None, Direction::West)
            .expect("merge should resolve target pane from other window");

        let log = harness.command_log();
        assert!(log.contains("split-pane --pane-id 0 --right --move-pane-id 1"));
    }

    #[test]
    fn merge_source_pane_prefers_explicit_target_window_hint() {
        let _env_guard = env_guard();
        let pid = 9808;
        let harness = WeztermHarness::new(pid);

        harness.set_response(
            "list --format json",
            0,
            r#"[
              {"window_id":1,"pane_id":1,"tab_id":1,"is_active":true,"foreground_process_name":"zsh"},
              {"window_id":2,"pane_id":2,"tab_id":2,"is_active":false,"foreground_process_name":"zsh"},
              {"window_id":3,"pane_id":3,"tab_id":3,"is_active":false,"foreground_process_name":"zsh"}
            ]"#,
            "",
        );
        harness.set_response(
            "list-clients --format json",
            0,
            r#"[{"pid":9808,"focused_pane_id":1}]"#,
            "",
        );
        harness.set_response("split-pane --pane-id 2 --right --move-pane-id 1", 0, "", "");

        WeztermBackend::mux_provider()
            .merge_source_pane_into_focused_target(pid, 1, pid, Some(2), Direction::West)
            .expect("merge should target hinted window pane");

        let log = harness.command_log();
        assert!(log.contains("split-pane --pane-id 2 --right --move-pane-id 1"));
    }
}
