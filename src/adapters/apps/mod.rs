pub mod emacs;
pub mod librefox;
pub mod nvim;
pub mod tmux;
pub mod vscode;
pub mod wezterm;

use crate::config::AppSection;
use crate::engine::runtime;
use crate::logging;

use emacs::EmacsBackend;
use librefox::Librefox;
use nvim::Nvim;
use tmux::Tmux;
use vscode::Vscode;
use wezterm::WeztermBackend;

pub use crate::engine::contracts::{
    unsupported_operation, AdapterCapabilities, AppAdapter, AppCapabilities, AppKind, DeepApp,
    MergeExecutionMode, MergePreparation, MoveDecision, TearResult, TopologyModifier,
    TopologyProvider, TopologySnapshot,
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

struct PolicyBoundApp {
    inner: Box<dyn AppAdapter>,
    scope: Option<(AppSection, &'static [&'static str])>,
}

impl PolicyBoundApp {
    fn new(inner: Box<dyn AppAdapter>) -> Self {
        let scope = inner.config_aliases().map(|aliases| {
            let section = match inner.kind() {
                AppKind::Browser => AppSection::Browser,
                AppKind::Editor => AppSection::Editor,
                AppKind::Terminal => AppSection::Terminal,
            };
            (section, aliases)
        });
        Self { inner, scope }
    }

    fn pane_policy(&self) -> Option<crate::config::PanePolicy> {
        let (section, aliases) = self.scope?;
        match section {
            AppSection::Browser => None,
            AppSection::Editor | AppSection::Terminal => {
                Some(crate::config::pane_policy_for(section, aliases))
            }
        }
    }
}

impl DeepApp for PolicyBoundApp {
    fn adapter_name(&self) -> &'static str {
        self.inner.adapter_name()
    }

    fn config_aliases(&self) -> Option<&'static [&'static str]> {
        self.inner.config_aliases()
    }

    fn kind(&self) -> AppKind {
        self.inner.kind()
    }

    fn capabilities(&self) -> AdapterCapabilities {
        let mut capabilities = self.inner.capabilities();
        if let Some(policy) = self.pane_policy() {
            capabilities.focus &= policy.focus_capability();
            capabilities.move_internal &= policy.move_capability();
            capabilities.resize_internal &= policy.resize_capability();
            capabilities.rearrange &= policy.move_capability();
            capabilities.tear_out &= policy.tear_out_capability();
        }
        capabilities
    }

}

impl TopologyProvider for PolicyBoundApp {
    fn can_focus(&self, dir: crate::engine::topology::Direction, pid: u32) -> anyhow::Result<bool> {
        if let Some(policy) = self.pane_policy() {
            if !policy.focus_allowed(dir) {
                return Ok(false);
            }
        }
        TopologyProvider::can_focus(self.inner.as_ref(), dir, pid)
    }

    fn move_decision(
        &self,
        dir: crate::engine::topology::Direction,
        pid: u32,
    ) -> anyhow::Result<MoveDecision> {
        if let Some(policy) = self.pane_policy() {
            if !policy.move_allowed(dir) {
                return Ok(MoveDecision::Passthrough);
            }
            let decision = TopologyProvider::move_decision(self.inner.as_ref(), dir, pid)?;
            if matches!(decision, MoveDecision::TearOut) && !policy.tear_out_capability() {
                return Ok(MoveDecision::Passthrough);
            }
            return Ok(decision);
        }
        TopologyProvider::move_decision(self.inner.as_ref(), dir, pid)
    }

    fn can_resize(
        &self,
        dir: crate::engine::topology::Direction,
        grow: bool,
        pid: u32,
    ) -> anyhow::Result<bool> {
        if let Some(policy) = self.pane_policy() {
            if !policy.resize_allowed(dir) {
                return Ok(false);
            }
        }
        TopologyProvider::can_resize(self.inner.as_ref(), dir, grow, pid)
    }
}

impl TopologyModifier for PolicyBoundApp {
    fn focus(&self, dir: crate::engine::topology::Direction, pid: u32) -> anyhow::Result<()> {
        if let Some(policy) = self.pane_policy() {
            if !policy.focus_allowed(dir) {
                return Err(unsupported_operation(self.adapter_name(), "focus"));
            }
        }
        TopologyModifier::focus(self.inner.as_ref(), dir, pid)
    }

    fn move_internal(
        &self,
        dir: crate::engine::topology::Direction,
        pid: u32,
    ) -> anyhow::Result<()> {
        if let Some(policy) = self.pane_policy() {
            if !policy.move_allowed(dir) {
                return Err(unsupported_operation(self.adapter_name(), "move_internal"));
            }
        }
        TopologyModifier::move_internal(self.inner.as_ref(), dir, pid)
    }

    fn resize_internal(
        &self,
        dir: crate::engine::topology::Direction,
        grow: bool,
        step: i32,
        pid: u32,
    ) -> anyhow::Result<()> {
        if let Some(policy) = self.pane_policy() {
            if !policy.resize_allowed(dir) {
                return Err(unsupported_operation(
                    self.adapter_name(),
                    "resize_internal",
                ));
            }
        }
        TopologyModifier::resize_internal(self.inner.as_ref(), dir, grow, step, pid)
    }

    fn rearrange(&self, dir: crate::engine::topology::Direction, pid: u32) -> anyhow::Result<()> {
        if let Some(policy) = self.pane_policy() {
            if !policy.move_allowed(dir) {
                return Err(unsupported_operation(self.adapter_name(), "rearrange"));
            }
        }
        TopologyModifier::rearrange(self.inner.as_ref(), dir, pid)
    }

    fn move_out(
        &self,
        dir: crate::engine::topology::Direction,
        pid: u32,
    ) -> anyhow::Result<TearResult> {
        if let Some(policy) = self.pane_policy() {
            if !policy.move_allowed(dir) || !policy.tear_out_capability() {
                return Err(unsupported_operation(self.adapter_name(), "move_out"));
            }
        }
        TopologyModifier::move_out(self.inner.as_ref(), dir, pid)
    }

    fn merge_into(
        &self,
        dir: crate::engine::topology::Direction,
        source_pid: u32,
    ) -> anyhow::Result<()> {
        TopologyModifier::merge_into(self.inner.as_ref(), dir, source_pid)
    }

    fn merge_execution_mode(&self) -> MergeExecutionMode {
        TopologyModifier::merge_execution_mode(self.inner.as_ref())
    }

    fn prepare_merge(
        &self,
        source_pid: Option<crate::engine::runtime::ProcessId>,
    ) -> anyhow::Result<MergePreparation> {
        TopologyModifier::prepare_merge(self.inner.as_ref(), source_pid)
    }

    fn augment_merge_preparation_for_target(
        &self,
        preparation: MergePreparation,
        target_window_id: Option<u64>,
    ) -> MergePreparation {
        TopologyModifier::augment_merge_preparation_for_target(
            self.inner.as_ref(),
            preparation,
            target_window_id,
        )
    }

    fn merge_into_target(
        &self,
        dir: crate::engine::topology::Direction,
        source_pid: Option<crate::engine::runtime::ProcessId>,
        target_pid: Option<crate::engine::runtime::ProcessId>,
        preparation: MergePreparation,
    ) -> anyhow::Result<()> {
        TopologyModifier::merge_into_target(
            self.inner.as_ref(),
            dir,
            source_pid,
            target_pid,
            preparation,
        )
    }
}

fn bind_policy(app: Box<dyn AppAdapter>) -> Box<dyn AppAdapter> {
    Box::new(PolicyBoundApp::new(app))
}

// ---------------------------------------------------------------------------
// App resolution
// ---------------------------------------------------------------------------

struct DirectAdapterSpec {
    name: &'static str,
    aliases: &'static [&'static str],
    app_ids: &'static [&'static str],
    section: AppSection,
    build: fn() -> Box<dyn AppAdapter>,
}

fn build_editor() -> Box<dyn AppAdapter> {
    Box::new(EmacsBackend)
}

fn build_librefox() -> Box<dyn AppAdapter> {
    Box::new(Librefox)
}

fn build_vscode() -> Box<dyn AppAdapter> {
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
pub fn default_domain_adapters() -> Vec<Box<dyn AppAdapter>> {
    vec![
        bind_policy(Box::new(WeztermBackend)),
        bind_policy(Box::new(EmacsBackend)),
    ]
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

fn resolve_direct_adapter(app_id: &str, preferred: Option<&str>) -> Option<Box<dyn AppAdapter>> {
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

        return Some(bind_policy((spec.build)()));
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
pub fn resolve_chain(app_id: &str, pid: u32, title: &str) -> Vec<Box<dyn AppAdapter>> {
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
fn resolve_terminal_chain(terminal_pid: u32) -> Vec<Box<dyn AppAdapter>> {
    let mut chain: Vec<Box<dyn AppAdapter>> = Vec::new();

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
        chain.push(bind_policy(Box::new(WeztermBackend)));
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
                        chain.push(bind_policy(Box::new(nvim)));
                    }
                }
                chain.push(bind_policy(Box::new(tmux)));
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
                    chain.push(bind_policy(Box::new(nvim)));
                }
            }
        }
        _ => {}
    }

    // Always include the terminal layer as outermost fallback so that
    // terminal-native pane operations can run when inner layers passthrough.
    chain.push(bind_policy(Box::new(WeztermBackend)));
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

    use crate::adapters::apps::{
        emacs, librefox::Librefox, nvim::Nvim, tmux::Tmux, vscode::Vscode, wezterm,
        TopologyModifier, TopologyProvider,
    };

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
    fn adapters_implement_topology_traits() {
        fn assert_topology_contracts<T: TopologyProvider + TopologyModifier>() {}
        assert_topology_contracts::<emacs::EmacsBackend>();
        assert_topology_contracts::<wezterm::WeztermBackend>();
        assert_topology_contracts::<Tmux>();
        assert_topology_contracts::<Nvim>();
        assert_topology_contracts::<Librefox>();
        assert_topology_contracts::<Vscode>();
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

        let old_override = set_env(
            "NIRI_DEEP_CONFIG",
            Some(config_dir.join("config.toml").to_str().expect("utf-8 path")),
        );
        crate::config::prepare().expect("config should load");

        let chain = resolve_chain(emacs::APP_IDS[0], 0, "");
        assert!(chain.is_empty());

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

        let old_override = set_env(
            "NIRI_DEEP_CONFIG",
            Some(config_dir.join("config.toml").to_str().expect("utf-8 path")),
        );
        crate::config::prepare().expect("config should load");

        let chain = resolve_chain(wezterm::APP_IDS[0], 0, "");
        assert!(chain.is_empty());

        restore_env("NIRI_DEEP_CONFIG", old_override);
        crate::config::prepare().expect("config should reload");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn resolved_editor_capabilities_follow_config_policy() {
        let _guard = env_guard();
        let root = unique_temp_dir("policy-editor");
        let config_dir = root.join("niri-deep");
        std::fs::create_dir_all(&config_dir).expect("config dir should be created");
        std::fs::write(
            config_dir.join("config.toml"),
            r#"
[app.editor.emacs]
enabled = true
focus.internal_panes.enabled = false
"#,
        )
        .expect("config file should be writable");

        let old_override = set_env(
            "NIRI_DEEP_CONFIG",
            Some(config_dir.join("config.toml").to_str().expect("utf-8 path")),
        );
        crate::config::prepare().expect("config should load");

        let chain = resolve_chain(emacs::APP_IDS[0], 0, "");
        assert_eq!(chain.len(), 1);
        assert!(!chain[0].capabilities().focus);

        restore_env("NIRI_DEEP_CONFIG", old_override);
        crate::config::prepare().expect("config should reload");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn resolved_terminal_capabilities_follow_config_policy() {
        let _guard = env_guard();
        let root = unique_temp_dir("policy-terminal");
        let config_dir = root.join("niri-deep");
        std::fs::create_dir_all(&config_dir).expect("config dir should be created");
        std::fs::write(
            config_dir.join("config.toml"),
            r#"
[app.terminal.wezterm]
enabled = true
resize.internal_panes.enabled = false
"#,
        )
        .expect("config file should be writable");

        let old_override = set_env(
            "NIRI_DEEP_CONFIG",
            Some(config_dir.join("config.toml").to_str().expect("utf-8 path")),
        );
        crate::config::prepare().expect("config should load");

        let chain = resolve_chain(wezterm::APP_IDS[0], 0, "");
        assert!(!chain.is_empty());
        let wezterm = chain
            .iter()
            .find(|app| app.adapter_name() == wezterm::ADAPTER_NAME)
            .expect("wezterm adapter should be in chain");
        assert!(!wezterm.capabilities().resize_internal);

        restore_env("NIRI_DEEP_CONFIG", old_override);
        crate::config::prepare().expect("config should reload");
        let _ = std::fs::remove_dir_all(root);
    }
}
