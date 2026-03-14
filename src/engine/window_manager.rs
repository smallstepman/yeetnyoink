use anyhow::{anyhow, Context, Result};

use crate::adapters::window_managers::{
    plan_tear_out, CapabilitySupport, WindowManagerCapabilities,
};
use crate::config::WmBackend;
use crate::engine::runtime::ProcessId;
use crate::engine::topology::Direction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeKind {
    Grow,
    Shrink,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResizeIntent {
    pub direction: Direction,
    pub kind: ResizeKind,
    pub step: i32,
}

impl ResizeIntent {
    pub const fn new(direction: Direction, kind: ResizeKind, step: i32) -> Self {
        Self {
            direction,
            kind,
            step,
        }
    }

    pub const fn grow(self) -> bool {
        matches!(self.kind, ResizeKind::Grow)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowRecord {
    pub id: u64,
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub pid: Option<ProcessId>,
    pub is_focused: bool,
    pub original_tile_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusedWindowRecord {
    pub id: u64,
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub pid: Option<ProcessId>,
    pub original_tile_index: usize,
}

impl FocusedWindowRecord {}

pub trait WindowManagerSession: Send {
    fn adapter_name(&self) -> &'static str;
    fn capabilities(&self) -> WindowManagerCapabilities;
    fn focused_window(&mut self) -> Result<FocusedWindowRecord>;
    fn windows(&mut self) -> Result<Vec<WindowRecord>>;
    fn focus_direction(&mut self, direction: Direction) -> Result<()>;
    fn move_direction(&mut self, direction: Direction) -> Result<()>;
    fn resize_with_intent(&mut self, intent: ResizeIntent) -> Result<()>;
    fn spawn(&mut self, command: Vec<String>) -> Result<()>;
    fn focus_window_by_id(&mut self, id: u64) -> Result<()>;
    fn close_window_by_id(&mut self, id: u64) -> Result<()>;
}

pub trait WindowManagerDomainFactory: Send {
    fn create_domain(
        &self,
        domain_id: crate::engine::topology::DomainId,
    ) -> Result<Box<dyn crate::engine::domain::ErasedDomain>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowCycleRequest {
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub spawn: Option<String>,
    pub new: bool,
    pub summon: bool,
}

pub trait WindowCycleProvider: Send {
    fn focus_or_cycle(&mut self, request: &WindowCycleRequest) -> Result<()>;
}

pub trait WindowTearOutComposer: Send {
    fn compose_tear_out(&mut self, direction: Direction, source_tile_index: usize) -> Result<()>;
}

#[derive(Default)]
pub struct WindowManagerFeatures {
    pub domain_factory: Option<Box<dyn WindowManagerDomainFactory>>,
    pub window_cycle: Option<Box<dyn WindowCycleProvider>>,
    pub tear_out_composer: Option<Box<dyn WindowTearOutComposer>>,
}

pub struct ConfiguredWindowManager {
    core: Box<dyn WindowManagerSession>,
    features: WindowManagerFeatures,
}

impl ConfiguredWindowManager {
    pub fn new(core: Box<dyn WindowManagerSession>, features: WindowManagerFeatures) -> Self {
        Self::try_new(core, features).expect(
            "configured window manager should have valid capabilities and required features",
        )
    }

    pub fn try_new(
        core: Box<dyn WindowManagerSession>,
        features: WindowManagerFeatures,
    ) -> Result<Self> {
        validate_configured_window_manager(core.as_ref(), &features)?;
        Ok(Self { core, features })
    }

    pub fn adapter_name(&self) -> &'static str {
        self.core.adapter_name()
    }

    pub fn capabilities(&self) -> WindowManagerCapabilities {
        self.core.capabilities()
    }

    pub fn focused_window(&mut self) -> Result<FocusedWindowRecord> {
        self.core.focused_window()
    }

    pub fn windows(&mut self) -> Result<Vec<WindowRecord>> {
        self.core.windows()
    }

    pub fn focus_direction(&mut self, direction: Direction) -> Result<()> {
        self.core.focus_direction(direction)
    }

    pub fn move_direction(&mut self, direction: Direction) -> Result<()> {
        self.core.move_direction(direction)
    }

    pub fn resize_with_intent(&mut self, intent: ResizeIntent) -> Result<()> {
        self.core.resize_with_intent(intent)
    }

    pub fn spawn(&mut self, command: Vec<String>) -> Result<()> {
        self.core.spawn(command)
    }

    pub fn focus_window_by_id(&mut self, id: u64) -> Result<()> {
        self.core.focus_window_by_id(id)
    }

    pub fn close_window_by_id(&mut self, id: u64) -> Result<()> {
        self.core.close_window_by_id(id)
    }

    pub fn domain_factory(&self) -> Option<&dyn WindowManagerDomainFactory> {
        self.features.domain_factory.as_deref()
    }

    pub fn window_cycle(&self) -> Option<&dyn WindowCycleProvider> {
        self.features.window_cycle.as_deref()
    }

    pub fn window_cycle_mut(&mut self) -> Option<&mut (dyn WindowCycleProvider + '_)> {
        match self.features.window_cycle.as_mut() {
            Some(provider) => Some(provider.as_mut()),
            None => None,
        }
    }

    pub fn tear_out_composer(&self) -> Option<&dyn WindowTearOutComposer> {
        self.features.tear_out_composer.as_deref()
    }

    pub fn tear_out_composer_mut(&mut self) -> Option<&mut (dyn WindowTearOutComposer + '_)> {
        match self.features.tear_out_composer.as_mut() {
            Some(composer) => Some(composer.as_mut()),
            None => None,
        }
    }
}

fn validate_configured_window_manager(
    core: &dyn WindowManagerSession,
    features: &WindowManagerFeatures,
) -> Result<()> {
    let capabilities = core.capabilities();
    let adapter_name = core.adapter_name();

    capabilities
        .validate()
        .with_context(|| format!("invalid capabilities for configured wm '{}'", adapter_name))?;

    if features.tear_out_composer.is_none() {
        for direction in [
            Direction::West,
            Direction::East,
            Direction::North,
            Direction::South,
        ] {
            if matches!(
                plan_tear_out(capabilities, direction),
                CapabilitySupport::Composed
            ) {
                return Err(anyhow!(
                    "configured wm '{}' is missing a tear-out composer for {direction}",
                    adapter_name
                ));
            }
        }
    }

    Ok(())
}

impl WindowManagerSession for ConfiguredWindowManager {
    fn adapter_name(&self) -> &'static str {
        ConfiguredWindowManager::adapter_name(self)
    }

    fn capabilities(&self) -> WindowManagerCapabilities {
        ConfiguredWindowManager::capabilities(self)
    }

    fn focused_window(&mut self) -> Result<FocusedWindowRecord> {
        ConfiguredWindowManager::focused_window(self)
    }

    fn windows(&mut self) -> Result<Vec<WindowRecord>> {
        ConfiguredWindowManager::windows(self)
    }

    fn focus_direction(&mut self, direction: Direction) -> Result<()> {
        ConfiguredWindowManager::focus_direction(self, direction)
    }

    fn move_direction(&mut self, direction: Direction) -> Result<()> {
        ConfiguredWindowManager::move_direction(self, direction)
    }

    fn resize_with_intent(&mut self, intent: ResizeIntent) -> Result<()> {
        ConfiguredWindowManager::resize_with_intent(self, intent)
    }

    fn spawn(&mut self, command: Vec<String>) -> Result<()> {
        ConfiguredWindowManager::spawn(self, command)
    }

    fn focus_window_by_id(&mut self, id: u64) -> Result<()> {
        ConfiguredWindowManager::focus_window_by_id(self, id)
    }

    fn close_window_by_id(&mut self, id: u64) -> Result<()> {
        ConfiguredWindowManager::close_window_by_id(self, id)
    }
}

pub trait WindowManagerSpec: Sync {
    fn backend(&self) -> WmBackend;
    fn name(&self) -> &'static str;
    fn connect(&self) -> Result<ConfiguredWindowManager>;
}

#[cfg(test)]
mod tests {
    use super::{
        ConfiguredWindowManager, FocusedWindowRecord, ResizeIntent, WindowCycleProvider,
        WindowCycleRequest, WindowManagerFeatures, WindowManagerSession, WindowRecord,
        WindowTearOutComposer,
    };
    use crate::adapters::window_managers::{
        plan_resize, plan_tear_out, validate_declared_capabilities, CapabilitySupport,
        PrimitiveWindowManagerCapabilities, WindowManagerCapabilities,
        WindowManagerCapabilityDescriptor,
    };
    #[cfg(target_os = "linux")]
    use crate::adapters::window_managers::NiriAdapter;
    #[cfg(target_os = "macos")]
    use crate::adapters::window_managers::yabai::YabaiAdapter;
    use crate::engine::topology::Direction;
    use anyhow::Result;
    use std::sync::{Arc, Mutex};

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
            tear_out: crate::adapters::window_managers::DirectionalCapability {
                west: CapabilitySupport::Composed,
                east: CapabilitySupport::Native,
                north: CapabilitySupport::Unsupported,
                south: CapabilitySupport::Unsupported,
            },
            resize: crate::adapters::window_managers::DirectionalCapability::uniform(
                CapabilitySupport::Unsupported,
            ),
        };
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
