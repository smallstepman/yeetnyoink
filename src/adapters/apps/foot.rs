/// Foot has no built-in pane control API, so all terminal semantics are delegated
/// to the configured external multiplexer (eg. tmux or zellij).
pub struct FootBackend;
pub const ADAPTER_NAME: &str = "terminal";
pub const ADAPTER_ALIASES: &[&str] = &["foot", "terminal"];
pub const APP_IDS: &[&str] = &["foot", "footclient"];
pub const TERMINAL_LAUNCH_PREFIX: &[&str] = &["foot"];

crate::adapters::apps::impl_terminal_host_backend!(FootBackend, TERMINAL_LAUNCH_PREFIX);

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::FootBackend;
    use crate::engine::contracts::AppAdapter;

    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::utils::env_guard()
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "yeetnyoink-foot-config-{prefix}-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("temp dir should be created");
        path
    }

    fn load_config(path: &Path) -> crate::config::Config {
        let old = crate::config::snapshot();
        crate::config::prepare_with_path(Some(path)).expect("config should load");
        old
    }

    fn restore_config(old: crate::config::Config) {
        crate::config::install(old);
    }

    #[test]
    fn default_capabilities_follow_tmux_mux_backend() {
        let _guard = env_guard();
        let root = unique_temp_dir("default-tmux");
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

        let app = FootBackend;
        let caps = AppAdapter::capabilities(&app);
        assert!(caps.probe);
        assert!(caps.focus);
        assert!(caps.move_internal);
        assert!(!caps.resize_internal);
        assert!(!caps.rearrange);
        assert!(caps.tear_out);
        assert!(caps.merge);

        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn advertises_config_aliases_for_policy_binding() {
        let app = FootBackend;
        assert_eq!(app.config_aliases(), Some(super::ADAPTER_ALIASES));
    }

    #[test]
    fn zellij_backend_selects_attach_command_with_foot_prefix() {
        let _guard = env_guard();
        let root = unique_temp_dir("zellij-attach");
        let config_dir = root.join("yeetnyoink");
        fs::create_dir_all(&config_dir).expect("config dir should be created");
        fs::write(
            config_dir.join("config.toml"),
            r#"
[app.terminal.foot]
enabled = true
mux_backend = "zellij"
"#,
        )
        .expect("config file should be writable");
        let old_config = load_config(&config_dir.join("config.toml"));

        let command = FootBackend::spawn_attach_command("dev".to_string());
        assert_eq!(
            command,
            Some(vec![
                "foot".to_string(),
                "zellij".to_string(),
                "attach".to_string(),
                "dev".to_string(),
            ])
        );

        restore_config(old_config);
        let _ = fs::remove_dir_all(root);
    }
}
