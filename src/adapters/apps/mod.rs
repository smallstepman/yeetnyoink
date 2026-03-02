#[path = "emacs.rs"]
pub mod editor_backend;
pub mod librefox;
pub mod nvim;
#[path = "wezterm.rs"]
pub mod terminal_backend;
pub mod tmux;
pub mod vscode;

use crate::engine::runtime;
use crate::logging;

use editor_backend::EditorBackend;
use librefox::Librefox;
use nvim::Nvim;
use terminal_backend::TerminalBackend;
use tmux::Tmux;
use vscode::Vscode;

use anyhow::{anyhow, Result};

use crate::engine::direction::Direction;
use crate::engine::runtime::ProcessId;

/// Developer note for adding a new adapter:
/// 1. Implement `DeepApp` and declare all booleans in `capabilities`.
/// 2. Keep unsupported operations disabled in `capabilities` so the orchestrator
///    classify them as `Unsupported` without runtime probes.
/// 3. Add adapter tests that cover focus/move/resize behavior and precedence.

/// What the app wants to do for a move operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveDecision {
    /// Swap/move internally within the app.
    Internal,
    /// Rearrange panes: no neighbor in move direction, but panes exist
    /// in other directions. Reorganize layout (e.g. horizontal → vertical).
    Rearrange,
    /// At the edge with multiple splits along the move axis — tear the buffer out.
    TearOut,
    /// Nothing to do internally, fall through to the compositor.
    Passthrough,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppKind {
    Browser,
    Editor,
    Terminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeExecutionMode {
    SourceFocused,
    TargetFocused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergePreparation {
    None,
    TerminalMuxSourcePane {
        pane_id: u64,
        target_window_id: Option<u64>,
    },
}

/// Result of tearing a buffer/pane out of an app.
pub struct TearResult {
    /// Command to spawn the torn-out content as a new window.
    /// None if the app already created the window itself.
    pub spawn_command: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowManagerCapabilities {
    pub probe: bool,
    pub focus: bool,
    pub move_internal: bool,
    pub resize_internal: bool,
    pub rearrange: bool,
    pub tear_out: bool,
    pub merge: bool,
}

impl WindowManagerCapabilities {
    pub const fn none() -> Self {
        Self {
            probe: false,
            focus: false,
            move_internal: false,
            resize_internal: false,
            rearrange: false,
            tear_out: false,
            merge: false,
        }
    }
}

pub type AdapterCapabilities = WindowManagerCapabilities;

/// Trait for apps that support deep focus/move integration with the current WM domain.
pub trait DeepApp: Send {
    /// Human-readable adapter name used in diagnostics.
    fn adapter_name(&self) -> &'static str;

    /// High-level app category used by domain resolution policy.
    fn kind(&self) -> AppKind;

    /// Explicit capability declaration used by orchestrator routing.
    fn capabilities(&self) -> AdapterCapabilities;

    /// Whether the app can navigate internally in this direction.
    fn can_focus(&self, dir: Direction, pid: u32) -> Result<bool>;

    /// Navigate internally in the given direction.
    fn focus(&self, dir: Direction, pid: u32) -> Result<()>;

    /// Decide what to do for a move operation in this direction.
    fn move_decision(&self, dir: Direction, pid: u32) -> Result<MoveDecision>;

    /// Swap/move the current buffer internally.
    fn move_internal(&self, dir: Direction, pid: u32) -> Result<()>;

    /// Whether the app can resize internally in this direction.
    fn can_resize(&self, _dir: Direction, _grow: bool, _pid: u32) -> Result<bool> {
        Ok(false)
    }

    /// Resize internally in the given direction.
    fn resize_internal(&self, _dir: Direction, _grow: bool, _step: i32, _pid: u32) -> Result<()> {
        Err(unsupported_operation(
            self.adapter_name(),
            "resize_internal",
        ))
    }

    /// Rearrange panes: move the current pane to `dir` by reorganizing the layout.
    /// e.g. [A|B*] move north → [B*-A] (horizontal to vertical).
    fn rearrange(&self, _dir: Direction, _pid: u32) -> Result<()> {
        Err(unsupported_operation(self.adapter_name(), "rearrange"))
    }

    /// Tear the current buffer/pane out, returning spawn info for a new window.
    fn move_out(&self, dir: Direction, pid: u32) -> Result<TearResult>;

    /// Merge the current window's content into the adjacent same-app window,
    /// and close the source. Called while the source window is still focused.
    /// `dir` is the direction toward the merge target.
    fn merge_into(&self, _dir: Direction, _source_pid: u32) -> Result<()> {
        Err(unsupported_operation(self.adapter_name(), "merge_into"))
    }

    /// Whether merge should execute while source or target window is focused.
    fn merge_execution_mode(&self) -> MergeExecutionMode {
        MergeExecutionMode::SourceFocused
    }

    /// Capture source-side merge state before focus moves to target window.
    fn prepare_merge(&self, _source_pid: Option<ProcessId>) -> Result<MergePreparation> {
        Ok(MergePreparation::None)
    }

    /// Merge source content into target window context.
    fn merge_into_target(
        &self,
        dir: Direction,
        source_pid: Option<ProcessId>,
        _target_pid: Option<ProcessId>,
        _preparation: MergePreparation,
    ) -> Result<()> {
        self.merge_into(dir, legacy_pid(source_pid))
    }
}

pub fn unsupported_operation(adapter: &str, operation: &str) -> anyhow::Error {
    anyhow!(
        "adapter '{}' does not support operation '{}'",
        adapter,
        operation
    )
}

fn legacy_pid(pid: Option<ProcessId>) -> u32 {
    pid.map(ProcessId::get).unwrap_or(0)
}
/// Find descendant PIDs whose /proc/<pid>/comm matches `name`.
pub fn find_descendants_by_comm(pid: u32, name: &str) -> Vec<u32> {
    runtime::find_descendants_by_comm(pid, name)
}

// ---------------------------------------------------------------------------
// App resolution
// ---------------------------------------------------------------------------

struct DirectAdapterSpec {
    name: &'static str,
    aliases: &'static [&'static str],
    app_ids: &'static [&'static str],
    build: fn() -> Box<dyn DeepApp>,
}

fn build_editor() -> Box<dyn DeepApp> {
    Box::new(EditorBackend)
}

fn build_librefox() -> Box<dyn DeepApp> {
    Box::new(Librefox)
}

fn build_vscode() -> Box<dyn DeepApp> {
    Box::new(Vscode)
}

const DIRECT_ADAPTERS: &[DirectAdapterSpec] = &[
    DirectAdapterSpec {
        name: editor_backend::ADAPTER_NAME,
        aliases: editor_backend::ADAPTER_ALIASES,
        app_ids: editor_backend::APP_IDS,
        build: build_editor,
    },
    DirectAdapterSpec {
        name: "librefox",
        aliases: &["librefox"],
        app_ids: &["librewolf", "LibreWolf", "firefox", "Firefox"],
        build: build_librefox,
    },
    DirectAdapterSpec {
        name: "vscode",
        aliases: &["vscode"],
        app_ids: &["code", "code-url-handler", "Code", "code-oss"],
        build: build_vscode,
    },
];

fn preferred_adapter_name() -> Option<String> {
    crate::config::app_adapter_override().and_then(|raw| {
        let normalized = raw.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            None
        } else {
            Some(normalized)
        }
    })
}

fn matches_adapter_alias(preferred: &str, aliases: &[&str]) -> bool {
    aliases.iter().any(|candidate| *candidate == preferred)
}

fn resolve_direct_adapter(app_id: &str, preferred: Option<&str>) -> Option<Box<dyn DeepApp>> {
    for spec in DIRECT_ADAPTERS {
        if !spec.app_ids.iter().any(|candidate| *candidate == app_id) {
            continue;
        }

        if let Some(preferred) = preferred {
            if !matches_adapter_alias(preferred, spec.aliases) {
                logging::debug(format!(
                    "resolve_chain: adapter override '{}' does not match direct adapter '{}'",
                    preferred, spec.name
                ));
                return None;
            }
        }

        return Some((spec.build)());
    }

    None
}

/// Resolve a chain of DeepApp handlers for a window, innermost-first.
///
/// For a terminal running `terminal → zsh → tmux → nvim`:
///   returns `[Nvim { .. }, Tmux { .. }]`
///
/// For a non-terminal editor:
///   returns `[EditorBackend]`
pub fn resolve_chain(app_id: &str, pid: u32, title: &str) -> Vec<Box<dyn DeepApp>> {
    logging::debug(format!(
        "resolve_chain: app_id={} pid={} title={}",
        app_id, pid, title
    ));
    let preferred = preferred_adapter_name();

    if terminal_backend::APP_IDS.contains(&app_id) {
        if let Some(preferred) = preferred.as_deref() {
            if !matches_adapter_alias(preferred, terminal_backend::ADAPTER_ALIASES) {
                logging::debug(format!(
                    "resolve_chain: adapter override '{}' disables terminal chain",
                    preferred
                ));
                return vec![];
            }
        }
        if !crate::config::terminal_enabled() {
            logging::debug("resolve_chain: terminal integration disabled via config");
            return vec![];
        }
        let chain = resolve_terminal_chain(pid);
        logging::debug(format!("resolve_chain: terminal depth={}", chain.len()));
        return chain;
    }

    if let Some(app) = resolve_direct_adapter(app_id, preferred.as_deref()) {
        logging::debug("resolve_chain: direct app match depth=1");
        return vec![app];
    }

    logging::debug("resolve_chain: no deep app match depth=0");
    vec![]
}

/// Resolve the app chain for a terminal window using multiplexer state,
/// to directly identify the active pane's foreground process, avoiding title heuristics.
fn resolve_terminal_chain(terminal_pid: u32) -> Vec<Box<dyn DeepApp>> {
    let mut chain: Vec<Box<dyn DeepApp>> = Vec::new();

    // Ask the terminal multiplexer backend for active pane foreground process name.
    let fg_hint = TerminalBackend::active_foreground_process(terminal_pid);
    let fg_base = fg_hint
        .as_deref()
        .map(runtime::normalize_process_name)
        .unwrap_or_default();
    logging::debug(format!(
        "resolve_terminal_chain: pid={} fg_hint={:?} fg_base={}",
        terminal_pid, fg_hint, fg_base
    ));

    // Find shell children of the terminal process.
    let shells: Vec<u32> = runtime::child_pids(terminal_pid)
        .into_iter()
        .filter(|&p| {
            std::fs::read_to_string(format!("/proc/{p}/comm"))
                .map(|c| matches!(c.trim(), "zsh" | "bash" | "fish"))
                .unwrap_or(false)
        })
        .collect();
    logging::debug(format!(
        "resolve_terminal_chain: shell_candidates={:?}",
        shells
    ));

    // Identify the shell owning the active pane. With a single shell (single
    // tab), take it directly. With multiple shells (multiple tabs), match by
    // the foreground process group reported by terminal multiplexer backend.
    let search_pid = if shells.len() <= 1 {
        shells.first().copied()
    } else if !fg_base.is_empty() {
        shells.iter().copied().find(|&shell_pid| {
            let Ok(stat) = std::fs::read_to_string(format!("/proc/{shell_pid}/stat")) else {
                return false;
            };
            let Some(tpgid) = runtime::parse_stat_tpgid(&stat) else {
                return false;
            };
            std::fs::read_to_string(format!("/proc/{tpgid}/comm"))
                .map(|c| runtime::normalize_process_name(c.trim()) == fg_base)
                .unwrap_or(false)
        })
    } else {
        None
    };

    let Some(search_pid) = search_pid else {
        logging::debug("resolve_terminal_chain: no focused shell match; using terminal layer only");
        chain.push(Box::new(TerminalBackend));
        return chain;
    };
    logging::debug(format!(
        "resolve_terminal_chain: selected shell pid={search_pid}"
    ));

    // Build the chain based on fg_base (most specific first).
    match fg_base.as_str() {
        "tmux" => {
            let tmux_pids = find_descendants_by_comm(search_pid, "tmux");
            logging::debug(format!(
                "resolve_terminal_chain: tmux descendants under shell {} => {:?}",
                search_pid, tmux_pids
            ));
            let found_tmux = tmux_pids
                .first()
                .and_then(|tmux_client_pid| Tmux::for_client_pid(*tmux_client_pid));
            if let Some(tmux) = found_tmux {
                if let Some(nvim_pid) = tmux.nvim_in_current_pane() {
                    if let Some(nvim) = Nvim::for_pid(nvim_pid) {
                        chain.push(Box::new(nvim));
                    }
                }
                chain.push(Box::new(tmux));
            }
        }
        "nvim" => {
            let nvim_pids = find_descendants_by_comm(search_pid, "nvim");
            logging::debug(format!(
                "resolve_terminal_chain: nvim descendants under shell {} => {:?}",
                search_pid, nvim_pids
            ));
            if let Some(&nvim_pid) = nvim_pids.first() {
                if let Some(nvim) = Nvim::for_pid(nvim_pid) {
                    chain.push(Box::new(nvim));
                }
            }
        }
        _ => {}
    }

    // Always include the terminal layer as outermost fallback so that
    // terminal-native pane operations can run when inner layers passthrough.
    chain.push(Box::new(TerminalBackend));
    logging::debug(format!(
        "resolve_terminal_chain: final depth={}",
        chain.len()
    ));

    chain
}

#[cfg(test)]
mod resolve_chain_tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use crate::adapters::apps::{editor_backend, terminal_backend};

    use super::resolve_chain;

    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::engine::test_support::env_guard()
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "niri-deep-app-resolve-{prefix}-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("temp dir should be created");
        path
    }

    fn set_env(key: &str, value: Option<&str>) -> Option<std::ffi::OsString> {
        let old = std::env::var_os(key);
        if let Some(value) = value {
            std::env::set_var(key, value);
        } else {
            std::env::remove_var(key);
        }
        old
    }

    fn restore_env(key: &str, old: Option<std::ffi::OsString>) {
        if let Some(old) = old {
            std::env::set_var(key, old);
        } else {
            std::env::remove_var(key);
        }
    }

    #[test]
    fn direct_match_without_override_returns_adapter() {
        let _guard = env_guard();
        let old_override = set_env("NIRI_DEEP_CONFIG", None);
        crate::config::prepare().expect("config should load");

        let chain = resolve_chain(editor_backend::APP_IDS[0], 0, "");
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].adapter_name(), editor_backend::ADAPTER_NAME);

        restore_env("NIRI_DEEP_CONFIG", old_override);
    }

    #[test]
    fn override_filters_non_matching_direct_adapter() {
        let _guard = env_guard();
        let root = unique_temp_dir("override-filter");
        let config_dir = root.join("niri-deep");
        std::fs::create_dir_all(&config_dir).expect("config dir should be created");
        std::fs::write(
            config_dir.join("config.toml"),
            r#"
[app.editor.vscode]
enabled = true
"#,
        )
        .expect("config file should be writable");

        let old_xdg = set_env("XDG_CONFIG_DIR", Some(root.to_str().expect("utf-8 path")));
        let old_override = set_env("NIRI_DEEP_CONFIG", None);
        crate::config::prepare().expect("config should load");

        let chain = resolve_chain(editor_backend::APP_IDS[0], 0, "");
        assert!(chain.is_empty());

        restore_env("XDG_CONFIG_DIR", old_xdg);
        restore_env("NIRI_DEEP_CONFIG", old_override);
        crate::config::prepare().expect("config should reload");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn override_applies_to_terminal_chain_selection() {
        let _guard = env_guard();
        let root = unique_temp_dir("override-terminal");
        let config_dir = root.join("niri-deep");
        std::fs::create_dir_all(&config_dir).expect("config dir should be created");
        std::fs::write(
            config_dir.join("config.toml"),
            r#"
[app.editor.editor]
enabled = true
"#,
        )
        .expect("config file should be writable");

        let old_xdg = set_env("XDG_CONFIG_DIR", Some(root.to_str().expect("utf-8 path")));
        let old_override = set_env("NIRI_DEEP_CONFIG", None);
        crate::config::prepare().expect("config should load");

        let chain = resolve_chain(terminal_backend::APP_IDS[0], 0, "");
        assert!(chain.is_empty());

        restore_env("XDG_CONFIG_DIR", old_xdg);
        restore_env("NIRI_DEEP_CONFIG", old_override);
        crate::config::prepare().expect("config should reload");
        let _ = std::fs::remove_dir_all(root);
    }
}
