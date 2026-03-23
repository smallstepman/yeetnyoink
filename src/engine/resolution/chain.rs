use crate::adapters::apps::{
    self, build_emacs,
    nvim::{self, Nvim},
    AppAdapter,
};
use crate::adapters::terminal_multiplexers::tmux::Tmux;
use crate::config::{AppSection, TerminalMuxBackend};
use crate::engine::resolution::policy::bind_app_policy;
use crate::engine::runtime::{self, ProcessId};
use crate::engine::topology::DomainId;
use crate::engine::transfer::WM_DOMAIN_ID;
use crate::logging;

pub struct RuntimeChainResolver;

static RUNTIME_CHAIN_RESOLVER: RuntimeChainResolver = RuntimeChainResolver;

pub fn runtime_chain_resolver() -> &'static RuntimeChainResolver {
    &RUNTIME_CHAIN_RESOLVER
}

pub fn resolve_app_chain(app_id: &str, pid: u32, title: &str) -> Vec<Box<dyn AppAdapter>> {
    runtime_chain_resolver().resolve_chain(app_id, pid, title)
}

pub fn default_app_domain_adapters() -> Vec<Box<dyn AppAdapter>> {
    runtime_chain_resolver().default_domain_adapters()
}

pub fn resolve_window_domain_id(
    app_id: Option<&str>,
    pid: Option<ProcessId>,
    title: Option<&str>,
) -> DomainId {
    runtime_chain_resolver().domain_id_for_window(app_id, pid, title)
}

fn preferred_terminal_adapter_name() -> Option<String> {
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

fn resolve_direct_adapter(app_id: &str) -> Option<Box<dyn AppAdapter>> {
    for spec in apps::DIRECT_ADAPTERS {
        if !spec.app_ids.iter().any(|candidate| *candidate == app_id) {
            continue;
        }

        if !crate::config::app_integration_enabled(spec.section, spec.aliases) {
            logging::debug(format!(
                "resolve_chain: direct adapter '{}' disabled via config",
                spec.name
            ));
            return None;
        }

        return Some(bind_app_policy((spec.build)()));
    }

    None
}

pub fn resolve_root_adapter(app_id: &str) -> Option<Box<dyn AppAdapter>> {
    if let Some(host) = apps::TERMINAL_HOSTS
        .iter()
        .find(|host| host.app_ids.contains(&app_id))
    {
        if !crate::config::terminal_chain_enabled_for(host.aliases) {
            logging::debug("resolve_root_adapter: terminal integration disabled via config");
            return None;
        }
        return Some(bind_app_policy((host.build)()));
    }

    resolve_direct_adapter(app_id)
}

fn tmux_candidate_pids(root_pid: u32) -> Vec<u32> {
    let mut candidates = Vec::new();
    if runtime::process_comm(root_pid).as_deref() == Some("tmux") {
        candidates.push(root_pid);
    }
    for pid in runtime::find_descendants_by_comm(root_pid, "tmux") {
        if !candidates.contains(&pid) {
            candidates.push(pid);
        }
    }
    candidates
}

fn resolve_tmux_for_root(
    root_pid: u32,
    terminal_launch_prefix: &[&str],
) -> (Vec<u32>, Option<Tmux>) {
    let candidates = tmux_candidate_pids(root_pid);
    let launch_prefix: Vec<String> = terminal_launch_prefix
        .iter()
        .map(|s| s.to_string())
        .collect();
    let tmux = candidates
        .iter()
        .copied()
        .find_map(|pid| Tmux::from_client_pid(pid, launch_prefix.clone()));
    (candidates, tmux)
}

fn shell_matches_foreground_tpgid(shell_pid: u32, fg_base: &str) -> bool {
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{shell_pid}/stat")) else {
        return false;
    };
    let Some(tpgid) = runtime::parse_stat_tpgid(&stat) else {
        return false;
    };
    runtime::process_comm(tpgid)
        .map(|comm| comm == fg_base)
        .unwrap_or(false)
}

#[cfg(any(test, not(target_os = "macos")))]
fn shell_pid_for_tty<F>(shells: &[u32], tty_name: Option<&str>, mut uses_tty: F) -> Option<u32>
where
    F: FnMut(u32, &str) -> bool,
{
    let tty_name = tty_name?.trim();
    if tty_name.is_empty() {
        return None;
    }
    shells
        .iter()
        .copied()
        .find(|&shell_pid| uses_tty(shell_pid, tty_name))
}

fn shell_pid_for_host_focused_tty(
    terminal_pid: u32,
    host: &apps::TerminalHostSpec,
    shells: &[u32],
) -> Option<u32> {
    let panes = crate::adapters::terminal_multiplexers::active_mux_provider(host.aliases)
        .list_panes_for_pid(terminal_pid)
        .ok()?;
    let focused_tty = crate::engine::contracts::TerminalPaneSnapshot::active_or_first(panes.iter())
        .and_then(|pane| pane.tty_name.as_deref());
    #[cfg(target_os = "macos")]
    let selected = {
        let _ = shells;
        focused_tty.and_then(runtime::shell_pid_for_tty_name)
    };
    #[cfg(not(target_os = "macos"))]
    let selected = shell_pid_for_tty(shells, focused_tty, runtime::process_uses_tty);
    if let (Some(shell_pid), Some(tty_name)) = (selected, focused_tty) {
        logging::debug(format!(
            "resolve_terminal_chain: selected shell pid={} from focused tty {}",
            shell_pid, tty_name
        ));
    }
    selected
}

fn push_nvim_for_pid(
    chain: &mut Vec<Box<dyn AppAdapter>>,
    nvim_pid: u32,
    mux_backend: TerminalMuxBackend,
) -> bool {
    if !crate::config::app_integration_enabled(AppSection::Editor, nvim::ADAPTER_ALIASES) {
        logging::debug("resolve_terminal_chain: nvim integration disabled via config");
        return false;
    }
    if let Some(nvim) = Nvim::for_pid(nvim_pid, mux_backend) {
        chain.push(bind_app_policy(Box::new(nvim)));
        true
    } else {
        false
    }
}

fn push_first_resolved_nvim_pid(
    chain: &mut Vec<Box<dyn AppAdapter>>,
    nvim_pids: impl IntoIterator<Item = u32>,
    mux_backend: TerminalMuxBackend,
) -> bool {
    for nvim_pid in nvim_pids {
        if push_nvim_for_pid(chain, nvim_pid, mux_backend) {
            return true;
        }
    }
    false
}

fn tmux_nvim_pid_for_roots(roots: &[u32], host: &apps::TerminalHostSpec) -> Option<u32> {
    for root_pid in roots {
        let (tmux_pids, found_tmux) = resolve_tmux_for_root(*root_pid, host.terminal_launch_prefix);
        logging::debug(format!(
            "resolve_terminal_chain: tmux nvim probe root={root_pid} candidates={tmux_pids:?}"
        ));
        let Some(tmux) = found_tmux else {
            continue;
        };
        if let Some(nvim_pid) = tmux.nvim_in_current_pane() {
            logging::debug(format!(
                "resolve_terminal_chain: tmux focused-pane nvim pid={nvim_pid} root={root_pid}"
            ));
            return Some(nvim_pid);
        }
    }
    None
}

fn resolve_terminal_chain(
    terminal_pid: u32,
    host: &apps::TerminalHostSpec,
) -> Vec<Box<dyn AppAdapter>> {
    let mut chain: Vec<Box<dyn AppAdapter>> = Vec::new();
    let host_mux_backend = crate::config::mux_policy_for(host.aliases).backend;
    let nvim_terminal_app = crate::config::editor_terminal_ui_app_for(nvim::ADAPTER_ALIASES);
    let nvim_host_allowed = nvim_terminal_app
        .as_deref()
        .map(|app| matches_adapter_alias(app, host.aliases))
        .unwrap_or(true);
    let nvim_mux_backend = crate::config::editor_terminal_mux_backend_for(nvim::ADAPTER_ALIASES)
        .unwrap_or(host_mux_backend);
    let allow_tmux_resolution = matches!(host_mux_backend, TerminalMuxBackend::Tmux)
        || matches!(nvim_mux_backend, TerminalMuxBackend::Tmux);

    let fg_hint = {
        let _span = tracing::debug_span!(
            "chain_resolver.active_foreground_process",
            pid = terminal_pid
        )
        .entered();
        crate::adapters::terminal_multiplexers::active_foreground_process(host.aliases, terminal_pid)
    };
    let fg_base = fg_hint
        .as_deref()
        .map(runtime::normalize_process_name)
        .unwrap_or_default();
    logging::debug(format!(
        "resolve_terminal_chain: pid={} fg_hint={:?} fg_base={}",
        terminal_pid, fg_hint, fg_base
    ));

    #[cfg(target_os = "macos")]
    let focused_tty_shell = shell_pid_for_host_focused_tty(terminal_pid, host, &[]);
    #[cfg(not(target_os = "macos"))]
    let focused_tty_shell: Option<u32> = None;

    let shells: Vec<u32> = if focused_tty_shell.is_none() {
        let shells: Vec<u32> = {
            let _span = tracing::debug_span!("chain_resolver.shell_candidates", pid = terminal_pid)
                .entered();
            runtime::child_pids(terminal_pid)
                .into_iter()
                .filter(|&pid| runtime::is_shell_pid(pid))
                .collect()
        };
        logging::debug(format!(
            "resolve_terminal_chain: shell_candidates={:?}",
            shells
        ));
        shells
    } else {
        Vec::new()
    };

    #[cfg(target_os = "macos")]
    let search_pid = focused_tty_shell.or_else(|| {
        if shells.len() <= 1 {
            shells.first().copied()
        } else if !fg_base.is_empty() {
            shells
                .iter()
                .copied()
                .find(|&shell_pid| shell_matches_foreground_tpgid(shell_pid, &fg_base))
                .or_else(|| {
                    shells.iter().copied().find(|&shell_pid| {
                        !runtime::find_descendants_by_comm(shell_pid, &fg_base).is_empty()
                    })
                })
        } else {
            None
        }
    });

    #[cfg(not(target_os = "macos"))]
    let search_pid = if let Some(shell_pid) = shell_pid_for_host_focused_tty(terminal_pid, host, &shells)
    {
        Some(shell_pid)
    } else if shells.len() <= 1 {
        shells.first().copied()
    } else if !fg_base.is_empty() {
        shells
            .iter()
            .copied()
            .find(|&shell_pid| shell_matches_foreground_tpgid(shell_pid, &fg_base))
            .or_else(|| {
                shells.iter().copied().find(|&shell_pid| {
                    !runtime::find_descendants_by_comm(shell_pid, &fg_base).is_empty()
                })
            })
    } else {
        None
    };

    let Some(search_pid) = search_pid else {
        if allow_tmux_resolution {
            logging::debug(
                "resolve_terminal_chain: no focused shell match; trying tmux fallback on all shells",
            );
            // Shell disambiguation by tpgid fails when the fg process is running *inside* tmux,
            // because the shell's tpgid points to the tmux client, not the inner fg process.
            // Fall back: try every shell candidate (and terminal root for direct tmux children).
            let mut fallback_roots = shells.clone();
            if !fallback_roots.contains(&terminal_pid) {
                fallback_roots.push(terminal_pid);
            }
            'tmux_fallback: for root_pid in fallback_roots {
                let (tmux_pids, found_tmux) =
                    resolve_tmux_for_root(root_pid, host.terminal_launch_prefix);
                logging::debug(format!(
                    "resolve_terminal_chain: tmux fallback root={root_pid} candidates={tmux_pids:?}"
                ));
                if let Some(tmux) = found_tmux {
                    if nvim_host_allowed {
                        if let Some(nvim_pid) = tmux.nvim_in_current_pane() {
                            let _ = push_nvim_for_pid(&mut chain, nvim_pid, nvim_mux_backend);
                        }
                    }
                    chain.push(bind_app_policy(Box::new(tmux)));
                    break 'tmux_fallback;
                }
            }
        } else {
            logging::debug(
                "resolve_terminal_chain: no focused shell match and tmux resolution is disabled",
            );
        }
        chain.push(bind_app_policy((host.build)()));
        return chain;
    };
    logging::debug(format!(
        "resolve_terminal_chain: selected shell pid={search_pid}"
    ));

    match fg_base.as_str() {
        "tmux" if allow_tmux_resolution => {
            let (tmux_pids, found_tmux) =
                resolve_tmux_for_root(search_pid, host.terminal_launch_prefix);
            logging::debug(format!(
                "resolve_terminal_chain: tmux descendants under shell {} => {:?}",
                search_pid, tmux_pids
            ));
            if let Some(tmux) = found_tmux {
                if nvim_host_allowed {
                    if let Some(nvim_pid) = tmux.nvim_in_current_pane() {
                        let _ = push_nvim_for_pid(&mut chain, nvim_pid, nvim_mux_backend);
                    }
                }
                chain.push(bind_app_policy(Box::new(tmux)));
            }
        }
        "tmux" => {}
        "nvim" => {
            let mut resolved_nvim = false;
            if nvim_host_allowed && nvim_mux_backend == TerminalMuxBackend::Tmux {
                let mut tmux_roots = vec![search_pid, terminal_pid];
                for shell_pid in shells.iter().copied() {
                    if !tmux_roots.contains(&shell_pid) {
                        tmux_roots.push(shell_pid);
                    }
                }
                if let Some(nvim_pid) = tmux_nvim_pid_for_roots(&tmux_roots, host) {
                    resolved_nvim = push_nvim_for_pid(&mut chain, nvim_pid, nvim_mux_backend);
                }
            }
            if nvim_host_allowed && !resolved_nvim {
                let nvim_pids = runtime::find_descendants_by_comm(search_pid, "nvim");
                logging::debug(format!(
                    "resolve_terminal_chain: nvim descendants under shell {} => {:?}",
                    search_pid, nvim_pids
                ));
                let _ = push_first_resolved_nvim_pid(&mut chain, nvim_pids, nvim_mux_backend);
            }
        }
        _ => {
            if allow_tmux_resolution {
                // fg_base is an arbitrary process running inside a mux (e.g. "node", "python").
                // Try tmux detection under the shell when tmux is the configured backend.
                let (tmux_pids, found_tmux) =
                    resolve_tmux_for_root(search_pid, host.terminal_launch_prefix);
                logging::debug(format!(
                    "resolve_terminal_chain: fg={fg_base} tmux descendants under shell {search_pid} => {tmux_pids:?}"
                ));
                if let Some(tmux) = found_tmux {
                    if nvim_host_allowed {
                        if let Some(nvim_pid) = tmux.nvim_in_current_pane() {
                            let _ = push_nvim_for_pid(&mut chain, nvim_pid, nvim_mux_backend);
                        }
                    }
                    chain.push(bind_app_policy(Box::new(tmux)));
                }
            }
        }
    }

    chain.push(bind_app_policy((host.build)()));
    logging::debug(format!(
        "resolve_terminal_chain: final depth={}",
        chain.len()
    ));

    chain
}

impl RuntimeChainResolver {
    fn resolve_chain(&self, app_id: &str, pid: u32, title: &str) -> Vec<Box<dyn AppAdapter>> {
        let _span = tracing::debug_span!(
            "chain_resolver.resolve_chain",
            app_id = app_id,
            pid = pid,
            title = title
        )
        .entered();
        logging::debug(format!(
            "resolve_chain: app_id={} pid={} title={}",
            app_id, pid, title
        ));

        if let Some(host) = apps::TERMINAL_HOSTS
            .iter()
            .find(|host| host.app_ids.contains(&app_id))
        {
            if !crate::config::terminal_chain_enabled_for(host.aliases) {
                logging::debug("resolve_chain: terminal integration disabled via config");
                return vec![];
            }
            let chain = resolve_terminal_chain(pid, host);
            logging::debug(format!("resolve_chain: terminal depth={}", chain.len()));
            return chain;
        }

        if let Some(app) = resolve_direct_adapter(app_id) {
            logging::debug("resolve_chain: direct app match depth=1");
            return vec![app];
        }

        logging::debug("resolve_chain: no deep app match depth=0");
        vec![]
    }

    fn default_domain_adapters(&self) -> Vec<Box<dyn AppAdapter>> {
        let terminal_adapter = preferred_terminal_adapter_name()
            .as_deref()
            .and_then(|preferred| {
                apps::TERMINAL_HOSTS
                    .iter()
                    .find(|host| matches_adapter_alias(preferred, host.aliases))
            })
            .or_else(|| apps::TERMINAL_HOSTS.first())
            .map(|host| bind_app_policy((host.build)()))
            .expect("terminal host catalog should not be empty");
        vec![terminal_adapter, bind_app_policy(build_emacs())]
    }

    fn domain_id_for_window(
        &self,
        app_id: Option<&str>,
        pid: Option<ProcessId>,
        title: Option<&str>,
    ) -> DomainId {
        let app_id = app_id.unwrap_or_default();
        let title = title.unwrap_or_default();
        let owner_pid = pid.map(ProcessId::get).unwrap_or(0);
        if let Some(kind) = self
            .resolve_chain(app_id, owner_pid, title)
            .into_iter()
            .map(|adapter| adapter.kind())
            .next()
        {
            return super::domain::domain_id_for_app_kind(kind);
        }
        WM_DOMAIN_ID
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{default_app_domain_adapters, resolve_app_chain};
    use crate::adapters::apps::{alacritty, emacs, foot, ghostty, kitty, wezterm};

    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::utils::env_guard()
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "yeetnyoink-chain-resolver-{prefix}-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("temp dir should be created");
        path
    }

    fn load_config(path: &std::path::Path) -> crate::config::Config {
        let old = crate::config::snapshot();
        crate::config::prepare_with_path(Some(path)).expect("config should load");
        old
    }

    fn restore_config(old: crate::config::Config) {
        crate::config::install(old);
    }

    #[test]
    fn foot_override_selects_foot_default_terminal_domain_adapter() {
        let _guard = env_guard();
        let root = unique_temp_dir("foot-default-domain");
        let config_dir = root.join("yeetnyoink");
        fs::create_dir_all(&config_dir).expect("config dir should be created");
        fs::write(
            config_dir.join("config.toml"),
            r#"
[app.terminal.foot]
enabled = true
"#,
        )
        .expect("config file should be writable");
        let old_config = load_config(&config_dir.join("config.toml"));

        let adapters = default_app_domain_adapters();
        assert_eq!(
            adapters
                .first()
                .and_then(|adapter| adapter.config_aliases())
                .map(|aliases| aliases[0]),
            Some(foot::ADAPTER_ALIASES[0])
        );

        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn alacritty_override_selects_alacritty_default_terminal_domain_adapter() {
        let _guard = env_guard();
        let root = unique_temp_dir("alacritty-default-domain");
        let config_dir = root.join("yeetnyoink");
        fs::create_dir_all(&config_dir).expect("config dir should be created");
        fs::write(
            config_dir.join("config.toml"),
            r#"
[app.terminal.alacritty]
enabled = true
"#,
        )
        .expect("config file should be writable");
        let old_config = load_config(&config_dir.join("config.toml"));

        let adapters = default_app_domain_adapters();
        assert_eq!(
            adapters
                .first()
                .and_then(|adapter| adapter.config_aliases())
                .map(|aliases| aliases[0]),
            Some(alacritty::ADAPTER_ALIASES[0])
        );

        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn ghostty_override_selects_ghostty_default_terminal_domain_adapter() {
        let _guard = env_guard();
        let root = unique_temp_dir("ghostty-default-domain");
        let config_dir = root.join("yeetnyoink");
        fs::create_dir_all(&config_dir).expect("config dir should be created");
        fs::write(
            config_dir.join("config.toml"),
            r#"
[app.terminal.ghostty]
enabled = true
"#,
        )
        .expect("config file should be writable");
        let old_config = load_config(&config_dir.join("config.toml"));

        let adapters = default_app_domain_adapters();
        assert_eq!(
            adapters
                .first()
                .and_then(|adapter| adapter.config_aliases())
                .map(|aliases| aliases[0]),
            Some(ghostty::ADAPTER_ALIASES[0])
        );

        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn librewolf_browser_profile_enables_librewolf_direct_adapter() {
        let _guard = env_guard();
        let root = unique_temp_dir("librewolf-direct-adapter");
        let config_dir = root.join("yeetnyoink");
        fs::create_dir_all(&config_dir).expect("config dir should be created");
        fs::write(
            config_dir.join("config.toml"),
            r#"
[app.browser.librewolf]
enabled = true
"#,
        )
        .expect("config file should be writable");
        let old_config = load_config(&config_dir.join("config.toml"));

        let chain = resolve_app_chain("librewolf", 0, "LibreWolf");
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].adapter_name(), "librewolf");

        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn brave_browser_profile_enables_chromium_direct_adapter() {
        let _guard = env_guard();
        let root = unique_temp_dir("brave-direct-adapter");
        let config_dir = root.join("yeetnyoink");
        fs::create_dir_all(&config_dir).expect("config dir should be created");
        fs::write(
            config_dir.join("config.toml"),
            r#"
[app.browser.brave]
enabled = true
"#,
        )
        .expect("config file should be writable");
        let old_config = load_config(&config_dir.join("config.toml"));

        let chain = resolve_app_chain("brave-browser", 0, "Brave Browser");
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].adapter_name(), "chromium");

        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn shell_pid_for_tty_prefers_matching_shell() {
        let selected =
            super::shell_pid_for_tty(&[111, 222, 333], Some("/dev/pts/4"), |pid, tty| {
                tty == "/dev/pts/4" && pid == 222
            });
        assert_eq!(selected, Some(222));
    }

    #[test]
    fn shell_pid_for_tty_ignores_missing_or_blank_tty() {
        assert_eq!(
            super::shell_pid_for_tty(&[111, 222], None, |_pid, _tty| true),
            None
        );
        assert_eq!(
            super::shell_pid_for_tty(&[111, 222], Some("  "), |_pid, _tty| true),
            None
        );
    }

    #[test]
    fn direct_match_without_override_returns_adapter() {
        let _guard = env_guard();
        let root = unique_temp_dir("direct-match");
        let config_dir = root.join("yeetnyoink");
        fs::create_dir_all(&config_dir).expect("config dir should be created");
        fs::write(
            config_dir.join("config.toml"),
            r#"
[app.editor.emacs]
enabled = true
"#,
        )
        .expect("config file should be writable");
        let old_config = load_config(&config_dir.join("config.toml"));

        let chain = resolve_app_chain(emacs::APP_IDS[0], 0, "");
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].adapter_name(), emacs::ADAPTER_NAME);

        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn other_editor_profiles_do_not_enable_unconfigured_direct_adapter_by_default() {
        let _guard = env_guard();
        let root = unique_temp_dir("override-filter");
        let config_dir = root.join("yeetnyoink");
        fs::create_dir_all(&config_dir).expect("config dir should be created");
        fs::write(
            config_dir.join("config.toml"),
            r#"
[app.editor.vscode]
enabled = true
"#,
        )
        .expect("config file should be writable");

        let old_config = load_config(&config_dir.join("config.toml"));

        let chain = resolve_app_chain(emacs::APP_IDS[0], 0, "");
        assert!(chain.is_empty());

        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn explicit_direct_adapter_disable_still_applies() {
        let _guard = env_guard();
        let root = unique_temp_dir("direct-disable");
        let config_dir = root.join("yeetnyoink");
        fs::create_dir_all(&config_dir).expect("config dir should be created");
        fs::write(
            config_dir.join("config.toml"),
            r#"
[app.editor.emacs]
enabled = false
"#,
        )
        .expect("config file should be writable");

        let old_config = load_config(&config_dir.join("config.toml"));

        let chain = resolve_app_chain(emacs::APP_IDS[0], 0, "");
        assert!(chain.is_empty());

        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn editor_profile_does_not_disable_terminal_chain_selection() {
        let _guard = env_guard();
        let root = unique_temp_dir("override-terminal");
        let config_dir = root.join("yeetnyoink");
        fs::create_dir_all(&config_dir).expect("config dir should be created");
        fs::write(
            config_dir.join("config.toml"),
            r#"
[app.editor.neovim]
enabled = true
[app.editor.neovim.ui.terminal]
app = "wezterm"
"#,
        )
        .expect("config file should be writable");

        let old_config = load_config(&config_dir.join("config.toml"));

        let chain = resolve_app_chain(wezterm::APP_IDS[0], 0, "");
        assert!(!chain.is_empty());
        assert_eq!(
            chain
                .first()
                .and_then(|adapter| adapter.config_aliases())
                .map(|aliases| aliases[0]),
            Some(wezterm::ADAPTER_ALIASES[0])
        );

        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn kitty_terminal_app_id_resolves_terminal_chain() {
        let _guard = env_guard();
        let root = unique_temp_dir("kitty-terminal-chain");
        let config_dir = root.join("yeetnyoink");
        fs::create_dir_all(&config_dir).expect("config dir should be created");
        fs::write(
            config_dir.join("config.toml"),
            r#"
[app.terminal.kitty]
enabled = true
"#,
        )
        .expect("config file should be writable");
        let old_config = load_config(&config_dir.join("config.toml"));

        let chain = resolve_app_chain(kitty::APP_IDS[0], 0, "");
        assert!(!chain.is_empty());
        assert_eq!(
            chain.last().map(|adapter| adapter.adapter_name()),
            Some("terminal")
        );

        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn wezterm_macos_bundle_id_resolves_terminal_chain() {
        let _guard = env_guard();
        let root = unique_temp_dir("wezterm-terminal-chain");
        let config_dir = root.join("yeetnyoink");
        fs::create_dir_all(&config_dir).expect("config dir should be created");
        fs::write(
            config_dir.join("config.toml"),
            r#"
[app.terminal.wezterm]
enabled = true
"#,
        )
        .expect("config file should be writable");
        let old_config = load_config(&config_dir.join("config.toml"));

        let chain = resolve_app_chain("com.github.wez.wezterm", 0, "");
        assert!(!chain.is_empty());
        assert_eq!(
            chain.last().map(|adapter| adapter.adapter_name()),
            Some("terminal")
        );

        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn foot_terminal_app_id_resolves_terminal_chain() {
        let _guard = env_guard();
        let root = unique_temp_dir("foot-terminal-chain");
        let config_dir = root.join("yeetnyoink");
        fs::create_dir_all(&config_dir).expect("config dir should be created");
        fs::write(
            config_dir.join("config.toml"),
            r#"
[app.terminal.foot]
enabled = true
"#,
        )
        .expect("config file should be writable");
        let old_config = load_config(&config_dir.join("config.toml"));

        let chain = resolve_app_chain(foot::APP_IDS[0], 0, "");
        assert!(!chain.is_empty());
        assert_eq!(
            chain.last().map(|adapter| adapter.adapter_name()),
            Some("terminal")
        );

        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn alacritty_terminal_app_id_resolves_terminal_chain() {
        let _guard = env_guard();
        let root = unique_temp_dir("alacritty-terminal-chain");
        let config_dir = root.join("yeetnyoink");
        fs::create_dir_all(&config_dir).expect("config dir should be created");
        fs::write(
            config_dir.join("config.toml"),
            r#"
[app.terminal.alacritty]
enabled = true
"#,
        )
        .expect("config file should be writable");
        let old_config = load_config(&config_dir.join("config.toml"));

        let chain = resolve_app_chain(alacritty::APP_IDS[0], 0, "");
        assert!(!chain.is_empty());
        assert_eq!(
            chain.last().map(|adapter| adapter.adapter_name()),
            Some("terminal")
        );

        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn ghostty_terminal_app_id_resolves_terminal_chain() {
        let _guard = env_guard();
        let root = unique_temp_dir("ghostty-terminal-chain");
        let config_dir = root.join("yeetnyoink");
        fs::create_dir_all(&config_dir).expect("config dir should be created");
        fs::write(
            config_dir.join("config.toml"),
            r#"
[app.terminal.ghostty]
enabled = true
"#,
        )
        .expect("config file should be writable");
        let old_config = load_config(&config_dir.join("config.toml"));

        let chain = resolve_app_chain(ghostty::APP_IDS[0], 0, "");
        assert!(!chain.is_empty());
        assert_eq!(
            chain.last().map(|adapter| adapter.adapter_name()),
            Some("terminal")
        );

        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }
}
