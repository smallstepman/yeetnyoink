use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use super::WEZTERM_HOST_ALIASES;
use crate::config::TerminalMuxBackend;
use crate::engine::contract::{
    AdapterCapabilities, MergeExecutionMode, MergePreparation, MoveDecision, TearResult,
    TerminalMultiplexerProvider, TopologyHandler,
};
use crate::engine::runtime::ProcessId;
use crate::engine::topology::Direction;
use crate::logging;

#[derive(Debug, Deserialize)]
struct WeztermMuxPane {
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
struct WeztermMuxClient {
    pid: u32,
    focused_pane_id: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WeztermMuxFocusSelection {
    MatchingPid(u64),
    AnyClient(u64),
}

impl WeztermMuxFocusSelection {
    fn pane_id(self) -> u64 {
        match self {
            Self::MatchingPid(pane_id) | Self::AnyClient(pane_id) => pane_id,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct WeztermMux;
pub(crate) static WEZTERM_MUX_PROVIDER: WeztermMux = WeztermMux;

struct WeztermMuxMergePreparation {
    pane_id: u64,
    target_window_id: Option<u64>,
}

impl WeztermMux {
    fn cli_output(pid: u32, args: &[&str]) -> Result<std::process::Output> {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .context("XDG_RUNTIME_DIR is not set; cannot locate wezterm socket")?;
        let sock_path = PathBuf::from(format!("{runtime_dir}/wezterm/gui-sock-{pid}"));
        if !sock_path.exists() {
            bail!("wezterm socket not found: {}", sock_path.display());
        }
        let sock = sock_path.to_string_lossy().to_string();
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

    fn list_panes(pid: u32) -> Result<Vec<WeztermMuxPane>> {
        let stdout = Self::cli_stdout(pid, &["list", "--format", "json"])?;
        let panes: Vec<WeztermMuxPane> =
            serde_json::from_str(&stdout).context("failed to parse wezterm pane list json")?;
        Ok(panes)
    }

    fn list_clients(pid: u32) -> Result<Vec<WeztermMuxClient>> {
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

        let clients: Vec<WeztermMuxClient> =
            serde_json::from_str(&stdout).context("failed to parse wezterm client list json")?;
        Ok(clients)
    }

    fn select_client_focused_pane<F>(
        clients: &[WeztermMuxClient],
        pid: u32,
        mut accept: F,
    ) -> Option<WeztermMuxFocusSelection>
    where
        F: FnMut(u64) -> bool,
    {
        clients
            .iter()
            .find_map(|client| {
                (client.pid == pid && client.focused_pane_id > 0 && accept(client.focused_pane_id))
                    .then_some(WeztermMuxFocusSelection::MatchingPid(
                        client.focused_pane_id,
                    ))
            })
            .or_else(|| {
                clients.iter().find_map(|client| {
                    (client.focused_pane_id > 0 && accept(client.focused_pane_id))
                        .then_some(WeztermMuxFocusSelection::AnyClient(client.focused_pane_id))
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
                WeztermMuxFocusSelection::MatchingPid(_) => logging::debug(format!(
                    "wezterm: pid={} focused pane from matching client = {}",
                    pid, pane_id
                )),
                WeztermMuxFocusSelection::AnyClient(_) => logging::debug(format!(
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
            let mut candidates: Vec<&WeztermMuxPane> = panes
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
                WeztermMuxFocusSelection::MatchingPid(_) => "matching client focused pane",
                WeztermMuxFocusSelection::AnyClient(_) => "any client focused pane",
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
}

impl TerminalMultiplexerProvider for WeztermMux {
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

    fn focused_pane_for_pid(&self, pid: u32) -> Result<u64> {
        Self::focused_pane_id(pid)
    }

    fn pane_neighbor_for_pid(&self, pid: u32, pane_id: u64, dir: Direction) -> Result<u64> {
        Self::pane_in_direction(pid, pane_id, dir)?
            .context("no terminal multiplexer pane exists in requested direction")
    }

    fn send_text_to_pane(&self, pid: u32, pane_id: u64, text: &str) -> Result<()> {
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

    fn mux_attach_args(&self, _target: String) -> Option<Vec<String>> {
        None
    }

    fn merge_source_pane_into_focused_target(
        &self,
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

        // Determine whether the mux bridge path should be used.
        let use_bridge = {
            let mux_policy = crate::config::mux_policy_for(WEZTERM_HOST_ALIASES);
            mux_policy.integration_enabled
                && mux_policy.bridge_enable_override() != Some(false)
                && mux_policy.backend == TerminalMuxBackend::Wezterm
        };

        if use_bridge && target_window_id.is_none() {
            logging::debug(format!(
                "wezterm: mux bridge enabled; enqueue merge source pane {} dir={}",
                source_pane_id, dir
            ));
            // Enqueue merge command via filesystem bridge.
            let dir_name = dir.to_string();
            let command = format!("merge {source_pane_id} {dir_name}\n");
            let runtime_dir =
                std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
            let bridge_dir = PathBuf::from(runtime_dir).join("niri-deep-wezterm-mux");
            fs::create_dir_all(&bridge_dir).with_context(|| {
                format!("failed to create mux bridge dir: {}", bridge_dir.display())
            })?;
            let final_path = bridge_dir.join("merge.cmd");
            let temp_path = bridge_dir.join(format!(
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
            return Ok(());
        }
        if use_bridge && target_window_id.is_some() {
            logging::debug(
                "wezterm: skipping mux bridge because explicit merge target is available",
            );
        }
        logging::debug("wezterm: mux bridge unavailable; using direct cli merge path");

        let target_pane_id = if let Some(window_id) = target_window_id {
            Self::merge_target_pane_id(target_pid, source_pane_id, Some(window_id))?
        } else {
            // Poll for focus transition away from source pane.
            const POLL_ATTEMPTS: usize = 3;
            const POLL_DELAY: Duration = Duration::from_millis(10);
            let mut transitioned_pane = None;
            for attempt in 0..POLL_ATTEMPTS {
                let panes = Self::list_panes(target_pid)?;
                let clients = Self::list_clients(target_pid)?;
                let pane_exists = |pane_id: u64| panes.iter().any(|p| p.pane_id == pane_id);
                if let Some(selection) =
                    Self::select_client_focused_pane(&clients, target_pid, pane_exists)
                {
                    let pane_id = selection.pane_id();
                    if pane_id != source_pane_id {
                        transitioned_pane = Some(pane_id);
                        break;
                    }
                }
                if attempt + 1 < POLL_ATTEMPTS {
                    std::thread::sleep(POLL_DELAY);
                }
            }
            if let Some(pane_id) = transitioned_pane {
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

    fn active_foreground_process(&self, pid: u32) -> Option<String> {
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

impl TopologyHandler for WeztermMux {
    fn can_focus(&self, dir: Direction, pid: u32) -> Result<bool> {
        let pane_id = Self::focused_pane_id(pid)?;
        Ok(Self::pane_in_direction(pid, pane_id, dir)?.is_some())
    }

    fn move_decision(&self, dir: Direction, pid: u32) -> Result<MoveDecision> {
        let pane_id = Self::focused_pane_id(pid)?;
        let panes = Self::list_panes(pid)?;
        let active_tab_id = panes
            .iter()
            .find(|p| p.pane_id == pane_id)
            .map(|p| p.tab_id)
            .context("active pane is not present in wezterm pane list")?;
        let pane_count = panes.iter().filter(|p| p.tab_id == active_tab_id).count() as u32;
        if pane_count <= 1 {
            logging::debug(format!(
                "wezterm: move_decision dir={dir} pane_count={} => Passthrough",
                pane_count
            ));
            return Ok(MoveDecision::Passthrough);
        }

        if Self::pane_in_direction(pid, pane_id, dir)?.is_some() {
            logging::debug(format!("wezterm: move_decision dir={dir} => Internal"));
            return Ok(MoveDecision::Internal);
        }

        let has_perpendicular_neighbor = match dir {
            Direction::North | Direction::South => {
                Self::pane_in_direction(pid, pane_id, Direction::West)?.is_some()
                    || Self::pane_in_direction(pid, pane_id, Direction::East)?.is_some()
            }
            Direction::West | Direction::East => {
                Self::pane_in_direction(pid, pane_id, Direction::North)?.is_some()
                    || Self::pane_in_direction(pid, pane_id, Direction::South)?.is_some()
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
            };
        // Fallback: pick any other pane in the same tab.
        let target = match target {
            Some(t) => t,
            None => {
                let panes = Self::list_panes(pid)?;
                let active_tab_id = panes
                    .iter()
                    .find(|p| p.pane_id == pane_id)
                    .map(|p| p.tab_id)
                    .context("active pane is not present in wezterm pane list")?;
                let mut candidates: Vec<u64> = panes
                    .into_iter()
                    .filter(|p| p.tab_id == active_tab_id && p.pane_id != pane_id)
                    .map(|p| p.pane_id)
                    .collect();
                candidates.sort_unstable();
                candidates
                    .into_iter()
                    .next()
                    .context("no perpendicular wezterm pane found for rearrange")?
            }
        };

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
        let source_pane_id = Self::focused_pane_id(source_pid.get())?;
        Ok(MergePreparation::with_payload(WeztermMuxMergePreparation {
            pane_id: source_pane_id,
            target_window_id: None,
        }))
    }

    fn augment_merge_preparation_for_target(
        &self,
        preparation: MergePreparation,
        target_window_id: Option<u64>,
    ) -> MergePreparation {
        preparation.map_payload::<WeztermMuxMergePreparation>(|mut preparation| {
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
            .into_payload::<WeztermMuxMergePreparation>()
            .context("source wezterm merge missing pane id")?;
        let target_pid = target_pid.context("target wezterm merge missing pid")?;
        self.merge_source_pane_into_focused_target(
            source_pid.get(),
            preparation.pane_id,
            target_pid.get(),
            preparation.target_window_id,
            dir,
        )
        .context("wezterm merge failed")
    }
}
