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
/// [app.editor.neovim]
/// enabled = true
/// [app.editor.neovim.ui.terminal]
/// app = "wezterm"
/// mux_backend = "inherit"
///
/// [wm]
/// enabled_integraton = 'niri'
/// ```
use crate::engine::topology::Direction;
use anyhow::{anyhow, Context, Result};
use etcetera::base_strategy::{choose_base_strategy, BaseStrategy};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};
use std::time::Duration;

// ---------------------------------------------------------------------------
// Top-level config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Config {
    #[serde(default)]
    pub wm: WmConfig,

    #[serde(default)]
    pub app: AppConfig,

    #[serde(default)]
    pub instrumentation: InstrumentationConfig,
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
    /// enabled_integration = "mangowc"
    /// ```
    pub enabled_integration: WmBackend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WmBackend {
    /// Niri - Wayland compositor, Linux only
    Niri,

    /// i3 - tiling WM for Linux/X11
    I3,

    /// Hyprland - Wayland compositor, Linux only
    Hyprland,

    /// Mangowc - Wayland compositor, Linux only
    Mangowc,

    /// Paneru - sliding/scrolling tiling WM for macOS (niri-like)
    Paneru,

    /// Yabai - tiling WM for macOS
    Yabai,
}

impl Default for WmBackend {
    fn default() -> Self {
        #[cfg(target_os = "linux")]
        {
            Self::Niri
        }
        #[cfg(target_os = "macos")]
        {
            Self::Yabai
        }
    }
}

impl WmBackend {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Niri => "niri",
            Self::I3 => "i3",
            Self::Hyprland => "hyprland",
            Self::Mangowc => "mangowc",
            Self::Paneru => "paneru",
            Self::Yabai => "yabai",
        }
    }

    pub const fn supported_on_current_platform(self) -> bool {
        match self {
            Self::Niri | Self::I3 | Self::Hyprland | Self::Mangowc => cfg!(target_os = "linux"),
            Self::Paneru | Self::Yabai => cfg!(target_os = "macos"),
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

    #[serde(default)]
    pub instrumentation: InstrumentationConfig,
}

// ---------------------------------------------------------------------------
// Instrumentation config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct InstrumentationConfig {
    #[serde(default)]
    pub logging: LoggingConfig,

    #[serde(default)]
    pub profiling: ProfilingConfig,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct LoggingConfig {
    #[serde(default)]
    pub quiet: bool,

    #[serde(default)]
    pub level: LogLevel,

    #[serde(default)]
    pub append_to_file: Option<PathBuf>,

    #[serde(default)]
    pub stream_to: LogStreamTo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Error,
    #[default]
    Info,
    Debug,
    Trace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogStreamTo {
    #[default]
    Stdout,
    Stderr,
    None,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ProfilingConfig {
    #[serde(default)]
    pub dump_directory: Option<PathBuf>,

    #[serde(default)]
    pub enabled: bool,
}

// ---------------------------------------------------------------------------
// Runtime config (DEPRECATED - fields moved to app.*.*.runtime)
// ---------------------------------------------------------------------------

pub const DEFAULT_VSCODE_REMOTE_CONTROL_HOST: &str = "127.0.0.1";
pub const DEFAULT_VSCODE_REMOTE_CONTROL_PORT: u16 = 3710;
pub const DEFAULT_VSCODE_FOCUS_SETTLE_MS: u64 = 50;

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

impl DirectionalBrowserFocus {
    fn action_for(&self, direction: Direction) -> Option<BrowserFocusAction> {
        direction.select(self.left, self.right, self.up, self.down)
    }
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

impl DirectionalBrowserMove {
    fn action_for(&self, direction: Direction) -> Option<BrowserMoveAction> {
        direction.select(self.left, self.right, self.up, self.down)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BrowserTabAxis {
    #[default]
    Horizontal,
    Vertical,
    VerticalFlipped,
}

impl BrowserTabAxis {
    fn default_focus_action(self, direction: Direction) -> BrowserFocusAction {
        match self {
            Self::Horizontal => direction.select(
                BrowserFocusAction::FocusPreviousTab,
                BrowserFocusAction::FocusNextTab,
                BrowserFocusAction::Ignore,
                BrowserFocusAction::Ignore,
            ),
            Self::Vertical => direction.select(
                BrowserFocusAction::Ignore,
                BrowserFocusAction::Ignore,
                BrowserFocusAction::FocusPreviousTab,
                BrowserFocusAction::FocusNextTab,
            ),
            Self::VerticalFlipped => direction.select(
                BrowserFocusAction::Ignore,
                BrowserFocusAction::Ignore,
                BrowserFocusAction::FocusNextTab,
                BrowserFocusAction::FocusPreviousTab,
            ),
        }
    }

    fn default_move_action(self, direction: Direction) -> BrowserMoveAction {
        match self {
            Self::Horizontal => direction.select(
                BrowserMoveAction::MoveTabBackward,
                BrowserMoveAction::MoveTabForward,
                BrowserMoveAction::Ignore,
                BrowserMoveAction::Ignore,
            ),
            Self::Vertical => direction.select(
                BrowserMoveAction::Ignore,
                BrowserMoveAction::Ignore,
                BrowserMoveAction::MoveTabBackward,
                BrowserMoveAction::MoveTabForward,
            ),
            Self::VerticalFlipped => direction.select(
                BrowserMoveAction::Ignore,
                BrowserMoveAction::Ignore,
                BrowserMoveAction::MoveTabForward,
                BrowserMoveAction::MoveTabBackward,
            ),
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

    /// Selects which directional axis maps to browser tab actions by default.
    ///
    /// - `horizontal` preserves the historical west/east tab behavior.
    /// - `vertical` routes north/south to previous/next tab and leaves
    ///   west/east to the window manager unless explicitly overridden.
    /// - `vertical_flipped` routes north/south to next/previous tab and leaves
    ///   west/east to the window manager unless explicitly overridden.
    #[serde(default)]
    pub tab_axis: BrowserTabAxis,

    #[serde(default)]
    pub focus: DirectionalBrowserFocus,

    #[serde(rename = "move", default)]
    pub movement: DirectionalBrowserMove,

    #[serde(default)]
    pub runtime: BrowserRuntimeConfig,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct BrowserRuntimeConfig {
    #[serde(default)]
    pub native_socket_path: Option<PathBuf>,
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

    /// Legacy fallback for host-tab focus when `variant.host_tabs` is unset.
    /// If false, focusing past the edgemost pane may move to the next host tab. Default true.
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

    /// Legacy fallback for host-tab move when `variant.host_tabs` is unset.
    /// If false, moving past the edgemost pane may send the pane to the next host tab. Default true.
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

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
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

    /// terminal-host-specific fields flattened into the same section.
    #[serde(flatten)]
    pub variant: TerminalVariantConfig,

    #[serde(default)]
    pub runtime: TerminalRuntimeConfig,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TerminalRuntimeConfig {
    #[serde(default)]
    pub zellij_break_plugin: Option<PathBuf>,
}

/// Fields that differ per terminal emulator.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TerminalVariantConfig {
    /// wezterm default: "wezterm"; kitty default: "kitty";
    /// foot/alacritty/ghostty/iterm2 default: "tmux".
    pub mux_backend: Option<TerminalMuxBackend>,

    /// Whether terminal host tabs participate in edge focus/move routing.
    pub host_tabs: Option<TerminalHostTabsMode>,

    /// Overrides the default tear-off scope (also settable via move.docking.tear_off.scope).
    pub tear_off_scope: Option<TerminalTearOffScope>,
}

// ---------------------------------------------------------------------------
// Editor app config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
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
    pub manage_terminal: bool,

    #[serde(default)]
    pub ui: EditorUiConfig,

    #[serde(default)]
    pub tear_off_scope: EditorTearOffScope,

    #[serde(default)]
    pub runtime: EditorRuntimeConfig,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct EditorRuntimeConfig {
    #[serde(default)]
    pub remote_control_host: Option<String>,

    #[serde(default)]
    pub remote_control_port: Option<u16>,

    #[serde(default)]
    pub state_file: Option<PathBuf>,

    #[serde(default)]
    pub focus_settle_ms: Option<u64>,

    #[serde(default)]
    pub test_clipboard_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct EditorUiConfig {
    #[serde(default)]
    pub terminal: Option<EditorTerminalUiConfig>,

    #[serde(default)]
    pub graphical: Option<EditorGraphicalUiConfig>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct EditorTerminalUiConfig {
    #[serde(default)]
    pub mux_backend: Option<EditorTerminalMuxBackend>,

    #[serde(default)]
    pub app: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct EditorGraphicalUiConfig {
    #[serde(default)]
    pub app: Option<String>,
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
pub enum EditorTerminalMuxBackend {
    #[serde(alias = "inherited")]
    Inherit,
    Tmux,
    Zellij,
    Wezterm,
    Kitty,
}

impl EditorTerminalMuxBackend {
    fn resolve(self, cfg: &Config, terminal_app: Option<&str>) -> Option<TerminalMuxBackend> {
        match self {
            Self::Inherit => terminal_app
                .and_then(|app| terminal_backend_for_aliases_from(cfg, &[app]))
                .or_else(|| single_enabled_terminal_backend(cfg)),
            Self::Tmux => Some(TerminalMuxBackend::Tmux),
            Self::Zellij => Some(TerminalMuxBackend::Zellij),
            Self::Wezterm => Some(TerminalMuxBackend::Wezterm),
            Self::Kitty => Some(TerminalMuxBackend::Kitty),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalTearOffScope {
    MuxPane,
    MuxWindow,
    MuxSession,
    TerminalTab,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TerminalHostTabsMode {
    #[default]
    Transparent,
    Focus,
    NativeFull,
}

impl TerminalHostTabsMode {
    fn enables_focus(self) -> bool {
        matches!(self, Self::Focus | Self::NativeFull)
    }

    fn enables_move(self) -> bool {
        matches!(self, Self::NativeFull)
    }
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
// Runtime state and loading
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

fn resolve_config_path(explicit: Option<&Path>) -> Result<(PathBuf, bool)> {
    if let Some(explicit) = explicit {
        return Ok((explicit.to_path_buf(), true));
    }

    let strategy = choose_base_strategy().context("failed to resolve config directory")?;
    Ok((
        strategy.config_dir().join("yeetnyoink").join("config.toml"),
        false,
    ))
}

fn load_config_from(path: &Path) -> Result<Config> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config file {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("failed to parse config file {}", path.display()))
}

pub fn prepare() -> Result<()> {
    prepare_with_path(None::<&Path>)
}

pub fn prepare_with_path(path: Option<&Path>) -> Result<()> {
    let (path, explicit) = resolve_config_path(path)?;
    if path.exists() {
        write_config(load_config_from(&path)?);
        return Ok(());
    }

    if explicit {
        return Err(anyhow!(
            "config override path does not exist: {}",
            path.display()
        ));
    }

    write_config(Config::default());
    Ok(())
}

pub fn snapshot() -> Config {
    read_config()
}

pub fn install(next: Config) {
    write_config(next);
}

pub fn update(mutator: impl FnOnce(&mut Config)) {
    match config_cell().write() {
        Ok(mut guard) => mutator(&mut guard),
        Err(poisoned) => mutator(&mut poisoned.into_inner()),
    }
}

// ---------------------------------------------------------------------------
// Instrumentation helpers
// ---------------------------------------------------------------------------

pub fn instrumentation_logging() -> LoggingConfig {
    read_config().instrumentation.logging
}

pub fn instrumentation_profiling() -> ProfilingConfig {
    read_config().instrumentation.profiling
}

pub fn logging_level() -> LogLevel {
    read_config().instrumentation.logging.level
}

pub fn logging_quiet_enabled() -> bool {
    read_config().instrumentation.logging.quiet
}

pub fn profiling_enabled() -> bool {
    read_config().instrumentation.profiling.enabled
}

pub fn profiling_dump_directory() -> Option<PathBuf> {
    read_config().instrumentation.profiling.dump_directory
}

// ---------------------------------------------------------------------------
// App runtime helpers
// ---------------------------------------------------------------------------

pub fn browser_native_socket_path(aliases: &[&str]) -> Option<PathBuf> {
    let cfg = read_config();
    profile_by_aliases(&cfg.app.browser, aliases)
        .and_then(|profile| profile.runtime.native_socket_path.clone())
}

pub fn chromium_native_socket_path(aliases: &[&str]) -> Option<PathBuf> {
    browser_native_socket_path(aliases)
}

pub fn firefox_native_socket_path(aliases: &[&str]) -> Option<PathBuf> {
    browser_native_socket_path(aliases)
}

pub fn vscode_runtime(aliases: &[&str]) -> Option<EditorRuntimeConfig> {
    let cfg = read_config();
    profile_by_aliases(&cfg.app.editor, aliases).map(|profile| profile.runtime.clone())
}

pub fn vscode_remote_control_host(aliases: &[&str]) -> String {
    vscode_runtime(aliases)
        .and_then(|runtime| runtime.remote_control_host)
        .and_then(|value| {
            let trimmed = value.trim().to_string();
            (!trimmed.is_empty()).then_some(trimmed)
        })
        .unwrap_or_else(|| DEFAULT_VSCODE_REMOTE_CONTROL_HOST.to_string())
}

pub fn vscode_remote_control_port(aliases: &[&str]) -> Option<u16> {
    vscode_runtime(aliases)
        .and_then(|runtime| runtime.remote_control_port)
        .filter(|port| *port > 0)
}

pub fn vscode_state_file_path(aliases: &[&str]) -> Option<PathBuf> {
    vscode_runtime(aliases).and_then(|runtime| runtime.state_file)
}

pub fn vscode_focus_settle_delay(aliases: &[&str]) -> Duration {
    Duration::from_millis(
        vscode_runtime(aliases)
            .and_then(|runtime| runtime.focus_settle_ms)
            .unwrap_or(DEFAULT_VSCODE_FOCUS_SETTLE_MS),
    )
}

pub fn vscode_test_clipboard_file(aliases: &[&str]) -> Option<PathBuf> {
    vscode_runtime(aliases).and_then(|runtime| runtime.test_clipboard_file)
}

pub fn terminal_zellij_break_plugin_path(aliases: &[&str]) -> Option<PathBuf> {
    let cfg = read_config();
    profile_by_aliases(&cfg.app.terminal, aliases)
        .and_then(|profile| profile.runtime.zellij_break_plugin.clone())
}

pub fn any_terminal_zellij_break_plugin_path() -> Option<PathBuf> {
    let cfg = read_config();
    cfg.app
        .terminal
        .values()
        .filter_map(|profile| profile.runtime.zellij_break_plugin.clone())
        .next()
}

fn normalize_override(value: &str) -> Option<String> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppSection {
    Browser,
    Terminal,
    Editor,
}

#[derive(Debug, Clone)]
pub struct PanePolicy {
    integration_enabled: bool,
    focus_internal: InternalPaneDirectionConfig,
    move_internal: InternalPaneDirectionConfig,
    resize_internal: InternalPaneDirectionConfig,
    tear_out_enabled: bool,
}

impl PanePolicy {
    fn defaults(integration_enabled: bool) -> Self {
        Self {
            integration_enabled,
            focus_internal: InternalPaneDirectionConfig::default(),
            move_internal: InternalPaneDirectionConfig::default(),
            resize_internal: InternalPaneDirectionConfig::default(),
            tear_out_enabled: true,
        }
    }

    pub fn integration_enabled(&self) -> bool {
        self.integration_enabled
    }

    pub fn focus_capability(&self) -> bool {
        self.integration_enabled && self.focus_internal.enabled
    }

    pub fn move_capability(&self) -> bool {
        self.integration_enabled && self.move_internal.enabled
    }

    pub fn resize_capability(&self) -> bool {
        self.integration_enabled && self.resize_internal.enabled
    }

    pub fn tear_out_capability(&self) -> bool {
        self.integration_enabled && self.tear_out_enabled
    }

    pub fn focus_allowed(&self, direction: Direction) -> bool {
        self.integration_enabled && check_allowed(&self.focus_internal, direction)
    }

    pub fn move_allowed(&self, direction: Direction) -> bool {
        self.integration_enabled && check_allowed(&self.move_internal, direction)
    }

    pub fn resize_allowed(&self, direction: Direction) -> bool {
        self.integration_enabled && check_allowed(&self.resize_internal, direction)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MuxPolicy {
    pub integration_enabled: bool,
    pub backend: TerminalMuxBackend,
}

fn alias_keys(aliases: &[&str]) -> Vec<String> {
    let mut keys = Vec::new();
    for alias in aliases {
        let Some(normalized) = normalize_override(alias) else {
            continue;
        };
        if !keys.contains(&normalized) {
            keys.push(normalized);
        }
    }
    keys
}

fn default_mux_backend_for_aliases(aliases: &[&str]) -> TerminalMuxBackend {
    let aliases = alias_keys(aliases);
    if aliases.iter().any(|alias| alias == "kitty") {
        return TerminalMuxBackend::Kitty;
    }
    if aliases
        .iter()
        .any(|alias| alias == "foot" || alias == "alacritty" || alias == "ghostty")
    {
        return TerminalMuxBackend::Tmux;
    }
    if aliases
        .iter()
        .any(|alias| alias == "iterm2" || alias == "iterm")
    {
        return TerminalMuxBackend::Tmux;
    }
    TerminalMuxBackend::Wezterm
}

fn profile_by_aliases<'a, T>(profiles: &'a HashMap<String, T>, aliases: &[&str]) -> Option<&'a T> {
    profile_entry_by_aliases(profiles, aliases).map(|(_, profile)| profile)
}

fn profile_entry_by_aliases<'a, T>(
    profiles: &'a HashMap<String, T>,
    aliases: &[&str],
) -> Option<(&'a str, &'a T)> {
    let aliases = alias_keys(aliases);
    if aliases.is_empty() {
        return None;
    }

    for alias in &aliases {
        if let Some((key, profile)) = profiles.get_key_value(alias) {
            return Some((key.as_str(), profile));
        }
    }

    for (key, profile) in profiles {
        if aliases.iter().any(|alias| key.eq_ignore_ascii_case(alias)) {
            return Some((key.as_str(), profile));
        }
    }

    None
}

fn section_enabled_from<T>(
    profiles: &HashMap<String, T>,
    aliases: &[&str],
    enabled: impl Fn(&T) -> bool,
) -> bool {
    profile_by_aliases(profiles, aliases)
        .map(enabled)
        .unwrap_or(false)
}

fn app_enabled_from(cfg: &Config, section: AppSection, aliases: &[&str]) -> bool {
    match section {
        AppSection::Browser => {
            section_enabled_from(&cfg.app.browser, aliases, |profile| profile.enabled)
        }
        AppSection::Terminal => {
            section_enabled_from(&cfg.app.terminal, aliases, |profile| profile.enabled)
        }
        AppSection::Editor => {
            section_enabled_from(&cfg.app.editor, aliases, |profile| profile.enabled)
        }
    }
}

pub fn selected_wm_backend() -> WmBackend {
    read_config().wm.enabled_integration
}

fn app_adapter_override_from(cfg: &Config) -> Option<String> {
    if cfg.app.terminal.len() == 1 {
        if let Some((key, app)) = cfg.app.terminal.iter().next() {
            if app.enabled {
                return normalize_override(key);
            }
        }
    }
    None
}

pub fn app_adapter_override() -> Option<String> {
    app_adapter_override_from(&read_config())
}

pub fn app_integration_enabled(section: AppSection, aliases: &[&str]) -> bool {
    app_enabled_from(&read_config(), section, aliases)
}

fn browser_profile_from<'a>(cfg: &'a Config, aliases: &[&str]) -> Option<&'a BrowserAppConfig> {
    profile_by_aliases(&cfg.app.browser, aliases)
}

fn browser_focus_action_from(
    cfg: &Config,
    aliases: &[&str],
    direction: Direction,
) -> BrowserFocusAction {
    let profile = browser_profile_from(cfg, aliases);
    let tab_axis = profile.map(|profile| profile.tab_axis).unwrap_or_default();
    profile
        .and_then(|profile| profile.focus.action_for(direction))
        .unwrap_or_else(|| tab_axis.default_focus_action(direction))
}

pub fn browser_focus_action_for(aliases: &[&str], direction: Direction) -> BrowserFocusAction {
    browser_focus_action_from(&read_config(), aliases, direction)
}

fn browser_move_action_from(
    cfg: &Config,
    aliases: &[&str],
    direction: Direction,
) -> BrowserMoveAction {
    let profile = browser_profile_from(cfg, aliases);
    let tab_axis = profile.map(|profile| profile.tab_axis).unwrap_or_default();
    profile
        .and_then(|profile| profile.movement.action_for(direction))
        .unwrap_or_else(|| tab_axis.default_move_action(direction))
}

pub fn browser_move_action_for(aliases: &[&str], direction: Direction) -> BrowserMoveAction {
    browser_move_action_from(&read_config(), aliases, direction)
}

fn pane_policy_from(cfg: &Config, section: AppSection, aliases: &[&str]) -> PanePolicy {
    match section {
        AppSection::Terminal => {
            let profile = profile_by_aliases(&cfg.app.terminal, aliases);
            let integration_enabled = profile.map(|profile| profile.enabled).unwrap_or(false);
            match profile {
                Some(profile) => PanePolicy {
                    integration_enabled,
                    focus_internal: profile.focus.pane.internal_panes.clone(),
                    move_internal: profile.movement.pane.internal_panes.clone(),
                    resize_internal: profile.resize.internal_panes.clone(),
                    tear_out_enabled: profile.movement.pane.docking.tear_off.enabled,
                },
                None => PanePolicy::defaults(integration_enabled),
            }
        }
        AppSection::Editor => {
            let profile = profile_by_aliases(&cfg.app.editor, aliases);
            let integration_enabled = profile.map(|profile| profile.enabled).unwrap_or(false);
            match profile {
                Some(profile) => PanePolicy {
                    integration_enabled,
                    focus_internal: profile.focus.internal_panes.clone(),
                    move_internal: profile.movement.internal_panes.clone(),
                    resize_internal: profile.resize.internal_panes.clone(),
                    tear_out_enabled: profile.movement.docking.tear_off.enabled,
                },
                None => PanePolicy::defaults(integration_enabled),
            }
        }
        AppSection::Browser => PanePolicy::defaults(app_enabled_from(cfg, section, aliases)),
    }
}

pub fn pane_policy_for(section: AppSection, aliases: &[&str]) -> PanePolicy {
    pane_policy_from(&read_config(), section, aliases)
}

fn terminal_host_tabs_focus_from(cfg: &Config, aliases: &[&str]) -> bool {
    let profile = profile_by_aliases(&cfg.app.terminal, aliases);
    if let Some(mode) = profile.and_then(|profile| profile.variant.host_tabs) {
        return mode.enables_focus();
    }
    profile
        .map(|profile| !profile.focus.ignore_tabs)
        .unwrap_or(false)
}

pub fn terminal_focus_host_tabs_for(aliases: &[&str]) -> bool {
    terminal_host_tabs_focus_from(&read_config(), aliases)
}

fn terminal_host_tabs_move_from(cfg: &Config, aliases: &[&str]) -> bool {
    let profile = profile_by_aliases(&cfg.app.terminal, aliases);
    if let Some(mode) = profile.and_then(|profile| profile.variant.host_tabs) {
        return mode.enables_move();
    }
    profile
        .map(|profile| !profile.movement.ignore_tabs)
        .unwrap_or(false)
}

pub fn terminal_move_host_tabs_for(aliases: &[&str]) -> bool {
    terminal_host_tabs_move_from(&read_config(), aliases)
}

fn mux_policy_from(cfg: &Config, aliases: &[&str]) -> MuxPolicy {
    let profile = profile_by_aliases(&cfg.app.terminal, aliases);
    let integration_enabled = profile.map(|profile| profile.enabled).unwrap_or(false);
    MuxPolicy {
        integration_enabled,
        backend: profile
            .and_then(|profile| profile.variant.mux_backend)
            .unwrap_or_else(|| default_mux_backend_for_aliases(aliases)),
    }
}

pub fn mux_policy_for(aliases: &[&str]) -> MuxPolicy {
    mux_policy_from(&read_config(), aliases)
}

fn terminal_profile_backend(key: &str, profile: &TerminalAppConfig) -> TerminalMuxBackend {
    profile
        .variant
        .mux_backend
        .unwrap_or_else(|| default_mux_backend_for_aliases(&[key]))
}

fn known_terminal_backend_for_aliases(aliases: &[&str]) -> Option<TerminalMuxBackend> {
    let aliases = alias_keys(aliases);
    if aliases.iter().any(|alias| alias == "kitty") {
        return Some(TerminalMuxBackend::Kitty);
    }
    if aliases.iter().any(|alias| alias == "wezterm") {
        return Some(TerminalMuxBackend::Wezterm);
    }
    if aliases.iter().any(|alias| {
        matches!(
            alias.as_str(),
            "foot" | "alacritty" | "ghostty" | "iterm2" | "iterm"
        )
    }) {
        return Some(TerminalMuxBackend::Tmux);
    }
    None
}

fn terminal_backend_for_aliases_from(cfg: &Config, aliases: &[&str]) -> Option<TerminalMuxBackend> {
    profile_entry_by_aliases(&cfg.app.terminal, aliases)
        .map(|(key, profile)| terminal_profile_backend(key, profile))
        .or_else(|| known_terminal_backend_for_aliases(aliases))
}

fn single_enabled_terminal_backend(cfg: &Config) -> Option<TerminalMuxBackend> {
    let mut enabled = cfg
        .app
        .terminal
        .iter()
        .filter(|(_, profile)| profile.enabled);
    let (key, profile) = enabled.next()?;
    if enabled.next().is_some() {
        return None;
    }
    Some(terminal_profile_backend(key, profile))
}

fn terminal_app_matches_aliases(app: &str, aliases: &[&str]) -> bool {
    let Some(normalized) = normalize_override(app) else {
        return false;
    };
    alias_keys(aliases).iter().any(|alias| alias == &normalized)
}

fn editor_terminal_ui_from<'a>(
    cfg: &'a Config,
    aliases: &[&str],
) -> Option<&'a EditorTerminalUiConfig> {
    profile_by_aliases(&cfg.app.editor, aliases).and_then(|profile| profile.ui.terminal.as_ref())
}

fn any_enabled_editor_targets_terminal_app_from(cfg: &Config, terminal_aliases: &[&str]) -> bool {
    cfg.app.editor.values().any(|profile| {
        profile.enabled
            && profile
                .ui
                .terminal
                .as_ref()
                .and_then(|ui| ui.app.as_deref())
                .is_some_and(|app| terminal_app_matches_aliases(app, terminal_aliases))
    })
}

pub fn terminal_chain_enabled_from(cfg: &Config, aliases: &[&str]) -> bool {
    app_enabled_from(cfg, AppSection::Terminal, aliases)
        || any_enabled_editor_targets_terminal_app_from(cfg, aliases)
}

pub fn terminal_chain_enabled_for(aliases: &[&str]) -> bool {
    terminal_chain_enabled_from(&read_config(), aliases)
}

fn editor_terminal_mux_backend_from(cfg: &Config, aliases: &[&str]) -> Option<TerminalMuxBackend> {
    let terminal_ui = editor_terminal_ui_from(cfg, aliases)?;
    let terminal_app = terminal_ui.app.as_deref();
    terminal_ui
        .mux_backend
        .and_then(|backend| backend.resolve(cfg, terminal_app))
        .or_else(|| terminal_app.and_then(|app| terminal_backend_for_aliases_from(cfg, &[app])))
}

pub fn editor_terminal_mux_backend_for(aliases: &[&str]) -> Option<TerminalMuxBackend> {
    editor_terminal_mux_backend_from(&read_config(), aliases)
}

fn editor_terminal_ui_app_from(cfg: &Config, aliases: &[&str]) -> Option<String> {
    editor_terminal_ui_from(cfg, aliases)
        .and_then(|ui| ui.app.as_deref())
        .and_then(normalize_override)
}

pub fn editor_terminal_ui_app_for(aliases: &[&str]) -> Option<String> {
    editor_terminal_ui_app_from(&read_config(), aliases)
}

fn editor_graphical_ui_app_from(cfg: &Config, aliases: &[&str]) -> Option<String> {
    profile_by_aliases(&cfg.app.editor, aliases)
        .and_then(|profile| profile.ui.graphical.as_ref())
        .and_then(|ui| ui.app.as_deref())
        .and_then(normalize_override)
}

pub fn editor_graphical_ui_app_for(aliases: &[&str]) -> Option<String> {
    editor_graphical_ui_app_from(&read_config(), aliases)
}

fn editor_tear_off_scope_from(cfg: &Config, aliases: &[&str]) -> EditorTearOffScope {
    profile_by_aliases(&cfg.app.editor, aliases)
        .map(|profile| profile.tear_off_scope)
        .unwrap_or_default()
}

pub fn editor_tear_off_scope_for(aliases: &[&str]) -> EditorTearOffScope {
    editor_tear_off_scope_from(&read_config(), aliases)
}

fn editor_manage_terminal_from(cfg: &Config, aliases: &[&str]) -> bool {
    profile_by_aliases(&cfg.app.editor, aliases)
        .map(|profile| profile.manage_terminal)
        .unwrap_or(false)
}

pub fn editor_manage_terminal_for(aliases: &[&str]) -> bool {
    editor_manage_terminal_from(&read_config(), aliases)
}

fn check_allowed(cfg: &InternalPaneDirectionConfig, direction: Direction) -> bool {
    cfg.enabled
        && cfg
            .allowed_directions
            .as_ref()
            .map(|dirs| dirs.contains(&direction))
            .unwrap_or(true)
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
[app.editor.emacs.ui.graphical]
app = "emacs"
"#;
        let parsed: Config = toml::from_str(sample).expect("sample config should parse");
        assert!(parsed.app.terminal.contains_key("wezterm"));
        assert!(parsed.app.editor.contains_key("emacs"));
        assert_eq!(
            editor_graphical_ui_app_from(&parsed, &["emacs", "editor"]),
            Some("emacs".to_string())
        );
    }

    #[test]
    fn pane_policy_resolves_matching_alias_without_cross_profile_fallback() {
        let sample = r#"
[app.terminal.wezterm]
enabled = true
focus.internal_panes.enabled = false

[app.terminal.kitty]
enabled = false
"#;
        let parsed: Config = toml::from_str(sample).expect("sample config should parse");

        let wezterm_policy =
            pane_policy_from(&parsed, AppSection::Terminal, &["terminal", "wezterm"]);
        assert!(wezterm_policy.integration_enabled());
        assert!(!wezterm_policy.focus_capability());

        let kitty_policy = pane_policy_from(&parsed, AppSection::Terminal, &["kitty"]);
        assert!(!kitty_policy.integration_enabled());
    }

    #[test]
    fn unmatched_alias_is_disabled_by_default() {
        let sample = r#"
[app.editor.vscode]
enabled = true
"#;
        let parsed: Config = toml::from_str(sample).expect("sample config should parse");

        assert!(app_enabled_from(&parsed, AppSection::Editor, &["vscode"]));
        assert!(!app_enabled_from(
            &parsed,
            AppSection::Editor,
            &["editor", "emacs"]
        ));
    }

    #[test]
    fn matching_alias_can_still_be_explicitly_disabled() {
        let sample = r#"
[app.editor.emacs]
enabled = false
"#;
        let parsed: Config = toml::from_str(sample).expect("sample config should parse");

        assert!(!app_enabled_from(
            &parsed,
            AppSection::Editor,
            &["editor", "emacs"]
        ));
        assert!(!app_enabled_from(&parsed, AppSection::Editor, &["vscode"]));
    }

    #[test]
    fn browser_focus_and_move_default_to_horizontal_tab_axis() {
        let sample = r#"
[app.browser.librewolf]
enabled = true
"#;
        let parsed: Config = toml::from_str(sample).expect("sample config should parse");

        assert_eq!(
            browser_focus_action_from(&parsed, &["firefox", "librewolf"], Direction::West),
            BrowserFocusAction::FocusPreviousTab
        );
        assert_eq!(
            browser_focus_action_from(&parsed, &["firefox", "librewolf"], Direction::East),
            BrowserFocusAction::FocusNextTab
        );
        assert_eq!(
            browser_focus_action_from(&parsed, &["firefox", "librewolf"], Direction::North),
            BrowserFocusAction::Ignore
        );
        assert_eq!(
            browser_focus_action_from(&parsed, &["firefox", "librewolf"], Direction::South),
            BrowserFocusAction::Ignore
        );

        assert_eq!(
            browser_move_action_from(&parsed, &["firefox", "librewolf"], Direction::West),
            BrowserMoveAction::MoveTabBackward
        );
        assert_eq!(
            browser_move_action_from(&parsed, &["firefox", "librewolf"], Direction::East),
            BrowserMoveAction::MoveTabForward
        );
        assert_eq!(
            browser_move_action_from(&parsed, &["firefox", "librewolf"], Direction::North),
            BrowserMoveAction::Ignore
        );
        assert_eq!(
            browser_move_action_from(&parsed, &["firefox", "librewolf"], Direction::South),
            BrowserMoveAction::Ignore
        );
    }

    #[test]
    fn browser_vertical_tab_axis_maps_north_up_and_south_down() {
        let sample = r#"
[app.browser.chromium]
enabled = true
tab_axis = "vertical"
"#;
        let parsed: Config = toml::from_str(sample).expect("sample config should parse");

        assert_eq!(
            browser_focus_action_from(&parsed, &["chromium", "chrome"], Direction::West),
            BrowserFocusAction::Ignore
        );
        assert_eq!(
            browser_focus_action_from(&parsed, &["chromium", "chrome"], Direction::East),
            BrowserFocusAction::Ignore
        );
        assert_eq!(
            browser_focus_action_from(&parsed, &["chromium", "chrome"], Direction::North),
            BrowserFocusAction::FocusPreviousTab
        );
        assert_eq!(
            browser_focus_action_from(&parsed, &["chromium", "chrome"], Direction::South),
            BrowserFocusAction::FocusNextTab
        );

        assert_eq!(
            browser_move_action_from(&parsed, &["chromium", "chrome"], Direction::West),
            BrowserMoveAction::Ignore
        );
        assert_eq!(
            browser_move_action_from(&parsed, &["chromium", "chrome"], Direction::East),
            BrowserMoveAction::Ignore
        );
        assert_eq!(
            browser_move_action_from(&parsed, &["chromium", "chrome"], Direction::North),
            BrowserMoveAction::MoveTabBackward
        );
        assert_eq!(
            browser_move_action_from(&parsed, &["chromium", "chrome"], Direction::South),
            BrowserMoveAction::MoveTabForward
        );
    }

    #[test]
    fn browser_vertical_flipped_tab_axis_preserves_previous_runtime_mapping() {
        let sample = r#"
[app.browser.chromium]
enabled = true
tab_axis = "vertical_flipped"
"#;
        let parsed: Config = toml::from_str(sample).expect("sample config should parse");

        assert_eq!(
            browser_focus_action_from(&parsed, &["chromium", "chrome"], Direction::North),
            BrowserFocusAction::FocusNextTab
        );
        assert_eq!(
            browser_focus_action_from(&parsed, &["chromium", "chrome"], Direction::South),
            BrowserFocusAction::FocusPreviousTab
        );
        assert_eq!(
            browser_move_action_from(&parsed, &["chromium", "chrome"], Direction::North),
            BrowserMoveAction::MoveTabForward
        );
        assert_eq!(
            browser_move_action_from(&parsed, &["chromium", "chrome"], Direction::South),
            BrowserMoveAction::MoveTabBackward
        );
    }

    #[test]
    fn browser_direction_overrides_beat_tab_axis_defaults() {
        let sample = r#"
[app.browser.librewolf]
enabled = true
tab_axis = "vertical"

[app.browser.librewolf.focus]
left = "focus_first_tab"

[app.browser.librewolf.move]
right = "move_tab_to_last_position"
"#;
        let parsed: Config = toml::from_str(sample).expect("sample config should parse");

        assert_eq!(
            browser_focus_action_from(&parsed, &["firefox", "librewolf"], Direction::West),
            BrowserFocusAction::FocusFirstTab
        );
        assert_eq!(
            browser_focus_action_from(&parsed, &["firefox", "librewolf"], Direction::North),
            BrowserFocusAction::FocusPreviousTab
        );
        assert_eq!(
            browser_move_action_from(&parsed, &["firefox", "librewolf"], Direction::East),
            BrowserMoveAction::MoveTabToLastPosition
        );
        assert_eq!(
            browser_move_action_from(&parsed, &["firefox", "librewolf"], Direction::South),
            BrowserMoveAction::MoveTabForward
        );
    }

    #[test]
    fn mux_policy_defaults_follow_terminal_host_alias() {
        let sample = r#"
[app.terminal.kitty]
enabled = true
"#;
        let parsed: Config = toml::from_str(sample).expect("sample config should parse");
        let kitty_policy = mux_policy_from(&parsed, &["kitty", "terminal"]);
        assert_eq!(kitty_policy.backend, TerminalMuxBackend::Kitty);

        let foot_policy = mux_policy_from(&parsed, &["foot", "terminal"]);
        assert_eq!(foot_policy.backend, TerminalMuxBackend::Tmux);

        let alacritty_policy = mux_policy_from(&parsed, &["alacritty", "terminal"]);
        assert_eq!(alacritty_policy.backend, TerminalMuxBackend::Tmux);

        let ghostty_policy = mux_policy_from(&parsed, &["ghostty", "terminal"]);
        assert_eq!(ghostty_policy.backend, TerminalMuxBackend::Tmux);

        let wezterm_policy = mux_policy_from(&parsed, &["wezterm", "terminal"]);
        assert_eq!(wezterm_policy.backend, TerminalMuxBackend::Wezterm);
    }

    #[test]
    fn terminal_host_tabs_defaults_to_transparent() {
        let sample = r#"
[app.terminal.wezterm]
enabled = true
"#;
        let parsed: Config = toml::from_str(sample).expect("sample config should parse");
        assert!(!terminal_host_tabs_focus_from(
            &parsed,
            &["wezterm", "terminal"]
        ));
        assert!(!terminal_host_tabs_move_from(
            &parsed,
            &["wezterm", "terminal"]
        ));
    }

    #[test]
    fn terminal_host_tabs_mode_controls_focus_and_move() {
        let sample = r#"
[app.terminal.wezterm]
enabled = true
host_tabs = "native_full"
"#;
        let parsed: Config = toml::from_str(sample).expect("sample config should parse");
        assert!(terminal_host_tabs_focus_from(
            &parsed,
            &["wezterm", "terminal"]
        ));
        assert!(terminal_host_tabs_move_from(
            &parsed,
            &["wezterm", "terminal"]
        ));

        let sample = r#"
[app.terminal.wezterm]
enabled = true
host_tabs = "focus"
"#;
        let parsed: Config = toml::from_str(sample).expect("sample config should parse");
        assert!(terminal_host_tabs_focus_from(
            &parsed,
            &["wezterm", "terminal"]
        ));
        assert!(!terminal_host_tabs_move_from(
            &parsed,
            &["wezterm", "terminal"]
        ));
    }

    #[test]
    fn terminal_host_tabs_explicit_mode_beats_legacy_ignore_tabs() {
        let sample = r#"
[app.terminal.wezterm]
enabled = true
host_tabs = "transparent"
focus.ignore_tabs = false
move.ignore_tabs = false
"#;
        let parsed: Config = toml::from_str(sample).expect("sample config should parse");
        assert!(!terminal_host_tabs_focus_from(
            &parsed,
            &["wezterm", "terminal"]
        ));
        assert!(!terminal_host_tabs_move_from(
            &parsed,
            &["wezterm", "terminal"]
        ));
    }

    #[test]
    fn terminal_host_tabs_falls_back_to_legacy_ignore_tabs() {
        let sample = r#"
[app.terminal.kitty]
enabled = true
focus.ignore_tabs = false
move.ignore_tabs = false
"#;
        let parsed: Config = toml::from_str(sample).expect("sample config should parse");
        assert!(terminal_host_tabs_focus_from(
            &parsed,
            &["kitty", "terminal"]
        ));
        assert!(terminal_host_tabs_move_from(
            &parsed,
            &["kitty", "terminal"]
        ));
    }

    #[test]
    fn editor_tear_off_scope_follows_matching_alias() {
        let sample = r#"
[app.editor.vscode]
enabled = true
tear_off_scope = "window"
"#;
        let parsed: Config = toml::from_str(sample).expect("sample config should parse");
        assert_eq!(
            editor_tear_off_scope_from(&parsed, &["vscode"]),
            EditorTearOffScope::Window
        );
        assert_eq!(
            editor_tear_off_scope_from(&parsed, &["emacs"]),
            EditorTearOffScope::Buffer
        );
    }

    #[test]
    fn editor_manage_terminal_follows_matching_alias() {
        let sample = r#"
[app.editor.vscode]
enabled = true
manage_terminal = true
"#;
        let parsed: Config = toml::from_str(sample).expect("sample config should parse");
        assert!(editor_manage_terminal_from(&parsed, &["vscode"]));
        assert!(!editor_manage_terminal_from(&parsed, &["emacs"]));
    }

    #[test]
    fn singleton_editor_profile_does_not_become_terminal_override() {
        let sample = r#"
[app.editor.neovim]
enabled = true
"#;
        let parsed: Config = toml::from_str(sample).expect("sample config should parse");
        assert_eq!(app_adapter_override_from(&parsed), None);
    }

    #[test]
    fn neovim_editor_profile_matches_nvim_aliases() {
        let sample = r#"
[app.editor.neovim]
enabled = true
move.internal_panes.enabled = true
"#;
        let parsed: Config = toml::from_str(sample).expect("sample config should parse");
        let policy = pane_policy_from(&parsed, AppSection::Editor, &["nvim", "neovim"]);
        assert!(policy.integration_enabled());
        assert!(policy.move_capability());
    }

    #[test]
    fn neovim_editor_terminal_ui_inherits_single_enabled_terminal_profile() {
        let sample = r#"
[app.terminal.wezterm]
enabled = true
mux_backend = "tmux"

[app.editor.neovim]
enabled = true
[app.editor.neovim.ui.terminal]
mux_backend = "inherit"
"#;
        let parsed: Config = toml::from_str(sample).expect("sample config should parse");
        assert_eq!(
            editor_terminal_mux_backend_from(&parsed, &["nvim", "neovim"]),
            Some(TerminalMuxBackend::Tmux)
        );
    }

    #[test]
    fn neovim_editor_terminal_ui_inherit_is_none_when_ambiguous() {
        let sample = r#"
[app.terminal.wezterm]
enabled = true
mux_backend = "tmux"

[app.terminal.kitty]
enabled = true
mux_backend = "kitty"

[app.editor.neovim]
enabled = true
[app.editor.neovim.ui.terminal]
mux_backend = "inherit"
"#;
        let parsed: Config = toml::from_str(sample).expect("sample config should parse");
        assert_eq!(
            editor_terminal_mux_backend_from(&parsed, &["nvim", "neovim"]),
            None
        );
    }

    #[test]
    fn explicit_neovim_editor_terminal_mux_backend_overrides_terminal_config() {
        let sample = r#"
[app.terminal.wezterm]
enabled = true
mux_backend = "tmux"

[app.editor.neovim]
enabled = true
[app.editor.neovim.ui.terminal]
mux_backend = "zellij"
"#;
        let parsed: Config = toml::from_str(sample).expect("sample config should parse");
        assert_eq!(
            editor_terminal_mux_backend_from(&parsed, &["nvim", "neovim"]),
            Some(TerminalMuxBackend::Zellij)
        );
    }

    #[test]
    fn neovim_editor_terminal_ui_app_defaults_mux_backend_from_target_terminal() {
        let sample = r#"
[app.editor.neovim]
enabled = true
[app.editor.neovim.ui.terminal]
app = "alacritty"
"#;
        let parsed: Config = toml::from_str(sample).expect("sample config should parse");
        assert_eq!(
            editor_terminal_ui_app_from(&parsed, &["nvim", "neovim"]),
            Some("alacritty".to_string())
        );
        assert_eq!(
            editor_terminal_mux_backend_from(&parsed, &["nvim", "neovim"]),
            Some(TerminalMuxBackend::Tmux)
        );
    }

    #[test]
    fn enabled_editor_ui_terminal_target_enables_terminal_chain() {
        let sample = r#"
[app.editor.neovim]
enabled = true
[app.editor.neovim.ui.terminal]
app = "wezterm"
"#;
        let parsed: Config = toml::from_str(sample).expect("sample config should parse");
        assert!(terminal_chain_enabled_from(
            &parsed,
            &["wezterm", "terminal"]
        ));
        assert!(!terminal_chain_enabled_from(
            &parsed,
            &["kitty", "terminal"]
        ));
    }

    #[test]
    fn wm_backend_deserializes_any_builtin_name() {
        assert_eq!(
            toml::from_str::<WmConfig>("enabled_integration = \"niri\"")
                .unwrap()
                .enabled_integration,
            WmBackend::Niri
        );
        assert_eq!(
            toml::from_str::<WmConfig>("enabled_integration = \"yabai\"")
                .unwrap()
                .enabled_integration,
            WmBackend::Yabai
        );
        assert_eq!(
            toml::from_str::<WmConfig>("enabled_integration = \"hyprland\"")
                .unwrap()
                .enabled_integration,
            WmBackend::Hyprland
        );
    }

    #[test]
    fn mangowc_wm_backend_config_accepts_value() {
        assert_eq!(
            toml::from_str::<WmConfig>("enabled_integration = \"mangowc\"")
                .unwrap()
                .enabled_integration,
            WmBackend::Mangowc
        );
    }
}

#[test]
fn repo_config_example_toml_parses() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("config.example.toml");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
    let _: Config = toml::from_str(&raw)
        .unwrap_or_else(|err| panic!("failed to parse {}: {err}", path.display()));
}
