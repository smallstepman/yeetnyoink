use std::any::TypeId;
use std::collections::HashSet;
use std::sync::OnceLock;

use anyhow::{anyhow, Context, Result};

use crate::engine::contracts::{
    AppAdapter, MergePreparation, TopologyHandler as AppTopologyHandler,
};
use crate::engine::resolution::domain::domain_id_for_app_kind;
use crate::engine::runtime::ProcessId;
use crate::engine::topology::{Direction, DomainId, Rect};
use crate::engine::wm::ConfiguredWindowManager;

use super::pipeline::{
    DomainLeafSnapshot, DomainSnapshot, ErasedDomain, TilingDomain, TopologyModifierImpl,
    TopologyProvider,
};
use super::registry::{PaneState, WM_DOMAIN_ID};

#[cfg(test)]
use super::registry::TERMINAL_DOMAIN_ID;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeWindowRef {
    pub window_id: u64,
    pub pid: Option<ProcessId>,
}

pub fn encode_native_window_ref(window_id: u64, pid: Option<ProcessId>) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(12);
    encoded.extend_from_slice(&window_id.to_le_bytes());
    encoded.extend_from_slice(&pid.map(ProcessId::get).unwrap_or(0).to_le_bytes());
    encoded
}

pub fn decode_native_window_ref(bytes: &[u8]) -> Option<NativeWindowRef> {
    if bytes.len() != 12 {
        return None;
    }
    let mut window = [0_u8; 8];
    window.copy_from_slice(&bytes[..8]);
    let mut pid = [0_u8; 4];
    pid.copy_from_slice(&bytes[8..12]);
    Some(NativeWindowRef {
        window_id: u64::from_le_bytes(window),
        pid: ProcessId::new(u32::from_le_bytes(pid)),
    })
}

struct AppMergePayload {
    source_pid: Option<ProcessId>,
    preparation: MergePreparation,
}

fn app_merge_payload_types() -> &'static [TypeId] {
    static TYPES: OnceLock<Vec<TypeId>> = OnceLock::new();
    TYPES
        .get_or_init(|| vec![TypeId::of::<AppMergePayload>()])
        .as_slice()
}

#[derive(Debug)]
struct UnsupportedDomainPlugin {
    domain_id: DomainId,
    name: &'static str,
}

impl UnsupportedDomainPlugin {
    fn new(domain_id: DomainId, name: &'static str) -> Self {
        Self { domain_id, name }
    }
}

impl ErasedDomain for UnsupportedDomainPlugin {
    fn domain_id(&self) -> DomainId {
        self.domain_id
    }

    fn domain_name(&self) -> &'static str {
        self.name
    }

    fn rect(&self) -> Rect {
        Rect {
            x: 0,
            y: 0,
            w: 10000,
            h: 10000,
        }
    }

    fn fetch_snapshot(&mut self) -> Result<DomainSnapshot> {
        Ok(DomainSnapshot {
            domain_id: self.domain_id,
            rect: self.rect(),
            leaves: vec![],
        })
    }

    fn supported_payload_types(&self) -> Vec<TypeId> {
        vec![]
    }

    fn tear_off(&mut self, _native_id: &[u8]) -> Result<Box<dyn PaneState>> {
        Err(anyhow!(
            "domain '{}' does not support tear-off payload transfer",
            self.name
        ))
    }

    fn merge_in(
        &mut self,
        _target_native_id: &[u8],
        _dir: Direction,
        _payload: Box<dyn PaneState>,
    ) -> Result<Vec<u8>> {
        Err(anyhow!(
            "domain '{}' does not support merge-in payload transfer",
            self.name
        ))
    }
}

pub struct AppDomainPlugin {
    domain_id: DomainId,
    adapter: Box<dyn AppAdapter>,
}

impl AppDomainPlugin {
    pub fn new(domain_id: DomainId, adapter: Box<dyn AppAdapter>) -> Self {
        Self { domain_id, adapter }
    }

    fn pid_from_native(native_id: &[u8]) -> Option<ProcessId> {
        decode_native_window_ref(native_id).and_then(|window| window.pid)
    }

    fn single_leaf_snapshot(&self) -> DomainSnapshot {
        DomainSnapshot {
            domain_id: self.domain_id,
            rect: TopologyProvider::rect(self),
            leaves: vec![DomainLeafSnapshot {
                id: 1,
                native_id: encode_native_window_ref(1, None),
                rect: TopologyProvider::rect(self),
                focused: true,
            }],
        }
    }
}

impl TopologyProvider for AppDomainPlugin {
    type NativeId = Vec<u8>;
    type Error = anyhow::Error;

    fn domain_name(&self) -> &'static str {
        self.adapter.adapter_name()
    }

    fn rect(&self) -> Rect {
        Rect {
            x: 0,
            y: 0,
            w: 10000,
            h: 10000,
        }
    }

    fn fetch_layout(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl TopologyModifierImpl for AppDomainPlugin {
    fn focus_impl(&mut self, _id: &Self::NativeId) -> Result<(), Self::Error> {
        Err(anyhow!(
            "adapter '{}' cannot focus by native pane id in legacy bridge mode",
            self.adapter.adapter_name()
        ))
    }

    fn move_impl(&mut self, native_id: &Self::NativeId, dir: Direction) -> Result<(), Self::Error> {
        let pid = Self::pid_from_native(native_id)
            .map(ProcessId::get)
            .context("move requires source pid in native id")?;
        AppTopologyHandler::move_internal(self.adapter.as_ref(), dir, pid)
            .with_context(|| format!("{} move_internal failed", self.adapter.adapter_name()))
    }

    fn tear_off_impl(
        &mut self,
        native_id: &Self::NativeId,
    ) -> Result<Box<dyn PaneState>, Self::Error> {
        let source_pid = Self::pid_from_native(native_id);
        let preparation = AppTopologyHandler::prepare_merge(self.adapter.as_ref(), source_pid)
            .with_context(|| format!("{} prepare_merge failed", self.adapter.adapter_name()))?;
        Ok(Box::new(AppMergePayload {
            source_pid,
            preparation,
        }))
    }

    fn merge_in_impl(
        &mut self,
        target_native_id: &Self::NativeId,
        dir: Direction,
        payload: Box<dyn PaneState>,
    ) -> Result<Self::NativeId, Self::Error> {
        let target_window_id =
            decode_native_window_ref(target_native_id).map(|window| window.window_id);
        let target_pid = Self::pid_from_native(target_native_id);
        let payload_any = payload.into_any();
        let merge_payload = payload_any
            .downcast::<AppMergePayload>()
            .map_err(|_| anyhow!("unsupported payload for '{}'", self.adapter.adapter_name()))?;
        let preparation = AppTopologyHandler::augment_merge_preparation_for_target(
            self.adapter.as_ref(),
            merge_payload.preparation,
            target_window_id,
        );
        AppTopologyHandler::merge_into_target(
            self.adapter.as_ref(),
            dir,
            merge_payload.source_pid,
            target_pid,
            preparation,
        )
        .with_context(|| format!("{} merge_into_target failed", self.adapter.adapter_name()))?;
        Ok(target_native_id.clone())
    }
}

impl TilingDomain for AppDomainPlugin {
    fn supported_payload_types(&self) -> &'static [TypeId] {
        if self.adapter.capabilities().merge {
            app_merge_payload_types()
        } else {
            &[]
        }
    }
}

impl ErasedDomain for AppDomainPlugin {
    fn domain_id(&self) -> DomainId {
        self.domain_id
    }

    fn domain_name(&self) -> &'static str {
        self.adapter.adapter_name()
    }

    fn rect(&self) -> Rect {
        TopologyProvider::rect(self)
    }

    fn fetch_snapshot(&mut self) -> Result<DomainSnapshot> {
        Ok(self.single_leaf_snapshot())
    }

    fn supported_payload_types(&self) -> Vec<TypeId> {
        TilingDomain::supported_payload_types(self).to_vec()
    }

    fn tear_off(&mut self, native_id: &[u8]) -> Result<Box<dyn PaneState>> {
        self.tear_off_impl(&native_id.to_vec())
    }

    fn merge_in(
        &mut self,
        target_native_id: &[u8],
        dir: Direction,
        payload: Box<dyn PaneState>,
    ) -> Result<Vec<u8>> {
        self.merge_in_impl(&target_native_id.to_vec(), dir, payload)
    }
}

pub fn domain_id_for_window(
    app_id: Option<&str>,
    pid: Option<ProcessId>,
    title: Option<&str>,
) -> DomainId {
    crate::engine::resolution::resolve_window_domain_id(app_id, pid, title)
}

pub fn runtime_domains_for_window_manager(
    wm: &mut ConfiguredWindowManager,
) -> Result<Vec<Box<dyn ErasedDomain>>> {
    let mut domains: Vec<Box<dyn ErasedDomain>> = Vec::new();

    if let Some(factory) = wm.domain_factory() {
        domains.push(factory.create_domain(WM_DOMAIN_ID)?);
    } else {
        domains.push(Box::new(UnsupportedDomainPlugin::new(
            WM_DOMAIN_ID,
            wm.adapter_name(),
        )));
    }

    for adapter in crate::engine::resolution::default_app_domain_adapters() {
        let domain_id = domain_id_for_app_kind(adapter.kind());
        domains.push(Box::new(AppDomainPlugin::new(domain_id, adapter)));
    }

    let focused = wm.focused_window()?;
    let app_id = focused.app_id.unwrap_or_default();
    let title = focused.title.unwrap_or_default();
    let pid = focused.pid;
    let owner_pid = pid.map(ProcessId::get).unwrap_or(0);
    let mut overridden = HashSet::new();
    for adapter in crate::engine::resolution::resolve_app_chain(&app_id, owner_pid, &title) {
        let domain_id = domain_id_for_app_kind(adapter.kind());
        if overridden.insert(domain_id) {
            domains.push(Box::new(AppDomainPlugin::new(domain_id, adapter)));
        }
    }

    Ok(domains)
}

#[cfg(test)]
mod tests {
    use super::{decode_native_window_ref, domain_id_for_window, encode_native_window_ref};
    use crate::adapters::apps::{alacritty, foot, ghostty, kitty, wezterm};
    use crate::engine::resolution::resolve_window_domain_id;
    use crate::engine::runtime::ProcessId;

    #[test]
    fn native_window_ref_roundtrip_keeps_window_and_pid() {
        let pid = ProcessId::new(4242).expect("pid should be valid");
        let encoded = encode_native_window_ref(99, Some(pid));
        let decoded = decode_native_window_ref(&encoded).expect("native id should decode");
        assert_eq!(decoded.window_id, 99);
        assert_eq!(decoded.pid.map(ProcessId::get), Some(4242));
    }

    #[test]
    fn terminal_app_ids_classify_to_terminal_domain() {
        let _guard = crate::utils::env_guard();
        let root = std::env::temp_dir().join(format!(
            "yeet-and-yoink-domain-wezterm-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be monotonic")
                .as_nanos()
        ));
        let config_dir = root.join("yeet-and-yoink");
        std::fs::create_dir_all(&config_dir).expect("config dir should be created");
        std::fs::write(
            config_dir.join("config.toml"),
            r#"
[app.terminal.wezterm]
enabled = true
"#,
        )
        .expect("config file should be writable");
        let old_config = crate::config::snapshot();
        crate::config::prepare_with_path(Some(&config_dir.join("config.toml")))
            .expect("config should load");

        let domain = resolve_window_domain_id(Some(wezterm::APP_IDS[0]), None, Some("term"));
        assert_eq!(domain, super::TERMINAL_DOMAIN_ID);

        crate::config::install(old_config);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn kitty_app_ids_classify_to_terminal_domain() {
        let _guard = crate::utils::env_guard();
        let root = std::env::temp_dir().join(format!(
            "yeet-and-yoink-domain-kitty-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be monotonic")
                .as_nanos()
        ));
        let config_dir = root.join("yeet-and-yoink");
        std::fs::create_dir_all(&config_dir).expect("config dir should be created");
        std::fs::write(
            config_dir.join("config.toml"),
            r#"
[app.terminal.kitty]
enabled = true
"#,
        )
        .expect("config file should be writable");
        let old_config = crate::config::snapshot();
        crate::config::prepare_with_path(Some(&config_dir.join("config.toml")))
            .expect("config should load");

        let domain = domain_id_for_window(Some(kitty::APP_IDS[0]), None, Some("term"));
        assert_eq!(domain, super::TERMINAL_DOMAIN_ID);

        crate::config::install(old_config);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn foot_app_ids_classify_to_terminal_domain() {
        let _guard = crate::utils::env_guard();
        let root = std::env::temp_dir().join(format!(
            "yeet-and-yoink-domain-foot-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be monotonic")
                .as_nanos()
        ));
        let config_dir = root.join("yeet-and-yoink");
        std::fs::create_dir_all(&config_dir).expect("config dir should be created");
        std::fs::write(
            config_dir.join("config.toml"),
            r#"
[app.terminal.foot]
enabled = true
"#,
        )
        .expect("config file should be writable");
        let old_config = crate::config::snapshot();
        crate::config::prepare_with_path(Some(&config_dir.join("config.toml")))
            .expect("config should load");

        let domain = domain_id_for_window(Some(foot::APP_IDS[0]), None, Some("term"));
        assert_eq!(domain, super::TERMINAL_DOMAIN_ID);

        crate::config::install(old_config);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn alacritty_app_ids_classify_to_terminal_domain() {
        let _guard = crate::utils::env_guard();
        let root = std::env::temp_dir().join(format!(
            "yeet-and-yoink-domain-alacritty-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be monotonic")
                .as_nanos()
        ));
        let config_dir = root.join("yeet-and-yoink");
        std::fs::create_dir_all(&config_dir).expect("config dir should be created");
        std::fs::write(
            config_dir.join("config.toml"),
            r#"
[app.terminal.alacritty]
enabled = true
"#,
        )
        .expect("config file should be writable");
        let old_config = crate::config::snapshot();
        crate::config::prepare_with_path(Some(&config_dir.join("config.toml")))
            .expect("config should load");

        let domain = domain_id_for_window(Some(alacritty::APP_IDS[0]), None, Some("term"));
        assert_eq!(domain, super::TERMINAL_DOMAIN_ID);

        crate::config::install(old_config);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn ghostty_app_ids_classify_to_terminal_domain() {
        let _guard = crate::utils::env_guard();
        let root = std::env::temp_dir().join(format!(
            "yeet-and-yoink-domain-ghostty-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be monotonic")
                .as_nanos()
        ));
        let config_dir = root.join("yeet-and-yoink");
        std::fs::create_dir_all(&config_dir).expect("config dir should be created");
        std::fs::write(
            config_dir.join("config.toml"),
            r#"
[app.terminal.ghostty]
enabled = true
"#,
        )
        .expect("config file should be writable");
        let old_config = crate::config::snapshot();
        crate::config::prepare_with_path(Some(&config_dir.join("config.toml")))
            .expect("config should load");

        let domain = domain_id_for_window(Some(ghostty::APP_IDS[0]), None, Some("term"));
        assert_eq!(domain, super::TERMINAL_DOMAIN_ID);

        crate::config::install(old_config);
        let _ = std::fs::remove_dir_all(root);
    }
}

#[cfg(test)]
mod configured_window_manager_tests {
    use std::any::TypeId;

    use anyhow::Result;

    use super::super::pipeline::{DomainLeafSnapshot, DomainSnapshot};
    use super::super::registry::{PaneState, WM_DOMAIN_ID};
    use super::{runtime_domains_for_window_manager, ErasedDomain};
    use crate::engine::topology::Rect;
    use crate::engine::wm::{
        ConfiguredWindowManager, FocusedWindowRecord, ResizeIntent, WindowManagerCapabilities,
        WindowManagerDomainFactory, WindowManagerFeatures, WindowManagerSession, WindowRecord,
    };

    struct FakeSession;

    impl WindowManagerSession for FakeSession {
        fn adapter_name(&self) -> &'static str {
            "fake"
        }

        fn capabilities(&self) -> WindowManagerCapabilities {
            WindowManagerCapabilities::none()
        }

        fn focused_window(&mut self) -> Result<FocusedWindowRecord> {
            Ok(FocusedWindowRecord {
                id: 1,
                app_id: Some("fake-app".into()),
                title: Some("fake-title".into()),
                pid: None,
                original_tile_index: 1,
            })
        }

        fn windows(&mut self) -> Result<Vec<WindowRecord>> {
            Ok(Vec::new())
        }

        fn focus_direction(
            &mut self,
            _direction: crate::engine::topology::Direction,
        ) -> Result<()> {
            Ok(())
        }

        fn move_direction(&mut self, _direction: crate::engine::topology::Direction) -> Result<()> {
            Ok(())
        }

        fn resize_with_intent(&mut self, _intent: ResizeIntent) -> Result<()> {
            Ok(())
        }

        fn spawn(&mut self, _command: Vec<String>) -> Result<()> {
            Ok(())
        }

        fn focus_window_by_id(&mut self, _id: u64) -> Result<()> {
            Ok(())
        }

        fn close_window_by_id(&mut self, _id: u64) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn runtime_domains_accept_configured_window_manager_handle() {
        let mut wm = fake_wm_without_domain_factory("fake");

        let domains = runtime_domains_for_window_manager(&mut wm)
            .expect("configured window manager should load runtime domains");

        assert_eq!(domains[0].domain_id(), WM_DOMAIN_ID);
    }

    #[test]
    fn runtime_domains_uses_wm_domain_factory_when_present() {
        let mut wm = fake_wm_with_domain_factory("wm-test");

        let domains = runtime_domains_for_window_manager(&mut wm).unwrap();

        assert!(domains
            .iter()
            .any(|domain| domain.domain_name() == "wm-test"));
    }

    #[test]
    fn runtime_domains_recreates_wm_domain_on_subsequent_calls() {
        let mut wm = fake_wm_with_domain_factory("wm-test");

        let first = runtime_domains_for_window_manager(&mut wm).unwrap();
        let second = runtime_domains_for_window_manager(&mut wm).unwrap();

        assert!(first.iter().any(|domain| domain.domain_name() == "wm-test"));
        assert!(second
            .iter()
            .any(|domain| domain.domain_name() == "wm-test"));
    }

    #[test]
    fn runtime_domains_uses_generic_unsupported_domain_when_factory_absent() {
        let mut wm = fake_wm_without_domain_factory("fake");

        let domains = runtime_domains_for_window_manager(&mut wm).unwrap();

        assert!(domains.iter().any(|domain| domain.domain_name() == "fake"));
    }

    fn fake_wm_with_domain_factory(name: &'static str) -> ConfiguredWindowManager {
        let mut features = WindowManagerFeatures::default();
        features.domain_factory = Some(Box::new(FakeDomainFactory::new(name)));
        ConfiguredWindowManager::new(Box::new(FakeSession), features)
    }

    fn fake_wm_without_domain_factory(name: &'static str) -> ConfiguredWindowManager {
        ConfiguredWindowManager::new(
            Box::new(NamedFakeSession(name)),
            WindowManagerFeatures::default(),
        )
    }

    struct FakeDomainFactory {
        name: &'static str,
    }

    impl FakeDomainFactory {
        fn new(name: &'static str) -> Self {
            Self { name }
        }
    }

    impl WindowManagerDomainFactory for FakeDomainFactory {
        fn create_domain(
            &self,
            _domain_id: crate::engine::topology::DomainId,
        ) -> Result<Box<dyn ErasedDomain>> {
            Ok(Box::new(FakeRuntimeDomain::new(self.name)))
        }
    }

    struct NamedFakeSession(&'static str);

    impl WindowManagerSession for NamedFakeSession {
        fn adapter_name(&self) -> &'static str {
            self.0
        }

        fn capabilities(&self) -> WindowManagerCapabilities {
            WindowManagerCapabilities::none()
        }

        fn focused_window(&mut self) -> Result<FocusedWindowRecord> {
            FakeSession.focused_window()
        }

        fn windows(&mut self) -> Result<Vec<WindowRecord>> {
            FakeSession.windows()
        }

        fn focus_direction(&mut self, direction: crate::engine::topology::Direction) -> Result<()> {
            FakeSession.focus_direction(direction)
        }

        fn move_direction(&mut self, direction: crate::engine::topology::Direction) -> Result<()> {
            FakeSession.move_direction(direction)
        }

        fn resize_with_intent(&mut self, intent: ResizeIntent) -> Result<()> {
            FakeSession.resize_with_intent(intent)
        }

        fn spawn(&mut self, command: Vec<String>) -> Result<()> {
            FakeSession.spawn(command)
        }

        fn focus_window_by_id(&mut self, id: u64) -> Result<()> {
            FakeSession.focus_window_by_id(id)
        }

        fn close_window_by_id(&mut self, id: u64) -> Result<()> {
            FakeSession.close_window_by_id(id)
        }
    }

    struct FakeRuntimeDomain {
        name: &'static str,
    }

    impl FakeRuntimeDomain {
        fn new(name: &'static str) -> Self {
            Self { name }
        }
    }

    impl ErasedDomain for FakeRuntimeDomain {
        fn domain_id(&self) -> u64 {
            WM_DOMAIN_ID
        }

        fn domain_name(&self) -> &'static str {
            self.name
        }

        fn rect(&self) -> Rect {
            Rect {
                x: 0,
                y: 0,
                w: 100,
                h: 100,
            }
        }

        fn fetch_snapshot(&mut self) -> Result<DomainSnapshot> {
            Ok(DomainSnapshot {
                domain_id: WM_DOMAIN_ID,
                rect: self.rect(),
                leaves: vec![DomainLeafSnapshot {
                    id: 1,
                    native_id: vec![1],
                    rect: self.rect(),
                    focused: true,
                }],
            })
        }

        fn supported_payload_types(&self) -> Vec<TypeId> {
            Vec::new()
        }

        fn tear_off(&mut self, _native_id: &[u8]) -> Result<Box<dyn PaneState>> {
            anyhow::bail!("fake runtime domain does not support tear-off")
        }

        fn merge_in(
            &mut self,
            _target_native_id: &[u8],
            _dir: crate::engine::topology::Direction,
            _payload: Box<dyn PaneState>,
        ) -> Result<Vec<u8>> {
            anyhow::bail!("fake runtime domain does not support merge-in")
        }
    }
}
