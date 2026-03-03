pub mod emacs;
pub mod librefox;
pub mod nvim;
pub mod tmux;
pub mod vscode;
pub mod wezterm;

use crate::engine::runtime;
use crate::logging;
use crate::config::AppSection;

use emacs::EmacsBackend;
use librefox::Librefox;
use nvim::Nvim;
use tmux::Tmux;
use vscode::Vscode;
use wezterm::WeztermBackend;

pub use crate::engine::contracts::{
    unsupported_operation, AdapterCapabilities, AppCapabilities, AppKind, DeepApp,
    MergeExecutionMode, MergePreparation, MoveDecision, TearResult,
};

/// Developer note for adding a new adapter:
/// 1. Implement `DeepApp` and declare all booleans in `capabilities`.
/// 2. Keep unsupported operations disabled in `capabilities` so the orchestrator
///    classify them as `Unsupported` without runtime probes.
/// 3. Add adapter tests that cover focus/move/resize behavior and precedence.

/// Find descendant PIDs whose /proc/<pid>/comm matches `name`.
pub(crate) fn find_descendants_by_comm(pid: u32, name: &str) -> Vec<u32> {
    runtime::find_descendants_by_comm(pid, name)
}

// ---------------------------------------------------------------------------
// App resolution
// ---------------------------------------------------------------------------

struct DirectAdapterSpec {
    name: &'static str,
    aliases: &'static [&'static str],
    app_ids: &'static [&'static str],
    section: AppSection,
    build: fn() -> Box<dyn DeepApp>,
}

fn build_editor() -> Box<dyn DeepApp> {
    Box::new(EmacsBackend)
}

fn build_librefox() -> Box<dyn DeepApp> {
    Box::new(Librefox)
}

fn build_vscode() -> Box<dyn DeepApp> {
    Box::new(Vscode)
}

const DIRECT_ADAPTERS: &[DirectAdapterSpec] = &[
    DirectAdapterSpec {
        name: emacs::ADAPTER_NAME,
        aliases: emacs::ADAPTER_ALIASES,
        app_ids: emacs::APP_IDS,
        section: AppSection::Editor,
        build: build_editor,
    },
    DirectAdapterSpec {
        name: "librefox",
        aliases: &["librefox"],
        app_ids: &["librewolf", "LibreWolf", "firefox", "Firefox"],
        section: AppSection::Browser,
        build: build_librefox,
    },
    DirectAdapterSpec {
        name: "vscode",
        aliases: &["vscode"],
        app_ids: &["code", "code-url-handler", "Code", "code-oss"],
        section: AppSection::Editor,
        build: build_vscode,
    },
];

/// Baseline adapters used to seed runtime domains even when the focused window
/// does not currently belong to that app kind.
pub fn default_domain_adapters() -> Vec<Box<dyn DeepApp>> {
    vec![Box::new(WeztermBackend), Box::new(EmacsBackend)]
}

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

        if !crate::config::app_integration_enabled(spec.section, spec.aliases) {
            logging::debug(format!(
                "resolve_chain: direct adapter '{}' disabled via config",
                spec.name
            ));
            return None;
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
///   returns `[EmacsBackend]`
pub fn resolve_chain(app_id: &str, pid: u32, title: &str) -> Vec<Box<dyn DeepApp>> {
    logging::debug(format!(
        "resolve_chain: app_id={} pid={} title={}",
        app_id, pid, title
    ));
    let preferred = preferred_adapter_name();

    if wezterm::APP_IDS.contains(&app_id) {
        if let Some(preferred) = preferred.as_deref() {
            if !matches_adapter_alias(preferred, wezterm::ADAPTER_ALIASES) {
                logging::debug(format!(
                    "resolve_chain: adapter override '{}' disables terminal chain",
                    preferred
                ));
                return vec![];
            }
        }
        if !crate::config::app_integration_enabled(AppSection::Terminal, wezterm::ADAPTER_ALIASES) {
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
    let fg_hint = WeztermBackend::active_foreground_process(terminal_pid);
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
        chain.push(Box::new(WeztermBackend));
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
    chain.push(Box::new(WeztermBackend));
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

    use crate::adapters::apps::{emacs, wezterm};

    use super::resolve_chain;

    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::utils::env_guard()
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

        let chain = resolve_chain(emacs::APP_IDS[0], 0, "");
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].adapter_name(), emacs::ADAPTER_NAME);

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

        let chain = resolve_chain(emacs::APP_IDS[0], 0, "");
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

        let chain = resolve_chain(wezterm::APP_IDS[0], 0, "");
        assert!(chain.is_empty());

        restore_env("XDG_CONFIG_DIR", old_xdg);
        restore_env("NIRI_DEEP_CONFIG", old_override);
        crate::config::prepare().expect("config should reload");
        let _ = std::fs::remove_dir_all(root);
    }
}
