use anyhow::{bail, Context, Result};

use crate::adapters::apps::terminal_mux::TerminalMuxProvider;
use crate::adapters::apps::AppAdapter;
use crate::engine::contract::{
    AdapterCapabilities, AppKind, MergeExecutionMode, MergePreparation, MoveDecision, TearResult,
    TopologyHandler,
};
use crate::engine::runtime::{self, CommandContext, ProcessId};
use crate::engine::topology::Direction;

pub struct Tmux {
    /// Tmux session name, used as `-t` target for all commands.
    session: String,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TmuxMuxProvider;

pub(crate) static TMUX_MUX_PROVIDER: TmuxMuxProvider = TmuxMuxProvider;

struct TmuxMuxMergePreparation {
    pane_id: u64,
}

fn parse_pane_id(raw: &str) -> Option<u64> {
    raw.trim().trim_start_matches('%').parse::<u64>().ok()
}

fn pane_target(pane_id: u64) -> String {
    format!("%{pane_id}")
}

fn pane_direction_flag(dir: Direction) -> &'static str {
    dir.tmux_flag()
}

fn tmux_query(session: &str, format: &str) -> Result<String> {
    let output = runtime::run_command_output(
        "tmux",
        &["display-message", "-t", session, "-p", format],
        &CommandContext {
            adapter: "tmux",
            action: "display-message",
            target: Some(session.to_string()),
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

fn current_pane_command_for_session(session: &str) -> Result<String> {
    tmux_query(session, "#{pane_current_command}")
}

fn focused_pane_id_for_session(session: &str) -> Result<u64> {
    let raw = tmux_query(session, "#{pane_id}")?;
    parse_pane_id(&raw).context("failed to parse tmux focused pane id")
}

fn pane_neighbor_for_session(session: &str, pane_id: u64, dir: Direction) -> Result<u64> {
    let format = match dir {
        Direction::West => "#{pane_left}",
        Direction::East => "#{pane_right}",
        Direction::North => "#{pane_top}",
        Direction::South => "#{pane_bottom}",
    };
    let pane_target = pane_target(pane_id);
    let output = runtime::run_command_output(
        "tmux",
        &["display-message", "-p", "-t", &pane_target, format],
        &CommandContext {
            adapter: "tmux",
            action: "display-message",
            target: Some(session.to_string()),
        },
    )
    .context("failed to query tmux pane neighbor")?;
    if !output.status.success() {
        bail!(
            "tmux neighbor query failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    parse_pane_id(&raw).context("no tmux pane exists in the requested direction")
}

fn send_text_to_pane_in_session(session: &str, pane_id: u64, text: &str) -> Result<()> {
    let pane_target = pane_target(pane_id);
    let has_trailing_newline = text.ends_with('\n');
    let lines: Vec<&str> = text.split('\n').collect();
    for (index, line) in lines.iter().enumerate() {
        if !line.is_empty() {
            runtime::run_command_status(
                "tmux",
                &["send-keys", "-t", &pane_target, "-l", line],
                &CommandContext {
                    adapter: "tmux",
                    action: "send-keys",
                    target: Some(session.to_string()),
                },
            )
            .with_context(|| format!("tmux send-keys literal failed for pane {pane_id}"))?;
        }
        let is_last = index + 1 == lines.len();
        if !is_last || has_trailing_newline {
            runtime::run_command_status(
                "tmux",
                &["send-keys", "-t", &pane_target, "Enter"],
                &CommandContext {
                    adapter: "tmux",
                    action: "send-keys",
                    target: Some(session.to_string()),
                },
            )
            .with_context(|| format!("tmux send-keys enter failed for pane {pane_id}"))?;
        }
    }
    Ok(())
}

fn merge_source_pane_into_focused_target_in_session(
    session: &str,
    source_pane_id: u64,
    dir: Direction,
) -> Result<()> {
    let target_pane_id = focused_pane_id_for_session(session)?;
    if target_pane_id == source_pane_id {
        bail!("source and target tmux panes are the same");
    }
    let source_target = pane_target(source_pane_id);
    let target_target = pane_target(target_pane_id);
    let target_side = dir.opposite();
    let split_axis = match target_side {
        Direction::West | Direction::East => "-h",
        Direction::North | Direction::South => "-v",
    };
    let mut args = vec![
        "join-pane".to_string(),
        "-s".to_string(),
        source_target,
        "-t".to_string(),
        target_target,
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
            target: Some(session.to_string()),
        },
    )
    .context("tmux join-pane failed for merge")
}

fn at_edge_in_session(session: &str, dir: Direction) -> Result<bool> {
    let format = match dir.positional() {
        "left" => "#{pane_at_left}",
        "right" => "#{pane_at_right}",
        "top" => "#{pane_at_top}",
        "bottom" => "#{pane_at_bottom}",
        _ => unreachable!("invalid positional direction"),
    };
    let val = tmux_query(session, format)?;
    Ok(val == "1")
}

fn pane_count_in_session(session: &str) -> Result<u32> {
    let val = tmux_query(session, "#{window_panes}")?;
    Ok(val.parse().unwrap_or(1))
}

fn tmux_attach_args(target: String) -> Vec<String> {
    vec![
        "tmux".into(),
        "attach-session".into(),
        "-t".into(),
        target,
    ]
}

fn tmux_spawn_command(target: String) -> Vec<String> {
    let mut command = vec!["wezterm".into(), "-e".into()];
    command.extend(tmux_attach_args(target));
    command
}

fn focus_session_in_direction(session: &str, dir: Direction) -> Result<()> {
    let flag = pane_direction_flag(dir);
    runtime::run_command_status(
        "tmux",
        &["select-pane", "-t", session, flag],
        &CommandContext {
            adapter: "tmux",
            action: "select-pane",
            target: Some(session.to_string()),
        },
    )
    .with_context(|| format!("tmux select-pane {flag} failed"))
}

fn swap_session_in_direction(session: &str, dir: Direction) -> Result<()> {
    let flag = pane_direction_flag(dir);
    runtime::run_command_status(
        "tmux",
        &["swap-pane", "-t", session, flag],
        &CommandContext {
            adapter: "tmux",
            action: "swap-pane",
            target: Some(session.to_string()),
        },
    )
    .with_context(|| format!("tmux swap-pane {flag} failed"))
}

fn move_out_of_session(session: &str) -> Result<TearResult> {
    let output = runtime::run_command_output(
        "tmux",
        &[
            "break-pane",
            "-t",
            session,
            "-d",
            "-P",
            "-F",
            "#{session_name}:#{window_index}",
        ],
        &CommandContext {
            adapter: "tmux",
            action: "break-pane",
            target: Some(session.to_string()),
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
    Ok(TearResult {
        spawn_command: Some(tmux_spawn_command(target)),
    })
}

fn resolve_tmux_for_terminal_pid(terminal_pid: u32) -> Result<Tmux> {
    let mut tmux_candidates: Vec<u32> = runtime::find_descendants_by_comm(terminal_pid, "tmux");
    let shell_candidates: Vec<u32> = runtime::child_pids(terminal_pid)
        .into_iter()
        .filter(|&pid| runtime::is_shell_pid(pid))
        .collect();
    if let Some(shell_pid) = shell_candidates.first().copied() {
        let nested = runtime::find_descendants_by_comm(shell_pid, "tmux");
        if !nested.is_empty() {
            tmux_candidates = nested;
        }
    }
    let client_pid = tmux_candidates.first().copied().with_context(|| {
        format!("tmux mux backend selected but no tmux client found for pid {terminal_pid}")
    })?;
    tmux_from_client_pid(client_pid).with_context(|| {
        format!("tmux mux backend selected but unable to map client {client_pid} to session")
    })
}

/// Create a tmux adapter from a known tmux client PID.
pub(crate) fn tmux_from_client_pid(client_pid: u32) -> Option<Tmux> {
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
                });
            }
        }
    }
    None
}

/// Check if the active pane in this session is running nvim/vim.
/// Returns the nvim process PID if found.
pub(crate) fn tmux_nvim_in_current_pane(tmux: &Tmux) -> Option<u32> {
    let cmd = current_pane_command_for_session(&tmux.session).ok()?;
    if cmd != "nvim" && cmd != "vim" {
        return None;
    }
    let pane_pid: u32 = tmux_query(&tmux.session, "#{pane_pid}").ok()?.parse().ok()?;
    let nvim_pids = super::find_descendants_by_comm(pane_pid, "nvim");
    nvim_pids.first().copied()
}

impl TerminalMuxProvider for TmuxMuxProvider {
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
        let tmux = resolve_tmux_for_terminal_pid(pid)?;
        focused_pane_id_for_session(&tmux.session)
    }

    fn pane_neighbor_for_pid(&self, pid: u32, pane_id: u64, dir: Direction) -> Result<u64> {
        let tmux = resolve_tmux_for_terminal_pid(pid)?;
        pane_neighbor_for_session(&tmux.session, pane_id, dir)
    }

    fn send_text_to_pane(&self, pid: u32, pane_id: u64, text: &str) -> Result<()> {
        let tmux = resolve_tmux_for_terminal_pid(pid)?;
        send_text_to_pane_in_session(&tmux.session, pane_id, text)
    }

    fn mux_attach_args(&self, target: String) -> Option<Vec<String>> {
        Some(tmux_attach_args(target))
    }

    fn merge_source_pane_into_focused_target(
        &self,
        _source_pid: u32,
        source_pane_id: u64,
        target_pid: u32,
        _target_window_id: Option<u64>,
        dir: Direction,
    ) -> Result<()> {
        let tmux = resolve_tmux_for_terminal_pid(target_pid)?;
        merge_source_pane_into_focused_target_in_session(&tmux.session, source_pane_id, dir)
    }

    fn active_foreground_process(&self, pid: u32) -> Option<String> {
        resolve_tmux_for_terminal_pid(pid)
            .ok()
            .and_then(|tmux| current_pane_command_for_session(&tmux.session).ok())
    }
}

impl TopologyHandler for TmuxMuxProvider {
    fn can_focus(&self, dir: Direction, pid: u32) -> Result<bool> {
        let tmux = resolve_tmux_for_terminal_pid(pid)?;
        Ok(!at_edge_in_session(&tmux.session, dir)?)
    }

    fn move_decision(&self, dir: Direction, pid: u32) -> Result<MoveDecision> {
        let tmux = resolve_tmux_for_terminal_pid(pid)?;
        let panes = pane_count_in_session(&tmux.session)?;
        if panes <= 1 {
            return Ok(MoveDecision::Passthrough);
        }
        if at_edge_in_session(&tmux.session, dir)? {
            Ok(MoveDecision::TearOut)
        } else {
            Ok(MoveDecision::Internal)
        }
    }

    fn can_resize(&self, _dir: Direction, _grow: bool, _pid: u32) -> Result<bool> {
        Ok(false)
    }

    fn focus(&self, dir: Direction, pid: u32) -> Result<()> {
        let tmux = resolve_tmux_for_terminal_pid(pid)?;
        focus_session_in_direction(&tmux.session, dir)
    }

    fn move_internal(&self, dir: Direction, pid: u32) -> Result<()> {
        let tmux = resolve_tmux_for_terminal_pid(pid)?;
        swap_session_in_direction(&tmux.session, dir)
    }

    fn move_out(&self, _dir: Direction, pid: u32) -> Result<TearResult> {
        let tmux = resolve_tmux_for_terminal_pid(pid)?;
        move_out_of_session(&tmux.session)
    }

    fn merge_execution_mode(&self) -> MergeExecutionMode {
        MergeExecutionMode::TargetFocused
    }

    fn prepare_merge(&self, source_pid: Option<ProcessId>) -> Result<MergePreparation> {
        let source_pid = source_pid.context("source tmux merge missing pid")?;
        let pane_id = self.focused_pane_for_pid(source_pid.get())?;
        Ok(MergePreparation::with_payload(TmuxMuxMergePreparation {
            pane_id,
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
        let preparation = preparation
            .into_payload::<TmuxMuxMergePreparation>()
            .context("source tmux merge missing pane id")?;
        self.merge_source_pane_into_focused_target(
            target_pid.get(),
            preparation.pane_id,
            target_pid.get(),
            None,
            dir,
        )
    }
}

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
        Ok(!at_edge_in_session(&self.session, dir)?)
    }

    fn move_decision(&self, dir: Direction, _pid: u32) -> Result<MoveDecision> {
        let panes = pane_count_in_session(&self.session)?;
        if panes <= 1 {
            return Ok(MoveDecision::Passthrough);
        }
        if at_edge_in_session(&self.session, dir)? {
            Ok(MoveDecision::TearOut)
        } else {
            Ok(MoveDecision::Internal)
        }
    }

    fn focus(&self, dir: Direction, _pid: u32) -> Result<()> {
        focus_session_in_direction(&self.session, dir)
    }

    fn move_internal(&self, dir: Direction, _pid: u32) -> Result<()> {
        swap_session_in_direction(&self.session, dir)
    }

    fn move_out(&self, _dir: Direction, _pid: u32) -> Result<TearResult> {
        move_out_of_session(&self.session)
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
