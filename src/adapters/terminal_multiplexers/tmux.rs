use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};

use crate::engine::contracts::{
    AdapterCapabilities, AppAdapter, AppKind, MergeExecutionMode, MergePreparation, TearResult,
    TerminalMultiplexerProvider, TerminalPaneSnapshot, TopologyHandler,
};
use crate::engine::runtime::{self, CommandContext, ProcessId};
use crate::engine::topology::{
    select_closest_in_direction, DirectedRect, Direction, DirectionalNeighbors, Rect,
};

// ---------------------------------------------------------------------------
// Tmux session cache - avoids repeated `tmux list-clients` calls
// ---------------------------------------------------------------------------

/// Cache TTL for tmux session mappings (100ms matches process table cache)
const TMUX_CACHE_TTL: Duration = Duration::from_millis(100);

#[derive(Clone)]
struct TmuxClientInfo {
    session_name: String,
    pane_id: u64,
    window_id: String,
    pane_current_command: Option<String>,
}

struct TmuxClientCache {
    /// Maps client_pid → client info
    clients: HashMap<u32, TmuxClientInfo>,
    /// When this cache was populated
    fetched_at: Option<Instant>,
}

impl TmuxClientCache {
    fn new() -> Self {
        Self {
            clients: HashMap::new(),
            fetched_at: None,
        }
    }

    fn is_stale(&self) -> bool {
        self.fetched_at
            .map(|t| t.elapsed() > TMUX_CACHE_TTL)
            .unwrap_or(true)
    }

    fn refresh(&mut self) {
        self.clients.clear();
        // Fetch session_name, pane_id, window_id, and pane_current_command in a single call
        let output = match runtime::run_command_output(
            "tmux",
            &[
                "list-clients",
                "-F",
                "#{client_pid}:#{session_name}:#{pane_id}:#{window_id}:#{pane_current_command}",
            ],
            &CommandContext::new("tmux", "list-clients"),
        ) {
            Ok(o) if o.status.success() => o,
            _ => {
                self.fetched_at = Some(Instant::now());
                return;
            }
        };
        let stdout = runtime::stdout_text(&output);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.splitn(5, ':').collect();
            if parts.len() >= 4 {
                if let (Ok(pid), Ok(pane_id)) = (
                    parts[0].parse::<u32>(),
                    parts[2].trim().trim_start_matches('%').parse::<u64>(),
                ) {
                    let window_id = parts[3].trim().to_string();
                    let pane_current_command = parts
                        .get(4)
                        .map(|s| s.trim())
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string());
                    self.clients.insert(
                        pid,
                        TmuxClientInfo {
                            session_name: parts[1].to_string(),
                            pane_id,
                            window_id,
                            pane_current_command,
                        },
                    );
                }
            }
        }
        self.fetched_at = Some(Instant::now());
    }

    fn get_client_info(&mut self, client_pid: u32) -> Option<TmuxClientInfo> {
        if self.is_stale() {
            self.refresh();
        }
        self.clients.get(&client_pid).cloned()
    }
}

thread_local! {
    static TMUX_CACHE: RefCell<TmuxClientCache> = RefCell::new(TmuxClientCache::new());
}

fn cached_client_info(client_pid: u32) -> Option<TmuxClientInfo> {
    TMUX_CACHE.with(|cache| cache.borrow_mut().get_client_info(client_pid))
}

fn cached_foreground_command(client_pid: u32) -> Option<String> {
    cached_client_info(client_pid).and_then(|info| info.pane_current_command)
}

fn cached_window_id(client_pid: u32) -> Option<String> {
    cached_client_info(client_pid).map(|info| info.window_id)
}

pub struct Tmux {
    session: TmuxSession,
    /// Terminal launch prefix for composing spawn commands (e.g. `["wezterm", "-e"]`).
    /// Set by the terminal host that detected this tmux session.
    terminal_launch_prefix: Vec<String>,
}

#[derive(Debug, Clone)]
struct TmuxSession {
    /// Tmux session name, used for attach/spawn operations.
    name: String,
    /// Tmux client pid that belongs to the hosting terminal window.
    client_pid: u32,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TmuxMuxProvider;

pub(crate) static TMUX_MUX_PROVIDER: TmuxMuxProvider = TmuxMuxProvider;

const DETACHED_SESSION_PREFIX: &str = "yeet-and-yoink-";

#[derive(Debug, Clone, Copy)]
struct TmuxPaneGeom {
    pane_id: u64,
    left: i32,
    top: i32,
    width: i32,
    height: i32,
}

impl TmuxPaneGeom {
    fn directed_rect(self) -> DirectedRect<u64> {
        DirectedRect {
            id: self.pane_id,
            rect: Rect {
                x: self.left,
                y: self.top,
                w: self.width,
                h: self.height,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Tmux — constructors, CLI primitive, pub(crate) queries
// ---------------------------------------------------------------------------

impl Tmux {
    pub(crate) fn from_client_pid(
        client_pid: u32,
        terminal_launch_prefix: Vec<String>,
    ) -> Option<Tmux> {
        let session = TmuxSession::from_client_pid(client_pid)?;
        Some(Tmux {
            session,
            terminal_launch_prefix,
        })
    }

    pub(crate) fn nvim_in_current_pane(&self) -> Option<u32> {
        self.session.nvim_in_current_pane()
    }
}

impl TmuxSession {
    fn from_client_pid(client_pid: u32) -> Option<TmuxSession> {
        let info = cached_client_info(client_pid)?;
        Some(TmuxSession {
            name: info.session_name,
            client_pid,
        })
    }

    /// Resolve a Tmux session from a terminal PID (walks process tree to find
    /// tmux client). Used by `TmuxMuxProvider` when tmux is the mux backend
    /// under a terminal host.
    fn for_terminal_pid(terminal_pid: u32) -> Result<TmuxSession> {
        let mut tmux_candidates: Vec<u32> = Vec::new();
        if runtime::process_comm(terminal_pid).as_deref() == Some("tmux") {
            tmux_candidates.push(terminal_pid);
        }
        for pid in runtime::find_descendants_by_comm(terminal_pid, "tmux") {
            if !tmux_candidates.contains(&pid) {
                tmux_candidates.push(pid);
            }
        }
        let shell_candidates: Vec<u32> = runtime::child_pids(terminal_pid)
            .into_iter()
            .filter(|&pid| runtime::is_shell_pid(pid))
            .collect();
        for shell_pid in shell_candidates {
            for nested_pid in runtime::find_descendants_by_comm(shell_pid, "tmux") {
                if !tmux_candidates.contains(&nested_pid) {
                    tmux_candidates.push(nested_pid);
                }
            }
        }
        let candidates_debug = format!("{tmux_candidates:?}");
        tmux_candidates
            .into_iter()
            .find_map(Self::from_client_pid)
            .with_context(|| {
                format!(
                    "tmux mux backend selected but unable to map terminal pid {} to tmux client candidates={}",
                    terminal_pid, candidates_debug
                )
            })
    }

    fn focused_pane_id_for_client(&self) -> Result<u64> {
        // Use cached client info when available
        if let Some(info) = cached_client_info(self.client_pid) {
            return Ok(info.pane_id);
        }
        // Fallback to direct query if cache miss (shouldn't happen normally)
        bail!("no tmux client match for pid {}", self.client_pid);
    }

    fn cached_foreground_command(&self) -> Option<String> {
        cached_foreground_command(self.client_pid)
    }

    fn cached_window_id(&self) -> Option<String> {
        cached_window_id(self.client_pid)
    }

    /// `tmux display-message -t <pane> -p <format>` targeted at a specific pane.
    fn query_pane(&self, pane_id: u64, format: &str) -> Result<String> {
        let pane_ref = format!("%{pane_id}");
        let output = runtime::run_command_output(
            "tmux",
            &["display-message", "-t", &pane_ref, "-p", format],
            &CommandContext::new("tmux", "display-message").with_target(pane_ref.clone()),
        )
        .context("failed to run tmux")?;
        if !output.status.success() {
            bail!(
                "tmux display-message failed: {}",
                runtime::stderr_text(&output)
            );
        }
        Ok(runtime::stdout_text(&output))
    }

    fn query_client_pane(&self, format: &str) -> Result<String> {
        let pane_id = self.focused_pane_id_for_client()?;
        self.query_pane(pane_id, format)
    }

    fn list_panes_for_window(&self, window_ref: &str) -> Result<Vec<TmuxPaneGeom>> {
        let output = runtime::run_command_output(
            "tmux",
            &[
                "list-panes",
                "-t",
                window_ref,
                "-F",
                "#{pane_id}:#{pane_left}:#{pane_top}:#{pane_width}:#{pane_height}",
            ],
            &CommandContext::new("tmux", "list-panes").with_target(window_ref.to_string()),
        )
        .context("failed to list tmux panes for window")?;
        if !output.status.success() {
            bail!("tmux list-panes failed: {}", runtime::stderr_text(&output));
        }
        let mut panes = Vec::new();
        let stdout = runtime::stdout_text(&output);
        for line in stdout.lines() {
            let mut fields = line.split(':');
            let pane_id = fields
                .next()
                .and_then(|value| value.trim().trim_start_matches('%').parse::<u64>().ok())
                .with_context(|| format!("invalid tmux pane id in list-panes output: {line}"))?;
            let left = fields
                .next()
                .and_then(|value| value.parse::<i32>().ok())
                .with_context(|| format!("invalid tmux pane left in list-panes output: {line}"))?;
            let top = fields
                .next()
                .and_then(|value| value.parse::<i32>().ok())
                .with_context(|| format!("invalid tmux pane top in list-panes output: {line}"))?;
            let width = fields
                .next()
                .and_then(|value| value.parse::<i32>().ok())
                .with_context(|| format!("invalid tmux pane width in list-panes output: {line}"))?;
            let height = fields
                .next()
                .and_then(|value| value.parse::<i32>().ok())
                .with_context(|| {
                    format!("invalid tmux pane height in list-panes output: {line}")
                })?;
            panes.push(TmuxPaneGeom {
                pane_id,
                left,
                top,
                width,
                height,
            });
        }
        if panes.is_empty() {
            bail!("tmux list-panes returned no panes for window {window_ref}");
        }
        Ok(panes)
    }

    fn pane_in_direction(&self, source_pane_id: u64, dir: Direction) -> Result<Option<u64>> {
        let window_ref = self.query_pane(source_pane_id, "#{window_id}")?;
        let panes = self.list_panes_for_window(&window_ref)?;
        panes
            .iter()
            .find(|pane| pane.pane_id == source_pane_id)
            .with_context(|| format!("source tmux pane %{source_pane_id} missing from window"))?;
        let rects: Vec<_> = panes.into_iter().map(TmuxPaneGeom::directed_rect).collect();
        Ok(select_closest_in_direction(&rects, source_pane_id, dir))
    }

    fn directional_neighbor_pane_id(&self, source_pane_id: u64, dir: Direction) -> Result<u64> {
        self.pane_in_direction(source_pane_id, dir)?
            .with_context(|| {
                format!("no tmux pane exists in requested direction {dir} from %{source_pane_id}")
            })
    }

    fn directional_neighbors(&self) -> Result<DirectionalNeighbors> {
        Ok(DirectionalNeighbors {
            west: self.query_client_pane("#{pane_at_left}")? != "1",
            east: self.query_client_pane("#{pane_at_right}")? != "1",
            north: self.query_client_pane("#{pane_at_top}")? != "1",
            south: self.query_client_pane("#{pane_at_bottom}")? != "1",
        })
    }

    fn window_count(&self) -> Result<u32> {
        Ok(self
            .query_client_pane("#{window_panes}")?
            .parse()
            .unwrap_or(1))
    }

    fn focus(&self, dir: Direction) -> Result<()> {
        let flag = dir.tmux_flag();
        let pane_ref = format!("%{}", self.focused_pane_id_for_client()?);
        runtime::run_command_status(
            "tmux",
            &["select-pane", "-t", &pane_ref, flag],
            &CommandContext::new("tmux", "select-pane").with_target(pane_ref.clone()),
        )
        .with_context(|| format!("tmux select-pane {flag} failed"))
    }

    fn move_internal(&self, dir: Direction) -> Result<()> {
        let source_pane_id = self.focused_pane_id_for_client()?;
        let target_pane_id = self.directional_neighbor_pane_id(source_pane_id, dir)?;
        let source_ref = format!("%{source_pane_id}");
        let target_ref = format!("%{target_pane_id}");
        runtime::run_command_status(
            "tmux",
            &["swap-pane", "-s", &source_ref, "-t", &target_ref],
            &CommandContext::new("tmux", "swap-pane")
                .with_target(format!("{source_ref}->{target_ref}")),
        )
        .context("tmux swap-pane failed")?;
        // Keep focus on the moved pane so repeated directional moves continue from
        // that pane (and can tear out at edges instead of swapping back).
        runtime::run_command_status(
            "tmux",
            &["select-pane", "-t", &source_ref],
            &CommandContext::new("tmux", "select-pane").with_target(source_ref.clone()),
        )
        .context("tmux select-pane after swap failed")
    }

    fn detach_target_for_new_client(&self, break_target: &str) -> Result<String> {
        let (source_session, window_index) = break_target
            .split_once(':')
            .context("tmux break-pane output missing session:window target")?;
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_millis())
            .unwrap_or(0);
        let detached_session = format!("{DETACHED_SESSION_PREFIX}{}-{suffix}", self.client_pid);
        runtime::run_command_status(
            "tmux",
            &[
                "new-session",
                "-Ad",
                "-s",
                &detached_session,
                "-t",
                source_session,
            ],
            &CommandContext::new("tmux", "new-session").with_target(detached_session.clone()),
        )
        .context("tmux new-session failed for tear-out detached client")?;
        Ok(format!("{detached_session}:{window_index}"))
    }

    fn move_out(&self, terminal_launch_prefix: &[String]) -> Result<TearResult> {
        let pane_ref = format!("%{}", self.focused_pane_id_for_client()?);
        let output = runtime::run_command_output(
            "tmux",
            &[
                "break-pane",
                "-s",
                &pane_ref,
                "-d",
                "-P",
                "-F",
                "#{session_name}:#{window_index}",
            ],
            &CommandContext::new("tmux", "break-pane").with_target(pane_ref.clone()),
        )
        .context("failed to run tmux break-pane")?;
        if !output.status.success() {
            bail!("tmux break-pane failed: {}", runtime::stderr_text(&output));
        }
        let target = runtime::stdout_text(&output);
        let attach_target = self.detach_target_for_new_client(&target)?;
        let mut spawn_args: Vec<String> = vec![
            "tmux".into(),
            "attach-session".into(),
            "-t".into(),
            attach_target,
        ];
        if !terminal_launch_prefix.is_empty() {
            let mut cmd = terminal_launch_prefix.to_vec();
            cmd.append(&mut spawn_args);
            spawn_args = cmd;
        }
        Ok(TearResult {
            spawn_command: Some(spawn_args),
        })
    }

    /// Check if the active pane in this session is running nvim/vim.
    /// Returns the nvim process PID if found.
    fn nvim_in_current_pane(&self) -> Option<u32> {
        let cmd = self.query_client_pane("#{pane_current_command}").ok()?;
        if cmd != "nvim" && cmd != "vim" {
            return None;
        }
        let pane_pid: u32 = self.query_client_pane("#{pane_pid}").ok()?.parse().ok()?;
        let nvim_pids = runtime::find_descendants_by_comm(pane_pid, "nvim");
        nvim_pids.first().copied()
    }
}

impl TmuxMuxProvider {
    fn with_session<T>(
        &self,
        pid: u32,
        apply: impl FnOnce(&TmuxSession) -> Result<T>,
    ) -> Result<T> {
        let session = TmuxSession::for_terminal_pid(pid)?;
        apply(&session)
    }
}

// ---------------------------------------------------------------------------
// TerminalMuxProvider — TmuxMuxProvider (tmux as mux backend under a
// terminal host like wezterm)
// ---------------------------------------------------------------------------

impl TerminalMultiplexerProvider for TmuxMuxProvider {
    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities::terminal_mux_defaults()
    }

    fn focused_pane_for_pid(&self, pid: u32) -> Result<u64> {
        self.with_session(pid, |session| session.focused_pane_id_for_client())
    }

    fn list_panes_for_pid(&self, pid: u32) -> Result<Vec<TerminalPaneSnapshot>> {
        self.with_session(pid, |session| {
            let focused_pane_id = session.focused_pane_id_for_client()?;
            // Use cached window_id to avoid extra tmux display-message call
            let window_ref = session.cached_window_id().unwrap_or_else(|| {
                session
                    .query_pane(focused_pane_id, "#{window_id}")
                    .unwrap_or_default()
            });
            let panes = session.list_panes_for_window(&window_ref)?;
            Ok(panes
                .into_iter()
                .map(|pane| TerminalPaneSnapshot {
                    pane_id: pane.pane_id,
                    tab_id: None,
                    window_id: None,
                    is_active: pane.pane_id == focused_pane_id,
                    foreground_process_name: None,
                    tty_name: None,
                })
                .collect())
        })
    }

    fn pane_in_direction_for_pid(
        &self,
        pid: u32,
        pane_id: u64,
        dir: Direction,
    ) -> Result<Option<u64>> {
        self.with_session(pid, |session| session.pane_in_direction(pane_id, dir))
    }

    fn send_text_to_pane(&self, pid: u32, pane_id: u64, text: &str) -> Result<()> {
        self.with_session(pid, |session| {
            let pane_ref = format!("%{pane_id}");
            let has_trailing_newline = text.ends_with('\n');
            let lines: Vec<&str> = text.split('\n').collect();
            for (index, line) in lines.iter().enumerate() {
                if !line.is_empty() {
                    runtime::run_command_status(
                        "tmux",
                        &["send-keys", "-t", &pane_ref, "-l", line],
                        &CommandContext::new("tmux", "send-keys").with_target(session.name.clone()),
                    )
                    .with_context(|| format!("tmux send-keys literal failed for pane {pane_id}"))?;
                }
                let is_last = index + 1 == lines.len();
                if !is_last || has_trailing_newline {
                    runtime::run_command_status(
                        "tmux",
                        &["send-keys", "-t", &pane_ref, "Enter"],
                        &CommandContext::new("tmux", "send-keys").with_target(session.name.clone()),
                    )
                    .with_context(|| format!("tmux send-keys enter failed for pane {pane_id}"))?;
                }
            }
            Ok(())
        })
    }

    fn mux_attach_args(&self, target: String) -> Option<Vec<String>> {
        Some(vec![
            "tmux".into(),
            "attach-session".into(),
            "-t".into(),
            target,
        ])
    }

    fn merge_source_pane_into_focused_target(
        &self,
        _source_pid: u32,
        source_pane_id: u64,
        target_pid: u32,
        _target_window_id: Option<u64>,
        dir: Direction,
    ) -> Result<()> {
        self.with_session(target_pid, |session| {
            let target_pane_id = session.focused_pane_id_for_client()?;
            if target_pane_id == source_pane_id {
                bail!("source and target tmux panes are the same");
            }
            let source_ref = format!("%{source_pane_id}");
            let target_ref = format!("%{target_pane_id}");
            let target_side = dir.opposite();
            let split_axis = target_side.axis().select("-h", "-v");
            let mut args = vec![
                "join-pane".to_string(),
                "-s".to_string(),
                source_ref,
                "-t".to_string(),
                target_ref,
                split_axis.to_string(),
            ];
            if matches!(target_side, Direction::West | Direction::North) {
                args.push("-b".to_string());
            }
            let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
            runtime::run_command_status(
                "tmux",
                &arg_refs,
                &CommandContext::new("tmux", "join-pane").with_target(session.name.clone()),
            )
            .context("tmux join-pane failed for merge")
        })
    }

    fn active_foreground_process(&self, pid: u32) -> Option<String> {
        // Try cached foreground command first (avoids tmux display-message call)
        self.with_session(pid, |session| Ok(session.cached_foreground_command()))
            .ok()
            .flatten()
    }
}

// ---------------------------------------------------------------------------
// TopologyHandler — TmuxMuxProvider delegates to TopologyHandler for Tmux
// ---------------------------------------------------------------------------

impl TopologyHandler for TmuxMuxProvider {
    fn can_focus(&self, dir: Direction, pid: u32) -> Result<bool> {
        self.can_focus_from_directional_neighbors(dir, pid)
    }

    fn directional_neighbors(&self, pid: u32) -> Result<DirectionalNeighbors> {
        self.with_session(pid, |session| session.directional_neighbors())
    }

    fn supports_rearrange_decision(&self) -> bool {
        false
    }

    fn window_count(&self, pid: u32) -> Result<u32> {
        self.active_scope_pane_count_for_pid(pid)
    }

    fn focus(&self, dir: Direction, pid: u32) -> Result<()> {
        self.with_session(pid, |session| session.focus(dir))
    }

    fn move_internal(&self, dir: Direction, pid: u32) -> Result<()> {
        self.with_session(pid, |session| session.move_internal(dir))
    }

    fn move_out(&self, _dir: Direction, pid: u32) -> Result<TearResult> {
        self.with_session(pid, |session| session.move_out(&[]))
    }

    fn merge_execution_mode(&self) -> MergeExecutionMode {
        MergeExecutionMode::TargetFocused
    }

    fn prepare_merge(&self, source_pid: Option<ProcessId>) -> Result<MergePreparation> {
        self.prepare_source_pane_merge(source_pid, "source tmux merge missing pid", |source_pid| {
            let (pane_id, session_name) = self.with_session(source_pid, |session| {
                let pane_id = session.focused_pane_id_for_client()?;
                let session_name = session.query_pane(pane_id, "#{session_name}")?;
                Ok((pane_id, session_name))
            })?;
            Ok((pane_id, session_name))
        })
    }

    fn merge_into_target(
        &self,
        dir: Direction,
        source_pid: Option<ProcessId>,
        target_pid: Option<ProcessId>,
        preparation: MergePreparation,
    ) -> Result<()> {
        let (_, target_pid, preparation) = self.resolve_source_pane_merge::<String>(
            source_pid,
            target_pid,
            preparation,
            "source tmux merge missing pid",
            "target tmux merge missing pid",
            "source tmux merge missing pane/session metadata",
        )?;
        let target_session_name =
            self.with_session(target_pid, |session| Ok(session.name.clone()))?;
        self.merge_source_pane_into_focused_target(
            target_pid,
            preparation.pane_id,
            target_pid,
            None,
            dir,
        )?;
        if preparation.meta.starts_with(DETACHED_SESSION_PREFIX)
            && preparation.meta != target_session_name
        {
            let detached_session = preparation.meta.clone();
            runtime::run_command_status(
                "tmux",
                &["kill-session", "-t", &detached_session],
                &CommandContext::new("tmux", "kill-session").with_target(detached_session.clone()),
            )
            .context("failed to cleanup detached tmux session after merge")?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// AppAdapter + TopologyHandler — Tmux as standalone app adapter
// ---------------------------------------------------------------------------

impl AppAdapter for Tmux {
    fn adapter_name(&self) -> &'static str {
        "tmux"
    }

    fn kind(&self) -> AppKind {
        AppKind::Terminal
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities::terminal_mux_defaults().with_merge(false)
    }
}

impl TopologyHandler for Tmux {
    fn directional_neighbors(&self, _pid: u32) -> Result<DirectionalNeighbors> {
        self.session.directional_neighbors()
    }

    fn supports_rearrange_decision(&self) -> bool {
        false
    }

    fn window_count(&self, _pid: u32) -> Result<u32> {
        self.session.window_count()
    }

    fn can_focus(&self, dir: Direction, _pid: u32) -> Result<bool> {
        self.can_focus_from_directional_neighbors(dir, 0)
    }

    fn focus(&self, dir: Direction, _pid: u32) -> Result<()> {
        self.session.focus(dir)
    }

    fn move_internal(&self, dir: Direction, _pid: u32) -> Result<()> {
        self.session.move_internal(dir)
    }

    fn move_out(&self, _dir: Direction, _pid: u32) -> Result<TearResult> {
        self.session.move_out(&self.terminal_launch_prefix)
    }
}

#[cfg(test)]
mod tests {
    use super::{Tmux, TmuxSession};
    use crate::engine::AppAdapter;

    #[test]
    fn declares_explicit_capability_contract() {
        let app = Tmux {
            session: TmuxSession {
                name: "test".to_string(),
                client_pid: 1,
            },
            terminal_launch_prefix: vec!["wezterm".into(), "-e".into()],
        };
        let caps = AppAdapter::capabilities(&app);
        assert!(caps.probe);
        assert!(caps.focus);
        assert!(caps.move_internal);
        assert!(!caps.resize_internal);
        assert!(caps.tear_out);
        assert!(!caps.rearrange);
        assert!(!caps.merge);
    }
}
