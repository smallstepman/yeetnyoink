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

pub fn run_command_status(program: &str, args: &[&str], context: &CommandContext) -> Result<()> {
    let output = run_command_output(program, args, context)?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
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
