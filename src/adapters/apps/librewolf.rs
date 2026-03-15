use anyhow::{Context, Result};

use crate::adapters::apps::AppAdapter;
use crate::engine::contracts::{
    AdapterCapabilities, AppKind, MergeExecutionMode, MergePreparation, MoveDecision, TearResult,
    TopologyHandler,
};
use crate::engine::topology::Direction;

pub const ADAPTER_NAME: &str = "librewolf";
pub const ADAPTER_ALIASES: &[&str] = &["librewolf", "firefox", "librefox"];
pub const APP_IDS: &[&str] = &["librewolf", "LibreWolf", "firefox", "Firefox"];
pub const FIREFOX_EXTENSION_ID: &str = "browser-bridge@yeet-and-yoink.dev";
pub const FIREFOX_NATIVE_HOST_NAME: &str = "com.yeet_and_yoink.firefox_bridge";

const NATIVE_BRIDGE: native_bridge::NativeBrowserDescriptor =
    native_bridge::NativeBrowserDescriptor {
        socket_path_override: crate::config::firefox_native_socket_path,
        socket_basename: "firefox-bridge.sock",
        unavailable_browser_hint:
            "Install/enable the yeet-and-yoink browser extension and keep LibreWolf/Firefox running.",
    };

pub struct Librewolf;

#[derive(Debug, Clone, Copy)]
struct BrowserMergePreparation {
    source_window_id: u64,
    source_tab_id: u64,
}

fn focus_target_index(state: native_bridge::BrowserTabState, dir: Direction) -> Option<usize> {
    match dir {
        Direction::West => state.active_tab_index.checked_sub(1),
        Direction::East => {
            (state.active_tab_index + 1 < state.tab_count).then_some(state.active_tab_index + 1)
        }
        Direction::North | Direction::South => None,
    }
}

fn move_target_index(state: native_bridge::BrowserTabState, dir: Direction) -> Option<usize> {
    match dir {
        Direction::West => {
            if state.active_tab_pinned {
                state.active_tab_index.checked_sub(1)
            } else if state.active_tab_index > state.pinned_tab_count {
                Some(state.active_tab_index - 1)
            } else {
                None
            }
        }
        Direction::East => {
            let upper_bound = if state.active_tab_pinned {
                state.pinned_tab_count
            } else {
                state.tab_count
            };
            (state.active_tab_index + 1 < upper_bound).then_some(state.active_tab_index + 1)
        }
        Direction::North | Direction::South => None,
    }
}

impl Librewolf {
    fn tab_state(&self) -> Result<native_bridge::BrowserTabState> {
        Ok(native_bridge::tab_state(&NATIVE_BRIDGE)?)
    }
}

pub fn run_native_host() -> Result<()> {
    native_bridge::run_native_host(&NATIVE_BRIDGE)
}

impl AppAdapter for Librewolf {
    fn adapter_name(&self) -> &'static str {
        ADAPTER_NAME
    }

    fn config_aliases(&self) -> Option<&'static [&'static str]> {
        Some(ADAPTER_ALIASES)
    }

    fn kind(&self) -> AppKind {
        AppKind::Browser
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            probe: true,
            focus: true,
            move_internal: true,
            resize_internal: false,
            rearrange: false,
            tear_out: true,
            merge: true,
        }
    }
}

impl TopologyHandler for Librewolf {
    fn can_focus(&self, dir: Direction, _pid: u32) -> Result<bool> {
        Ok(focus_target_index(self.tab_state()?, dir).is_some())
    }

    fn window_count(&self, _pid: u32) -> Result<u32> {
        Ok(self.tab_state()?.tab_count as u32)
    }

    fn move_decision(&self, dir: Direction, _pid: u32) -> Result<MoveDecision> {
        let state = self.tab_state()?;
        if state.tab_count <= 1 {
            return Ok(MoveDecision::Passthrough);
        }
        match dir {
            Direction::West | Direction::East => Ok(if move_target_index(state, dir).is_some() {
                MoveDecision::Internal
            } else {
                MoveDecision::TearOut
            }),
            Direction::North | Direction::South => Ok(MoveDecision::TearOut),
        }
    }

    fn focus(&self, dir: Direction, _pid: u32) -> Result<()> {
        let state = self.tab_state()?;
        focus_target_index(state, dir)
            .with_context(|| format!("{ADAPTER_NAME} cannot focus {dir} inside the tab strip"))?;
        Ok(native_bridge::focus(&NATIVE_BRIDGE, dir)?)
    }

    fn move_internal(&self, dir: Direction, _pid: u32) -> Result<()> {
        let state = self.tab_state()?;
        move_target_index(state, dir)
            .with_context(|| format!("{ADAPTER_NAME} cannot move the current tab {dir}"))?;
        Ok(native_bridge::move_tab(&NATIVE_BRIDGE, dir)?)
    }

    fn move_out(&self, _dir: Direction, _pid: u32) -> Result<TearResult> {
        native_bridge::tear_out(&NATIVE_BRIDGE)?;
        Ok(TearResult {
            spawn_command: None,
        })
    }

    fn merge_execution_mode(&self) -> MergeExecutionMode {
        MergeExecutionMode::TargetFocused
    }

    fn prepare_merge(
        &self,
        _source_pid: Option<crate::engine::runtime::ProcessId>,
    ) -> Result<MergePreparation> {
        let state = self.tab_state()?;
        let source_window_id = state
            .window_id
            .context("browser bridge did not report the source window id")?;
        let source_tab_id = state
            .active_tab_id
            .context("browser bridge did not report the active tab id")?;
        Ok(MergePreparation::with_payload(BrowserMergePreparation {
            source_window_id,
            source_tab_id,
        }))
    }

    fn merge_into_target(
        &self,
        dir: Direction,
        _source_pid: Option<crate::engine::runtime::ProcessId>,
        _target_pid: Option<crate::engine::runtime::ProcessId>,
        preparation: MergePreparation,
    ) -> Result<()> {
        let preparation = preparation
            .into_payload::<BrowserMergePreparation>()
            .context("librewolf merge requires source browser window and tab ids")?;
        native_bridge::merge_tab_into_focused_window(
            &NATIVE_BRIDGE,
            dir,
            preparation.source_window_id,
            preparation.source_tab_id,
        )?;
        Ok(())
    }
}

pub(crate) mod native_bridge {
    #[cfg(not(unix))]
    compile_error!("browser native bridge requires a Unix platform");

    use anyhow::{anyhow, bail, Context, Result};
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;
    use std::fs;
    use std::io::{self, BufRead, BufReader, Read, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
    use std::sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    };
    use std::thread;
    use std::time::Duration;

    use crate::engine::topology::Direction;

    const SOCKET_IO_TIMEOUT: Duration = Duration::from_secs(2);
    const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
    const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(10);

    #[derive(Debug, Clone, Copy)]
    pub(crate) struct NativeBrowserDescriptor {
        pub socket_path_override: fn() -> Option<PathBuf>,
        pub socket_basename: &'static str,
        pub unavailable_browser_hint: &'static str,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum BrowserBridgeErrorKind {
        Unavailable,
        Protocol,
        Remote,
    }

    #[derive(Debug)]
    pub(crate) struct BrowserBridgeError {
        #[allow(dead_code)]
        kind: BrowserBridgeErrorKind,
        message: String,
    }

    impl BrowserBridgeError {
        fn new(kind: BrowserBridgeErrorKind, message: impl Into<String>) -> Self {
            Self {
                kind,
                message: message.into(),
            }
        }

        #[allow(dead_code)]
        pub(crate) fn kind(&self) -> BrowserBridgeErrorKind {
            self.kind
        }
    }

    impl std::fmt::Display for BrowserBridgeError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.message)
        }
    }

    impl std::error::Error for BrowserBridgeError {}

    #[derive(Debug, Clone, Copy, Serialize, Deserialize)]
    pub(crate) struct BrowserTabState {
        #[serde(default, rename = "windowId")]
        pub window_id: Option<u64>,
        #[serde(default, rename = "activeTabId")]
        pub active_tab_id: Option<u64>,
        #[serde(rename = "activeTabIndex")]
        pub active_tab_index: usize,
        #[serde(rename = "tabCount")]
        pub tab_count: usize,
        #[serde(rename = "pinnedTabCount")]
        pub pinned_tab_count: usize,
        #[serde(rename = "activeTabPinned")]
        pub active_tab_pinned: bool,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(tag = "command", rename_all = "snake_case")]
    pub(crate) enum BrowserBridgeCommand {
        GetTabState,
        Focus {
            direction: Direction,
        },
        MoveTab {
            direction: Direction,
        },
        TearOut,
        MergeTab {
            source_window_id: u64,
            source_tab_id: u64,
            direction: Direction,
        },
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct HostRequestMessage {
        id: u64,
        #[serde(flatten)]
        command: BrowserBridgeCommand,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct HostResponseMessage {
        id: u64,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        state: Option<BrowserTabState>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct ClientResponseMessage {
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        state: Option<BrowserTabState>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    }

    struct HostState {
        stdout: Mutex<io::Stdout>,
        next_id: AtomicU64,
        pending: Mutex<HashMap<u64, mpsc::Sender<HostResponseMessage>>>,
        running: AtomicBool,
    }

    impl HostState {
        fn new() -> Self {
            Self {
                stdout: Mutex::new(io::stdout()),
                next_id: AtomicU64::new(1),
                pending: Mutex::new(HashMap::new()),
                running: AtomicBool::new(true),
            }
        }

        fn dispatch(&self, command: BrowserBridgeCommand) -> Result<ClientResponseMessage> {
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            let (tx, rx) = mpsc::channel();
            self.pending
                .lock()
                .map_err(|_| anyhow!("browser bridge pending request table was poisoned"))?
                .insert(id, tx);

            let request = HostRequestMessage { id, command };
            if let Err(err) = self.write_request(&request) {
                self.pending
                    .lock()
                    .map_err(|_| anyhow!("browser bridge pending request table was poisoned"))?
                    .remove(&id);
                return Err(err);
            }

            let response = rx
                .recv_timeout(REQUEST_TIMEOUT)
                .context("browser extension did not answer the native bridge request in time")?;
            Ok(ClientResponseMessage {
                ok: response.ok,
                state: response.state,
                error: response.error,
            })
        }

        fn write_request(&self, request: &HostRequestMessage) -> Result<()> {
            let mut stdout = self
                .stdout
                .lock()
                .map_err(|_| anyhow!("browser bridge stdout was poisoned"))?;
            write_native_message(&mut *stdout, request)
        }

        fn handle_response(&self, response: HostResponseMessage) {
            let Some(sender) = self
                .pending
                .lock()
                .ok()
                .and_then(|mut pending| pending.remove(&response.id))
            else {
                return;
            };
            let _ = sender.send(response);
        }

        fn fail_all_pending(&self, message: &str) {
            let mut pending = match self.pending.lock() {
                Ok(pending) => pending,
                Err(_) => return,
            };
            for (id, sender) in pending.drain() {
                let _ = sender.send(HostResponseMessage {
                    id,
                    ok: false,
                    state: None,
                    error: Some(message.to_string()),
                });
            }
        }

        fn stop(&self) {
            self.running.store(false, Ordering::Relaxed);
            self.fail_all_pending("browser extension disconnected from the native bridge");
        }
    }

    pub(crate) fn browser_bridge_socket_path(config: &NativeBrowserDescriptor) -> PathBuf {
        if let Some(value) = (config.socket_path_override)() {
            return value;
        }

        default_socket_root().join(config.socket_basename)
    }

    pub(crate) fn tab_state(
        config: &NativeBrowserDescriptor,
    ) -> std::result::Result<BrowserTabState, BrowserBridgeError> {
        let response = request(config, BrowserBridgeCommand::GetTabState)?;
        response.state.ok_or_else(|| {
            BrowserBridgeError::new(
                BrowserBridgeErrorKind::Protocol,
                "browser bridge get_tab_state response was missing tab state",
            )
        })
    }

    pub(crate) fn focus(
        config: &NativeBrowserDescriptor,
        direction: Direction,
    ) -> std::result::Result<(), BrowserBridgeError> {
        request(config, BrowserBridgeCommand::Focus { direction }).map(|_| ())
    }

    pub(crate) fn move_tab(
        config: &NativeBrowserDescriptor,
        direction: Direction,
    ) -> std::result::Result<(), BrowserBridgeError> {
        request(config, BrowserBridgeCommand::MoveTab { direction }).map(|_| ())
    }

    pub(crate) fn tear_out(
        config: &NativeBrowserDescriptor,
    ) -> std::result::Result<(), BrowserBridgeError> {
        request(config, BrowserBridgeCommand::TearOut).map(|_| ())
    }

    pub(crate) fn merge_tab_into_focused_window(
        config: &NativeBrowserDescriptor,
        direction: Direction,
        source_window_id: u64,
        source_tab_id: u64,
    ) -> std::result::Result<(), BrowserBridgeError> {
        request(
            config,
            BrowserBridgeCommand::MergeTab {
                source_window_id,
                source_tab_id,
                direction,
            },
        )
        .map(|_| ())
    }

    fn request(
        config: &NativeBrowserDescriptor,
        command: BrowserBridgeCommand,
    ) -> std::result::Result<ClientResponseMessage, BrowserBridgeError> {
        let socket_path = browser_bridge_socket_path(config);
        let mut stream = UnixStream::connect(&socket_path).map_err(|err| {
            BrowserBridgeError::new(
                BrowserBridgeErrorKind::Unavailable,
                format!(
                    "browser native bridge is unavailable at {}: {}. {}",
                    socket_path.display(),
                    err,
                    config.unavailable_browser_hint,
                ),
            )
        })?;
        stream
            .set_read_timeout(Some(SOCKET_IO_TIMEOUT))
            .map_err(|err| {
                BrowserBridgeError::new(
                    BrowserBridgeErrorKind::Protocol,
                    format!("failed to configure browser bridge read timeout: {err}"),
                )
            })?;
        stream
            .set_write_timeout(Some(SOCKET_IO_TIMEOUT))
            .map_err(|err| {
                BrowserBridgeError::new(
                    BrowserBridgeErrorKind::Protocol,
                    format!("failed to configure browser bridge write timeout: {err}"),
                )
            })?;

        serde_json::to_writer(&mut stream, &command).map_err(|err| {
            BrowserBridgeError::new(
                BrowserBridgeErrorKind::Protocol,
                format!("failed to serialize browser bridge request: {err}"),
            )
        })?;
        stream.write_all(b"\n").map_err(|err| {
            BrowserBridgeError::new(
                BrowserBridgeErrorKind::Protocol,
                format!("failed to terminate browser bridge request: {err}"),
            )
        })?;
        stream.flush().map_err(|err| {
            BrowserBridgeError::new(
                BrowserBridgeErrorKind::Protocol,
                format!("failed to flush browser bridge request: {err}"),
            )
        })?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        let bytes = reader.read_line(&mut line).map_err(|err| {
            BrowserBridgeError::new(
                BrowserBridgeErrorKind::Protocol,
                format!("failed to read browser bridge response: {err}"),
            )
        })?;
        if bytes == 0 {
            return Err(BrowserBridgeError::new(
                BrowserBridgeErrorKind::Unavailable,
                "browser native bridge closed the socket before replying",
            ));
        }
        let response: ClientResponseMessage = serde_json::from_str(line.trim()).map_err(|err| {
            BrowserBridgeError::new(
                BrowserBridgeErrorKind::Protocol,
                format!("failed to parse browser bridge response: {err}"),
            )
        })?;
        if !response.ok {
            return Err(BrowserBridgeError::new(
                BrowserBridgeErrorKind::Remote,
                response
                    .error
                    .clone()
                    .unwrap_or_else(|| "browser bridge command failed".to_string()),
            ));
        }
        Ok(response)
    }

    pub(crate) fn run_native_host(config: &NativeBrowserDescriptor) -> Result<()> {
        let socket_path = browser_bridge_socket_path(config);
        let listener = bind_socket(&socket_path)?;
        listener
            .set_nonblocking(true)
            .with_context(|| format!("failed to make {} nonblocking", socket_path.display()))?;

        let state = Arc::new(HostState::new());
        let reader_state = Arc::clone(&state);
        let wake_path = socket_path.clone();
        let reader = thread::spawn(move || {
            let stdin = io::stdin();
            let mut stdin = stdin.lock();
            let result = read_extension_loop(&mut stdin, &reader_state);
            reader_state.stop();
            let _ = UnixStream::connect(&wake_path);
            result
        });

        while state.running.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let state = Arc::clone(&state);
                    thread::spawn(move || {
                        let _ = handle_local_client(stream, &state);
                    });
                }
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(ACCEPT_POLL_INTERVAL);
                }
                Err(err) => {
                    state.stop();
                    return Err(err)
                        .with_context(|| format!("failed to accept {}", socket_path.display()));
                }
            }
        }

        match reader.join() {
            Ok(result) => result?,
            Err(_) => bail!("browser native bridge stdin thread panicked"),
        }

        Ok(())
    }

    fn handle_local_client(stream: UnixStream, state: &HostState) -> Result<()> {
        stream
            .set_nonblocking(false)
            .context("failed to make browser bridge local stream blocking")?;
        stream
            .set_read_timeout(Some(SOCKET_IO_TIMEOUT))
            .context("failed to configure browser bridge local read timeout")?;
        stream
            .set_write_timeout(Some(SOCKET_IO_TIMEOUT))
            .context("failed to configure browser bridge local write timeout")?;

        let mut reader =
            BufReader::new(stream.try_clone().context("failed to clone local stream")?);
        let mut line = String::new();
        let bytes = reader
            .read_line(&mut line)
            .context("failed to read local bridge request")?;
        if bytes == 0 {
            bail!("local browser bridge client closed without sending a request");
        }
        let command: BrowserBridgeCommand =
            serde_json::from_str(line.trim()).context("failed to parse local bridge request")?;
        let response = match state.dispatch(command) {
            Ok(response) => response,
            Err(err) => ClientResponseMessage {
                ok: false,
                state: None,
                error: Some(format!("{err:#}")),
            },
        };

        let mut stream = reader.into_inner();
        serde_json::to_writer(&mut stream, &response)
            .context("failed to serialize local bridge response")?;
        stream
            .write_all(b"\n")
            .context("failed to terminate local bridge response")?;
        stream
            .flush()
            .context("failed to flush local bridge response")?;
        Ok(())
    }

    fn read_extension_loop(reader: &mut dyn Read, state: &HostState) -> Result<()> {
        loop {
            let Some(payload) = read_native_message(reader)? else {
                return Ok(());
            };
            let response: HostResponseMessage = serde_json::from_slice(&payload)
                .context("failed to parse browser extension reply")?;
            state.handle_response(response);
        }
    }

    fn bind_socket(path: &Path) -> Result<UnixListener> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create browser native bridge directory {}",
                    parent.display()
                )
            })?;
        }

        if path.exists() {
            match UnixStream::connect(path) {
                Ok(_) => bail!(
                    "browser native bridge socket {} is already active",
                    path.display()
                ),
                Err(_) => fs::remove_file(path).with_context(|| {
                    format!(
                        "failed to remove stale browser native bridge socket {}",
                        path.display()
                    )
                })?,
            }
        }

        let listener = UnixListener::bind(path).with_context(|| {
            format!(
                "failed to bind browser native bridge socket {}",
                path.display()
            )
        })?;
        Ok(listener)
    }

    fn default_socket_root() -> PathBuf {
        if cfg!(target_os = "macos") {
            let home = std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/tmp"));
            home.join("Library")
                .join("Application Support")
                .join("yeet-and-yoink")
        } else {
            std::env::var_os("XDG_RUNTIME_DIR")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join("yeet-and-yoink")
        }
    }

    pub(super) fn write_native_message(
        writer: &mut dyn Write,
        payload: &impl Serialize,
    ) -> Result<()> {
        let body =
            serde_json::to_vec(payload).context("failed to encode browser native message")?;
        let len =
            u32::try_from(body.len()).context("browser native message was unexpectedly large")?;
        writer
            .write_all(&len.to_ne_bytes())
            .context("failed to write browser native message length")?;
        writer
            .write_all(&body)
            .context("failed to write browser native message body")?;
        writer
            .flush()
            .context("failed to flush browser native message")
    }

    pub(super) fn read_native_message(reader: &mut dyn Read) -> Result<Option<Vec<u8>>> {
        let mut len = [0u8; 4];
        match reader.read_exact(&mut len) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(err) => return Err(err).context("failed to read browser native message length"),
        }
        let len = u32::from_ne_bytes(len) as usize;
        let mut payload = vec![0u8; len];
        reader
            .read_exact(&mut payload)
            .context("failed to read browser native message body")?;
        Ok(Some(payload))
    }

    #[cfg(test)]
    mod tests {
        use super::{
            handle_local_client, BrowserTabState, HostResponseMessage, HostState, SOCKET_IO_TIMEOUT,
        };
        use serde_json::json;
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::{UnixListener, UnixStream};
        use std::path::PathBuf;
        use std::sync::Arc;
        use std::thread;
        use std::time::{Duration, SystemTime, UNIX_EPOCH};

        fn unique_socket_path(prefix: &str) -> PathBuf {
            PathBuf::from(format!(
                "/tmp/{}-{}-{}.sock",
                prefix,
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("clock should be monotonic enough for tests")
                    .as_nanos()
            ))
        }

        #[test]
        fn local_client_handler_waits_for_delayed_request_on_nonblocking_listener() {
            let socket_path = unique_socket_path("yny-firefox-local-client");
            let listener =
                UnixListener::bind(&socket_path).expect("local client test socket should bind");
            listener
                .set_nonblocking(true)
                .expect("local client test listener should become nonblocking");

            let state = Arc::new(HostState::new());
            let server_state = Arc::clone(&state);
            let server = thread::spawn(move || {
                let (stream, _) = loop {
                    match listener.accept() {
                        Ok(pair) => break pair,
                        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(err) => panic!("local client test accept failed: {err}"),
                    }
                };
                handle_local_client(stream, &server_state)
            });

            let mut client =
                UnixStream::connect(&socket_path).expect("local client test should connect");
            client
                .set_read_timeout(Some(SOCKET_IO_TIMEOUT))
                .expect("local client test should set read timeout");
            thread::sleep(Duration::from_millis(50));
            client
                .write_all(b"{\"command\":\"get_tab_state\"}\n")
                .expect("local client test request should write");
            client
                .flush()
                .expect("local client test request should flush");

            for _ in 0..50 {
                if state
                    .pending
                    .lock()
                    .expect("pending table mutex poisoned")
                    .contains_key(&1)
                {
                    state.handle_response(HostResponseMessage {
                        id: 1,
                        ok: true,
                        state: Some(BrowserTabState {
                            window_id: None,
                            active_tab_id: None,
                            active_tab_index: 1,
                            tab_count: 3,
                            pinned_tab_count: 0,
                            active_tab_pinned: false,
                        }),
                        error: None,
                    });
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }

            let mut response_line = String::new();
            let bytes = BufReader::new(client)
                .read_line(&mut response_line)
                .expect("local client test response should read");
            assert!(bytes > 0, "local client test should receive a response");
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(response_line.trim())
                    .expect("local client test response should parse"),
                json!({
                    "ok": true,
                    "state": {
                        "windowId": null,
                        "activeTabId": null,
                        "activeTabIndex": 1,
                        "tabCount": 3,
                        "pinnedTabCount": 0,
                        "activeTabPinned": false
                    }
                })
            );

            server
                .join()
                .expect("local client handler thread should join")
                .expect("local client handler should succeed");
            let _ = std::fs::remove_file(&socket_path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        native_bridge, Librewolf, MergeExecutionMode, MoveDecision, TopologyHandler,
        ADAPTER_ALIASES, ADAPTER_NAME, NATIVE_BRIDGE,
    };
    use crate::engine::contracts::AppAdapter;
    use crate::engine::topology::Direction;
    use serde_json::{json, Value};
    use std::collections::VecDeque;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    };
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    struct NativeHarness {
        socket_path: PathBuf,
        log: Arc<Mutex<Vec<Value>>>,
        queue: Arc<Mutex<VecDeque<Value>>>,
        running: Arc<AtomicBool>,
        handle: Option<thread::JoinHandle<()>>,
        old_socket: Option<PathBuf>,
    }

    impl NativeHarness {
        fn new(responses: Vec<Value>) -> Self {
            let socket_path = std::env::temp_dir().join(format!(
                "yny-firefox-bridge-{}-{}.sock",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("clock should be monotonic enough for tests")
                    .as_nanos()
            ));
            let listener = UnixListener::bind(&socket_path)
                .expect("failed to bind fake browser bridge socket");
            listener
                .set_nonblocking(true)
                .expect("failed to make fake bridge listener nonblocking");
            let queue = Arc::new(Mutex::new(VecDeque::from(responses)));
            let log = Arc::new(Mutex::new(Vec::new()));
            let running = Arc::new(AtomicBool::new(true));
            let queue_thread = Arc::clone(&queue);
            let log_thread = Arc::clone(&log);
            let running_thread = Arc::clone(&running);
            let handle = thread::spawn(move || {
                while running_thread.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            // On macOS, accepted sockets may inherit nonblocking from listener
                            stream
                                .set_nonblocking(false)
                                .expect("failed to make accepted stream blocking");
                            handle_native_connection(stream, &queue_thread, &log_thread)
                        }
                        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(err) => panic!("fake bridge listener accept failed: {err}"),
                    }
                }
            });

            let old_socket = crate::config::firefox_native_socket_path();
            crate::config::update(|cfg| {
                cfg.runtime.browser_native.firefox_socket_path = Some(socket_path.clone());
            });

            Self {
                socket_path,
                log,
                queue,
                running,
                handle: Some(handle),
                old_socket,
            }
        }

        fn requests(&self) -> Vec<Value> {
            self.log.lock().expect("log mutex poisoned").clone()
        }
    }

    impl Drop for NativeHarness {
        fn drop(&mut self) {
            self.running.store(false, Ordering::Relaxed);
            let _ = UnixStream::connect(&self.socket_path);
            if let Some(handle) = self.handle.take() {
                handle.join().expect("fake bridge listener should join");
            }
            let _ = std::fs::remove_file(&self.socket_path);
            crate::config::update(|cfg| {
                cfg.runtime.browser_native.firefox_socket_path = self.old_socket.clone();
            });
            assert!(
                self.queue.lock().expect("queue mutex poisoned").is_empty(),
                "all fake browser bridge responses should be consumed"
            );
        }
    }

    fn handle_native_connection(
        mut stream: UnixStream,
        queue: &Arc<Mutex<VecDeque<Value>>>,
        log: &Arc<Mutex<Vec<Value>>>,
    ) {
        stream
            .set_nonblocking(false)
            .expect("local bridge stream should become blocking");
        let mut reader = BufReader::new(stream.try_clone().expect("local stream should clone"));
        let mut line = String::new();
        let bytes = reader
            .read_line(&mut line)
            .expect("local bridge request should be readable");
        assert!(bytes > 0, "local bridge client should send a request");
        let payload = serde_json::from_str::<Value>(line.trim())
            .expect("local bridge request should be json");
        log.lock().expect("log mutex poisoned").push(payload);

        let response = queue
            .lock()
            .expect("queue mutex poisoned")
            .pop_front()
            .expect("unexpected extra browser bridge request");
        serde_json::to_writer(&mut stream, &response)
            .expect("local bridge response should serialize");
        stream
            .write_all(b"\n")
            .expect("local bridge response newline should write");
        stream.flush().expect("local bridge response should flush");
    }

    fn unique_socket_path(prefix: &str) -> PathBuf {
        PathBuf::from(format!(
            "/tmp/{}-{}-{}.sock",
            prefix,
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock should be monotonic enough for tests")
                .as_nanos()
        ))
    }

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::utils::env_guard()
    }

    #[test]
    fn socket_path_uses_config_override() {
        let _guard = env_guard();
        let old = crate::config::snapshot();
        crate::config::update(|cfg| {
            cfg.runtime.browser_native.firefox_socket_path =
                Some(std::path::PathBuf::from("/tmp/yny-firefox-test.sock"));
        });

        assert_eq!(
            native_bridge::browser_bridge_socket_path(&NATIVE_BRIDGE),
            std::path::PathBuf::from("/tmp/yny-firefox-test.sock")
        );

        crate::config::install(old);
    }

    #[test]
    fn native_message_roundtrips() {
        let payload = json!({
            "id": 7,
            "command": "focus",
            "direction": "East",
        });
        let mut bytes = Vec::new();
        native_bridge::write_native_message(&mut bytes, &payload).expect("message should encode");
        let decoded = native_bridge::read_native_message(&mut std::io::Cursor::new(bytes))
            .expect("message should decode")
            .expect("message should exist");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&decoded).expect("json should parse"),
            payload
        );
    }

    #[test]
    fn browser_bridge_command_serializes_direction() {
        let value = serde_json::to_value(native_bridge::BrowserBridgeCommand::MoveTab {
            direction: Direction::East,
        })
        .expect("command should serialize");
        assert_eq!(
            value,
            json!({
                "command": "move_tab",
                "direction": "East",
            })
        );
    }

    #[test]
    fn browser_bridge_merge_command_serializes_payload() {
        let value = serde_json::to_value(native_bridge::BrowserBridgeCommand::MergeTab {
            source_window_id: 17,
            source_tab_id: 23,
            direction: Direction::North,
        })
        .expect("command should serialize");
        assert_eq!(
            value,
            json!({
                "command": "merge_tab",
                "source_window_id": 17,
                "source_tab_id": 23,
                "direction": "North",
            })
        );
    }

    #[test]
    fn fake_native_harness_waits_for_delayed_request_on_nonblocking_listener() {
        let socket_path = unique_socket_path("yny-firefox-fake-harness");
        let listener =
            UnixListener::bind(&socket_path).expect("fake harness test socket should bind");
        listener
            .set_nonblocking(true)
            .expect("fake harness test listener should become nonblocking");
        let queue = Arc::new(Mutex::new(VecDeque::from(vec![json!({
            "ok": true,
            "state": {
                "activeTabIndex": 1,
                "tabCount": 3,
                "pinnedTabCount": 0,
                "activeTabPinned": false
            }
        })])));
        let log = Arc::new(Mutex::new(Vec::new()));
        let queue_thread = Arc::clone(&queue);
        let log_thread = Arc::clone(&log);
        let server = thread::spawn(move || {
            let (stream, _) = loop {
                match listener.accept() {
                    Ok(pair) => break pair,
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(err) => panic!("fake harness test accept failed: {err}"),
                }
            };
            handle_native_connection(stream, &queue_thread, &log_thread);
        });

        let mut client = UnixStream::connect(&socket_path).expect("fake harness client connects");
        thread::sleep(Duration::from_millis(50));
        client
            .write_all(b"{\"command\":\"get_tab_state\"}\n")
            .expect("fake harness request should write");
        client.flush().expect("fake harness request should flush");

        let mut response_line = String::new();
        let bytes = BufReader::new(client)
            .read_line(&mut response_line)
            .expect("fake harness response should read");
        assert!(bytes > 0, "fake harness client should receive a response");
        assert_eq!(
            serde_json::from_str::<Value>(response_line.trim())
                .expect("fake harness response should parse"),
            json!({
                "ok": true,
                "state": {
                    "activeTabIndex": 1,
                    "tabCount": 3,
                    "pinnedTabCount": 0,
                    "activeTabPinned": false
                }
            })
        );
        assert_eq!(
            log.lock().expect("log mutex poisoned").clone(),
            vec![json!({ "command": "get_tab_state" })]
        );

        server.join().expect("fake harness thread should join");
        let _ = std::fs::remove_file(&socket_path);
        assert!(
            queue.lock().expect("queue mutex poisoned").is_empty(),
            "fake harness test should consume queued response"
        );
    }

    #[test]
    fn declares_explicit_capability_contract() {
        let app = Librewolf;
        let caps = AppAdapter::capabilities(&app);
        assert!(caps.probe);
        assert!(caps.focus);
        assert!(caps.move_internal);
        assert!(!caps.resize_internal);
        assert!(caps.tear_out);
        assert!(caps.merge);
        assert!(!caps.rearrange);
        assert_eq!(app.adapter_name(), ADAPTER_NAME);
        assert_eq!(app.config_aliases(), Some(ADAPTER_ALIASES));
    }

    #[test]
    fn can_focus_uses_active_tab_index_via_native_bridge() {
        let _guard = env_guard();
        let harness = NativeHarness::new(vec![
            json!({
                "ok": true,
                "state": {
                    "activeTabIndex": 0,
                    "tabCount": 3,
                    "pinnedTabCount": 0,
                    "activeTabPinned": false
                }
            }),
            json!({
                "ok": true,
                "state": {
                    "activeTabIndex": 1,
                    "tabCount": 3,
                    "pinnedTabCount": 0,
                    "activeTabPinned": false
                }
            }),
        ]);
        let app = Librewolf;

        assert!(!app
            .can_focus(Direction::West, 0)
            .expect("west focus probe should succeed"));
        assert!(app
            .can_focus(Direction::East, 0)
            .expect("east focus probe should succeed"));
        assert_eq!(
            harness.requests(),
            vec![
                json!({ "command": "get_tab_state" }),
                json!({ "command": "get_tab_state" }),
            ]
        );
    }

    #[test]
    fn window_count_comes_from_native_bridge_state() {
        let _guard = env_guard();
        let _harness = NativeHarness::new(vec![json!({
            "ok": true,
            "state": {
                "activeTabIndex": 1,
                "tabCount": 4,
                "pinnedTabCount": 1,
                "activeTabPinned": false
            }
        })]);
        let app = Librewolf;

        assert_eq!(app.window_count(0).expect("window_count should succeed"), 4);
    }

    #[test]
    fn move_decision_tears_out_at_pinned_boundary() {
        let _guard = env_guard();
        let _harness = NativeHarness::new(vec![json!({
            "ok": true,
            "state": {
                "activeTabIndex": 2,
                "tabCount": 5,
                "pinnedTabCount": 2,
                "activeTabPinned": false
            }
        })]);
        let app = Librewolf;

        let decision = app
            .move_decision(Direction::West, 0)
            .expect("move_decision should succeed");
        assert!(matches!(decision, MoveDecision::TearOut));
    }

    #[test]
    fn move_decision_tears_out_vertically_when_multiple_tabs_exist() {
        let _guard = env_guard();
        let _harness = NativeHarness::new(vec![json!({
            "ok": true,
            "state": {
                "activeTabIndex": 1,
                "tabCount": 3,
                "pinnedTabCount": 0,
                "activeTabPinned": false
            }
        })]);
        let app = Librewolf;

        let decision = app
            .move_decision(Direction::North, 0)
            .expect("vertical move_decision should succeed");
        assert!(matches!(decision, MoveDecision::TearOut));
    }

    #[test]
    fn focus_moves_to_adjacent_tab_via_native_bridge() {
        let _guard = env_guard();
        let harness = NativeHarness::new(vec![
            json!({
                "ok": true,
                "state": {
                    "activeTabIndex": 1,
                    "tabCount": 3,
                    "pinnedTabCount": 0,
                    "activeTabPinned": false
                }
            }),
            json!({ "ok": true }),
        ]);
        let app = Librewolf;

        app.focus(Direction::East, 0)
            .expect("focus east should succeed through native bridge");
        assert_eq!(
            harness.requests(),
            vec![
                json!({ "command": "get_tab_state" }),
                json!({
                    "command": "focus",
                    "direction": "East",
                }),
            ]
        );
    }

    #[test]
    fn move_internal_moves_current_tab_via_native_bridge() {
        let _guard = env_guard();
        let harness = NativeHarness::new(vec![
            json!({
                "ok": true,
                "state": {
                    "activeTabIndex": 2,
                    "tabCount": 5,
                    "pinnedTabCount": 1,
                    "activeTabPinned": false
                }
            }),
            json!({ "ok": true }),
        ]);
        let app = Librewolf;

        app.move_internal(Direction::East, 0)
            .expect("move east should succeed through native bridge");
        assert_eq!(
            harness.requests(),
            vec![
                json!({ "command": "get_tab_state" }),
                json!({
                    "command": "move_tab",
                    "direction": "East",
                }),
            ]
        );
    }

    #[test]
    fn tear_out_returns_no_spawn_command() {
        let _guard = env_guard();
        let harness = NativeHarness::new(vec![json!({"ok":true})]);
        let app = Librewolf;

        let result = app
            .move_out(Direction::East, 0)
            .expect("move_out should succeed through native bridge");
        assert!(result.spawn_command.is_none());
        assert_eq!(harness.requests(), vec![json!({"command":"tear_out"})]);
    }

    #[test]
    fn merge_moves_torn_out_tab_into_focused_target_window() {
        let _guard = env_guard();
        let harness = NativeHarness::new(vec![
            json!({
                "ok": true,
                "state": {
                    "windowId": 8,
                    "activeTabId": 42,
                    "activeTabIndex": 0,
                    "tabCount": 1,
                    "pinnedTabCount": 0,
                    "activeTabPinned": false
                }
            }),
            json!({ "ok": true }),
        ]);
        let app = Librewolf;

        assert!(matches!(
            TopologyHandler::merge_execution_mode(&app),
            MergeExecutionMode::TargetFocused
        ));
        let preparation = app
            .prepare_merge(None)
            .expect("prepare_merge should capture source browser tab ids");
        app.merge_into_target(Direction::North, None, None, preparation)
            .expect("merge_into_target should succeed");
        assert_eq!(
            harness.requests(),
            vec![
                json!({ "command": "get_tab_state" }),
                json!({
                    "command": "merge_tab",
                    "source_window_id": 8,
                    "source_tab_id": 42,
                    "direction": "North",
                }),
            ]
        );
    }
}
