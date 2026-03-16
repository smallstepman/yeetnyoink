use anyhow::{bail, Context, Result};
use serde::{Deserialize, Deserializer};
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::adapters::apps::AppAdapter;
use crate::adapters::terminal_multiplexers;
use crate::config::TerminalMuxBackend;
use crate::engine::contracts::{
    AdapterCapabilities, AppKind, MergeExecutionMode, MergePreparation, MoveDecision, TearResult,
    TerminalMultiplexerProvider, TopologyHandler,
};
use crate::engine::runtime::{self, CommandContext};
use crate::engine::topology::Direction;
use crate::logging;

pub const ADAPTER_NAME: &str = "nvim";
pub const ADAPTER_ALIASES: &[&str] = &["nvim", "neovim"];

pub struct Nvim {
    /// Path to the nvim RPC socket.
    server_addr: String,
    terminal_mux: NvimTerminalMux,
}

#[derive(Debug, Clone, Deserialize)]
struct BufferSnapshot {
    path: String,
    line: u32,
    col: u32,
    #[serde(default)]
    buftype: String,
    #[serde(deserialize_with = "deserialize_nvim_modified_flag")]
    modified: bool,
}

#[derive(Debug, Clone)]
struct NvimMergePayload {
    snapshot: BufferSnapshot,
    source_server_addr: String,
    source_pane_id: Option<u64>,
    source_swap_path: Option<String>,
    source_is_torn_out: bool,
}

impl BufferSnapshot {
    fn is_file_backed(&self) -> bool {
        self.buftype.trim().is_empty() && !self.path.trim().is_empty()
    }
}

fn deserialize_nvim_modified_flag<'de, D>(deserializer: D) -> std::result::Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum ModifiedFlag {
        Bool(bool),
        Integer(i64),
        String(String),
    }

    match ModifiedFlag::deserialize(deserializer)? {
        ModifiedFlag::Bool(value) => Ok(value),
        ModifiedFlag::Integer(0) => Ok(false),
        ModifiedFlag::Integer(1) => Ok(true),
        ModifiedFlag::Integer(value) => Err(serde::de::Error::custom(format!(
            "expected modified flag to be 0 or 1, got {value}"
        ))),
        ModifiedFlag::String(value) => match value.trim().to_ascii_lowercase().as_str() {
            "0" | "false" | "v:false" => Ok(false),
            "1" | "true" | "v:true" => Ok(true),
            other => Err(serde::de::Error::custom(format!(
                "expected modified flag to be bool-like, got {other}"
            ))),
        },
    }
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
        let (resolved_pid, addr) = Self::find_socket_for_pid(nvim_pid).ok()?;
        if resolved_pid != nvim_pid {
            logging::debug(format!(
                "nvim: resolved socket for pid {} via descendant pid {}",
                nvim_pid, resolved_pid
            ));
        }
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
    /// 1. Try the requested pid and any descendant nvim pids, because terminal discovery
    ///    can report the outer UI process while the RPC socket belongs to an embedded child.
    /// 2. For each candidate, read /proc/<pid>/environ for NVIM= or NVIM_LISTEN_ADDRESS=
    /// 3. Fallback: scan XDG_RUNTIME_DIR for nvim.<pid>.* sockets
    fn find_socket_for_pid(pid: u32) -> Result<(u32, String)> {
        let mut candidates = vec![pid];
        for descendant in runtime::find_descendants_by_comm(pid, "nvim") {
            if !candidates.contains(&descendant) {
                candidates.push(descendant);
            }
        }
        Self::find_socket_for_candidates(candidates)
    }

    fn find_socket_for_candidates<I>(candidates: I) -> Result<(u32, String)>
    where
        I: IntoIterator<Item = u32>,
    {
        let mut attempted = Vec::new();
        for pid in candidates {
            attempted.push(pid);
            if let Ok(addr) = Self::find_socket_for_exact_pid(pid) {
                return Ok((pid, addr));
            }
        }
        bail!("no nvim socket found for pid candidates {:?}", attempted)
    }

    fn find_socket_for_exact_pid(pid: u32) -> Result<String> {
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
        r#"luaeval("(function() local ok,m=pcall(require,'smart-splits.mux'); if not ok then return -1 end; local mux=m.get(); if not mux then return -1 end; local id=mux.current_pane_id(); if id == nil then return -1 end; return id end)()")"#
    }

    fn smart_mux_split_pane_expr(dir: Direction) -> String {
        let dir = Self::smart_splits_direction(dir);
        format!(
            r#"luaeval("(function() local ok,m=pcall(require,'smart-splits.mux'); if not ok then return false end; local mux=m.get(); if not mux then return false end; return mux.split_pane('{dir}') end)()")"#
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
        "json_encode({'path':expand('%:p'),'line':line('.'),'col':col('.'),'buftype':&buftype,'modified':&modified})"
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

    fn ensure_mergeable_snapshot(snapshot: BufferSnapshot, action: &str) -> Result<BufferSnapshot> {
        if !snapshot.is_file_backed() {
            let kind = if snapshot.buftype.trim().is_empty() {
                "unnamed"
            } else {
                snapshot.buftype.trim()
            };
            bail!("{action} requires a file-backed buffer; current buffer type is {kind}");
        }
        if snapshot.modified {
            bail!("{action} requires a saved buffer; please save first");
        }
        Ok(snapshot)
    }

    fn current_mergeable_snapshot(&self, action: &str) -> Result<BufferSnapshot> {
        Self::ensure_mergeable_snapshot(self.current_buffer_snapshot()?, action)
    }

    fn swap_path_expr() -> &'static str {
        r#"luaeval("(function() local path = vim.fn.swapname(vim.fn.bufnr('%')); if path == nil then return '' end; return path end)()")"#
    }

    fn current_swap_path(&self) -> Result<Option<String>> {
        let path = self.remote_expr(Self::swap_path_expr())?.trim().to_string();
        if path.is_empty() {
            Ok(None)
        } else {
            Ok(Some(path))
        }
    }

    fn is_torn_out_instance(&self) -> bool {
        Path::new(&self.server_addr)
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("tearout-"))
    }

    fn target_socket_path() -> Result<std::path::PathBuf> {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
        let base_dir = std::path::PathBuf::from(runtime_dir).join("yeetnyoink-nvim");
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

    fn vimscript_single_quote(value: &str) -> String {
        value.replace('\'', "''")
    }

    fn lua_double_quote(value: &str) -> String {
        let mut escaped = String::with_capacity(value.len() + 2);
        escaped.push('"');
        for ch in value.chars() {
            match ch {
                '\\' => escaped.push_str("\\\\"),
                '"' => escaped.push_str("\\\""),
                '\n' => escaped.push_str("\\n"),
                '\r' => escaped.push_str("\\r"),
                '\t' => escaped.push_str("\\t"),
                _ => escaped.push(ch),
            }
        }
        escaped.push('"');
        escaped
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

    fn merge_target_split_command(side: Direction) -> &'static str {
        match side {
            Direction::West => "topleft vsplit",
            Direction::East => "botright vsplit",
            Direction::North => "topleft split",
            Direction::South => "botright split",
        }
    }

    fn merge_target_expr(dir: Direction, snapshot: &BufferSnapshot) -> String {
        let split_cmd = Self::lua_double_quote(Self::merge_target_split_command(dir.opposite()));
        let path = Self::lua_double_quote(&snapshot.path);
        let lua = format!(
            "(function() vim.cmd({split_cmd}); vim.cmd(\"edit \" .. vim.fn.fnameescape({path})); vim.fn.cursor({}, {}); return 1 end)()",
            snapshot.line.max(1),
            snapshot.col.max(1)
        );
        format!("luaeval('{}')", Self::vimscript_single_quote(&lua))
    }

    fn tear_out_source_cleanup_expr() -> &'static str {
        r#"luaeval("(function() local bufnr = vim.fn.bufnr('%'); if vim.fn.winnr('$') > 1 then vim.cmd('silent! close') end; if vim.fn.bufexists(bufnr) == 1 then vim.cmd('silent! bdelete! ' .. bufnr) end; return 1 end)()")"#
    }

    fn quit_expr() -> &'static str {
        r#"luaeval("(function() vim.cmd('silent! qa!'); return 1 end)()")"#
    }

    fn wait_for_server_exit(server_addr: &str) {
        for _ in 0..20 {
            if Self::remote_expr_on(server_addr, "1").is_err() {
                return;
            }
            sleep(Duration::from_millis(25));
        }
    }

    fn wait_for_swap_clear(swap_path: Option<&str>) {
        let Some(swap_path) = swap_path.filter(|value| !value.trim().is_empty()) else {
            return;
        };
        for _ in 0..40 {
            if !Path::new(swap_path).exists() {
                return;
            }
            sleep(Duration::from_millis(25));
        }
    }

    fn close_torn_out_source_before_merge(&self, payload: &NvimMergePayload) -> Result<()> {
        if !payload.source_is_torn_out || payload.source_server_addr == self.server_addr {
            return Ok(());
        }
        Self::remote_expr_on(&payload.source_server_addr, Self::quit_expr())
            .context("failed to quit torn-out nvim source before merge")?;
        Self::wait_for_server_exit(&payload.source_server_addr);
        Self::wait_for_swap_clear(payload.source_swap_path.as_deref());
        Ok(())
    }

    fn cleanup_torn_out_source_pane_after_merge(
        &self,
        terminal_pid: Option<crate::engine::runtime::ProcessId>,
        payload: &NvimMergePayload,
    ) {
        let Some(terminal_pid) = terminal_pid.map(crate::engine::runtime::ProcessId::get) else {
            return;
        };
        let Some(source_pane_id) = payload.source_pane_id else {
            return;
        };
        if !payload.source_is_torn_out || payload.source_server_addr == self.server_addr {
            return;
        }
        let _ =
            self.terminal_mux_provider()
                .send_text_to_pane(terminal_pid, source_pane_id, "exit\n");
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

        let snapshot = self.current_mergeable_snapshot("nvim tear-out")?;

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

        self.remote_expr(Self::tear_out_source_cleanup_expr())
            .context("failed to remove torn-out buffer from source nvim")?;
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
            merge: true,
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

    fn merge_execution_mode(&self) -> MergeExecutionMode {
        MergeExecutionMode::TargetFocused
    }

    fn prepare_merge(
        &self,
        source_pid: Option<crate::engine::runtime::ProcessId>,
    ) -> Result<MergePreparation> {
        Ok(MergePreparation::with_payload(NvimMergePayload {
            snapshot: self.current_mergeable_snapshot("nvim merge-back")?,
            source_server_addr: self.server_addr.clone(),
            source_pane_id: source_pid.and_then(|pid| {
                self.terminal_mux_provider()
                    .focused_pane_for_pid(pid.get())
                    .ok()
            }),
            source_swap_path: self.current_swap_path()?,
            source_is_torn_out: self.is_torn_out_instance(),
        }))
    }

    fn merge_into_target(
        &self,
        dir: Direction,
        source_pid: Option<crate::engine::runtime::ProcessId>,
        target_pid: Option<crate::engine::runtime::ProcessId>,
        preparation: MergePreparation,
    ) -> Result<()> {
        let payload = preparation
            .into_payload::<NvimMergePayload>()
            .context("nvim merge preparation missing")?;
        self.close_torn_out_source_before_merge(&payload)?;
        self.remote_expr(&Self::merge_target_expr(dir, &payload.snapshot))
            .context("failed to merge nvim buffer into target split")?;
        if source_pid == target_pid {
            self.cleanup_torn_out_source_pane_after_merge(target_pid, &payload);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use anyhow::{bail, Context, Result};
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Mutex, OnceLock};

    use super::{BufferSnapshot, Nvim, NvimMergePayload, NvimTerminalMux, ADAPTER_ALIASES};
    use crate::engine::contracts::{
        AdapterCapabilities, AppAdapter, MoveDecision, TearResult, TerminalMultiplexerProvider,
        TopologyHandler,
    };
    use crate::engine::runtime::ProcessId;
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
        assert!(caps.merge);
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
        let suffix = Command::new("cksum")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(stdin) = child.stdin.as_mut() {
                    let _ = stdin.write_all(key.as_bytes());
                }
                child.wait_with_output()
            })
            .ok()
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .and_then(|stdout| stdout.split_whitespace().next().map(|s| s.to_string()))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "0".to_string());
        let max_prefix_len = 120usize.saturating_sub(suffix.len() + 1);
        if sanitized.len() > max_prefix_len {
            sanitized.truncate(max_prefix_len);
        }
        format!("{sanitized}_{suffix}")
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
                "yeetnyoink-nvim-test-{}-{unique}",
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
hash_suffix="$(printf '%s' "$key" | cksum | cut -d ' ' -f1)"
max_prefix_len=$((120 - ${#hash_suffix} - 1))
safe_key="$(printf '%s' "$safe_key" | cut -c1-"$max_prefix_len")_${hash_suffix}"
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
            "--server /tmp/source.sock --remote-expr json_encode({'path':expand('%:p'),'line':line('.'),'col':col('.'),'buftype':&buftype,'modified':&modified})",
            0,
            r#"{"path":"/tmp/main.rs","line":10,"col":4,"buftype":"","modified":0}"#,
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
        harness.set_nvim_response(
            &format!(
                "--server /tmp/source.sock --remote-expr {}",
                Nvim::tear_out_source_cleanup_expr()
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
    fn find_socket_falls_back_to_later_nvim_pid_candidate() {
        let _env_guard = env_guard();
        let harness = NvimHarness::new();
        let expected = harness.base.join("runtime").join("nvim.200.0");
        fs::write(&expected, "").expect("socket placeholder should be writable");

        let (resolved_pid, socket) =
            Nvim::find_socket_for_candidates([100, 200]).expect("socket should resolve");

        assert_eq!(resolved_pid, 200);
        assert_eq!(PathBuf::from(socket), expected);
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
    fn current_buffer_snapshot_accepts_numeric_modified_flag() {
        let _env_guard = env_guard();
        let harness = NvimHarness::new();
        let app = test_app();

        harness.set_nvim_response(
            "--server /tmp/source.sock --remote-expr json_encode({'path':expand('%:p'),'line':line('.'),'col':col('.'),'buftype':&buftype,'modified':&modified})",
            0,
            r#"{"path":" /tmp/main.rs ","line":10,"col":4,"buftype":"","modified":0}"#,
            "",
        );

        let snapshot = app
            .current_buffer_snapshot()
            .expect("numeric modified snapshot should parse");

        assert_eq!(snapshot.path, "/tmp/main.rs");
        assert_eq!(snapshot.line, 10);
        assert_eq!(snapshot.col, 4);
        assert!(!snapshot.modified);
    }

    #[test]
    fn smart_mux_luaeval_wraps_statements_in_function_expression() {
        assert!(Nvim::smart_mux_current_pane_id_expr().contains("(function()"));
        assert!(Nvim::smart_mux_current_pane_id_expr().contains("end)()"));
        assert!(Nvim::smart_mux_split_pane_expr(Direction::East).contains("(function()"));
        assert!(Nvim::smart_mux_split_pane_expr(Direction::East).contains("end)()"));
    }

    #[test]
    fn merge_target_expr_uses_valid_lua_function_expression() {
        let expr = Nvim::merge_target_expr(
            Direction::East,
            &BufferSnapshot {
                path: "/tmp/it's.rs".to_string(),
                line: 12,
                col: 7,
                buftype: String::new(),
                modified: false,
            },
        );

        assert!(expr.starts_with("luaeval('"));
        assert!(expr.contains("(function()"));
        assert!(expr.contains("vim.cmd(\"edit \" .. vim.fn.fnameescape("));
        assert!(expr.contains("vim.fn.cursor(12, 7)"));
        assert!(expr.contains("/tmp/it''s.rs"));
        assert!(expr.contains("end)()"));
        assert!(!expr.contains("execute("));
    }

    #[test]
    fn swap_path_expr_uses_buffer_aware_lua_function_expression() {
        let expr = Nvim::swap_path_expr();
        assert!(expr.starts_with("luaeval(\""));
        assert!(expr.contains("(function()"));
        assert!(expr.contains("vim.fn.swapname(vim.fn.bufnr('%'))"));
        assert!(expr.contains("end)()"));
    }

    #[test]
    fn tear_out_source_cleanup_expr_closes_and_deletes_buffer() {
        let expr = Nvim::tear_out_source_cleanup_expr();
        assert!(expr.starts_with("luaeval(\""));
        assert!(expr.contains("vim.fn.bufnr('%')"));
        assert!(expr.contains("vim.cmd('silent! close')"));
        assert!(expr.contains("vim.cmd('silent! bdelete! ' .. bufnr)"));
    }

    #[test]
    fn move_internal_tear_out_rejects_terminal_buffers() {
        let _env_guard = env_guard();
        let harness = NvimHarness::new();
        let app = test_app();

        harness.set_nvim_response(
            "--server /tmp/source.sock --remote-expr winnr()==winnr('l')",
            0,
            "1\n",
            "",
        );
        harness.set_nvim_response(
            "--server /tmp/source.sock --remote-expr json_encode({'path':expand('%:p'),'line':line('.'),'col':col('.'),'buftype':&buftype,'modified':&modified})",
            0,
            r#"{"path":"term://~/project//123:/bin/zsh","line":5,"col":3,"buftype":"terminal","modified":0}"#,
            "",
        );

        let err = app
            .move_internal(Direction::East, 7777)
            .expect_err("terminal buffers should not tear out as file buffers");

        assert!(err.to_string().contains(
            "nvim tear-out requires a file-backed buffer; current buffer type is terminal"
        ));
        let log = harness.nvim_log();
        assert!(!log.contains("smart-splits.mux"));
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
        assert!(nvim_log.contains(&format!(
            "--server /tmp/source.sock --remote-expr {}",
            Nvim::tear_out_source_cleanup_expr()
        )));
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

    #[test]
    fn prepare_merge_captures_file_backed_snapshot() {
        let _env_guard = env_guard();
        let harness = NvimHarness::new();
        let app = test_app();
        StubMuxProvider::reset(Some(77), None);

        harness.set_nvim_response(
            "--server /tmp/source.sock --remote-expr json_encode({'path':expand('%:p'),'line':line('.'),'col':col('.'),'buftype':&buftype,'modified':&modified})",
            0,
            r#"{"path":"/tmp/lib.rs","line":12,"col":7,"buftype":"","modified":0}"#,
            "",
        );
        harness.set_nvim_response(
            &format!(
                "--server /tmp/source.sock --remote-expr {}",
                Nvim::swap_path_expr()
            ),
            0,
            "",
            "",
        );

        let preparation = TopologyHandler::prepare_merge(&app, ProcessId::new(5555))
            .expect("prepare_merge should succeed");
        let payload = preparation
            .into_payload::<NvimMergePayload>()
            .expect("merge payload should exist");

        assert_eq!(payload.snapshot.path, "/tmp/lib.rs");
        assert_eq!(payload.snapshot.line, 12);
        assert_eq!(payload.snapshot.col, 7);
        assert_eq!(payload.source_pane_id, Some(77));
        assert_eq!(payload.source_swap_path, None);
        assert!(!payload.source_is_torn_out);
    }

    #[test]
    fn merge_into_target_opens_buffer_in_target_nvim_split() {
        let _env_guard = env_guard();
        let harness = NvimHarness::new();
        let source = test_app();
        let target = Nvim::for_test("/tmp/target.sock", NvimTerminalMux::Stub);

        harness.set_nvim_response(
            "--server /tmp/source.sock --remote-expr json_encode({'path':expand('%:p'),'line':line('.'),'col':col('.'),'buftype':&buftype,'modified':&modified})",
            0,
            r#"{"path":"/tmp/lib.rs","line":12,"col":7,"buftype":"","modified":0}"#,
            "",
        );
        harness.set_nvim_response(
            &format!(
                "--server /tmp/source.sock --remote-expr {}",
                Nvim::swap_path_expr()
            ),
            0,
            "",
            "",
        );
        harness.set_nvim_response(
            &format!(
                "--server /tmp/target.sock --remote-expr {}",
                Nvim::merge_target_expr(
                    Direction::East,
                    &BufferSnapshot {
                        path: "/tmp/lib.rs".to_string(),
                        line: 12,
                        col: 7,
                        buftype: String::new(),
                        modified: false,
                    }
                )
            ),
            0,
            "1\n",
            "",
        );

        let preparation =
            TopologyHandler::prepare_merge(&source, None).expect("prepare_merge should succeed");
        TopologyHandler::merge_into_target(&target, Direction::East, None, None, preparation)
            .expect("merge_into_target should succeed");

        let log = harness.nvim_log();
        assert!(log.contains("--server /tmp/target.sock --remote-expr"));
        assert!(log.contains("topleft vsplit"));
        assert!(log.contains("edit "));
        assert!(log.contains("vim.fn.cursor(12, 7)"));
    }

    #[test]
    fn merge_into_target_closes_torn_out_source_pane_after_same_terminal_merge() {
        let _env_guard = env_guard();
        let harness = NvimHarness::new();
        let source = Nvim::for_test("/tmp/tearout-123.sock", NvimTerminalMux::Stub);
        let target = Nvim::for_test("/tmp/target.sock", NvimTerminalMux::Stub);
        StubMuxProvider::reset(Some(99), None);
        let swap_path = harness.base.join(".lib.rs.swp");

        harness.set_nvim_response(
            "--server /tmp/tearout-123.sock --remote-expr json_encode({'path':expand('%:p'),'line':line('.'),'col':col('.'),'buftype':&buftype,'modified':&modified})",
            0,
            r#"{"path":"/tmp/lib.rs","line":12,"col":7,"buftype":"","modified":0}"#,
            "",
        );
        harness.set_nvim_response(
            &format!(
                "--server /tmp/tearout-123.sock --remote-expr {}",
                Nvim::swap_path_expr()
            ),
            0,
            &format!("{}\n", swap_path.display()),
            "",
        );
        harness.set_nvim_response(
            &format!(
                "--server /tmp/target.sock --remote-expr {}",
                Nvim::merge_target_expr(
                    Direction::East,
                    &BufferSnapshot {
                        path: "/tmp/lib.rs".to_string(),
                        line: 12,
                        col: 7,
                        buftype: String::new(),
                        modified: false,
                    }
                )
            ),
            0,
            "1\n",
            "",
        );
        harness.set_nvim_response(
            &format!(
                "--server /tmp/tearout-123.sock --remote-expr {}",
                Nvim::quit_expr()
            ),
            0,
            "1\n",
            "",
        );
        harness.set_nvim_response(
            "--server /tmp/tearout-123.sock --remote-expr 1",
            1,
            "",
            "closed",
        );
        fs::write(&swap_path, "").expect("swapfile placeholder should exist");

        let preparation = TopologyHandler::prepare_merge(&source, ProcessId::new(7777))
            .expect("prepare_merge should succeed");
        fs::remove_file(&swap_path).expect("swapfile placeholder should be removable");
        TopologyHandler::merge_into_target(
            &target,
            Direction::East,
            ProcessId::new(7777),
            ProcessId::new(7777),
            preparation,
        )
        .expect("merge_into_target should succeed");

        let log = harness.nvim_log();
        assert!(log.contains("--server /tmp/tearout-123.sock --remote-expr"));
        let quit_pos = log
            .find(&format!(
                "--server /tmp/tearout-123.sock --remote-expr {}",
                Nvim::quit_expr()
            ))
            .expect("quit command should be logged");
        let target_pos = log
            .find("--server /tmp/target.sock --remote-expr")
            .expect("target merge command should be logged");
        assert!(
            quit_pos < target_pos,
            "source should quit before target opens file"
        );
        let send_calls = StubMuxProvider::send_calls();
        assert_eq!(send_calls.len(), 1);
        assert_eq!(send_calls[0].0, 7777);
        assert_eq!(send_calls[0].1, 99);
        assert_eq!(send_calls[0].2, "exit\n");
    }
}
