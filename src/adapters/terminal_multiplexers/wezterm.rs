use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::engine::contracts::{
    AdapterCapabilities, MergeExecutionMode, MergePreparation, MoveDecision, SourcePaneMerge,
    TearResult, TerminalMultiplexerProvider, TerminalPaneSnapshot, TopologyHandler,
};
use crate::engine::runtime::{self, ProcessId};
use crate::engine::topology::{Direction, DirectionalNeighbors};
use crate::logging;

#[derive(Debug, Clone, Deserialize)]
struct WeztermMuxPane {
    #[serde(default)]
    window_id: u64,
    pane_id: u64,
    tab_id: u64,
    #[serde(default)]
    is_active: bool,
    #[serde(default)]
    foreground_process_name: String,
    #[serde(default)]
    tty_name: String,
}

#[derive(Debug, Clone, Deserialize)]
struct WeztermMuxClient {
    pid: u32,
    focused_pane_id: u64,
}

#[derive(Debug, Clone, Default)]
struct WeztermMuxQueryCache {
    panes: Option<Vec<WeztermMuxPane>>,
}

thread_local! {
    static WEZTERM_QUERY_CACHE: RefCell<HashMap<u32, WeztermMuxQueryCache>> =
        RefCell::new(HashMap::new());
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WeztermMuxFocusSelection {
    MatchingPid(u64),
    AnyClient(u64),
}

impl From<WeztermMuxFocusSelection> for u64 {
    fn from(value: WeztermMuxFocusSelection) -> Self {
        match value {
            WeztermMuxFocusSelection::MatchingPid(pane_id)
            | WeztermMuxFocusSelection::AnyClient(pane_id) => pane_id,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct WeztermMux;
pub(crate) static WEZTERM_MUX_PROVIDER: WeztermMux = WeztermMux;

#[derive(Debug, Clone, Copy)]
struct WeztermHostTab {
    tab_id: u64,
    active_pane_id: u64,
}

impl WeztermMux {
    fn socket_override_for_pid<F>(pid: u32, runtime_dir: Option<&str>, socket_exists: F) -> Option<String>
    where
        F: FnOnce(&std::path::Path) -> bool,
    {
        let runtime_dir = runtime_dir?;
        let sock_path = PathBuf::from(format!("{runtime_dir}/wezterm/gui-sock-{pid}"));
        socket_exists(&sock_path).then(|| sock_path.to_string_lossy().to_string())
    }

    fn raw_panes_for_pid(&self, pid: u32) -> Result<Vec<WeztermMuxPane>> {
        if let Some(panes) = WEZTERM_QUERY_CACHE.with(|cache| {
            cache
                .borrow()
                .get(&pid)
                .and_then(|entry| entry.panes.clone())
        }) {
            logging::debug(format!("wezterm: pid={} pane list cache hit", pid));
            return Ok(panes);
        }

        let panes: Vec<WeztermMuxPane> = self.cli_json_for_pid(
            pid,
            &["list", "--format", "json"],
            "failed to parse wezterm pane list json",
        )?;
        WEZTERM_QUERY_CACHE.with(|cache| {
            cache.borrow_mut().entry(pid).or_default().panes = Some(panes.clone());
        });
        Ok(panes)
    }

    fn normalized_process_name(name: &str) -> Option<String> {
        let normalized = runtime::normalize_process_name(name);
        (!normalized.is_empty()).then_some(normalized)
    }

    fn pane_foreground_process_name_with_tty_fallback<F>(
        pane: &WeztermMuxPane,
        tty_fallback: F,
    ) -> Option<String>
    where
        F: FnOnce(&str) -> Option<String>,
    {
        Self::normalized_process_name(&pane.foreground_process_name).or_else(|| {
            let tty_name = pane.tty_name.trim();
            (!tty_name.is_empty())
                .then(|| tty_fallback(tty_name))
                .flatten()
                .and_then(|name| Self::normalized_process_name(&name))
        })
    }

    fn list_clients(&self, pid: u32) -> Result<Vec<WeztermMuxClient>> {
        let output = match self.cli_output_for_pid(pid, &["list-clients", "--format", "json"]) {
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

        let stdout = runtime::stdout_text(&output);
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

    fn preserves_query_cache(args: &[&str]) -> bool {
        matches!(
            args,
            ["list", "--format", "json"]
                | ["list-clients", "--format", "json"]
                | ["get-pane-direction", ..]
        )
    }

    fn invalidate_query_cache(&self, pid: u32) {
        WEZTERM_QUERY_CACHE.with(|cache| {
            cache.borrow_mut().remove(&pid);
        });
    }

    #[cfg(test)]
    pub(crate) fn clear_query_cache_for_tests() {
        WEZTERM_QUERY_CACHE.with(|cache| cache.borrow_mut().clear());
    }

    fn select_client_focused_pane<F>(
        &self,
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

    fn merge_target_pane_id(
        &self,
        pid: u32,
        source_pane_id: u64,
        target_window_id: Option<u64>,
    ) -> Result<u64> {
        let panes = self.list_panes_for_pid(pid)?;
        let clients = self.list_clients(pid)?;
        let pane_exists = |pane_id: u64| panes.iter().any(|p| p.pane_id == pane_id);
        let not_source = |pane_id: u64| pane_id != source_pane_id;

        if let Some(target_window_id) = target_window_id {
            let candidates = panes.iter().filter(|pane| {
                pane.window_id == Some(target_window_id) && pane.pane_id != source_pane_id
            });
            if let Some(selected) = TerminalPaneSnapshot::active_or_first(candidates) {
                logging::debug(format!(
                    "wezterm: merge target from explicit window {} pane = {}",
                    target_window_id, selected.pane_id
                ));
                return Ok(selected.pane_id);
            }
        }

        if let Some(selection) = self.select_client_focused_pane(&clients, pid, |pane_id| {
            pane_exists(pane_id) && not_source(pane_id)
        }) {
            let pane_id: u64 = selection.into();
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
            .and_then(|p| p.window_id)
        {
            let different_window_candidates = TerminalPaneSnapshot::unique_ids(
                panes
                    .iter()
                    .filter(|p| p.window_id != Some(source_window_id)),
            );
            if different_window_candidates.len() == 1 {
                let pane_id = different_window_candidates[0];
                logging::debug(format!(
                    "wezterm: merge target from non-source window candidate = {}",
                    pane_id
                ));
                return Ok(pane_id);
            }

            let active_different_window_candidates = TerminalPaneSnapshot::unique_ids(
                panes
                    .iter()
                    .filter(|p| p.window_id != Some(source_window_id) && p.is_active),
            );
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

        let other_panes =
            TerminalPaneSnapshot::unique_ids(panes.iter().filter(|p| p.pane_id != source_pane_id));
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

    fn host_tabs_for_pid(&self, pid: u32) -> Result<(u64, Vec<WeztermHostTab>, usize)> {
        let focused_pane_id = self.focused_pane_for_pid(pid)?;
        let panes = self.raw_panes_for_pid(pid)?;
        let focused = panes
            .iter()
            .find(|pane| pane.pane_id == focused_pane_id)
            .context("focused pane is not present in wezterm pane list")?;

        let mut tabs = Vec::new();
        for pane in panes
            .iter()
            .filter(|pane| pane.window_id == focused.window_id)
        {
            if let Some(existing) = tabs
                .iter_mut()
                .find(|tab: &&mut WeztermHostTab| tab.tab_id == pane.tab_id)
            {
                if pane.is_active {
                    existing.active_pane_id = pane.pane_id;
                }
                continue;
            }
            tabs.push(WeztermHostTab {
                tab_id: pane.tab_id,
                active_pane_id: pane.pane_id,
            });
        }

        let current_idx = tabs
            .iter()
            .position(|tab| tab.tab_id == focused.tab_id)
            .context("focused wezterm tab is not present in pane list")?;
        Ok((focused_pane_id, tabs, current_idx))
    }

    fn adjacent_host_tab_for_pid(
        &self,
        pid: u32,
        dir: Direction,
    ) -> Result<Option<WeztermHostTab>> {
        let (_, tabs, current_idx) = self.host_tabs_for_pid(pid)?;
        let target_idx = match dir {
            Direction::West if current_idx > 0 => Some(current_idx - 1),
            Direction::East if current_idx + 1 < tabs.len() => Some(current_idx + 1),
            _ => None,
        };
        Ok(target_idx.map(|idx| tabs[idx]))
    }

    pub(crate) fn can_focus_host_tab(&self, pid: u32, dir: Direction) -> Result<bool> {
        Ok(self.adjacent_host_tab_for_pid(pid, dir)?.is_some())
    }

    pub(crate) fn focus_host_tab(&self, pid: u32, dir: Direction) -> Result<()> {
        let (pane_id, _, _) = self.host_tabs_for_pid(pid)?;
        self.adjacent_host_tab_for_pid(pid, dir)?
            .context("no adjacent wezterm host tab exists in requested direction")?;
        let pane_id_str = pane_id.to_string();
        let relative = match dir {
            Direction::West => "-1",
            Direction::East => "1",
            _ => bail!("wezterm host tabs only support west/east routing"),
        };
        self.cli_stdout_for_pid(
            pid,
            &[
                "activate-tab",
                "--tab-relative",
                relative,
                "--no-wrap",
                "--pane-id",
                &pane_id_str,
            ],
        )?;
        Ok(())
    }

    pub(crate) fn can_move_to_host_tab(&self, pid: u32, dir: Direction) -> Result<bool> {
        Ok(self.adjacent_host_tab_for_pid(pid, dir)?.is_some())
    }

    pub(crate) fn move_pane_to_host_tab(&self, pid: u32, dir: Direction) -> Result<()> {
        let source_pane_id = self.focused_pane_for_pid(pid)?;
        let target = self
            .adjacent_host_tab_for_pid(pid, dir)?
            .context("no adjacent wezterm host tab exists in requested direction")?;
        if source_pane_id == target.active_pane_id {
            bail!("source and target wezterm panes are the same");
        }
        let source_pane_id_str = source_pane_id.to_string();
        let target_pane_id_str = target.active_pane_id.to_string();
        let target_side = dir
            .opposite()
            .select("--left", "--right", "--top", "--bottom");
        self.cli_stdout_for_pid(
            pid,
            &[
                "split-pane",
                "--pane-id",
                &target_pane_id_str,
                target_side,
                "--move-pane-id",
                &source_pane_id_str,
            ],
        )?;
        self.cli_stdout_for_pid(pid, &["activate-pane", "--pane-id", &source_pane_id_str])?;
        Ok(())
    }
}

impl TerminalMultiplexerProvider for WeztermMux {
    fn cli_output_for_pid(&self, pid: u32, args: &[&str]) -> Result<std::process::Output> {
        if !Self::preserves_query_cache(args) {
            self.invalidate_query_cache(pid);
        }
        let socket_override = Self::socket_override_for_pid(
            pid,
            std::env::var("XDG_RUNTIME_DIR").ok().as_deref(),
            |path| path.exists(),
        );
        let mut command = Command::new("wezterm");
        if let Some(sock) = socket_override.as_deref() {
            logging::debug(format!(
                "wezterm: pid={} cli {:?} via WEZTERM_UNIX_SOCKET",
                pid, args
            ));
            command.env("WEZTERM_UNIX_SOCKET", sock);
        } else {
            logging::debug(format!(
                "wezterm: pid={} cli {:?} via wezterm auto-discovery",
                pid, args
            ));
        }
        let output = command
            .args(["cli"])
            .args(args)
            .output()
            .context("failed to run wezterm cli")?;
        logging::debug(format!(
            "wezterm: pid={} cli {:?} status={} stdout={:?} stderr={:?}",
            pid,
            args,
            output.status,
            runtime::stdout_text(&output),
            runtime::stderr_text(&output)
        ));
        Ok(output)
    }

    fn list_panes_for_pid(&self, pid: u32) -> Result<Vec<TerminalPaneSnapshot>> {
        let panes = self.raw_panes_for_pid(pid)?;
        Ok(panes
            .into_iter()
            .map(|pane| TerminalPaneSnapshot {
                pane_id: pane.pane_id,
                tab_id: Some(pane.tab_id),
                window_id: Some(pane.window_id),
                is_active: pane.is_active,
                foreground_process_name: Some(pane.foreground_process_name),
                tty_name: (!pane.tty_name.trim().is_empty()).then_some(pane.tty_name),
            })
            .collect())
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities::terminal_mux_defaults()
            .with_resize_internal(true)
            .with_rearrange(true)
    }

    fn focused_pane_for_pid(&self, pid: u32) -> Result<u64> {
        let clients = self.list_clients(pid)?;
        logging::debug(format!(
            "wezterm: pid={} focused-pane lookup clients={}",
            pid,
            clients.len()
        ));

        if let Some(selection) = self.select_client_focused_pane(&clients, pid, |_| true) {
            let pane_id: u64 = selection.into();
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

        let panes = self.list_panes_for_pid(pid)?;
        if let Ok(pane_id) =
            self.focused_pane_from_snapshots(&panes, "unable to determine focused wezterm pane")
        {
            logging::debug(format!(
                "wezterm: pid={} focused pane from pane-list fallback = {}",
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

    fn active_foreground_process(&self, pid: u32) -> Option<String> {
        let pane_id = self.focused_pane_for_pid(pid).ok()?;
        let panes = self.raw_panes_for_pid(pid).ok()?;
        panes
            .into_iter()
            .find(|pane| pane.pane_id == pane_id)
            .and_then(|pane| {
                Self::pane_foreground_process_name_with_tty_fallback(&pane, |tty_name| {
                    runtime::foreground_process_name_for_tty_in_tree(pid, tty_name)
                })
            })
    }

    fn pane_in_direction_for_pid(
        &self,
        pid: u32,
        pane_id: u64,
        dir: Direction,
    ) -> Result<Option<u64>> {
        let pane_id_str = pane_id.to_string();
        let output = self.cli_output_for_pid(
            pid,
            &[
                "get-pane-direction",
                dir.select("Left", "Right", "Up", "Down"),
                "--pane-id",
                &pane_id_str,
            ],
        )?;

        if !output.status.success() {
            return Ok(None);
        }

        let stdout = runtime::stdout_text(&output);
        if stdout.is_empty() {
            return Ok(None);
        }
        let id = stdout.parse::<u64>().with_context(|| {
            format!("invalid pane id from wezterm get-pane-direction: {stdout}")
        })?;
        Ok(Some(id))
    }

    fn send_text_to_pane(&self, pid: u32, pane_id: u64, text: &str) -> Result<()> {
        let pane_id_str = pane_id.to_string();
        self.cli_status_for_pid(
            pid,
            &["send-text", "--pane-id", &pane_id_str, "--no-paste", text],
        )?;
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
        logging::debug("wezterm: using direct cli merge path");

        let target_pane_id = if let Some(window_id) = target_window_id {
            self.merge_target_pane_id(target_pid, source_pane_id, Some(window_id))?
        } else {
            // Poll for focus transition away from source pane.
            const POLL_ATTEMPTS: usize = 3;
            const POLL_DELAY: Duration = Duration::from_millis(10);
            let mut transitioned_pane = None;
            for attempt in 0..POLL_ATTEMPTS {
                self.invalidate_query_cache(target_pid);
                let panes = self.list_panes_for_pid(target_pid)?;
                let clients = self.list_clients(target_pid)?;
                let pane_exists = |pane_id: u64| panes.iter().any(|p| p.pane_id == pane_id);
                if let Some(selection) =
                    self.select_client_focused_pane(&clients, target_pid, pane_exists)
                {
                    let pane_id: u64 = selection.into();
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
                self.merge_target_pane_id(target_pid, source_pane_id, None)?
            }
        };
        if target_pane_id == source_pane_id {
            bail!("source and target panes are the same");
        }

        let target_pane_id_str = target_pane_id.to_string();
        let source_pane_id_str = source_pane_id.to_string();
        let target_side = dir
            .opposite()
            .select("--left", "--right", "--top", "--bottom");
        logging::debug(format!(
            "wezterm: merge source pane {} into target pane {} side={}",
            source_pane_id, target_pane_id, target_side
        ));
        self.cli_stdout_for_pid(
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
}

impl TopologyHandler for WeztermMux {
    fn directional_neighbors(&self, pid: u32) -> Result<DirectionalNeighbors> {
        self.directional_neighbors_from_pane_lookup(pid)
    }

    fn window_count(&self, pid: u32) -> Result<u32> {
        self.active_scope_pane_count_for_pid(pid)
    }

    fn can_focus(&self, dir: Direction, pid: u32) -> Result<bool> {
        self.can_focus_from_pane_lookup(dir, pid)
    }

    fn move_decision(&self, dir: Direction, pid: u32) -> Result<MoveDecision> {
        let decision = self.move_decision_from_pane_lookup(dir, pid, true)?;
        logging::debug(format!(
            "wezterm: move_decision dir={dir} => {:?}",
            decision
        ));
        Ok(decision)
    }

    fn can_resize(&self, _dir: Direction, _grow: bool, _pid: u32) -> Result<bool> {
        Ok(true)
    }

    fn focus(&self, dir: Direction, pid: u32) -> Result<()> {
        let (_, pane_id_str) = self.focused_pane_arg_for_pid(pid)?;
        self.cli_stdout_for_pid(
            pid,
            &[
                "activate-pane-direction",
                dir.select("Left", "Right", "Up", "Down"),
                "--pane-id",
                &pane_id_str,
            ],
        )?;
        Ok(())
    }

    fn move_internal(&self, dir: Direction, pid: u32) -> Result<()> {
        let (pane_id, pane_id_str) = self.focused_pane_arg_for_pid(pid)?;
        let neighbor = self
            .pane_in_direction_for_pid(pid, pane_id, dir)?
            .context("no wezterm pane exists in the requested move direction")?;
        let neighbor_str = neighbor.to_string();
        self.cli_stdout_for_pid(
            pid,
            &[
                "split-pane",
                "--pane-id",
                &neighbor_str,
                dir.select("--left", "--right", "--top", "--bottom"),
                "--move-pane-id",
                &pane_id_str,
            ],
        )?;
        Ok(())
    }

    fn resize_internal(&self, dir: Direction, grow: bool, step: i32, pid: u32) -> Result<()> {
        let (_, pane_id_str) = self.focused_pane_arg_for_pid(pid)?;
        let amount = step.max(1).to_string();
        let direction = if grow { dir } else { dir.opposite() };
        self.cli_stdout_for_pid(
            pid,
            &[
                "adjust-pane-size",
                "--pane-id",
                &pane_id_str,
                "--amount",
                &amount,
                direction.select("Left", "Right", "Up", "Down"),
            ],
        )?;
        Ok(())
    }

    fn rearrange(&self, dir: Direction, pid: u32) -> Result<()> {
        let (pane_id, pane_id_str) = self.focused_pane_arg_for_pid(pid)?;
        let target = self.perpendicular_pane_for_pid(pid, pane_id, dir)?;
        // Fallback: pick any other pane in the same tab.
        let target = match target {
            Some(t) => t,
            None => {
                let mut candidates: Vec<u64> = self
                    .active_scope_panes_for_pid(pid)?
                    .into_iter()
                    .filter(|p| p.pane_id != pane_id)
                    .map(|p| p.pane_id)
                    .collect();
                candidates.sort_unstable();
                candidates
                    .into_iter()
                    .next()
                    .context("no perpendicular wezterm pane found for rearrange")?
            }
        };

        let target_str = target.to_string();
        self.cli_stdout_for_pid(
            pid,
            &[
                "split-pane",
                "--pane-id",
                &target_str,
                dir.select("--left", "--right", "--top", "--bottom"),
                "--move-pane-id",
                &pane_id_str,
            ],
        )?;
        Ok(())
    }

    fn move_out(&self, _dir: Direction, pid: u32) -> Result<TearResult> {
        let (_, pane_id_str) = self.focused_pane_arg_for_pid(pid)?;
        self.cli_stdout_for_pid(
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
        self.prepare_source_pane_merge(
            source_pid,
            "source wezterm merge missing pid",
            |source_pid| Ok((self.focused_pane_for_pid(source_pid)?, None::<u64>)),
        )
    }

    fn augment_merge_preparation_for_target(
        &self,
        preparation: MergePreparation,
        target_window_id: Option<u64>,
    ) -> MergePreparation {
        preparation.map_payload::<SourcePaneMerge<Option<u64>>>(|mut preparation| {
            preparation.meta = target_window_id;
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
        let (source_pid, target_pid, preparation) = self.resolve_source_pane_merge::<Option<u64>>(
            source_pid,
            target_pid,
            preparation,
            "source wezterm merge missing pid",
            "target wezterm merge missing pid",
            "source wezterm merge missing pane id",
        )?;
        self.merge_source_pane_into_focused_target(
            source_pid,
            preparation.pane_id,
            target_pid,
            preparation.meta,
            dir,
        )
        .context("wezterm merge failed")
    }
}

#[cfg(test)]
mod tests {
    use super::{WeztermMux, WeztermMuxPane};
    use std::path::Path;

    #[test]
    fn pane_foreground_process_uses_tty_fallback_when_snapshot_field_is_empty() {
        let pane = WeztermMuxPane {
            window_id: 1,
            pane_id: 42,
            tab_id: 7,
            is_active: true,
            foreground_process_name: String::new(),
            tty_name: "/dev/pts/104".to_string(),
        };

        let fg = WeztermMux::pane_foreground_process_name_with_tty_fallback(&pane, |tty_name| {
            assert_eq!(tty_name, "/dev/pts/104");
            Some("nvim".to_string())
        });

        assert_eq!(fg.as_deref(), Some("nvim"));
    }

    #[test]
    fn pane_foreground_process_prefers_snapshot_name_over_tty_fallback() {
        let pane = WeztermMuxPane {
            window_id: 1,
            pane_id: 42,
            tab_id: 7,
            is_active: true,
            foreground_process_name: "/usr/bin/tmux: client".to_string(),
            tty_name: "/dev/pts/104".to_string(),
        };

        let fg = WeztermMux::pane_foreground_process_name_with_tty_fallback(&pane, |_| {
            Some("nvim".to_string())
        });

        assert_eq!(fg.as_deref(), Some("tmux"));
    }

    #[test]
    fn socket_override_for_pid_returns_none_without_runtime_dir() {
        let sock = WeztermMux::socket_override_for_pid(3350, None, |_path| true);
        assert_eq!(sock, None);
    }

    #[test]
    fn socket_override_for_pid_returns_none_when_socket_path_is_missing() {
        let sock = WeztermMux::socket_override_for_pid(3350, Some("/tmp/runtime"), |_path| false);
        assert_eq!(sock, None);
    }

    #[test]
    fn socket_override_for_pid_uses_xdg_gui_socket_when_present() {
        let sock = WeztermMux::socket_override_for_pid(
            3350,
            Some("/tmp/runtime"),
            |path: &Path| path == Path::new("/tmp/runtime/wezterm/gui-sock-3350"),
        );

        assert_eq!(sock.as_deref(), Some("/tmp/runtime/wezterm/gui-sock-3350"));
    }
}
