use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use anyhow::{bail, Context, Result};

use crate::engine::contract::{
    AdapterCapabilities, MergeExecutionMode, MergePreparation, MoveDecision, TearResult,
    TerminalMultiplexerProvider, TerminalPaneSnapshot, TopologyHandler,
};
use crate::engine::runtime::{self, ProcessId};
use crate::engine::topology::{Direction, DirectionalNeighbors};
use crate::logging;

#[derive(Debug, Clone, Copy)]
struct ZellijLayoutSnapshot {
    pane_count: u32,
    focused_pane_id: u64,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ZellijMuxProvider;

pub(crate) static ZELLIJ_MUX_PROVIDER: ZellijMuxProvider = ZellijMuxProvider;
static SESSION_CACHE: OnceLock<Mutex<HashMap<u32, String>>> = OnceLock::new();
const SOURCE_PANE_ENV: &str = "YEET_AND_YOINK_ZELLIJ_SOURCE_PANE_ID";

impl ZellijMuxProvider {
    fn session_cache() -> &'static Mutex<HashMap<u32, String>> {
        SESSION_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
    }

    fn cached_session_name(pid: u32) -> Option<String> {
        let cache = Self::session_cache().lock().ok()?;
        cache.get(&pid).cloned()
    }

    fn store_session_name(pid: u32, session: &str) {
        if session.trim().is_empty() {
            return;
        }
        if let Ok(mut cache) = Self::session_cache().lock() {
            cache.insert(pid, session.trim().to_string());
        }
    }

    fn session_required_prompt(stderr: &str) -> bool {
        stderr
            .to_ascii_lowercase()
            .contains("please specify the session name")
    }

    fn session_from_server_path(path: &str) -> Option<String> {
        let name = Path::new(path).file_name()?.to_str()?.trim();
        if name.is_empty() {
            None
        } else {
            Some(name.to_string())
        }
    }

    fn session_name_from_cmdline(pid: u32) -> Option<String> {
        let args = runtime::process_cmdline_args(pid)?;
        for (index, arg) in args.iter().enumerate() {
            if arg == "--session" {
                if let Some(value) = args.get(index + 1) {
                    let value = value.trim();
                    if !value.is_empty() {
                        return Some(value.to_string());
                    }
                }
            }
            if let Some(value) = arg.strip_prefix("--session=") {
                let value = value.trim();
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
            if arg == "--server" {
                if let Some(value) = args.get(index + 1) {
                    if let Some(session) = Self::session_from_server_path(value) {
                        return Some(session);
                    }
                }
            }
            if let Some(value) = arg.strip_prefix("--server=") {
                if let Some(session) = Self::session_from_server_path(value) {
                    return Some(session);
                }
            }
            if arg == "attach" {
                if let Some(value) = args.get(index + 1) {
                    let value = value.trim();
                    if !value.is_empty() && !value.starts_with('-') {
                        return Some(value.to_string());
                    }
                }
            }
        }
        None
    }

    fn session_name_by_kitty_pid(kitty_pid: u32) -> Option<String> {
        let kitty_pid = kitty_pid.to_string();
        for zellij_pid in runtime::all_pids()
            .into_iter()
            .filter(|pid| runtime::process_comm(*pid).as_deref() == Some("zellij"))
        {
            let tree = runtime::ProcessTree::for_pid(zellij_pid);
            if let Some(session) = tree.find_map(|candidate| {
                let session = runtime::process_environ_var(candidate, "ZELLIJ_SESSION_NAME")?;
                let candidate_kitty_pid = runtime::process_environ_var(candidate, "KITTY_PID")?;
                (candidate_kitty_pid == kitty_pid).then_some(session)
            }) {
                return Some(session);
            }
        }
        None
    }

    fn session_name_for_pid(pid: u32) -> Option<String> {
        if pid == 0 {
            return None;
        }
        if let Some(session) = Self::cached_session_name(pid) {
            return Some(session);
        }

        let tree = runtime::ProcessTree::for_pid(pid);

        if let Some(session) = tree.env_var("ZELLIJ_SESSION_NAME") {
            Self::store_session_name(pid, &session);
            return Some(session);
        }

        if let Some(session) = tree.find_map_by_comm("zellij", Self::session_name_from_cmdline) {
            Self::store_session_name(pid, &session);
            return Some(session);
        }

        if let Some(session) = tree.find_map_by_comm("zellij", |pid| {
            runtime::socket_path_for_pid_from_proc_net_unix(pid, "zellij")
                .and_then(|path| Self::session_from_server_path(&path))
        }) {
            Self::store_session_name(pid, &session);
            return Some(session);
        }

        if let Some(session) = tree.find_map_by_comm("zellij", |pid| {
            runtime::socket_path_for_pid_from_ss(pid, "zellij")
                .and_then(|path| Self::session_from_server_path(&path))
        }) {
            Self::store_session_name(pid, &session);
            return Some(session);
        }

        if let Some(session) = Self::session_name_by_kitty_pid(pid) {
            Self::store_session_name(pid, &session);
            return Some(session);
        }

        None
    }

    fn parse_layout_snapshot(layout: &str) -> ZellijLayoutSnapshot {
        let mut pane_count = 0_u32;
        let mut focused_pane_id: Option<u64> = None;
        let mut depth = 0_i32;
        let mut in_focused_tab = false;
        let mut focused_tab_depth = 0_i32;

        for line in layout.lines() {
            let trimmed = line.trim();

            if !in_focused_tab
                && trimmed.starts_with("tab ")
                && trimmed.contains("focus=true")
                && trimmed.contains('{')
            {
                in_focused_tab = true;
                focused_tab_depth = depth;
            }

            if in_focused_tab
                && trimmed.starts_with("pane")
                && !trimmed.contains('{')
                && !trimmed.contains("plugin location=")
            {
                pane_count += 1;
                if focused_pane_id.is_none()
                    && (trimmed.contains("focus=true")
                        || trimmed.contains("focused=true")
                        || trimmed.contains("is_focused=true"))
                {
                    focused_pane_id = Some(pane_count as u64);
                }
            }

            let opens = trimmed.bytes().filter(|byte| *byte == b'{').count() as i32;
            let closes = trimmed.bytes().filter(|byte| *byte == b'}').count() as i32;
            depth += opens - closes;

            if in_focused_tab && depth <= focused_tab_depth {
                in_focused_tab = false;
            }
        }

        if pane_count == 0 {
            for line in layout.lines() {
                let trimmed = line.trim();
                if !trimmed.starts_with("pane")
                    || trimmed.contains('{')
                    || trimmed.contains("plugin location=")
                {
                    continue;
                }
                pane_count += 1;
                if focused_pane_id.is_none()
                    && (trimmed.contains("focus=true")
                        || trimmed.contains("focused=true")
                        || trimmed.contains("is_focused=true"))
                {
                    focused_pane_id = Some(pane_count as u64);
                }
            }
        }

        if pane_count == 0 {
            pane_count = 1;
        }
        ZellijLayoutSnapshot {
            pane_count,
            focused_pane_id: focused_pane_id.unwrap_or(1),
        }
    }

    fn no_mirror_config_path() -> Option<String> {
        let path = std::env::temp_dir().join("yeet-and-yoink-zellij-no-mirror.kdl");
        let contents = "mirror_session false\n";
        match std::fs::read_to_string(&path) {
            Ok(current) if current == contents => {}
            _ => {
                if let Err(err) = std::fs::write(&path, contents) {
                    logging::debug(format!(
                        "zellij: failed to write no-mirror config {} err={:#}",
                        path.display(),
                        err
                    ));
                    return None;
                }
            }
        }
        path.to_str().map(|value| value.to_string())
    }

    fn permissions_cache_path() -> Option<std::path::PathBuf> {
        if let Some(cache_home) = std::env::var_os("XDG_CACHE_HOME") {
            return Some(std::path::PathBuf::from(cache_home).join("zellij/permissions.kdl"));
        }
        std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .map(|home| home.join(".cache/zellij/permissions.kdl"))
    }

    fn permission_block_contains(raw: &str, node_name: &str, permission: &str) -> bool {
        for marker in [format!("\"{node_name}\" {{"), format!("{node_name} {{")] {
            let mut search_start = 0;
            while let Some(offset) = raw[search_start..].find(&marker) {
                let start = search_start + offset + marker.len();
                let tail = &raw[start..];
                let Some(end) = tail.find('}') else {
                    break;
                };
                let block = &tail[..end];
                if block.lines().any(|line| line.trim() == permission) {
                    return true;
                }
                search_start = start + end + 1;
            }
        }
        false
    }

    fn ensure_break_plugin_permissions(plugin_url: &str) -> Result<()> {
        let Some(cache_path) = Self::permissions_cache_path() else {
            return Ok(());
        };
        let permission_key = plugin_url
            .strip_prefix("file:")
            .unwrap_or(plugin_url)
            .trim();
        if permission_key.is_empty() {
            return Ok(());
        }
        let cache = std::fs::read_to_string(&cache_path).unwrap_or_default();
        let has_change =
            Self::permission_block_contains(&cache, permission_key, "ChangeApplicationState")
                || Self::permission_block_contains(
                    &cache,
                    &format!("file:{permission_key}"),
                    "ChangeApplicationState",
                );
        let has_read =
            Self::permission_block_contains(&cache, permission_key, "ReadApplicationState")
                || Self::permission_block_contains(
                    &cache,
                    &format!("file:{permission_key}"),
                    "ReadApplicationState",
                );
        let has_pipe_read = Self::permission_block_contains(&cache, permission_key, "ReadCliPipes")
            || Self::permission_block_contains(
                &cache,
                &format!("file:{permission_key}"),
                "ReadCliPipes",
            );
        if has_change && has_read && has_pipe_read {
            return Ok(());
        }

        if let Some(parent) = cache_path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create zellij permissions cache dir {}",
                    parent.display()
                )
            })?;
        }

        let mut updated = cache;
        if !updated.is_empty() && !updated.ends_with('\n') {
            updated.push('\n');
        }
        let escaped_key = permission_key.replace('\\', "\\\\").replace('"', "\\\"");
        updated.push_str(&format!(
            "\"{escaped_key}\" {{\n    ChangeApplicationState\n    ReadApplicationState\n    ReadCliPipes\n}}\n"
        ));
        std::fs::write(&cache_path, updated).with_context(|| {
            format!(
                "failed to write zellij permissions cache {}",
                cache_path.display()
            )
        })?;
        Ok(())
    }

    fn break_plugin_url() -> Option<String> {
        fn local_source_newer_than_artifact(path: &Path) -> bool {
            let Ok(artifact_meta) = std::fs::metadata(path) else {
                return false;
            };
            let Ok(artifact_modified) = artifact_meta.modified() else {
                return false;
            };
            for ancestor in path.ancestors() {
                let source = ancestor.join("plugins/zellij-break/src/main.rs");
                let Ok(source_meta) = std::fs::metadata(&source) else {
                    continue;
                };
                let Ok(source_modified) = source_meta.modified() else {
                    continue;
                };
                if source_modified > artifact_modified {
                    logging::debug(format!(
                        "zellij: skipping stale break plugin artifact {} (source newer: {})",
                        path.display(),
                        source.display()
                    ));
                    return true;
                }
            }
            false
        }

        fn file_url(path: &Path) -> Option<String> {
            if !path.exists() {
                return None;
            }
            if local_source_newer_than_artifact(path) {
                return None;
            }
            let canonical = path.canonicalize().ok()?;
            Some(format!("file:{}", canonical.display()))
        }

        fn push_candidates(base: &Path, candidates: &mut Vec<std::path::PathBuf>) {
            candidates.push(
                base.join("zellij/plugins/yeet-and-yoink-zellij-break.wasm")
                    .to_path_buf(),
            );
            candidates.push(
                base.join("zellij/plugins/yeet_and_yoink_zellij_break.wasm")
                    .to_path_buf(),
            );
            candidates.push(
                base.join(
                    "plugins/zellij-break/target/wasm32-wasip1/release/yeet-and-yoink-zellij-break.wasm",
                )
                .to_path_buf(),
            );
            candidates.push(
                base.join(
                    "plugins/zellij-break/target/wasm32-wasip1/release/yeet_and_yoink_zellij_break.wasm",
                )
                .to_path_buf(),
            );
            candidates.push(
                base.join("target/wasm32-wasip1/release/yeet-and-yoink-zellij-break.wasm")
                    .to_path_buf(),
            );
            candidates.push(
                base.join("target/wasm32-wasip1/release/yeet_and_yoink_zellij_break.wasm")
                    .to_path_buf(),
            );
        }

        fn push_nix_store_candidates(candidates: &mut Vec<std::path::PathBuf>) {
            let Ok(entries) = std::fs::read_dir("/nix/store") else {
                return;
            };
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if !name.contains("yeet-and-yoink-zellij-break") {
                    continue;
                }
                let base = entry.path();
                candidates.push(base.join("yeet-and-yoink-zellij-break.wasm"));
                candidates.push(base.join("yeet_and_yoink_zellij_break.wasm"));
            }
        }

        fn candidate_recency_score(path: &Path) -> i64 {
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                return path.metadata().map(|meta| meta.ctime()).unwrap_or_default();
            }
            #[cfg(not(unix))]
            {
                let _ = path;
                0
            }
        }

        let env_override = std::env::var_os("NIRI_DEEP_ZELLIJ_BREAK_PLUGIN");
        if let Some(path) = env_override.as_deref() {
            if let Some(url) = file_url(Path::new(path)) {
                return Some(url);
            }
        }

        let mut candidates = Vec::new();
        if let Some(config_base) = std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(std::path::PathBuf::from)
                    .map(|home| home.join(".config"))
            })
        {
            push_candidates(&config_base, &mut candidates);
        }
        push_candidates(Path::new(env!("CARGO_MANIFEST_DIR")), &mut candidates);
        if let Ok(cwd) = std::env::current_dir() {
            push_candidates(&cwd, &mut candidates);
        }
        if let Ok(exe) = std::env::current_exe() {
            for ancestor in exe.ancestors().skip(1).take(8) {
                push_candidates(ancestor, &mut candidates);
            }
        }
        push_nix_store_candidates(&mut candidates);

        let mut ranked_candidates: Vec<_> = candidates
            .into_iter()
            .filter(|candidate| candidate.exists())
            .collect();
        ranked_candidates
            .sort_by_key(|candidate| std::cmp::Reverse(candidate_recency_score(candidate)));
        for candidate in ranked_candidates {
            if let Some(url) = file_url(&candidate) {
                return Some(url);
            }
        }
        None
    }

    fn parse_client_pane_id(line: &str) -> Option<String> {
        let mut columns = line.split_whitespace();
        let _client_id = columns.next()?;
        let pane_id = columns.next()?.trim();
        Self::normalize_pane_id(pane_id)
    }

    fn normalize_pane_id(raw: &str) -> Option<String> {
        let pane_id = raw.trim();
        if pane_id.is_empty() {
            return None;
        }
        if let Some(value) = pane_id.strip_prefix("terminal_") {
            let parsed = value.parse::<u32>().ok()?;
            return (parsed > 0).then(|| format!("terminal_{parsed}"));
        }
        if let Some(value) = pane_id.strip_prefix("plugin_") {
            let parsed = value.parse::<u32>().ok()?;
            return (parsed > 0).then(|| format!("plugin_{parsed}"));
        }
        let parsed = pane_id.parse::<u32>().ok()?;
        (parsed > 0).then(|| parsed.to_string())
    }

    fn is_terminal_pane_id(pane_id: &str) -> bool {
        pane_id.starts_with("terminal_") || pane_id.chars().all(|ch| ch.is_ascii_digit())
    }

    fn pane_id_number(raw: &str) -> Option<u64> {
        if let Some(value) = raw.trim().strip_prefix("terminal_") {
            return value.parse::<u64>().ok().filter(|value| *value > 0);
        }
        if let Some(value) = raw.trim().strip_prefix("plugin_") {
            return value.parse::<u64>().ok().filter(|value| *value > 0);
        }
        raw.trim().parse::<u64>().ok().filter(|value| *value > 0)
    }

    fn pane_id_from_environ(pid: u32) -> Option<String> {
        runtime::ProcessTree::for_pid(pid).find_map(|candidate| {
            runtime::process_environ_var(candidate, SOURCE_PANE_ENV)
                .and_then(|pane_id| Self::normalize_pane_id(&pane_id))
                .or_else(|| {
                    runtime::process_environ_var(candidate, "ZELLIJ_PANE_ID")
                        .and_then(|pane_id| Self::normalize_pane_id(&pane_id))
                })
        })
    }

    fn parse_pipe_query_pane_id(stdout: &str) -> Option<String> {
        for line in stdout.lines() {
            let trimmed = line.trim();
            if let Some(value) = trimmed.strip_prefix("pane_id=") {
                if let Some(pane_id) = Self::normalize_pane_id(value) {
                    return Some(pane_id);
                }
            }
            if let Some(pane_id) = Self::normalize_pane_id(trimmed) {
                return Some(pane_id);
            }
        }
        None
    }

    fn focused_pane_id_via_plugin(&self, pid: u32) -> Result<Option<String>> {
        if pid == 0 {
            return Ok(None);
        }
        let Some(plugin_url) = Self::break_plugin_url() else {
            return Ok(None);
        };
        if let Err(err) = Self::ensure_break_plugin_permissions(&plugin_url) {
            logging::debug(format!(
                "zellij: failed to update permission cache for {} err={:#}",
                plugin_url, err
            ));
        }
        let timeout = std::time::Duration::from_secs(2);
        for _ in 0..3 {
            let output = self.cli_output_for_pid_with_timeout(
                pid,
                &[
                    "pipe",
                    "--plugin",
                    &plugin_url,
                    "--name",
                    "yeet_and_yoink_break",
                    "--args",
                    "action=query-pane-id",
                    "--",
                    "query-pane-id",
                ],
                timeout,
            )?;
            let Some(output) = output else {
                break;
            };
            let stderr = runtime::stderr_text(&output);
            if Self::session_required_prompt(&stderr) {
                bail!(
                    "zellij session is required for pane query plugin; unable to resolve session for pid {}",
                    pid
                );
            }
            if !output.status.success() {
                bail!("zellij pane query plugin command failed: {}", stderr);
            }
            let stdout = runtime::stdout_text(&output);
            if let Some(pane_id) = Self::parse_pipe_query_pane_id(&stdout) {
                if Self::is_terminal_pane_id(&pane_id) {
                    return Ok(Some(pane_id));
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        Ok(None)
    }

    fn focused_pane_id_for_pid(&self, pid: u32) -> Result<Option<String>> {
        if let Some(pane_id) = Self::pane_id_from_environ(pid) {
            if Self::is_terminal_pane_id(&pane_id) {
                return Ok(Some(pane_id));
            }
        }
        if let Some(pane_id) = self.focused_pane_id_via_plugin(pid)? {
            if Self::is_terminal_pane_id(&pane_id) {
                return Ok(Some(pane_id));
            }
        }
        let clients = self.cli_stdout_for_pid(pid, &["action", "list-clients"])?;
        for line in clients.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("CLIENT_ID") {
                continue;
            }
            if let Some(pane_id) = Self::parse_client_pane_id(trimmed) {
                if Self::is_terminal_pane_id(&pane_id) {
                    return Ok(Some(pane_id));
                }
            }
        }
        Ok(None)
    }

    fn client_count_for_pid(&self, pid: u32) -> Result<usize> {
        let clients = self.cli_stdout_for_pid(pid, &["action", "list-clients"])?;
        Ok(clients
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with("CLIENT_ID"))
            .count())
    }

    fn invoke_break_plugin(
        &self,
        pid: u32,
        plugin_url: &str,
        pane_id: Option<&str>,
        source_tab_index: Option<usize>,
        source_client_count: Option<usize>,
        keep_source_tab_focused: bool,
    ) -> Result<()> {
        let timeout = std::time::Duration::from_secs(3);
        let mut arg_fields = Vec::new();
        if let Some(pane_id) = pane_id {
            arg_fields.push(format!("pane_id={pane_id}"));
        }
        if keep_source_tab_focused {
            arg_fields.push("focus_new_tab=false".to_string());
        }
        if let Some(source_tab_index) = source_tab_index {
            arg_fields.push(format!("source_tab_index={source_tab_index}"));
        }
        if let Some(source_client_count) = source_client_count {
            if source_client_count > 0 {
                arg_fields.push(format!("source_client_count={source_client_count}"));
            }
        }
        let output = if arg_fields.is_empty() {
            self.cli_output_for_pid_with_timeout(
                pid,
                &[
                    "pipe",
                    "--plugin",
                    plugin_url,
                    "--name",
                    "yeet_and_yoink_break",
                    "--",
                    "break",
                ],
                timeout,
            )
        } else {
            let args_value = arg_fields.join(",");
            self.cli_output_for_pid_with_timeout(
                pid,
                &[
                    "pipe",
                    "--plugin",
                    plugin_url,
                    "--name",
                    "yeet_and_yoink_break",
                    "--args",
                    &args_value,
                    "--",
                    "break",
                ],
                timeout,
            )
        }?;
        let Some(output) = output else {
            logging::debug(format!(
                "zellij: pipe command timed out for {}; continuing to verify detach state",
                plugin_url
            ));
            return Ok(());
        };
        let stderr = runtime::stderr_text(&output);
        if Self::session_required_prompt(&stderr) {
            bail!(
                "zellij session is required for break plugin; unable to resolve session for pid {}",
                pid
            );
        }
        if !output.status.success() {
            bail!("zellij break plugin command failed: {}", stderr);
        }
        Ok(())
    }

    fn invoke_merge_plugin(
        &self,
        pid: u32,
        plugin_url: &str,
        source_pane_id: u64,
        target_tab_index: Option<usize>,
    ) -> Result<()> {
        let timeout = std::time::Duration::from_secs(3);
        let mut args_value = format!("action=merge,source_pane_id=terminal_{source_pane_id}");
        if let Some(target_tab_index) = target_tab_index {
            args_value.push_str(&format!(",target_tab_index={target_tab_index}"));
        }
        let output = self.cli_output_for_pid_with_timeout(
            pid,
            &[
                "pipe",
                "--plugin",
                plugin_url,
                "--name",
                "yeet_and_yoink_break",
                "--args",
                &args_value,
                "--",
                "merge",
            ],
            timeout,
        )?;
        let Some(output) = output else {
            logging::debug(format!(
                "zellij: merge pipe command timed out for {}; continuing",
                plugin_url
            ));
            return Ok(());
        };
        let stderr = runtime::stderr_text(&output);
        if Self::session_required_prompt(&stderr) {
            bail!(
                "zellij session is required for merge plugin; unable to resolve session for pid {}",
                pid
            );
        }
        if !output.status.success() {
            bail!("zellij merge plugin command failed: {}", stderr);
        }
        Ok(())
    }

    fn cli_output_for_pid_with_timeout(
        &self,
        pid: u32,
        args: &[&str],
        timeout: std::time::Duration,
    ) -> Result<Option<std::process::Output>> {
        let mut command = std::process::Command::new("zellij");
        if let Some(session) = Self::session_name_for_pid(pid) {
            logging::debug(format!(
                "zellij: pid={} using session '{}' for {:?}",
                pid, session, args
            ));
            command.args(["--session", &session]);
        } else {
            logging::debug(format!(
                "zellij: pid={} without explicit session for {:?}",
                pid, args
            ));
        }
        command
            .args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = command.spawn().context("failed to run zellij command")?;
        let started = std::time::Instant::now();
        loop {
            match child
                .try_wait()
                .context("failed while waiting for zellij command")?
            {
                Some(status) => {
                    use std::io::Read as _;
                    let mut stdout = Vec::new();
                    if let Some(mut reader) = child.stdout.take() {
                        reader
                            .read_to_end(&mut stdout)
                            .context("failed to read zellij stdout")?;
                    }
                    let mut stderr = Vec::new();
                    if let Some(mut reader) = child.stderr.take() {
                        reader
                            .read_to_end(&mut stderr)
                            .context("failed to read zellij stderr")?;
                    }
                    return Ok(Some(std::process::Output {
                        status,
                        stdout,
                        stderr,
                    }));
                }
                None if started.elapsed() < timeout => {
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
                None => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Ok(None);
                }
            }
        }
    }

    fn layout_snapshot(&self, pid: u32) -> Result<ZellijLayoutSnapshot> {
        let layout = self.cli_stdout_for_pid(pid, &["action", "dump-layout"])?;
        Ok(Self::parse_layout_snapshot(&layout))
    }

    fn active_tab_index_for_pid(&self, pid: u32) -> Result<Option<usize>> {
        let layout = self.cli_stdout_for_pid(pid, &["action", "dump-layout"])?;
        let mut tab_index = 0usize;
        for line in layout.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("tab ") && trimmed.contains('{') {
                if trimmed.contains("focus=true") {
                    return Ok(Some(tab_index));
                }
                tab_index += 1;
            }
        }
        Ok(None)
    }

    fn no_neighbor(stderr: &str) -> bool {
        let value = stderr.to_ascii_lowercase();
        value.contains("no pane")
            || value.contains("cannot move focus")
            || value.contains("failed to move focus")
            || value.contains("no focusable")
            || value.contains("nothing to move focus")
    }

    fn try_move_focus(&self, pid: u32, dir: Direction) -> Result<bool> {
        let output = self.cli_output_for_pid(pid, &["action", "move-focus", dir.egocentric()])?;
        if output.status.success() {
            return Ok(true);
        }
        let stderr = runtime::stderr_text(&output);
        if Self::no_neighbor(&stderr) {
            return Ok(false);
        }
        bail!("zellij action move-focus {} failed: {}", dir, stderr.trim());
    }

    fn probe_neighbor(&self, pid: u32, dir: Direction) -> Result<Option<u64>> {
        let before = self.focused_pane_for_pid(pid)?;
        if !self.try_move_focus(pid, dir)? {
            return Ok(None);
        }
        let after = self.focused_pane_for_pid(pid).unwrap_or(before);
        if after != before {
            let _ = self.try_move_focus(pid, dir.opposite());
            return Ok(Some(after));
        }
        Ok(None)
    }
}

impl TerminalMultiplexerProvider for ZellijMuxProvider {
    fn cli_output_for_pid(&self, pid: u32, args: &[&str]) -> Result<std::process::Output> {
        let mut command = std::process::Command::new("zellij");
        if let Some(session) = Self::session_name_for_pid(pid) {
            logging::debug(format!(
                "zellij: pid={} using session '{}' for {:?}",
                pid, session, args
            ));
            command.args(["--session", &session]);
        } else {
            logging::debug(format!(
                "zellij: pid={} without explicit session for {:?}",
                pid, args
            ));
        }
        command
            .args(args)
            .output()
            .context("failed to run zellij command")
    }

    fn cli_stdout_for_pid(&self, pid: u32, args: &[&str]) -> Result<String> {
        let output = self.cli_output_for_pid(pid, args)?;
        let stderr = runtime::stderr_text(&output);
        if Self::session_required_prompt(&stderr) {
            bail!(
                "zellij session is required for {:?}; unable to resolve session for pid {}",
                args,
                pid
            );
        }
        if !output.status.success() {
            bail!("terminal multiplexer command {:?} failed: {}", args, stderr);
        }
        Ok(runtime::stdout_text(&output))
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities::terminal_mux_defaults()
    }

    fn list_panes_for_pid(&self, pid: u32) -> Result<Vec<TerminalPaneSnapshot>> {
        let snapshot = self.layout_snapshot(pid)?;
        Ok((1..=snapshot.pane_count)
            .map(|pane_id| TerminalPaneSnapshot {
                pane_id: pane_id as u64,
                tab_id: None,
                window_id: None,
                is_active: pane_id as u64 == snapshot.focused_pane_id,
                foreground_process_name: None,
            })
            .collect())
    }

    fn focused_pane_for_pid(&self, pid: u32) -> Result<u64> {
        Ok(self.layout_snapshot(pid)?.focused_pane_id)
    }

    fn pane_in_direction_for_pid(
        &self,
        pid: u32,
        pane_id: u64,
        dir: Direction,
    ) -> Result<Option<u64>> {
        let focused = self.focused_pane_for_pid(pid)?;
        if focused != pane_id {
            return Ok(None);
        }
        self.probe_neighbor(pid, dir)
    }

    fn send_text_to_pane(&self, pid: u32, pane_id: u64, text: &str) -> Result<()> {
        let focused_pane = self.focused_pane_for_pid(pid)?;
        if focused_pane != pane_id {
            let mut focused_target = false;
            for direction in Direction::ALL {
                if self.probe_neighbor(pid, direction)? == Some(pane_id) {
                    self.focus(direction, pid)?;
                    focused_target = true;
                    break;
                }
            }
            if !focused_target {
                bail!(
                    "zellij can only write to the focused pane or an adjacent pane; requested pane {} but focused pane is {}",
                    pane_id,
                    focused_pane
                );
            }
        }

        let has_trailing_newline = text.ends_with('\n');
        let lines: Vec<&str> = text.split('\n').collect();
        for (index, line) in lines.iter().enumerate() {
            if !line.is_empty() {
                self.cli_stdout_for_pid(pid, &["action", "write-chars", line])?;
            }
            let is_last = index + 1 == lines.len();
            if !is_last || has_trailing_newline {
                self.cli_stdout_for_pid(pid, &["action", "write", "13"])?;
            }
        }
        Ok(())
    }

    fn mux_attach_args(&self, target: String) -> Option<Vec<String>> {
        Some(vec!["zellij".into(), "attach".into(), target])
    }

    fn merge_source_pane_into_focused_target(
        &self,
        source_pid: u32,
        source_pane_id: u64,
        target_pid: u32,
        _target_window_id: Option<u64>,
        _dir: Direction,
    ) -> Result<()> {
        let source_session = Self::session_name_for_pid(source_pid)
            .context("source zellij merge missing session")?;
        let target_session = Self::session_name_for_pid(target_pid)
            .context("target zellij merge missing session")?;
        if source_session != target_session {
            bail!(
                "source and target zellij sessions differ ({} != {})",
                source_session,
                target_session
            );
        }
        let plugin_url = Self::break_plugin_url().with_context(|| {
            "zellij break plugin not found; build plugins/zellij-break so plugins/zellij-break/target/wasm32-wasip1/release/yeet-and-yoink-zellij-break.wasm exists"
        })?;
        if let Err(err) = Self::ensure_break_plugin_permissions(&plugin_url) {
            logging::debug(format!(
                "zellij: failed to update permission cache for {} err={:#}",
                plugin_url, err
            ));
        }
        if source_pid == target_pid {
            if let Some(target_pane) = self.focused_pane_id_for_pid(target_pid)? {
                if Self::pane_id_number(&target_pane) == Some(source_pane_id) {
                    bail!("source and target zellij panes are the same");
                }
            }
        }
        let target_tab_index = self.active_tab_index_for_pid(target_pid).unwrap_or(None);
        self.invoke_merge_plugin(target_pid, &plugin_url, source_pane_id, target_tab_index)?;
        Ok(())
    }
}

impl TopologyHandler for ZellijMuxProvider {
    fn directional_neighbors(&self, pid: u32) -> Result<DirectionalNeighbors> {
        self.directional_neighbors_from_pane_lookup(pid)
    }

    fn supports_rearrange_decision(&self) -> bool {
        false
    }

    fn window_count(&self, pid: u32) -> Result<u32> {
        self.active_scope_pane_count_for_pid(pid)
    }

    fn can_focus(&self, dir: Direction, pid: u32) -> Result<bool> {
        self.can_focus_from_pane_lookup(dir, pid)
    }

    fn move_decision(&self, dir: Direction, pid: u32) -> Result<MoveDecision> {
        self.move_decision_from_pane_lookup(dir, pid, false)
    }

    fn focus(&self, dir: Direction, pid: u32) -> Result<()> {
        self.cli_stdout_for_pid(pid, &["action", "move-focus", dir.egocentric()])?;
        Ok(())
    }

    fn move_internal(&self, dir: Direction, pid: u32) -> Result<()> {
        self.cli_stdout_for_pid(pid, &["action", "move-pane", dir.egocentric()])?;
        Ok(())
    }

    fn move_out(&self, _dir: Direction, pid: u32) -> Result<TearResult> {
        let session = Self::session_name_for_pid(pid)
            .with_context(|| format!("unable to resolve zellij session for pid {pid}"))?;
        let pane_id = self.focused_pane_id_for_pid(pid)?;
        let source_tab_index = self.active_tab_index_for_pid(pid).unwrap_or(None);
        let source_client_count = self
            .client_count_for_pid(pid)
            .ok()
            .filter(|count| *count > 0);
        let plugin_url = Self::break_plugin_url().with_context(|| {
            "zellij break plugin not found; build plugins/zellij-break so plugins/zellij-break/target/wasm32-wasip1/release/yeet-and-yoink-zellij-break.wasm exists"
        })?;
        if let Err(err) = Self::ensure_break_plugin_permissions(&plugin_url) {
            logging::debug(format!(
                "zellij: failed to update permission cache for {} err={:#}",
                plugin_url, err
            ));
        }

        self.invoke_break_plugin(
            pid,
            &plugin_url,
            pane_id.as_deref(),
            source_tab_index,
            source_client_count,
            false,
        )?;
        if pane_id.is_none() {
            for _ in 0..2 {
                std::thread::sleep(std::time::Duration::from_millis(60));
                self.invoke_break_plugin(
                    pid,
                    &plugin_url,
                    None,
                    source_tab_index,
                    source_client_count,
                    false,
                )?;
            }
        }
        let mut detached = false;
        for _ in 0..4 {
            if self.window_count(pid).unwrap_or(u32::MAX) <= 1 {
                detached = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(40));
        }
        if !detached {
            if pane_id.is_none() {
                bail!(
                    "zellij break plugin did not detach pane; unable to resolve focused zellij pane id for pid {}",
                    pid
                );
            }
            let pane_label = pane_id.as_deref().unwrap_or("<focused>");
            bail!(
                "zellij break plugin did not detach pane {}; grant ChangeApplicationState for {} (eg. `zellij action launch-or-focus-plugin {}` and press 'y')",
                pane_label,
                plugin_url,
                plugin_url
            );
        }

        let attach_command = if let Some(config_path) = Self::no_mirror_config_path() {
            let mut command = Vec::new();
            if let Some(pane_id) = pane_id.as_deref() {
                command.push("env".to_string());
                command.push(format!("{SOURCE_PANE_ENV}={pane_id}"));
            }
            command.extend([
                "zellij".to_string(),
                "--config".to_string(),
                config_path,
                "attach".to_string(),
                session.clone(),
            ]);
            command
        } else {
            let mut command = Vec::new();
            if let Some(pane_id) = pane_id.as_deref() {
                command.push("env".to_string());
                command.push(format!("{SOURCE_PANE_ENV}={pane_id}"));
            }
            command.extend(["zellij".to_string(), "attach".to_string(), session.clone()]);
            command
        };

        Ok(TearResult {
            spawn_command: Some(attach_command),
        })
    }

    fn merge_execution_mode(&self) -> MergeExecutionMode {
        MergeExecutionMode::TargetFocused
    }

    fn prepare_merge(&self, source_pid: Option<ProcessId>) -> Result<MergePreparation> {
        self.prepare_source_pane_merge(
            source_pid,
            "source zellij merge missing pid",
            |source_pid| {
                let pane_id = self
                    .focused_pane_id_for_pid(source_pid)?
                    .and_then(|pane_id| Self::pane_id_number(&pane_id))
                    .context("source zellij merge missing pane id")?;
                Ok((pane_id, ()))
            },
        )
    }

    fn merge_into_target(
        &self,
        dir: Direction,
        source_pid: Option<ProcessId>,
        target_pid: Option<ProcessId>,
        preparation: MergePreparation,
    ) -> Result<()> {
        let (source_pid, target_pid, preparation) = self.resolve_source_pane_merge::<()>(
            source_pid,
            target_pid,
            preparation,
            "source zellij merge missing pid",
            "target zellij merge missing pid",
            "source zellij merge missing pane metadata",
        )?;
        self.merge_source_pane_into_focused_target(
            source_pid,
            preparation.pane_id,
            target_pid,
            None,
            dir,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{ZellijMuxProvider, SOURCE_PANE_ENV};
    use crate::engine::contract::{MoveDecision, TerminalMultiplexerProvider, TopologyHandler};
    use crate::engine::runtime::ProcessId;
    use crate::engine::topology::Direction;

    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    const TEST_RESPONSES_ENV: &str = "ZELLIJ_TEST_RESPONSES_DIR";
    const TEST_LOG_ENV: &str = "ZELLIJ_TEST_LOG";

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::utils::env_guard()
    }

    struct ZellijHarness {
        base: PathBuf,
        responses_dir: PathBuf,
        log_file: PathBuf,
        old_path: Option<OsString>,
        old_responses_dir: Option<OsString>,
        old_log_file: Option<OsString>,
    }

    impl ZellijHarness {
        fn new() -> Self {
            let unique = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let base = std::env::temp_dir().join(format!(
                "yeet-and-yoink-zellij-mux-test-{}-{unique}",
                std::process::id()
            ));
            let bin_dir = base.join("bin");
            let responses_dir = base.join("responses");
            let log_file = base.join("commands.log");
            fs::create_dir_all(&bin_dir).expect("failed to create fake bin dir");
            fs::create_dir_all(&responses_dir).expect("failed to create fake responses dir");

            let fake_zellij = bin_dir.join("zellij");
            fs::write(
                &fake_zellij,
                r#"#!/bin/sh
set -eu
key="$*"
printf '%s\n' "$key" >> "${ZELLIJ_TEST_LOG}"
safe_key="$(printf '%s' "$key" | tr -c 'A-Za-z0-9._-' '_')"
status_file="${ZELLIJ_TEST_RESPONSES_DIR}/${safe_key}.status"
stdout_file="${ZELLIJ_TEST_RESPONSES_DIR}/${safe_key}.stdout"
stderr_file="${ZELLIJ_TEST_RESPONSES_DIR}/${safe_key}.stderr"
status=0
if [ -f "$status_file" ]; then
  status="$(cat "$status_file")"
fi
if [ -f "$stdout_file" ]; then
  cat "$stdout_file"
fi
if [ -f "$stderr_file" ]; then
  cat "$stderr_file" >&2
fi
exit "$status"
"#,
            )
            .expect("failed to write fake zellij script");
            let mut permissions = fs::metadata(&fake_zellij)
                .expect("failed to stat fake zellij script")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&fake_zellij, permissions)
                .expect("failed to chmod fake zellij script");

            let old_path = std::env::var_os("PATH");
            let old_responses_dir = std::env::var_os(TEST_RESPONSES_ENV);
            let old_log_file = std::env::var_os(TEST_LOG_ENV);
            let mut path_entries = vec![bin_dir];
            if let Some(ref old) = old_path {
                path_entries.extend(std::env::split_paths(old));
            }
            let path = std::env::join_paths(path_entries).expect("failed to compose PATH");
            std::env::set_var("PATH", path);
            std::env::set_var(TEST_RESPONSES_ENV, &responses_dir);
            std::env::set_var(TEST_LOG_ENV, &log_file);

            Self {
                base,
                responses_dir,
                log_file,
                old_path,
                old_responses_dir,
                old_log_file,
            }
        }

        fn set_response(&self, key: &str, status: i32, stdout: &str, stderr: &str) {
            let safe_key: String = key
                .chars()
                .map(|ch| {
                    if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                        ch
                    } else {
                        '_'
                    }
                })
                .collect();
            fs::write(
                self.responses_dir.join(format!("{safe_key}.status")),
                status.to_string(),
            )
            .expect("failed to write fake status");
            fs::write(
                self.responses_dir.join(format!("{safe_key}.stdout")),
                stdout,
            )
            .expect("failed to write fake stdout");
            fs::write(
                self.responses_dir.join(format!("{safe_key}.stderr")),
                stderr,
            )
            .expect("failed to write fake stderr");
        }

        fn command_log(&self) -> String {
            fs::read_to_string(&self.log_file).unwrap_or_default()
        }
    }

    impl Drop for ZellijHarness {
        fn drop(&mut self) {
            if let Some(value) = &self.old_path {
                std::env::set_var("PATH", value);
            } else {
                std::env::remove_var("PATH");
            }
            if let Some(value) = &self.old_responses_dir {
                std::env::set_var(TEST_RESPONSES_ENV, value);
            } else {
                std::env::remove_var(TEST_RESPONSES_ENV);
            }
            if let Some(value) = &self.old_log_file {
                std::env::set_var(TEST_LOG_ENV, value);
            } else {
                std::env::remove_var(TEST_LOG_ENV);
            }
            let _ = fs::remove_dir_all(&self.base);
        }
    }

    #[test]
    fn focused_pane_and_window_count_come_from_dump_layout() {
        let _guard = env_guard();
        let harness = ZellijHarness::new();
        harness.set_response(
            "action dump-layout",
            0,
            r#"
            layout {
              pane split_direction="vertical" focus=true
              pane split_direction="vertical"
            }
            "#,
            "",
        );
        let provider = ZellijMuxProvider;
        assert_eq!(provider.focused_pane_for_pid(0).expect("focused pane"), 1);
        assert_eq!(provider.window_count(0).expect("window_count"), 2);
    }

    #[test]
    fn send_text_to_pane_writes_chars_and_enter_to_focused_pane() {
        let _guard = env_guard();
        let harness = ZellijHarness::new();
        let pid = 9301;
        ZellijMuxProvider::store_session_name(pid, "nvim-zellij-test");

        harness.set_response(
            "--session nvim-zellij-test action dump-layout",
            0,
            r#"
            layout {
              pane split_direction="vertical" focus=true
              pane split_direction="vertical"
            }
            "#,
            "",
        );
        harness.set_response(
            "--session nvim-zellij-test action write-chars echo hello",
            0,
            "",
            "",
        );
        harness.set_response("--session nvim-zellij-test action write 13", 0, "", "");

        let provider = ZellijMuxProvider;
        provider
            .send_text_to_pane(pid, 1, "echo hello\n")
            .expect("send_text_to_pane should succeed");

        let log = harness.command_log();
        assert!(log.contains("--session nvim-zellij-test action write-chars echo hello"));
        assert!(log.contains("--session nvim-zellij-test action write 13"));
    }

    #[test]
    fn send_text_to_pane_errors_when_target_pane_cannot_be_focused() {
        let _guard = env_guard();
        let harness = ZellijHarness::new();
        let pid = 9302;
        ZellijMuxProvider::store_session_name(pid, "nvim-zellij-test");

        harness.set_response(
            "--session nvim-zellij-test action dump-layout",
            0,
            r#"
            layout {
              pane split_direction="vertical" focus=true
              pane split_direction="vertical"
            }
            "#,
            "",
        );

        let provider = ZellijMuxProvider;
        let err = provider
            .send_text_to_pane(pid, 2, "echo hello\n")
            .expect_err("send_text_to_pane should reject unreachable pane");
        assert!(format!("{err:#}").contains("focused pane or an adjacent pane"));
        assert!(!harness.command_log().contains("action write-chars"));
    }

    #[test]
    fn focused_tab_layout_ignores_non_active_tabs_and_plugin_panes() {
        let _guard = env_guard();
        let harness = ZellijHarness::new();
        harness.set_response(
            "action dump-layout",
            0,
            r#"
            layout {
              tab name="Tab #1" focus=true {
                pane size=1 borderless=true { plugin location="zellij:tab-bar" }
                pane split_direction="vertical" {
                  pane cwd="/tmp" focus=true
                  pane cwd="/tmp"
                }
                pane size=1 borderless=true { plugin location="zellij:status-bar" }
              }
              tab name="Tab #2" {
                pane cwd="/elsewhere"
              }
              new_tab_template {
                pane
              }
            }
            "#,
            "",
        );

        let provider = ZellijMuxProvider;
        assert_eq!(provider.window_count(0).expect("window_count"), 2);
        assert_eq!(provider.focused_pane_for_pid(0).expect("focused pane"), 1);
    }

    #[test]
    fn can_focus_probe_does_not_move_back_when_focus_did_not_change() {
        let _guard = env_guard();
        let harness = ZellijHarness::new();
        harness.set_response(
            "action dump-layout",
            0,
            r#"
            layout {
              pane split_direction="vertical" focus=true
              pane split_direction="vertical"
            }
            "#,
            "",
        );
        harness.set_response("action move-focus right", 0, "", "");

        let provider = ZellijMuxProvider;
        let can_focus = provider
            .can_focus(Direction::East, 0)
            .expect("can_focus should succeed");
        assert!(!can_focus);

        let log = harness.command_log();
        assert!(log.contains("action move-focus right"));
        assert!(!log.contains("action move-focus left"));
    }

    #[test]
    fn move_decision_uses_tear_out_when_edge_has_no_neighbor() {
        let _guard = env_guard();
        let harness = ZellijHarness::new();
        harness.set_response(
            "action dump-layout",
            0,
            r#"
            layout {
              tab focus=true {
                pane split_direction="vertical" {
                  pane focus=true
                  pane
                }
              }
            }
            "#,
            "",
        );
        harness.set_response("action move-focus right", 0, "", "");

        let provider = ZellijMuxProvider;
        let decision = provider
            .move_decision(Direction::East, 0)
            .expect("move_decision should succeed");
        assert_eq!(decision, MoveDecision::TearOut);
    }

    #[test]
    fn detects_session_required_prompt() {
        assert!(ZellijMuxProvider::session_required_prompt(
            "Please specify the session name to send actions to."
        ));
    }

    #[test]
    fn extracts_session_name_from_server_path() {
        assert_eq!(
            ZellijMuxProvider::session_from_server_path(
                "/run/user/1000/zellij/0.43.1/jumping-quasar"
            ),
            Some("jumping-quasar".to_string())
        );
    }

    #[test]
    fn focused_pane_errors_when_session_name_is_required() {
        let _guard = env_guard();
        let harness = ZellijHarness::new();
        harness.set_response(
            "action dump-layout",
            0,
            "jumping-quasar [Created 1m ago]",
            "Please specify the session name to send actions to.",
        );
        let provider = ZellijMuxProvider;
        let error = provider
            .focused_pane_for_pid(0)
            .expect_err("missing session should error");
        assert!(error
            .to_string()
            .contains("unable to resolve session for pid 0"));
    }

    #[test]
    fn focus_uses_move_focus_action() {
        let _guard = env_guard();
        let harness = ZellijHarness::new();
        harness.set_response("action move-focus left", 0, "", "");
        let provider = ZellijMuxProvider;
        provider
            .focus(Direction::West, 0)
            .expect("focus should succeed");
        assert!(harness.command_log().contains("action move-focus left"));
    }

    #[test]
    fn move_internal_uses_move_pane_action() {
        let _guard = env_guard();
        let harness = ZellijHarness::new();
        harness.set_response("action move-pane right", 0, "", "");
        let provider = ZellijMuxProvider;
        provider
            .move_internal(Direction::East, 0)
            .expect("move_internal should succeed");
        assert!(harness.command_log().contains("action move-pane right"));
    }

    #[test]
    fn parse_client_pane_id_accepts_terminal_and_plugin_ids() {
        assert_eq!(
            ZellijMuxProvider::parse_client_pane_id("1 terminal_42 N/A"),
            Some("terminal_42".to_string())
        );
        assert_eq!(
            ZellijMuxProvider::parse_client_pane_id("2 plugin_7 zellij:tab-bar"),
            Some("plugin_7".to_string())
        );
        assert_eq!(
            ZellijMuxProvider::parse_client_pane_id("3 12 bash"),
            Some("12".to_string())
        );
        assert_eq!(
            ZellijMuxProvider::parse_client_pane_id("CLIENT_ID ZELLIJ_PANE_ID RUNNING_COMMAND"),
            None
        );
        assert_eq!(
            ZellijMuxProvider::normalize_pane_id("terminal_9"),
            Some("terminal_9".to_string())
        );
        assert_eq!(
            ZellijMuxProvider::normalize_pane_id("  plugin_3 "),
            Some("plugin_3".to_string())
        );
        assert_eq!(ZellijMuxProvider::normalize_pane_id("terminal_0"), None);
        assert_eq!(ZellijMuxProvider::normalize_pane_id("0"), None);
        assert_eq!(ZellijMuxProvider::normalize_pane_id("pane_x"), None);
    }

    #[test]
    fn focused_pane_id_for_pid_ignores_plugin_client_ids() {
        let _guard = env_guard();
        let harness = ZellijHarness::new();
        ZellijMuxProvider::store_session_name(91, "plugin-pane");
        harness.set_response(
            "--session plugin-pane action list-clients",
            0,
            r#"
            CLIENT_ID ZELLIJ_PANE_ID RUNNING_COMMAND
            1 plugin_8 zellij:tab-bar
            "#,
            "",
        );
        let provider = ZellijMuxProvider;
        assert_eq!(provider.focused_pane_id_for_pid(91).expect("query"), None);
    }

    #[test]
    fn focused_pane_id_for_pid_prefers_spawned_source_pane_env() {
        let _guard = env_guard();
        let harness = ZellijHarness::new();
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .env(SOURCE_PANE_ENV, "terminal_27")
            .spawn()
            .expect("sleep should spawn");
        let provider = ZellijMuxProvider;
        let pane_id = provider
            .focused_pane_id_for_pid(child.id())
            .expect("query should succeed");
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(pane_id, Some("terminal_27".to_string()));
        assert!(!harness.command_log().contains("action list-clients"));
    }

    #[test]
    fn parse_pipe_query_pane_id_accepts_plain_and_prefixed_values() {
        assert_eq!(
            ZellijMuxProvider::parse_pipe_query_pane_id("terminal_8\n"),
            Some("terminal_8".to_string())
        );
        assert_eq!(
            ZellijMuxProvider::parse_pipe_query_pane_id("pane_id=terminal_12\n"),
            Some("terminal_12".to_string())
        );
        assert_eq!(ZellijMuxProvider::parse_pipe_query_pane_id(""), None);
    }

    #[test]
    fn focused_pane_id_for_pid_uses_plugin_query_before_list_clients() {
        let _guard = env_guard();
        let harness = ZellijHarness::new();
        let plugin_path = harness.base.join("yeet_and_yoink_zellij_break.wasm");
        fs::write(&plugin_path, "fake wasm").expect("plugin placeholder should be written");
        std::env::set_var(
            "NIRI_DEEP_ZELLIJ_BREAK_PLUGIN",
            plugin_path.to_str().expect("utf-8 plugin path"),
        );
        ZellijMuxProvider::store_session_name(92, "query-pane");
        let plugin_url = ZellijMuxProvider::break_plugin_url().expect("plugin URL should resolve");
        harness.set_response(
            &format!(
                "--session query-pane pipe --plugin {plugin_url} --name yeet_and_yoink_break --args action=query-pane-id -- query-pane-id"
            ),
            0,
            "terminal_19\n",
            "",
        );

        let provider = ZellijMuxProvider;
        let pane_id = provider
            .focused_pane_id_for_pid(92)
            .expect("query should succeed");
        assert_eq!(pane_id, Some("terminal_19".to_string()));
        let log = harness.command_log();
        assert!(log.contains("action=query-pane-id"));
        assert!(!log.contains("action list-clients"));
        std::env::remove_var("NIRI_DEEP_ZELLIJ_BREAK_PLUGIN");
    }

    #[test]
    fn permission_block_contains_scans_all_matching_nodes() {
        let raw = r#"
        "/tmp/yeet-and-yoink-zellij-break.wasm" {
            ChangeApplicationState
        }
        "/tmp/yeet-and-yoink-zellij-break.wasm" {
            ChangeApplicationState
            ReadApplicationState
        }
        "#;
        assert!(ZellijMuxProvider::permission_block_contains(
            raw,
            "/tmp/yeet-and-yoink-zellij-break.wasm",
            "ReadApplicationState"
        ));
    }

    #[test]
    fn move_out_uses_break_plugin_pipe_and_returns_attach_command() {
        let _guard = env_guard();
        let harness = ZellijHarness::new();
        let plugin_path = harness.base.join("yeet_and_yoink_zellij_break.wasm");
        fs::write(&plugin_path, "fake wasm").expect("plugin placeholder should be written");
        std::env::set_var(
            "NIRI_DEEP_ZELLIJ_BREAK_PLUGIN",
            plugin_path.to_str().expect("utf-8 plugin path"),
        );
        ZellijMuxProvider::store_session_name(42, "jumping-quasar");
        harness.set_response(
            "--session jumping-quasar action list-clients",
            0,
            r#"
            CLIENT_ID ZELLIJ_PANE_ID RUNNING_COMMAND
            1 terminal_9 N/A
            "#,
            "",
        );

        let plugin_url = ZellijMuxProvider::break_plugin_url().expect("plugin URL should resolve");
        harness.set_response(
            &format!(
                "--session jumping-quasar pipe --plugin {plugin_url} --name yeet_and_yoink_break --args pane_id=terminal_9,source_tab_index=0,source_client_count=1 -- break"
            ),
            0,
            "",
            "",
        );
        harness.set_response(
            "--session jumping-quasar action dump-layout",
            0,
            r#"
            layout {
              tab focus=true {
                pane focus=true
              }
            }
            "#,
            "",
        );

        let provider = ZellijMuxProvider;
        let tear = provider
            .move_out(Direction::North, 42)
            .expect("move_out should succeed");
        let command = tear.spawn_command.expect("spawn command should exist");
        assert_eq!(command.first(), Some(&"env".to_string()));
        assert!(command.contains(&format!("{SOURCE_PANE_ENV}=terminal_9")));
        assert!(command.contains(&"zellij".to_string()));
        assert!(command.contains(&"attach".to_string()));
        assert!(command.contains(&"jumping-quasar".to_string()));
        if command.contains(&"--config".to_string()) {
            let config_index = command
                .iter()
                .position(|arg| arg == "--config")
                .expect("--config should be present");
            assert!(command.get(config_index + 1).is_some());
        }

        let log = harness.command_log();
        assert!(log.contains("action list-clients"));
        assert!(log.contains("pipe --plugin"));
        std::env::remove_var("NIRI_DEEP_ZELLIJ_BREAK_PLUGIN");
    }

    #[test]
    fn move_out_errors_when_break_plugin_does_not_detach_pane() {
        let _guard = env_guard();
        let harness = ZellijHarness::new();
        let plugin_path = harness.base.join("yeet_and_yoink_zellij_break.wasm");
        fs::write(&plugin_path, "fake wasm").expect("plugin placeholder should be written");
        std::env::set_var(
            "NIRI_DEEP_ZELLIJ_BREAK_PLUGIN",
            plugin_path.to_str().expect("utf-8 plugin path"),
        );
        ZellijMuxProvider::store_session_name(77, "glowing-zebra");
        harness.set_response(
            "--session glowing-zebra action list-clients",
            0,
            r#"
            CLIENT_ID ZELLIJ_PANE_ID RUNNING_COMMAND
            1 terminal_4 N/A
            "#,
            "",
        );
        let plugin_url = ZellijMuxProvider::break_plugin_url().expect("plugin URL should resolve");
        harness.set_response(
            &format!(
                "--session glowing-zebra pipe --plugin {plugin_url} --name yeet_and_yoink_break --args pane_id=terminal_4,source_tab_index=0,source_client_count=1 -- break"
            ),
            0,
            "",
            "",
        );
        harness.set_response(
            "--session glowing-zebra action dump-layout",
            0,
            r#"
            layout {
              tab focus=true {
                pane split_direction="vertical" {
                  pane focus=true
                  pane
                }
              }
            }
            "#,
            "",
        );

        let provider = ZellijMuxProvider;
        let err = match provider.move_out(Direction::North, 77) {
            Ok(_) => panic!("move_out should fail when pane stays attached"),
            Err(err) => err,
        };
        assert!(
            err.to_string().contains("did not detach pane terminal_4"),
            "unexpected error: {err:#}"
        );
        std::env::remove_var("NIRI_DEEP_ZELLIJ_BREAK_PLUGIN");
    }

    #[test]
    fn move_out_retries_break_without_pane_id_when_list_clients_is_terminal_zero() {
        let _guard = env_guard();
        let harness = ZellijHarness::new();
        let plugin_path = harness.base.join("yeet_and_yoink_zellij_break.wasm");
        fs::write(&plugin_path, "fake wasm").expect("plugin placeholder should be written");
        std::env::set_var(
            "NIRI_DEEP_ZELLIJ_BREAK_PLUGIN",
            plugin_path.to_str().expect("utf-8 plugin path"),
        );
        ZellijMuxProvider::store_session_name(55, "wise-mouse");
        harness.set_response(
            "--session wise-mouse action list-clients",
            0,
            r#"
            CLIENT_ID ZELLIJ_PANE_ID RUNNING_COMMAND
            1 terminal_0 N/A
            "#,
            "",
        );
        let plugin_url = ZellijMuxProvider::break_plugin_url().expect("plugin URL should resolve");
        harness.set_response(
            &format!(
                "--session wise-mouse pipe --plugin {plugin_url} --name yeet_and_yoink_break --args source_tab_index=0,source_client_count=1 -- break"
            ),
            0,
            "",
            "",
        );
        harness.set_response(
            "--session wise-mouse action dump-layout",
            0,
            r#"
            layout {
              tab focus=true {
                pane focus=true
              }
            }
            "#,
            "",
        );

        let provider = ZellijMuxProvider;
        let tear = provider
            .move_out(Direction::West, 55)
            .expect("move_out should succeed");
        assert!(tear.spawn_command.is_some());
        let log = harness.command_log();
        assert!(log.contains("action list-clients"));
        assert!(log.contains(
            "--name yeet_and_yoink_break --args source_tab_index=0,source_client_count=1 -- break"
        ));
        assert!(!log.contains("--args pane_id=terminal_0"));
        std::env::remove_var("NIRI_DEEP_ZELLIJ_BREAK_PLUGIN");
    }

    #[test]
    fn merge_into_target_uses_plugin_merge_pipe_with_source_pane_metadata() {
        let _guard = env_guard();
        let harness = ZellijHarness::new();
        let plugin_path = harness.base.join("yeet_and_yoink_zellij_break.wasm");
        fs::write(&plugin_path, "fake wasm").expect("plugin placeholder should be written");
        std::env::set_var(
            "NIRI_DEEP_ZELLIJ_BREAK_PLUGIN",
            plugin_path.to_str().expect("utf-8 plugin path"),
        );
        ZellijMuxProvider::store_session_name(50, "merge-shared");
        ZellijMuxProvider::store_session_name(60, "merge-shared");
        harness.set_response(
            "--session merge-shared action list-clients",
            0,
            r#"
            CLIENT_ID ZELLIJ_PANE_ID RUNNING_COMMAND
            1 terminal_11 N/A
            "#,
            "",
        );
        harness.set_response(
            "--session merge-shared action dump-layout",
            0,
            r#"
            layout {
              tab focus=true {
                pane focus=true
              }
            }
            "#,
            "",
        );
        let plugin_url = ZellijMuxProvider::break_plugin_url().expect("plugin URL should resolve");
        harness.set_response(
            &format!(
                "--session merge-shared pipe --plugin {plugin_url} --name yeet_and_yoink_break --args action=merge,source_pane_id=terminal_11,target_tab_index=0 -- merge"
            ),
            0,
            "",
            "",
        );

        let provider = ZellijMuxProvider;
        let preparation = provider
            .prepare_merge(ProcessId::new(50))
            .expect("prepare_merge should succeed");
        provider
            .merge_into_target(
                Direction::West,
                ProcessId::new(50),
                ProcessId::new(60),
                preparation,
            )
            .expect("merge_into_target should succeed");

        let log = harness.command_log();
        assert!(log.contains("pipe --plugin"));
        assert!(log.contains("action=merge"));
        assert!(log.contains("source_pane_id=terminal_11"));
        assert!(log.contains("target_tab_index=0"));
        assert!(log.contains("-- merge"));
        std::env::remove_var("NIRI_DEEP_ZELLIJ_BREAK_PLUGIN");
    }

    #[test]
    fn merge_into_target_rejects_cross_session_merge() {
        let _guard = env_guard();
        let harness = ZellijHarness::new();
        let plugin_path = harness.base.join("yeet_and_yoink_zellij_break.wasm");
        fs::write(&plugin_path, "fake wasm").expect("plugin placeholder should be written");
        std::env::set_var(
            "NIRI_DEEP_ZELLIJ_BREAK_PLUGIN",
            plugin_path.to_str().expect("utf-8 plugin path"),
        );
        ZellijMuxProvider::store_session_name(70, "merge-source");
        ZellijMuxProvider::store_session_name(71, "merge-target");
        harness.set_response(
            "--session merge-source action list-clients",
            0,
            r#"
            CLIENT_ID ZELLIJ_PANE_ID RUNNING_COMMAND
            1 terminal_13 N/A
            "#,
            "",
        );

        let provider = ZellijMuxProvider;
        let preparation = provider
            .prepare_merge(ProcessId::new(70))
            .expect("prepare_merge should succeed");
        let err = provider
            .merge_into_target(
                Direction::West,
                ProcessId::new(70),
                ProcessId::new(71),
                preparation,
            )
            .expect_err("cross-session merge should fail");
        assert!(err.to_string().contains("sessions differ"));
        let log = harness.command_log();
        assert!(!log.contains("action=merge"));
        assert!(!log.contains("-- merge"));
        std::env::remove_var("NIRI_DEEP_ZELLIJ_BREAK_PLUGIN");
    }
}
