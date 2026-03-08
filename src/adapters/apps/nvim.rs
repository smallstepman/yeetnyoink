use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::adapters::apps::AppAdapter;
use crate::adapters::terminal_multiplexers;
use crate::config::TerminalMuxBackend;
use crate::engine::contract::{
    AdapterCapabilities, AppKind, MoveDecision, TearResult, TerminalMultiplexerProvider,
    TopologyHandler,
};
use crate::engine::runtime::{self, CommandContext};
use crate::engine::topology::Direction;
use crate::logging;

pub const ADAPTER_NAME: &str = "nvim";
pub const ADAPTER_ALIASES: &[&str] = &["nvim"];

pub struct Nvim {
    /// Path to the nvim RPC socket.
    server_addr: String,
    terminal_mux: NvimTerminalMux,
}

#[derive(Debug, Deserialize)]
struct BufferSnapshot {
    path: String,
    line: u32,
    col: u32,
    modified: bool,
}

#[derive(Clone, Copy)]
enum NvimTerminalMux {
    Wezterm,
    Tmux,
    Zellij,
    Kitty,
    #[cfg(test)]
    Stub,
}

impl NvimTerminalMux {
    fn from_backend(backend: TerminalMuxBackend) -> Self {
        match backend {
            TerminalMuxBackend::Wezterm => Self::Wezterm,
            TerminalMuxBackend::Tmux => Self::Tmux,
            TerminalMuxBackend::Zellij => Self::Zellij,
            TerminalMuxBackend::Kitty => Self::Kitty,
        }
    }

    fn provider(self) -> &'static dyn TerminalMultiplexerProvider {
        match self {
            Self::Wezterm => &terminal_multiplexers::wezterm::WEZTERM_MUX_PROVIDER,
            Self::Tmux => &terminal_multiplexers::tmux::TMUX_MUX_PROVIDER,
            Self::Zellij => &terminal_multiplexers::zellij::ZELLIJ_MUX_PROVIDER,
            Self::Kitty => &terminal_multiplexers::kitty::KITTY_MUX_PROVIDER,
            #[cfg(test)]
            Self::Stub => &tests::STUB_MUX_PROVIDER,
        }
    }
}

impl Nvim {
    /// Create an Nvim handler for a specific nvim process by finding its socket.
    pub fn for_pid(nvim_pid: u32, terminal_mux_backend: TerminalMuxBackend) -> Option<Self> {
        let addr = Self::find_socket_for_pid(nvim_pid).ok()?;
        Some(Nvim {
            server_addr: addr,
            terminal_mux: NvimTerminalMux::from_backend(terminal_mux_backend),
        })
    }

    #[cfg(test)]
    fn for_test(server_addr: &str, terminal_mux: NvimTerminalMux) -> Self {
        Self {
            server_addr: server_addr.to_string(),
            terminal_mux,
        }
    }

    fn terminal_mux_provider(&self) -> &'static dyn TerminalMultiplexerProvider {
        self.terminal_mux.provider()
    }

    /// Find the nvim RPC socket for a specific PID.
    ///
    /// Strategy:
    /// 1. Read /proc/<pid>/environ for NVIM= or NVIM_LISTEN_ADDRESS=
    /// 2. Fallback: scan XDG_RUNTIME_DIR for nvim.<pid>.* sockets
    fn find_socket_for_pid(pid: u32) -> Result<String> {
        // Try reading the nvim process's environment
        if let Some(addr) = runtime::process_environ_var(pid, "NVIM") {
            return Ok(addr);
        }
        if let Some(addr) = runtime::process_environ_var(pid, "NVIM_LISTEN_ADDRESS") {
            return Ok(addr);
        }

        // Fallback: scan for socket files matching the PID
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
        let search_dirs = [runtime_dir.as_str(), "/tmp"];
        for dir in &search_dirs {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    let prefix = format!("nvim.{pid}.");
                    if name.starts_with(&prefix) {
                        let path = entry.path();
                        return Ok(path.to_string_lossy().to_string());
                    }
                }
            }
        }

        bail!("no nvim socket found for pid {pid}")
    }

    fn remote_expr_on(server_addr: &str, expr: &str) -> Result<String> {
        let output = runtime::run_command_output(
            "nvim",
            &["--server", server_addr, "--remote-expr", expr],
            &CommandContext::new("nvim", "remote-expr").with_target(server_addr.to_string()),
        )
        .context("failed to run nvim --remote-expr")?;
        if !output.status.success() {
            bail!(
                "nvim --remote-expr failed: {}",
                runtime::stderr_text(&output)
            );
        }
        Ok(runtime::stdout_text(&output))
    }

    fn remote_expr(&self, expr: &str) -> Result<String> {
        Self::remote_expr_on(&self.server_addr, expr)
    }

    fn remote_send_on(server_addr: &str, keys: &str) -> Result<()> {
        runtime::run_command_status(
            "nvim",
            &["--server", server_addr, "--remote-send", keys],
            &CommandContext::new("nvim", "remote-send").with_target(server_addr.to_string()),
        )
        .context("failed to run nvim --remote-send")
    }

    fn remote_send(&self, keys: &str) -> Result<()> {
        Self::remote_send_on(&self.server_addr, keys)
    }

    fn winnr_at_edge(dir: Direction) -> String {
        format!("winnr()==winnr('{}')", dir.vim_key())
    }

    fn wincmd_key(dir: Direction) -> char {
        dir.vim_key()
    }

    fn smart_splits_direction(dir: Direction) -> &'static str {
        dir.egocentric()
    }

    fn smart_mux_current_pane_id_expr() -> &'static str {
        r#"luaeval("local ok,m=pcall(require,'smart-splits.mux'); if not ok then return -1 end; local mux=m.get(); if not mux then return -1 end; local id=mux.current_pane_id(); if id == nil then return -1 end; return id")"#
    }

    fn smart_mux_split_pane_expr(dir: Direction) -> String {
        let dir = Self::smart_splits_direction(dir);
        format!(
            r#"luaeval("local ok,m=pcall(require,'smart-splits.mux'); if not ok then return false end; local mux=m.get(); if not mux then return false end; return mux.split_pane('{dir}')")"#
        )
    }

    fn parse_nvim_bool(value: &str) -> bool {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "v:true"
        )
    }

    fn smart_mux_current_pane_id(&self) -> Result<u64> {
        let pane_id = self
            .remote_expr(Self::smart_mux_current_pane_id_expr())?
            .trim()
            .parse::<i64>()
            .unwrap_or(-1);
        if pane_id <= 0 {
            bail!("smart-splits mux current_pane_id unavailable");
        }
        Ok(pane_id as u64)
    }

    fn smart_mux_split_pane(&self, dir: Direction) -> Result<bool> {
        let expr = Self::smart_mux_split_pane_expr(dir);
        let output = self.remote_expr(&expr)?;
        Ok(Self::parse_nvim_bool(&output))
    }

    fn visible_window_count_expr() -> &'static str {
        "winnr('$')"
    }

    fn visible_window_count(&self) -> Result<u32> {
        Ok(self
            .remote_expr(Self::visible_window_count_expr())?
            .parse()
            .unwrap_or(1))
    }

    fn at_edge(&self, dir: Direction) -> Result<bool> {
        Ok(self.remote_expr(&Self::winnr_at_edge(dir))? == "1")
    }

    fn snapshot_expr() -> &'static str {
        "json_encode({'path':expand('%:p'),'line':line('.'),'col':col('.'),'modified':&modified})"
    }

    fn current_buffer_snapshot(&self) -> Result<BufferSnapshot> {
        let json = self.remote_expr(Self::snapshot_expr())?;
        let mut snapshot: BufferSnapshot = serde_json::from_str(&json)
            .with_context(|| format!("invalid nvim snapshot json: {json}"))?;
        snapshot.path = snapshot.path.trim().to_string();
        if snapshot.line == 0 {
            snapshot.line = 1;
        }
        Ok(snapshot)
    }

    fn target_socket_path() -> Result<std::path::PathBuf> {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
        let base_dir = std::path::PathBuf::from(runtime_dir).join("yeet-and-yoink-nvim");
        std::fs::create_dir_all(&base_dir).with_context(|| {
            format!("failed to create nvim tear-out dir: {}", base_dir.display())
        })?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        Ok(base_dir.join(format!("tearout-{}-{stamp}.sock", std::process::id())))
    }

    fn shell_single_quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }

    fn launch_target_nvim_command(target_socket: &str, snapshot: &BufferSnapshot) -> String {
        let line_arg = format!("+{}", snapshot.line.max(1));
        format!(
            "nvim --listen {} {} {}\n",
            Self::shell_single_quote(target_socket),
            line_arg,
            Self::shell_single_quote(&snapshot.path)
        )
    }

    fn wait_for_target_nvim(target_socket: &str) -> Result<()> {
        for _ in 0..20 {
            if Path::new(target_socket).exists() && Self::remote_expr_on(target_socket, "1").is_ok()
            {
                return Ok(());
            }
            sleep(Duration::from_millis(25));
        }
        bail!("timed out waiting for torn-out nvim target to become ready")
    }

    fn wait_for_target_terminal_pane(
        &self,
        dir: Direction,
        terminal_pid: u32,
        source_pane_id: u64,
    ) -> Result<u64> {
        for _ in 0..20 {
            if let Ok(pane_id) = self
                .terminal_mux_provider()
                .focused_pane_for_pid(terminal_pid)
            {
                if pane_id != source_pane_id {
                    return Ok(pane_id);
                }
            }
            if let Ok(Some(pane_id)) = self.terminal_mux_provider().pane_in_direction_for_pid(
                terminal_pid,
                source_pane_id,
                dir,
            ) {
                return Ok(pane_id);
            }
            if let Ok(pane_id) = self.smart_mux_current_pane_id() {
                if pane_id != source_pane_id {
                    return Ok(pane_id);
                }
            }
            sleep(Duration::from_millis(25));
        }
        bail!("timed out waiting for smart-splits target pane after nvim split")
    }

    fn tear_out_to_terminal_pane(&self, dir: Direction, terminal_pid: u32) -> Result<()> {
        if terminal_pid == 0 {
            bail!("missing terminal multiplexer pid for nvim tear-out");
        }

        let snapshot = self.current_buffer_snapshot()?;
        if snapshot.path.is_empty() {
            bail!("nvim tear-out requires a file-backed buffer");
        }
        if snapshot.modified {
            bail!("nvim tear-out requires a saved buffer; please save first");
        }

        let source_pane_id = self.smart_mux_current_pane_id()?;
        if !self.smart_mux_split_pane(dir)? {
            bail!("smart-splits mux split_pane failed; ensure smart-splits.nvim is configured");
        }
        let target_pane_id =
            self.wait_for_target_terminal_pane(dir, terminal_pid, source_pane_id)?;
        let target_socket = Self::target_socket_path()?;
        let target_socket = target_socket.to_string_lossy().to_string();
        let launch_command = Self::launch_target_nvim_command(&target_socket, &snapshot);

        logging::debug(format!(
            "nvim: tear-out dir={} terminal_pid={} source_pane={} target_pane={} path={} line={} col={}",
            dir,
            terminal_pid,
            source_pane_id,
            target_pane_id,
            snapshot.path,
            snapshot.line,
            snapshot.col
        ));

        self.terminal_mux_provider().send_text_to_pane(
            terminal_pid,
            target_pane_id,
            &launch_command,
        )?;
        Self::wait_for_target_nvim(&target_socket)?;

        if snapshot.col > 1 {
            Self::remote_send_on(&target_socket, &format!("<Esc>{}|", snapshot.col))?;
        }

        // Tear out by closing the source split after target is ready.
        self.remote_send("<Esc><C-w>c")?;
        Ok(())
    }
}

impl AppAdapter for Nvim {
    fn adapter_name(&self) -> &'static str {
        ADAPTER_NAME
    }

    fn config_aliases(&self) -> Option<&'static [&'static str]> {
        Some(ADAPTER_ALIASES)
    }

    fn kind(&self) -> AppKind {
        AppKind::Editor
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            probe: true,
            focus: true,
            move_internal: true,
            resize_internal: false,
            rearrange: false,
            tear_out: false,
            merge: false,
        }
    }

    fn eval(
        &self,
        expression: &str,
        _pid: Option<crate::engine::runtime::ProcessId>,
    ) -> Result<String> {
        self.remote_expr(expression)
    }
}

impl TopologyHandler for Nvim {
    fn can_focus(&self, dir: Direction, _pid: u32) -> Result<bool> {
        let expr = Self::winnr_at_edge(dir);
        let result = self.remote_expr(&expr)?;
        Ok(result != "1")
    }

    fn move_decision(&self, dir: Direction, _pid: u32) -> Result<MoveDecision> {
        let win_count = self.visible_window_count()?;
        if win_count <= 1 {
            return Ok(MoveDecision::Passthrough);
        }
        let _ = dir;
        Ok(MoveDecision::Internal)
    }

    fn focus(&self, dir: Direction, _pid: u32) -> Result<()> {
        let key = Self::wincmd_key(dir);
        self.remote_send(&format!("<C-w>{key}"))
    }

    fn move_internal(&self, dir: Direction, pid: u32) -> Result<()> {
        if self.at_edge(dir)? {
            return self.tear_out_to_terminal_pane(dir, pid);
        }
        let key = Self::wincmd_key(dir).to_ascii_uppercase();
        self.remote_send(&format!("<C-w>{key}"))
    }

    fn move_out(&self, _dir: Direction, _pid: u32) -> Result<TearResult> {
        // v1: should not be reached — move_decision returns Passthrough at edge.
        bail!("nvim move_out is unreachable; tear-out is handled in move_internal")
    }
}

#[cfg(test)]
mod tests {
    use anyhow::{bail, Context, Result};
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Mutex, OnceLock};

    use super::{Nvim, NvimTerminalMux, ADAPTER_ALIASES};
    use crate::engine::contract::{
        AdapterCapabilities, AppAdapter, MoveDecision, TearResult, TerminalMultiplexerProvider,
        TopologyHandler,
    };
    use crate::engine::topology::Direction;

    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    pub(super) static STUB_MUX_PROVIDER: StubMuxProvider = StubMuxProvider;
    static STUB_MUX_STATE: OnceLock<Mutex<StubMuxState>> = OnceLock::new();

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::utils::env_guard()
    }

    #[derive(Default)]
    struct StubMuxState {
        focused_pane: Option<u64>,
        neighbor_pane: Option<u64>,
        send_calls: Vec<(u32, u64, String)>,
    }

    pub(super) struct StubMuxProvider;

    impl StubMuxProvider {
        fn state() -> &'static Mutex<StubMuxState> {
            STUB_MUX_STATE.get_or_init(|| Mutex::new(StubMuxState::default()))
        }

        fn reset(focused_pane: Option<u64>, neighbor_pane: Option<u64>) {
            let mut state = Self::state().lock().expect("stub mux state should lock");
            *state = StubMuxState {
                focused_pane,
                neighbor_pane,
                send_calls: Vec::new(),
            };
        }

        fn send_calls() -> Vec<(u32, u64, String)> {
            Self::state()
                .lock()
                .expect("stub mux state should lock")
                .send_calls
                .clone()
        }

        fn launch_socket_path(text: &str) -> Option<PathBuf> {
            let (_, rest) = text.split_once("--listen ")?;
            let socket = if let Some(rest) = rest.strip_prefix('\'') {
                rest.split('\'').next()?
            } else {
                rest.split_whitespace().next()?
            };
            Some(PathBuf::from(socket))
        }

        fn create_target_socket(text: &str) -> Result<()> {
            let Some(socket_path) = Self::launch_socket_path(text) else {
                return Ok(());
            };
            if let Some(parent) = socket_path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            fs::write(&socket_path, "")
                .with_context(|| format!("failed to create {}", socket_path.display()))?;
            Ok(())
        }
    }

    impl TopologyHandler for StubMuxProvider {
        fn can_focus(&self, _dir: Direction, _pid: u32) -> Result<bool> {
            Ok(false)
        }

        fn focus(&self, _dir: Direction, _pid: u32) -> Result<()> {
            Ok(())
        }

        fn move_internal(&self, _dir: Direction, _pid: u32) -> Result<()> {
            Ok(())
        }

        fn move_out(&self, _dir: Direction, _pid: u32) -> Result<TearResult> {
            bail!("stub mux move_out should not be called")
        }
    }

    impl TerminalMultiplexerProvider for StubMuxProvider {
        fn capabilities(&self) -> AdapterCapabilities {
            AdapterCapabilities {
                probe: true,
                focus: true,
                move_internal: true,
                resize_internal: true,
                rearrange: true,
                tear_out: true,
                merge: true,
            }
        }

        fn focused_pane_for_pid(&self, _pid: u32) -> Result<u64> {
            Self::state()
                .lock()
                .expect("stub mux state should lock")
                .focused_pane
                .context("missing focused pane in stub mux state")
        }

        fn pane_in_direction_for_pid(
            &self,
            _pid: u32,
            _pane_id: u64,
            _dir: Direction,
        ) -> Result<Option<u64>> {
            Ok(Self::state()
                .lock()
                .expect("stub mux state should lock")
                .neighbor_pane)
        }

        fn send_text_to_pane(&self, pid: u32, pane_id: u64, text: &str) -> Result<()> {
            Self::create_target_socket(text)?;
            Self::state()
                .lock()
                .expect("stub mux state should lock")
                .send_calls
                .push((pid, pane_id, text.to_string()));
            Ok(())
        }

        fn mux_attach_args(&self, _target: String) -> Option<Vec<String>> {
            None
        }

        fn merge_source_pane_into_focused_target(
            &self,
            _source_pid: u32,
            _source_pane_id: u64,
            _target_pid: u32,
            _target_window_id: Option<u64>,
            _dir: Direction,
        ) -> Result<()> {
            bail!("stub mux merge should not be called")
        }
    }

    #[test]
    fn declares_explicit_capability_contract() {
        let app = Nvim::for_test("/tmp/test.sock", NvimTerminalMux::Stub);
        let caps = AppAdapter::capabilities(&app);
        assert_eq!(app.config_aliases(), Some(ADAPTER_ALIASES));
        assert!(caps.probe);
        assert!(caps.focus);
        assert!(caps.move_internal);
        assert!(!caps.resize_internal);
        assert!(!caps.rearrange);
        assert!(!caps.tear_out);
        assert!(!caps.merge);
    }

    fn sanitize_key(key: &str) -> String {
        let mut sanitized: String = key
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        if sanitized.len() > 120 {
            sanitized.truncate(120);
        }
        sanitized
    }

    struct NvimHarness {
        base: PathBuf,
        nvim_responses_dir: PathBuf,
        nvim_log_file: PathBuf,
        old_path: Option<OsString>,
        old_runtime_dir: Option<OsString>,
        old_nvim_responses_dir: Option<OsString>,
        old_nvim_log_file: Option<OsString>,
    }

    impl NvimHarness {
        fn new() -> Self {
            let unique = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let base = std::env::temp_dir().join(format!(
                "yeet-and-yoink-nvim-test-{}-{unique}",
                std::process::id()
            ));
            let bin_dir = base.join("bin");
            let runtime_dir = base.join("runtime");
            let nvim_responses_dir = base.join("nvim-responses");
            let nvim_log_file = base.join("nvim.log");

            fs::create_dir_all(&bin_dir).expect("failed to create fake bin dir");
            fs::create_dir_all(&runtime_dir).expect("failed to create fake runtime dir");
            fs::create_dir_all(&nvim_responses_dir).expect("failed to create nvim responses dir");

            let fake_nvim = bin_dir.join("nvim");
            fs::write(
                &fake_nvim,
                r#"#!/bin/sh
set -eu
key="$*"
printf '%s\n' "$key" >> "${NVIM_TEST_LOG}"
safe_key="$(printf '%s' "$key" | tr -c 'A-Za-z0-9._-' '_' | cut -c1-120)"
status_file="${NVIM_TEST_RESPONSES_DIR}/${safe_key}.status"
stdout_file="${NVIM_TEST_RESPONSES_DIR}/${safe_key}.stdout"
stderr_file="${NVIM_TEST_RESPONSES_DIR}/${safe_key}.stderr"
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
            .expect("failed to write fake nvim");
            let mut nvim_perms = fs::metadata(&fake_nvim)
                .expect("failed to stat fake nvim")
                .permissions();
            nvim_perms.set_mode(0o755);
            fs::set_permissions(&fake_nvim, nvim_perms).expect("failed to chmod fake nvim");

            let old_path = std::env::var_os("PATH");
            let old_runtime_dir = std::env::var_os("XDG_RUNTIME_DIR");
            let old_nvim_responses_dir = std::env::var_os("NVIM_TEST_RESPONSES_DIR");
            let old_nvim_log_file = std::env::var_os("NVIM_TEST_LOG");

            let mut path_entries = vec![bin_dir];
            if let Some(ref old) = old_path {
                path_entries.extend(std::env::split_paths(old));
            }
            let path = std::env::join_paths(path_entries).expect("failed to join PATH entries");

            std::env::set_var("PATH", path);
            std::env::set_var("XDG_RUNTIME_DIR", &runtime_dir);
            std::env::set_var("NVIM_TEST_RESPONSES_DIR", &nvim_responses_dir);
            std::env::set_var("NVIM_TEST_LOG", &nvim_log_file);

            Self {
                base,
                nvim_responses_dir,
                nvim_log_file,
                old_path,
                old_runtime_dir,
                old_nvim_responses_dir,
                old_nvim_log_file,
            }
        }

        fn set_nvim_response(&self, key: &str, status: i32, stdout: &str, stderr: &str) {
            let safe_key = sanitize_key(key);
            fs::write(
                self.nvim_responses_dir.join(format!("{safe_key}.status")),
                status.to_string(),
            )
            .expect("failed to write fake nvim status");
            fs::write(
                self.nvim_responses_dir.join(format!("{safe_key}.stdout")),
                stdout,
            )
            .expect("failed to write fake nvim stdout");
            fs::write(
                self.nvim_responses_dir.join(format!("{safe_key}.stderr")),
                stderr,
            )
            .expect("failed to write fake nvim stderr");
        }

        fn nvim_log(&self) -> String {
            fs::read_to_string(&self.nvim_log_file).unwrap_or_default()
        }
    }

    impl Drop for NvimHarness {
        fn drop(&mut self) {
            if let Some(value) = &self.old_path {
                std::env::set_var("PATH", value);
            } else {
                std::env::remove_var("PATH");
            }

            if let Some(value) = &self.old_runtime_dir {
                std::env::set_var("XDG_RUNTIME_DIR", value);
            } else {
                std::env::remove_var("XDG_RUNTIME_DIR");
            }

            if let Some(value) = &self.old_nvim_responses_dir {
                std::env::set_var("NVIM_TEST_RESPONSES_DIR", value);
            } else {
                std::env::remove_var("NVIM_TEST_RESPONSES_DIR");
            }

            if let Some(value) = &self.old_nvim_log_file {
                std::env::set_var("NVIM_TEST_LOG", value);
            } else {
                std::env::remove_var("NVIM_TEST_LOG");
            }

            let _ = fs::remove_dir_all(&self.base);
        }
    }

    fn test_app() -> Nvim {
        Nvim::for_test("/tmp/source.sock", NvimTerminalMux::Stub)
    }

    fn prime_tear_out_responses(harness: &NvimHarness, dir: Direction) {
        harness.set_nvim_response(
            &format!(
                "--server /tmp/source.sock --remote-expr {}",
                Nvim::winnr_at_edge(dir)
            ),
            0,
            "1\n",
            "",
        );
        harness.set_nvim_response(
            "--server /tmp/source.sock --remote-expr json_encode({'path':expand('%:p'),'line':line('.'),'col':col('.'),'modified':&modified})",
            0,
            r#"{"path":"/tmp/main.rs","line":10,"col":4,"modified":false}"#,
            "",
        );
        harness.set_nvim_response(
            &format!(
                "--server /tmp/source.sock --remote-expr {}",
                Nvim::smart_mux_current_pane_id_expr()
            ),
            0,
            "55\n",
            "",
        );
        harness.set_nvim_response(
            &format!(
                "--server /tmp/source.sock --remote-expr {}",
                Nvim::smart_mux_split_pane_expr(dir)
            ),
            0,
            "1\n",
            "",
        );
    }

    #[test]
    fn move_decision_passthrough_when_single_window() {
        let _env_guard = env_guard();
        let harness = NvimHarness::new();
        let app = test_app();

        harness.set_nvim_response(
            "--server /tmp/source.sock --remote-expr winnr('$')",
            0,
            "1\n",
            "",
        );

        let decision = app
            .move_decision(Direction::East, 4242)
            .expect("move_decision should succeed");
        assert!(matches!(decision, MoveDecision::Passthrough));
    }

    #[test]
    fn move_decision_internal_when_multiple_windows() {
        let _env_guard = env_guard();
        let harness = NvimHarness::new();
        let app = test_app();

        harness.set_nvim_response(
            "--server /tmp/source.sock --remote-expr winnr('$')",
            0,
            "3\n",
            "",
        );

        let decision = app
            .move_decision(Direction::West, 4343)
            .expect("move_decision should succeed");
        assert!(matches!(decision, MoveDecision::Internal));
    }

    #[test]
    fn move_internal_swaps_when_not_at_edge() {
        let _env_guard = env_guard();
        let harness = NvimHarness::new();
        let app = test_app();

        harness.set_nvim_response(
            "--server /tmp/source.sock --remote-expr winnr()==winnr('l')",
            0,
            "0\n",
            "",
        );

        app.move_internal(Direction::East, 4444)
            .expect("move_internal should swap");

        let log = harness.nvim_log();
        assert!(log.contains("--server /tmp/source.sock --remote-send <C-w>L"));
    }

    #[test]
    fn move_internal_tears_out_into_focused_mux_target_after_split() {
        let _env_guard = env_guard();
        let harness = NvimHarness::new();
        let app = test_app();
        StubMuxProvider::reset(Some(77), None);
        prime_tear_out_responses(&harness, Direction::East);

        app.move_internal(Direction::East, 5555)
            .expect("move_internal should tear out");

        let send_calls = StubMuxProvider::send_calls();
        assert_eq!(send_calls.len(), 1);
        assert_eq!(send_calls[0].0, 5555);
        assert_eq!(send_calls[0].1, 77);
        assert!(send_calls[0].2.contains("nvim --listen "));

        let nvim_log = harness.nvim_log();
        assert!(nvim_log.contains("smart-splits.mux"));
        assert!(nvim_log.contains("return mux.split_pane('right')"));
        assert!(nvim_log.contains("--remote-send <Esc>4|"));
        assert!(nvim_log.contains("--server /tmp/source.sock --remote-send <Esc><C-w>c"));
    }

    #[test]
    fn move_internal_falls_back_to_neighbor_lookup_when_split_keeps_source_focused() {
        let _env_guard = env_guard();
        let harness = NvimHarness::new();
        let app = test_app();
        StubMuxProvider::reset(Some(55), Some(88));
        prime_tear_out_responses(&harness, Direction::East);

        app.move_internal(Direction::East, 6666)
            .expect("move_internal should tear out");

        let send_calls = StubMuxProvider::send_calls();
        assert_eq!(send_calls.len(), 1);
        assert_eq!(send_calls[0].0, 6666);
        assert_eq!(send_calls[0].1, 88);
    }
}
