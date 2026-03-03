use anyhow::{anyhow, Context, Result};
use etcetera::base_strategy::{choose_base_strategy, BaseStrategy};
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
use std::path::{Path, PathBuf};
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

fn resolve_config_path() -> Result<(PathBuf, bool)> {
    if let Some(explicit) = std::env::var_os("NIRI_DEEP_CONFIG").map(PathBuf::from) {
        return Ok((explicit, true));
    }

    let strategy = choose_base_strategy().context("failed to resolve config directory")?;
    Ok((
        strategy.config_dir().join("niri-deep").join("config.toml"),
        false,
    ))
}

fn load_config_from(path: &Path) -> Result<Config> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config file {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("failed to parse config file {}", path.display()))
}

pub fn prepare() -> Result<()> {
    let (path, explicit) = resolve_config_path()?;
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
    pub enable_override: Option<bool>,
}

impl MuxPolicy {
    pub fn bridge_enable_override(self) -> Option<bool> {
        self.enable_override.or_else(|| match self.backend {
            TerminalMuxBackend::Wezterm => None,
            TerminalMuxBackend::Tmux | TerminalMuxBackend::Zellij | TerminalMuxBackend::Kitty => {
                Some(false)
            }
        })
    }
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

fn profile_by_aliases<'a, T>(profiles: &'a HashMap<String, T>, aliases: &[&str]) -> Option<&'a T> {
    let aliases = alias_keys(aliases);
    if aliases.is_empty() {
        return None;
    }

    for alias in &aliases {
        if let Some(profile) = profiles.get(alias) {
            return Some(profile);
        }
    }

    for (key, profile) in profiles {
        if aliases.iter().any(|alias| key.eq_ignore_ascii_case(alias)) {
            return Some(profile);
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
        .unwrap_or(profiles.is_empty())
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

pub fn app_integration_enabled(section: AppSection, aliases: &[&str]) -> bool {
    app_enabled_from(&read_config(), section, aliases)
}

fn pane_policy_from(cfg: &Config, section: AppSection, aliases: &[&str]) -> PanePolicy {
    match section {
        AppSection::Terminal => {
            let profile = profile_by_aliases(&cfg.app.terminal, aliases);
            let integration_enabled = profile
                .map(|profile| profile.enabled)
                .unwrap_or(cfg.app.terminal.is_empty());
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
            let integration_enabled = profile
                .map(|profile| profile.enabled)
                .unwrap_or(cfg.app.editor.is_empty());
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

fn mux_policy_from(cfg: &Config, aliases: &[&str]) -> MuxPolicy {
    let profile = profile_by_aliases(&cfg.app.terminal, aliases);
    let integration_enabled = profile
        .map(|profile| profile.enabled)
        .unwrap_or(cfg.app.terminal.is_empty());
    MuxPolicy {
        integration_enabled,
        backend: profile
            .and_then(|profile| profile.variant.mux_backend)
            .unwrap_or(TerminalMuxBackend::Wezterm),
        enable_override: profile.and_then(|profile| profile.variant.mux.enable),
    }
}

pub fn mux_policy_for(aliases: &[&str]) -> MuxPolicy {
    mux_policy_from(&read_config(), aliases)
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
"#;
        let parsed: Config = toml::from_str(sample).expect("sample config should parse");
        assert!(parsed.app.terminal.contains_key("wezterm"));
        assert!(parsed.app.editor.contains_key("emacs"));
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
    fn unmatched_alias_defaults_to_disabled_when_section_is_explicitly_configured() {
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
}
