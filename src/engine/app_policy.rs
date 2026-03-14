use crate::adapters::apps::{
    unsupported_operation, AppAdapter, AppKind, MergeExecutionMode, MergePreparation,
    MoveDecision, TearResult, TopologyHandler,
};
use crate::config::AppSection;

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

impl AppAdapter for PolicyBoundApp {
    fn adapter_name(&self) -> &'static str {
        self.inner.adapter_name()
    }

    fn config_aliases(&self) -> Option<&'static [&'static str]> {
        self.inner.config_aliases()
    }

    fn kind(&self) -> AppKind {
        self.inner.kind()
    }

    fn capabilities(&self) -> crate::engine::contract::AdapterCapabilities {
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

    fn eval(
        &self,
        expression: &str,
        pid: Option<crate::engine::runtime::ProcessId>,
    ) -> anyhow::Result<String> {
        self.inner.eval(expression, pid)
    }
}

impl TopologyHandler for PolicyBoundApp {
    fn can_focus(&self, dir: crate::engine::topology::Direction, pid: u32) -> anyhow::Result<bool> {
        if let Some(policy) = self.pane_policy() {
            if !policy.focus_allowed(dir) {
                return Ok(false);
            }
        }
        TopologyHandler::can_focus(self.inner.as_ref(), dir, pid)
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
            let decision = TopologyHandler::move_decision(self.inner.as_ref(), dir, pid)?;
            if matches!(decision, MoveDecision::TearOut) && !policy.tear_out_capability() {
                return Ok(MoveDecision::Passthrough);
            }
            return Ok(decision);
        }
        TopologyHandler::move_decision(self.inner.as_ref(), dir, pid)
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
        TopologyHandler::can_resize(self.inner.as_ref(), dir, grow, pid)
    }

    fn at_side(&self, dir: crate::engine::topology::Direction, pid: u32) -> anyhow::Result<bool> {
        TopologyHandler::at_side(self.inner.as_ref(), dir, pid)
    }

    fn window_count(&self, pid: u32) -> anyhow::Result<u32> {
        TopologyHandler::window_count(self.inner.as_ref(), pid)
    }

    fn focus(&self, dir: crate::engine::topology::Direction, pid: u32) -> anyhow::Result<()> {
        if let Some(policy) = self.pane_policy() {
            if !policy.focus_allowed(dir) {
                return Err(unsupported_operation(self.adapter_name(), "focus"));
            }
        }
        TopologyHandler::focus(self.inner.as_ref(), dir, pid)
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
        TopologyHandler::move_internal(self.inner.as_ref(), dir, pid)
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
        TopologyHandler::resize_internal(self.inner.as_ref(), dir, grow, step, pid)
    }

    fn rearrange(&self, dir: crate::engine::topology::Direction, pid: u32) -> anyhow::Result<()> {
        if let Some(policy) = self.pane_policy() {
            if !policy.move_allowed(dir) {
                return Err(unsupported_operation(self.adapter_name(), "rearrange"));
            }
        }
        TopologyHandler::rearrange(self.inner.as_ref(), dir, pid)
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
        TopologyHandler::move_out(self.inner.as_ref(), dir, pid)
    }

    fn merge_into(
        &self,
        dir: crate::engine::topology::Direction,
        source_pid: u32,
    ) -> anyhow::Result<()> {
        TopologyHandler::merge_into(self.inner.as_ref(), dir, source_pid)
    }

    fn merge_execution_mode(&self) -> MergeExecutionMode {
        TopologyHandler::merge_execution_mode(self.inner.as_ref())
    }

    fn prepare_merge(
        &self,
        source_pid: Option<crate::engine::runtime::ProcessId>,
    ) -> anyhow::Result<MergePreparation> {
        TopologyHandler::prepare_merge(self.inner.as_ref(), source_pid)
    }

    fn augment_merge_preparation_for_target(
        &self,
        preparation: MergePreparation,
        target_window_id: Option<u64>,
    ) -> MergePreparation {
        TopologyHandler::augment_merge_preparation_for_target(
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
        TopologyHandler::merge_into_target(
            self.inner.as_ref(),
            dir,
            source_pid,
            target_pid,
            preparation,
        )
    }
}

pub(crate) fn bind_app_policy(app: Box<dyn AppAdapter>) -> Box<dyn AppAdapter> {
    Box::new(PolicyBoundApp::new(app))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use crate::adapters::apps::{emacs, wezterm};

    use super::bind_app_policy;

    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::utils::env_guard()
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "yeet-and-yoink-engine-policy-{prefix}-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("temp dir should be created");
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
    fn resolved_editor_capabilities_follow_config_policy() {
        let _guard = env_guard();
        let root = unique_temp_dir("editor");
        let config_dir = root.join("yeet-and-yoink");
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

        let old_config = load_config(&config_dir.join("config.toml"));

        let wrapped = bind_app_policy(Box::new(emacs::EmacsBackend));
        assert!(!wrapped.capabilities().focus);

        restore_config(old_config);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn resolved_terminal_capabilities_follow_config_policy() {
        let _guard = env_guard();
        let root = unique_temp_dir("terminal");
        let config_dir = root.join("yeet-and-yoink");
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

        let old_config = load_config(&config_dir.join("config.toml"));

        let wrapped = bind_app_policy(Box::new(wezterm::WeztermBackend));
        assert!(!wrapped.capabilities().resize_internal);

        restore_config(old_config);
        let _ = std::fs::remove_dir_all(root);
    }
}
