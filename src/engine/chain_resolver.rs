use crate::adapters::apps::{
    alacritty,
    chromium::{self, Chromium},
    emacs, foot, ghostty, kitty,
    librewolf::{self, Librewolf},
    nvim::{self, Nvim},
    vscode::Vscode,
    wezterm, AppAdapter, AppKind,
};
use crate::adapters::terminal_multiplexers::tmux::Tmux;
use crate::config::{AppSection, TerminalMuxBackend};
use crate::engine::contract::ChainResolver;
use crate::engine::domain::{EDITOR_DOMAIN_ID, TERMINAL_DOMAIN_ID, WM_DOMAIN_ID};
use crate::engine::runtime::{self, ProcessId};
use crate::engine::topology::DomainId;
use crate::logging;

pub struct RuntimeChainResolver;

static RUNTIME_CHAIN_RESOLVER: RuntimeChainResolver = RuntimeChainResolver;

pub fn runtime_chain_resolver() -> &'static RuntimeChainResolver {
    &RUNTIME_CHAIN_RESOLVER
}

struct DirectAdapterSpec {
    name: &'static str,
    aliases: &'static [&'static str],
    app_ids: &'static [&'static str],
    section: AppSection,
    build: fn() -> Box<dyn AppAdapter>,
}

fn build_editor() -> Box<dyn AppAdapter> {
    crate::engine::app_policy::bind_app_policy(Box::new(emacs::EmacsBackend))
}

fn build_librewolf() -> Box<dyn AppAdapter> {
    crate::engine::app_policy::bind_app_policy(Box::new(Librewolf))
}

fn build_chromium() -> Box<dyn AppAdapter> {
    crate::engine::app_policy::bind_app_policy(Box::new(Chromium))
}

fn build_vscode() -> Box<dyn AppAdapter> {
    crate::engine::app_policy::bind_app_policy(Box::new(Vscode))
}

fn build_wezterm_terminal() -> Box<dyn AppAdapter> {
    crate::engine::app_policy::bind_app_policy(Box::new(wezterm::WeztermBackend))
}

fn build_kitty_terminal() -> Box<dyn AppAdapter> {
    crate::engine::app_policy::bind_app_policy(Box::new(kitty::KittyBackend))
}

fn build_foot_terminal() -> Box<dyn AppAdapter> {
    crate::engine::app_policy::bind_app_policy(Box::new(foot::FootBackend))
}

fn build_alacritty_terminal() -> Box<dyn AppAdapter> {
    crate::engine::app_policy::bind_app_policy(Box::new(alacritty::AlacrittyBackend))
}

fn build_ghostty_terminal() -> Box<dyn AppAdapter> {
    crate::engine::app_policy::bind_app_policy(Box::new(ghostty::GhosttyBackend))
}

struct TerminalHostSpec {
    aliases: &'static [&'static str],
    app_ids: &'static [&'static str],
    terminal_launch_prefix: &'static [&'static str],
    build: fn() -> Box<dyn AppAdapter>,
}

const TERMINAL_HOSTS: &[TerminalHostSpec] = &[
    TerminalHostSpec {
        aliases: wezterm::ADAPTER_ALIASES,
        app_ids: wezterm::APP_IDS,
        terminal_launch_prefix: wezterm::TERMINAL_LAUNCH_PREFIX,
        build: build_wezterm_terminal,
    },
    TerminalHostSpec {
        aliases: kitty::ADAPTER_ALIASES,
        app_ids: kitty::APP_IDS,
        terminal_launch_prefix: kitty::TERMINAL_LAUNCH_PREFIX,
        build: build_kitty_terminal,
    },
    TerminalHostSpec {
        aliases: foot::ADAPTER_ALIASES,
        app_ids: foot::APP_IDS,
        terminal_launch_prefix: foot::TERMINAL_LAUNCH_PREFIX,
        build: build_foot_terminal,
    },
    TerminalHostSpec {
        aliases: alacritty::ADAPTER_ALIASES,
        app_ids: alacritty::APP_IDS,
        terminal_launch_prefix: alacritty::TERMINAL_LAUNCH_PREFIX,
        build: build_alacritty_terminal,
    },
    TerminalHostSpec {
        aliases: ghostty::ADAPTER_ALIASES,
        app_ids: ghostty::APP_IDS,
        terminal_launch_prefix: ghostty::TERMINAL_LAUNCH_PREFIX,
        build: build_ghostty_terminal,
    },
];

const DIRECT_ADAPTERS: &[DirectAdapterSpec] = &[
    DirectAdapterSpec {
        name: emacs::ADAPTER_NAME,
        aliases: emacs::ADAPTER_ALIASES,
        app_ids: emacs::APP_IDS,
        section: AppSection::Editor,
        build: build_editor,
    },
    DirectAdapterSpec {
        name: librewolf::ADAPTER_NAME,
        aliases: librewolf::ADAPTER_ALIASES,
        app_ids: librewolf::APP_IDS,
        section: AppSection::Browser,
        build: build_librewolf,
    },
    DirectAdapterSpec {
        name: chromium::ADAPTER_NAME,
        aliases: chromium::ADAPTER_ALIASES,
        app_ids: chromium::APP_IDS,
        section: AppSection::Browser,
        build: build_chromium,
    },
    DirectAdapterSpec {
        name: "vscode",
        aliases: &["vscode"],
        app_ids: &["code", "code-url-handler", "Code", "code-oss"],
        section: AppSection::Editor,
        build: build_vscode,
    },
];

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
    for spec in DIRECT_ADAPTERS {
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

        return Some((spec.build)());
    }

    None
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
    host: &TerminalHostSpec,
    shells: &[u32],
) -> Option<u32> {
    let panes = crate::adapters::terminal_multiplexers::active_mux_provider(host.aliases)
        .list_panes_for_pid(terminal_pid)
        .ok()?;
    let focused_tty = crate::engine::contract::TerminalPaneSnapshot::active_or_first(panes.iter())
        .and_then(|pane| pane.tty_name.as_deref());
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
        chain.push(crate::engine::app_policy::bind_app_policy(Box::new(nvim)));
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

fn tmux_nvim_pid_for_roots(roots: &[u32], host: &TerminalHostSpec) -> Option<u32> {
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

fn resolve_terminal_chain(terminal_pid: u32, host: &TerminalHostSpec) -> Vec<Box<dyn AppAdapter>> {
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

    let fg_hint = crate::adapters::terminal_multiplexers::active_foreground_process(
        host.aliases,
        terminal_pid,
    );
    let fg_base = fg_hint
        .as_deref()
        .map(runtime::normalize_process_name)
        .unwrap_or_default();
    logging::debug(format!(
        "resolve_terminal_chain: pid={} fg_hint={:?} fg_base={}",
        terminal_pid, fg_hint, fg_base
    ));

    let shells: Vec<u32> = runtime::child_pids(terminal_pid)
        .into_iter()
        .filter(|&pid| runtime::is_shell_pid(pid))
        .collect();
    logging::debug(format!(
        "resolve_terminal_chain: shell_candidates={:?}",
        shells
    ));

    let search_pid =
        if let Some(shell_pid) = shell_pid_for_host_focused_tty(terminal_pid, host, &shells) {
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
                    chain.push(crate::engine::app_policy::bind_app_policy(Box::new(tmux)));
                    break 'tmux_fallback;
                }
            }
        } else {
            logging::debug(
                "resolve_terminal_chain: no focused shell match and tmux resolution is disabled",
            );
        }
        chain.push((host.build)());
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
                chain.push(crate::engine::app_policy::bind_app_policy(Box::new(tmux)));
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
                    chain.push(crate::engine::app_policy::bind_app_policy(Box::new(tmux)));
                }
            }
        }
    }

    chain.push((host.build)());
    logging::debug(format!(
        "resolve_terminal_chain: final depth={}",
        chain.len()
    ));

    chain
}

fn domain_id_for_app_kind(kind: AppKind) -> DomainId {
    match kind {
        AppKind::Terminal => TERMINAL_DOMAIN_ID,
        AppKind::Editor => EDITOR_DOMAIN_ID,
        AppKind::Browser => WM_DOMAIN_ID,
    }
}

impl ChainResolver for RuntimeChainResolver {
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

        if let Some(host) = TERMINAL_HOSTS
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
        let terminal_adapter = match preferred_terminal_adapter_name().as_deref() {
            Some(preferred) if matches_adapter_alias(preferred, alacritty::ADAPTER_ALIASES) => {
                build_alacritty_terminal()
            }
            Some(preferred) if matches_adapter_alias(preferred, foot::ADAPTER_ALIASES) => {
                build_foot_terminal()
            }
            Some(preferred) if matches_adapter_alias(preferred, ghostty::ADAPTER_ALIASES) => {
                build_ghostty_terminal()
            }
            Some(preferred) if matches_adapter_alias(preferred, kitty::ADAPTER_ALIASES) => {
                build_kitty_terminal()
            }
            _ => build_wezterm_terminal(),
        };
        vec![
            terminal_adapter,
            crate::engine::app_policy::bind_app_policy(Box::new(emacs::EmacsBackend)),
        ]
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
            return domain_id_for_app_kind(kind);
        }
        WM_DOMAIN_ID
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{runtime_chain_resolver, ChainResolver};
    use crate::adapters::apps::{alacritty, foot, ghostty};

    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::utils::env_guard()
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "yeet-and-yoink-chain-resolver-{prefix}-{}-{id}",
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
        let config_dir = root.join("yeet-and-yoink");
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

        let adapters = runtime_chain_resolver().default_domain_adapters();
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
        let config_dir = root.join("yeet-and-yoink");
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

        let adapters = runtime_chain_resolver().default_domain_adapters();
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
        let config_dir = root.join("yeet-and-yoink");
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

        let adapters = runtime_chain_resolver().default_domain_adapters();
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
        let config_dir = root.join("yeet-and-yoink");
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

        let chain = runtime_chain_resolver().resolve_chain("librewolf", 0, "LibreWolf");
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].adapter_name(), "librewolf");

        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn brave_browser_profile_enables_chromium_direct_adapter() {
        let _guard = env_guard();
        let root = unique_temp_dir("brave-direct-adapter");
        let config_dir = root.join("yeet-and-yoink");
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

        let chain = runtime_chain_resolver().resolve_chain("brave-browser", 0, "Brave Browser");
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
}
