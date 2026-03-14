//! Linux-specific process discovery using the /proc filesystem.

use std::collections::HashSet;
use std::process::Command;

use super::{is_shell_comm, normalize_process_name, process_tree_pids};

// ---------------------------------------------------------------------------
// Process enumeration
// ---------------------------------------------------------------------------

/// List all process IDs on the system by scanning /proc.
pub fn all_pids() -> Vec<u32> {
    let mut pids = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return pids;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Ok(pid) = name.to_string_lossy().parse::<u32>() else {
            continue;
        };
        pids.push(pid);
    }
    pids
}

/// Collect all direct child PIDs of all threads for a process.
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

// ---------------------------------------------------------------------------
// Process metadata
// ---------------------------------------------------------------------------

/// Get the command name (comm) for a process.
pub fn process_comm(pid: u32) -> Option<String> {
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|value| normalize_process_name(value.trim()))
}

/// Get an environment variable from a process.
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

/// Get the command line arguments for a process.
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

/// Get the target of a file descriptor symlink.
pub fn process_fd_target(pid: u32, fd: u32) -> Option<String> {
    std::fs::read_link(format!("/proc/{pid}/fd/{fd}"))
        .ok()
        .map(|target| target.to_string_lossy().to_string())
}

/// Check if a process uses a specific TTY.
pub fn process_uses_tty(pid: u32, tty_name: &str) -> bool {
    [0_u32, 1, 2]
        .into_iter()
        .filter_map(|fd| process_fd_target(pid, fd))
        .any(|target| target == tty_name)
}

// ---------------------------------------------------------------------------
// Socket discovery
// ---------------------------------------------------------------------------

/// Extract socket inode from /proc/pid/fd symlink target like "socket:[12345]".
pub fn socket_inode_from_fd_target(target: &str) -> Option<u64> {
    let value = target.trim().strip_prefix("socket:[")?.strip_suffix(']')?;
    value.parse::<u64>().ok().filter(|inode| *inode > 0)
}

/// Get all socket inodes for a process by scanning /proc/pid/fd.
pub fn socket_inodes_for_pid(pid: u32) -> HashSet<u64> {
    let mut inodes = HashSet::new();
    let Ok(entries) = std::fs::read_dir(format!("/proc/{pid}/fd")) else {
        return inodes;
    };
    for entry in entries.flatten() {
        let Ok(target) = std::fs::read_link(entry.path()) else {
            continue;
        };
        let target = target.to_string_lossy();
        let Some(inode) = socket_inode_from_fd_target(&target) else {
            continue;
        };
        inodes.insert(inode);
    }
    inodes
}

/// Parse /proc/net/unix output to find socket path by inode.
pub fn socket_path_from_proc_net_unix(
    raw: &str,
    socket_inodes: &HashSet<u64>,
    path_contains: &str,
) -> Option<String> {
    if socket_inodes.is_empty() {
        return None;
    }
    for line in raw.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let Some(inode) = fields.get(6).and_then(|value| value.parse::<u64>().ok()) else {
            continue;
        };
        if !socket_inodes.contains(&inode) {
            continue;
        }
        let Some(path) = fields.get(7) else {
            continue;
        };
        if !path.contains(path_contains) {
            continue;
        }
        return Some((*path).to_string());
    }
    None
}

/// Find socket path for a process using /proc/net/unix.
pub fn socket_path_for_pid_from_proc_net_unix(pid: u32, path_contains: &str) -> Option<String> {
    let socket_inodes = socket_inodes_for_pid(pid);
    let raw = std::fs::read_to_string("/proc/net/unix").ok()?;
    socket_path_from_proc_net_unix(&raw, &socket_inodes, path_contains)
}

/// Parse `ss -xnp` output to find socket path by inode.
pub fn socket_path_from_ss_output(
    raw: &str,
    socket_inodes: &HashSet<u64>,
    path_contains: &str,
) -> Option<String> {
    if socket_inodes.is_empty() {
        return None;
    }
    for line in raw.lines() {
        if !socket_inodes.iter().any(|inode| {
            line.contains(&format!(" {inode} "))
                || line.contains(&format!(" {inode} users:"))
                || line.ends_with(&format!(" {inode}"))
        }) {
            continue;
        }
        for token in line.split_whitespace() {
            if !token.starts_with('/') || !token.contains(path_contains) {
                continue;
            }
            return Some(token.to_string());
        }
    }
    None
}

/// Find socket path for a process using `ss -xnp` command.
pub fn socket_path_for_pid_from_ss(pid: u32, path_contains: &str) -> Option<String> {
    let socket_inodes = socket_inodes_for_pid(pid);
    if socket_inodes.is_empty() {
        return None;
    }
    let output = Command::new("ss").args(["-xnp"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    socket_path_from_ss_output(
        &String::from_utf8_lossy(&output.stdout),
        &socket_inodes,
        path_contains,
    )
}

/// Combined socket path discovery: try /proc/net/unix first, then ss.
pub fn socket_path_for_pid(pid: u32, path_contains: &str) -> Option<String> {
    socket_path_for_pid_from_proc_net_unix(pid, path_contains)
        .or_else(|| socket_path_for_pid_from_ss(pid, path_contains))
}

// ---------------------------------------------------------------------------
// Foreground process detection
// ---------------------------------------------------------------------------

/// Parse tpgid (terminal foreground process group ID) from /proc/pid/stat.
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

/// Parse pgrp (process group ID) from /proc/pid/stat.
pub fn parse_stat_pgrp(stat: &str) -> Option<u32> {
    let after_comm = stat.rsplit_once(')')?.1;
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    fields
        .get(2)?
        .parse::<i32>()
        .ok()
        .filter(|value| *value > 0)
        .map(|value| value as u32)
}

/// Find the foreground process name for a TTY within a process tree.
pub fn foreground_process_name_for_tty_in_tree(root_pid: u32, tty_name: &str) -> Option<String> {
    if root_pid == 0 || tty_name.trim().is_empty() {
        return None;
    }

    let candidates: Vec<(u32, String, Option<u32>, Option<u32>)> = process_tree_pids(root_pid)
        .into_iter()
        .filter(|pid| process_uses_tty(*pid, tty_name))
        .filter_map(|pid| {
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok();
            let comm = process_comm(pid)?;
            Some((
                pid,
                comm,
                stat.as_deref().and_then(parse_stat_pgrp),
                stat.as_deref().and_then(parse_stat_tpgid),
            ))
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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[cfg(unix)]
    use std::os::unix::net::{UnixListener, UnixStream};

    #[test]
    fn all_pids_includes_current_process() {
        assert!(all_pids().into_iter().any(|pid| pid == std::process::id()));
    }

    #[test]
    fn parse_stat_pgrp_reads_process_group() {
        let stat = "1234 (zsh) S 1 4321 4321 34817 4321 4194560 28954 275 0 0 32 5 0 0 20 0 1 0 123456 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0";
        assert_eq!(parse_stat_pgrp(stat), Some(4321));
        assert_eq!(parse_stat_tpgid(stat), Some(4321));
    }

    #[test]
    fn fd_target_helpers_handle_current_process() {
        let pid = std::process::id();
        let _ = process_fd_target(pid, 0);
        let _ = process_uses_tty(pid, "/dev/does-not-exist");
        let _ = foreground_process_name_for_tty_in_tree(pid, "/dev/does-not-exist");
    }

    #[test]
    fn extracts_socket_inode_from_fd_target() {
        assert_eq!(socket_inode_from_fd_target("socket:[458030]"), Some(458030));
        assert_eq!(socket_inode_from_fd_target("/dev/pts/1"), None);
    }

    #[test]
    fn extracts_socket_path_from_proc_net_unix_socket_entries() {
        let mut inodes = HashSet::new();
        inodes.insert(458030);
        let raw = r#"
Num       RefCount Protocol Flags    Type St Inode Path
0000000000000000: 00000002 00000000 00010000 0001 01 458030 /run/user/1000/zellij/0.43.1/implacable-oboe
0000000000000000: 00000003 00000000 00000000 0001 03 458031 /tmp/other.sock
"#;
        assert_eq!(
            socket_path_from_proc_net_unix(raw, &inodes, "zellij"),
            Some("/run/user/1000/zellij/0.43.1/implacable-oboe".to_string())
        );
    }

    #[test]
    fn extracts_socket_path_from_ss_output_via_peer_inode() {
        let mut inodes = HashSet::new();
        inodes.insert(455551);
        let raw = r#"
u_str ESTAB 0 0 * 455551 * 458031 users:(("zellij",pid=134518,fd=6),("zellij",pid=134518,fd=5))
u_str ESTAB 0 0 /run/user/1000/zellij/0.43.1/implacable-oboe 458031 * 455551 users:(("zellij",pid=134525,fd=7),("zellij",pid=134525,fd=6))
"#;
        assert_eq!(
            socket_path_from_ss_output(raw, &inodes, "zellij"),
            Some("/run/user/1000/zellij/0.43.1/implacable-oboe".to_string())
        );
    }

    #[test]
    fn socket_path_for_pid_from_proc_net_unix_reads_proc_entries() {
        let base = std::env::temp_dir().join(format!(
            "yeet-and-yoink-runtime-socket-test-{}",
            std::process::id()
        ));
        let socket_dir = base.join("zellij");
        std::fs::create_dir_all(&socket_dir).expect("socket dir should be created");
        let socket_path = socket_dir.join("mock-session");
        let listener = UnixListener::bind(&socket_path).expect("unix listener should bind");
        let _stream = UnixStream::connect(&socket_path).expect("unix stream should connect");

        let discovered = socket_path_for_pid_from_proc_net_unix(std::process::id(), "zellij");
        drop(listener);
        let _ = std::fs::remove_file(&socket_path);
        let _ = std::fs::remove_dir_all(&base);

        assert_eq!(discovered, Some(socket_path.to_string_lossy().to_string()));
    }
}
