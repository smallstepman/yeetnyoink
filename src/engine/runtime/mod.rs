//! Platform-abstracted process discovery and runtime utilities.
//!
//! This module provides cross-platform functions for:
//! - Process tree walking (parent/child relationships)
//! - Process metadata (comm, environ, cmdline)
//! - Socket discovery (for IPC with editors/multiplexers)
//! - Foreground process detection (for terminal chain resolution)

use std::collections::HashSet;
use std::num::NonZeroU32;
use std::process::{Command, Output};

use anyhow::{anyhow, Context, Result};

// Platform-specific implementations
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

// Re-export platform-specific implementations
#[cfg(target_os = "linux")]
pub use linux::*;
#[cfg(target_os = "macos")]
pub use macos::*;

// ---------------------------------------------------------------------------
// Shared types (platform-independent)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProcessId(NonZeroU32);

impl ProcessId {
    pub fn new(value: u32) -> Option<Self> {
        NonZeroU32::new(value).map(Self)
    }

    pub fn get(self) -> u32 {
        self.0.get()
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProcessTree {
    pids: Vec<u32>,
}

impl ProcessTree {
    pub fn for_pid(pid: u32) -> Self {
        Self {
            pids: process_tree_pids(pid),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = u32> + '_ {
        self.pids.iter().copied()
    }

    pub fn env_var(&self, key: &str) -> Option<String> {
        self.find_map(|pid| process_environ_var(pid, key))
    }

    pub fn find_map<T>(&self, find: impl FnMut(u32) -> Option<T>) -> Option<T> {
        self.iter().find_map(find)
    }

    pub fn find_map_by_comm<T>(
        &self,
        name: &str,
        mut find: impl FnMut(u32) -> Option<T>,
    ) -> Option<T> {
        self.iter()
            .filter(|pid| process_comm(*pid).as_deref() == Some(name))
            .find_map(|pid| find(pid))
    }
}

// ---------------------------------------------------------------------------
// Shared command execution utilities (platform-independent)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct CommandContext {
    pub adapter: &'static str,
    pub action: &'static str,
    pub target: Option<String>,
}

impl CommandContext {
    pub fn new(adapter: &'static str, action: &'static str) -> Self {
        Self {
            adapter,
            action,
            target: None,
        }
    }

    pub fn with_target(mut self, target: impl Into<String>) -> Self {
        self.target = Some(target.into());
        self
    }
}

fn command_identity(program: &str, args: &[&str], context: &CommandContext) -> String {
    let rendered_args = if args.is_empty() {
        String::new()
    } else {
        format!(" {}", args.join(" "))
    };
    let target = context
        .target
        .as_deref()
        .map(|value| format!(" target={value}"))
        .unwrap_or_default();
    format!(
        "{}::{}{} => {}{}",
        context.adapter, context.action, target, program, rendered_args
    )
}

pub fn run_command_output(
    program: &str,
    args: &[&str],
    context: &CommandContext,
) -> Result<Output> {
    Command::new(program).args(args).output().with_context(|| {
        format!(
            "failed to execute {}",
            command_identity(program, args, context)
        )
    })
}

pub fn stdout_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

pub fn stderr_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_string()
}

pub fn run_command_status(program: &str, args: &[&str], context: &CommandContext) -> Result<()> {
    let output = run_command_output(program, args, context)?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = stderr_text(&output);
    let identity = command_identity(program, args, context);
    if stderr.is_empty() {
        Err(anyhow!("{} failed with status {}", identity, output.status))
    } else {
        Err(anyhow!("{} failed: {}", identity, stderr))
    }
}

// ---------------------------------------------------------------------------
// Shared utility functions (platform-independent)
// ---------------------------------------------------------------------------

pub fn normalize_process_name(comm: &str) -> String {
    let without_path = comm.rsplit('/').next().unwrap_or(comm).trim();
    without_path
        .split(':')
        .next()
        .unwrap_or(without_path)
        .trim()
        .to_string()
}

pub fn is_shell_comm(comm: &str) -> bool {
    matches!(
        normalize_process_name(comm).as_str(),
        "bash" | "fish" | "zsh" | "sh" | "dash" | "ksh" | "tcsh" | "csh" | "nu" | "xonsh"
    )
}

pub fn is_shell_pid(pid: u32) -> bool {
    process_comm(pid)
        .map(|comm| is_shell_comm(&comm))
        .unwrap_or(false)
}

pub fn descendant_pids(pid: u32) -> Vec<u32> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    let mut stack = child_pids(pid);
    while let Some(current) = stack.pop() {
        if !seen.insert(current) {
            continue;
        }
        result.push(current);
        stack.extend(child_pids(current));
    }
    result
}

pub fn process_tree_pids(pid: u32) -> Vec<u32> {
    if pid == 0 {
        return Vec::new();
    }
    let mut result = descendant_pids(pid);
    result.insert(0, pid);
    result.sort_unstable();
    result.dedup();
    result
}

pub fn find_descendants_by_comm(pid: u32, name: &str) -> Vec<u32> {
    descendant_pids(pid)
        .into_iter()
        .filter(|candidate| process_comm(*candidate).as_deref() == Some(name))
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::process::Output;

    use super::{
        process_cmdline_args, process_comm, process_environ_var, process_tree_pids, stderr_text,
        stdout_text, CommandContext, ProcessTree,
    };

    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;

    #[test]
    fn command_context_builder_sets_target() {
        let context = CommandContext::new("tmux", "list-panes").with_target("%1");
        assert_eq!(context.adapter, "tmux");
        assert_eq!(context.action, "list-panes");
        assert_eq!(context.target.as_deref(), Some("%1"));
    }

    #[test]
    fn process_tree_pids_rejects_zero_pid() {
        assert!(process_tree_pids(0).is_empty());
    }

    #[test]
    fn process_cmdline_args_reads_current_process() {
        let args = process_cmdline_args(std::process::id()).expect("current process has cmdline");
        assert!(!args.is_empty());
    }

    #[test]
    fn process_environ_var_reads_current_process() {
        if let Ok(path) = std::env::var("PATH") {
            let discovered = process_environ_var(std::process::id(), "PATH");
            // On macOS, `ps eww` may truncate long values, so check prefix
            if let Some(discovered) = discovered {
                assert!(
                    path.starts_with(&discovered) || discovered.starts_with(&path),
                    "PATH should match or be a prefix: discovered={discovered}, expected={path}"
                );
            }
        }
    }

    #[test]
    fn process_tree_helper_reads_env_and_finds_current_process() {
        let tree = ProcessTree::for_pid(std::process::id());
        assert!(tree.iter().any(|pid| pid == std::process::id()));
        if let Ok(path) = std::env::var("PATH") {
            // On macOS, `ps eww` may truncate long values, so check prefix
            if let Some(discovered) = tree.env_var("PATH") {
                assert!(
                    path.starts_with(&discovered) || discovered.starts_with(&path),
                    "PATH should match or be a prefix"
                );
            }
        }
        assert!(tree
            .find_map(|pid| (pid == std::process::id()).then_some(pid))
            .is_some());
        if let Some(comm) = process_comm(std::process::id()) {
            assert!(tree
                .find_map_by_comm(&comm, |pid| (pid == std::process::id()).then_some(pid))
                .is_some());
        }
    }

    #[cfg(unix)]
    #[test]
    fn output_text_helpers_trim_output() {
        let output = Output {
            status: std::process::ExitStatus::from_raw(0),
            stdout: b" hello \n".to_vec(),
            stderr: b" error \n".to_vec(),
        };
        assert_eq!(stdout_text(&output), "hello");
        assert_eq!(stderr_text(&output), "error");
    }
}
