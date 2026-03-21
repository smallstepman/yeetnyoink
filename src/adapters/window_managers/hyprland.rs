//! Hyprland window manager adapter for Linux.
//!
//! Hyprland is a dynamic tiling Wayland compositor.
//! This adapter communicates via `hyprctl` CLI and socket API.

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::config::WmBackend;
use crate::engine::runtime::{self, CommandContext, ProcessId};
use crate::engine::topology::Direction;
use crate::engine::wm::{
    validate_declared_capabilities, CapabilitySupport, ConfiguredWindowManager,
    DirectionalCapability, FocusedWindowRecord, PrimitiveWindowManagerCapabilities, ResizeIntent,
    ResizeKind, WindowManagerCapabilities, WindowManagerCapabilityDescriptor,
    WindowManagerFeatures, WindowManagerSession, WindowManagerSpec, WindowRecord,
};

pub struct HyprlandAdapter {
    transport: Box<dyn HyprlandTransport>,
}

pub struct HyprlandSpec;

pub static HYPRLAND_SPEC: HyprlandSpec = HyprlandSpec;

impl WindowManagerSpec for HyprlandSpec {
    fn backend(&self) -> WmBackend {
        WmBackend::Hyprland
    }

    fn name(&self) -> &'static str {
        HyprlandAdapter::NAME
    }

    fn connect(&self) -> Result<ConfiguredWindowManager> {
        ConfiguredWindowManager::try_new(
            Box::new(HyprlandAdapter::connect()?),
            WindowManagerFeatures::default(),
        )
    }
}

/// Trait for dispatching Hyprland commands.
/// Default runtime uses real hyprctl; tests inject a mock transport.
trait HyprlandTransport: Send {
    fn execute(&mut self, action: &'static str, args: Vec<String>) -> Result<String>;
}

/// Real transport: executes `hyprctl` with arguments.
struct RealTransport;

impl HyprlandTransport for RealTransport {
    fn execute(&mut self, action: &'static str, args: Vec<String>) -> Result<String> {
        let args_strs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let output = runtime::run_command_output(
            "hyprctl",
            &args_strs,
            &CommandContext::new(HyprlandAdapter::NAME, action),
        )?;
        if !output.status.success() {
            anyhow::bail!("hyprctl failed: {}", runtime::stderr_text(&output));
        }
        Ok(runtime::stdout_text(&output))
    }
}

impl HyprlandAdapter {
    pub fn connect() -> Result<Self> {
        validate_declared_capabilities::<Self>()?;
        // TODO: Verify hyprctl is available and Hyprland is running
        Ok(Self {
            transport: Box::new(RealTransport),
        })
    }
}

impl WindowManagerCapabilityDescriptor for HyprlandAdapter {
    const NAME: &'static str = "hyprland";
    const CAPABILITIES: WindowManagerCapabilities = WindowManagerCapabilities {
        primitives: PrimitiveWindowManagerCapabilities {
            tear_out_right: false,
            move_column: false,
            consume_into_column_and_move: false,
            set_window_width: true,
            set_window_height: true,
        },
        tear_out: DirectionalCapability::uniform(CapabilitySupport::Unsupported),
        resize: DirectionalCapability::uniform(CapabilitySupport::Native),
    };
}

impl WindowManagerSession for HyprlandAdapter {
    fn adapter_name(&self) -> &'static str {
        Self::NAME
    }

    fn capabilities(&self) -> WindowManagerCapabilities {
        Self::CAPABILITIES
    }

    fn focused_window(&mut self) -> Result<FocusedWindowRecord> {
        let output = self.transport.execute("focused_window", vec!["-j".into(), "activewindow".into()])?;
        let active: HyprlandClient = serde_json::from_str(&output)
            .context("failed to parse activewindow JSON")?;
        
        // Check if this is the null sentinel (empty workspace case).
        // Note: This returns early with an error rather than filtering like windows()
        // does, which is acceptable because callers expect at least one focused window
        // or an error, not an empty list.
        if is_null_activewindow(&active) {
            anyhow::bail!("no focused window");
        }
        
        let id = parse_window_address(&active.address)?;
        let pid = active.process_id();
        Ok(FocusedWindowRecord {
            id,
            app_id: active.class,
            title: active.title,
            pid,
            original_tile_index: 1,
        })
    }

    fn windows(&mut self) -> Result<Vec<WindowRecord>> {
        let active_json = self.transport.execute("windows_activewindow", vec!["-j".into(), "activewindow".into()])?;
        let clients_json = self.transport.execute("windows_clients", vec!["-j".into(), "clients".into()])?;
        parse_clients_with_focus(&active_json, &clients_json)
    }

    fn focus_direction(&mut self, direction: Direction) -> Result<()> {
        self.transport.execute(
            "focus_direction",
            vec![
                "dispatch".into(),
                "movefocus".into(),
                direction_to_hyprland(direction).into(),
            ],
        )?;
        Ok(())
    }

    fn move_direction(&mut self, direction: Direction) -> Result<()> {
        self.transport.execute(
            "move_direction",
            vec![
                "dispatch".into(),
                "movewindow".into(),
                direction_to_hyprland(direction).into(),
            ],
        )?;
        Ok(())
    }

    fn resize_with_intent(&mut self, intent: ResizeIntent) -> Result<()> {
        let grow = matches!(intent.kind, ResizeKind::Grow);
        let (dx, dy) = resize_delta(intent.direction, grow, intent.step);
        self.transport.execute(
            "resize_with_intent",
            vec![
                "dispatch".into(),
                "resizeactive".into(),
                dx.to_string(),
                dy.to_string(),
            ],
        )?;
        Ok(())
    }

    fn spawn(&mut self, command: Vec<String>) -> Result<()> {
        let joined = command.iter().map(|arg| shell_quote(arg)).collect::<Vec<_>>().join(" ");
        self.transport.execute(
            "spawn",
            vec![
                "dispatch".into(),
                "exec".into(),
                joined,
            ],
        )?;
        Ok(())
    }

    fn focus_window_by_id(&mut self, id: u64) -> Result<()> {
        self.transport.execute(
            "focus_window_by_id",
            vec![
                "dispatch".into(),
                "focuswindow".into(),
                format_window_selector(id),
            ],
        )?;
        Ok(())
    }

    fn close_window_by_id(&mut self, id: u64) -> Result<()> {
        self.transport.execute(
            "close_window_by_id",
            vec![
                "dispatch".into(),
                "closewindow".into(),
                format_window_selector(id),
            ],
        )?;
        Ok(())
    }
}

fn parse_window_address(raw: &str) -> Result<u64> {
    let trimmed = raw.trim().strip_prefix("0x").unwrap_or(raw.trim());
    u64::from_str_radix(trimmed, 16).context("invalid Hyprland window address")
}

fn format_window_selector(id: u64) -> String {
    format!("address:0x{id:x}")
}

fn direction_to_hyprland(dir: Direction) -> &'static str {
    match dir {
        Direction::West => "l",
        Direction::East => "r",
        Direction::North => "u",
        Direction::South => "d",
    }
}

fn parse_clients_with_focus(
    active_json: &str,
    clients_json: &str,
) -> Result<Vec<WindowRecord>> {
    let active: HyprlandClient = serde_json::from_str(active_json)
        .context("failed to parse active window JSON")?;
    
    // Determine the focused window ID, if any
    let active_addr = if is_null_activewindow(&active) {
        None
    } else {
        Some(parse_window_address(&active.address)?)
    };

    let clients: Vec<HyprlandClient> = serde_json::from_str(clients_json)
        .context("failed to parse clients JSON")?;

    let mut windows = Vec::new();
    for client in clients {
        // Skip the null sentinel (Hyprland's empty workspace indicator) and unmapped windows.
        // Only mapped == Some(false) is excluded; mapped: None is allowed for compatibility
        // with Hyprland payloads that omit the field.
        if is_null_activewindow(&client) || client.mapped == Some(false) {
            continue;
        }
        let id = parse_window_address(&client.address)?;
        let is_focused = active_addr == Some(id);
        let pid = client.process_id();
        windows.push(WindowRecord {
            id,
            app_id: client.class,
            title: client.title,
            pid,
            original_tile_index: 1,
            is_focused,
        });
    }
    Ok(windows)
}

/// Returns true if the client represents Hyprland's null activewindow sentinel
/// (appears on empty workspaces with address "((null))").
fn is_null_activewindow(client: &HyprlandClient) -> bool {
    client.address == "((null))"
}

// Hyprland (Wayland compositor) uses a coordinate system where the Y axis
// grows downward (positive Y points down). Therefore when we "grow" north
// (i.e., expand the window upward) we must apply a negative Y delta.
fn resize_delta(direction: Direction, grow: bool, step: i32) -> (i32, i32) {
    let step = step.abs().max(1);
    match (direction, grow) {
        (Direction::West, true) => (-step, 0),
        (Direction::West, false) => (step, 0),
        (Direction::East, true) => (step, 0),
        (Direction::East, false) => (-step, 0),
        (Direction::North, true) => (0, -step),
        (Direction::North, false) => (0, step),
        (Direction::South, true) => (0, step),
        (Direction::South, false) => (0, -step),
    }
}

#[derive(Debug, Deserialize)]
struct HyprlandClient {
    address: String,
    #[serde(default)]
    class: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    pid: Option<u32>,
    #[serde(default)]
    mapped: Option<bool>,
}

impl HyprlandClient {
    /// Convert the raw pid (u32) from Hyprland into a domain ProcessId.
    /// Returns None when pid is missing or zero.
    fn process_id(&self) -> Option<ProcessId> {
        self.pid.and_then(ProcessId::new)
    }
}

/// Safely quotes a string argument for shell execution.
/// Wraps the argument in single quotes and escapes any embedded single quotes.
fn shell_quote(arg: &str) -> String {
    if arg.is_empty() {
        return "''".to_string();
    }
    // If the argument contains no special characters, return it as-is.
    if arg.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.' || c == '/') {
        return arg.to_string();
    }
    // Otherwise, wrap in single quotes and escape any embedded single quotes.
    format!("'{}'", arg.replace('\'', "'\\''"))
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::topology::Direction;
    use crate::engine::wm::ResizeKind;
    use std::sync::{Arc, Mutex};

    type CallLog = Arc<Mutex<Vec<Vec<String>>>>;
    type ResponseMap = Arc<Mutex<std::collections::HashMap<Vec<String>, String>>>;

    // MockTransport records dispatched commands and can serve canned query responses.
    struct MockTransport {
        calls: CallLog,
        responses: ResponseMap,
    }

    impl MockTransport {
        fn new(calls: CallLog) -> Self {
            Self { calls, responses: Arc::new(Mutex::new(std::collections::HashMap::new())) }
        }

        fn with_responses(calls: CallLog, responses: ResponseMap) -> Self {
            Self { calls, responses }
        }
    }

    impl HyprlandTransport for MockTransport {
        fn execute(&mut self, _action: &'static str, args: Vec<String>) -> Result<String> {
            self.calls.lock().unwrap().push(args.clone());
            // Check if we have a canned response for this query
            if let Some(response) = self.responses.lock().unwrap().get(&args) {
                return Ok(response.clone());
            }
            // dispatch commands don't need a response
            if args.first().map(|s| s.as_str()) == Some("dispatch") {
                Ok(String::new())
            } else {
                anyhow::bail!("unexpected query command in mock without canned response: {:?}", args)
            }
        }
    }

    fn test_adapter() -> (HyprlandAdapter, CallLog) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let adapter = HyprlandAdapter {
            transport: Box::new(MockTransport::new(Arc::clone(&calls))),
        };
        (adapter, calls)
    }

    fn test_adapter_with_responses(responses: ResponseMap) -> (HyprlandAdapter, CallLog) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let adapter = HyprlandAdapter {
            transport: Box::new(MockTransport::with_responses(Arc::clone(&calls), responses)),
        };
        (adapter, calls)
    }

    #[test]
    fn hyprland_marks_focused_client_from_activewindow_address() {
        let active = r#"{"address":"0x20","class":"foot","title":"shell","pid":200}"#;
        let clients = r#"[{"address":"0x10","class":"firefox","title":"docs","pid":100,"mapped":true},{"address":"0x20","class":"foot","title":"shell","pid":200,"mapped":true}]"#;

        let windows = parse_clients_with_focus(active, clients).unwrap();
        assert!(windows.iter().any(|window| window.id == 0x20 && window.is_focused));
    }

    #[test]
    fn hyprland_close_window_by_id_dispatches_expected_command() {
        let (mut adapter, calls) = test_adapter();
        adapter.close_window_by_id(0x2a).unwrap();
        let calls = calls.lock().unwrap();
        assert_eq!(
            calls.as_slice(),
            &[vec!["dispatch", "closewindow", "address:0x2a"]]
        );
    }

    #[test]
    fn hyprland_focus_direction_dispatches_movefocus_north() {
        let (mut adapter, calls) = test_adapter();
        adapter.focus_direction(Direction::North).unwrap();
        let calls = calls.lock().unwrap();
        assert_eq!(
            calls.as_slice(),
            &[vec!["dispatch", "movefocus", "u"]]
        );
    }

    #[test]
    fn hyprland_focus_window_by_id_dispatches_expected_command() {
        let (mut adapter, calls) = test_adapter();
        adapter.focus_window_by_id(0x1ff).unwrap();
        let calls = calls.lock().unwrap();
        assert_eq!(
            calls.as_slice(),
            &[vec!["dispatch", "focuswindow", "address:0x1ff"]]
        );
    }

    #[test]
    fn hyprland_resize_with_intent_dispatches_signed_resizeactive_delta() {
        let (mut adapter, calls) = test_adapter();
        adapter
            .resize_with_intent(ResizeIntent::new(Direction::East, ResizeKind::Grow, 40))
            .unwrap();
        let calls = calls.lock().unwrap();
        assert_eq!(
            calls.as_slice(),
            &[vec!["dispatch", "resizeactive", "40", "0"]]
        );
    }

    #[test]
    fn hyprland_move_and_spawn_dispatch_expected_commands() {
        let (mut adapter, calls) = test_adapter();
        adapter.move_direction(Direction::East).unwrap();
        adapter.spawn(vec!["foot".into(), "--app-id".into(), "smoke".into()]).unwrap();
        let calls = calls.lock().unwrap();
        assert_eq!(
            calls.as_slice(),
            &[
                vec!["dispatch", "movewindow", "r"],
                vec!["dispatch", "exec", "foot --app-id smoke"],
            ]
        );
    }

    #[test]
    fn hyprland_spawn_quotes_arguments_with_spaces() {
        let (mut adapter, calls) = test_adapter();
        adapter.spawn(vec![
            "bash".into(),
            "-c".into(),
            "echo hello world".into(),
        ]).unwrap();
        let calls = calls.lock().unwrap();
        assert_eq!(
            calls.as_slice(),
            &[vec!["dispatch", "exec", "bash -c 'echo hello world'"]]
        );
    }

    #[test]
    fn hyprland_parses_window_address_hex() {
        assert_eq!(parse_window_address("0x2a").unwrap(), 0x2a);
    }

    #[test]
    fn hyprland_parses_window_address_bare_hex() {
        assert_eq!(parse_window_address("2a").unwrap(), 0x2a);
        assert_eq!(parse_window_address("  2a  ").unwrap(), 0x2a);
    }

    #[test]
    fn hyprland_formats_window_selector() {
        assert_eq!(format_window_selector(0x2a), "address:0x2a");
    }

    #[test]
    fn hyprland_resize_delta_matches_direction_and_intent() {
        let s = 40;
        // East
        assert_eq!(resize_delta(Direction::East, true, s), (s, 0));
        assert_eq!(resize_delta(Direction::East, false, s), (-s, 0));
        // West
        assert_eq!(resize_delta(Direction::West, true, s), (-s, 0));
        assert_eq!(resize_delta(Direction::West, false, s), (s, 0));
        // North
        assert_eq!(resize_delta(Direction::North, true, s), (0, -s));
        assert_eq!(resize_delta(Direction::North, false, s), (0, s));
        // South
        assert_eq!(resize_delta(Direction::South, true, s), (0, s));
        assert_eq!(resize_delta(Direction::South, false, s), (0, -s));
    }

    #[test]
    fn hyprland_parses_activewindow_json() {
        let sample = r#"{
            "address": "0xaaaae329c5d0",
            "mapped": true,
            "hidden": false,
            "at": [910, 18],
            "size": [872, 1094],
            "workspace": { "id": 1, "name": "1" },
            "floating": false,
            "pseudo": false,
            "monitor": 0,
            "class": "foot",
            "title": "Investigating Hyprland",
            "initialClass": "foot",
            "initialTitle": "foot",
            "pid": 9316,
            "xwayland": false,
            "pinned": false,
            "fullscreen": 0,
            "fullscreenClient": 0,
            "grouped": [],
            "tags": [],
            "swallowing": "0x0",
            "focusHistoryID": 0,
            "inhibitingIdle": false,
            "xdgTag": "",
            "xdgDescription": ""
        }"#;
        let window: HyprlandClient = serde_json::from_str(sample).unwrap();
        assert_eq!(window.address, "0xaaaae329c5d0");
        assert_eq!(window.class.as_deref(), Some("foot"));
        assert_eq!(window.title.as_deref(), Some("Investigating Hyprland"));
        assert_eq!(window.pid, Some(9316));
        assert_eq!(window.mapped, Some(true));
        // helper should convert to a domain ProcessId
        assert_eq!(window.process_id(), ProcessId::new(9316));
    }

    #[test]
    fn hyprland_capabilities_are_valid() {
        use crate::engine::wm::validate_declared_capabilities;
        validate_declared_capabilities::<HyprlandAdapter>()
            .expect("hyprland capability descriptor should be valid");
    }

    #[test]
    fn hyprland_parses_json_without_pid() {
        let sample = r#"{
            "address": "0x1234",
            "mapped": false,
            "class": "foot",
            "title": "No PID"
        }"#;
        let window: HyprlandClient = serde_json::from_str(sample).unwrap();
        assert_eq!(window.address, "0x1234");
        assert_eq!(window.pid, None);
        assert_eq!(window.process_id(), None);
    }

    #[test]
    fn hyprland_parse_clients_tolerates_null_activewindow_sentinel() {
        let active = r#"{"address":"((null))","mapped":false,"class":null,"title":null,"pid":null}"#;
        let clients = r#"[{"address":"0x10","class":"firefox","title":"docs","pid":100,"mapped":true}]"#;

        let windows = parse_clients_with_focus(active, clients).unwrap();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].id, 0x10);
        assert!(!windows[0].is_focused); // No focused window
    }

    #[test]
    fn hyprland_parse_clients_skips_null_sentinel_in_clients_list() {
        let active = r#"{"address":"0x20","class":"foot","title":"shell","pid":200,"mapped":true}"#;
        let clients = r#"[
            {"address":"0x10","class":"firefox","title":"docs","pid":100,"mapped":true},
            {"address":"((null))","mapped":false,"class":null,"title":null,"pid":null},
            {"address":"0x20","class":"foot","title":"shell","pid":200,"mapped":true}
        ]"#;

        let windows = parse_clients_with_focus(active, clients).unwrap();
        assert_eq!(windows.len(), 2);
        assert!(windows.iter().any(|w| w.id == 0x10));
        assert!(windows.iter().any(|w| w.id == 0x20 && w.is_focused));
        // Verify null sentinel was filtered out (only 2 windows, not 3)
    }

    #[test]
    fn hyprland_parse_clients_with_empty_workspace_returns_empty_list() {
        let active = r#"{"address":"((null))","mapped":false,"class":null,"title":null,"pid":null}"#;
        let clients = r#"[]"#;

        let windows = parse_clients_with_focus(active, clients).unwrap();
        assert!(windows.is_empty());
    }

    #[test]
    fn hyprland_is_null_activewindow_recognizes_sentinel() {
        let null_window = HyprlandClient {
            address: "((null))".into(),
            class: None,
            title: None,
            pid: None,
            mapped: Some(false),
        };
        assert!(is_null_activewindow(&null_window));
    }

    #[test]
    fn hyprland_is_null_activewindow_rejects_real_windows() {
        let real_window = HyprlandClient {
            address: "0x1234".into(),
            class: Some("foot".into()),
            title: Some("shell".into()),
            pid: Some(100),
            mapped: Some(true),
        };
        assert!(!is_null_activewindow(&real_window));
    }

    #[test]
    fn hyprland_focused_window_queries_activewindow() {
        let responses = Arc::new(Mutex::new(std::collections::HashMap::from([
            (vec!["-j".into(), "activewindow".into()],
             r#"{"address":"0x20","class":"foot","title":"shell","pid":200,"mapped":true}"#.into()),
        ])));
        let (mut adapter, calls) = test_adapter_with_responses(responses);

        let focused = adapter.focused_window().unwrap();
        assert_eq!(focused.id, 0x20);
        assert_eq!(focused.app_id.as_deref(), Some("foot"));
        assert_eq!(focused.title.as_deref(), Some("shell"));

        let calls = calls.lock().unwrap();
        assert_eq!(calls.as_slice(), &[vec!["-j", "activewindow"]]);
    }

    #[test]
    fn hyprland_focused_window_errors_on_null_activewindow() {
        let responses = Arc::new(Mutex::new(std::collections::HashMap::from([
            (vec!["-j".into(), "activewindow".into()],
             r#"{"address":"((null))","mapped":false,"class":null,"title":null,"pid":null}"#.into()),
        ])));
        let (mut adapter, _calls) = test_adapter_with_responses(responses);

        let result = adapter.focused_window();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no focused window"));
    }

    #[test]
    fn hyprland_windows_queries_activewindow_then_clients() {
        let responses = Arc::new(Mutex::new(std::collections::HashMap::from([
            (vec!["-j".into(), "activewindow".into()],
             r#"{"address":"0x20","class":"foot","title":"shell","pid":200,"mapped":true}"#.into()),
            (vec!["-j".into(), "clients".into()],
             r#"[{"address":"0x10","class":"firefox","title":"docs","pid":100,"mapped":true},{"address":"0x20","class":"foot","title":"shell","pid":200,"mapped":true}]"#.into()),
        ])));
        let (mut adapter, calls) = test_adapter_with_responses(responses);

        let windows = adapter.windows().unwrap();
        assert_eq!(windows.len(), 2);
        assert!(windows.iter().any(|w| w.id == 0x10 && !w.is_focused));
        assert!(windows.iter().any(|w| w.id == 0x20 && w.is_focused));

        let calls = calls.lock().unwrap();
        assert_eq!(
            calls.as_slice(),
            &[vec!["-j", "activewindow"], vec!["-j", "clients"]]
        );
    }
}
