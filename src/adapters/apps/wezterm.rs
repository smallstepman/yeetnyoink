//! # WezTerm integration capability map (for niri-deep)
//!
//! This module implements the WezTerm side of directional focus/move semantics used by
//! `niri-deep`, including:
//!
//! - pane-local directional focus,
//! - in-window pane rearrange,
//! - tear-out to new window,
//! - merge back into a neighboring WezTerm window,
//! - optional mux-bridge orchestration for ambiguous focus transitions.
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
//! niri-deep currently uses CLI-first control, with an optional Lua/mux bridge path
//! (`NIRI_DEEP_WEZTERM_MUX_BRIDGE=1`) for cases where deciding merge targets purely from
//! transient CLI focus metadata is less reliable.
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
//! - `wezterm cli` can prefer the mux server with `--prefer-mux`; niri-deep currently
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
//! This split between CLI and Lua capabilities is why the bridge strategy can be useful:
//! Rust can enqueue intent, and WezTerm-side code can execute in the correct focused GUI
//! context while still using `wezterm cli split-pane --move-pane-id`.
//!
//! ## GUI <-> mux mapping capabilities
//!
//! - `window:mux_window()` converts GUI window -> mux window.
//! - `mux_window:gui_window()` converts mux window -> GUI window (when visible/active).
//! - `wezterm.gui.gui_window_for_mux_window(window_id)` resolves mux window id to GUI
//!   window object, when such mapping exists in the active workspace.
//! - `wezterm.gui.gui_windows()` lists GUI windows in stable order.
//! - `window:is_focused()` allows focus-gated bridge processing to avoid wrong-window
//!   command consumption.
//!
//! ## Events and lifecycle hooks worth knowing
//!
//! - `update-status` is the current periodic status event.
//! - `update-right-status` is older/deprecated but still commonly used and present in this
//!   repo for bridge polling cadence.
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
//! These matter if niri-deep is later extended to cross-domain or workspace-aware routing.
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
//! - https://wezterm.org/config/lua/window-events/update-right-status.html
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
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::adapters::apps::AppAdapter;
use crate::config::TerminalMuxBackend;
use crate::engine::contract::{
    AdapterCapabilities, AppKind, MergeExecutionMode, MergePreparation, MoveDecision, TearResult,
    TopologyHandler,
};
use crate::engine::runtime::ProcessId;
use crate::engine::topology::Direction;
use crate::logging;

#[derive(Debug, Deserialize)]
struct WezPaneInfo {
    #[serde(default)]
    window_id: u64,
    pane_id: u64,
    tab_id: u64,
    #[serde(default)]
    is_active: bool,
    #[serde(default)]
    foreground_process_name: String,
}

#[derive(Debug, Deserialize)]
struct WezClientInfo {
    pid: u32,
    focused_pane_id: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClientFocusSelection {
    MatchingPid(u64),
    AnyClient(u64),
}

impl ClientFocusSelection {
    fn pane_id(self) -> u64 {
        match self {
            Self::MatchingPid(pane_id) | Self::AnyClient(pane_id) => pane_id,
        }
    }
}

pub struct WeztermBackend;
pub const ADAPTER_NAME: &str = "terminal";
pub const ADAPTER_ALIASES: &[&str] = &["wezterm", "terminal"];
pub const APP_IDS: &[&str] = &["org.wezfurlong.wezterm"];

struct WeztermMergePreparation {
    pane_id: u64,
    target_window_id: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MuxBridgeMode {
    Disabled,
    Enabled,
    Auto,
}

impl WeztermBackend {
    const NON_SOURCE_PANE_POLL_ATTEMPTS: usize = 3;
    const NON_SOURCE_PANE_POLL_DELAY: Duration = Duration::from_millis(10);
    const MUX_BRIDGE_READY_MAX_AGE: Duration = Duration::from_secs(2);
    const MUX_BRIDGE_READY_FILE: &'static str = "ready";

    fn mux_policy() -> crate::config::MuxPolicy {
        crate::config::mux_policy_for(ADAPTER_ALIASES)
    }

    fn mux_bridge_mode() -> MuxBridgeMode {
        let mux_policy = Self::mux_policy();
        if !mux_policy.integration_enabled {
            return MuxBridgeMode::Disabled;
        }
        if let Some(enabled) = mux_policy.bridge_enable_override() {
            if !enabled {
                return MuxBridgeMode::Disabled;
            }
            return match mux_policy.backend {
                TerminalMuxBackend::Wezterm => MuxBridgeMode::Enabled,
                TerminalMuxBackend::Tmux
                | TerminalMuxBackend::Zellij
                | TerminalMuxBackend::Kitty => MuxBridgeMode::Disabled,
            };
        }
        match mux_policy.backend {
            TerminalMuxBackend::Wezterm => MuxBridgeMode::Auto,
            TerminalMuxBackend::Tmux | TerminalMuxBackend::Zellij | TerminalMuxBackend::Kitty => {
                MuxBridgeMode::Disabled
            }
        }
    }

    fn should_use_mux_bridge() -> bool {
        match Self::mux_bridge_mode() {
            MuxBridgeMode::Disabled => false,
            MuxBridgeMode::Enabled => true,
            MuxBridgeMode::Auto => Self::mux_bridge_ready(),
        }
    }

    fn enqueue_mux_merge_command(source_pane_id: u64, dir: Direction) -> Result<()> {
        let dir_name = dir.to_string();
        let command = format!("merge {source_pane_id} {dir_name}\n");
        let dir_path = Self::mux_bridge_dir();
        fs::create_dir_all(&dir_path)
            .with_context(|| format!("failed to create mux bridge dir: {}", dir_path.display()))?;
        let final_path = dir_path.join("merge.cmd");
        let temp_path = dir_path.join(format!(
            "merge.cmd.tmp-{}-{}",
            std::process::id(),
            source_pane_id
        ));
        fs::write(&temp_path, command).with_context(|| {
            format!(
                "failed to write mux bridge temp command: {}",
                temp_path.display()
            )
        })?;
        fs::rename(&temp_path, &final_path).with_context(|| {
            format!(
                "failed to publish mux bridge command {} -> {}",
                temp_path.display(),
                final_path.display()
            )
        })?;
        Ok(())
    }

    fn mux_bridge_dir() -> PathBuf {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(runtime_dir).join("niri-deep-wezterm-mux")
    }

    fn mux_bridge_ready_path() -> PathBuf {
        Self::mux_bridge_dir().join(Self::MUX_BRIDGE_READY_FILE)
    }

    fn mux_bridge_ready() -> bool {
        let ready_path = Self::mux_bridge_ready_path();
        let Ok(metadata) = fs::metadata(&ready_path) else {
            return false;
        };
        let Ok(modified) = metadata.modified() else {
            return false;
        };
        let Ok(age) = modified.elapsed() else {
            return false;
        };
        age <= Self::MUX_BRIDGE_READY_MAX_AGE
    }

    pub fn focused_pane_for_pid(pid: u32) -> Result<u64> {
        Self::focused_pane_id(pid)
    }

    pub fn pane_neighbor_for_pid(pid: u32, pane_id: u64, dir: Direction) -> Result<u64> {
        Self::pane_in_direction(pid, pane_id, dir)?
            .context("no terminal multiplexer pane exists in requested direction")
    }

    pub fn send_text_to_pane(pid: u32, pane_id: u64, text: &str) -> Result<()> {
        let pane_id_str = pane_id.to_string();
        let output = Self::cli_output(
            pid,
            &["send-text", "--pane-id", &pane_id_str, "--no-paste", text],
        )?;
        if !output.status.success() {
            bail!(
                "terminal multiplexer send-text failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }

    pub fn spawn_attach_command(target: String) -> Vec<String> {
        vec![
            "wezterm".into(),
            "-e".into(),
            "tmux".into(),
            "attach-session".into(),
            "-t".into(),
            target,
        ]
    }

    pub fn merge_source_pane_into_focused_target(
        source_pid: u32,
        source_pane_id: u64,
        target_pid: u32,
        target_window_id: Option<u64>,
        dir: Direction,
    ) -> Result<()> {
        if source_pid == 0 || target_pid == 0 {
            bail!("invalid wezterm pid for merge");
        }
        if source_pid != target_pid {
            bail!("cannot merge panes across different wezterm instances");
        }

        if Self::should_use_mux_bridge() && target_window_id.is_none() {
            logging::debug(format!(
                "wezterm: mux bridge enabled; enqueue merge source pane {} dir={}",
                source_pane_id, dir
            ));
            Self::enqueue_mux_merge_command(source_pane_id, dir)?;
            return Ok(());
        }
        if Self::should_use_mux_bridge() && target_window_id.is_some() {
            logging::debug(
                "wezterm: skipping mux bridge because explicit merge target is available",
            );
        }
        logging::debug("wezterm: mux bridge unavailable; using direct cli merge path");

        let target_pane_id = if let Some(window_id) = target_window_id {
            Self::merge_target_pane_id(target_pid, source_pane_id, Some(window_id))?
        } else {
            if let Some(pane_id) =
                Self::wait_for_non_source_focused_client_pane(target_pid, source_pane_id)?
            {
                logging::debug(format!(
                    "wezterm: merge target from focused client transition = {}",
                    pane_id
                ));
                pane_id
            } else {
                Self::merge_target_pane_id(target_pid, source_pane_id, None)?
            }
        };
        if target_pane_id == source_pane_id {
            bail!("source and target panes are the same");
        }

        let target_pane_id_str = target_pane_id.to_string();
        let source_pane_id_str = source_pane_id.to_string();
        let target_side = Self::split_flag(dir.opposite());
        logging::debug(format!(
            "wezterm: merge source pane {} into target pane {} side={}",
            source_pane_id, target_pane_id, target_side
        ));
        Self::cli_stdout(
            target_pid,
            &[
                "split-pane",
                "--pane-id",
                &target_pane_id_str,
                target_side,
                "--move-pane-id",
                &source_pane_id_str,
            ],
        )?;
        Ok(())
    }

    fn wait_for_non_source_focused_client_pane(
        pid: u32,
        source_pane_id: u64,
    ) -> Result<Option<u64>> {
        for attempt in 0..Self::NON_SOURCE_PANE_POLL_ATTEMPTS {
            if let Some(pane_id) = Self::focused_client_pane_id(pid)? {
                if pane_id != source_pane_id {
                    return Ok(Some(pane_id));
                }
            }
            if attempt + 1 < Self::NON_SOURCE_PANE_POLL_ATTEMPTS {
                std::thread::sleep(Self::NON_SOURCE_PANE_POLL_DELAY);
            }
        }
        Ok(None)
    }

    fn focused_client_pane_id(pid: u32) -> Result<Option<u64>> {
        let panes = Self::list_panes(pid)?;
        let clients = Self::list_clients(pid)?;
        let pane_exists = |pane_id: u64| panes.iter().any(|p| p.pane_id == pane_id);

        if let Some(selection) = Self::select_client_focused_pane(&clients, pid, pane_exists) {
            return Ok(Some(selection.pane_id()));
        }

        Ok(None)
    }

    fn merge_target_pane_id(
        pid: u32,
        source_pane_id: u64,
        target_window_id: Option<u64>,
    ) -> Result<u64> {
        let panes = Self::list_panes(pid)?;
        let clients = Self::list_clients(pid)?;
        let pane_exists = |pane_id: u64| panes.iter().any(|p| p.pane_id == pane_id);
        let not_source = |pane_id: u64| pane_id != source_pane_id;

        if let Some(target_window_id) = target_window_id {
            let mut candidates: Vec<&WezPaneInfo> = panes
                .iter()
                .filter(|pane| pane.window_id == target_window_id && pane.pane_id != source_pane_id)
                .collect();
            candidates.sort_by_key(|pane| pane.pane_id);
            if let Some(active) = candidates.iter().copied().find(|pane| pane.is_active) {
                logging::debug(format!(
                    "wezterm: merge target from explicit window {} active pane = {}",
                    target_window_id, active.pane_id
                ));
                return Ok(active.pane_id);
            }
            if let Some(first) = candidates.first() {
                logging::debug(format!(
                    "wezterm: merge target from explicit window {} pane = {}",
                    target_window_id, first.pane_id
                ));
                return Ok(first.pane_id);
            }
        }

        if let Some(selection) = Self::select_client_focused_pane(&clients, pid, |pane_id| {
            pane_exists(pane_id) && not_source(pane_id)
        }) {
            let pane_id = selection.pane_id();
            let origin = match selection {
                ClientFocusSelection::MatchingPid(_) => "matching client focused pane",
                ClientFocusSelection::AnyClient(_) => "any client focused pane",
            };
            logging::debug(format!(
                "wezterm: merge target from {} = {}",
                origin, pane_id
            ));
            return Ok(pane_id);
        }

        if let Some(source_window_id) = panes
            .iter()
            .find(|p| p.pane_id == source_pane_id)
            .map(|p| p.window_id)
        {
            let mut different_window_candidates: Vec<u64> = panes
                .iter()
                .filter(|p| p.window_id != source_window_id)
                .map(|p| p.pane_id)
                .collect();
            different_window_candidates.sort_unstable();
            different_window_candidates.dedup();
            if different_window_candidates.len() == 1 {
                let pane_id = different_window_candidates[0];
                logging::debug(format!(
                    "wezterm: merge target from non-source window candidate = {}",
                    pane_id
                ));
                return Ok(pane_id);
            }

            let mut active_different_window_candidates: Vec<u64> = panes
                .iter()
                .filter(|p| p.window_id != source_window_id && p.is_active)
                .map(|p| p.pane_id)
                .collect();
            active_different_window_candidates.sort_unstable();
            active_different_window_candidates.dedup();
            if active_different_window_candidates.len() == 1 {
                let pane_id = active_different_window_candidates[0];
                logging::debug(format!(
                    "wezterm: merge target from active non-source window candidate = {}",
                    pane_id
                ));
                return Ok(pane_id);
            } else if active_different_window_candidates.len() > 1 {
                logging::debug(format!(
                    "wezterm: ambiguous active non-source window candidates = {:?}",
                    active_different_window_candidates
                ));
                bail!("ambiguous merge target pane across non-source windows");
            }
        }

        let mut other_panes: Vec<u64> = panes
            .iter()
            .filter(|p| p.pane_id != source_pane_id)
            .map(|p| p.pane_id)
            .collect();
        other_panes.sort_unstable();
        other_panes.dedup();
        if other_panes.len() == 1 {
            let pane_id = other_panes[0];
            logging::debug(format!(
                "wezterm: merge target from sole non-source pane candidate = {}",
                pane_id
            ));
            return Ok(pane_id);
        }

        bail!("unable to resolve merge target pane (ambiguous)")
    }

    fn wezterm_socket_path(pid: u32) -> Result<PathBuf> {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .context("XDG_RUNTIME_DIR is not set; cannot locate wezterm socket")?;
        let path = PathBuf::from(format!("{runtime_dir}/wezterm/gui-sock-{pid}"));
        if !path.exists() {
            bail!("wezterm socket not found: {}", path.display());
        }
        Ok(path)
    }

    fn cli_output(pid: u32, args: &[&str]) -> Result<std::process::Output> {
        let sock = Self::wezterm_socket_path(pid)?;
        let sock = sock.to_string_lossy().to_string();
        logging::debug(format!(
            "wezterm: pid={} cli {:?} via WEZTERM_UNIX_SOCKET",
            pid, args
        ));
        let output = Command::new("wezterm")
            .env("WEZTERM_UNIX_SOCKET", &sock)
            .args(["cli"])
            .args(args)
            .output()
            .context("failed to run wezterm cli")?;
        logging::debug(format!(
            "wezterm: pid={} cli {:?} status={} stdout={:?} stderr={:?}",
            pid,
            args,
            output.status,
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
        Ok(output)
    }

    fn cli_stdout(pid: u32, args: &[&str]) -> Result<String> {
        let output = Self::cli_output(pid, args)?;
        if !output.status.success() {
            bail!(
                "wezterm cli {:?} failed: {}",
                args,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn list_panes(pid: u32) -> Result<Vec<WezPaneInfo>> {
        let stdout = Self::cli_stdout(pid, &["list", "--format", "json"])?;
        let panes: Vec<WezPaneInfo> =
            serde_json::from_str(&stdout).context("failed to parse wezterm pane list json")?;
        Ok(panes)
    }

    fn list_clients(pid: u32) -> Result<Vec<WezClientInfo>> {
        let output = match Self::cli_output(pid, &["list-clients", "--format", "json"]) {
            Ok(output) => output,
            // Graceful fallback for older wezterm builds that may not support list-clients.
            Err(e) => {
                logging::debug(format!(
                    "wezterm: pid={} list-clients unavailable, continuing without it: {e:#}",
                    pid
                ));
                return Ok(vec![]);
            }
        };

        if !output.status.success() {
            logging::debug(format!(
                "wezterm: pid={} list-clients exited non-zero, ignoring",
                pid
            ));
            return Ok(vec![]);
        }

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if stdout.is_empty() {
            logging::debug(format!(
                "wezterm: pid={} list-clients returned empty output",
                pid
            ));
            return Ok(vec![]);
        }

        let clients: Vec<WezClientInfo> =
            serde_json::from_str(&stdout).context("failed to parse wezterm client list json")?;
        Ok(clients)
    }

    fn select_client_focused_pane<F>(
        clients: &[WezClientInfo],
        pid: u32,
        mut accept: F,
    ) -> Option<ClientFocusSelection>
    where
        F: FnMut(u64) -> bool,
    {
        clients
            .iter()
            .find_map(|client| {
                (client.pid == pid && client.focused_pane_id > 0 && accept(client.focused_pane_id))
                    .then_some(ClientFocusSelection::MatchingPid(client.focused_pane_id))
            })
            .or_else(|| {
                clients.iter().find_map(|client| {
                    (client.focused_pane_id > 0 && accept(client.focused_pane_id))
                        .then_some(ClientFocusSelection::AnyClient(client.focused_pane_id))
                })
            })
    }

    fn focused_pane_id(pid: u32) -> Result<u64> {
        let clients = Self::list_clients(pid)?;
        logging::debug(format!(
            "wezterm: pid={} focused-pane lookup clients={}",
            pid,
            clients.len()
        ));

        if let Some(selection) = Self::select_client_focused_pane(&clients, pid, |_| true) {
            let pane_id = selection.pane_id();
            match selection {
                ClientFocusSelection::MatchingPid(_) => logging::debug(format!(
                    "wezterm: pid={} focused pane from matching client = {}",
                    pid, pane_id
                )),
                ClientFocusSelection::AnyClient(_) => logging::debug(format!(
                    "wezterm: pid={} focused pane from any client fallback = {}",
                    pid, pane_id
                )),
            }
            return Ok(pane_id);
        }

        let panes = Self::list_panes(pid)?;
        if let Some(pane_id) = panes.iter().find(|p| p.is_active).map(|p| p.pane_id) {
            logging::debug(format!(
                "wezterm: pid={} focused pane from active pane fallback = {}",
                pid, pane_id
            ));
            return Ok(pane_id);
        }

        logging::debug(format!(
            "wezterm: pid={} unable to determine focused pane",
            pid
        ));
        bail!("unable to determine focused wezterm pane")
    }

    fn pane_count_in_active_tab(pid: u32, active_pane_id: u64) -> Result<u32> {
        let panes = Self::list_panes(pid)?;
        let active_tab_id = panes
            .iter()
            .find(|p| p.pane_id == active_pane_id)
            .map(|p| p.tab_id)
            .context("active pane is not present in wezterm pane list")?;
        Ok(panes.iter().filter(|p| p.tab_id == active_tab_id).count() as u32)
    }

    fn direction_name(dir: Direction) -> &'static str {
        match dir.egocentric() {
            "left" => "Left",
            "right" => "Right",
            "up" => "Up",
            "down" => "Down",
            _ => unreachable!("invalid egocentric direction"),
        }
    }

    fn split_flag(dir: Direction) -> &'static str {
        match dir.positional() {
            "left" => "--left",
            "right" => "--right",
            "top" => "--top",
            "bottom" => "--bottom",
            _ => unreachable!("invalid positional direction"),
        }
    }

    fn pane_in_direction(pid: u32, pane_id: u64, dir: Direction) -> Result<Option<u64>> {
        let pane_id_str = pane_id.to_string();
        let output = Self::cli_output(
            pid,
            &[
                "get-pane-direction",
                Self::direction_name(dir),
                "--pane-id",
                &pane_id_str,
            ],
        )?;

        if !output.status.success() {
            return Ok(None);
        }

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if stdout.is_empty() {
            return Ok(None);
        }
        let id = stdout.parse::<u64>().with_context(|| {
            format!("invalid pane id from wezterm get-pane-direction: {stdout}")
        })?;
        Ok(Some(id))
    }

    fn has_neighbor(pid: u32, pane_id: u64, dir: Direction) -> Result<bool> {
        Ok(Self::pane_in_direction(pid, pane_id, dir)?.is_some())
    }

    fn fallback_rearrange_target(pid: u32, pane_id: u64) -> Result<Option<u64>> {
        let panes = Self::list_panes(pid)?;
        let active_tab_id = panes
            .iter()
            .find(|pane| pane.pane_id == pane_id)
            .map(|pane| pane.tab_id)
            .context("active pane is not present in wezterm pane list")?;
        let mut candidates: Vec<u64> = panes
            .into_iter()
            .filter(|pane| pane.tab_id == active_tab_id && pane.pane_id != pane_id)
            .map(|pane| pane.pane_id)
            .collect();
        candidates.sort_unstable();
        Ok(candidates.into_iter().next())
    }

    /// Returns the foreground process name of the active pane for the WezTerm
    /// instance identified by `pid`, using `wezterm cli list --format json`.
    pub fn active_foreground_process(pid: u32) -> Option<String> {
        let pane_id = Self::focused_pane_id(pid).ok()?;
        let panes = Self::list_panes(pid).ok()?;
        panes
            .into_iter()
            .find(|p| p.pane_id == pane_id)
            .and_then(|p| {
                let name = p.foreground_process_name.trim().to_string();
                (!name.is_empty()).then_some(name)
            })
    }
}

impl AppAdapter for WeztermBackend {
    fn adapter_name(&self) -> &'static str {
        ADAPTER_NAME
    }

    fn config_aliases(&self) -> Option<&'static [&'static str]> {
        Some(ADAPTER_ALIASES)
    }

    fn kind(&self) -> AppKind {
        AppKind::Terminal
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            probe: true,
            focus: true,
            move_internal: true,
            resize_internal: true,
            rearrange: true,
            tear_out: true,
            merge: true,
        }
    }
}

impl TopologyHandler for WeztermBackend {
    fn can_focus(&self, dir: Direction, pid: u32) -> Result<bool> {
        let pane_id = Self::focused_pane_id(pid)?;
        Self::has_neighbor(pid, pane_id, dir)
    }

    fn move_decision(&self, dir: Direction, pid: u32) -> Result<MoveDecision> {
        let pane_id = Self::focused_pane_id(pid)?;
        let pane_count = Self::pane_count_in_active_tab(pid, pane_id)?;
        if pane_count <= 1 {
            logging::debug(format!(
                "wezterm: move_decision dir={dir} pane_count={} => Passthrough",
                pane_count
            ));
            return Ok(MoveDecision::Passthrough);
        }

        if Self::has_neighbor(pid, pane_id, dir)? {
            logging::debug(format!("wezterm: move_decision dir={dir} => Internal"));
            return Ok(MoveDecision::Internal);
        }

        let has_perpendicular_neighbor = match dir {
            Direction::North | Direction::South => {
                Self::has_neighbor(pid, pane_id, Direction::West)?
                    || Self::has_neighbor(pid, pane_id, Direction::East)?
            }
            Direction::West | Direction::East => {
                Self::has_neighbor(pid, pane_id, Direction::North)?
                    || Self::has_neighbor(pid, pane_id, Direction::South)?
            }
        };
        if has_perpendicular_neighbor {
            logging::debug(format!("wezterm: move_decision dir={dir} => Rearrange"));
            return Ok(MoveDecision::Rearrange);
        }

        logging::debug(format!("wezterm: move_decision dir={dir} => TearOut"));
        Ok(MoveDecision::TearOut)
    }

    fn can_resize(&self, _dir: Direction, _grow: bool, _pid: u32) -> Result<bool> {
        Ok(true)
    }

    fn focus(&self, dir: Direction, pid: u32) -> Result<()> {
        let pane_id = Self::focused_pane_id(pid)?;
        let pane_id_str = pane_id.to_string();
        Self::cli_stdout(
            pid,
            &[
                "activate-pane-direction",
                Self::direction_name(dir),
                "--pane-id",
                &pane_id_str,
            ],
        )?;
        Ok(())
    }

    fn move_internal(&self, dir: Direction, pid: u32) -> Result<()> {
        let pane_id = Self::focused_pane_id(pid)?;
        let neighbor = Self::pane_in_direction(pid, pane_id, dir)?
            .context("no wezterm pane exists in the requested move direction")?;
        let pane_id_str = pane_id.to_string();
        let neighbor_str = neighbor.to_string();
        Self::cli_stdout(
            pid,
            &[
                "split-pane",
                "--pane-id",
                &neighbor_str,
                Self::split_flag(dir),
                "--move-pane-id",
                &pane_id_str,
            ],
        )?;
        Ok(())
    }

    fn resize_internal(&self, dir: Direction, grow: bool, step: i32, pid: u32) -> Result<()> {
        let pane_id = Self::focused_pane_id(pid)?;
        let pane_id_str = pane_id.to_string();
        let amount = step.max(1).to_string();
        let direction = if grow { dir } else { dir.opposite() };
        Self::cli_stdout(
            pid,
            &[
                "adjust-pane-size",
                "--pane-id",
                &pane_id_str,
                "--amount",
                &amount,
                Self::direction_name(direction),
            ],
        )?;
        Ok(())
    }

    fn rearrange(&self, dir: Direction, pid: u32) -> Result<()> {
        let pane_id = Self::focused_pane_id(pid)?;
        let target =
            match dir {
                Direction::North | Direction::South => {
                    Self::pane_in_direction(pid, pane_id, Direction::West)?
                        .or(Self::pane_in_direction(pid, pane_id, Direction::East)?)
                }
                Direction::West | Direction::East => {
                    Self::pane_in_direction(pid, pane_id, Direction::North)?
                        .or(Self::pane_in_direction(pid, pane_id, Direction::South)?)
                }
            }
            .or(Self::fallback_rearrange_target(pid, pane_id)?)
            .context("no perpendicular wezterm pane found for rearrange")?;

        let pane_id_str = pane_id.to_string();
        let target_str = target.to_string();
        Self::cli_stdout(
            pid,
            &[
                "split-pane",
                "--pane-id",
                &target_str,
                Self::split_flag(dir),
                "--move-pane-id",
                &pane_id_str,
            ],
        )?;
        Ok(())
    }

    fn move_out(&self, _dir: Direction, pid: u32) -> Result<TearResult> {
        let pane_id = Self::focused_pane_id(pid)?;
        let pane_id_str = pane_id.to_string();
        Self::cli_stdout(
            pid,
            &[
                "move-pane-to-new-tab",
                "--new-window",
                "--pane-id",
                &pane_id_str,
            ],
        )?;
        Ok(TearResult {
            spawn_command: None,
        })
    }

    fn merge_execution_mode(&self) -> MergeExecutionMode {
        MergeExecutionMode::TargetFocused
    }

    fn prepare_merge(&self, source_pid: Option<ProcessId>) -> Result<MergePreparation> {
        let source_pid = source_pid.context("source wezterm merge missing pid")?;
        let source_pane_id = Self::focused_pane_for_pid(source_pid.get())?;
        Ok(MergePreparation::with_payload(WeztermMergePreparation {
            pane_id: source_pane_id,
            target_window_id: None,
        }))
    }

    fn augment_merge_preparation_for_target(
        &self,
        preparation: MergePreparation,
        target_window_id: Option<u64>,
    ) -> MergePreparation {
        preparation.map_payload::<WeztermMergePreparation>(|mut preparation| {
            preparation.target_window_id = target_window_id;
            preparation
        })
    }

    fn merge_into_target(
        &self,
        dir: Direction,
        source_pid: Option<ProcessId>,
        target_pid: Option<ProcessId>,
        preparation: MergePreparation,
    ) -> Result<()> {
        let source_pid = source_pid.context("source wezterm merge missing pid")?;
        let preparation = preparation
            .into_payload::<WeztermMergePreparation>()
            .context("source wezterm merge missing pane id")?;
        let target_pid = target_pid.context("target wezterm merge missing pid")?;
        Self::merge_source_pane_into_focused_target(
            source_pid.get(),
            preparation.pane_id,
            target_pid.get(),
            preparation.target_window_id,
            dir,
        )
        .context("wezterm merge failed")
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::WeztermBackend;
    use crate::engine::contract::{AppAdapter, MoveDecision, TopologyHandler};
    use crate::engine::topology::Direction;

    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::utils::env_guard()
    }

    #[test]
    fn declares_explicit_capability_contract() {
        let app = WeztermBackend;
        let caps = AppAdapter::capabilities(&app);
        assert!(caps.probe);
        assert!(caps.focus);
        assert!(caps.move_internal);
        assert!(caps.resize_internal);
        assert!(caps.rearrange);
        assert!(caps.tear_out);
        assert!(caps.merge);
    }

    #[test]
    fn advertises_config_aliases_for_policy_binding() {
        let app = WeztermBackend;
        assert_eq!(app.config_aliases(), Some(super::ADAPTER_ALIASES));
    }

    struct WeztermHarness {
        base: PathBuf,
        runtime_dir: PathBuf,
        responses_dir: PathBuf,
        log_file: PathBuf,
        old_path: Option<OsString>,
        old_runtime_dir: Option<OsString>,
        old_responses_dir: Option<OsString>,
        old_log_file: Option<OsString>,
    }

    impl WeztermHarness {
        fn new(pid: u32) -> Self {
            let unique = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let base = std::env::temp_dir().join(format!("niri-deep-wezterm-test-{pid}-{unique}"));
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

            let mut path_entries = vec![bin_dir];
            if let Some(ref old) = old_path {
                path_entries.extend(std::env::split_paths(old));
            }
            let path = std::env::join_paths(path_entries).expect("failed to join PATH entries");

            std::env::set_var("PATH", path);
            std::env::set_var("XDG_RUNTIME_DIR", &runtime_dir);
            std::env::set_var("WEZTERM_TEST_RESPONSES_DIR", &responses_dir);
            std::env::set_var("WEZTERM_TEST_LOG", &log_file);

            Self {
                base,
                runtime_dir,
                responses_dir,
                log_file,
                old_path,
                old_runtime_dir,
                old_responses_dir,
                old_log_file,
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

        let fg = WeztermBackend::active_foreground_process(pid);
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
        let old_bridge = std::env::var_os("NIRI_DEEP_WEZTERM_MUX_BRIDGE");
        std::env::set_var("NIRI_DEEP_WEZTERM_MUX_BRIDGE", "0");

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

        WeztermBackend::merge_source_pane_into_focused_target(pid, 10, pid, None, Direction::West)
            .expect("merge should succeed");
        let log = harness.command_log();
        assert!(log.contains("split-pane --pane-id 9 --right --move-pane-id 10"));

        if let Some(value) = old_bridge {
            std::env::set_var("NIRI_DEEP_WEZTERM_MUX_BRIDGE", value);
        } else {
            std::env::remove_var("NIRI_DEEP_WEZTERM_MUX_BRIDGE");
        }
    }

    #[test]
    fn merge_source_pane_can_enqueue_mux_bridge_command() {
        let _env_guard = env_guard();
        let pid = 9696;
        let harness = WeztermHarness::new(pid);
        let bridge_dir = harness.runtime_dir.join("niri-deep-wezterm-mux");
        fs::create_dir_all(&bridge_dir).expect("bridge dir should be creatable");
        fs::write(bridge_dir.join("ready"), "ready\n").expect("ready marker should be writable");

        harness.set_response(
            "list --format json",
            0,
            r#"[{"pane_id":9,"tab_id":5,"is_active":true,"foreground_process_name":"zsh"}]"#,
            "",
        );
        harness.set_response(
            "list-clients --format json",
            0,
            r#"[{"pid":9696,"focused_pane_id":9}]"#,
            "",
        );

        WeztermBackend::merge_source_pane_into_focused_target(pid, 10, pid, None, Direction::West)
            .expect("merge enqueue should succeed");

        let bridge_cmd = harness
            .runtime_dir
            .join("niri-deep-wezterm-mux")
            .join("merge.cmd");
        let payload = fs::read_to_string(&bridge_cmd).expect("bridge command file should exist");
        assert_eq!(payload.trim(), "merge 10 west");

        let log = harness.command_log();
        assert!(!log.contains("split-pane --pane-id"));
    }

    #[test]
    fn merge_source_pane_auto_mode_uses_bridge_when_ready() {
        let _env_guard = env_guard();
        let pid = 9707;
        let harness = WeztermHarness::new(pid);
        let old_bridge = std::env::var_os("NIRI_DEEP_WEZTERM_MUX_BRIDGE");
        std::env::remove_var("NIRI_DEEP_WEZTERM_MUX_BRIDGE");

        let bridge_dir = harness.runtime_dir.join("niri-deep-wezterm-mux");
        fs::create_dir_all(&bridge_dir).expect("bridge dir should be creatable");
        fs::write(bridge_dir.join("ready"), "ready\n").expect("ready marker should be writable");

        WeztermBackend::merge_source_pane_into_focused_target(pid, 10, pid, None, Direction::West)
            .expect("auto bridge enqueue should succeed");

        let bridge_cmd = bridge_dir.join("merge.cmd");
        let payload = fs::read_to_string(&bridge_cmd).expect("bridge command file should exist");
        assert_eq!(payload.trim(), "merge 10 west");

        let log = harness.command_log();
        assert!(!log.contains("split-pane --pane-id"));

        if let Some(value) = old_bridge {
            std::env::set_var("NIRI_DEEP_WEZTERM_MUX_BRIDGE", value);
        } else {
            std::env::remove_var("NIRI_DEEP_WEZTERM_MUX_BRIDGE");
        }
    }

    #[test]
    fn merge_source_pane_with_target_hint_bypasses_bridge() {
        let _env_guard = env_guard();
        let pid = 9711;
        let harness = WeztermHarness::new(pid);
        let old_bridge = std::env::var_os("NIRI_DEEP_WEZTERM_MUX_BRIDGE");
        std::env::remove_var("NIRI_DEEP_WEZTERM_MUX_BRIDGE");

        let bridge_dir = harness.runtime_dir.join("niri-deep-wezterm-mux");
        fs::create_dir_all(&bridge_dir).expect("bridge dir should be creatable");
        fs::write(bridge_dir.join("ready"), "ready\n").expect("ready marker should be writable");

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

        WeztermBackend::merge_source_pane_into_focused_target(
            pid,
            10,
            pid,
            Some(2),
            Direction::West,
        )
        .expect("merge with explicit target should use direct cli");

        let log = harness.command_log();
        assert!(log.contains("split-pane --pane-id 20 --right --move-pane-id 10"));
        let bridge_cmd = bridge_dir.join("merge.cmd");
        assert!(!bridge_cmd.exists());

        if let Some(value) = old_bridge {
            std::env::set_var("NIRI_DEEP_WEZTERM_MUX_BRIDGE", value);
        } else {
            std::env::remove_var("NIRI_DEEP_WEZTERM_MUX_BRIDGE");
        }
    }

    #[test]
    fn merge_source_pane_defaults_to_direct_cli_when_bridge_not_ready() {
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

        WeztermBackend::merge_source_pane_into_focused_target(pid, 10, pid, None, Direction::West)
            .expect("default direct merge should succeed");

        let bridge_cmd = harness
            .runtime_dir
            .join("niri-deep-wezterm-mux")
            .join("merge.cmd");
        assert!(!bridge_cmd.exists());

        let log = harness.command_log();
        assert!(log.contains("split-pane --pane-id 9 --right --move-pane-id 10"));
    }

    #[test]
    fn merge_source_pane_resolves_target_from_other_window_when_client_focus_is_source() {
        let _env_guard = env_guard();
        let pid = 9797;
        let harness = WeztermHarness::new(pid);
        let old_bridge = std::env::var_os("NIRI_DEEP_WEZTERM_MUX_BRIDGE");
        std::env::set_var("NIRI_DEEP_WEZTERM_MUX_BRIDGE", "0");

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

        WeztermBackend::merge_source_pane_into_focused_target(pid, 1, pid, None, Direction::West)
            .expect("merge should resolve target pane from other window");

        let log = harness.command_log();
        assert!(log.contains("split-pane --pane-id 0 --right --move-pane-id 1"));

        if let Some(value) = old_bridge {
            std::env::set_var("NIRI_DEEP_WEZTERM_MUX_BRIDGE", value);
        } else {
            std::env::remove_var("NIRI_DEEP_WEZTERM_MUX_BRIDGE");
        }
    }

    #[test]
    fn merge_source_pane_prefers_explicit_target_window_hint() {
        let _env_guard = env_guard();
        let pid = 9808;
        let harness = WeztermHarness::new(pid);
        let old_bridge = std::env::var_os("NIRI_DEEP_WEZTERM_MUX_BRIDGE");
        std::env::set_var("NIRI_DEEP_WEZTERM_MUX_BRIDGE", "0");

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

        WeztermBackend::merge_source_pane_into_focused_target(
            pid,
            1,
            pid,
            Some(2),
            Direction::West,
        )
        .expect("merge should target hinted window pane");

        let log = harness.command_log();
        assert!(log.contains("split-pane --pane-id 2 --right --move-pane-id 1"));

        if let Some(value) = old_bridge {
            std::env::set_var("NIRI_DEEP_WEZTERM_MUX_BRIDGE", value);
        } else {
            std::env::remove_var("NIRI_DEEP_WEZTERM_MUX_BRIDGE");
        }
    }

    #[test]
    fn merge_source_pane_config_overrides_legacy_env_var_toggle() {
        let _env_guard = env_guard();
        let pid = 9898;
        let harness = WeztermHarness::new(pid);
        let old_bridge = std::env::var_os("NIRI_DEEP_WEZTERM_MUX_BRIDGE");
        let old_config_override = std::env::var_os("NIRI_DEEP_CONFIG");
        std::env::set_var("NIRI_DEEP_WEZTERM_MUX_BRIDGE", "1");

        let config_root = harness.base.join("config-root");
        let config_dir = config_root.join("niri-deep");
        fs::create_dir_all(&config_dir).expect("config dir should be creatable");
        fs::write(
            config_dir.join("config.toml"),
            r#"
[app.terminal.wezterm]
enabled = true
mux_backend = "wezterm"

[app.terminal.wezterm.mux]
enable = false
"#,
        )
        .expect("config file should be writable");
        std::env::set_var("NIRI_DEEP_CONFIG", config_dir.join("config.toml"));
        crate::config::prepare().expect("config should load");

        harness.set_response(
            "list --format json",
            0,
            r#"[{"pane_id":9,"tab_id":5,"is_active":true,"foreground_process_name":"zsh"}]"#,
            "",
        );
        harness.set_response(
            "list-clients --format json",
            0,
            r#"[{"pid":9898,"focused_pane_id":9}]"#,
            "",
        );
        harness.set_response(
            "split-pane --pane-id 9 --right --move-pane-id 10",
            0,
            "",
            "",
        );

        WeztermBackend::merge_source_pane_into_focused_target(pid, 10, pid, None, Direction::West)
            .expect("merge should use direct cli when config disables mux bridge");

        let log = harness.command_log();
        assert!(log.contains("split-pane --pane-id 9 --right --move-pane-id 10"));
        let bridge_cmd = harness
            .runtime_dir
            .join("niri-deep-wezterm-mux")
            .join("merge.cmd");
        assert!(!bridge_cmd.exists());

        if let Some(value) = old_bridge {
            std::env::set_var("NIRI_DEEP_WEZTERM_MUX_BRIDGE", value);
        } else {
            std::env::remove_var("NIRI_DEEP_WEZTERM_MUX_BRIDGE");
        }
        if let Some(value) = old_config_override {
            std::env::set_var("NIRI_DEEP_CONFIG", value);
        } else {
            std::env::remove_var("NIRI_DEEP_CONFIG");
        }
        crate::config::prepare().expect("config should reload");
    }
}
