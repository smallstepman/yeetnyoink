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

use super::{is_shell_comm, normalize_process_name};

// ---------------------------------------------------------------------------
// Process table cache
// ---------------------------------------------------------------------------

/// Cached process information from a single `ps` call.
#[derive(Debug, Clone)]
struct ProcessInfo {
    ppid: u32,
    comm: String,
}

#[derive(Debug, Clone)]
struct TtyProcessInfo {
    pid: u32,
    pgrp: Option<u32>,
    tpgid: Option<u32>,
    comm: String,
}

#[derive(Debug, Clone)]
struct TtyProcessCacheEntry {
    processes: Vec<TtyProcessInfo>,
    last_refresh: Instant,
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

fn parse_positive_process_group(raw: &str) -> Option<u32> {
    raw.trim()
        .parse::<i32>()
        .ok()
        .filter(|value| *value > 0)
        .map(|value| value as u32)
}

fn normalize_tty_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "??" {
        return None;
    }
    Some(trimmed.strip_prefix("/dev/").unwrap_or(trimmed).to_string())
}

fn tty_processes(tty_name: &str) -> Vec<TtyProcessInfo> {
    let Some(tty_name) = normalize_tty_name(tty_name) else {
        return Vec::new();
    };
    if let Some(processes) = TTY_PROCESS_CACHE.with(|cache| {
        cache
            .borrow()
            .get(&tty_name)
            .filter(|entry| entry.last_refresh.elapsed() <= Duration::from_millis(100))
            .map(|entry| entry.processes.clone())
    }) {
        return processes;
    }
    let _span = tracing::debug_span!("runtime.macos.tty_processes", tty = %tty_name).entered();
    let output = Command::new("ps")
        .args(["-t", &tty_name, "-o", "pid=,ppid=,pgid=,tpgid=,comm="])
        .output()
        .ok();

    let Some(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    let processes = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 5 {
                return None;
            }
            Some(TtyProcessInfo {
                pid: parts[0].parse::<u32>().ok()?,
                pgrp: parse_positive_process_group(parts[2]),
                tpgid: parse_positive_process_group(parts[3]),
                comm: normalize_process_name(&parts[4..].join(" ")),
            })
        })
        .collect::<Vec<_>>();
    TTY_PROCESS_CACHE.with(|cache| {
        cache.borrow_mut().insert(
            tty_name,
            TtyProcessCacheEntry {
                processes: processes.clone(),
                last_refresh: Instant::now(),
            },
        );
    });
    processes
}

thread_local! {
    static PROCESS_CACHE: RefCell<ProcessTableCache> = RefCell::new(ProcessTableCache::new());
    static TTY_PROCESS_CACHE: RefCell<HashMap<String, TtyProcessCacheEntry>> =
        RefCell::new(HashMap::new());
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
    tty_processes(tty_name)
        .into_iter()
        .any(|process| process.pid == pid)
}

/// Find the most relevant shell process attached to a specific TTY.
pub fn shell_pid_for_tty_name(tty_name: &str) -> Option<u32> {
    tty_processes(tty_name)
        .into_iter()
        .rev()
        .find(|process| is_shell_comm(&process.comm))
        .map(|process| process.pid)
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

    let candidates: Vec<(u32, String, Option<u32>, Option<u32>)> = tty_processes(tty_name)
        .into_iter()
        .map(|process| (process.pid, process.comm, process.pgrp, process.tpgid))
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
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn reset_process_cache_for_tests() {
        PROCESS_CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            cache.processes.clear();
            cache.last_refresh = Instant::now() - Duration::from_secs(1000);
        });
        TTY_PROCESS_CACHE.with(|cache| cache.borrow_mut().clear());
    }

    struct PathGuard {
        original: Option<OsString>,
    }

    impl PathGuard {
        fn prepend(dir: &Path) -> Self {
            let original = std::env::var_os("PATH");
            let mut value = OsString::from(dir);
            if let Some(existing) = &original {
                value.push(":");
                value.push(existing);
            }
            std::env::set_var("PATH", &value);
            Self { original }
        }
    }

    impl Drop for PathGuard {
        fn drop(&mut self) {
            match self.original.take() {
                Some(value) => std::env::set_var("PATH", value),
                None => std::env::remove_var("PATH"),
            }
        }
    }

    struct FakeProcessTools {
        _guard: PathGuard,
        root: PathBuf,
        lsof_count_file: PathBuf,
    }

    impl FakeProcessTools {
        fn install() -> Self {
            let root = std::env::temp_dir().join(format!(
                "yny-macos-runtime-test-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ));
            fs::create_dir_all(&root).expect("failed to create fake tool dir");
            let lsof_count_file = root.join("lsof-count");
            fs::write(&lsof_count_file, "0\n").expect("failed to initialize lsof count");
            write_executable(
                &root.join("ps"),
                r#"#!/bin/sh
case "$*" in
  "-A -o pid=,ppid=,comm=")
    cat <<'EOF'
100 1 wezterm-gui
101 100 zsh
102 101 nvim
EOF
    ;;
  "-t ttys001 -o pid=,ppid=,pgid=,tpgid=,comm=")
    cat <<'EOF'
101 100 101 102 zsh
102 101 102 102 nvim
EOF
    ;;
  *)
    echo "unexpected ps args: $*" >&2
    exit 1
    ;;
esac
"#,
            );
            write_executable(
                &root.join("lsof"),
                &format!(
                    r#"#!/bin/sh
count="$(cat "{0}")"
echo $((count + 1)) > "{0}"
exit 1
"#,
                    lsof_count_file.display()
                ),
            );
            let guard = PathGuard::prepend(&root);
            Self {
                _guard: guard,
                root,
                lsof_count_file,
            }
        }

        fn lsof_invocations(&self) -> u32 {
            fs::read_to_string(&self.lsof_count_file)
                .expect("failed to read lsof count")
                .trim()
                .parse()
                .expect("invalid lsof count")
        }
    }

    impl Drop for FakeProcessTools {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    struct TtyOnlyProcessTools {
        _guard: PathGuard,
        root: PathBuf,
        lsof_count_file: PathBuf,
    }

    impl TtyOnlyProcessTools {
        fn install() -> Self {
            let root = std::env::temp_dir().join(format!(
                "yny-macos-runtime-tty-only-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ));
            fs::create_dir_all(&root).expect("failed to create fake tool dir");
            let lsof_count_file = root.join("lsof-count");
            fs::write(&lsof_count_file, "0\n").expect("failed to initialize lsof count");
            write_executable(
                &root.join("ps"),
                r#"#!/bin/sh
case "$*" in
  "-t ttys001 -o pid=,ppid=,pgid=,tpgid=,comm=")
    cat <<'EOF'
101 100 101 102 zsh
102 101 102 102 nvim
EOF
    ;;
  *)
    echo "unexpected ps args: $*" >&2
    exit 1
    ;;
esac
"#,
            );
            write_executable(
                &root.join("lsof"),
                &format!(
                    r#"#!/bin/sh
count="$(cat "{0}")"
echo $((count + 1)) > "{0}"
exit 1
"#,
                    lsof_count_file.display()
                ),
            );
            let guard = PathGuard::prepend(&root);
            Self {
                _guard: guard,
                root,
                lsof_count_file,
            }
        }

        fn lsof_invocations(&self) -> u32 {
            fs::read_to_string(&self.lsof_count_file)
                .expect("failed to read lsof count")
                .trim()
                .parse()
                .expect("invalid lsof count")
        }
    }

    impl Drop for TtyOnlyProcessTools {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn write_executable(path: &Path, contents: &str) {
        fs::write(path, contents).expect("failed to write fake executable");
        let mut permissions = fs::metadata(path)
            .expect("failed to stat fake executable")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("failed to chmod fake executable");
    }

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

    #[test]
    fn process_uses_tty_reads_tty_scoped_ps_without_lsof() {
        let _lock = env_lock().lock().expect("env lock poisoned");
        let tools = FakeProcessTools::install();
        reset_process_cache_for_tests();

        assert!(process_uses_tty(101, "ttys001"));
        assert!(process_uses_tty(102, "ttys001"));
        assert_eq!(tools.lsof_invocations(), 0);
    }

    #[test]
    fn shell_pid_for_tty_name_reads_tty_scoped_ps_without_lsof() {
        let _lock = env_lock().lock().expect("env lock poisoned");
        let tools = FakeProcessTools::install();
        reset_process_cache_for_tests();

        assert_eq!(shell_pid_for_tty_name("ttys001"), Some(101));
        assert_eq!(tools.lsof_invocations(), 0);
    }

    #[test]
    fn foreground_process_name_for_tty_in_tree_uses_tty_scoped_ps_without_lsof() {
        let _lock = env_lock().lock().expect("env lock poisoned");
        let tools = FakeProcessTools::install();
        reset_process_cache_for_tests();

        assert_eq!(
            foreground_process_name_for_tty_in_tree(100, "ttys001"),
            Some("nvim".to_string())
        );
        assert_eq!(tools.lsof_invocations(), 0);
    }

    #[test]
    fn foreground_process_name_for_tty_in_tree_does_not_need_process_tree_scan() {
        let _lock = env_lock().lock().expect("env lock poisoned");
        let tools = TtyOnlyProcessTools::install();
        reset_process_cache_for_tests();

        assert_eq!(
            foreground_process_name_for_tty_in_tree(100, "ttys001"),
            Some("nvim".to_string())
        );
        assert_eq!(tools.lsof_invocations(), 0);
    }
}
