pub(crate) mod capabilities;
pub(crate) mod configured;
pub(crate) mod session;

pub use capabilities::*;
pub use configured::*;
pub use session::*;

// ---------------------------------------------------------------------------
// Window manager connection helpers (moved from window_manager.rs shim)
// ---------------------------------------------------------------------------

use crate::adapters::window_managers::spec_for_backend;
use crate::config::selected_wm_backend;
use anyhow::{anyhow, Context, Result};

fn connect_backend(
    backend: crate::config::WmBackend,
    spec: &'static dyn WindowManagerSpec,
) -> Result<ConfiguredWindowManager> {
    if spec.backend() != backend {
        return Err(anyhow!(
            "wm backend '{}' resolved to mismatched spec '{}'",
            backend.as_str(),
            spec.name()
        ));
    }

    spec.connect()
        .with_context(|| format!("failed to connect configured wm '{}'", spec.name()))
}

#[cfg(test)]
pub fn connect_backend_for_test(
    backend: crate::config::WmBackend,
    spec: &'static dyn WindowManagerSpec,
) -> Result<ConfiguredWindowManager> {
    connect_backend(backend, spec)
}

pub fn connect_selected() -> Result<ConfiguredWindowManager> {
    let _span = tracing::debug_span!("window_managers.connect_selected").entered();
    let backend = selected_wm_backend();
    let spec = spec_for_backend(backend);
    connect_backend(backend, spec)
}

#[cfg(test)]
mod tests {
    use super::{
        plan_resize, plan_tear_out, validate_declared_capabilities, CapabilitySupport,
        ConfiguredWindowManager, DirectionalCapability, FocusedWindowRecord,
        PrimitiveWindowManagerCapabilities, ResizeIntent, WindowCycleProvider, WindowCycleRequest,
        WindowManagerCapabilities, WindowManagerCapabilityDescriptor, WindowManagerFeatures,
        WindowManagerSession, WindowManagerSpec, WindowRecord, WindowTearOutComposer,
    };
    use crate::adapters::window_managers::spec_for_backend;
    #[cfg(target_os = "macos")]
    use crate::adapters::window_managers::yabai::YabaiAdapter;
    #[cfg(target_os = "linux")]
    use crate::adapters::window_managers::NiriAdapter;
    use crate::config::WmBackend;
    use crate::engine::topology::Direction;
    use anyhow::Result;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use tracing::Subscriber;
    use tracing_subscriber::layer::{Context as LayerContext, Layer};
    use tracing_subscriber::prelude::*;

    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn configured_window_manager_delegates_to_object_safe_core() {
        let mut wm = fake_configured_wm();
        assert_eq!(wm.adapter_name(), "fake");
        assert_eq!(wm.focused_window().unwrap().id, 42);
        wm.focus_direction(Direction::West).unwrap();
        assert_eq!(wm.take_calls(), vec!["focus_direction:west"]);
    }

    #[test]
    fn configured_window_manager_exposes_optional_capabilities_independently() {
        let wm = fake_configured_wm_with_cycle_provider();
        assert!(wm.window_cycle().is_some());
        assert!(wm.domain_factory().is_none());
    }

    #[test]
    fn configured_window_manager_rejects_composed_tear_out_without_composer() {
        let calls = Arc::new(Mutex::new(Vec::new()));

        let err = match ConfiguredWindowManager::try_new(
            Box::new(FakeSession::with_capabilities(
                calls,
                composed_tear_out_capabilities(Direction::North),
            )),
            WindowManagerFeatures::default(),
        ) {
            Ok(_) => panic!("composed tear-out should require a tear-out composer"),
            Err(err) => err,
        };

        assert!(err.to_string().contains("missing a tear-out composer"));
    }

    #[test]
    fn configured_window_manager_accepts_composed_tear_out_with_composer() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut features = WindowManagerFeatures::default();
        features.tear_out_composer = Some(Box::new(FakeTearOutComposer));

        let wm = ConfiguredWindowManager::new(
            Box::new(FakeSession::with_capabilities(
                calls,
                composed_tear_out_capabilities(Direction::North),
            )),
            features,
        );

        assert!(wm.tear_out_composer().is_some());
    }

    #[test]
    fn declared_capabilities_fail_validation_when_composed_primitives_missing() {
        let error = validate_declared_capabilities::<InvalidComposedCapabilities>()
            .expect_err("invalid composed capabilities should fail validation");
        assert!(error
            .to_string()
            .contains("invalid capabilities for adapter"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn niri_capabilities_are_valid() {
        validate_declared_capabilities::<NiriAdapter>()
            .expect("niri capability descriptor should be valid");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn tear_out_and_resize_plans_resolve_native_composed_and_unsupported() {
        let capabilities = NiriAdapter::CAPABILITIES;
        assert_eq!(
            plan_tear_out(capabilities, Direction::East),
            CapabilitySupport::Native
        );
        assert_eq!(
            plan_tear_out(capabilities, Direction::North),
            CapabilitySupport::Composed
        );
        assert_eq!(
            plan_resize(capabilities, Direction::West),
            CapabilitySupport::Native
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn yabai_capabilities_are_valid() {
        validate_declared_capabilities::<YabaiAdapter>()
            .expect("yabai capability descriptor should be valid");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn yabai_tear_out_and_resize_plans() {
        let capabilities = YabaiAdapter::CAPABILITIES;
        assert_eq!(
            plan_tear_out(capabilities, Direction::East),
            CapabilitySupport::Unsupported
        );
        assert_eq!(
            plan_resize(capabilities, Direction::West),
            CapabilitySupport::Native
        );
    }

    #[test]
    fn built_in_connectors_are_typed_as_configured_window_managers() {
        fn assert_spec(_spec: &'static dyn WindowManagerSpec) {}

        assert_spec(spec_for_backend(WmBackend::Niri));
        assert_spec(spec_for_backend(WmBackend::I3));
        assert_spec(spec_for_backend(WmBackend::Hyprland));
        assert_spec(spec_for_backend(WmBackend::Mangowc));
        assert_spec(spec_for_backend(WmBackend::Paneru));
        assert_spec(spec_for_backend(WmBackend::Yabai));
        let _ = super::connect_selected as fn() -> Result<ConfiguredWindowManager>;
    }

    #[test]
    fn mangowc_wm_backend_spec_is_registered() {
        fn assert_spec(_spec: &'static dyn WindowManagerSpec) {}
        assert_spec(spec_for_backend(WmBackend::Mangowc));
    }

    #[test]
    fn mangowc_wm_backend_runtime_support_matches_platform() {
        #[cfg(target_os = "linux")]
        {
            if let Err(err) = spec_for_backend(WmBackend::Mangowc).connect() {
                assert!(
                    !err.to_string()
                        .contains("wm backend 'mangowc' is not yet supported at runtime"),
                    "linux mangowc backend should now fail only for real runtime causes: {err}"
                );
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            let err = spec_for_backend(WmBackend::Mangowc)
                .connect()
                .expect_err("non-linux mangowc backend should stay unsupported");
            assert!(err.to_string().contains("not supported"));
        }
    }

    #[test]
    fn connect_selected_reports_configured_backend_failure_without_fallback() {
        let err =
            match super::connect_backend_for_test(WmBackend::Niri, failing_spec(WmBackend::Niri)) {
                Ok(_) => panic!("configured backend should fail without fallback"),
                Err(err) => err,
            };
        assert!(err.to_string().contains("niri"));
        assert!(!err.to_string().contains("i3"));
    }

    #[test]
    fn connect_selected_emits_legacy_adapter_span_name() {
        let _guard = crate::utils::env_guard();
        let root = unique_temp_dir("connect-selected-span");
        let config_dir = root.join("yeetnyoink");
        std::fs::create_dir_all(&config_dir).expect("config dir should be created");
        let config_path = config_dir.join("config.toml");
        std::fs::write(
            &config_path,
            format!(
                r#"
[wm]
enabled_integration = "{backend}"
"#,
                backend = unsupported_backend().as_str()
            ),
        )
        .expect("config file should be writable");

        let old_config = crate::config::snapshot();
        crate::config::prepare_with_path(Some(&config_path)).expect("config should load");

        let span_names = Arc::new(Mutex::new(Vec::new()));
        let subscriber =
            tracing_subscriber::registry().with(SpanNameLayer::new(span_names.clone()));
        let result = tracing::subscriber::with_default(subscriber, super::connect_selected);

        crate::config::install(old_config);
        let _ = std::fs::remove_dir_all(root);

        assert!(result.is_err(), "unsupported backend should fail quickly");
        assert!(span_names
            .lock()
            .expect("span names lock should not be poisoned")
            .iter()
            .any(|name| name == "window_managers.connect_selected"));
    }

    #[test]
    fn failing_spec_uses_requested_backend() {
        let spec = failing_spec(WmBackend::Yabai);

        assert_eq!(spec.backend(), WmBackend::Yabai);
        assert_eq!(spec.name(), "yabai");
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "yeetnyoink-window-manager-{prefix}-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("temp dir should be created");
        path
    }

    fn unsupported_backend() -> WmBackend {
        #[cfg(target_os = "macos")]
        {
            WmBackend::Niri
        }

        #[cfg(target_os = "linux")]
        {
            WmBackend::Yabai
        }
    }

    struct SpanNameLayer {
        names: Arc<Mutex<Vec<String>>>,
    }

    impl SpanNameLayer {
        fn new(names: Arc<Mutex<Vec<String>>>) -> Self {
            Self { names }
        }
    }

    impl<S> Layer<S> for SpanNameLayer
    where
        S: Subscriber,
    {
        fn on_new_span(
            &self,
            attrs: &tracing::span::Attributes<'_>,
            _id: &tracing::Id,
            _ctx: LayerContext<'_, S>,
        ) {
            self.names
                .lock()
                .expect("span names lock should not be poisoned")
                .push(attrs.metadata().name().to_string());
        }
    }

    fn fake_configured_wm() -> TestConfiguredWindowManager {
        let calls = Arc::new(Mutex::new(Vec::new()));
        TestConfiguredWindowManager::new(
            ConfiguredWindowManager::new(
                Box::new(FakeSession::new(calls.clone())),
                WindowManagerFeatures::default(),
            ),
            calls,
        )
    }

    fn fake_configured_wm_with_cycle_provider() -> TestConfiguredWindowManager {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut features = WindowManagerFeatures::default();
        features.window_cycle = Some(Box::new(FakeCycleProvider));
        TestConfiguredWindowManager::new(
            ConfiguredWindowManager::new(Box::new(FakeSession::new(calls.clone())), features),
            calls,
        )
    }

    fn composed_tear_out_capabilities(direction: Direction) -> WindowManagerCapabilities {
        let mut capabilities = WindowManagerCapabilities::none();
        capabilities.primitives.tear_out_right = true;
        capabilities.primitives.move_column = true;
        capabilities.primitives.consume_into_column_and_move = true;

        match direction {
            Direction::West => capabilities.tear_out.west = CapabilitySupport::Composed,
            Direction::East => capabilities.tear_out.east = CapabilitySupport::Composed,
            Direction::North => capabilities.tear_out.north = CapabilitySupport::Composed,
            Direction::South => capabilities.tear_out.south = CapabilitySupport::Composed,
        }

        capabilities
    }

    struct InvalidComposedCapabilities;

    impl WindowManagerCapabilityDescriptor for InvalidComposedCapabilities {
        const NAME: &'static str = "invalid";
        const CAPABILITIES: WindowManagerCapabilities = WindowManagerCapabilities {
            primitives: PrimitiveWindowManagerCapabilities {
                tear_out_right: true,
                move_column: false,
                consume_into_column_and_move: false,
                set_window_width: true,
                set_window_height: true,
            },
            tear_out: DirectionalCapability {
                west: CapabilitySupport::Composed,
                east: CapabilitySupport::Native,
                north: CapabilitySupport::Unsupported,
                south: CapabilitySupport::Unsupported,
            },
            resize: DirectionalCapability::uniform(CapabilitySupport::Unsupported),
        };
    }

    fn failing_spec(backend: WmBackend) -> &'static dyn WindowManagerSpec {
        Box::leak(Box::new(FailingSpec { backend }))
    }

    struct FailingSpec {
        backend: WmBackend,
    }

    impl WindowManagerSpec for FailingSpec {
        fn backend(&self) -> WmBackend {
            self.backend
        }

        fn name(&self) -> &'static str {
            self.backend.as_str()
        }

        fn connect(&self) -> Result<ConfiguredWindowManager> {
            Err(anyhow::anyhow!("{} connection failed", self.name()))
        }
    }

    struct TestConfiguredWindowManager {
        wm: ConfiguredWindowManager,
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl TestConfiguredWindowManager {
        fn new(wm: ConfiguredWindowManager, calls: Arc<Mutex<Vec<String>>>) -> Self {
            Self { wm, calls }
        }

        fn take_calls(&mut self) -> Vec<String> {
            let mut calls = self
                .calls
                .lock()
                .expect("calls mutex should not be poisoned");
            std::mem::take(&mut *calls)
        }
    }

    impl std::ops::Deref for TestConfiguredWindowManager {
        type Target = ConfiguredWindowManager;

        fn deref(&self) -> &Self::Target {
            &self.wm
        }
    }

    impl std::ops::DerefMut for TestConfiguredWindowManager {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.wm
        }
    }

    struct FakeSession {
        calls: Arc<Mutex<Vec<String>>>,
        capabilities: WindowManagerCapabilities,
    }

    impl FakeSession {
        fn new(calls: Arc<Mutex<Vec<String>>>) -> Self {
            Self {
                calls,
                capabilities: WindowManagerCapabilities::none(),
            }
        }

        fn with_capabilities(
            calls: Arc<Mutex<Vec<String>>>,
            capabilities: WindowManagerCapabilities,
        ) -> Self {
            Self {
                calls,
                capabilities,
            }
        }

        fn push_call(&self, call: impl Into<String>) {
            self.calls
                .lock()
                .expect("calls mutex should not be poisoned")
                .push(call.into());
        }
    }

    impl WindowManagerSession for FakeSession {
        fn adapter_name(&self) -> &'static str {
            "fake"
        }

        fn capabilities(&self) -> WindowManagerCapabilities {
            self.capabilities
        }

        fn focused_window(&mut self) -> Result<FocusedWindowRecord> {
            Ok(FocusedWindowRecord {
                id: 42,
                app_id: Some("fake-app".to_string()),
                title: Some("fake-title".to_string()),
                pid: None,
                original_tile_index: 1,
            })
        }

        fn windows(&mut self) -> Result<Vec<WindowRecord>> {
            Ok(Vec::new())
        }

        fn focus_direction(&mut self, direction: Direction) -> Result<()> {
            self.push_call(format!("focus_direction:{direction}"));
            Ok(())
        }

        fn move_direction(&mut self, _direction: Direction) -> Result<()> {
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

    struct FakeCycleProvider;

    impl WindowCycleProvider for FakeCycleProvider {
        fn focus_or_cycle(&mut self, _request: &WindowCycleRequest) -> Result<()> {
            Ok(())
        }
    }

    struct FakeTearOutComposer;

    impl WindowTearOutComposer for FakeTearOutComposer {
        fn compose_tear_out(
            &mut self,
            _direction: Direction,
            _source_tile_index: usize,
        ) -> Result<()> {
            Ok(())
        }
    }
}
