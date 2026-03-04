use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};

use crate::engine::contract::{
    AdapterCapabilities, AppAdapter, AppKind, MergeExecutionMode, MergePreparation, MoveDecision,
    TearResult, TerminalMultiplexerProvider, TopologyHandler,
};
use crate::engine::runtime::{self, CommandContext, ProcessId};
use crate::engine::topology::Direction;

pub struct Tmux {
    /// Tmux session name, used for attach/spawn operations.
    session: String,
    /// Tmux client pid that belongs to the hosting terminal window.
    client_pid: u32,
    /// Terminal launch prefix for composing spawn commands (e.g. `["wezterm", "-e"]`).
    /// Set by the terminal host that detected this tmux session.
    terminal_launch_prefix: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TmuxMuxProvider;

pub(crate) static TMUX_MUX_PROVIDER: TmuxMuxProvider = TmuxMuxProvider;

const DETACHED_SESSION_PREFIX: &str = "niri-deep-";

struct TmuxMuxMergePreparation {
    pane_id: u64,
    session_name: String,
}

#[derive(Debug, Clone, Copy)]
struct TmuxPaneGeom {
    pane_id: u64,
    left: i32,
    top: i32,
    width: i32,
    height: i32,
}

// ---------------------------------------------------------------------------
// Tmux — constructors, CLI primitive, pub(crate) queries
// ---------------------------------------------------------------------------

impl Tmux {
    /// Create a Tmux adapter from a known tmux client PID.
    /// `terminal_launch_prefix` is the command prefix for the hosting terminal
    /// (e.g. `["wezterm", "-e"]`).
    pub(crate) fn from_client_pid(
        client_pid: u32,
        terminal_launch_prefix: Vec<String>,
    ) -> Option<Tmux> {
        let output = runtime::run_command_output(
            "tmux",
            &["list-clients", "-F", "#{client_pid}:#{session_name}"],
            &CommandContext {
                adapter: "tmux",
                action: "list-clients",
                target: Some(client_pid.to_string()),
            },
        )
        .ok()?;
        if !output.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let target = client_pid.to_string();
        for line in stdout.lines() {
            if let Some((pid_str, session)) = line.split_once(':') {
                if pid_str == target {
                    return Some(Tmux {
                        session: session.to_string(),
                        client_pid,
                        terminal_launch_prefix: terminal_launch_prefix.clone(),
                    });
                }
            }
        }
        None
    }

    /// Resolve a Tmux session from a terminal PID (walks process tree to find
    /// tmux client). Used by `TmuxMuxProvider` when tmux is the mux backend
    /// under a terminal host.
    fn for_terminal_pid(terminal_pid: u32) -> Result<Tmux> {
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
            .find_map(|candidate_pid| Self::from_client_pid(candidate_pid, vec![]))
            .with_context(|| {
                format!(
                    "tmux mux backend selected but unable to map terminal pid {} to tmux client candidates={}",
                    terminal_pid, candidates_debug
                )
            })
    }

    fn focused_pane_id_for_client(&self) -> Result<u64> {
        let output = runtime::run_command_output(
            "tmux",
            &["list-clients", "-F", "#{client_pid}:#{pane_id}"],
            &CommandContext {
                adapter: "tmux",
                action: "list-clients",
                target: Some(self.client_pid.to_string()),
            },
        )
        .context("failed to query tmux clients for focused pane")?;
        if !output.status.success() {
            bail!(
                "tmux list-clients failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let target = self.client_pid.to_string();
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            if let Some((pid_str, pane_str)) = line.split_once(':') {
                if pid_str == target {
                    return pane_str
                        .trim()
                        .trim_start_matches('%')
                        .parse::<u64>()
                        .ok()
                        .context("failed to parse tmux focused pane id for client");
                }
            }
        }
        bail!("no tmux client match for pid {}", self.client_pid);
    }

    /// `tmux display-message -t <pane> -p <format>` targeted at a specific pane.
    fn query_pane(&self, pane_id: u64, format: &str) -> Result<String> {
        let pane_ref = format!("%{pane_id}");
        let output = runtime::run_command_output(
            "tmux",
            &["display-message", "-t", &pane_ref, "-p", format],
            &CommandContext {
                adapter: "tmux",
                action: "display-message",
                target: Some(pane_ref.clone()),
            },
        )
        .context("failed to run tmux")?;
        if !output.status.success() {
            bail!(
                "tmux display-message failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
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
            &CommandContext {
                adapter: "tmux",
                action: "list-panes",
                target: Some(window_ref.to_string()),
            },
        )
        .context("failed to list tmux panes for window")?;
        if !output.status.success() {
            bail!(
                "tmux list-panes failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let mut panes = Vec::new();
        for line in String::from_utf8_lossy(&output.stdout).lines() {
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
                .with_context(|| format!("invalid tmux pane height in list-panes output: {line}"))?;
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

    fn overlap_len(a_start: i32, a_len: i32, b_start: i32, b_len: i32) -> i32 {
        let a_end = a_start + a_len;
        let b_end = b_start + b_len;
        (a_end.min(b_end) - a_start.max(b_start)).max(0)
    }

    fn directional_neighbor_pane_id(&self, source_pane_id: u64, dir: Direction) -> Result<u64> {
        let window_ref = self.query_pane(source_pane_id, "#{window_id}")?;
        let panes = self.list_panes_for_window(&window_ref)?;
        let source = panes
            .iter()
            .find(|pane| pane.pane_id == source_pane_id)
            .with_context(|| format!("source tmux pane %{source_pane_id} missing from window"))?;

        let mut best: Option<(u64, i32, i32)> = None;
        for pane in panes.iter().copied().filter(|pane| pane.pane_id != source_pane_id) {
            let (distance, overlap) = match dir {
                Direction::West => {
                    if pane.left + pane.width <= source.left {
                        (
                            source.left - (pane.left + pane.width),
                            Self::overlap_len(source.top, source.height, pane.top, pane.height),
                        )
                    } else {
                        (i32::MAX, 0)
                    }
                }
                Direction::East => {
                    if pane.left >= source.left + source.width {
                        (
                            pane.left - (source.left + source.width),
                            Self::overlap_len(source.top, source.height, pane.top, pane.height),
                        )
                    } else {
                        (i32::MAX, 0)
                    }
                }
                Direction::North => {
                    if pane.top + pane.height <= source.top {
                        (
                            source.top - (pane.top + pane.height),
                            Self::overlap_len(source.left, source.width, pane.left, pane.width),
                        )
                    } else {
                        (i32::MAX, 0)
                    }
                }
                Direction::South => {
                    if pane.top >= source.top + source.height {
                        (
                            pane.top - (source.top + source.height),
                            Self::overlap_len(source.left, source.width, pane.left, pane.width),
                        )
                    } else {
                        (i32::MAX, 0)
                    }
                }
            };
            if overlap <= 0 || distance == i32::MAX {
                continue;
            }
            match best {
                Some((_, best_distance, best_overlap))
                    if best_distance < distance
                        || (best_distance == distance && best_overlap >= overlap) => {}
                _ => best = Some((pane.pane_id, distance, overlap)),
            }
        }

        best.map(|(pane_id, _, _)| pane_id).with_context(|| {
            format!("no tmux pane exists in requested direction {dir} from %{source_pane_id}")
        })
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
            &CommandContext {
                adapter: "tmux",
                action: "new-session",
                target: Some(detached_session.clone()),
            },
        )
        .context("tmux new-session failed for tear-out detached client")?;
        Ok(format!("{detached_session}:{window_index}"))
    }

    /// Check if the active pane in this session is running nvim/vim.
    /// Returns the nvim process PID if found.
    pub(crate) fn nvim_in_current_pane(&self) -> Option<u32> {
        let cmd = self.query_client_pane("#{pane_current_command}").ok()?;
        if cmd != "nvim" && cmd != "vim" {
            return None;
        }
        let pane_pid: u32 = self.query_client_pane("#{pane_pid}").ok()?.parse().ok()?;
        let nvim_pids = runtime::find_descendants_by_comm(pane_pid, "nvim");
        nvim_pids.first().copied()
    }
}

// ---------------------------------------------------------------------------
// TerminalMuxProvider — TmuxMuxProvider (tmux as mux backend under a
// terminal host like wezterm)
// ---------------------------------------------------------------------------

impl TerminalMultiplexerProvider for TmuxMuxProvider {
    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            probe: true,
            focus: true,
            move_internal: true,
            resize_internal: false,
            rearrange: false,
            tear_out: true,
            merge: true,
        }
    }

    fn focused_pane_for_pid(&self, pid: u32) -> Result<u64> {
        let tmux = Tmux::for_terminal_pid(pid)?;
        tmux.focused_pane_id_for_client()
    }

    fn pane_neighbor_for_pid(&self, pid: u32, pane_id: u64, dir: Direction) -> Result<u64> {
        let tmux = Tmux::for_terminal_pid(pid)?;
        tmux.directional_neighbor_pane_id(pane_id, dir)
    }

    fn send_text_to_pane(&self, pid: u32, pane_id: u64, text: &str) -> Result<()> {
        let tmux = Tmux::for_terminal_pid(pid)?;
        let pane_ref = format!("%{pane_id}");
        let has_trailing_newline = text.ends_with('\n');
        let lines: Vec<&str> = text.split('\n').collect();
        for (index, line) in lines.iter().enumerate() {
            if !line.is_empty() {
                runtime::run_command_status(
                    "tmux",
                    &["send-keys", "-t", &pane_ref, "-l", line],
                    &CommandContext {
                        adapter: "tmux",
                        action: "send-keys",
                        target: Some(tmux.session.clone()),
                    },
                )
                .with_context(|| format!("tmux send-keys literal failed for pane {pane_id}"))?;
            }
            let is_last = index + 1 == lines.len();
            if !is_last || has_trailing_newline {
                runtime::run_command_status(
                    "tmux",
                    &["send-keys", "-t", &pane_ref, "Enter"],
                    &CommandContext {
                        adapter: "tmux",
                        action: "send-keys",
                        target: Some(tmux.session.clone()),
                    },
                )
                .with_context(|| format!("tmux send-keys enter failed for pane {pane_id}"))?;
            }
        }
        Ok(())
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
        let tmux = Tmux::for_terminal_pid(target_pid)?;
        let target_pane_id = tmux.focused_pane_id_for_client()?;
        if target_pane_id == source_pane_id {
            bail!("source and target tmux panes are the same");
        }
        let source_ref = format!("%{source_pane_id}");
        let target_ref = format!("%{target_pane_id}");
        let target_side = dir.opposite();
        let split_axis = match target_side {
            Direction::West | Direction::East => "-h",
            Direction::North | Direction::South => "-v",
        };
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
            &CommandContext {
                adapter: "tmux",
                action: "join-pane",
                target: Some(tmux.session.clone()),
            },
        )
        .context("tmux join-pane failed for merge")
    }

    fn active_foreground_process(&self, pid: u32) -> Option<String> {
        let tmux = Tmux::for_terminal_pid(pid).ok()?;
        tmux.query_client_pane("#{pane_current_command}").ok()
    }
}

// ---------------------------------------------------------------------------
// TopologyHandler — TmuxMuxProvider delegates to TopologyHandler for Tmux
// ---------------------------------------------------------------------------

impl TopologyHandler for TmuxMuxProvider {
    fn can_focus(&self, dir: Direction, pid: u32) -> Result<bool> {
        Tmux::for_terminal_pid(pid)?.can_focus(dir, pid)
    }

    fn move_decision(&self, dir: Direction, pid: u32) -> Result<MoveDecision> {
        Tmux::for_terminal_pid(pid)?.move_decision(dir, pid)
    }

    fn can_resize(&self, _dir: Direction, _grow: bool, _pid: u32) -> Result<bool> {
        Ok(false)
    }

    fn focus(&self, dir: Direction, pid: u32) -> Result<()> {
        Tmux::for_terminal_pid(pid)?.focus(dir, pid)
    }

    fn move_internal(&self, dir: Direction, pid: u32) -> Result<()> {
        Tmux::for_terminal_pid(pid)?.move_internal(dir, pid)
    }

    fn move_out(&self, dir: Direction, pid: u32) -> Result<TearResult> {
        Tmux::for_terminal_pid(pid)?.move_out(dir, pid)
    }

    fn merge_execution_mode(&self) -> MergeExecutionMode {
        MergeExecutionMode::TargetFocused
    }

    fn prepare_merge(&self, source_pid: Option<ProcessId>) -> Result<MergePreparation> {
        let source_pid = source_pid.context("source tmux merge missing pid")?;
        let source_tmux = Tmux::for_terminal_pid(source_pid.get())?;
        let pane_id = source_tmux.focused_pane_id_for_client()?;
        let session_name = source_tmux.query_pane(pane_id, "#{session_name}")?;
        Ok(MergePreparation::with_payload(TmuxMuxMergePreparation {
            pane_id,
            session_name,
        }))
    }

    fn merge_into_target(
        &self,
        dir: Direction,
        _source_pid: Option<ProcessId>,
        target_pid: Option<ProcessId>,
        preparation: MergePreparation,
    ) -> Result<()> {
        let target_pid = target_pid.context("target tmux merge missing pid")?;
        let target_tmux = Tmux::for_terminal_pid(target_pid.get())?;
        let target_session_name = target_tmux.session.clone();
        let preparation = preparation
            .into_payload::<TmuxMuxMergePreparation>()
            .context("source tmux merge missing pane/session metadata")?;
        self.merge_source_pane_into_focused_target(
            target_pid.get(),
            preparation.pane_id,
            target_pid.get(),
            None,
            dir,
        )?;
        if preparation.session_name.starts_with(DETACHED_SESSION_PREFIX)
            && preparation.session_name != target_session_name
        {
            let detached_session = preparation.session_name.clone();
            runtime::run_command_status(
                "tmux",
                &["kill-session", "-t", &detached_session],
                &CommandContext {
                    adapter: "tmux",
                    action: "kill-session",
                    target: Some(detached_session.clone()),
                },
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
        AdapterCapabilities {
            probe: true,
            focus: true,
            move_internal: true,
            resize_internal: false,
            rearrange: false,
            tear_out: true,
            merge: false,
        }
    }
}

impl TopologyHandler for Tmux {
    fn can_focus(&self, dir: Direction, _pid: u32) -> Result<bool> {
        let format = match dir.positional() {
            "left" => "#{pane_at_left}",
            "right" => "#{pane_at_right}",
            "top" => "#{pane_at_top}",
            "bottom" => "#{pane_at_bottom}",
            _ => unreachable!("invalid positional direction"),
        };
        Ok(self.query_client_pane(format)? != "1")
    }

    fn move_decision(&self, dir: Direction, pid: u32) -> Result<MoveDecision> {
        let panes: u32 = self.query_client_pane("#{window_panes}")?.parse().unwrap_or(1);
        if panes <= 1 {
            return Ok(MoveDecision::Passthrough);
        }
        if self.can_focus(dir, pid)? {
            Ok(MoveDecision::Internal)
        } else {
            Ok(MoveDecision::TearOut)
        }
    }

    fn focus(&self, dir: Direction, _pid: u32) -> Result<()> {
        let flag = dir.tmux_flag();
        let pane_ref = format!("%{}", self.focused_pane_id_for_client()?);
        runtime::run_command_status(
            "tmux",
            &["select-pane", "-t", &pane_ref, flag],
            &CommandContext {
                adapter: "tmux",
                action: "select-pane",
                target: Some(pane_ref.clone()),
            },
        )
        .with_context(|| format!("tmux select-pane {flag} failed"))
    }

    fn move_internal(&self, dir: Direction, _pid: u32) -> Result<()> {
        let source_pane_id = self.focused_pane_id_for_client()?;
        let target_pane_id = self.directional_neighbor_pane_id(source_pane_id, dir)?;
        let source_ref = format!("%{source_pane_id}");
        let target_ref = format!("%{target_pane_id}");
        runtime::run_command_status(
            "tmux",
            &["swap-pane", "-s", &source_ref, "-t", &target_ref],
            &CommandContext {
                adapter: "tmux",
                action: "swap-pane",
                target: Some(format!("{source_ref}->{target_ref}")),
            },
        )
        .context("tmux swap-pane failed")?;
        // Keep focus on the moved pane so repeated directional moves continue from
        // that pane (and can tear out at edges instead of swapping back).
        runtime::run_command_status(
            "tmux",
            &["select-pane", "-t", &source_ref],
            &CommandContext {
                adapter: "tmux",
                action: "select-pane",
                target: Some(source_ref.clone()),
            },
        )
        .context("tmux select-pane after swap failed")
    }

    fn move_out(&self, _dir: Direction, _pid: u32) -> Result<TearResult> {
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
            &CommandContext {
                adapter: "tmux",
                action: "break-pane",
                target: Some(pane_ref.clone()),
            },
        )
        .context("failed to run tmux break-pane")?;
        if !output.status.success() {
            bail!(
                "tmux break-pane failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let target = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let attach_target = self.detach_target_for_new_client(&target)?;
        let mut spawn_args: Vec<String> =
            vec!["tmux".into(), "attach-session".into(), "-t".into(), attach_target];
        if !self.terminal_launch_prefix.is_empty() {
            let mut cmd = self.terminal_launch_prefix.clone();
            cmd.append(&mut spawn_args);
            spawn_args = cmd;
        }
        Ok(TearResult {
            spawn_command: Some(spawn_args),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Tmux;
    use crate::engine::contract::AppAdapter;

    #[test]
    fn declares_explicit_capability_contract() {
        let app = Tmux {
            session: "test".to_string(),
            client_pid: 1,
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
