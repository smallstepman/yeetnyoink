use std::collections::HashSet;
use std::num::NonZeroU32;
use std::process::{Command, Output};

use anyhow::{anyhow, Context, Result};

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

/// Collect all direct child pids of all threads for a process.
pub fn child_pids(pid: u32) -> Vec<u32> {
    let task_dir = format!("/proc/{pid}/task");
    std::fs::read_dir(&task_dir)
        .into_iter()
        .flatten()
        .flatten()
        .flat_map(|entry| {
            let tid = entry.file_name().to_string_lossy().parse::<u32>().ok();
            tid.map_or_else(Vec::new, |tid| {
                std::fs::read_to_string(format!("/proc/{pid}/task/{tid}/children"))
                    .map(|contents| {
                        contents
                            .split_whitespace()
                            .filter_map(|token| token.parse::<u32>().ok())
                            .collect::<Vec<u32>>()
                    })
                    .unwrap_or_default()
            })
        })
        .collect()
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

pub fn normalize_process_name(comm: &str) -> String {
    let without_path = comm.rsplit('/').next().unwrap_or(comm).trim();
    without_path
        .split(':')
        .next()
        .unwrap_or(without_path)
        .trim()
        .to_string()
}

pub fn process_comm(pid: u32) -> Option<String> {
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|value| normalize_process_name(value.trim()))
}

pub fn process_environ_var(pid: u32, key: &str) -> Option<String> {
    let environ = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
    let prefix = format!("{key}=");
    for chunk in environ.split(|byte| *byte == 0) {
        let entry = String::from_utf8_lossy(chunk);
        if let Some(value) = entry.strip_prefix(&prefix) {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

pub fn process_cmdline_args(pid: u32) -> Option<Vec<String>> {
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    Some(
        cmdline
            .split(|byte| *byte == 0)
            .filter(|segment| !segment.is_empty())
            .map(|segment| String::from_utf8_lossy(segment).to_string())
            .collect(),
    )
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

pub fn parse_stat_tpgid(stat: &str) -> Option<u32> {
    let after_comm = stat.rsplit_once(')')?.1;
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    fields
        .get(5)?
        .parse::<i32>()
        .ok()
        .filter(|value| *value > 0)
        .map(|value| value as u32)
}

#[cfg(test)]
mod tests {
    use std::process::Output;

    use super::{
        process_cmdline_args, process_environ_var, process_tree_pids, stderr_text, stdout_text,
        CommandContext,
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
            assert_eq!(process_environ_var(std::process::id(), "PATH"), Some(path));
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
