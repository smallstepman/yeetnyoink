//! Paneru window manager adapter for macOS.
//!
//! Paneru is a sliding/scrolling tiling window manager for macOS inspired by niri.
//! Windows are arranged on an infinite horizontal strip per monitor.
//! This adapter communicates via `paneru send-cmd` CLI.

use anyhow::{Context, Result, bail};

use crate::config::WmBackend;
use crate::engine::runtime::{self, CommandContext, ProcessId};
use crate::engine::topology::Direction;
use crate::engine::wm::{
    CapabilitySupport, ConfiguredWindowManager, DirectionalCapability, FloatingFocusMode,
    FocusedWindowRecord, PrimitiveWindowManagerCapabilities, ResizeIntent,
    WindowManagerCapabilities, WindowManagerCapabilityDescriptor, WindowManagerFeatures,
    WindowManagerSession, WindowManagerSpec, WindowRecord, validate_declared_capabilities,
};
use crate::logging;

pub struct PaneruAdapter;

pub struct PaneruSpec;

pub static PANERU_SPEC: PaneruSpec = PaneruSpec;

impl WindowManagerSpec for PaneruSpec {
    fn backend(&self) -> WmBackend {
        WmBackend::Paneru
    }

    fn name(&self) -> &'static str {
        PaneruAdapter::NAME
    }

    fn connect(&self) -> Result<ConfiguredWindowManager> {
        ConfiguredWindowManager::try_new(
            Box::new(PaneruAdapter::connect()?),
            WindowManagerFeatures::default(),
        )
    }

    fn floating_focus_mode(&self) -> FloatingFocusMode {
        PaneruAdapter::FLOATING_FOCUS_MODE
    }
}

impl PaneruAdapter {
    pub fn connect() -> Result<Self> {
        validate_declared_capabilities::<Self>()?;
        // Verify paneru is running by checking socket existence
        if !std::path::Path::new("/tmp/paneru.socket").exists() {
            bail!("paneru socket not found at /tmp/paneru.socket - is paneru running?");
        }
        Ok(Self)
    }

    fn send_cmd_status(action: &'static str, args: &[&str]) -> Result<()> {
        runtime::run_command_status(
            "paneru",
            &[&["send-cmd"], args].concat(),
            &CommandContext::new(Self::NAME, action),
        )
    }

    fn direction_name(direction: Direction) -> &'static str {
        match direction {
            Direction::West => "west",
            Direction::East => "east",
            Direction::North => "north",
            Direction::South => "south",
        }
    }
}

#[derive(Clone)]
struct PaneruWindowData {
    id: u64,
    app_id: Option<String>,
    title: Option<String>,
    pid: Option<ProcessId>,
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

impl WindowManagerCapabilityDescriptor for PaneruAdapter {
    const NAME: &'static str = "paneru";
    const CAPABILITIES: WindowManagerCapabilities = WindowManagerCapabilities {
        primitives: PrimitiveWindowManagerCapabilities {
            // Paneru has window swap but not niri-style column operations
            tear_out_right: false,
            move_column: false,
            consume_into_column_and_move: false,
            // Paneru supports resize cycling via window_resize/grow/shrink
            set_window_width: true,
            set_window_height: false, // Only horizontal strip, height is display-based
        },
        // Paneru supports moving windows to other displays
        tear_out: DirectionalCapability {
            west: CapabilitySupport::Unsupported,
            east: CapabilitySupport::Unsupported,
            // North/South can move to adjacent displays
            north: CapabilitySupport::Native,
            south: CapabilitySupport::Native,
        },
        // Resize is supported via grow/shrink preset widths
        resize: DirectionalCapability {
            west: CapabilitySupport::Native,
            east: CapabilitySupport::Native,
            // Vertical resize not really applicable in paneru's model
            north: CapabilitySupport::Unsupported,
            south: CapabilitySupport::Unsupported,
        },
    };
    const FLOATING_FOCUS_MODE: FloatingFocusMode = FloatingFocusMode::TilingOnly;
}

impl WindowManagerSession for PaneruAdapter {
    fn adapter_name(&self) -> &'static str {
        Self::NAME
    }

    fn capabilities(&self) -> WindowManagerCapabilities {
        Self::CAPABILITIES
    }

    fn focused_window(&mut self) -> Result<FocusedWindowRecord> {
        // Paneru doesn't expose focused window query via CLI
        // We need to use macOS accessibility APIs or frontmost app detection
        let frontmost = get_frontmost_window()?;
        Ok(FocusedWindowRecord {
            id: frontmost.id,
            app_id: frontmost.app_id,
            title: frontmost.title,
            pid: frontmost.pid,
            original_tile_index: 1,
        })
    }

    fn windows(&mut self) -> Result<Vec<WindowRecord>> {
        // Paneru doesn't expose window list via CLI
        // Return empty for now - orchestrator will fall back to WM-level operations
        logging::debug("paneru: window list query not supported via CLI");
        Ok(vec![])
    }

    fn focus_direction(&mut self, direction: Direction) -> Result<()> {
        Self::send_cmd_status(
            "focus",
            &["window", "focus", Self::direction_name(direction)],
        )
    }

    fn move_direction(&mut self, direction: Direction) -> Result<()> {
        // In paneru, swap exchanges positions with neighbor
        Self::send_cmd_status("move", &["window", "swap", Self::direction_name(direction)])
    }

    fn resize_with_intent(&mut self, intent: ResizeIntent) -> Result<()> {
        // Paneru uses preset width cycling, not pixel-based resize
        match intent.direction {
            Direction::West | Direction::East => {
                if intent.grow() {
                    Self::send_cmd_status("resize", &["window", "grow"])
                } else {
                    Self::send_cmd_status("resize", &["window", "shrink"])
                }
            }
            Direction::North | Direction::South => {
                // Vertical resize not meaningful in paneru's horizontal strip model
                logging::debug("paneru: vertical resize not supported");
                Ok(())
            }
        }
    }

    fn spawn(&mut self, command: Vec<String>) -> Result<()> {
        // Use macOS `open` or direct execution
        if command.is_empty() {
            bail!("spawn: empty command");
        }
        let (program, args) = command.split_first().context("spawn: empty command")?;
        let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        runtime::run_command_status(
            program,
            &args_refs,
            &CommandContext::new(Self::NAME, "spawn"),
        )
    }

    fn focus_window_by_id(&mut self, _id: u64) -> Result<()> {
        // Paneru doesn't expose focus-by-id via CLI
        // Would need accessibility API or AppleScript
        bail!("paneru: focus_window_by_id not supported via CLI")
    }

    fn close_window_by_id(&mut self, _id: u64) -> Result<()> {
        // Paneru doesn't expose close-by-id via CLI
        bail!("paneru: close_window_by_id not supported via CLI")
    }
}

/// Get the frontmost window using macOS lsappinfo (much faster than AppleScript)
fn get_frontmost_window() -> Result<PaneruWindowData> {
    // Get frontmost app ASN
    let front_output = std::process::Command::new("lsappinfo")
        .arg("front")
        .output()
        .context("failed to run lsappinfo front")?;

    if !front_output.status.success() {
        bail!(
            "lsappinfo front failed: {}",
            String::from_utf8_lossy(&front_output.stderr)
        );
    }

    let asn = String::from_utf8_lossy(&front_output.stdout)
        .trim()
        .to_string();

    if asn.is_empty() {
        bail!("lsappinfo front returned empty ASN");
    }

    // Get app info for the ASN
    let info_output = std::process::Command::new("lsappinfo")
        .args([
            "info", "-only", "pid", "-only", "name", "-only", "bundleid", &asn,
        ])
        .output()
        .context("failed to run lsappinfo info")?;

    if !info_output.status.success() {
        bail!(
            "lsappinfo info failed: {}",
            String::from_utf8_lossy(&info_output.stderr)
        );
    }

    let info = String::from_utf8_lossy(&info_output.stdout);

    let mut pid: u32 = 0;
    let mut app_name: Option<String> = None;
    let mut bundle_id: Option<String> = None;

    for line in info.lines() {
        if let Some(value) = line.strip_prefix("\"pid\"=") {
            pid = value.parse().unwrap_or(0);
        } else if let Some(value) = line.strip_prefix("\"LSDisplayName\"=") {
            app_name = Some(value.trim_matches('"').to_string());
        } else if let Some(value) = line.strip_prefix("\"CFBundleIdentifier\"=") {
            bundle_id = Some(value.trim_matches('"').to_string());
        }
    }

    // Prefer bundle ID as app_id (more stable), fallback to display name
    let app_id = bundle_id.or(app_name);

    if pid == 0 {
        bail!("failed to get PID from lsappinfo: {}", info);
    }

    Ok(PaneruWindowData {
        id: pid as u64,
        app_id,
        title: None, // lsappinfo doesn't provide window titles
        pid: ProcessId::new(pid),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direction_names_match_paneru_api() {
        assert_eq!(PaneruAdapter::direction_name(Direction::West), "west");
        assert_eq!(PaneruAdapter::direction_name(Direction::East), "east");
        assert_eq!(PaneruAdapter::direction_name(Direction::North), "north");
        assert_eq!(PaneruAdapter::direction_name(Direction::South), "south");
    }

    #[test]
    fn capabilities_are_valid() {
        assert!(PaneruAdapter::CAPABILITIES.validate().is_ok());
    }
}
