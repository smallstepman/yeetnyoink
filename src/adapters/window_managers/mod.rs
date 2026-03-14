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
#[cfg(any(test, target_os = "linux"))]
use crate::engine::runtime::ProcessId;
use crate::engine::topology::Direction;
pub use crate::engine::window_manager::{
    ConfiguredWindowManager, FocusedWindowRecord, ResizeIntent, ResizeKind, WindowCycleProvider,
    WindowCycleRequest, WindowManagerDomainFactory, WindowManagerFeatures, WindowManagerSession,
    WindowManagerSpec, WindowRecord, WindowTearOutComposer,
};

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
        ConfiguredWindowManager, WindowManagerSpec,
    };
    use crate::config::WmBackend;
    use anyhow::Result;

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
}
