use crate::config::AppSection;
pub use crate::engine::contract::{
    unsupported_operation, AdapterCapabilities, AppAdapter, AppCapabilities, AppKind,
    MergeExecutionMode, MergePreparation, MoveDecision, TearResult, TopologyHandler,
    TopologySnapshot,
};

macro_rules! delegate_topology_to_mux_provider {
    ($ty:ty, $launch_prefix:expr) => {
        impl crate::engine::contract::TopologyHandler for $ty {
            fn can_focus(
                &self,
                dir: crate::engine::topology::Direction,
                pid: u32,
            ) -> anyhow::Result<bool> {
                Self::mux_provider().can_focus(dir, pid)
            }

            fn move_decision(
                &self,
                dir: crate::engine::topology::Direction,
                pid: u32,
            ) -> anyhow::Result<crate::engine::contract::MoveDecision> {
                Self::mux_provider().move_decision(dir, pid)
            }

            fn can_resize(
                &self,
                dir: crate::engine::topology::Direction,
                grow: bool,
                pid: u32,
            ) -> anyhow::Result<bool> {
                Self::mux_provider().can_resize(dir, grow, pid)
            }

            fn focus(
                &self,
                dir: crate::engine::topology::Direction,
                pid: u32,
            ) -> anyhow::Result<()> {
                Self::mux_provider().focus(dir, pid)
            }

            fn move_internal(
                &self,
                dir: crate::engine::topology::Direction,
                pid: u32,
            ) -> anyhow::Result<()> {
                Self::mux_provider().move_internal(dir, pid)
            }

            fn resize_internal(
                &self,
                dir: crate::engine::topology::Direction,
                grow: bool,
                step: i32,
                pid: u32,
            ) -> anyhow::Result<()> {
                Self::mux_provider().resize_internal(dir, grow, step, pid)
            }

            fn rearrange(
                &self,
                dir: crate::engine::topology::Direction,
                pid: u32,
            ) -> anyhow::Result<()> {
                Self::mux_provider().rearrange(dir, pid)
            }

            fn move_out(
                &self,
                dir: crate::engine::topology::Direction,
                pid: u32,
            ) -> anyhow::Result<crate::engine::contract::TearResult> {
                Ok(
                    crate::adapters::terminal_multiplexers::prepend_terminal_launch_prefix(
                        $launch_prefix,
                        Self::mux_provider().move_out(dir, pid)?,
                    ),
                )
            }

            fn merge_execution_mode(&self) -> crate::engine::contract::MergeExecutionMode {
                Self::mux_provider().merge_execution_mode()
            }

            fn prepare_merge(
                &self,
                source_pid: Option<crate::engine::runtime::ProcessId>,
            ) -> anyhow::Result<crate::engine::contract::MergePreparation> {
                Self::mux_provider().prepare_merge(source_pid)
            }

            fn augment_merge_preparation_for_target(
                &self,
                preparation: crate::engine::contract::MergePreparation,
                target_window_id: Option<u64>,
            ) -> crate::engine::contract::MergePreparation {
                Self::mux_provider()
                    .augment_merge_preparation_for_target(preparation, target_window_id)
            }

            fn merge_into_target(
                &self,
                dir: crate::engine::topology::Direction,
                source_pid: Option<crate::engine::runtime::ProcessId>,
                target_pid: Option<crate::engine::runtime::ProcessId>,
                preparation: crate::engine::contract::MergePreparation,
            ) -> anyhow::Result<()> {
                Self::mux_provider().merge_into_target(dir, source_pid, target_pid, preparation)
            }
        }
    };
}

pub(crate) use delegate_topology_to_mux_provider;

macro_rules! impl_terminal_host_backend {
    ($ty:ty, $launch_prefix:expr) => {
        impl $ty {
            pub(crate) fn mux_provider(
            ) -> &'static dyn crate::engine::contract::TerminalMultiplexerProvider {
                crate::adapters::terminal_multiplexers::active_mux_provider(ADAPTER_ALIASES)
            }

            pub fn spawn_attach_command(target: String) -> Option<Vec<String>> {
                crate::adapters::terminal_multiplexers::spawn_attach_command(
                    ADAPTER_ALIASES,
                    $launch_prefix,
                    target,
                )
            }
        }

        impl crate::adapters::apps::AppAdapter for $ty {
            fn adapter_name(&self) -> &'static str {
                ADAPTER_NAME
            }

            fn config_aliases(&self) -> Option<&'static [&'static str]> {
                Some(ADAPTER_ALIASES)
            }

            fn kind(&self) -> crate::engine::contract::AppKind {
                crate::engine::contract::AppKind::Terminal
            }

            fn capabilities(&self) -> crate::engine::contract::AdapterCapabilities {
                Self::mux_provider().capabilities()
            }
        }

        crate::adapters::apps::delegate_topology_to_mux_provider!($ty, $launch_prefix);
    };
}

pub(crate) use impl_terminal_host_backend;

pub mod alacritty;
pub mod chromium;
pub mod emacs;
pub mod foot;
pub mod ghostty;
pub mod kitty;
pub mod librewolf;
pub mod nvim;
pub mod vscode;
pub mod wezterm;

pub(crate) use crate::engine::resolution::catalog::{DirectAdapterSpec, TerminalHostSpec};

pub(crate) fn build_emacs() -> Box<dyn AppAdapter> {
    Box::new(emacs::EmacsBackend)
}

pub(crate) fn build_librewolf() -> Box<dyn AppAdapter> {
    Box::new(librewolf::Librewolf)
}

pub(crate) fn build_chromium() -> Box<dyn AppAdapter> {
    Box::new(chromium::Chromium)
}

pub(crate) fn build_vscode() -> Box<dyn AppAdapter> {
    Box::new(vscode::Vscode)
}

pub(crate) fn build_wezterm_terminal() -> Box<dyn AppAdapter> {
    Box::new(wezterm::WeztermBackend)
}

pub(crate) fn build_kitty_terminal() -> Box<dyn AppAdapter> {
    Box::new(kitty::KittyBackend)
}

pub(crate) fn build_foot_terminal() -> Box<dyn AppAdapter> {
    Box::new(foot::FootBackend)
}

pub(crate) fn build_alacritty_terminal() -> Box<dyn AppAdapter> {
    Box::new(alacritty::AlacrittyBackend)
}

pub(crate) fn build_ghostty_terminal() -> Box<dyn AppAdapter> {
    Box::new(ghostty::GhosttyBackend)
}

pub(crate) const TERMINAL_HOSTS: &[TerminalHostSpec] = &[
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

pub(crate) const DIRECT_ADAPTERS: &[DirectAdapterSpec] = &[
    DirectAdapterSpec {
        name: emacs::ADAPTER_NAME,
        aliases: emacs::ADAPTER_ALIASES,
        app_ids: emacs::APP_IDS,
        section: AppSection::Editor,
        build: build_emacs,
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

/// Developer note for adding a new adapter:
/// 1. Implement `AppAdapter` and declare all booleans in `capabilities`.
/// 2. Keep unsupported operations disabled in `capabilities` so the orchestrator
///    classify them as `Unsupported` without runtime probes.
/// 3. Add adapter tests that cover focus/move/resize behavior and precedence.

#[cfg(test)]
mod resolve_chain_tests {
    use crate::adapters::apps::{
        alacritty, chromium::Chromium, emacs, foot, ghostty, kitty, librewolf::Librewolf,
        nvim::Nvim, vscode::Vscode, wezterm, TopologyHandler,
    };
    use crate::adapters::terminal_multiplexers::tmux::Tmux;

    #[test]
    fn adapters_implement_topology_traits() {
        fn assert_topology_contracts<T: TopologyHandler>() {}
        assert_topology_contracts::<alacritty::AlacrittyBackend>();
        assert_topology_contracts::<emacs::EmacsBackend>();
        assert_topology_contracts::<foot::FootBackend>();
        assert_topology_contracts::<ghostty::GhosttyBackend>();
        assert_topology_contracts::<kitty::KittyBackend>();
        assert_topology_contracts::<wezterm::WeztermBackend>();
        assert_topology_contracts::<Tmux>();
        assert_topology_contracts::<Nvim>();
        assert_topology_contracts::<Chromium>();
        assert_topology_contracts::<Librewolf>();
        assert_topology_contracts::<Vscode>();
    }
}
