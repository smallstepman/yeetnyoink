use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::ErrorKind;
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tungstenite::{client::client, Message, WebSocket};

use crate::adapters::apps::AppAdapter;
use crate::config::EditorTearOffScope;
use crate::engine::contract::{
    AdapterCapabilities, AppKind, MergeExecutionMode, MergePreparation, MoveDecision, TearResult,
    TopologyHandler,
};
use crate::engine::runtime::{self, process_tree_pids, ProcessId};
use crate::engine::topology::Direction;
use crate::logging;

const ADAPTER_NAME: &str = "vscode";
const DEFAULT_REMOTE_CONTROL_HOST: &str = "127.0.0.1";
const DEFAULT_REMOTE_CONTROL_PORT: u16 = 3710;
const ADAPTER_ALIASES: &[&str] = &["vscode"];
const REMOTE_CONTROL_HOST_ENV: &str = "NIRI_DEEP_VSCODE_REMOTE_CONTROL_HOST";
const REMOTE_CONTROL_PORT_ENV: &str = "NIRI_DEEP_VSCODE_REMOTE_CONTROL_PORT";
const STATE_FILE_ENV: &str = "NIRI_DEEP_VSCODE_STATE_FILE";
const FOCUS_SETTLE_ENV: &str = "NIRI_DEEP_VSCODE_FOCUS_SETTLE_MS";
const PROCESS_REMOTE_CONTROL_PORT_ENV: &str = "REMOTE_CONTROL_PORT";
const TEST_CLIPBOARD_FILE_ENV: &str = "NIRI_DEEP_VSCODE_TEST_CLIPBOARD_FILE";
const REMOTE_CONTROL_TIMEOUT: Duration = Duration::from_secs(1);
const DEFAULT_FOCUS_SETTLE_DELAY: Duration = Duration::from_millis(50);
const RESTORE_SETTLE_DELAY: Duration = Duration::from_millis(50);
const CLIPBOARD_SETTLE_DELAY: Duration = Duration::from_millis(20);
const MAX_CAPTURED_GROUP_TABS: usize = 64;
const GEOMETRY_EPSILON: f64 = 0.000_001;
const MISSING_ACTIVE_FILE_PATH_MESSAGE: &str =
    "VS Code did not expose an active file path for merge preparation";
const GET_EDITOR_LAYOUT_COMMAND: &str = "vscode.getEditorLayout";
const MOVE_EDITOR_TO_NEW_WINDOW_COMMAND: &str = "workbench.action.moveEditorToNewWindow";
const MOVE_EDITOR_GROUP_TO_NEW_WINDOW_COMMAND: &str = "workbench.action.moveEditorGroupToNewWindow";
const RESTORE_EDITORS_TO_MAIN_WINDOW_COMMAND: &str = "workbench.action.restoreEditorsToMainWindow";
const COPY_ACTIVE_FILE_PATH_COMMAND: &str = "copyFilePath";
const CLOSE_ACTIVE_EDITOR_COMMAND: &str = "workbench.action.closeActiveEditor";
const CLOSE_OTHER_EDITORS_IN_GROUP_COMMAND: &str = "workbench.action.closeOtherEditors";
const KEEP_EDITOR_COMMAND: &str = "workbench.action.keepEditor";
const OPEN_EDITOR_AT_INDEX_COMMAND: &str = "workbench.action.openEditorAtIndex";
const VSCODE_OPEN_COMMAND: &str = "vscode.open";
const FOCUS_ACTIVE_EDITOR_GROUP_COMMAND: &str = "workbench.action.focusActiveEditorGroup";
const FOCUS_SIDEBAR_COMMAND: &str = "workbench.action.focusSideBar";
const FOCUS_TERMINAL_COMMAND: &str = "workbench.action.terminal.focus";
const FOCUS_PREVIOUS_TERMINAL_COMMAND: &str = "workbench.action.terminal.focusPrevious";
const FOCUS_NEXT_TERMINAL_COMMAND: &str = "workbench.action.terminal.focusNext";
const MOVE_TERMINAL_TO_NEW_WINDOW_COMMAND: &str = "workbench.action.terminal.moveIntoNewWindow";
const MOVE_TERMINAL_TO_PANEL_COMMAND: &str = "workbench.action.terminal.moveToTerminalPanel";

/// VS Code / Code OSS integration via the vscode-remote-control websocket bridge.
///
/// VS Code exposes the editor split tree through `vscode.getEditorLayout`, but does not expose
/// the active group through any command we can call over the bridge. To make edge handling useful
/// anyway, we persist a small per-window focus model and narrow the set of possible active editor
/// leaves whenever directional focus succeeds.
pub struct Vscode;

#[derive(Debug, Serialize)]
struct VscodeRemoteRequest<'a> {
    command: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    args: Option<Vec<Value>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GroupOrientation {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EditorLayout {
    #[serde(default)]
    orientation: Option<u8>,
    #[serde(default)]
    groups: Vec<EditorLayoutNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EditorLayoutNode {
    #[serde(default)]
    size: Option<f64>,
    #[serde(default)]
    groups: Vec<EditorLayoutNode>,
}

#[derive(Debug, Clone, Copy)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

#[derive(Debug, Clone)]
struct LayoutSnapshot {
    layout: EditorLayout,
    leaves: Vec<Rect>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
enum FocusSurface {
    #[default]
    Editors,
    SideBar,
    Terminal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VscodeWindowState {
    layout_signature: String,
    possible_leaves: Vec<usize>,
    surface: FocusSurface,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pending: Option<PendingFocusState>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct VscodeStateStore {
    windows: HashMap<String, VscodeWindowState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingFocusState {
    possible_leaves: Vec<usize>,
    surface: FocusSurface,
    settle_until_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FocusModel {
    possible_leaves: Vec<usize>,
    surface: FocusSurface,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
enum VscodeMergePreparation {
    Buffer {
        path: String,
    },
    Group {
        paths: Vec<String>,
        active_index: usize,
    },
    Terminal,
    RestoreMainWindow {
        scope: EditorTearOffScope,
    },
}

impl GroupOrientation {
    fn from_wire(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Horizontal),
            1 => Some(Self::Vertical),
            _ => None,
        }
    }

    fn child(self) -> Self {
        match self {
            Self::Horizontal => Self::Vertical,
            Self::Vertical => Self::Horizontal,
        }
    }
}

impl EditorLayout {
    fn leaf_count(&self) -> u32 {
        if self.groups.is_empty() {
            1
        } else {
            self.groups.iter().map(EditorLayoutNode::leaf_count).sum()
        }
    }
}

impl EditorLayoutNode {
    fn leaf_count(&self) -> u32 {
        if self.groups.is_empty() {
            1
        } else {
            self.groups.iter().map(Self::leaf_count).sum()
        }
    }
}

impl Rect {
    fn horizontal_overlap(self, other: Self) -> f64 {
        range_overlap(self.x, self.x + self.w, other.x, other.x + other.w)
    }

    fn vertical_overlap(self, other: Self) -> f64 {
        range_overlap(self.y, self.y + self.h, other.y, other.y + other.h)
    }
}

impl LayoutSnapshot {
    fn new(layout: EditorLayout) -> Self {
        let mut leaves = Vec::new();
        if layout.groups.is_empty() {
            leaves.push(Rect {
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0,
            });
        } else if let Some(root_orientation) =
            layout.orientation.and_then(GroupOrientation::from_wire)
        {
            Self::collect_leaf_rects(
                &layout.groups,
                root_orientation,
                Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 1.0,
                    h: 1.0,
                },
                &mut leaves,
            );
        } else {
            leaves.resize(
                layout.leaf_count() as usize,
                Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 1.0,
                    h: 1.0,
                },
            );
        }
        Self { layout, leaves }
    }

    fn collect_leaf_rects(
        nodes: &[EditorLayoutNode],
        orientation: GroupOrientation,
        bounds: Rect,
        leaves: &mut Vec<Rect>,
    ) {
        if nodes.is_empty() {
            leaves.push(bounds);
            return;
        }

        let mut weights: Vec<f64> = nodes
            .iter()
            .map(|node| node.size.filter(|value| *value > 0.0).unwrap_or(1.0))
            .collect();
        let total: f64 = weights.iter().sum();
        if total <= GEOMETRY_EPSILON {
            weights.fill(1.0);
        }
        let total: f64 = weights.iter().sum();

        let mut offset = 0.0;
        for (node, weight) in nodes.iter().zip(weights.into_iter()) {
            let fraction = if total <= GEOMETRY_EPSILON {
                1.0 / nodes.len() as f64
            } else {
                weight / total
            };
            let child = match orientation {
                GroupOrientation::Horizontal => Rect {
                    x: bounds.x + bounds.w * offset,
                    y: bounds.y,
                    w: bounds.w * fraction,
                    h: bounds.h,
                },
                GroupOrientation::Vertical => Rect {
                    x: bounds.x,
                    y: bounds.y + bounds.h * offset,
                    w: bounds.w,
                    h: bounds.h * fraction,
                },
            };
            offset += fraction;

            if node.groups.is_empty() {
                leaves.push(child);
            } else {
                Self::collect_leaf_rects(&node.groups, orientation.child(), child, leaves);
            }
        }
    }

    fn leaf_count(&self) -> usize {
        self.leaves.len().max(1)
    }

    fn layout_signature(&self) -> String {
        serde_json::to_string(&self.layout)
            .unwrap_or_else(|_| format!("leaf-count:{}", self.layout.leaf_count()))
    }

    fn leaf_indices(&self) -> Vec<usize> {
        (0..self.leaf_count()).collect()
    }

    fn has_any_neighbor(&self, leaves: &[usize], dir: Direction) -> bool {
        leaves
            .iter()
            .copied()
            .any(|leaf| self.neighbor_for_leaf(leaf, dir).is_some())
    }

    fn all_have_neighbor(&self, leaves: &[usize], dir: Direction) -> bool {
        !leaves.is_empty()
            && leaves
                .iter()
                .copied()
                .all(|leaf| self.neighbor_for_leaf(leaf, dir).is_some())
    }

    fn apply_focus_transition(&self, leaves: &[usize], dir: Direction) -> Vec<usize> {
        let mut result: Vec<usize> = leaves
            .iter()
            .copied()
            .map(|leaf| self.neighbor_for_leaf(leaf, dir).unwrap_or(leaf))
            .collect();
        result.sort_unstable();
        result.dedup();
        if result.is_empty() {
            self.leaf_indices()
        } else {
            result
        }
    }

    fn neighbor_for_leaf(&self, source_index: usize, dir: Direction) -> Option<usize> {
        let source = *self.leaves.get(source_index)?;
        let mut best: Option<(f64, f64, usize)> = None;

        for (candidate_index, candidate) in self.leaves.iter().copied().enumerate() {
            if candidate_index == source_index {
                continue;
            }

            let (distance, overlap) = match dir {
                Direction::West => (
                    source.x - (candidate.x + candidate.w),
                    source.vertical_overlap(candidate),
                ),
                Direction::East => (
                    candidate.x - (source.x + source.w),
                    source.vertical_overlap(candidate),
                ),
                Direction::North => (
                    source.y - (candidate.y + candidate.h),
                    source.horizontal_overlap(candidate),
                ),
                Direction::South => (
                    candidate.y - (source.y + source.h),
                    source.horizontal_overlap(candidate),
                ),
            };

            if overlap <= GEOMETRY_EPSILON || distance < -GEOMETRY_EPSILON {
                continue;
            }

            let score = (distance.max(0.0), -overlap, candidate_index);
            if best.map_or(true, |current| score < current) {
                best = Some(score);
            }
        }

        best.map(|(_, _, index)| index)
    }
}

fn range_overlap(a_start: f64, a_end: f64, b_start: f64, b_end: f64) -> f64 {
    (a_end.min(b_end) - a_start.max(b_start)).max(0.0)
}

impl VscodeWindowState {
    fn default_for(snapshot: &LayoutSnapshot) -> Self {
        Self {
            layout_signature: snapshot.layout_signature(),
            possible_leaves: snapshot.leaf_indices(),
            surface: FocusSurface::Editors,
            pending: None,
        }
    }
}

impl FocusModel {
    fn new(surface: FocusSurface, possible_leaves: Vec<usize>) -> Self {
        Self {
            surface,
            possible_leaves,
        }
    }
}

impl Vscode {
    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    fn remote_control_host() -> String {
        std::env::var(REMOTE_CONTROL_HOST_ENV)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_REMOTE_CONTROL_HOST.to_string())
    }

    fn focus_settle_delay() -> Duration {
        std::env::var(FOCUS_SETTLE_ENV)
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .map(Duration::from_millis)
            .unwrap_or(DEFAULT_FOCUS_SETTLE_DELAY)
    }

    fn parse_port(value: &str, source: &str) -> Result<u16> {
        let port: u16 = value
            .trim()
            .parse()
            .with_context(|| format!("invalid {source} value {value:?}"))?;
        if port == 0 {
            bail!("{source} must be greater than zero");
        }
        Ok(port)
    }

    fn port_cache() -> &'static Mutex<HashMap<u32, u16>> {
        static CACHE: OnceLock<Mutex<HashMap<u32, u16>>> = OnceLock::new();
        CACHE.get_or_init(|| Mutex::new(HashMap::new()))
    }

    fn cached_port(pid: u32) -> Option<u16> {
        Self::port_cache()
            .lock()
            .ok()
            .and_then(|cache| cache.get(&pid).copied())
    }

    fn remember_port(pid: u32, port: u16) {
        if let Ok(mut cache) = Self::port_cache().lock() {
            cache.insert(pid, port);
        }
    }

    fn push_unique_port(ports: &mut Vec<u16>, port: u16) {
        if !ports.contains(&port) {
            ports.push(port);
        }
    }

    fn candidate_ports(pid: u32) -> Result<Vec<u16>> {
        let _span = tracing::debug_span!("vscode.candidate_ports", pid = pid).entered();
        if let Ok(value) = std::env::var(REMOTE_CONTROL_PORT_ENV) {
            let value = value.trim();
            if !value.is_empty() {
                return Ok(vec![Self::parse_port(value, REMOTE_CONTROL_PORT_ENV)?]);
            }
        }

        let mut ports = Vec::new();
        if let Some(port) = Self::cached_port(pid) {
            Self::push_unique_port(&mut ports, port);
        }

        for candidate_pid in process_tree_pids(pid) {
            if let Some(value) =
                runtime::process_environ_var(candidate_pid, PROCESS_REMOTE_CONTROL_PORT_ENV)
            {
                match Self::parse_port(&value, PROCESS_REMOTE_CONTROL_PORT_ENV) {
                    Ok(port) => Self::push_unique_port(&mut ports, port),
                    Err(err) => logging::debug(format!(
                        "vscode: ignoring invalid {} from pid {} err={:#}",
                        PROCESS_REMOTE_CONTROL_PORT_ENV, candidate_pid, err
                    )),
                }
            }
        }

        for port in Self::listener_ports_for_process_tree(pid) {
            Self::push_unique_port(&mut ports, port);
        }

        Self::push_unique_port(&mut ports, DEFAULT_REMOTE_CONTROL_PORT);
        Ok(ports)
    }

    fn listener_ports_for_process_tree(pid: u32) -> Vec<u16> {
        let _span =
            tracing::debug_span!("vscode.listener_ports_for_process_tree", pid = pid).entered();
        let mut inodes = HashSet::new();
        for candidate_pid in process_tree_pids(pid) {
            inodes.extend(Self::socket_inodes_for_pid(candidate_pid));
        }
        if inodes.is_empty() {
            return Vec::new();
        }

        let mut ports = Vec::new();
        for proc_file in ["/proc/net/tcp", "/proc/net/tcp6"] {
            let Ok(contents) = fs::read_to_string(proc_file) else {
                continue;
            };
            for port in Self::parse_listening_tcp_ports(&contents, &inodes) {
                Self::push_unique_port(&mut ports, port);
            }
        }
        ports
    }

    fn socket_inodes_for_pid(pid: u32) -> HashSet<u64> {
        let mut inodes = HashSet::new();
        let Ok(entries) = fs::read_dir(format!("/proc/{pid}/fd")) else {
            return inodes;
        };

        for entry in entries.flatten() {
            let Ok(target) = fs::read_link(entry.path()) else {
                continue;
            };
            if let Some(inode) = Self::parse_socket_inode(&target.to_string_lossy()) {
                inodes.insert(inode);
            }
        }
        inodes
    }

    fn parse_socket_inode(target: &str) -> Option<u64> {
        let inner = target
            .trim()
            .strip_prefix("socket:[")?
            .strip_suffix(']')?
            .trim();
        inner.parse().ok()
    }

    fn parse_listening_tcp_ports(contents: &str, inodes: &HashSet<u64>) -> Vec<u16> {
        let mut ports = Vec::new();
        for line in contents.lines().skip(1) {
            let columns: Vec<&str> = line.split_whitespace().collect();
            if columns.len() <= 9 || columns[3] != "0A" {
                continue;
            }

            let Ok(inode) = columns[9].parse::<u64>() else {
                continue;
            };
            if !inodes.contains(&inode) {
                continue;
            }

            let Some(port_hex) = columns[1].rsplit(':').next() else {
                continue;
            };
            let Ok(port) = u16::from_str_radix(port_hex, 16) else {
                continue;
            };
            Self::push_unique_port(&mut ports, port);
        }
        ports
    }

    fn build_ws_url(host: &str, port: u16) -> String {
        format!("ws://{host}:{port}")
    }

    fn serialize_request_payload(command: &str, args: &[Value]) -> Result<String> {
        serde_json::to_string(&VscodeRemoteRequest {
            command,
            args: (!args.is_empty()).then(|| args.to_vec()),
        })
        .context("failed to serialize VS Code remote-control request")
    }

    fn connect_socket(host: &str, port: u16, url: &str) -> Result<WebSocket<TcpStream>> {
        let stream = TcpStream::connect((host, port))
            .with_context(|| format!("failed to connect to VS Code remote-control at {url}"))?;
        stream
            .set_read_timeout(Some(REMOTE_CONTROL_TIMEOUT))
            .with_context(|| format!("failed to set VS Code read timeout for {url}"))?;
        stream
            .set_write_timeout(Some(REMOTE_CONTROL_TIMEOUT))
            .with_context(|| format!("failed to set VS Code write timeout for {url}"))?;

        let (socket, _) = client(url, stream)
            .with_context(|| format!("failed to open VS Code websocket at {url}"))?;
        Ok(socket)
    }

    fn parse_response_value(stdout: &str) -> Result<Value> {
        let stdout = stdout.trim();
        if stdout.is_empty() {
            return Ok(Value::Null);
        }
        if let Ok(value) = serde_json::from_str(stdout) {
            return Ok(value);
        }
        for line in stdout.lines().rev() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(value) = serde_json::from_str(line) {
                return Ok(value);
            }
        }
        bail!("invalid VS Code remote response json: {stdout}");
    }

    fn read_response(socket: &mut WebSocket<TcpStream>, url: &str, command: &str) -> Result<Value> {
        loop {
            match socket.read() {
                Ok(Message::Text(text)) => return Self::parse_response_value(text.as_ref()),
                Ok(Message::Binary(bytes)) => {
                    let text = String::from_utf8_lossy(&bytes);
                    return Self::parse_response_value(&text);
                }
                Ok(Message::Close(_)) => return Ok(Value::Null),
                Ok(Message::Ping(payload)) => {
                    socket.send(Message::Pong(payload)).with_context(|| {
                        format!("failed to reply to VS Code ping for {command}")
                    })?;
                }
                Ok(Message::Pong(_)) | Ok(Message::Frame(_)) => {}
                Err(tungstenite::Error::Io(err))
                    if matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                {
                    return Ok(Value::Null);
                }
                Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                    return Ok(Value::Null);
                }
                Err(err) => {
                    bail!("VS Code remote-control command `{command}` failed via {url}: {err}")
                }
            }
        }
    }

    fn send_request_to_url(host: &str, port: u16, command: &str, args: &[Value]) -> Result<Value> {
        let _span = tracing::debug_span!(
            "vscode.send_request_to_url",
            host = host,
            port = port,
            command = command,
            arg_count = args.len()
        )
        .entered();
        let url = Self::build_ws_url(host, port);
        let payload = Self::serialize_request_payload(command, args)?;

        logging::debug(format!(
            "vscode: url={} command={} payload={}",
            url, command, payload
        ));

        let mut socket = Self::connect_socket(host, port, &url)?;
        socket
            .send(Message::Text(payload.into()))
            .with_context(|| {
                format!("failed to send VS Code remote-control command `{command}`")
            })?;

        let response = Self::read_response(&mut socket, &url, command);
        let _ = socket.close(None);
        response
    }

    fn send_command_to_url(host: &str, port: u16, command: &str, args: &[Value]) -> Result<()> {
        let _span = tracing::debug_span!(
            "vscode.send_command_to_url",
            host = host,
            port = port,
            command = command,
            arg_count = args.len()
        )
        .entered();
        let url = Self::build_ws_url(host, port);
        let payload = Self::serialize_request_payload(command, args)?;

        logging::debug(format!(
            "vscode: url={} command={} payload={}",
            url, command, payload
        ));

        let mut socket = Self::connect_socket(host, port, &url)?;
        socket
            .send(Message::Text(payload.into()))
            .with_context(|| {
                format!("failed to send VS Code remote-control command `{command}`")
            })?;
        let _ = socket.close(None);
        Ok(())
    }

    fn request_value(pid: u32, command: &str, args: &[Value]) -> Result<Value> {
        let _span = tracing::debug_span!(
            "vscode.request_value",
            pid = pid,
            command = command,
            arg_count = args.len()
        )
        .entered();
        let host = Self::remote_control_host();
        let ports = Self::candidate_ports(pid)?;
        logging::debug(format!(
            "vscode: pid={} command={} candidate_ports={:?}",
            pid, command, ports
        ));

        let mut errors = Vec::new();
        for port in ports {
            match Self::send_request_to_url(&host, port, command, args) {
                Ok(value) => {
                    Self::remember_port(pid, port);
                    return Ok(value);
                }
                Err(err) => errors.push(format!("{}: {err:#}", Self::build_ws_url(&host, port))),
            }
        }

        bail!(
            "unable to run VS Code command `{command}` for pid {pid}; {}",
            errors.join("; ")
        );
    }

    fn layout_for_pid(pid: u32) -> Result<EditorLayout> {
        let _span = tracing::debug_span!("vscode.layout_for_pid", pid = pid).entered();
        let value = Self::request_value(pid, GET_EDITOR_LAYOUT_COMMAND, &[])?;
        serde_json::from_value::<EditorLayout>(value)
            .context("invalid VS Code editor layout response")
    }

    fn snapshot_for_pid(pid: u32) -> Result<LayoutSnapshot> {
        let _span = tracing::debug_span!("vscode.snapshot_for_pid", pid = pid).entered();
        Ok(LayoutSnapshot::new(Self::layout_for_pid(pid)?))
    }

    fn run_command(pid: u32, command: &str) -> Result<()> {
        let _span =
            tracing::debug_span!("vscode.run_command", pid = pid, command = command).entered();
        Self::run_command_with_args(pid, command, &[])
    }

    fn run_command_with_args(pid: u32, command: &str, args: &[Value]) -> Result<()> {
        let _span = tracing::debug_span!(
            "vscode.run_command_with_args",
            pid = pid,
            command = command,
            arg_count = args.len()
        )
        .entered();
        let host = Self::remote_control_host();
        let ports = Self::candidate_ports(pid)?;
        logging::debug(format!(
            "vscode: pid={} command={} args={} candidate_ports={:?}",
            pid,
            command,
            serde_json::to_string(args).unwrap_or_else(|_| "<args>".to_string()),
            ports
        ));

        let mut errors = Vec::new();
        for port in ports {
            match Self::send_command_to_url(&host, port, command, args) {
                Ok(()) => {
                    Self::remember_port(pid, port);
                    return Ok(());
                }
                Err(err) => errors.push(format!("{}: {err:#}", Self::build_ws_url(&host, port))),
            }
        }

        bail!(
            "unable to run VS Code command `{command}` for pid {pid}; {}",
            errors.join("; ")
        );
    }

    fn focus_command(dir: Direction) -> &'static str {
        dir.select(
            "workbench.action.focusLeftGroupWithoutWrap",
            "workbench.action.focusRightGroupWithoutWrap",
            "workbench.action.focusAboveGroupWithoutWrap",
            "workbench.action.focusBelowGroupWithoutWrap",
        )
    }

    fn move_command(dir: Direction) -> &'static str {
        dir.select(
            "workbench.action.moveEditorToLeftGroup",
            "workbench.action.moveEditorToRightGroup",
            "workbench.action.moveEditorToAboveGroup",
            "workbench.action.moveEditorToBelowGroup",
        )
    }

    fn split_command(dir: Direction) -> &'static str {
        dir.select(
            "workbench.action.splitEditorLeft",
            "workbench.action.splitEditorRight",
            "workbench.action.splitEditorUp",
            "workbench.action.splitEditorDown",
        )
    }

    fn tear_off_scope() -> EditorTearOffScope {
        crate::config::editor_tear_off_scope_for(ADAPTER_ALIASES)
    }

    fn manage_terminal() -> bool {
        crate::config::editor_manage_terminal_for(ADAPTER_ALIASES)
    }

    fn workbench_surface_for_direction(dir: Direction) -> Option<FocusSurface> {
        match dir {
            Direction::West => Some(FocusSurface::SideBar),
            Direction::South => Some(FocusSurface::Terminal),
            Direction::East | Direction::North => None,
        }
    }

    fn workbench_focus_command(surface: FocusSurface) -> &'static str {
        match surface {
            FocusSurface::Editors => FOCUS_ACTIVE_EDITOR_GROUP_COMMAND,
            FocusSurface::SideBar => FOCUS_SIDEBAR_COMMAND,
            FocusSurface::Terminal => FOCUS_TERMINAL_COMMAND,
        }
    }

    fn terminal_focus_command(dir: Direction) -> Option<&'static str> {
        match dir {
            Direction::West => Some(FOCUS_PREVIOUS_TERMINAL_COMMAND),
            Direction::East => Some(FOCUS_NEXT_TERMINAL_COMMAND),
            Direction::North | Direction::South => None,
        }
    }

    fn model_can_focus(snapshot: &LayoutSnapshot, model: &FocusModel, dir: Direction) -> bool {
        match model.surface {
            FocusSurface::Editors => {
                snapshot.has_any_neighbor(&model.possible_leaves, dir)
                    || Self::workbench_surface_for_direction(dir).is_some()
            }
            FocusSurface::SideBar => dir == Direction::East,
            FocusSurface::Terminal => {
                dir == Direction::North
                    || (Self::manage_terminal() && Self::terminal_focus_command(dir).is_some())
            }
        }
    }

    fn model_can_return_to_editors(model: &FocusModel, dir: Direction) -> bool {
        matches!(
            (model.surface, dir),
            (FocusSurface::SideBar, Direction::East) | (FocusSurface::Terminal, Direction::North)
        )
    }

    fn model_edge_surface(
        snapshot: &LayoutSnapshot,
        model: &FocusModel,
        dir: Direction,
    ) -> Option<FocusSurface> {
        (model.surface == FocusSurface::Editors
            && !snapshot.has_any_neighbor(&model.possible_leaves, dir))
        .then(|| Self::workbench_surface_for_direction(dir))
        .flatten()
    }

    fn normalize_possible_leaves(snapshot: &LayoutSnapshot, leaves: &mut Vec<usize>) {
        leaves.retain(|leaf| *leaf < snapshot.leaf_count());
        leaves.sort_unstable();
        leaves.dedup();
        if leaves.is_empty() {
            *leaves = snapshot.leaf_indices();
        }
    }

    fn active_models(state: &VscodeWindowState) -> Vec<FocusModel> {
        let mut models = vec![FocusModel::new(
            state.surface,
            state.possible_leaves.clone(),
        )];
        if let Some(pending) = &state.pending {
            let pending_model = FocusModel::new(pending.surface, pending.possible_leaves.clone());
            if pending_model != models[0] {
                models.push(pending_model);
            }
        }
        models
    }

    fn stage_focus_model(state: &mut VscodeWindowState, next: FocusModel) {
        let delay = Self::focus_settle_delay();
        if delay.as_millis() == 0 {
            state.surface = next.surface;
            state.possible_leaves = next.possible_leaves;
            state.pending = None;
            return;
        }
        state.pending = Some(PendingFocusState {
            possible_leaves: next.possible_leaves,
            surface: next.surface,
            settle_until_ms: Self::now_ms() + delay.as_millis() as u64,
        });
    }

    fn wait_for_focus_settle() {
        let delay = Self::focus_settle_delay();
        if delay.as_millis() > 0 {
            sleep(delay);
        }
    }

    fn state_file_path() -> PathBuf {
        if let Ok(path) = std::env::var(STATE_FILE_ENV) {
            let path = path.trim();
            if !path.is_empty() {
                return PathBuf::from(path);
            }
        }

        let base = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(base)
            .join("yeet-and-yoink")
            .join("vscode-state.json")
    }

    fn load_state_store() -> VscodeStateStore {
        let path = Self::state_file_path();
        let Ok(contents) = fs::read_to_string(path) else {
            return VscodeStateStore::default();
        };
        serde_json::from_str(&contents).unwrap_or_default()
    }

    fn save_state_store(store: &VscodeStateStore) {
        let path = Self::state_file_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
        if let Ok(bytes) = serde_json::to_vec_pretty(store) {
            let _ = fs::write(&tmp, bytes);
            let _ = fs::rename(&tmp, &path);
            let _ = fs::remove_file(&tmp);
        }
    }

    fn normalize_window_state(
        snapshot: &LayoutSnapshot,
        state: Option<VscodeWindowState>,
    ) -> VscodeWindowState {
        let mut state = state.unwrap_or_else(|| VscodeWindowState::default_for(snapshot));
        let signature = snapshot.layout_signature();
        if state.layout_signature != signature {
            return VscodeWindowState::default_for(snapshot);
        }

        Self::normalize_possible_leaves(snapshot, &mut state.possible_leaves);
        if let Some(pending) = state.pending.as_mut() {
            Self::normalize_possible_leaves(snapshot, &mut pending.possible_leaves);
            if pending.settle_until_ms <= Self::now_ms() {
                state.possible_leaves = pending.possible_leaves.clone();
                state.surface = pending.surface;
                state.pending = None;
            }
        }
        state
    }

    fn load_window_state(pid: u32, snapshot: &LayoutSnapshot) -> VscodeWindowState {
        let key = pid.to_string();
        let store = Self::load_state_store();
        Self::normalize_window_state(snapshot, store.windows.get(&key).cloned())
    }

    fn store_window_state(pid: u32, state: VscodeWindowState) {
        let key = pid.to_string();
        let mut store = Self::load_state_store();
        store.windows.insert(key, state);
        Self::save_state_store(&store);
    }

    fn clear_window_state(pid: u32) {
        let key = pid.to_string();
        let mut store = Self::load_state_store();
        store.windows.remove(&key);
        Self::save_state_store(&store);
    }

    fn wait_for_restore_settle() {
        let _span = tracing::debug_span!("vscode.wait_for_restore_settle").entered();
        if RESTORE_SETTLE_DELAY.as_millis() > 0 {
            sleep(RESTORE_SETTLE_DELAY);
        }
    }

    fn wait_for_clipboard_settle() {
        if CLIPBOARD_SETTLE_DELAY.as_millis() > 0 {
            sleep(CLIPBOARD_SETTLE_DELAY);
        }
    }

    fn clipboard_probe_token() -> String {
        format!(
            "__yeet_and_yoink_vscode_probe_{}_{}__",
            std::process::id(),
            Self::now_ms()
        )
    }

    fn clipboard_file_override() -> Option<PathBuf> {
        std::env::var(TEST_CLIPBOARD_FILE_ENV)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    }

    fn read_clipboard_text() -> Result<String> {
        if let Some(path) = Self::clipboard_file_override() {
            return fs::read_to_string(path).context("failed to read VS Code test clipboard file");
        }

        let mut errors = Vec::new();

        #[cfg(target_os = "macos")]
        let commands: &[(&str, &[&str])] = &[("pbpaste", &[])];
        #[cfg(target_os = "windows")]
        let commands: &[(&str, &[&str])] = &[(
            "powershell",
            &["-NoProfile", "-Command", "Get-Clipboard -Raw"],
        )];
        #[cfg(all(unix, not(target_os = "macos")))]
        let commands: &[(&str, &[&str])] = &[
            ("wl-paste", &["--no-newline"]),
            ("xclip", &["-selection", "clipboard", "-o"]),
            ("xsel", &["--clipboard", "--output"]),
        ];

        for (program, args) in commands {
            match Command::new(program).args(*args).output() {
                Ok(output) if output.status.success() => {
                    return Ok(runtime::stdout_text(&output));
                }
                Ok(output) => errors.push(format!(
                    "{} {:?}: {}",
                    program,
                    args,
                    runtime::stderr_text(&output).trim()
                )),
                Err(err) if err.kind() == ErrorKind::NotFound => {}
                Err(err) => errors.push(format!("{program} {:?}: {err}", args)),
            }
        }

        bail!(
            "unable to read clipboard while preparing VS Code merge; tried {}",
            errors.join("; ")
        );
    }

    fn write_clipboard_text(text: &str) -> Result<()> {
        if let Some(path) = Self::clipboard_file_override() {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            fs::write(path, text).context("failed to write VS Code test clipboard file")?;
            return Ok(());
        }

        let mut errors = Vec::new();

        #[cfg(target_os = "macos")]
        let commands: &[(&str, &[&str])] = &[("pbcopy", &[])];
        #[cfg(target_os = "windows")]
        let commands: &[(&str, &[&str])] =
            &[("powershell", &["-NoProfile", "-Command", "Set-Clipboard"])];
        #[cfg(all(unix, not(target_os = "macos")))]
        let commands: &[(&str, &[&str])] = &[
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
        ];

        for (program, args) in commands {
            let mut child = match Command::new(program)
                .args(*args)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
            {
                Ok(child) => child,
                Err(err) if err.kind() == ErrorKind::NotFound => continue,
                Err(err) => {
                    errors.push(format!("{program} {:?}: {err}", args));
                    continue;
                }
            };

            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                stdin
                    .write_all(text.as_bytes())
                    .with_context(|| format!("failed to write clipboard payload to {program}"))?;
            }

            let output = child
                .wait_with_output()
                .with_context(|| format!("failed to wait for clipboard writer {program}"))?;
            if output.status.success() {
                return Ok(());
            }

            errors.push(format!(
                "{} {:?}: {}",
                program,
                args,
                runtime::stderr_text(&output).trim()
            ));
        }

        bail!(
            "unable to restore clipboard after preparing VS Code merge; tried {}",
            errors.join("; ")
        );
    }

    fn read_captured_file_path(probe: Option<&str>) -> Result<String> {
        let capture = Self::read_clipboard_text()
            .context("failed to read active VS Code editor path from clipboard")?;
        let capture = capture.trim().to_string();
        if capture.is_empty() || probe.is_some_and(|marker| capture == marker) {
            bail!(MISSING_ACTIVE_FILE_PATH_MESSAGE);
        }
        Ok(capture)
    }

    fn copy_active_file_path_without_restoring_clipboard(pid: u32) -> Result<String> {
        let probe = Self::clipboard_probe_token();
        let probe = match Self::write_clipboard_text(&probe) {
            Ok(()) => Some(probe),
            Err(_) => None,
        };
        Self::run_command(pid, COPY_ACTIVE_FILE_PATH_COMMAND)?;
        Self::wait_for_clipboard_settle();
        Self::read_captured_file_path(probe.as_deref())
    }

    fn capture_active_file_path(pid: u32) -> Result<String> {
        let previous_clipboard = Self::read_clipboard_text().ok();
        let capture = Self::copy_active_file_path_without_restoring_clipboard(pid);
        if let Some(previous) = previous_clipboard {
            if let Err(err) = Self::write_clipboard_text(&previous) {
                logging::debug(format!(
                    "vscode: unable to restore clipboard after merge preparation err={:#}",
                    err
                ));
            }
        }
        capture
    }

    fn capture_active_group_paths(pid: u32) -> Result<(Vec<String>, usize)> {
        let previous_clipboard = Self::read_clipboard_text().ok();
        let result = (|| {
            let original_path = Self::copy_active_file_path_without_restoring_clipboard(pid)?;
            let mut captured = Vec::new();
            let mut original_index = None;
            let mut last_path: Option<String> = None;
            let mut repeated_last = 0usize;

            for index in 0..MAX_CAPTURED_GROUP_TABS {
                Self::run_command_with_args(
                    pid,
                    OPEN_EDITOR_AT_INDEX_COMMAND,
                    &[Value::from(index as u64)],
                )?;
                Self::wait_for_focus_settle();
                let path = Self::copy_active_file_path_without_restoring_clipboard(pid)?;
                if last_path.as_deref() == Some(path.as_str()) {
                    repeated_last += 1;
                    if repeated_last >= 1 {
                        break;
                    }
                } else {
                    repeated_last = 0;
                }

                if path == original_path && original_index.is_none() {
                    original_index = Some(captured.len());
                }
                captured.push(path.clone());
                last_path = Some(path);
            }

            if captured.is_empty() {
                captured.push(original_path.clone());
            }
            if original_index.is_none() {
                original_index = captured.iter().position(|path| *path == original_path);
            }
            let active_index = original_index
                .unwrap_or(0)
                .min(captured.len().saturating_sub(1));

            let _ = Self::run_command_with_args(
                pid,
                OPEN_EDITOR_AT_INDEX_COMMAND,
                &[Value::from(active_index as u64)],
            );
            Self::wait_for_focus_settle();

            Ok((captured, active_index))
        })();

        if let Some(previous) = previous_clipboard {
            if let Err(err) = Self::write_clipboard_text(&previous) {
                logging::debug(format!(
                    "vscode: unable to restore clipboard after group merge preparation err={:#}",
                    err
                ));
            }
        }

        result
    }

    fn should_prepare_terminal_merge(err: &anyhow::Error) -> bool {
        Self::manage_terminal()
            && err
                .chain()
                .any(|cause| cause.to_string().contains(MISSING_ACTIVE_FILE_PATH_MESSAGE))
    }

    fn open_path_in_active_group(pid: u32, path: &str) -> Result<()> {
        Self::run_command_with_args(pid, VSCODE_OPEN_COMMAND, &[Value::String(path.to_string())])?;
        Self::run_command(pid, KEEP_EDITOR_COMMAND)
    }

    fn refresh_window_state(pid: u32) {
        if let Ok(snapshot) = Self::snapshot_for_pid(pid) {
            Self::store_window_state(pid, VscodeWindowState::default_for(&snapshot));
        } else {
            Self::clear_window_state(pid);
        }
    }
}

impl AppAdapter for Vscode {
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
            tear_out: true,
            merge: true,
        }
    }
}

impl TopologyHandler for Vscode {
    fn can_focus(&self, dir: Direction, pid: u32) -> Result<bool> {
        let _span = tracing::debug_span!("vscode.can_focus", pid = pid, ?dir).entered();
        let snapshot = Self::snapshot_for_pid(pid)?;
        let state = Self::load_window_state(pid, &snapshot);
        let models = Self::active_models(&state);
        let can_focus = models
            .iter()
            .any(|model| Self::model_can_focus(&snapshot, model, dir));
        logging::debug(format!(
            "vscode: can_focus pid={} dir={} surface={:?} possible_leaves={:?} pending={:?} leaf_count={} can_focus={}",
            pid,
            dir,
            state.surface,
            state.possible_leaves,
            state
                .pending
                .as_ref()
                .map(|pending| (&pending.surface, &pending.possible_leaves, pending.settle_until_ms)),
            snapshot.leaf_count(),
            can_focus
        ));
        Ok(can_focus)
    }

    fn window_count(&self, pid: u32) -> Result<u32> {
        Ok(Self::layout_for_pid(pid)?.leaf_count())
    }

    fn move_decision(&self, dir: Direction, pid: u32) -> Result<MoveDecision> {
        let _span = tracing::debug_span!("vscode.move_decision", pid = pid, ?dir).entered();
        let snapshot = Self::snapshot_for_pid(pid)?;
        let state = Self::load_window_state(pid, &snapshot);
        let models = Self::active_models(&state);
        let leaf_count = snapshot.leaf_count();
        if Self::manage_terminal()
            && models
                .iter()
                .rev()
                .any(|model| model.surface == FocusSurface::Terminal)
        {
            logging::debug(format!(
                "vscode: move_decision pid={} dir={} surface={:?} possible_leaves={:?} pending={:?} leaf_count={} any_neighbor={} decision={:?}",
                pid,
                dir,
                state.surface,
                state.possible_leaves,
                state
                    .pending
                    .as_ref()
                    .map(|pending| (&pending.surface, &pending.possible_leaves, pending.settle_until_ms)),
                leaf_count,
                false,
                MoveDecision::TearOut
            ));
            return Ok(MoveDecision::TearOut);
        }
        let mut candidate_leaves = models
            .iter()
            .rev()
            .find(|model| model.surface == FocusSurface::Editors)
            .map(|model| model.possible_leaves.clone())
            .unwrap_or_default();
        if !candidate_leaves.is_empty() {
            Self::normalize_possible_leaves(&snapshot, &mut candidate_leaves);
        }
        let any_neighbor = snapshot.has_any_neighbor(&candidate_leaves, dir);
        let decision = if leaf_count <= 1
            || candidate_leaves.is_empty()
            || models
                .iter()
                .any(|model| model.surface != FocusSurface::Editors)
        {
            MoveDecision::Passthrough
        } else if !any_neighbor {
            MoveDecision::TearOut
        } else {
            MoveDecision::Internal
        };
        logging::debug(format!(
            "vscode: move_decision pid={} dir={} surface={:?} possible_leaves={:?} pending={:?} leaf_count={} any_neighbor={} decision={:?}",
            pid,
            dir,
            state.surface,
            state.possible_leaves,
            state
                .pending
                .as_ref()
                .map(|pending| (&pending.surface, &pending.possible_leaves, pending.settle_until_ms)),
            leaf_count,
            any_neighbor,
            decision
        ));
        Ok(decision)
    }

    fn focus(&self, dir: Direction, pid: u32) -> Result<()> {
        let _span = tracing::debug_span!("vscode.focus", pid = pid, ?dir).entered();
        let snapshot = Self::snapshot_for_pid(pid)?;
        let mut state = Self::load_window_state(pid, &snapshot);
        let models = Self::active_models(&state);

        if let Some(model) = models
            .iter()
            .rev()
            .find(|model| Self::model_can_return_to_editors(model, dir))
        {
            Self::run_command(pid, Self::workbench_focus_command(FocusSurface::Editors))?;
            Self::stage_focus_model(
                &mut state,
                FocusModel::new(FocusSurface::Editors, model.possible_leaves.clone()),
            );
        } else if let Some(model) = models.iter().rev().find(|model| {
            model.surface == FocusSurface::Terminal && Self::terminal_focus_command(dir).is_some()
        }) {
            Self::run_command(
                pid,
                Self::terminal_focus_command(dir)
                    .context("vscode terminal focus expected terminal command")?,
            )?;
            Self::stage_focus_model(
                &mut state,
                FocusModel::new(FocusSurface::Terminal, model.possible_leaves.clone()),
            );
        } else if models.iter().any(|model| {
            model.surface == FocusSurface::Editors
                && snapshot.has_any_neighbor(&model.possible_leaves, dir)
        }) {
            let base = models
                .iter()
                .rev()
                .find(|model| model.surface == FocusSurface::Editors)
                .context("vscode focus expected editor model")?;
            Self::run_command(pid, Self::focus_command(dir))?;
            let next_leaves = if snapshot.has_any_neighbor(&base.possible_leaves, dir) {
                snapshot.apply_focus_transition(&base.possible_leaves, dir)
            } else {
                base.possible_leaves.clone()
            };
            Self::stage_focus_model(
                &mut state,
                FocusModel::new(FocusSurface::Editors, next_leaves),
            );
        } else if let Some((model, surface)) = models.iter().rev().find_map(|model| {
            Self::model_edge_surface(&snapshot, model, dir).map(|surface| (model, surface))
        }) {
            Self::run_command(pid, Self::workbench_focus_command(surface))?;
            Self::stage_focus_model(
                &mut state,
                FocusModel::new(surface, model.possible_leaves.clone()),
            );
        } else {
            bail!("vscode focus {} has no internal target", dir);
        }

        logging::debug(format!(
            "vscode: focus pid={} dir={} surface={:?} possible_leaves={:?} pending={:?}",
            pid,
            dir,
            state.surface,
            state.possible_leaves,
            state.pending.as_ref().map(|pending| (
                &pending.surface,
                &pending.possible_leaves,
                pending.settle_until_ms
            ))
        ));
        Self::store_window_state(pid, state);
        Self::wait_for_focus_settle();
        Ok(())
    }

    fn move_internal(&self, dir: Direction, pid: u32) -> Result<()> {
        let _span = tracing::debug_span!("vscode.move_internal", pid = pid, ?dir).entered();
        let snapshot = Self::snapshot_for_pid(pid)?;
        let mut state = Self::load_window_state(pid, &snapshot);
        let models = Self::active_models(&state);
        if models
            .iter()
            .any(|model| model.surface != FocusSurface::Editors)
        {
            bail!("vscode move_internal only supports editor focus surface");
        }
        let base = models
            .iter()
            .rev()
            .find(|model| model.surface == FocusSurface::Editors)
            .context("vscode move_internal expected editor model")?;

        let any_neighbor = snapshot.has_any_neighbor(&base.possible_leaves, dir);
        let all_neighbors = snapshot.all_have_neighbor(&base.possible_leaves, dir);
        if !any_neighbor {
            bail!("vscode move_internal reached edge; move_out should be used");
        }

        Self::run_command(pid, Self::move_command(dir))?;
        Self::wait_for_focus_settle();
        let next_snapshot = Self::snapshot_for_pid(pid)?;
        state = if all_neighbors {
            let mut next_state = state;
            next_state.possible_leaves =
                snapshot.apply_focus_transition(&base.possible_leaves, dir);
            next_state.layout_signature = next_snapshot.layout_signature();
            next_state.surface = FocusSurface::Editors;
            next_state.pending = None;
            Self::normalize_window_state(&next_snapshot, Some(next_state))
        } else {
            VscodeWindowState::default_for(&next_snapshot)
        };

        logging::debug(format!(
            "vscode: move_internal pid={} dir={} all_neighbors={} new_possible_leaves={:?} leaf_count={}",
            pid,
            dir,
            all_neighbors,
            state.possible_leaves,
            next_snapshot.leaf_count()
        ));
        Self::store_window_state(pid, state);
        Ok(())
    }

    fn move_out(&self, _dir: Direction, pid: u32) -> Result<TearResult> {
        let scope = Self::tear_off_scope();
        let _span = tracing::debug_span!("vscode.move_out", pid = pid, ?scope).entered();
        let snapshot = Self::snapshot_for_pid(pid)?;
        let state = Self::load_window_state(pid, &snapshot);
        let command = if Self::manage_terminal()
            && Self::active_models(&state)
                .iter()
                .rev()
                .any(|model| model.surface == FocusSurface::Terminal)
        {
            MOVE_TERMINAL_TO_NEW_WINDOW_COMMAND
        } else {
            match scope {
                EditorTearOffScope::Buffer => MOVE_EDITOR_TO_NEW_WINDOW_COMMAND,
                EditorTearOffScope::Window => MOVE_EDITOR_GROUP_TO_NEW_WINDOW_COMMAND,
                EditorTearOffScope::Workspace => {
                    bail!("VS Code workspace tear-off is not implemented yet")
                }
            }
        };
        Self::run_command(pid, command)?;
        Self::wait_for_focus_settle();
        Self::refresh_window_state(pid);
        Ok(TearResult {
            spawn_command: None,
        })
    }

    fn merge_execution_mode(&self) -> MergeExecutionMode {
        MergeExecutionMode::TargetFocused
    }

    fn prepare_merge(&self, source_pid: Option<ProcessId>) -> Result<MergePreparation> {
        let source_pid = source_pid.context("VS Code merge requires source pid")?;
        if Self::manage_terminal() {
            if let Ok(snapshot) = Self::snapshot_for_pid(source_pid.get()) {
                let state = Self::load_window_state(source_pid.get(), &snapshot);
                if Self::active_models(&state)
                    .iter()
                    .rev()
                    .any(|model| model.surface == FocusSurface::Terminal)
                {
                    return Ok(MergePreparation::with_payload(
                        VscodeMergePreparation::Terminal,
                    ));
                }
            }
        }
        let preparation = match Self::tear_off_scope() {
            EditorTearOffScope::Buffer => match Self::capture_active_file_path(source_pid.get()) {
                Ok(path) => VscodeMergePreparation::Buffer { path },
                Err(err) if Self::should_prepare_terminal_merge(&err) => {
                    VscodeMergePreparation::Terminal
                }
                Err(err) => return Err(err),
            },
            EditorTearOffScope::Window => {
                match Self::capture_active_group_paths(source_pid.get()) {
                    Ok((paths, active_index)) => VscodeMergePreparation::Group {
                        paths,
                        active_index,
                    },
                    Err(err) if Self::should_prepare_terminal_merge(&err) => {
                        VscodeMergePreparation::Terminal
                    }
                    Err(err) => return Err(err),
                }
            }
            scope @ EditorTearOffScope::Workspace => {
                VscodeMergePreparation::RestoreMainWindow { scope }
            }
        };
        Ok(MergePreparation::with_payload(preparation))
    }

    fn merge_into_target(
        &self,
        dir: Direction,
        source_pid: Option<ProcessId>,
        target_pid: Option<ProcessId>,
        preparation: MergePreparation,
    ) -> Result<()> {
        let source_pid = source_pid
            .context("VS Code merge requires source pid")?
            .get();
        let preparation = preparation
            .into_payload::<VscodeMergePreparation>()
            .context("VS Code merge preparation missing")?;

        match preparation {
            VscodeMergePreparation::Buffer { path } => {
                let target_pid =
                    target_pid.context("VS Code target-focused merge requires target pid")?;
                let target_pid = target_pid.get();
                Self::run_command(target_pid, Self::split_command(dir.opposite()))?;
                Self::wait_for_focus_settle();
                Self::open_path_in_active_group(target_pid, &path)?;
                Self::wait_for_focus_settle();
                Self::run_command(target_pid, CLOSE_OTHER_EDITORS_IN_GROUP_COMMAND)?;
                Self::refresh_window_state(target_pid);
                Self::clear_window_state(source_pid);
            }
            VscodeMergePreparation::Group {
                paths,
                active_index,
            } => {
                let target_pid =
                    target_pid.context("VS Code target-focused merge requires target pid")?;
                let target_pid = target_pid.get();
                Self::run_command(target_pid, Self::split_command(dir.opposite()))?;
                Self::wait_for_focus_settle();
                Self::run_command(target_pid, CLOSE_ACTIVE_EDITOR_COMMAND)?;
                Self::wait_for_focus_settle();
                for path in &paths {
                    Self::open_path_in_active_group(target_pid, path)?;
                    Self::wait_for_focus_settle();
                }
                let _ = Self::run_command_with_args(
                    target_pid,
                    OPEN_EDITOR_AT_INDEX_COMMAND,
                    &[Value::from(active_index as u64)],
                );
                Self::wait_for_focus_settle();
                Self::refresh_window_state(target_pid);
                Self::clear_window_state(source_pid);
            }
            VscodeMergePreparation::Terminal => {
                Self::run_command(source_pid, MOVE_TERMINAL_TO_PANEL_COMMAND)?;
                Self::wait_for_restore_settle();
                Self::clear_window_state(source_pid);
            }
            VscodeMergePreparation::RestoreMainWindow { .. } => {
                Self::run_command(source_pid, RESTORE_EDITORS_TO_MAIN_WINDOW_COMMAND)?;
                Self::wait_for_restore_settle();
                Self::clear_window_state(source_pid);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EditorLayout, FocusSurface, LayoutSnapshot, Vscode, VscodeMergePreparation,
        VscodeWindowState,
    };
    use crate::config::{self, EditorTearOffScope};
    use crate::engine::contract::{AppAdapter, MergePreparation, MoveDecision, TopologyHandler};
    use crate::engine::runtime::ProcessId;
    use crate::engine::topology::Direction;
    use crate::utils::env_guard;
    use std::ffi::OsString;
    use std::fs;
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    use tungstenite::{accept, Message};

    fn unique_temp_dir(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("yeet-and-yoink-{label}-{stamp}"))
    }

    fn set_env(key: &str, value: Option<&str>) -> Option<OsString> {
        let old = std::env::var_os(key);
        if let Some(value) = value {
            std::env::set_var(key, value);
        } else {
            std::env::remove_var(key);
        }
        old
    }

    fn restore_env(key: &str, old: Option<OsString>) {
        if let Some(old) = old {
            std::env::set_var(key, old);
        } else {
            std::env::remove_var(key);
        }
    }

    fn set_focus_settle_immediate() -> Option<OsString> {
        set_env(super::FOCUS_SETTLE_ENV, Some("0"))
    }

    fn load_vscode_config(root: &PathBuf, scope: Option<EditorTearOffScope>) -> Option<OsString> {
        load_vscode_config_with_terminal(root, scope, false)
    }

    fn load_vscode_config_with_terminal(
        root: &PathBuf,
        scope: Option<EditorTearOffScope>,
        manage_terminal: bool,
    ) -> Option<OsString> {
        let config_path = root.join("config.toml");
        let mut config = String::from("[app.editor.vscode]\nenabled = true\n");
        if manage_terminal {
            config.push_str("manage_terminal = true\n");
        }
        if let Some(scope) = scope {
            config.push_str(&format!(
                "tear_off_scope = \"{}\"\n",
                match scope {
                    EditorTearOffScope::Buffer => "buffer",
                    EditorTearOffScope::Window => "window",
                    EditorTearOffScope::Workspace => "workspace",
                }
            ));
        }
        fs::write(&config_path, config).expect("config file should be written");
        let old = set_env(
            "NIRI_DEEP_CONFIG",
            Some(config_path.to_str().expect("utf-8 path")),
        );
        config::prepare().expect("config should load");
        old
    }

    fn restore_config(old: Option<OsString>) {
        restore_env("NIRI_DEEP_CONFIG", old);
        config::prepare().expect("config should reload");
    }

    fn set_clipboard_file(path: &PathBuf) -> Option<OsString> {
        set_env(
            super::TEST_CLIPBOARD_FILE_ENV,
            Some(path.to_str().expect("utf-8 path")),
        )
    }

    fn two_column_layout_json() -> &'static str {
        r#"{"orientation":0,"groups":[{"size":0.5},{"size":0.5}]}"#
    }

    fn two_row_layout_json() -> &'static str {
        r#"{"orientation":1,"groups":[{"size":0.5},{"size":0.5}]}"#
    }

    fn single_group_layout_json() -> &'static str {
        r#"{"orientation":0,"groups":[{"size":1.0}]}"#
    }

    struct TestWsServer {
        port: u16,
        expected_messages: usize,
        payload_rx: mpsc::Receiver<String>,
        handle: thread::JoinHandle<()>,
    }

    impl TestWsServer {
        fn spawn(responses: Vec<Option<&str>>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
            let port = listener
                .local_addr()
                .expect("listener addr should exist")
                .port();
            let (payload_tx, payload_rx) = mpsc::channel();
            let expected_messages = responses.len();
            let responses: Vec<Option<String>> = responses
                .into_iter()
                .map(|value| value.map(ToString::to_string))
                .collect();

            let handle = thread::spawn(move || {
                for response in responses {
                    let (stream, _) = listener.accept().expect("client should connect");
                    let mut socket = accept(stream).expect("websocket handshake should succeed");
                    let message = socket.read().expect("request should be readable");
                    let payload = match message {
                        Message::Text(text) => text.to_string(),
                        Message::Binary(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                        other => panic!("unexpected websocket message: {other:?}"),
                    };
                    payload_tx.send(payload).expect("payload should be sent");

                    if let Some(response) = response {
                        socket
                            .send(Message::Text(response.into()))
                            .expect("response should be written");
                    }

                    let _ = socket.close(None);
                }
            });

            Self {
                port,
                expected_messages,
                payload_rx,
                handle,
            }
        }

        fn finish(self) -> Vec<String> {
            let mut payloads = Vec::with_capacity(self.expected_messages);
            for _ in 0..self.expected_messages {
                payloads.push(
                    self.payload_rx
                        .recv_timeout(Duration::from_secs(2))
                        .expect("payload should be captured"),
                );
            }
            self.handle.join().expect("server thread should join");
            payloads
        }
    }

    fn read_state_file(path: &PathBuf) -> String {
        fs::read_to_string(path).expect("state file should exist")
    }

    #[test]
    fn declares_explicit_capability_contract() {
        let app = Vscode;
        let caps = AppAdapter::capabilities(&app);
        assert!(caps.probe);
        assert!(caps.focus);
        assert!(caps.move_internal);
        assert!(!caps.resize_internal);
        assert!(!caps.rearrange);
        assert!(caps.tear_out);
        assert!(caps.merge);
    }

    #[test]
    fn declares_config_aliases() {
        let app = Vscode;
        assert_eq!(app.config_aliases(), Some(super::ADAPTER_ALIASES));
    }

    #[test]
    fn command_mapping_matches_vscode_ids() {
        assert_eq!(
            Vscode::focus_command(Direction::West),
            "workbench.action.focusLeftGroupWithoutWrap"
        );
        assert_eq!(
            Vscode::focus_command(Direction::East),
            "workbench.action.focusRightGroupWithoutWrap"
        );
        assert_eq!(
            Vscode::focus_command(Direction::North),
            "workbench.action.focusAboveGroupWithoutWrap"
        );
        assert_eq!(
            Vscode::focus_command(Direction::South),
            "workbench.action.focusBelowGroupWithoutWrap"
        );
        assert_eq!(
            Vscode::move_command(Direction::West),
            "workbench.action.moveEditorToLeftGroup"
        );
        assert_eq!(
            Vscode::move_command(Direction::East),
            "workbench.action.moveEditorToRightGroup"
        );
        assert_eq!(
            Vscode::move_command(Direction::North),
            "workbench.action.moveEditorToAboveGroup"
        );
        assert_eq!(
            Vscode::move_command(Direction::South),
            "workbench.action.moveEditorToBelowGroup"
        );
        assert_eq!(
            Vscode::split_command(Direction::West),
            "workbench.action.splitEditorLeft"
        );
        assert_eq!(
            Vscode::split_command(Direction::East),
            "workbench.action.splitEditorRight"
        );
        assert_eq!(
            Vscode::split_command(Direction::North),
            "workbench.action.splitEditorUp"
        );
        assert_eq!(
            Vscode::split_command(Direction::South),
            "workbench.action.splitEditorDown"
        );
        assert_eq!(
            Vscode::workbench_focus_command(FocusSurface::Editors),
            super::FOCUS_ACTIVE_EDITOR_GROUP_COMMAND
        );
        assert_eq!(
            Vscode::workbench_focus_command(FocusSurface::SideBar),
            super::FOCUS_SIDEBAR_COMMAND
        );
        assert_eq!(
            Vscode::workbench_focus_command(FocusSurface::Terminal),
            super::FOCUS_TERMINAL_COMMAND
        );
        assert_eq!(
            Vscode::terminal_focus_command(Direction::West),
            Some(super::FOCUS_PREVIOUS_TERMINAL_COMMAND)
        );
        assert_eq!(
            Vscode::terminal_focus_command(Direction::East),
            Some(super::FOCUS_NEXT_TERMINAL_COMMAND)
        );
        assert_eq!(Vscode::terminal_focus_command(Direction::North), None);
    }

    #[test]
    fn layout_snapshot_detects_neighbors() {
        let layout: EditorLayout =
            serde_json::from_str(two_column_layout_json()).expect("layout json should parse");
        let snapshot = LayoutSnapshot::new(layout);
        assert_eq!(snapshot.leaf_count(), 2);
        assert_eq!(snapshot.neighbor_for_leaf(0, Direction::East), Some(1));
        assert_eq!(snapshot.neighbor_for_leaf(1, Direction::West), Some(0));
        assert_eq!(snapshot.neighbor_for_leaf(0, Direction::West), None);
        assert_eq!(
            snapshot.apply_focus_transition(&[0, 1], Direction::West),
            vec![0]
        );
    }

    #[test]
    fn parse_socket_inode_reads_proc_fd_symlink_targets() {
        assert_eq!(Vscode::parse_socket_inode("socket:[12345]"), Some(12345));
        assert_eq!(Vscode::parse_socket_inode("anon_inode:[eventfd]"), None);
        assert_eq!(Vscode::parse_socket_inode("/tmp/not-a-socket"), None);
    }

    #[test]
    fn parse_listening_ports_filters_by_inode_and_state() {
        let contents = "\
  sl  local_address rem_address st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n\
   0: 0100007F:0E7E 00000000:0000 0A 00000000:00000000 00:00000000 00000000 1000 0 123 1 0000000000000000 100 0 0 10 0\n\
   1: 0100007F:1771 00000000:0000 01 00000000:00000000 00:00000000 00000000 1000 0 123 1 0000000000000000 100 0 0 10 0\n\
   2: 0100007F:1772 00000000:0000 0A 00000000:00000000 00:00000000 00000000 1000 0 456 1 0000000000000000 100 0 0 10 0\n";
        let inodes = std::collections::HashSet::from([123_u64]);
        assert_eq!(
            Vscode::parse_listening_tcp_ports(contents, &inodes),
            vec![3710]
        );
    }

    #[test]
    fn window_count_queries_layout_over_websocket() {
        let _guard = env_guard();
        let root = unique_temp_dir("vscode-window-count");
        fs::create_dir_all(&root).expect("temp dir should be created");
        let state_file = root.join("state.json");
        let server = TestWsServer::spawn(vec![Some(two_column_layout_json())]);
        let old_port = set_env(
            super::REMOTE_CONTROL_PORT_ENV,
            Some(&server.port.to_string()),
        );
        let old_state = set_env(
            super::STATE_FILE_ENV,
            Some(state_file.to_str().expect("utf-8 path")),
        );

        let app = Vscode;
        let count = TopologyHandler::window_count(&app, 42).expect("layout query should succeed");
        assert_eq!(count, 2);

        let payloads = server.finish();
        assert_eq!(payloads.len(), 1);
        assert!(payloads[0].contains("\"command\":\"vscode.getEditorLayout\""));

        restore_env(super::REMOTE_CONTROL_PORT_ENV, old_port);
        restore_env(super::STATE_FILE_ENV, old_state);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn run_command_does_not_wait_for_response_or_close() {
        let _guard = env_guard();
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let port = listener
            .local_addr()
            .expect("listener addr should exist")
            .port();
        let (payload_tx, payload_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let server_handle = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("client should connect");
            let mut socket = accept(stream).expect("websocket handshake should succeed");
            let payload = match socket.read().expect("request should be readable") {
                Message::Text(text) => text.to_string(),
                Message::Binary(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                other => panic!("unexpected websocket message: {other:?}"),
            };
            payload_tx
                .send(payload)
                .expect("payload should be captured");
            release_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("test should release server");
            let _ = socket.close(None);
        });
        let old_port = set_env(super::REMOTE_CONTROL_PORT_ENV, Some(&port.to_string()));

        let (done_tx, done_rx) = mpsc::channel();
        let command_handle = thread::spawn(move || {
            let result = Vscode::run_command(55, super::FOCUS_SIDEBAR_COMMAND)
                .map_err(|err| format!("{err:#}"));
            done_tx.send(result).expect("result should be sent");
        });

        let payload = payload_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("payload should arrive");
        assert!(payload.contains("\"command\":\"workbench.action.focusSideBar\""));
        let result = done_rx
            .recv_timeout(Duration::from_millis(150))
            .expect("command should complete without waiting for a websocket response");
        assert!(result.is_ok(), "command should succeed: {result:?}");

        release_tx.send(()).expect("server should be released");
        command_handle
            .join()
            .expect("command thread should join cleanly");
        server_handle
            .join()
            .expect("server thread should join cleanly");

        restore_env(super::REMOTE_CONTROL_PORT_ENV, old_port);
    }

    #[test]
    fn focus_west_collapses_possible_leaves_and_persists_state() {
        let _guard = env_guard();
        let root = unique_temp_dir("vscode-focus-west");
        fs::create_dir_all(&root).expect("temp dir should be created");
        let state_file = root.join("state.json");
        let server = TestWsServer::spawn(vec![Some(two_column_layout_json()), None]);
        let old_port = set_env(
            super::REMOTE_CONTROL_PORT_ENV,
            Some(&server.port.to_string()),
        );
        let old_state = set_env(
            super::STATE_FILE_ENV,
            Some(state_file.to_str().expect("utf-8 path")),
        );
        let old_settle = set_focus_settle_immediate();

        let app = Vscode;
        TopologyHandler::focus(&app, Direction::West, 77).expect("focus should succeed");

        let payloads = server.finish();
        assert_eq!(payloads.len(), 2);
        assert!(payloads[0].contains("\"command\":\"vscode.getEditorLayout\""));
        assert!(payloads[1].contains("\"command\":\"workbench.action.focusLeftGroupWithoutWrap\""));
        let state = read_state_file(&state_file);
        assert!(
            state.contains("\"possible_leaves\": [\n        0\n      ]")
                || state.contains("\"possible_leaves\":[0]")
        );

        restore_env(super::REMOTE_CONTROL_PORT_ENV, old_port);
        restore_env(super::STATE_FILE_ENV, old_state);
        restore_env(super::FOCUS_SETTLE_ENV, old_settle);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn can_focus_stays_internal_while_pending_transition_is_unsettled() {
        let _guard = env_guard();
        let root = unique_temp_dir("vscode-pending-can-focus");
        fs::create_dir_all(&root).expect("temp dir should be created");
        let state_file = root.join("state.json");
        let server = TestWsServer::spawn(vec![Some(two_column_layout_json())]);
        let old_port = set_env(
            super::REMOTE_CONTROL_PORT_ENV,
            Some(&server.port.to_string()),
        );
        let old_state = set_env(
            super::STATE_FILE_ENV,
            Some(state_file.to_str().expect("utf-8 path")),
        );

        let pid = 188;
        Vscode::store_window_state(
            pid,
            super::VscodeWindowState {
                layout_signature: LayoutSnapshot::new(
                    serde_json::from_str(two_column_layout_json()).expect("layout should parse"),
                )
                .layout_signature(),
                possible_leaves: vec![0, 1],
                surface: FocusSurface::Editors,
                pending: Some(super::PendingFocusState {
                    possible_leaves: vec![0],
                    surface: FocusSurface::Editors,
                    settle_until_ms: u64::MAX,
                }),
            },
        );

        let app = Vscode;
        assert!(TopologyHandler::can_focus(&app, Direction::West, pid)
            .expect("can_focus should succeed while transition is pending"));

        let payloads = server.finish();
        assert_eq!(payloads.len(), 1);
        assert!(payloads[0].contains("\"command\":\"vscode.getEditorLayout\""));

        restore_env(super::REMOTE_CONTROL_PORT_ENV, old_port);
        restore_env(super::STATE_FILE_ENV, old_state);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn focus_keeps_using_editor_commands_while_pending_edge_transition_is_unsettled() {
        let _guard = env_guard();
        let root = unique_temp_dir("vscode-pending-focus");
        fs::create_dir_all(&root).expect("temp dir should be created");
        let state_file = root.join("state.json");
        let server = TestWsServer::spawn(vec![Some(two_column_layout_json()), None]);
        let old_port = set_env(
            super::REMOTE_CONTROL_PORT_ENV,
            Some(&server.port.to_string()),
        );
        let old_state = set_env(
            super::STATE_FILE_ENV,
            Some(state_file.to_str().expect("utf-8 path")),
        );
        let old_settle = set_focus_settle_immediate();

        let pid = 189;
        Vscode::store_window_state(
            pid,
            super::VscodeWindowState {
                layout_signature: LayoutSnapshot::new(
                    serde_json::from_str(two_column_layout_json()).expect("layout should parse"),
                )
                .layout_signature(),
                possible_leaves: vec![0, 1],
                surface: FocusSurface::Editors,
                pending: Some(super::PendingFocusState {
                    possible_leaves: vec![0],
                    surface: FocusSurface::Editors,
                    settle_until_ms: u64::MAX,
                }),
            },
        );

        let app = Vscode;
        TopologyHandler::focus(&app, Direction::West, pid)
            .expect("focus should stay inside editor groups while pending");

        let payloads = server.finish();
        assert_eq!(payloads.len(), 2);
        assert!(payloads[1].contains("\"command\":\"workbench.action.focusLeftGroupWithoutWrap\""));

        restore_env(super::REMOTE_CONTROL_PORT_ENV, old_port);
        restore_env(super::STATE_FILE_ENV, old_state);
        restore_env(super::FOCUS_SETTLE_ENV, old_settle);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn focus_editor_edge_enters_sidebar_then_returns_to_editor() {
        let _guard = env_guard();
        let root = unique_temp_dir("vscode-sidebar-focus");
        fs::create_dir_all(&root).expect("temp dir should be created");
        let state_file = root.join("state.json");
        let server = TestWsServer::spawn(vec![
            Some(single_group_layout_json()),
            None,
            Some(single_group_layout_json()),
            None,
        ]);
        let old_port = set_env(
            super::REMOTE_CONTROL_PORT_ENV,
            Some(&server.port.to_string()),
        );
        let old_state = set_env(
            super::STATE_FILE_ENV,
            Some(state_file.to_str().expect("utf-8 path")),
        );
        let old_settle = set_focus_settle_immediate();

        let app = Vscode;
        TopologyHandler::focus(&app, Direction::West, 88).expect("focus sidebar should succeed");
        TopologyHandler::focus(&app, Direction::East, 88).expect("focus editor should succeed");

        let payloads = server.finish();
        assert_eq!(payloads.len(), 4);
        assert!(payloads[1].contains("\"command\":\"workbench.action.focusSideBar\""));
        assert!(payloads[3].contains("\"command\":\"workbench.action.focusActiveEditorGroup\""));

        restore_env(super::REMOTE_CONTROL_PORT_ENV, old_port);
        restore_env(super::STATE_FILE_ENV, old_state);
        restore_env(super::FOCUS_SETTLE_ENV, old_settle);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn focus_terminal_west_cycles_previous_terminal_when_enabled() {
        let _guard = env_guard();
        let root = unique_temp_dir("vscode-terminal-focus");
        fs::create_dir_all(&root).expect("temp dir should be created");
        let state_file = root.join("state.json");
        let server = TestWsServer::spawn(vec![
            Some(single_group_layout_json()),
            Some(single_group_layout_json()),
            None,
        ]);
        let old_port = set_env(
            super::REMOTE_CONTROL_PORT_ENV,
            Some(&server.port.to_string()),
        );
        let old_state = set_env(
            super::STATE_FILE_ENV,
            Some(state_file.to_str().expect("utf-8 path")),
        );
        let old_settle = set_focus_settle_immediate();
        let old_config = load_vscode_config_with_terminal(&root, None, true);

        let pid = 408;
        Vscode::store_window_state(
            pid,
            VscodeWindowState {
                layout_signature: LayoutSnapshot::new(
                    serde_json::from_str(single_group_layout_json()).expect("layout should parse"),
                )
                .layout_signature(),
                possible_leaves: vec![0],
                surface: FocusSurface::Terminal,
                pending: None,
            },
        );

        let app = Vscode;
        assert!(TopologyHandler::can_focus(&app, Direction::West, pid)
            .expect("terminal focus should be enabled"));
        TopologyHandler::focus(&app, Direction::West, pid)
            .expect("terminal focus should cycle previous terminal");

        let payloads = server.finish();
        assert_eq!(payloads.len(), 3);
        assert!(payloads[2].contains("\"command\":\"workbench.action.terminal.focusPrevious\""));
        let state = read_state_file(&state_file);
        assert!(
            state.contains("\"surface\": \"terminal\"")
                || state.contains("\"surface\":\"terminal\"")
        );

        restore_env(super::REMOTE_CONTROL_PORT_ENV, old_port);
        restore_env(super::STATE_FILE_ENV, old_state);
        restore_env(super::FOCUS_SETTLE_ENV, old_settle);
        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn terminal_surface_move_decision_uses_tear_out_when_enabled() {
        let _guard = env_guard();
        let root = unique_temp_dir("vscode-terminal-move-decision");
        fs::create_dir_all(&root).expect("temp dir should be created");
        let state_file = root.join("state.json");
        let server = TestWsServer::spawn(vec![Some(single_group_layout_json())]);
        let old_port = set_env(
            super::REMOTE_CONTROL_PORT_ENV,
            Some(&server.port.to_string()),
        );
        let old_state = set_env(
            super::STATE_FILE_ENV,
            Some(state_file.to_str().expect("utf-8 path")),
        );
        let old_config = load_vscode_config_with_terminal(&root, None, true);

        let pid = 409;
        Vscode::store_window_state(
            pid,
            VscodeWindowState {
                layout_signature: LayoutSnapshot::new(
                    serde_json::from_str(single_group_layout_json()).expect("layout should parse"),
                )
                .layout_signature(),
                possible_leaves: vec![0],
                surface: FocusSurface::Terminal,
                pending: None,
            },
        );

        let app = Vscode;
        let decision = TopologyHandler::move_decision(&app, Direction::East, pid)
            .expect("move_decision should succeed");
        assert_eq!(decision, MoveDecision::TearOut);

        let payloads = server.finish();
        assert_eq!(payloads.len(), 1);
        assert!(payloads[0].contains("\"command\":\"vscode.getEditorLayout\""));

        restore_env(super::REMOTE_CONTROL_PORT_ENV, old_port);
        restore_env(super::STATE_FILE_ENV, old_state);
        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn move_decision_becomes_tear_out_after_focus_history_reaches_edge() {
        let _guard = env_guard();
        let root = unique_temp_dir("vscode-tearout-decision");
        fs::create_dir_all(&root).expect("temp dir should be created");
        let state_file = root.join("state.json");
        let server = TestWsServer::spawn(vec![
            Some(two_column_layout_json()),
            None,
            Some(two_column_layout_json()),
        ]);
        let old_port = set_env(
            super::REMOTE_CONTROL_PORT_ENV,
            Some(&server.port.to_string()),
        );
        let old_state = set_env(
            super::STATE_FILE_ENV,
            Some(state_file.to_str().expect("utf-8 path")),
        );
        let old_settle = set_focus_settle_immediate();

        let app = Vscode;
        TopologyHandler::focus(&app, Direction::West, 99).expect("focus should succeed");
        let decision = TopologyHandler::move_decision(&app, Direction::West, 99)
            .expect("move_decision should succeed");
        assert_eq!(decision, MoveDecision::TearOut);

        let payloads = server.finish();
        assert_eq!(payloads.len(), 3);
        assert!(payloads[2].contains("\"command\":\"vscode.getEditorLayout\""));

        restore_env(super::REMOTE_CONTROL_PORT_ENV, old_port);
        restore_env(super::STATE_FILE_ENV, old_state);
        restore_env(super::FOCUS_SETTLE_ENV, old_settle);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn move_decision_prefers_pending_editor_model_over_stale_base_state() {
        let _guard = env_guard();
        let root = unique_temp_dir("vscode-pending-move-decision");
        fs::create_dir_all(&root).expect("temp dir should be created");
        let state_file = root.join("state.json");
        let server = TestWsServer::spawn(vec![Some(two_column_layout_json())]);
        let old_port = set_env(
            super::REMOTE_CONTROL_PORT_ENV,
            Some(&server.port.to_string()),
        );
        let old_state = set_env(
            super::STATE_FILE_ENV,
            Some(state_file.to_str().expect("utf-8 path")),
        );

        let pid = 190;
        Vscode::store_window_state(
            pid,
            VscodeWindowState {
                layout_signature: LayoutSnapshot::new(
                    serde_json::from_str(two_column_layout_json()).expect("layout should parse"),
                )
                .layout_signature(),
                possible_leaves: vec![0, 1],
                surface: FocusSurface::Editors,
                pending: Some(super::PendingFocusState {
                    possible_leaves: vec![1],
                    surface: FocusSurface::Editors,
                    settle_until_ms: u64::MAX,
                }),
            },
        );

        let app = Vscode;
        let decision = TopologyHandler::move_decision(&app, Direction::East, pid)
            .expect("move_decision should succeed with pending focus state");
        assert_eq!(decision, MoveDecision::TearOut);

        let payloads = server.finish();
        assert_eq!(payloads.len(), 1);
        assert!(payloads[0].contains("\"command\":\"vscode.getEditorLayout\""));

        restore_env(super::REMOTE_CONTROL_PORT_ENV, old_port);
        restore_env(super::STATE_FILE_ENV, old_state);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn move_internal_sends_directional_remote_command_and_refreshes_state() {
        let _guard = env_guard();
        let root = unique_temp_dir("vscode-move");
        fs::create_dir_all(&root).expect("temp dir should be created");
        let state_file = root.join("state.json");
        let server = TestWsServer::spawn(vec![
            Some(two_row_layout_json()),
            None,
            Some(two_row_layout_json()),
        ]);
        let old_port = set_env(
            super::REMOTE_CONTROL_PORT_ENV,
            Some(&server.port.to_string()),
        );
        let old_state = set_env(
            super::STATE_FILE_ENV,
            Some(state_file.to_str().expect("utf-8 path")),
        );
        let old_settle = set_focus_settle_immediate();
        let old_config = load_vscode_config(&root, None);

        let app = Vscode;
        let pid = 123;
        Vscode::store_window_state(
            pid,
            super::VscodeWindowState {
                layout_signature: LayoutSnapshot::new(
                    serde_json::from_str(two_row_layout_json()).expect("layout should parse"),
                )
                .layout_signature(),
                possible_leaves: vec![0],
                surface: FocusSurface::Editors,
                pending: None,
            },
        );
        TopologyHandler::move_internal(&app, Direction::South, pid)
            .expect("directional move should succeed");

        let payloads = server.finish();
        assert_eq!(payloads.len(), 3);
        assert!(payloads[1].contains("\"command\":\"workbench.action.moveEditorToBelowGroup\""));

        restore_env(super::REMOTE_CONTROL_PORT_ENV, old_port);
        restore_env(super::STATE_FILE_ENV, old_state);
        restore_env(super::FOCUS_SETTLE_ENV, old_settle);
        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn move_out_uses_configured_editor_group_tear_off_scope() {
        let _guard = env_guard();
        let root = unique_temp_dir("vscode-group-tearout");
        fs::create_dir_all(&root).expect("temp dir should be created");
        let state_file = root.join("state.json");
        let server = TestWsServer::spawn(vec![
            Some(single_group_layout_json()),
            None,
            Some(single_group_layout_json()),
        ]);
        let old_port = set_env(
            super::REMOTE_CONTROL_PORT_ENV,
            Some(&server.port.to_string()),
        );
        let old_state = set_env(
            super::STATE_FILE_ENV,
            Some(state_file.to_str().expect("utf-8 path")),
        );
        let old_settle = set_focus_settle_immediate();
        let old_config = load_vscode_config(&root, Some(EditorTearOffScope::Window));

        let app = Vscode;
        TopologyHandler::move_out(&app, Direction::East, 321).expect("tear-out should succeed");

        let payloads = server.finish();
        assert_eq!(payloads.len(), 3);
        assert!(payloads[0].contains("\"command\":\"vscode.getEditorLayout\""));
        assert!(payloads[1].contains("\"command\":\"workbench.action.moveEditorGroupToNewWindow\""));
        assert!(payloads[2].contains("\"command\":\"vscode.getEditorLayout\""));

        restore_env(super::REMOTE_CONTROL_PORT_ENV, old_port);
        restore_env(super::STATE_FILE_ENV, old_state);
        restore_env(super::FOCUS_SETTLE_ENV, old_settle);
        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn move_out_uses_terminal_window_command_when_terminal_management_is_enabled() {
        let _guard = env_guard();
        let root = unique_temp_dir("vscode-terminal-tearout");
        fs::create_dir_all(&root).expect("temp dir should be created");
        let state_file = root.join("state.json");
        let server = TestWsServer::spawn(vec![
            Some(single_group_layout_json()),
            None,
            Some(single_group_layout_json()),
        ]);
        let old_port = set_env(
            super::REMOTE_CONTROL_PORT_ENV,
            Some(&server.port.to_string()),
        );
        let old_state = set_env(
            super::STATE_FILE_ENV,
            Some(state_file.to_str().expect("utf-8 path")),
        );
        let old_settle = set_focus_settle_immediate();
        let old_config = load_vscode_config_with_terminal(&root, None, true);

        let pid = 654;
        Vscode::store_window_state(
            pid,
            VscodeWindowState {
                layout_signature: LayoutSnapshot::new(
                    serde_json::from_str(single_group_layout_json()).expect("layout should parse"),
                )
                .layout_signature(),
                possible_leaves: vec![0],
                surface: FocusSurface::Terminal,
                pending: None,
            },
        );

        let app = Vscode;
        TopologyHandler::move_out(&app, Direction::East, pid)
            .expect("terminal tear-out should succeed");

        let payloads = server.finish();
        assert_eq!(payloads.len(), 3);
        assert!(payloads[0].contains("\"command\":\"vscode.getEditorLayout\""));
        assert!(payloads[1].contains("\"command\":\"workbench.action.terminal.moveIntoNewWindow\""));
        assert!(payloads[2].contains("\"command\":\"vscode.getEditorLayout\""));

        restore_env(super::REMOTE_CONTROL_PORT_ENV, old_port);
        restore_env(super::STATE_FILE_ENV, old_state);
        restore_env(super::FOCUS_SETTLE_ENV, old_settle);
        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn prepare_merge_captures_active_file_path_for_buffer_scope() {
        let _guard = env_guard();
        let root = unique_temp_dir("vscode-prepare-merge-buffer");
        fs::create_dir_all(&root).expect("temp dir should be created");
        let clipboard_file = root.join("clipboard.txt");
        fs::write(&clipboard_file, "previous clipboard").expect("clipboard seed should be written");
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let port = listener
            .local_addr()
            .expect("listener addr should exist")
            .port();
        let expected_path = root.join("active.rs");
        let expected_path_str = expected_path.to_str().expect("utf-8 path").to_string();
        let clipboard_path = clipboard_file.clone();
        let expected_path_for_server = expected_path_str.clone();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("client should connect");
            let mut socket = accept(stream).expect("websocket handshake should succeed");
            let payload = match socket.read().expect("request should be readable") {
                Message::Text(text) => text.to_string(),
                Message::Binary(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                other => panic!("unexpected websocket message: {other:?}"),
            };
            fs::write(&clipboard_path, &expected_path_for_server)
                .expect("clipboard file should update");
            let _ = socket.close(None);
            payload
        });
        let old_port = set_env(super::REMOTE_CONTROL_PORT_ENV, Some(&port.to_string()));
        let old_clipboard = set_clipboard_file(&clipboard_file);
        let old_config = load_vscode_config(&root, Some(EditorTearOffScope::Buffer));

        let app = Vscode;
        let preparation = TopologyHandler::prepare_merge(
            &app,
            Some(ProcessId::new(555).expect("pid should be non-zero")),
        )
        .expect("prepare_merge should succeed");
        let payload = server.join().expect("server thread should join");
        assert!(payload.contains("\"command\":\"copyFilePath\""));
        assert_eq!(
            preparation.into_payload::<VscodeMergePreparation>(),
            Some(VscodeMergePreparation::Buffer {
                path: expected_path_str.clone()
            })
        );
        assert_eq!(
            fs::read_to_string(&clipboard_file).expect("clipboard should be restored"),
            "previous clipboard"
        );

        restore_env(super::REMOTE_CONTROL_PORT_ENV, old_port);
        restore_env(super::TEST_CLIPBOARD_FILE_ENV, old_clipboard);
        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn prepare_merge_falls_back_to_terminal_payload_when_no_file_path_is_exposed() {
        let _guard = env_guard();
        let root = unique_temp_dir("vscode-prepare-merge-terminal");
        fs::create_dir_all(&root).expect("temp dir should be created");
        let clipboard_file = root.join("clipboard.txt");
        fs::write(&clipboard_file, "previous clipboard").expect("clipboard seed should be written");
        let server = TestWsServer::spawn(vec![Some(single_group_layout_json()), None]);
        let old_port = set_env(
            super::REMOTE_CONTROL_PORT_ENV,
            Some(&server.port.to_string()),
        );
        let old_clipboard = set_clipboard_file(&clipboard_file);
        let old_config =
            load_vscode_config_with_terminal(&root, Some(EditorTearOffScope::Buffer), true);

        let app = Vscode;
        let preparation = TopologyHandler::prepare_merge(
            &app,
            Some(ProcessId::new(557).expect("pid should be non-zero")),
        )
        .expect("prepare_merge should treat missing file path as terminal");
        let payloads = server.finish();
        assert_eq!(payloads.len(), 2);
        assert!(payloads[0].contains("\"command\":\"vscode.getEditorLayout\""));
        assert!(payloads[1].contains("\"command\":\"copyFilePath\""));
        assert_eq!(
            preparation.into_payload::<VscodeMergePreparation>(),
            Some(VscodeMergePreparation::Terminal)
        );
        assert_eq!(
            fs::read_to_string(&clipboard_file).expect("clipboard should be restored"),
            "previous clipboard"
        );

        restore_env(super::REMOTE_CONTROL_PORT_ENV, old_port);
        restore_env(super::TEST_CLIPBOARD_FILE_ENV, old_clipboard);
        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn buffer_merge_splits_target_opens_path_and_prunes_duplicate_editor() {
        let _guard = env_guard();
        let root = unique_temp_dir("vscode-buffer-merge");
        fs::create_dir_all(&root).expect("temp dir should be created");
        let state_file = root.join("state.json");
        let server =
            TestWsServer::spawn(vec![None, None, None, None, Some(two_column_layout_json())]);
        let old_port = set_env(
            super::REMOTE_CONTROL_PORT_ENV,
            Some(&server.port.to_string()),
        );
        let old_state = set_env(
            super::STATE_FILE_ENV,
            Some(state_file.to_str().expect("utf-8 path")),
        );
        let old_settle = set_focus_settle_immediate();
        let old_config = load_vscode_config(&root, Some(EditorTearOffScope::Buffer));

        let app = Vscode;
        TopologyHandler::merge_into_target(
            &app,
            Direction::West,
            Some(ProcessId::new(700).expect("pid should be non-zero")),
            Some(ProcessId::new(701).expect("pid should be non-zero")),
            MergePreparation::with_payload(VscodeMergePreparation::Buffer {
                path: "/tmp/merged.rs".to_string(),
            }),
        )
        .expect("buffer merge should succeed");

        let payloads = server.finish();
        assert_eq!(payloads.len(), 5);
        assert!(payloads[0].contains("\"command\":\"workbench.action.splitEditorRight\""));
        assert!(payloads[1].contains("\"command\":\"vscode.open\""));
        assert!(payloads[1].contains("/tmp/merged.rs"));
        assert!(payloads[2].contains("\"command\":\"workbench.action.keepEditor\""));
        assert!(payloads[3].contains("\"command\":\"workbench.action.closeOtherEditors\""));
        assert!(payloads[4].contains("\"command\":\"vscode.getEditorLayout\""));

        restore_env(super::REMOTE_CONTROL_PORT_ENV, old_port);
        restore_env(super::STATE_FILE_ENV, old_state);
        restore_env(super::FOCUS_SETTLE_ENV, old_settle);
        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn prepare_merge_captures_group_paths_for_window_scope() {
        let _guard = env_guard();
        let root = unique_temp_dir("vscode-prepare-merge-group");
        fs::create_dir_all(&root).expect("temp dir should be created");
        let clipboard_file = root.join("clipboard.txt");
        fs::write(&clipboard_file, "previous clipboard").expect("clipboard seed should be written");
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let port = listener
            .local_addr()
            .expect("listener addr should exist")
            .port();
        let clipboard_path = clipboard_file.clone();
        let paths = vec![
            "/tmp/group-a.rs".to_string(),
            "/tmp/group-b.rs".to_string(),
            "/tmp/group-c.rs".to_string(),
        ];
        let expected_paths = paths.clone();
        let paths_for_server = paths.clone();
        let server = thread::spawn(move || {
            let mut current_index = 1usize;
            let mut payloads = Vec::new();
            for _ in 0..10 {
                let (stream, _) = listener.accept().expect("client should connect");
                let mut socket = accept(stream).expect("websocket handshake should succeed");
                let payload = match socket.read().expect("request should be readable") {
                    Message::Text(text) => text.to_string(),
                    Message::Binary(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                    other => panic!("unexpected websocket message: {other:?}"),
                };
                let value: serde_json::Value =
                    serde_json::from_str(&payload).expect("payload json should parse");
                let command = value["command"].as_str().expect("command should exist");
                match command {
                    super::COPY_ACTIVE_FILE_PATH_COMMAND => {
                        fs::write(&clipboard_path, &paths_for_server[current_index])
                            .expect("clipboard file should update");
                    }
                    super::OPEN_EDITOR_AT_INDEX_COMMAND => {
                        let next =
                            value["args"][0].as_u64().expect("index arg should exist") as usize;
                        if next < paths_for_server.len() {
                            current_index = next;
                        }
                    }
                    other => panic!("unexpected command {other}"),
                }
                payloads.push(payload);
                let _ = socket.close(None);
            }
            payloads
        });
        let old_port = set_env(super::REMOTE_CONTROL_PORT_ENV, Some(&port.to_string()));
        let old_clipboard = set_clipboard_file(&clipboard_file);
        let old_config = load_vscode_config(&root, Some(EditorTearOffScope::Window));

        let app = Vscode;
        let preparation = TopologyHandler::prepare_merge(
            &app,
            Some(ProcessId::new(556).expect("pid should be non-zero")),
        )
        .expect("prepare_merge should succeed");
        let payloads = server.join().expect("server thread should join");
        assert_eq!(payloads.len(), 10);
        assert!(payloads[0].contains("\"command\":\"copyFilePath\""));
        assert!(payloads[1].contains("\"command\":\"workbench.action.openEditorAtIndex\""));
        assert!(payloads[9].contains("\"args\":[1]"));
        assert_eq!(
            preparation.into_payload::<VscodeMergePreparation>(),
            Some(VscodeMergePreparation::Group {
                paths: expected_paths,
                active_index: 1,
            })
        );
        assert_eq!(
            fs::read_to_string(&clipboard_file).expect("clipboard should be restored"),
            "previous clipboard"
        );

        restore_env(super::REMOTE_CONTROL_PORT_ENV, old_port);
        restore_env(super::TEST_CLIPBOARD_FILE_ENV, old_clipboard);
        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn group_merge_splits_target_clears_duplicate_and_reopens_tabs() {
        let _guard = env_guard();
        let root = unique_temp_dir("vscode-group-merge");
        fs::create_dir_all(&root).expect("temp dir should be created");
        let state_file = root.join("state.json");
        let server = TestWsServer::spawn(vec![
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(two_column_layout_json()),
        ]);
        let old_port = set_env(
            super::REMOTE_CONTROL_PORT_ENV,
            Some(&server.port.to_string()),
        );
        let old_state = set_env(
            super::STATE_FILE_ENV,
            Some(state_file.to_str().expect("utf-8 path")),
        );
        let old_settle = set_focus_settle_immediate();
        let old_config = load_vscode_config(&root, Some(EditorTearOffScope::Window));

        let app = Vscode;
        TopologyHandler::merge_into_target(
            &app,
            Direction::West,
            Some(ProcessId::new(710).expect("pid should be non-zero")),
            Some(ProcessId::new(711).expect("pid should be non-zero")),
            MergePreparation::with_payload(VscodeMergePreparation::Group {
                paths: vec!["/tmp/group-a.rs".to_string(), "/tmp/group-b.rs".to_string()],
                active_index: 1,
            }),
        )
        .expect("group merge should succeed");

        let payloads = server.finish();
        assert_eq!(payloads.len(), 8);
        assert!(payloads[0].contains("\"command\":\"workbench.action.splitEditorRight\""));
        assert!(payloads[1].contains("\"command\":\"workbench.action.closeActiveEditor\""));
        assert!(payloads[2].contains("\"command\":\"vscode.open\""));
        assert!(payloads[2].contains("/tmp/group-a.rs"));
        assert!(payloads[3].contains("\"command\":\"workbench.action.keepEditor\""));
        assert!(payloads[4].contains("\"command\":\"vscode.open\""));
        assert!(payloads[4].contains("/tmp/group-b.rs"));
        assert!(payloads[5].contains("\"command\":\"workbench.action.keepEditor\""));
        assert!(payloads[6].contains("\"command\":\"workbench.action.openEditorAtIndex\""));
        assert!(payloads[6].contains("\"args\":[1]"));
        assert!(payloads[7].contains("\"command\":\"vscode.getEditorLayout\""));

        restore_env(super::REMOTE_CONTROL_PORT_ENV, old_port);
        restore_env(super::STATE_FILE_ENV, old_state);
        restore_env(super::FOCUS_SETTLE_ENV, old_settle);
        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn terminal_merge_moves_instance_back_to_panel() {
        let _guard = env_guard();
        let root = unique_temp_dir("vscode-terminal-merge");
        fs::create_dir_all(&root).expect("temp dir should be created");
        let state_file = root.join("state.json");
        let server = TestWsServer::spawn(vec![None]);
        let old_port = set_env(
            super::REMOTE_CONTROL_PORT_ENV,
            Some(&server.port.to_string()),
        );
        let old_state = set_env(
            super::STATE_FILE_ENV,
            Some(state_file.to_str().expect("utf-8 path")),
        );
        let old_config = load_vscode_config_with_terminal(&root, None, true);

        let app = Vscode;
        TopologyHandler::merge_into_target(
            &app,
            Direction::West,
            Some(ProcessId::new(712).expect("pid should be non-zero")),
            Some(ProcessId::new(713).expect("pid should be non-zero")),
            MergePreparation::with_payload(VscodeMergePreparation::Terminal),
        )
        .expect("terminal merge should succeed");

        let payloads = server.finish();
        assert_eq!(payloads.len(), 1);
        assert!(
            payloads[0].contains("\"command\":\"workbench.action.terminal.moveToTerminalPanel\"")
        );

        restore_env(super::REMOTE_CONTROL_PORT_ENV, old_port);
        restore_env(super::STATE_FILE_ENV, old_state);
        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn move_decision_errors_when_bridge_is_unavailable() {
        let _guard = env_guard();
        let root = unique_temp_dir("vscode-unavailable");
        fs::create_dir_all(&root).expect("temp dir should be created");
        let state_file = root.join("state.json");

        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let port = listener
            .local_addr()
            .expect("listener addr should exist")
            .port();
        drop(listener);

        let old_port = set_env(super::REMOTE_CONTROL_PORT_ENV, Some(&port.to_string()));
        let old_state = set_env(
            super::STATE_FILE_ENV,
            Some(state_file.to_str().expect("utf-8 path")),
        );

        let app = Vscode;
        let err = TopologyHandler::move_decision(&app, Direction::West, 100)
            .expect_err("move_decision should fail without a bridge");
        assert!(err.to_string().contains("unable to run VS Code command"));

        restore_env(super::REMOTE_CONTROL_PORT_ENV, old_port);
        restore_env(super::STATE_FILE_ENV, old_state);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn merge_errors_when_target_buffer_merge_cannot_contact_vscode() {
        let _guard = env_guard();
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let port = listener
            .local_addr()
            .expect("listener addr should exist")
            .port();
        drop(listener);
        let old_port = set_env(super::REMOTE_CONTROL_PORT_ENV, Some(&port.to_string()));

        let app = Vscode;
        let err = TopologyHandler::merge_into_target(
            &app,
            Direction::West,
            Some(ProcessId::new(424242).expect("pid should be non-zero")),
            Some(ProcessId::new(424243).expect("pid should be non-zero")),
            MergePreparation::with_payload(VscodeMergePreparation::Buffer {
                path: "/tmp/missing.rs".to_string(),
            }),
        )
        .expect_err("merge should fail when split/open cannot contact vscode");
        assert!(
            err.to_string().contains("unable to run VS Code command")
                || err.to_string().contains("failed to connect")
        );

        restore_env(super::REMOTE_CONTROL_PORT_ENV, old_port);
    }

    #[test]
    fn workspace_scope_merge_falls_back_to_restore_after_short_settle() {
        let _guard = env_guard();
        let root = unique_temp_dir("vscode-merge-short-settle");
        fs::create_dir_all(&root).expect("temp dir should be created");
        let state_file = root.join("state.json");
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let port = listener
            .local_addr()
            .expect("listener addr should exist")
            .port();
        let (payload_tx, payload_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let server_handle = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("client should connect");
            let mut socket = accept(stream).expect("websocket handshake should succeed");
            let payload = match socket.read().expect("request should be readable") {
                Message::Text(text) => text.to_string(),
                Message::Binary(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                other => panic!("unexpected websocket message: {other:?}"),
            };
            payload_tx
                .send(payload)
                .expect("payload should be captured");
            release_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("test should release server");
            let _ = socket.close(None);
        });
        let old_port = set_env(super::REMOTE_CONTROL_PORT_ENV, Some(&port.to_string()));
        let old_state = set_env(
            super::STATE_FILE_ENV,
            Some(state_file.to_str().expect("utf-8 path")),
        );
        let old_config = load_vscode_config(&root, Some(EditorTearOffScope::Workspace));
        let pid = 424243;
        Vscode::store_window_state(
            pid,
            VscodeWindowState {
                layout_signature: "test-layout".to_string(),
                possible_leaves: vec![0],
                surface: FocusSurface::Editors,
                pending: None,
            },
        );

        let (done_tx, done_rx) = mpsc::channel();
        let merge_handle = thread::spawn(move || {
            let app = Vscode;
            let result = TopologyHandler::merge_into_target(
                &app,
                Direction::West,
                Some(ProcessId::new(pid).expect("pid should be non-zero")),
                Some(ProcessId::new(424244).expect("pid should be non-zero")),
                MergePreparation::with_payload(VscodeMergePreparation::RestoreMainWindow {
                    scope: EditorTearOffScope::Workspace,
                }),
            )
            .map_err(|err| format!("{err:#}"));
            done_tx.send(result).expect("merge result should be sent");
        });

        let payload = payload_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("payload should arrive");
        assert!(payload.contains("\"command\":\"workbench.action.restoreEditorsToMainWindow\""));
        let result = done_rx
            .recv_timeout(Duration::from_millis(500))
            .expect("merge should return after a short fixed settle");
        assert!(result.is_ok(), "merge should succeed: {result:?}");
        assert!(
            !read_state_file(&state_file).contains(&format!("\"{pid}\"")),
            "merge should clear the cached VS Code window state"
        );

        release_tx.send(()).expect("server should be released");
        merge_handle
            .join()
            .expect("merge thread should join cleanly");
        server_handle
            .join()
            .expect("server thread should join cleanly");

        restore_env(super::REMOTE_CONTROL_PORT_ENV, old_port);
        restore_env(super::STATE_FILE_ENV, old_state);
        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }
}
