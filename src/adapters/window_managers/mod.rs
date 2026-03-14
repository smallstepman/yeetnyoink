#[cfg(target_os = "linux")]
pub mod i3;
#[cfg(any(test, target_os = "linux"))]
pub mod niri;
#[cfg(target_os = "macos")]
pub mod paneru;
#[cfg(target_os = "macos")]
pub mod yabai;

use anyhow::{anyhow, Context, Result};

#[cfg(target_os = "linux")]
use crate::adapters::window_managers::i3::I3_SPEC;
#[cfg(any(test, target_os = "linux"))]
use crate::adapters::window_managers::niri::Niri;
#[cfg(target_os = "linux")]
use crate::adapters::window_managers::niri::NIRI_SPEC;
#[cfg(target_os = "macos")]
use crate::adapters::window_managers::paneru::PANERU_SPEC;
#[cfg(target_os = "macos")]
use crate::adapters::window_managers::yabai::YABAI_SPEC;
use crate::config::{selected_wm_backend, WmBackend};
use crate::engine::runtime::ProcessId;
use crate::engine::topology::Direction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilitySupport {
    Native,
    Composed,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectionalCapability {
    pub west: CapabilitySupport,
    pub east: CapabilitySupport,
    pub north: CapabilitySupport,
    pub south: CapabilitySupport,
}

impl DirectionalCapability {
    pub const fn uniform(value: CapabilitySupport) -> Self {
        Self {
            west: value,
            east: value,
            north: value,
            south: value,
        }
    }

    pub const fn for_direction(self, direction: Direction) -> CapabilitySupport {
        match direction {
            Direction::West => self.west,
            Direction::East => self.east,
            Direction::North => self.north,
            Direction::South => self.south,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrimitiveWindowManagerCapabilities {
    pub tear_out_right: bool,
    pub move_column: bool,
    pub consume_into_column_and_move: bool,
    pub set_window_width: bool,
    pub set_window_height: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowManagerCapabilities {
    pub primitives: PrimitiveWindowManagerCapabilities,
    pub tear_out: DirectionalCapability,
    pub resize: DirectionalCapability,
}

impl WindowManagerCapabilities {
    pub const fn none() -> Self {
        Self {
            primitives: PrimitiveWindowManagerCapabilities {
                tear_out_right: false,
                move_column: false,
                consume_into_column_and_move: false,
                set_window_width: false,
                set_window_height: false,
            },
            tear_out: DirectionalCapability::uniform(CapabilitySupport::Unsupported),
            resize: DirectionalCapability::uniform(CapabilitySupport::Unsupported),
        }
    }

    pub fn validate(self) -> Result<()> {
        for direction in [
            Direction::West,
            Direction::East,
            Direction::North,
            Direction::South,
        ] {
            if matches!(
                self.tear_out.for_direction(direction),
                CapabilitySupport::Composed
            ) && !supports_composed_tear_out(self, direction)
            {
                return Err(anyhow!(
                    "invalid capability declaration: tear_out.{direction} is composed but required primitives are missing"
                ));
            }

            if matches!(
                self.resize.for_direction(direction),
                CapabilitySupport::Composed
            ) && !supports_composed_resize(self, direction)
            {
                return Err(anyhow!(
                    "invalid capability declaration: resize.{direction} is composed but required primitives are missing"
                ));
            }
        }

        Ok(())
    }
}

pub fn plan_tear_out(
    capabilities: WindowManagerCapabilities,
    direction: Direction,
) -> CapabilitySupport {
    match capabilities.tear_out.for_direction(direction) {
        CapabilitySupport::Native => CapabilitySupport::Native,
        CapabilitySupport::Composed if supports_composed_tear_out(capabilities, direction) => {
            CapabilitySupport::Composed
        }
        _ => CapabilitySupport::Unsupported,
    }
}

pub fn plan_resize(
    capabilities: WindowManagerCapabilities,
    direction: Direction,
) -> CapabilitySupport {
    match capabilities.resize.for_direction(direction) {
        CapabilitySupport::Native => CapabilitySupport::Native,
        CapabilitySupport::Composed if supports_composed_resize(capabilities, direction) => {
            CapabilitySupport::Composed
        }
        _ => CapabilitySupport::Unsupported,
    }
}

fn supports_composed_tear_out(
    capabilities: WindowManagerCapabilities,
    direction: Direction,
) -> bool {
    if !capabilities.primitives.tear_out_right {
        return false;
    }

    match direction {
        Direction::East => capabilities.primitives.move_column,
        Direction::West => capabilities.primitives.move_column,
        Direction::North | Direction::South => capabilities.primitives.consume_into_column_and_move,
    }
}

fn supports_composed_resize(capabilities: WindowManagerCapabilities, direction: Direction) -> bool {
    match direction {
        Direction::West | Direction::East => capabilities.primitives.set_window_width,
        Direction::North | Direction::South => capabilities.primitives.set_window_height,
    }
}

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

impl FocusedWindowRecord {
}

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
        Self { core, features }
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

pub trait WindowManagerCapabilityDescriptor {
    const NAME: &'static str;
    const CAPABILITIES: WindowManagerCapabilities;
}

pub fn validate_declared_capabilities<T: WindowManagerCapabilityDescriptor>() -> Result<()> {
    T::CAPABILITIES
        .validate()
        .with_context(|| format!("invalid capabilities for adapter '{}'", T::NAME))
}

// ---------------------------------------------------------------------------
// Linux: Niri adapter
// ---------------------------------------------------------------------------

#[cfg(any(test, target_os = "linux"))]
pub struct NiriAdapter {
    pub(crate) inner: std::sync::Arc<std::sync::Mutex<Niri>>,
}

#[cfg(any(test, target_os = "linux"))]
impl NiriAdapter {
    pub fn connect() -> Result<Self> {
        validate_declared_capabilities::<Self>()?;
        Ok(Self::from_shared(std::sync::Arc::new(
            std::sync::Mutex::new(Niri::connect()?),
        )))
    }

    pub(crate) fn from_shared(inner: std::sync::Arc<std::sync::Mutex<Niri>>) -> Self {
        Self { inner }
    }

    fn with_inner<R>(&self, f: impl FnOnce(&mut Niri) -> Result<R>) -> Result<R> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| anyhow!("niri adapter mutex should not be poisoned"))?;
        f(&mut inner)
    }
}

#[cfg(any(test, target_os = "linux"))]
impl WindowManagerCapabilityDescriptor for NiriAdapter {
    const NAME: &'static str = "niri";
    const CAPABILITIES: WindowManagerCapabilities = WindowManagerCapabilities {
        primitives: PrimitiveWindowManagerCapabilities {
            tear_out_right: true,
            move_column: true,
            consume_into_column_and_move: true,
            set_window_width: true,
            set_window_height: true,
        },
        tear_out: DirectionalCapability {
            west: CapabilitySupport::Composed,
            east: CapabilitySupport::Native,
            north: CapabilitySupport::Composed,
            south: CapabilitySupport::Composed,
        },
        resize: DirectionalCapability {
            west: CapabilitySupport::Native,
            east: CapabilitySupport::Native,
            north: CapabilitySupport::Native,
            south: CapabilitySupport::Native,
        },
    };
}

#[cfg(any(test, target_os = "linux"))]
impl WindowManagerSession for NiriAdapter {
    fn adapter_name(&self) -> &'static str {
        Self::NAME
    }

    fn capabilities(&self) -> WindowManagerCapabilities {
        Self::CAPABILITIES
    }

    fn focused_window(&mut self) -> Result<FocusedWindowRecord> {
        let window = self.with_inner(|inner| inner.focused_window())?;
        Ok(FocusedWindowRecord {
            id: window.id,
            app_id: window.app_id,
            title: window.title,
            pid: window
                .pid
                .and_then(|raw| u32::try_from(raw).ok())
                .and_then(ProcessId::new),
            original_tile_index: window
                .layout
                .pos_in_scrolling_layout
                .map(|(_, tile_idx)| tile_idx)
                .unwrap_or(1),
        })
    }

    fn windows(&mut self) -> Result<Vec<WindowRecord>> {
        Ok(self
            .with_inner(|inner| inner.windows())?
            .into_iter()
            .map(|window| WindowRecord {
                id: window.id,
                app_id: window.app_id,
                title: window.title,
                pid: window
                    .pid
                    .and_then(|raw| u32::try_from(raw).ok())
                    .and_then(ProcessId::new),
                is_focused: window.is_focused,
                original_tile_index: window
                    .layout
                    .pos_in_scrolling_layout
                    .map(|(_, tile_idx)| tile_idx)
                    .unwrap_or(1),
            })
            .collect())
    }

    fn focus_direction(&mut self, direction: Direction) -> Result<()> {
        self.with_inner(|inner| inner.focus_direction(direction))
    }

    fn move_direction(&mut self, direction: Direction) -> Result<()> {
        self.with_inner(|inner| inner.move_direction(direction))
    }

    fn resize_with_intent(&mut self, intent: ResizeIntent) -> Result<()> {
        self.with_inner(|inner| {
            inner.resize_window(intent.direction, intent.grow(), intent.step.abs().max(1))
        })
    }

    fn spawn(&mut self, command: Vec<String>) -> Result<()> {
        self.with_inner(|inner| inner.spawn(command))
    }

    fn focus_window_by_id(&mut self, id: u64) -> Result<()> {
        self.with_inner(|inner| inner.focus_window_by_id(id))
    }

    fn close_window_by_id(&mut self, id: u64) -> Result<()> {
        self.with_inner(|inner| inner.close_window_by_id(id))
    }
}

#[cfg(any(test, target_os = "linux"))]
impl WindowTearOutComposer for NiriAdapter {
    fn compose_tear_out(&mut self, direction: Direction, source_tile_index: usize) -> Result<()> {
        self.with_inner(|inner| match direction {
            Direction::West | Direction::East => inner.move_column(direction),
            Direction::North | Direction::South => {
                inner.consume_into_column_and_move(direction, source_tile_index)
            }
        })
    }
}

struct UnsupportedWindowManagerSpec {
    backend: WmBackend,
    name: &'static str,
}

impl WindowManagerSpec for UnsupportedWindowManagerSpec {
    fn backend(&self) -> WmBackend {
        self.backend
    }

    fn name(&self) -> &'static str {
        self.name
    }

    fn connect(&self) -> Result<ConfiguredWindowManager> {
        Err(anyhow!(
            "wm backend '{}' is not supported on {}",
            self.name,
            std::env::consts::OS
        ))
    }
}

#[cfg(not(target_os = "linux"))]
static UNSUPPORTED_NIRI_SPEC: UnsupportedWindowManagerSpec = UnsupportedWindowManagerSpec {
    backend: WmBackend::Niri,
    name: "niri",
};
#[cfg(not(target_os = "linux"))]
static UNSUPPORTED_I3_SPEC: UnsupportedWindowManagerSpec = UnsupportedWindowManagerSpec {
    backend: WmBackend::I3,
    name: "i3",
};
#[cfg(not(target_os = "macos"))]
static UNSUPPORTED_PANERU_SPEC: UnsupportedWindowManagerSpec = UnsupportedWindowManagerSpec {
    backend: WmBackend::Paneru,
    name: "paneru",
};
#[cfg(not(target_os = "macos"))]
static UNSUPPORTED_YABAI_SPEC: UnsupportedWindowManagerSpec = UnsupportedWindowManagerSpec {
    backend: WmBackend::Yabai,
    name: "yabai",
};

pub fn spec_for_backend(backend: WmBackend) -> &'static dyn WindowManagerSpec {
    match backend {
        WmBackend::Niri => {
            #[cfg(target_os = "linux")]
            {
                &NIRI_SPEC
            }
            #[cfg(not(target_os = "linux"))]
            {
                &UNSUPPORTED_NIRI_SPEC
            }
        }
        WmBackend::I3 => {
            #[cfg(target_os = "linux")]
            {
                &I3_SPEC
            }
            #[cfg(not(target_os = "linux"))]
            {
                &UNSUPPORTED_I3_SPEC
            }
        }
        WmBackend::Paneru => {
            #[cfg(target_os = "macos")]
            {
                &PANERU_SPEC
            }
            #[cfg(not(target_os = "macos"))]
            {
                &UNSUPPORTED_PANERU_SPEC
            }
        }
        WmBackend::Yabai => {
            #[cfg(target_os = "macos")]
            {
                &YABAI_SPEC
            }
            #[cfg(not(target_os = "macos"))]
            {
                &UNSUPPORTED_YABAI_SPEC
            }
        }
    }
}

fn connect_backend(
    backend: WmBackend,
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
fn connect_backend_for_test(
    backend: WmBackend,
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
        PrimitiveWindowManagerCapabilities, ResizeIntent, WindowCycleProvider,
        WindowManagerCapabilities, WindowManagerCapabilityDescriptor, WindowManagerFeatures,
        WindowManagerSession, WindowManagerSpec,
    };
    use crate::config::WmBackend;
    use crate::engine::topology::Direction;
    use anyhow::Result;
    use std::sync::{Arc, Mutex};

    #[cfg(target_os = "linux")]
    use super::NiriAdapter;
    #[cfg(target_os = "macos")]
    use crate::adapters::window_managers::yabai::YabaiAdapter;

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

    #[cfg(target_os = "macos")]
    #[test]
    fn yabai_capabilities_are_valid() {
        validate_declared_capabilities::<YabaiAdapter>()
            .expect("yabai capability descriptor should be valid");
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
    fn built_in_connectors_are_typed_as_configured_window_managers() {
        fn assert_spec(_spec: &'static dyn WindowManagerSpec) {}

        assert_spec(super::spec_for_backend(WmBackend::Niri));
        assert_spec(super::spec_for_backend(WmBackend::I3));
        assert_spec(super::spec_for_backend(WmBackend::Paneru));
        assert_spec(super::spec_for_backend(WmBackend::Yabai));
        let _ = super::connect_selected as fn() -> Result<ConfiguredWindowManager>;
    }

    #[test]
    fn connect_selected_reports_configured_backend_failure_without_fallback() {
        let err = match connect_backend_for_test(WmBackend::Niri, failing_spec(WmBackend::Niri)) {
            Ok(_) => panic!("configured backend should fail without fallback"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("niri"));
        assert!(!err.to_string().contains("i3"));
    }

    #[test]
    fn failing_spec_uses_requested_backend() {
        let spec = failing_spec(WmBackend::Yabai);

        assert_eq!(spec.backend(), WmBackend::Yabai);
        assert_eq!(spec.name(), "yabai");
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
    }

    impl FakeSession {
        fn new(calls: Arc<Mutex<Vec<String>>>) -> Self {
            Self { calls }
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
            WindowManagerCapabilities::none()
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

        fn windows(&mut self) -> Result<Vec<super::WindowRecord>> {
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
        fn focus_or_cycle(&mut self, _request: &super::WindowCycleRequest) -> Result<()> {
            Ok(())
        }
    }

    fn connect_backend_for_test(
        backend: WmBackend,
        spec: &'static dyn WindowManagerSpec,
    ) -> Result<ConfiguredWindowManager> {
        super::connect_backend_for_test(backend, spec)
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

    #[cfg(target_os = "macos")]
    #[test]
    fn yabai_tear_out_and_resize_plans() {
        let capabilities = YabaiAdapter::CAPABILITIES;
        // Yabai doesn't support tear-out
        assert_eq!(
            plan_tear_out(capabilities, Direction::East),
            CapabilitySupport::Unsupported
        );
        // Yabai has native resize
        assert_eq!(
            plan_resize(capabilities, Direction::West),
            CapabilitySupport::Native
        );
    }
}
