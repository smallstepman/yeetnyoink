use crate::config::AppSection;
use crate::engine::contracts::AppAdapter;

pub(crate) struct DirectAdapterSpec {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub app_ids: &'static [&'static str],
    pub section: AppSection,
    pub build: fn() -> Box<dyn AppAdapter>,
}

pub(crate) struct TerminalHostSpec {
    pub aliases: &'static [&'static str],
    pub app_ids: &'static [&'static str],
    pub terminal_launch_prefix: &'static [&'static str],
    pub build: fn() -> Box<dyn AppAdapter>,
}
