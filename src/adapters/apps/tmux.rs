use anyhow::{bail, Context, Result};

use crate::adapters::apps::terminal_backend::TerminalBackend;
use crate::adapters::apps::{AdapterCapabilities, AppKind, DeepApp, MoveDecision, TearResult};
use crate::engine::runtime::{self, CommandContext};
use crate::engine::topology::Direction;

pub struct Tmux {
    /// Tmux session name, used as `-t` target for all commands.
    session: String,
}

impl Tmux {
    /// Create a Tmux handler from a known tmux client PID.
    pub fn for_client_pid(client_pid: u32) -> Option<Self> {
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
    pub fn nvim_in_current_pane(&self) -> Option<u32> {
        let cmd = self.current_pane_command().ok()?;
        if cmd != "nvim" && cmd != "vim" {
            return None;
        }
        let pane_pid: u32 = self.tmux_query("#{pane_pid}").ok()?.parse().ok()?;
        let nvim_pids = super::find_descendants_by_comm(pane_pid, "nvim");
        nvim_pids.first().copied()
    }

    /// Return the active pane command for this session (e.g. nvim, zsh).
    pub fn current_pane_command(&self) -> Result<String> {
        self.tmux_query("#{pane_current_command}")
    }

    fn tmux_query(&self, format: &str) -> Result<String> {
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

    fn at_edge(&self, dir: Direction) -> Result<bool> {
        let format = match dir {
            Direction::West => "#{pane_at_left}",
            Direction::East => "#{pane_at_right}",
            Direction::North => "#{pane_at_top}",
            Direction::South => "#{pane_at_bottom}",
        };
        let val = self.tmux_query(format)?;
        Ok(val == "1")
    }

    fn pane_count(&self) -> Result<u32> {
        let val = self.tmux_query("#{window_panes}")?;
        Ok(val.parse().unwrap_or(1))
    }

    fn pane_direction_flag(dir: Direction) -> &'static str {
        match dir {
            Direction::West => "-L",
            Direction::East => "-R",
            Direction::North => "-U",
            Direction::South => "-D",
        }
    }
}

impl DeepApp for Tmux {
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

    fn can_focus(&self, dir: Direction, _pid: u32) -> Result<bool> {
        Ok(!self.at_edge(dir)?)
    }

    fn focus(&self, dir: Direction, _pid: u32) -> Result<()> {
        let flag = Self::pane_direction_flag(dir);
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

    fn move_decision(&self, dir: Direction, _pid: u32) -> Result<MoveDecision> {
        let panes = self.pane_count()?;
        if panes <= 1 {
            return Ok(MoveDecision::Passthrough);
        }
        if self.at_edge(dir)? {
            Ok(MoveDecision::TearOut)
        } else {
            Ok(MoveDecision::Internal)
        }
    }

    fn move_internal(&self, dir: Direction, _pid: u32) -> Result<()> {
        let flag = Self::pane_direction_flag(dir);
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

        Ok(TearResult {
            spawn_command: Some(TerminalBackend::spawn_attach_command(target)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Tmux;
    use crate::adapters::apps::DeepApp;

    #[test]
    fn declares_explicit_capability_contract() {
        let app = Tmux {
            session: "test".to_string(),
        };
        let caps = DeepApp::capabilities(&app);
        assert!(caps.probe);
        assert!(caps.focus);
        assert!(caps.move_internal);
        assert!(!caps.resize_internal);
        assert!(caps.tear_out);
        assert!(!caps.rearrange);
        assert!(!caps.merge);
    }
}
