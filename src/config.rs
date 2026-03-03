use anyhow::{anyhow, Context, Result};
/// Config types for WM app-integration configuration.
///
/// Deserializes TOML like:
///
/// ```toml
/// [app.terminal.wezterm]
/// enabled = true
/// mux_backend = "tmux"
/// focus.internal_panes = true
/// focus.internal_panes.allowed_directions = ["N", "S", "W", "E"]
/// move.docking.tear_off.enabled = true
/// move.docking.tear_off.only_if_edgemost = true
///
/// [app.browser.chrome]
/// enabled = true
/// focus.left = "previous_tab"
/// focus.right = "next_tab"
/// move.left = "backward"
/// move.right = "forward"
///
/// [wm]
/// enabled_integraton = 'niri'
/// ```
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};

use crate::engine::topology::Direction;

// ---------------------------------------------------------------------------
// Top-level config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Config {
    #[serde(default)]
    pub wm: WmConfig,

    #[serde(default)]
    pub app: AppConfig,
}

// ---------------------------------------------------------------------------
// Window manager selection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct WmConfig {
    /// Selects the active WM integration. Only one can be active at a time.
    ///
    /// ```toml
    /// [wm]
    /// enabled_integration = "aerospace"
    /// ```
    pub enabled_integration: WmBackend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WmBackend {
    /// Niri - Wayland compositor, Linux only
    #[cfg(target_os = "linux")]
    Niri,

    /// i3 - tiling WM for Linux/X11
    #[cfg(target_os = "linux")]
    I3,

    /// Yabai - tiling WM for macOS
    #[cfg(target_os = "macos")]
    Yabai,

    /// AeroSpace - tiling WM for macOS
    #[cfg(target_os = "macos")]
    Aerospace,
}

impl Default for WmBackend {
    fn default() -> Self {
        #[cfg(target_os = "linux")]
        {
            Self::Niri
        }
        #[cfg(target_os = "macos")]
        {
            Self::Aerospace
        }
    }
}

// ---------------------------------------------------------------------------
// App section
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AppConfig {
    #[serde(default)]
    pub browser: HashMap<String, BrowserAppConfig>,

    #[serde(default)]
    pub terminal: HashMap<String, TerminalAppConfig>,

    #[serde(default)]
    pub editor: HashMap<String, EditorAppConfig>,
}

// ---------------------------------------------------------------------------
// Shared docking config
// ---------------------------------------------------------------------------

/// `move.docking.tear_off.*`
///
/// Represented as a table so that sibling keys (`only_if_edgemost`, `scope`, …)
/// can coexist.  Use `tear_off.enabled` where the spec says `tear_off = bool`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TearOffConfig {
    /// Whether tearing off (popout into new window) is allowed. Default true.
    #[serde(default = "default_true")]
    pub enabled: bool,

    #[serde(default)]
    pub strategy: TearOffStrategy,

    /// Terminal-specific granularity of what gets torn off.
    pub scope: Option<TerminalTearOffScope>,
}

impl Default for TearOffConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            strategy: TearOffStrategy::default(),
            scope: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TearOffStrategy {
    /// browser: Only tear off if the tab is the first or last in the tab list.
    /// terminal/editor: Only tear off if the pane's longest edge is flush with the window edge.
    #[default]
    OnlyIfEdgemost,
    /// terminal/editor: Only tear off if the pane is flush with the window edge in all allowed directions.
    /// browser: same as OnlyIfEdgemost
    OnceItNeighborsWithWindowEdge,
    /// Always allow tearing off, regardless of the pane's position.
    Always,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DockingConfig {
    #[serde(default)]
    pub tear_off: TearOffConfig,

    /// Whether merging a pane back into the app window is allowed. Default true.
    #[serde(default = "default_true")]
    pub snap_back: bool,
}

impl Default for DockingConfig {
    fn default() -> Self {
        Self {
            tear_off: TearOffConfig::default(),
            snap_back: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Browser app config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DirectionalBrowserFocus {
    pub left: Option<BrowserFocusAction>,
    pub right: Option<BrowserFocusAction>,
    pub up: Option<BrowserFocusAction>,
    pub down: Option<BrowserFocusAction>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DirectionalBrowserMove {
    pub left: Option<BrowserMoveAction>,
    pub right: Option<BrowserMoveAction>,
    pub up: Option<BrowserMoveAction>,
    pub down: Option<BrowserMoveAction>,

    #[serde(default)]
    pub docking: DockingConfig,
}

impl Default for DirectionalBrowserMove {
    fn default() -> Self {
        Self {
            left: None,
            right: None,
            up: None,
            down: None,
            docking: DockingConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct BrowserAppConfig {
    /// Must be true to activate the integration. Default false.
    #[serde(default)]
    pub enabled: bool,

    /// Prevent the WM from moving the app window. Default false.
    #[serde(default)]
    pub anchor_app_window: bool,

    #[serde(default)]
    pub focus: DirectionalBrowserFocus,

    #[serde(rename = "move", default)]
    pub movement: DirectionalBrowserMove,
}

// ---------------------------------------------------------------------------
// Terminal / Editor shared internal-pane configs
// ---------------------------------------------------------------------------

/// Shared config for internal-pane focus/move/resize.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InternalPaneDirectionConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub allowed_directions: Option<Vec<Direction>>,
}

impl Default for InternalPaneDirectionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            allowed_directions: None,
        }
    }
}

pub type InternalPaneFocusConfig = InternalPaneDirectionConfig;
pub type InternalPaneMoveConfig = InternalPaneDirectionConfig;
pub type InternalPaneResizeConfig = InternalPaneDirectionConfig;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct PaneFocusConfig {
    #[serde(default)]
    pub internal_panes: InternalPaneFocusConfig,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct PaneResizeConfig {
    #[serde(default)]
    pub internal_panes: InternalPaneResizeConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PaneMoveConfig {
    #[serde(default)]
    pub internal_panes: InternalPaneMoveConfig,

    #[serde(default)]
    pub docking: DockingConfig,
}

impl Default for PaneMoveConfig {
    fn default() -> Self {
        Self {
            internal_panes: InternalPaneMoveConfig::default(),
            docking: DockingConfig::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Terminal app config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TerminalFocusConfig {
    #[serde(flatten)]
    pub pane: PaneFocusConfig,

    /// If false, focusing past the edgemost pane moves to the next tab. Default true.
    #[serde(default = "default_true")]
    pub ignore_tabs: bool,
}

impl Default for TerminalFocusConfig {
    fn default() -> Self {
        Self {
            pane: PaneFocusConfig::default(),
            ignore_tabs: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TerminalMoveConfig {
    #[serde(flatten)]
    pub pane: PaneMoveConfig,

    /// If false, moving past the edgemost pane sends the pane to the next tab. Default true.
    #[serde(default = "default_true")]
    pub ignore_tabs: bool,
}

impl Default for TerminalMoveConfig {
    fn default() -> Self {
        Self {
            pane: PaneMoveConfig::default(),
            ignore_tabs: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TerminalAppConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub anchor_app_window: bool,

    #[serde(default)]
    pub focus: TerminalFocusConfig,

    #[serde(rename = "move", default)]
    pub movement: TerminalMoveConfig,

    #[serde(default)]
    pub resize: PaneResizeConfig,

    /// wezterm/kitty/iterm2-specific fields flattened into the same section.
    #[serde(flatten)]
    pub variant: TerminalVariantConfig,
}

/// Fields that differ per terminal emulator.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TerminalVariantConfig {
    /// wezterm default: "wezterm"; kitty default: "kitty"; iterm2: tmux or zellij only.
    pub mux_backend: Option<TerminalMuxBackend>,

    /// Overrides the default tear-off scope (also settable via move.docking.tear_off.scope).
    pub tear_off_scope: Option<TerminalTearOffScope>,

    /// Optional mux bridge override.
    #[serde(default)]
    pub mux: TerminalMuxControl,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TerminalMuxControl {
    pub enable: Option<bool>,
}

// ---------------------------------------------------------------------------
// Editor app config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EditorAppConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub anchor_app_window: bool,

    #[serde(default)]
    pub focus: PaneFocusConfig,

    #[serde(default)]
    pub resize: PaneResizeConfig,

    #[serde(rename = "move", default)]
    pub movement: PaneMoveConfig,

    #[serde(default)]
    pub tear_off_scope: EditorTearOffScope,
}

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserFocusAction {
    Ignore,
    FocusPreviousTab,
    FocusNextTab,
    FocusFirstTab,
    FocusLastTab,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserMoveAction {
    Ignore,
    MoveTabBackward,
    MoveTabForward,
    MoveTabToFirstPosition,
    MoveTabToLastPosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalMuxBackend {
    Tmux,
    Zellij,
    Wezterm,
    Kitty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalTearOffScope {
    MuxPane,
    MuxWindow,
    MuxSession,
    TerminalTab,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EditorTearOffScope {
    Buffer,
    Window,
    Workspace,
}

impl Default for EditorTearOffScope {
    fn default() -> Self {
        Self::Buffer
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn default_true() -> bool {
    true
}

static ACTIVE_CONFIG: OnceLock<RwLock<Config>> = OnceLock::new();

fn config_cell() -> &'static RwLock<Config> {
    ACTIVE_CONFIG.get_or_init(|| RwLock::new(Config::default()))
}

fn read_config() -> Config {
    config_cell()
        .read()
        .map(|guard| guard.clone())
        .unwrap_or_else(|poisoned| poisoned.into_inner().clone())
}

fn write_config(next: Config) {
    match config_cell().write() {
        Ok(mut guard) => *guard = next,
        Err(poisoned) => *poisoned.into_inner() = next,
    }
}

fn config_paths() -> Vec<PathBuf> {
    if let Some(explicit) = std::env::var_os("NIRI_DEEP_CONFIG").map(PathBuf::from) {
        return vec![explicit];
    }

    let mut paths = Vec::new();
    if let Some(xdg_dir) = std::env::var_os("XDG_CONFIG_DIR").map(PathBuf::from) {
        paths.push(xdg_dir.join("niri-deep").join("config.toml"));
    }
    if let Some(xdg_home) = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from) {
        paths.push(xdg_home.join("niri-deep").join("config.toml"));
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        paths.push(home.join(".config").join("niri-deep").join("config.toml"));
    }
    paths
}

fn load_config_from(path: &PathBuf) -> Result<Config> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config file {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("failed to parse config file {}", path.display()))
}

pub fn prepare() -> Result<()> {
    let explicit = std::env::var_os("NIRI_DEEP_CONFIG").is_some();
    let paths = config_paths();

    for path in &paths {
        if path.exists() {
            write_config(load_config_from(path)?);
            return Ok(());
        }
    }

    if explicit {
        let path = paths
            .first()
            .cloned()
            .ok_or_else(|| anyhow!("NIRI_DEEP_CONFIG is set but no file path was resolved"))?;
        return Err(anyhow!(
            "config override path does not exist: {}",
            path.display()
        ));
    }

    write_config(Config::default());
    Ok(())
}

fn normalize_override(value: &str) -> Option<String> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn terminal_profile<'a>(cfg: &'a Config) -> Option<&'a TerminalAppConfig> {
    cfg.app
        .terminal
        .get("wezterm")
        .or_else(|| cfg.app.terminal.values().find(|item| item.enabled))
        .or_else(|| cfg.app.terminal.values().next())
}

fn editor_profile<'a>(cfg: &'a Config) -> Option<&'a EditorAppConfig> {
    cfg.app
        .editor
        .get("emacs")
        .or_else(|| cfg.app.editor.values().find(|item| item.enabled))
        .or_else(|| cfg.app.editor.values().next())
}

pub fn wm_adapter_override() -> Option<String> {
    let cfg = read_config();
    let value = match cfg.wm.enabled_integration {
        #[cfg(target_os = "linux")]
        WmBackend::Niri => "niri",
        #[cfg(target_os = "linux")]
        WmBackend::I3 => "i3",
        #[cfg(target_os = "macos")]
        WmBackend::Yabai => "yabai",
        #[cfg(target_os = "macos")]
        WmBackend::Aerospace => "aerospace",
    };
    normalize_override(value)
}

pub fn app_adapter_override() -> Option<String> {
    let cfg = read_config();
    if cfg.app.editor.len() == 1 {
        if let Some((key, app)) = cfg.app.editor.iter().next() {
            if app.enabled {
                return normalize_override(key);
            }
        }
    }
    if cfg.app.terminal.len() == 1 {
        if let Some((key, app)) = cfg.app.terminal.iter().next() {
            if app.enabled {
                return normalize_override(key);
            }
        }
    }
    None
}

pub fn terminal_enabled() -> bool {
    terminal_profile(&read_config())
        .map(|profile| profile.enabled)
        .unwrap_or(true)
}

pub fn terminal_mux_enable() -> Option<bool> {
    let cfg = read_config();
    let profile = terminal_profile(&cfg)?;
    profile
        .variant
        .mux
        .enable
        .or_else(|| match profile.variant.mux_backend {
            Some(TerminalMuxBackend::Wezterm) | None => None,
            Some(_) => Some(false),
        })
}

pub fn terminal_mux_backend() -> TerminalMuxBackend {
    terminal_profile(&read_config())
        .and_then(|profile| profile.variant.mux_backend)
        .unwrap_or(TerminalMuxBackend::Wezterm)
}

pub fn terminal_focus_internal_enabled() -> bool {
    terminal_profile(&read_config())
        .map(|profile| profile.focus.pane.internal_panes.enabled)
        .unwrap_or(true)
}

pub fn terminal_move_internal_enabled() -> bool {
    terminal_profile(&read_config())
        .map(|profile| profile.movement.pane.internal_panes.enabled)
        .unwrap_or(true)
}

pub fn terminal_resize_internal_enabled() -> bool {
    terminal_profile(&read_config())
        .map(|profile| profile.resize.internal_panes.enabled)
        .unwrap_or(true)
}

pub fn terminal_move_tearout_enabled() -> bool {
    terminal_profile(&read_config())
        .map(|profile| profile.movement.pane.docking.tear_off.enabled)
        .unwrap_or(true)
}

fn check_allowed(cfg: Option<&InternalPaneDirectionConfig>, direction: Direction) -> bool {
    cfg.map(|c| {
        c.enabled
            && c.allowed_directions
                .as_ref()
                .map(|dirs| dirs.contains(&direction))
                .unwrap_or(true)
    })
    .unwrap_or(true)
}

pub fn terminal_focus_allowed(direction: Direction) -> bool {
    check_allowed(
        terminal_profile(&read_config()).map(|p| &p.focus.pane.internal_panes),
        direction,
    )
}

pub fn terminal_move_allowed(direction: Direction) -> bool {
    check_allowed(
        terminal_profile(&read_config()).map(|p| &p.movement.pane.internal_panes),
        direction,
    )
}

pub fn terminal_resize_allowed(direction: Direction) -> bool {
    check_allowed(
        terminal_profile(&read_config()).map(|p| &p.resize.internal_panes),
        direction,
    )
}

pub fn editor_focus_internal_enabled() -> bool {
    editor_profile(&read_config())
        .map(|profile| profile.focus.internal_panes.enabled)
        .unwrap_or(true)
}

pub fn editor_move_internal_enabled() -> bool {
    editor_profile(&read_config())
        .map(|profile| profile.movement.internal_panes.enabled)
        .unwrap_or(true)
}

pub fn editor_resize_internal_enabled() -> bool {
    editor_profile(&read_config())
        .map(|profile| profile.resize.internal_panes.enabled)
        .unwrap_or(true)
}

pub fn editor_move_tearout_enabled() -> bool {
    editor_profile(&read_config())
        .map(|profile| profile.movement.docking.tear_off.enabled)
        .unwrap_or(true)
}

pub fn editor_focus_allowed(direction: Direction) -> bool {
    check_allowed(
        editor_profile(&read_config()).map(|p| &p.focus.internal_panes),
        direction,
    )
}

pub fn editor_move_allowed(direction: Direction) -> bool {
    check_allowed(
        editor_profile(&read_config()).map(|p| &p.movement.internal_panes),
        direction,
    )
}

pub fn editor_resize_allowed(direction: Direction) -> bool {
    check_allowed(
        editor_profile(&read_config()).map(|p| &p.resize.internal_panes),
        direction,
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_terminal_editor_and_mux_controls() {
        let sample = r#"
[app.terminal.wezterm]
enabled = true
mux_backend = "wezterm"
focus.internal_panes.enabled = true
move.internal_panes.enabled = true
resize.internal_panes.enabled = true
move.docking.tear_off.enabled = true
move.docking.snap_back = true
[app.terminal.wezterm.mux]
enable = false

[app.editor.emacs]
enabled = true
focus.internal_panes.enabled = true
move.internal_panes.enabled = true
resize.internal_panes.enabled = true
move.docking.tear_off.enabled = true
"#;
        let parsed: Config = toml::from_str(sample).expect("sample config should parse");
        assert!(parsed.app.terminal.contains_key("wezterm"));
        assert!(parsed.app.editor.contains_key("emacs"));
    }
}
