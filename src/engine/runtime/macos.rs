//! macOS-specific process discovery using system commands.
//!
//! On macOS, we don't have /proc, so we use:
//! - `ps` for process enumeration and metadata
//! - `pgrep` for child process discovery
//! - `lsof` for socket/file descriptor discovery
//!
//! To minimize shell spawning overhead, we cache the process table and query from memory.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::process::Command;
use std::time::{Duration, Instant};

use super::{is_shell_comm, normalize_process_name, process_tree_pids};

// ---------------------------------------------------------------------------
// Process table cache
// ---------------------------------------------------------------------------

/// Cached process information from a single `ps` call.
#[derive(Debug, Clone)]
struct ProcessInfo {
    ppid: u32,
    comm: String,
}

/// A cached snapshot of the process table.
struct ProcessTableCache {
    /// Map from pid to process info
    processes: HashMap<u32, ProcessInfo>,
    /// When the cache was last refreshed
    last_refresh: Instant,
}

impl ProcessTableCache {
    fn new() -> Self {
        Self {
            processes: HashMap::new(),
            last_refresh: Instant::now() - Duration::from_secs(1000), // Force initial refresh
        }
    }

    fn is_stale(&self) -> bool {
        // Cache is valid for 100ms - enough for a single command but fresh enough for accuracy
        self.last_refresh.elapsed() > Duration::from_millis(100)
    }

    fn refresh(&mut self) {
        let output = Command::new("ps")
            .args(["-A", "-o", "pid=,ppid=,comm="])
            .output()
            .ok();

        let Some(output) = output else {
            return;
        };

        if !output.status.success() {
            return;
        }

        self.processes.clear();

        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                if let (Ok(pid), Ok(ppid)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) {
                    // comm may contain spaces, so join remaining parts
                    let comm = parts[2..].join(" ");
                    self.processes.insert(
                        pid,
                        ProcessInfo {
                            ppid,
                            comm: normalize_process_name(&comm),
                        },
                    );
                }
            }
        }

        self.last_refresh = Instant::now();
    }

    fn ensure_fresh(&mut self) {
        if self.is_stale() {
            self.refresh();
        }
    }

    fn get(&self, pid: u32) -> Option<&ProcessInfo> {
        self.processes.get(&pid)
    }

    fn child_pids(&self, pid: u32) -> Vec<u32> {
        self.processes
            .iter()
            .filter_map(|(&child_pid, info)| {
                if info.ppid == pid {
                    Some(child_pid)
                } else {
                    None
                }
            })
            .collect()
    }

    fn all_pids(&self) -> Vec<u32> {
        self.processes.keys().copied().collect()
    }
}

thread_local! {
    static PROCESS_CACHE: RefCell<ProcessTableCache> = RefCell::new(ProcessTableCache::new());
}

/// Ensure the process cache is fresh and run a query against it.
fn with_cache<F, R>(f: F) -> R
where
    F: FnOnce(&ProcessTableCache) -> R,
{
    PROCESS_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache.ensure_fresh();
        f(&cache)
    })
}

// ---------------------------------------------------------------------------
// Process enumeration (cached)
// ---------------------------------------------------------------------------

/// List all process IDs on the system.
pub fn all_pids() -> Vec<u32> {
    with_cache(|cache| cache.all_pids())
}

/// Get direct child PIDs of a process.
pub fn child_pids(pid: u32) -> Vec<u32> {
    with_cache(|cache| cache.child_pids(pid))
}

// ---------------------------------------------------------------------------
// Process metadata (cached where possible)
// ---------------------------------------------------------------------------

/// Get the command name (comm) for a process.
pub fn process_comm(pid: u32) -> Option<String> {
    with_cache(|cache| cache.get(pid).map(|info| info.comm.clone()))
}

/// Get an environment variable from a process using `ps eww`.
///
/// Note: This requires the process to be owned by the current user or root.
pub fn process_environ_var(pid: u32, key: &str) -> Option<String> {
    let output = Command::new("ps")
        .args(["eww", "-p", &pid.to_string(), "-o", "command="])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let output_str = String::from_utf8_lossy(&output.stdout);
    let prefix = format!("{key}=");

    // Environment variables appear after the command, space-separated
    for token in output_str.split_whitespace() {
        if let Some(value) = token.strip_prefix(&prefix) {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }

    None
}

/// Get the command line arguments for a process using `ps`.
pub fn process_cmdline_args(pid: u32) -> Option<Vec<String>> {
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "args="])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let args_str = String::from_utf8_lossy(&output.stdout);
    let args_str = args_str.trim();
    if args_str.is_empty() {
        return None;
    }

    // Simple split by whitespace - this won't handle quoted arguments perfectly
    // but matches the behavior we need for most cases
    Some(args_str.split_whitespace().map(|s| s.to_string()).collect())
}

/// Get the target of a file descriptor using `lsof`.
pub fn process_fd_target(pid: u32, fd: u32) -> Option<String> {
    let output = Command::new("lsof")
        .args(["-p", &pid.to_string(), "-a", "-d", &fd.to_string(), "-Fn"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    // lsof -Fn output format: lines starting with 'n' contain the name
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some(path) = line.strip_prefix('n') {
            return Some(path.to_string());
        }
    }

    None
}

/// Check if a process uses a specific TTY.
pub fn process_uses_tty(pid: u32, tty_name: &str) -> bool {
    // Check stdin, stdout, stderr (fd 0, 1, 2)
    [0_u32, 1, 2]
        .into_iter()
        .filter_map(|fd| process_fd_target(pid, fd))
        .any(|target| target == tty_name)
}

// ---------------------------------------------------------------------------
// Socket discovery
// ---------------------------------------------------------------------------

/// Find socket path for a process using `lsof -U` (Unix domain sockets).
pub fn socket_path_for_pid(pid: u32, path_contains: &str) -> Option<String> {
    let output = Command::new("lsof")
        .args(["-p", &pid.to_string(), "-U", "-Fn"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    // lsof -Fn output format: lines starting with 'n' contain the socket path
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some(path) = line.strip_prefix('n') {
            if path.contains(path_contains) && path.starts_with('/') {
                return Some(path.to_string());
            }
        }
    }

    None
}

/// Compatibility function for Linux API - uses lsof on macOS.
pub fn socket_path_for_pid_from_proc_net_unix(pid: u32, path_contains: &str) -> Option<String> {
    socket_path_for_pid(pid, path_contains)
}

/// Compatibility function for Linux API - uses lsof on macOS.
pub fn socket_path_for_pid_from_ss(pid: u32, path_contains: &str) -> Option<String> {
    socket_path_for_pid(pid, path_contains)
}

// ---------------------------------------------------------------------------
// Foreground process detection
// ---------------------------------------------------------------------------

/// Get tpgid (terminal foreground process group ID) for a process.
fn get_process_tpgid(pid: u32) -> Option<u32> {
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "tpgid="])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<i32>()
        .ok()
        .filter(|v| *v > 0)
        .map(|v| v as u32)
}

/// Get pgrp (process group ID) for a process.
fn get_process_pgrp(pid: u32) -> Option<u32> {
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "pgid="])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<i32>()
        .ok()
        .filter(|v| *v > 0)
        .map(|v| v as u32)
}

/// Find the foreground process name for a TTY within a process tree.
pub fn foreground_process_name_for_tty_in_tree(root_pid: u32, tty_name: &str) -> Option<String> {
    let _span = tracing::debug_span!(
        "runtime.macos.foreground_process_name_for_tty",
        root_pid,
        tty = tty_name
    )
    .entered();
    if root_pid == 0 || tty_name.trim().is_empty() {
        return None;
    }

    let candidates: Vec<(u32, String, Option<u32>, Option<u32>)> = process_tree_pids(root_pid)
        .into_iter()
        .filter(|pid| process_uses_tty(*pid, tty_name))
        .filter_map(|pid| {
            let comm = process_comm(pid)?;
            let pgrp = get_process_pgrp(pid);
            let tpgid = get_process_tpgid(pid);
            Some((pid, comm, pgrp, tpgid))
        })
        .collect();

    let pick = |entries: &[(u32, String, Option<u32>, Option<u32>)]| {
        entries
            .iter()
            .rev()
            .find(|(_, comm, _, _)| !is_shell_comm(comm))
            .or_else(|| entries.iter().rev().next())
            .map(|(_, comm, _, _)| comm.clone())
    };

    let foreground_group: Vec<_> = candidates
        .iter()
        .filter(|(_, _, pgrp, tpgid)| pgrp.is_some() && pgrp == tpgid)
        .cloned()
        .collect();
    pick(&foreground_group).or_else(|| pick(&candidates))
}

// ---------------------------------------------------------------------------
// Linux compatibility stubs
// ---------------------------------------------------------------------------

// These functions exist on Linux but don't have direct macOS equivalents.
// We provide stub implementations that return empty/None for compatibility.

/// Socket inode extraction - not applicable on macOS, returns None.
pub fn socket_inode_from_fd_target(_target: &str) -> Option<u64> {
    // macOS doesn't use socket:[inode] format
    None
}

/// Socket inodes for pid - not applicable on macOS, returns empty set.
pub fn socket_inodes_for_pid(_pid: u32) -> HashSet<u64> {
    // macOS doesn't expose socket inodes the same way
    HashSet::new()
}

/// Parse /proc/net/unix - not available on macOS, returns None.
pub fn socket_path_from_proc_net_unix(
    _raw: &str,
    _socket_inodes: &HashSet<u64>,
    _path_contains: &str,
) -> Option<String> {
    None
}

/// Parse ss output - ss is not available on macOS, returns None.
pub fn socket_path_from_ss_output(
    _raw: &str,
    _socket_inodes: &HashSet<u64>,
    _path_contains: &str,
) -> Option<String> {
    None
}

/// Parse tpgid from /proc/pid/stat - not available on macOS.
/// Use get_process_tpgid() directly instead.
pub fn parse_stat_tpgid(_stat: &str) -> Option<u32> {
    None
}

/// Parse pgrp from /proc/pid/stat - not available on macOS.
/// Use get_process_pgrp() directly instead.
pub fn parse_stat_pgrp(_stat: &str) -> Option<u32> {
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_pids_includes_current_process() {
        let pids = all_pids();
        assert!(
            pids.into_iter().any(|pid| pid == std::process::id()),
            "current process should be in pid list"
        );
    }

    #[test]
    fn process_comm_returns_current_process_name() {
        let comm = process_comm(std::process::id());
        assert!(comm.is_some(), "should get comm for current process");
    }

    #[test]
    fn process_cmdline_args_returns_args() {
        let args = process_cmdline_args(std::process::id());
        assert!(args.is_some(), "should get args for current process");
        assert!(!args.unwrap().is_empty(), "args should not be empty");
    }

    #[test]
    fn child_pids_returns_empty_for_leaf_process() {
        // Test processes typically don't have children
        let children = child_pids(std::process::id());
        // Just verify it doesn't crash - may or may not have children
        let _ = children;
    }
}
