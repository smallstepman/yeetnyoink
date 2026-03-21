use anyhow::{Context, Result};
use serde::Deserialize;

use crate::engine::topology::Direction;
use crate::engine::runtime::ProcessId;

pub fn parse_window_address(raw: &str) -> Result<u64> {
    let trimmed = raw.trim().strip_prefix("0x").unwrap_or(raw.trim());
    u64::from_str_radix(trimmed, 16).context("invalid Hyprland window address")
}

pub fn format_window_selector(id: u64) -> String {
    format!("address:0x{id:x}")
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
}
