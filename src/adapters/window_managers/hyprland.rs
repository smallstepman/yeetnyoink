//! Hyprland window manager adapter for Linux.
//!
//! Hyprland is a dynamic tiling Wayland compositor.
//! This adapter communicates via `hyprctl` CLI and socket API.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::any::Any;

use crate::config::WmBackend;
use crate::engine::runtime::ProcessId;
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
pub trait HyprlandTransport: Send {
    fn execute(&mut self, args: Vec<String>) -> Result<String>;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// Real transport: executes `hyprctl` with arguments.
struct RealTransport;

impl HyprlandTransport for RealTransport {
    fn execute(&mut self, args: Vec<String>) -> Result<String> {
        let output = std::process::Command::new("hyprctl")
            .args(&args)
            .output()
            .context("failed to execute hyprctl")?;
        if !output.status.success() {
            anyhow::bail!(
                "hyprctl failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(String::from_utf8(output.stdout)?)
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
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
        let output = self.transport.execute(vec!["-j".into(), "activewindow".into()])?;
        let active: HyprlandClient = serde_json::from_str(&output)
            .context("failed to parse activewindow JSON")?;
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
        let active_json = self.transport.execute(vec!["-j".into(), "activewindow".into()])?;
        let clients_json = self.transport.execute(vec!["-j".into(), "clients".into()])?;
        parse_clients_with_focus(&active_json, &clients_json)
    }

    fn focus_direction(&mut self, direction: Direction) -> Result<()> {
        self.transport.execute(vec![
            "dispatch".into(),
            "movefocus".into(),
            direction_to_hyprland(direction).into(),
        ])?;
        Ok(())
    }

    fn move_direction(&mut self, direction: Direction) -> Result<()> {
        self.transport.execute(vec![
            "dispatch".into(),
            "movewindow".into(),
            direction_to_hyprland(direction).into(),
        ])?;
        Ok(())
    }

    fn resize_with_intent(&mut self, intent: ResizeIntent) -> Result<()> {
        let grow = matches!(intent.kind, ResizeKind::Grow);
        let (dx, dy) = resize_delta(intent.direction, grow, intent.step);
        self.transport.execute(vec![
            "dispatch".into(),
            "resizeactive".into(),
            dx.to_string(),
            dy.to_string(),
        ])?;
        Ok(())
    }

    fn spawn(&mut self, command: Vec<String>) -> Result<()> {
        let joined = command.join(" ");
        self.transport.execute(vec![
            "dispatch".into(),
            "exec".into(),
            joined,
        ])?;
        Ok(())
    }

    fn focus_window_by_id(&mut self, id: u64) -> Result<()> {
        self.transport.execute(vec![
            "dispatch".into(),
            "focuswindow".into(),
            format_window_selector(id),
        ])?;
        Ok(())
    }

    fn close_window_by_id(&mut self, id: u64) -> Result<()> {
        self.transport.execute(vec![
            "dispatch".into(),
            "closewindow".into(),
            format_window_selector(id),
        ])?;
        Ok(())
    }
}

pub fn parse_window_address(raw: &str) -> Result<u64> {
    let trimmed = raw.trim().strip_prefix("0x").unwrap_or(raw.trim());
    u64::from_str_radix(trimmed, 16).context("invalid Hyprland window address")
}

pub fn format_window_selector(id: u64) -> String {
    format!("address:0x{id:x}")
}

pub fn direction_to_hyprland(dir: Direction) -> &'static str {
    match dir {
        Direction::West => "l",
        Direction::East => "r",
        Direction::North => "u",
        Direction::South => "d",
    }
}

pub fn parse_clients_with_focus(
    active_json: &str,
    clients_json: &str,
) -> Result<Vec<WindowRecord>> {
    let active: HyprlandClient = serde_json::from_str(active_json)
        .context("failed to parse active window JSON")?;
    let active_addr = parse_window_address(&active.address)?;

    let clients: Vec<HyprlandClient> = serde_json::from_str(clients_json)
        .context("failed to parse clients JSON")?;

    let mut windows = Vec::new();
    for client in clients {
        if client.mapped == Some(false) {
            continue;
        }
        let id = parse_window_address(&client.address)?;
        let is_focused = id == active_addr;
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

// Hyprland (Wayland compositor) uses a coordinate system where the Y axis
// grows downward (positive Y points down). Therefore when we "grow" north
// (i.e., expand the window upward) we must apply a negative Y delta.
pub fn resize_delta(direction: Direction, grow: bool, step: i32) -> (i32, i32) {
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
pub struct HyprlandClient {
    pub address: String,
    #[serde(default)]
    pub class: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub pid: Option<u32>,
    #[serde(default)]
    pub mapped: Option<bool>,
}

impl HyprlandClient {
    /// Convert the raw pid (u32) from Hyprland into a domain ProcessId.
    /// Returns None when pid is missing or zero.
    pub fn process_id(&self) -> Option<ProcessId> {
        self.pid.and_then(ProcessId::new)
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::topology::Direction;
    use crate::engine::wm::ResizeKind;
    use std::any::Any;
    use std::cell::RefCell;

    #[derive(Default)]
    struct MockTransport {
        calls: RefCell<Vec<Vec<String>>>,
    }

    impl HyprlandTransport for MockTransport {
        fn execute(&mut self, args: Vec<String>) -> Result<String> {
            self.calls.borrow_mut().push(args.clone());
            // dispatch commands don't need a response
            if args.first().map(|s| s.as_str()) == Some("dispatch") {
                Ok(String::new())
            } else {
                anyhow::bail!("unexpected non-dispatch command in mock: {:?}", args)
            }
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    fn test_adapter() -> HyprlandAdapter {
        HyprlandAdapter {
            transport: Box::new(MockTransport::default()),
        }
    }

    impl HyprlandAdapter {
        fn take_status_calls(&mut self) -> Vec<Vec<String>> {
            if let Some(mock) = self.transport.as_any_mut().downcast_mut::<MockTransport>() {
                std::mem::take(&mut *mock.calls.borrow_mut())
            } else {
                vec![]
            }
        }
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
        let mut adapter = test_adapter();
        adapter.close_window_by_id(0x2a).unwrap();
        assert_eq!(
            adapter.take_status_calls(),
            vec![vec!["dispatch", "closewindow", "address:0x2a"]]
        );
    }

    #[test]
    fn hyprland_resize_with_intent_dispatches_signed_resizeactive_delta() {
        let mut adapter = test_adapter();
        adapter
            .resize_with_intent(ResizeIntent::new(Direction::East, ResizeKind::Grow, 40))
            .unwrap();
        assert_eq!(
            adapter.take_status_calls(),
            vec![vec!["dispatch", "resizeactive", "40", "0"]]
        );
    }

    #[test]
    fn hyprland_move_and_spawn_dispatch_expected_commands() {
        let mut adapter = test_adapter();
        adapter.move_direction(Direction::East).unwrap();
        adapter.spawn(vec!["foot".into(), "--app-id".into(), "smoke".into()]).unwrap();
        assert_eq!(
            adapter.take_status_calls(),
            vec![
                vec!["dispatch", "movewindow", "r"],
                vec!["dispatch", "exec", "foot --app-id smoke"],
            ]
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
}
