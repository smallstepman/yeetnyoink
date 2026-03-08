use crate::adapters::apps::{
    self, alacritty, emacs, foot, ghostty, kitty, librefox::Librefox, nvim::Nvim, vscode::Vscode,
    wezterm, AppAdapter, AppKind,
};
use crate::adapters::terminal_multiplexers::tmux::Tmux;
use crate::config::AppSection;
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
    apps::bind_policy(Box::new(emacs::EmacsBackend))
}

fn build_librefox() -> Box<dyn AppAdapter> {
    apps::bind_policy(Box::new(Librefox))
}

fn build_vscode() -> Box<dyn AppAdapter> {
    apps::bind_policy(Box::new(Vscode))
}

fn build_wezterm_terminal() -> Box<dyn AppAdapter> {
    apps::bind_policy(Box::new(wezterm::WeztermBackend))
}

fn build_kitty_terminal() -> Box<dyn AppAdapter> {
    apps::bind_policy(Box::new(kitty::KittyBackend))
}

fn build_foot_terminal() -> Box<dyn AppAdapter> {
    apps::bind_policy(Box::new(foot::FootBackend))
}

fn build_alacritty_terminal() -> Box<dyn AppAdapter> {
    apps::bind_policy(Box::new(alacritty::AlacrittyBackend))
}

fn build_ghostty_terminal() -> Box<dyn AppAdapter> {
    apps::bind_policy(Box::new(ghostty::GhosttyBackend))
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

fn resolve_terminal_chain(terminal_pid: u32, host: &TerminalHostSpec) -> Vec<Box<dyn AppAdapter>> {
    let mut chain: Vec<Box<dyn AppAdapter>> = Vec::new();
    let mux_backend = crate::config::mux_policy_for(host.aliases).backend;

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

    let search_pid = if shells.len() <= 1 {
        shells.first().copied()
    } else if !fg_base.is_empty() {
        shells
            .iter()
            .copied()
            .find(|&shell_pid| !runtime::find_descendants_by_comm(shell_pid, &fg_base).is_empty())
            .or_else(|| {
                shells.iter().copied().find(|&shell_pid| {
                    let Ok(stat) = std::fs::read_to_string(format!("/proc/{shell_pid}/stat"))
                    else {
                        return false;
                    };
                    let Some(tpgid) = runtime::parse_stat_tpgid(&stat) else {
                        return false;
                    };
                    runtime::process_comm(tpgid)
                        .map(|comm| comm == fg_base)
                        .unwrap_or(false)
                })
            })
    } else {
        None
    };

    let Some(search_pid) = search_pid else {
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
                if let Some(nvim_pid) = tmux.nvim_in_current_pane() {
                    if let Some(nvim) = Nvim::for_pid(nvim_pid, mux_backend) {
                        chain.push(apps::bind_policy(Box::new(nvim)));
                    }
                }
                chain.push(apps::bind_policy(Box::new(tmux)));
                break 'tmux_fallback;
            }
        }
        chain.push((host.build)());
        return chain;
    };
    logging::debug(format!(
        "resolve_terminal_chain: selected shell pid={search_pid}"
    ));

    match fg_base.as_str() {
        "tmux" => {
            let (tmux_pids, found_tmux) =
                resolve_tmux_for_root(search_pid, host.terminal_launch_prefix);
            logging::debug(format!(
                "resolve_terminal_chain: tmux descendants under shell {} => {:?}",
                search_pid, tmux_pids
            ));
            if let Some(tmux) = found_tmux {
                if let Some(nvim_pid) = tmux.nvim_in_current_pane() {
                    if let Some(nvim) = Nvim::for_pid(nvim_pid, mux_backend) {
                        chain.push(apps::bind_policy(Box::new(nvim)));
                    }
                }
                chain.push(apps::bind_policy(Box::new(tmux)));
            }
        }
        "nvim" => {
            let nvim_pids = runtime::find_descendants_by_comm(search_pid, "nvim");
            logging::debug(format!(
                "resolve_terminal_chain: nvim descendants under shell {} => {:?}",
                search_pid, nvim_pids
            ));
            if let Some(&nvim_pid) = nvim_pids.first() {
                if let Some(nvim) = Nvim::for_pid(nvim_pid, mux_backend) {
                    chain.push(apps::bind_policy(Box::new(nvim)));
                }
            }
        }
        _ => {
            // fg_base is an arbitrary process running inside a mux (e.g. "node", "python").
            // Try tmux detection under the shell regardless.
            let (tmux_pids, found_tmux) =
                resolve_tmux_for_root(search_pid, host.terminal_launch_prefix);
            logging::debug(format!(
                "resolve_terminal_chain: fg={fg_base} tmux descendants under shell {search_pid} => {tmux_pids:?}"
            ));
            if let Some(tmux) = found_tmux {
                if let Some(nvim_pid) = tmux.nvim_in_current_pane() {
                    if let Some(nvim) = Nvim::for_pid(nvim_pid, mux_backend) {
                        chain.push(apps::bind_policy(Box::new(nvim)));
                    }
                }
                chain.push(apps::bind_policy(Box::new(tmux)));
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
        let preferred = preferred_adapter_name();

        if let Some(host) = TERMINAL_HOSTS
            .iter()
            .find(|host| host.app_ids.contains(&app_id))
        {
            if let Some(preferred) = preferred.as_deref() {
                if !matches_adapter_alias(preferred, host.aliases) {
                    logging::debug(format!(
                        "resolve_chain: adapter override '{}' disables terminal chain",
                        preferred
                    ));
                    return vec![];
                }
            }
            if !crate::config::app_integration_enabled(AppSection::Terminal, host.aliases) {
                logging::debug("resolve_chain: terminal integration disabled via config");
                return vec![];
            }
            let chain = resolve_terminal_chain(pid, host);
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

    fn default_domain_adapters(&self) -> Vec<Box<dyn AppAdapter>> {
        let terminal_adapter = match preferred_adapter_name().as_deref() {
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
            apps::bind_policy(Box::new(emacs::EmacsBackend)),
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
    use std::ffi::OsString;
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

    fn set_env(key: &str, value: Option<&str>) -> Option<OsString> {
        let old = std::env::var_os(key);
        if let Some(value) = value {
            std::env::set_var(key, value);
        } else {
            std::env::remove_var(key);
        }
        old
    }

    fn restore_env(key: &str, old: Option<OsString>) {
        if let Some(value) = old {
            std::env::set_var(key, value);
        } else {
            std::env::remove_var(key);
        }
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
        let old_override = set_env(
            "NIRI_DEEP_CONFIG",
            Some(config_dir.join("config.toml").to_str().expect("utf-8 path")),
        );
        crate::config::prepare().expect("config should load");

        let adapters = runtime_chain_resolver().default_domain_adapters();
        assert_eq!(
            adapters
                .first()
                .and_then(|adapter| adapter.config_aliases())
                .map(|aliases| aliases[0]),
            Some(foot::ADAPTER_ALIASES[0])
        );

        restore_env("NIRI_DEEP_CONFIG", old_override);
        crate::config::prepare().expect("config should reload");
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
        let old_override = set_env(
            "NIRI_DEEP_CONFIG",
            Some(config_dir.join("config.toml").to_str().expect("utf-8 path")),
        );
        crate::config::prepare().expect("config should load");

        let adapters = runtime_chain_resolver().default_domain_adapters();
        assert_eq!(
            adapters
                .first()
                .and_then(|adapter| adapter.config_aliases())
                .map(|aliases| aliases[0]),
            Some(alacritty::ADAPTER_ALIASES[0])
        );

        restore_env("NIRI_DEEP_CONFIG", old_override);
        crate::config::prepare().expect("config should reload");
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
        let old_override = set_env(
            "NIRI_DEEP_CONFIG",
            Some(config_dir.join("config.toml").to_str().expect("utf-8 path")),
        );
        crate::config::prepare().expect("config should load");

        let adapters = runtime_chain_resolver().default_domain_adapters();
        assert_eq!(
            adapters
                .first()
                .and_then(|adapter| adapter.config_aliases())
                .map(|aliases| aliases[0]),
            Some(ghostty::ADAPTER_ALIASES[0])
        );

        restore_env("NIRI_DEEP_CONFIG", old_override);
        crate::config::prepare().expect("config should reload");
        let _ = fs::remove_dir_all(root);
    }
}
