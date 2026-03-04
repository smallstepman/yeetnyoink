use anyhow::{bail, Context, Result};

use crate::adapters::apps::AppAdapter;
use crate::engine::contract::{
    AdapterCapabilities, AppKind, MergeExecutionMode, MergePreparation, MoveDecision, TearResult,
    TerminalMuxProvider, TopologyHandler,
};
use crate::engine::runtime::{self, CommandContext, ProcessId};
use crate::engine::topology::Direction;

pub struct Tmux {
    /// Tmux session name, used as `-t` target for all commands.
    session: String,
    /// Terminal launch prefix for composing spawn commands (e.g. `["wezterm", "-e"]`).
    /// Set by the terminal host that detected this tmux session.
    terminal_launch_prefix: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TmuxMuxProvider;

pub(crate) static TMUX_MUX_PROVIDER: TmuxMuxProvider = TmuxMuxProvider;

struct TmuxMuxMergePreparation {
    pane_id: u64,
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
        Self::from_client_pid(client_pid, vec![]).with_context(|| {
            format!("tmux mux backend selected but unable to map client {client_pid} to session")
        })
    }

    /// `tmux display-message -t <session> -p <format>` — the single CLI
    /// primitive that all session queries are built on.
    fn query(&self, format: &str) -> Result<String> {
        let output = runtime::run_command_output(
            "tmux",
            &["display-message", "-t", &self.session, "-p", format],
            &CommandContext {
                adapter: "tmux",
                action: "display-message",
                target: Some(self.session.clone()),
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

    /// Check if the active pane in this session is running nvim/vim.
    /// Returns the nvim process PID if found.
    pub(crate) fn nvim_in_current_pane(&self) -> Option<u32> {
        let cmd = self.query("#{pane_current_command}").ok()?;
        if cmd != "nvim" && cmd != "vim" {
            return None;
        }
        let pane_pid: u32 = self.query("#{pane_pid}").ok()?.parse().ok()?;
        let nvim_pids = super::find_descendants_by_comm(pane_pid, "nvim");
        nvim_pids.first().copied()
    }
}

// ---------------------------------------------------------------------------
// TerminalMuxProvider — TmuxMuxProvider (tmux as mux backend under a
// terminal host like wezterm)
// ---------------------------------------------------------------------------

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
        let tmux = Tmux::for_terminal_pid(pid)?;
        let raw = tmux.query("#{pane_id}")?;
        raw.trim()
            .trim_start_matches('%')
            .parse::<u64>()
            .ok()
            .context("failed to parse tmux focused pane id")
    }

    fn pane_neighbor_for_pid(&self, pid: u32, pane_id: u64, dir: Direction) -> Result<u64> {
        let tmux = Tmux::for_terminal_pid(pid)?;
        let format = match dir {
            Direction::West => "#{pane_left}",
            Direction::East => "#{pane_right}",
            Direction::North => "#{pane_top}",
            Direction::South => "#{pane_bottom}",
        };
        let pane_ref = format!("%{pane_id}");
        let output = runtime::run_command_output(
            "tmux",
            &["display-message", "-p", "-t", &pane_ref, format],
            &CommandContext {
                adapter: "tmux",
                action: "display-message",
                target: Some(tmux.session.clone()),
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
        raw.trim()
            .trim_start_matches('%')
            .parse::<u64>()
            .ok()
            .context("no tmux pane exists in the requested direction")
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
        let target_pane_id: u64 = tmux
            .query("#{pane_id}")?
            .trim()
            .trim_start_matches('%')
            .parse::<u64>()
            .ok()
            .context("failed to parse tmux focused pane id for merge target")?;
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
        tmux.query("#{pane_current_command}").ok()
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
        Ok(self.query(format)? != "1")
    }

    fn move_decision(&self, dir: Direction, pid: u32) -> Result<MoveDecision> {
        let panes: u32 = self.query("#{window_panes}")?.parse().unwrap_or(1);
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
        runtime::run_command_status(
            "tmux",
            &["select-pane", "-t", &self.session, flag],
            &CommandContext {
                adapter: "tmux",
                action: "select-pane",
                target: Some(self.session.clone()),
            },
        )
        .with_context(|| format!("tmux select-pane {flag} failed"))
    }

    fn move_internal(&self, dir: Direction, _pid: u32) -> Result<()> {
        let flag = dir.tmux_flag();
        runtime::run_command_status(
            "tmux",
            &["swap-pane", "-t", &self.session, flag],
            &CommandContext {
                adapter: "tmux",
                action: "swap-pane",
                target: Some(self.session.clone()),
            },
        )
        .with_context(|| format!("tmux swap-pane {flag} failed"))
    }

    fn move_out(&self, _dir: Direction, _pid: u32) -> Result<TearResult> {
        let output = runtime::run_command_output(
            "tmux",
            &[
                "break-pane",
                "-t",
                &self.session,
                "-d",
                "-P",
                "-F",
                "#{session_name}:#{window_index}",
            ],
            &CommandContext {
                adapter: "tmux",
                action: "break-pane",
                target: Some(self.session.clone()),
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
        let mut spawn_args: Vec<String> =
            vec!["tmux".into(), "attach-session".into(), "-t".into(), target];
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
