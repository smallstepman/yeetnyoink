#[cfg(not(unix))]
compile_error!("browser native bridge requires a Unix platform");

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
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
pub enum BrowserInstallTarget {
    Firefox,
    Chromium,
    Chrome,
    Brave,
    Edge,
}

#[derive(Debug, Clone)]
pub struct BrowserInstallReport {
    pub browser: BrowserInstallTarget,
    pub yny_path: PathBuf,
    pub written_paths: Vec<PathBuf>,
    pub next_step_hint: &'static str,
}

impl BrowserInstallTarget {
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "firefox" | "librewolf" => Ok(Self::Firefox),
            "chromium" => Ok(Self::Chromium),
            "chrome" | "google-chrome" => Ok(Self::Chrome),
            "brave" | "brave-browser" => Ok(Self::Brave),
            "edge" | "microsoft-edge" => Ok(Self::Edge),
            other => bail!(
                "unsupported browser install target {other:?}; expected firefox, librewolf, chromium, chrome, brave, or edge"
            ),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Firefox => "Firefox/LibreWolf",
            Self::Chromium => "Chromium",
            Self::Chrome => "Chrome",
            Self::Brave => "Brave",
            Self::Edge => "Edge",
        }
    }

    fn browser_host_mode(self) -> &'static str {
        match self {
            Self::Firefox => "firefox",
            Self::Chromium | Self::Chrome | Self::Brave | Self::Edge => "chromium",
        }
    }

    fn manifest_name(self) -> &'static str {
        match self {
            Self::Firefox => "com.yeet_and_yoink.firefox_bridge.json",
            Self::Chromium | Self::Chrome | Self::Brave | Self::Edge => {
                "com.yeet_and_yoink.chromium_bridge.json"
            }
        }
    }

    fn wrapper_name(self) -> &'static str {
        match self {
            Self::Firefox => "yeet-and-yoink-firefox-host",
            Self::Chromium | Self::Chrome | Self::Brave | Self::Edge => {
                "yeet-and-yoink-chromium-host"
            }
        }
    }

    fn host_name(self) -> &'static str {
        match self {
            Self::Firefox => "com.yeet_and_yoink.firefox_bridge",
            Self::Chromium | Self::Chrome | Self::Brave | Self::Edge => {
                "com.yeet_and_yoink.chromium_bridge"
            }
        }
    }

    fn manifest_description(self) -> &'static str {
        match self {
            Self::Firefox => "Native host for the yeet-and-yoink Firefox bridge",
            Self::Chromium | Self::Chrome | Self::Brave | Self::Edge => {
                "Native host for the yeet-and-yoink Chromium bridge"
            }
        }
    }

    fn manifest_allow_key(self) -> &'static str {
        match self {
            Self::Firefox => "allowed_extensions",
            Self::Chromium | Self::Chrome | Self::Brave | Self::Edge => "allowed_origins",
        }
    }

    fn manifest_allow_values(self) -> &'static [&'static str] {
        match self {
            Self::Firefox => &["browser-bridge@yeet-and-yoink"],
            Self::Chromium | Self::Chrome | Self::Brave | Self::Edge => {
                &["chrome-extension://oigofebnnajpegmncnciacecfhlokkbp/"]
            }
        }
    }

    fn next_step_hint(self) -> &'static str {
        match self {
            Self::Firefox => {
                "Ensure the Yeet and Yoink Browser Bridge add-on is installed/enabled in Firefox or LibreWolf, then restart the browser."
            }
            Self::Chromium | Self::Chrome | Self::Brave | Self::Edge => {
                "Ensure the yeet-and-yoink Chromium-family extension is loaded/enabled in the target browser, then restart the browser."
            }
        }
    }

    fn default_manifest_dirs(self) -> Result<Vec<PathBuf>> {
        let home = home_dir()?;
        match (std::env::consts::OS, self) {
            ("linux", Self::Firefox) => Ok(vec![
                home.join(".mozilla").join("native-messaging-hosts"),
                home.join(".librewolf").join("native-messaging-hosts"),
            ]),
            ("macos", Self::Firefox) => Ok(vec![home
                .join("Library")
                .join("Application Support")
                .join("Mozilla")
                .join("NativeMessagingHosts")]),
            ("linux", Self::Chromium) => Ok(vec![home
                .join(".config")
                .join("chromium")
                .join("NativeMessagingHosts")]),
            ("linux", Self::Chrome) => Ok(vec![home
                .join(".config")
                .join("google-chrome")
                .join("NativeMessagingHosts")]),
            ("linux", Self::Brave) => Ok(vec![home
                .join(".config")
                .join("BraveSoftware")
                .join("Brave-Browser")
                .join("NativeMessagingHosts")]),
            ("linux", Self::Edge) => Ok(vec![home
                .join(".config")
                .join("microsoft-edge")
                .join("NativeMessagingHosts")]),
            ("macos", Self::Chromium) => Ok(vec![home
                .join("Library")
                .join("Application Support")
                .join("Chromium")
                .join("NativeMessagingHosts")]),
            ("macos", Self::Chrome) => Ok(vec![home
                .join("Library")
                .join("Application Support")
                .join("Google")
                .join("Chrome")
                .join("NativeMessagingHosts")]),
            ("macos", Self::Brave) => Ok(vec![home
                .join("Library")
                .join("Application Support")
                .join("BraveSoftware")
                .join("Brave-Browser")
                .join("NativeMessagingHosts")]),
            ("macos", Self::Edge) => Ok(vec![home
                .join("Library")
                .join("Application Support")
                .join("Microsoft Edge")
                .join("NativeMessagingHosts")]),
            _ => bail!(
                "unsupported browser/platform combination for {}; pass --manifest-dir explicitly",
                self.label()
            ),
        }
    }
}

pub fn install_native_host(
    target: BrowserInstallTarget,
    yny_path: &Path,
    manifest_dir: Option<&Path>,
) -> Result<BrowserInstallReport> {
    let yny_path = resolve_install_binary_path(yny_path)?;
    let target_dirs = if let Some(dir) = manifest_dir {
        vec![resolve_output_dir(dir)?]
    } else {
        target.default_manifest_dirs()?
    };

    let mut written_paths = Vec::new();
    for target_dir in target_dirs {
        fs::create_dir_all(&target_dir).with_context(|| {
            format!(
                "failed to create native host directory {}",
                target_dir.display()
            )
        })?;

        let wrapper_path = target_dir.join(target.wrapper_name());
        write_wrapper_script(&wrapper_path, &yny_path, target.browser_host_mode())?;
        written_paths.push(wrapper_path.clone());

        let manifest_path = target_dir.join(target.manifest_name());
        fs::write(
            &manifest_path,
            native_host_manifest_json(target, &wrapper_path)?,
        )
        .with_context(|| {
            format!(
                "failed to write native host manifest {}",
                manifest_path.display()
            )
        })?;
        written_paths.push(manifest_path);
    }

    Ok(BrowserInstallReport {
        browser: target,
        yny_path,
        written_paths,
        next_step_hint: target.next_step_hint(),
    })
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
    let result = (|| -> Result<()> {
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
    })();
    drop(listener);
    let _ = fs::remove_file(&socket_path);
    result
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

    let mut reader = BufReader::new(stream.try_clone().context("failed to clone local stream")?);
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
        let response: HostResponseMessage =
            serde_json::from_slice(&payload).context("failed to parse browser extension reply")?;
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

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .context("HOME is not set; pass --manifest-dir explicitly")
}

fn resolve_install_binary_path(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() {
        bail!("yny path must be absolute; got {}", path.display());
    }
    if !path.exists() {
        bail!("yny path does not exist: {}", path.display());
    }
    Ok(path.to_path_buf())
}

fn resolve_output_dir(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()
            .context("failed to resolve current directory for --manifest-dir")?
            .join(path))
    }
}

fn write_wrapper_script(path: &Path, yny_path: &Path, browser_host_mode: &str) -> Result<()> {
    let script = format!(
        "#!/bin/sh\nexec {} browser-host {}\n",
        shell_single_quote(yny_path),
        browser_host_mode
    );
    fs::write(path, script)
        .with_context(|| format!("failed to write browser host wrapper {}", path.display()))?;
    let mut permissions = fs::metadata(path)
        .with_context(|| format!("failed to stat browser host wrapper {}", path.display()))?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
        .with_context(|| format!("failed to chmod browser host wrapper {}", path.display()))?;
    Ok(())
}

fn shell_single_quote(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let escaped = raw.replace('\'', "'\"'\"'");
    format!("'{escaped}'")
}

fn native_host_manifest_json(target: BrowserInstallTarget, wrapper_path: &Path) -> Result<String> {
    let mut object = serde_json::Map::new();
    object.insert("name".to_string(), serde_json::json!(target.host_name()));
    object.insert(
        "description".to_string(),
        serde_json::json!(target.manifest_description()),
    );
    object.insert(
        "path".to_string(),
        serde_json::json!(wrapper_path.to_string_lossy()),
    );
    object.insert("type".to_string(), serde_json::json!("stdio"));
    object.insert(
        target.manifest_allow_key().to_string(),
        serde_json::json!(target.manifest_allow_values()),
    );

    let mut json = serde_json::to_string_pretty(&serde_json::Value::Object(object))
        .context("failed to encode browser native host manifest")?;
    json.push('\n');
    Ok(json)
}

pub(crate) fn write_native_message(
    writer: &mut dyn Write,
    payload: &impl Serialize,
) -> Result<()> {
    let body = serde_json::to_vec(payload).context("failed to encode browser native message")?;
    let len = u32::try_from(body.len()).context("browser native message was unexpectedly large")?;
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

pub(crate) fn read_native_message(reader: &mut dyn Read) -> Result<Option<Vec<u8>>> {
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
        handle_local_client, install_native_host, BrowserInstallTarget, BrowserTabState,
        HostResponseMessage, HostState, SOCKET_IO_TIMEOUT,
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

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "{}-{}-{}",
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

    #[test]
    fn install_native_host_writes_firefox_manifest_and_wrapper() {
        let temp_dir = unique_temp_dir("yny-setup-firefox");
        std::fs::create_dir_all(&temp_dir).expect("temp manifest dir should exist");
        let yny_path = std::env::current_exe().expect("current test binary path should resolve");

        let report = install_native_host(BrowserInstallTarget::Firefox, &yny_path, Some(&temp_dir))
            .expect("firefox native host install should succeed");

        let wrapper_path = temp_dir.join("yeet-and-yoink-firefox-host");
        let manifest_path = temp_dir.join("com.yeet_and_yoink.firefox_bridge.json");
        assert_eq!(report.browser, BrowserInstallTarget::Firefox);
        assert_eq!(report.yny_path, yny_path);
        assert!(report.written_paths.contains(&wrapper_path));
        assert!(report.written_paths.contains(&manifest_path));
        assert_eq!(
            std::fs::read_to_string(&wrapper_path).expect("wrapper should read"),
            format!(
                "#!/bin/sh\nexec '{}' browser-host firefox\n",
                yny_path.display()
            )
        );

        let manifest: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&manifest_path).expect("manifest should read"),
        )
        .expect("manifest should be valid json");
        assert_eq!(
            manifest,
            json!({
                "name": "com.yeet_and_yoink.firefox_bridge",
                "description": "Native host for the yeet-and-yoink Firefox bridge",
                "path": wrapper_path,
                "type": "stdio",
                "allowed_extensions": ["browser-bridge@yeet-and-yoink"]
            })
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn install_native_host_writes_chromium_manifest_with_allowed_origins() {
        let temp_dir = unique_temp_dir("yny-setup-chromium");
        std::fs::create_dir_all(&temp_dir).expect("temp manifest dir should exist");
        let yny_path = std::env::current_exe().expect("current test binary path should resolve");

        install_native_host(BrowserInstallTarget::Brave, &yny_path, Some(&temp_dir))
            .expect("chromium native host install should succeed");

        let wrapper_path = temp_dir.join("yeet-and-yoink-chromium-host");
        let manifest_path = temp_dir.join("com.yeet_and_yoink.chromium_bridge.json");
        assert_eq!(
            std::fs::read_to_string(&wrapper_path).expect("wrapper should read"),
            format!(
                "#!/bin/sh\nexec '{}' browser-host chromium\n",
                yny_path.display()
            )
        );

        let manifest: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&manifest_path).expect("manifest should read"),
        )
        .expect("manifest should be valid json");
        assert_eq!(
            manifest,
            json!({
                "name": "com.yeet_and_yoink.chromium_bridge",
                "description": "Native host for the yeet-and-yoink Chromium bridge",
                "path": wrapper_path,
                "type": "stdio",
                "allowed_origins": ["chrome-extension://oigofebnnajpegmncnciacecfhlokkbp/"]
            })
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
