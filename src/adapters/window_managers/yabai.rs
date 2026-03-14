//! Yabai window manager adapter for macOS.
//!
//! Yabai is a tiling window manager for macOS. This adapter communicates
//! with yabai via the `yabai -m` command-line interface.

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::adapters::window_managers::{
    validate_declared_capabilities, CapabilitySupport, ConfiguredWindowManager,
    DirectionalCapability, FocusedWindowRecord, PrimitiveWindowManagerCapabilities, ResizeIntent,
    WindowManagerCapabilities, WindowManagerCapabilityDescriptor, WindowManagerFeatures,
    WindowManagerSession, WindowManagerSpec, WindowRecord,
};
use crate::config::WmBackend;
use crate::engine::runtime::{self, CommandContext, ProcessId};
use crate::engine::topology::Direction;

pub struct YabaiAdapter;

pub struct YabaiSpec;

pub static YABAI_SPEC: YabaiSpec = YabaiSpec;

impl WindowManagerSpec for YabaiSpec {
    fn backend(&self) -> WmBackend {
        WmBackend::Yabai
    }

    fn name(&self) -> &'static str {
        YabaiAdapter::NAME
    }

    fn connect(&self) -> Result<ConfiguredWindowManager> {
        Ok(ConfiguredWindowManager::new(
            Box::new(YabaiAdapter::connect()?),
            WindowManagerFeatures::default(),
        ))
    }
}

impl YabaiAdapter {
    pub fn connect() -> Result<Self> {
        validate_declared_capabilities::<Self>()?;
        // Verify yabai is running by querying displays
        let output = Self::command_output("connect", &["query", "--displays"])?;
        if !output.status.success() {
            bail!(
                "yabai is not running or not accessible: {}",
                runtime::stderr_text(&output)
            );
        }
        Ok(Self)
    }

    fn command_output(action: &'static str, args: &[&str]) -> Result<std::process::Output> {
        runtime::run_command_output("yabai", &[&["-m"], args].concat(), &CommandContext::new(Self::NAME, action))
    }

    fn command_status(action: &'static str, args: &[&str]) -> Result<()> {
        runtime::run_command_status("yabai", &[&["-m"], args].concat(), &CommandContext::new(Self::NAME, action))
    }

    fn direction_name(direction: Direction) -> &'static str {
        match direction {
            Direction::West => "west",
            Direction::East => "east",
            Direction::North => "north",
            Direction::South => "south",
        }
    }

    fn query_windows(&mut self) -> Result<Vec<YabaiWindow>> {
        let output = Self::command_output("query_windows", &["query", "--windows"])?;
        if !output.status.success() {
            bail!(
                "yabai query --windows failed: {}",
                runtime::stderr_text(&output)
            );
        }
        serde_json::from_slice(&output.stdout).context("failed to parse yabai windows json")
    }

    fn focused_window_data(&mut self) -> Result<YabaiWindowData> {
        let windows = self.query_windows()?;
        windows
            .into_iter()
            .find(|w| w.has_focus)
            .map(YabaiWindowData::from)
            .context("no focused yabai window found")
    }
}

/// Raw window data from yabai JSON output.
#[derive(Debug, Clone, Deserialize)]
struct YabaiWindow {
    id: u64,
    #[serde(default)]
    pid: u32,
    #[serde(default)]
    app: String,
    #[serde(default)]
    title: String,
    #[serde(default, rename = "has-focus")]
    has_focus: bool,
    #[serde(default)]
    space: u32,
    #[serde(default)]
    display: u32,
    #[serde(default, rename = "is-floating")]
    is_floating: bool,
    #[serde(default, rename = "is-minimized")]
    is_minimized: bool,
    // Geometry fields for potential future use
    #[serde(default)]
    frame: YabaiFrame,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct YabaiFrame {
    #[serde(default)]
    x: f64,
    #[serde(default)]
    y: f64,
    #[serde(default)]
    w: f64,
    #[serde(default)]
    h: f64,
}

/// Processed window data.
#[derive(Clone)]
struct YabaiWindowData {
    id: u64,
    app_id: Option<String>,
    title: Option<String>,
    pid: Option<ProcessId>,
    is_focused: bool,
}

impl From<YabaiWindow> for YabaiWindowData {
    fn from(window: YabaiWindow) -> Self {
        Self {
            id: window.id,
            app_id: non_empty(window.app),
            title: non_empty(window.title),
            pid: ProcessId::new(window.pid),
            is_focused: window.has_focus,
        }
    }
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

impl WindowManagerCapabilityDescriptor for YabaiAdapter {
    const NAME: &'static str = "yabai";
    const CAPABILITIES: WindowManagerCapabilities = WindowManagerCapabilities {
        primitives: PrimitiveWindowManagerCapabilities {
            // Yabai doesn't have niri-style column operations
            tear_out_right: false,
            move_column: false,
            consume_into_column_and_move: false,
            // Yabai supports resize via --resize
            set_window_width: true,
            set_window_height: true,
        },
        // Tear-out requires moving window to another space/display
        // This is possible via `yabai -m window --space` but not directional
        tear_out: DirectionalCapability::uniform(CapabilitySupport::Unsupported),
        // Resize is natively supported in all directions
        resize: DirectionalCapability::uniform(CapabilitySupport::Native),
    };
}

impl WindowManagerSession for YabaiAdapter {
    fn adapter_name(&self) -> &'static str {
        Self::NAME
    }

    fn capabilities(&self) -> WindowManagerCapabilities {
        Self::CAPABILITIES
    }

    fn focused_window(&mut self) -> Result<FocusedWindowRecord> {
        let focused = self.focused_window_data()?;
        Ok(FocusedWindowRecord {
            id: focused.id,
            app_id: focused.app_id,
            title: focused.title,
            pid: focused.pid,
            original_tile_index: 1,
        })
    }

    fn windows(&mut self) -> Result<Vec<WindowRecord>> {
        let windows = self.query_windows()?;
        Ok(windows
            .into_iter()
            .map(|window| {
                let data = YabaiWindowData::from(window);
                WindowRecord {
                    id: data.id,
                    app_id: data.app_id,
                    title: data.title,
                    pid: data.pid,
                    is_focused: data.is_focused,
                    original_tile_index: 1,
                }
            })
            .collect())
    }

    fn focus_direction(&mut self, direction: Direction) -> Result<()> {
        Self::command_status("focus", &["window", "--focus", Self::direction_name(direction)])
    }

    fn move_direction(&mut self, direction: Direction) -> Result<()> {
        // In yabai, --swap exchanges positions with neighbor
        // --warp moves the window to the position (like i3 move)
        // Using --swap for consistency with tiling behavior
        Self::command_status("move", &["window", "--swap", Self::direction_name(direction)])
    }

    fn resize_with_intent(&mut self, intent: ResizeIntent) -> Result<()> {
        // yabai resize format: --resize <edge>:<dx>:<dy>
        // For growing/shrinking, we modify the edge in the direction of growth
        let (edge, dx, dy) = match intent.direction {
            Direction::West => {
                if intent.grow() {
                    ("left", -intent.step.abs(), 0)
                } else {
                    ("left", intent.step.abs(), 0)
                }
            }
            Direction::East => {
                if intent.grow() {
                    ("right", intent.step.abs(), 0)
                } else {
                    ("right", -intent.step.abs(), 0)
                }
            }
            Direction::North => {
                if intent.grow() {
                    ("top", 0, -intent.step.abs())
                } else {
                    ("top", 0, intent.step.abs())
                }
            }
            Direction::South => {
                if intent.grow() {
                    ("bottom", 0, intent.step.abs())
                } else {
                    ("bottom", 0, -intent.step.abs())
                }
            }
        };

        let resize_arg = format!("{edge}:{dx}:{dy}");
        Self::command_status("resize", &["window", "--resize", &resize_arg])
    }

    fn spawn(&mut self, command: Vec<String>) -> Result<()> {
        // Yabai doesn't have a spawn command - use macOS `open` or direct execution
        // For now, just execute the command directly
        if command.is_empty() {
            bail!("spawn: empty command");
        }
        let (program, args) = command.split_first().context("spawn: empty command")?;
        let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        runtime::run_command_status(program, &args_refs, &CommandContext::new(Self::NAME, "spawn"))
    }

    fn focus_window_by_id(&mut self, id: u64) -> Result<()> {
        let id_str = id.to_string();
        Self::command_status("focus_window_by_id", &["window", "--focus", &id_str])
    }

    fn close_window_by_id(&mut self, id: u64) -> Result<()> {
        let id_str = id.to_string();
        Self::command_status("close_window_by_id", &["window", "--close", &id_str])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_yabai_window_json() {
        let sample = r#"[
            {
                "id": 1234,
                "pid": 5678,
                "app": "WezTerm",
                "title": "~ - zsh",
                "has-focus": true,
                "space": 1,
                "display": 1,
                "is-floating": false,
                "is-minimized": false,
                "frame": { "x": 0.0, "y": 25.0, "w": 1920.0, "h": 1055.0 }
            },
            {
                "id": 2345,
                "pid": 6789,
                "app": "Firefox",
                "title": "GitHub",
                "has-focus": false,
                "space": 1,
                "display": 1,
                "is-floating": false,
                "is-minimized": false,
                "frame": { "x": 960.0, "y": 25.0, "w": 960.0, "h": 1055.0 }
            }
        ]"#;

        let windows: Vec<YabaiWindow> =
            serde_json::from_str(sample).expect("should parse yabai json");
        assert_eq!(windows.len(), 2);

        let focused = windows.iter().find(|w| w.has_focus).expect("should have focused window");
        assert_eq!(focused.id, 1234);
        assert_eq!(focused.app, "WezTerm");
        assert_eq!(focused.pid, 5678);
    }

    #[test]
    fn direction_names_match_yabai_api() {
        assert_eq!(YabaiAdapter::direction_name(Direction::West), "west");
        assert_eq!(YabaiAdapter::direction_name(Direction::East), "east");
        assert_eq!(YabaiAdapter::direction_name(Direction::North), "north");
        assert_eq!(YabaiAdapter::direction_name(Direction::South), "south");
    }
}
