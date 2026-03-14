pub mod i3;
pub mod niri;

use anyhow::{anyhow, Context, Result};

use crate::adapters::window_managers::i3::{I3Adapter, I3FocusedWindow};
use crate::adapters::window_managers::niri::Niri;
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

pub trait FocusedWindowView {
    fn id(&self) -> u64;
    fn app_id(&self) -> Option<&str>;
    fn title(&self) -> Option<&str>;
    fn pid(&self) -> Option<ProcessId>;
    fn original_tile_index(&self) -> usize;
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
    pub fn from_view(window: impl FocusedWindowView) -> Self {
        Self {
            id: window.id(),
            app_id: window.app_id().map(str::to_owned),
            title: window.title().map(str::to_owned),
            pid: window.pid(),
            original_tile_index: window.original_tile_index(),
        }
    }
}

impl FocusedWindowView for FocusedWindowRecord {
    fn id(&self) -> u64 {
        self.id
    }

    fn app_id(&self) -> Option<&str> {
        self.app_id.as_deref()
    }

    fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    fn pid(&self) -> Option<ProcessId> {
        self.pid
    }

    fn original_tile_index(&self) -> usize {
        self.original_tile_index
    }
}

pub trait WindowManagerSession: Send {
    fn adapter_name(&self) -> &'static str;
    fn capabilities(&self) -> WindowManagerCapabilities;
    fn focused_window(&mut self) -> Result<FocusedWindowRecord>;
    fn windows(&mut self) -> Result<Vec<WindowRecord>>;
    fn focus_direction(&mut self, direction: Direction) -> Result<()>;
    fn move_direction(&mut self, direction: Direction) -> Result<()>;
    fn move_column(&mut self, direction: Direction) -> Result<()>;
    fn consume_into_column_and_move(
        &mut self,
        direction: Direction,
        original_tile_index: usize,
    ) -> Result<()>;
    fn resize_with_intent(&mut self, intent: ResizeIntent) -> Result<()>;
    fn spawn(&mut self, command: Vec<String>) -> Result<()>;
    fn focus_window_by_id(&mut self, id: u64) -> Result<()>;
    fn close_window_by_id(&mut self, id: u64) -> Result<()>;
}

impl<T> WindowManagerSession for T
where
    T: WindowManagerAdapter + Send,
{
    fn adapter_name(&self) -> &'static str {
        WindowManagerMetadata::adapter_name(self)
    }

    fn capabilities(&self) -> WindowManagerCapabilities {
        WindowManagerMetadata::capabilities(self)
    }

    fn focused_window(&mut self) -> Result<FocusedWindowRecord> {
        self.with_focused_window(|window| Ok(FocusedWindowRecord::from_view(window)))
    }

    fn windows(&mut self) -> Result<Vec<WindowRecord>> {
        WindowManagerIntrospection::windows(self)
    }

    fn focus_direction(&mut self, direction: Direction) -> Result<()> {
        WindowManagerExecution::focus_direction(self, direction)
    }

    fn move_direction(&mut self, direction: Direction) -> Result<()> {
        WindowManagerExecution::move_direction(self, direction)
    }

    fn move_column(&mut self, direction: Direction) -> Result<()> {
        WindowManagerExecution::move_column(self, direction)
    }

    fn consume_into_column_and_move(
        &mut self,
        direction: Direction,
        original_tile_index: usize,
    ) -> Result<()> {
        WindowManagerExecution::consume_into_column_and_move(self, direction, original_tile_index)
    }

    fn resize_with_intent(&mut self, intent: ResizeIntent) -> Result<()> {
        WindowManagerExecution::resize_with_intent(self, intent)
    }

    fn spawn(&mut self, command: Vec<String>) -> Result<()> {
        WindowManagerExecution::spawn(self, command)
    }

    fn focus_window_by_id(&mut self, id: u64) -> Result<()> {
        WindowManagerExecution::focus_window_by_id(self, id)
    }

    fn close_window_by_id(&mut self, id: u64) -> Result<()> {
        WindowManagerExecution::close_window_by_id(self, id)
    }
}

pub trait WindowManagerDomainFactory: Send {}

pub trait WindowCycleProvider: Send {}

pub trait WindowTearOutComposer: Send {}

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

    pub fn move_column(&mut self, direction: Direction) -> Result<()> {
        self.core.move_column(direction)
    }

    pub fn consume_into_column_and_move(
        &mut self,
        direction: Direction,
        original_tile_index: usize,
    ) -> Result<()> {
        self.core
            .consume_into_column_and_move(direction, original_tile_index)
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

    pub fn tear_out_composer(&self) -> Option<&dyn WindowTearOutComposer> {
        self.features.tear_out_composer.as_deref()
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

    fn move_column(&mut self, direction: Direction) -> Result<()> {
        ConfiguredWindowManager::move_column(self, direction)
    }

    fn consume_into_column_and_move(
        &mut self,
        direction: Direction,
        original_tile_index: usize,
    ) -> Result<()> {
        ConfiguredWindowManager::consume_into_column_and_move(self, direction, original_tile_index)
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

pub trait WindowManagerCapabilityDescriptor {
    const NAME: &'static str;
    const CAPABILITIES: WindowManagerCapabilities;
}

pub fn validate_declared_capabilities<T: WindowManagerCapabilityDescriptor>() -> Result<()> {
    T::CAPABILITIES
        .validate()
        .with_context(|| format!("invalid capabilities for adapter '{}'", T::NAME))
}

pub trait WindowManagerMetadata {
    fn adapter_name(&self) -> &'static str;
    fn capabilities(&self) -> WindowManagerCapabilities;
}

impl<T: WindowManagerCapabilityDescriptor> WindowManagerMetadata for T {
    fn adapter_name(&self) -> &'static str {
        T::NAME
    }

    fn capabilities(&self) -> WindowManagerCapabilities {
        T::CAPABILITIES
    }
}

pub trait WindowManagerIntrospection {
    /// Compile-time guardrail: adapter introspection cannot be implemented
    /// without declaring its GAT window view and required methods.
    ///
    /// ```compile_fail
    /// use yeet_and_yoink::wm::WindowManagerIntrospection;
    ///
    /// struct MissingPieces;
    ///
    /// impl WindowManagerIntrospection for MissingPieces {}
    /// ```
    type FocusedWindow<'a>: FocusedWindowView
    where
        Self: 'a;

    fn with_focused_window<R>(
        &mut self,
        visit: impl for<'a> FnOnce(Self::FocusedWindow<'a>) -> Result<R>,
    ) -> Result<R>;

    fn windows(&mut self) -> Result<Vec<WindowRecord>>;
}

pub trait WindowManagerExecution {
    fn focus_direction(&mut self, direction: Direction) -> Result<()>;
    fn move_direction(&mut self, direction: Direction) -> Result<()>;
    fn move_column(&mut self, direction: Direction) -> Result<()>;
    fn consume_into_column_and_move(
        &mut self,
        direction: Direction,
        original_tile_index: usize,
    ) -> Result<()>;
    fn resize_with_intent(&mut self, intent: ResizeIntent) -> Result<()>;
    fn spawn(&mut self, command: Vec<String>) -> Result<()>;
    fn focus_window_by_id(&mut self, id: u64) -> Result<()>;
    fn close_window_by_id(&mut self, id: u64) -> Result<()>;
}

/// Compile-time guardrail: callers cannot treat a type as an adapter
/// unless all supertraits are implemented.
///
/// ```compile_fail
/// use yeet_and_yoink::wm::WindowManagerAdapter;
///
/// struct NotAnAdapter;
///
/// fn require_adapter<T: WindowManagerAdapter>() {}
///
/// fn main() {
///     require_adapter::<NotAnAdapter>();
/// }
/// ```
pub trait WindowManagerAdapter:
    WindowManagerMetadata + WindowManagerIntrospection + WindowManagerExecution
{
}

impl<T> WindowManagerAdapter for T where
    T: WindowManagerMetadata + WindowManagerIntrospection + WindowManagerExecution
{
}

pub struct NiriAdapter {
    inner: Niri,
}

impl NiriAdapter {
    pub fn connect() -> Result<Self> {
        validate_declared_capabilities::<Self>()?;
        Ok(Self {
            inner: Niri::connect()?,
        })
    }
}

#[derive(Clone, Copy)]
pub struct NiriFocusedWindow<'a> {
    inner: &'a niri_ipc::Window,
}

impl FocusedWindowView for NiriFocusedWindow<'_> {
    fn id(&self) -> u64 {
        self.inner.id
    }

    fn app_id(&self) -> Option<&str> {
        self.inner.app_id.as_deref()
    }

    fn title(&self) -> Option<&str> {
        self.inner.title.as_deref()
    }

    fn pid(&self) -> Option<ProcessId> {
        self.inner
            .pid
            .and_then(|raw| u32::try_from(raw).ok())
            .and_then(ProcessId::new)
    }

    fn original_tile_index(&self) -> usize {
        self.inner
            .layout
            .pos_in_scrolling_layout
            .map(|(_, tile_idx)| tile_idx)
            .unwrap_or(1)
    }
}

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

impl WindowManagerIntrospection for NiriAdapter {
    type FocusedWindow<'a>
        = NiriFocusedWindow<'a>
    where
        Self: 'a;

    fn with_focused_window<R>(
        &mut self,
        visit: impl for<'a> FnOnce(Self::FocusedWindow<'a>) -> Result<R>,
    ) -> Result<R> {
        let window = self.inner.focused_window()?;
        visit(NiriFocusedWindow { inner: &window })
    }

    fn windows(&mut self) -> Result<Vec<WindowRecord>> {
        Ok(self
            .inner
            .windows()?
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
}

impl WindowManagerExecution for NiriAdapter {
    fn focus_direction(&mut self, direction: Direction) -> Result<()> {
        self.inner.focus_direction(direction)
    }

    fn move_direction(&mut self, direction: Direction) -> Result<()> {
        self.inner.move_direction(direction)
    }

    fn move_column(&mut self, direction: Direction) -> Result<()> {
        self.inner.move_column(direction)
    }

    fn consume_into_column_and_move(
        &mut self,
        direction: Direction,
        original_tile_index: usize,
    ) -> Result<()> {
        self.inner
            .consume_into_column_and_move(direction, original_tile_index)
    }

    fn resize_with_intent(&mut self, intent: ResizeIntent) -> Result<()> {
        self.inner
            .resize_window(intent.direction, intent.grow(), intent.step.abs().max(1))
    }

    fn spawn(&mut self, command: Vec<String>) -> Result<()> {
        self.inner.spawn(command)
    }

    fn focus_window_by_id(&mut self, id: u64) -> Result<()> {
        self.inner.focus_window_by_id(id)
    }

    fn close_window_by_id(&mut self, id: u64) -> Result<()> {
        self.inner.close_window_by_id(id)
    }
}

pub struct WindowManagerRegistration {
    pub name: &'static str,
    pub priority: u8,
    pub detector: fn() -> bool,
    pub capabilities: WindowManagerCapabilities,
    pub connect: fn() -> Result<ConfiguredWindowManager>,
}

fn detect_niri() -> bool {
    std::env::var_os("NIRI_SOCKET").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

fn connect_niri() -> Result<ConfiguredWindowManager> {
    Ok(ConfiguredWindowManager::new(
        Box::new(NiriAdapter::connect()?),
        WindowManagerFeatures::default(),
    ))
}

fn detect_i3() -> bool {
    std::env::var_os("I3SOCK").is_some()
        || std::env::var_os("SWAYSOCK").is_some()
        || std::env::var("XDG_CURRENT_DESKTOP")
            .map(|value| {
                let value = value.to_ascii_lowercase();
                value.contains("i3") || value.contains("sway")
            })
            .unwrap_or(false)
}

fn connect_i3() -> Result<ConfiguredWindowManager> {
    Ok(ConfiguredWindowManager::new(
        Box::new(I3Adapter::connect()?),
        WindowManagerFeatures::default(),
    ))
}

const REGISTRY: &[WindowManagerRegistration] = &[
    WindowManagerRegistration {
        name: NiriAdapter::NAME,
        priority: 100,
        detector: detect_niri,
        capabilities: NiriAdapter::CAPABILITIES,
        connect: connect_niri,
    },
    WindowManagerRegistration {
        name: I3Adapter::NAME,
        priority: 90,
        detector: detect_i3,
        capabilities: I3Adapter::CAPABILITIES,
        connect: connect_i3,
    },
];

fn preferred_window_manager_name() -> Option<String> {
    crate::config::wm_adapter_override().and_then(|raw| {
        let normalized = raw.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            None
        } else {
            Some(normalized)
        }
    })
}

pub fn connect_selected() -> Result<ConfiguredWindowManager> {
    let _span = tracing::debug_span!("window_managers.connect_selected").entered();
    let preferred = preferred_window_manager_name();

    if let Some(preferred) = preferred.as_deref() {
        if let Some(registration) = REGISTRY.iter().find(|item| item.name == preferred) {
            registration.capabilities.validate().with_context(|| {
                format!("invalid wm capability declaration for '{}'", preferred)
            })?;
            return (registration.connect)();
        }
        return Err(anyhow!(
            "unknown window-manager adapter override '{}'",
            preferred
        ));
    }

    let registration = REGISTRY
        .iter()
        .filter(|item| (item.detector)())
        .max_by_key(|item| item.priority)
        .or_else(|| REGISTRY.first())
        .context("no window-manager adapters are registered")?;

    registration.capabilities.validate().with_context(|| {
        format!(
            "invalid wm capability declaration for '{}'",
            registration.name
        )
    })?;
    (registration.connect)()
}

pub enum SelectedWindowManager {
    Niri(NiriAdapter),
    I3(I3Adapter),
}

#[derive(Clone, Copy)]
pub enum SelectedFocusedWindow<'a> {
    Niri(NiriFocusedWindow<'a>),
    I3(I3FocusedWindow<'a>),
}

impl FocusedWindowView for SelectedFocusedWindow<'_> {
    fn id(&self) -> u64 {
        match self {
            Self::Niri(inner) => inner.id(),
            Self::I3(inner) => inner.id(),
        }
    }

    fn app_id(&self) -> Option<&str> {
        match self {
            Self::Niri(inner) => inner.app_id(),
            Self::I3(inner) => inner.app_id(),
        }
    }

    fn title(&self) -> Option<&str> {
        match self {
            Self::Niri(inner) => inner.title(),
            Self::I3(inner) => inner.title(),
        }
    }

    fn pid(&self) -> Option<ProcessId> {
        match self {
            Self::Niri(inner) => inner.pid(),
            Self::I3(inner) => inner.pid(),
        }
    }

    fn original_tile_index(&self) -> usize {
        match self {
            Self::Niri(inner) => inner.original_tile_index(),
            Self::I3(inner) => inner.original_tile_index(),
        }
    }
}

impl WindowManagerMetadata for SelectedWindowManager {
    fn adapter_name(&self) -> &'static str {
        match self {
            Self::Niri(inner) => WindowManagerMetadata::adapter_name(inner),
            Self::I3(inner) => WindowManagerMetadata::adapter_name(inner),
        }
    }

    fn capabilities(&self) -> WindowManagerCapabilities {
        match self {
            Self::Niri(inner) => WindowManagerMetadata::capabilities(inner),
            Self::I3(inner) => WindowManagerMetadata::capabilities(inner),
        }
    }
}

impl WindowManagerIntrospection for SelectedWindowManager {
    type FocusedWindow<'a>
        = SelectedFocusedWindow<'a>
    where
        Self: 'a;

    fn with_focused_window<R>(
        &mut self,
        visit: impl for<'a> FnOnce(Self::FocusedWindow<'a>) -> Result<R>,
    ) -> Result<R> {
        match self {
            Self::Niri(inner) => {
                inner.with_focused_window(|window| visit(SelectedFocusedWindow::Niri(window)))
            }
            Self::I3(inner) => {
                inner.with_focused_window(|window| visit(SelectedFocusedWindow::I3(window)))
            }
        }
    }

    fn windows(&mut self) -> Result<Vec<WindowRecord>> {
        match self {
            Self::Niri(inner) => WindowManagerIntrospection::windows(inner),
            Self::I3(inner) => WindowManagerIntrospection::windows(inner),
        }
    }
}

impl WindowManagerExecution for SelectedWindowManager {
    fn focus_direction(&mut self, direction: Direction) -> Result<()> {
        match self {
            Self::Niri(inner) => WindowManagerExecution::focus_direction(inner, direction),
            Self::I3(inner) => WindowManagerExecution::focus_direction(inner, direction),
        }
    }

    fn move_direction(&mut self, direction: Direction) -> Result<()> {
        match self {
            Self::Niri(inner) => WindowManagerExecution::move_direction(inner, direction),
            Self::I3(inner) => WindowManagerExecution::move_direction(inner, direction),
        }
    }

    fn move_column(&mut self, direction: Direction) -> Result<()> {
        match self {
            Self::Niri(inner) => WindowManagerExecution::move_column(inner, direction),
            Self::I3(inner) => WindowManagerExecution::move_column(inner, direction),
        }
    }

    fn consume_into_column_and_move(
        &mut self,
        direction: Direction,
        original_tile_index: usize,
    ) -> Result<()> {
        match self {
            Self::Niri(inner) => WindowManagerExecution::consume_into_column_and_move(
                inner,
                direction,
                original_tile_index,
            ),
            Self::I3(inner) => WindowManagerExecution::consume_into_column_and_move(
                inner,
                direction,
                original_tile_index,
            ),
        }
    }

    fn resize_with_intent(&mut self, intent: ResizeIntent) -> Result<()> {
        match self {
            Self::Niri(inner) => WindowManagerExecution::resize_with_intent(inner, intent),
            Self::I3(inner) => WindowManagerExecution::resize_with_intent(inner, intent),
        }
    }

    fn spawn(&mut self, command: Vec<String>) -> Result<()> {
        match self {
            Self::Niri(inner) => WindowManagerExecution::spawn(inner, command),
            Self::I3(inner) => WindowManagerExecution::spawn(inner, command),
        }
    }

    fn focus_window_by_id(&mut self, id: u64) -> Result<()> {
        match self {
            Self::Niri(inner) => WindowManagerExecution::focus_window_by_id(inner, id),
            Self::I3(inner) => WindowManagerExecution::focus_window_by_id(inner, id),
        }
    }

    fn close_window_by_id(&mut self, id: u64) -> Result<()> {
        match self {
            Self::Niri(inner) => WindowManagerExecution::close_window_by_id(inner, id),
            Self::I3(inner) => WindowManagerExecution::close_window_by_id(inner, id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        plan_resize, plan_tear_out, validate_declared_capabilities, CapabilitySupport,
        ConfiguredWindowManager, DirectionalCapability, FocusedWindowRecord, NiriAdapter,
        PrimitiveWindowManagerCapabilities, ResizeIntent, WindowCycleProvider,
        WindowManagerCapabilities, WindowManagerCapabilityDescriptor, WindowManagerFeatures,
        WindowManagerSession,
    };
    use crate::engine::topology::Direction;
    use anyhow::Result;
    use std::sync::{Arc, Mutex};

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

    #[test]
    fn niri_capabilities_are_valid() {
        validate_declared_capabilities::<NiriAdapter>()
            .expect("niri capability descriptor should be valid");
    }

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
        fn assert_connector(_connect: fn() -> Result<ConfiguredWindowManager>) {}

        assert_connector(super::connect_niri);
        assert_connector(super::connect_i3);
        let _ = super::connect_selected as fn() -> Result<ConfiguredWindowManager>;
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

        fn move_column(&mut self, _direction: Direction) -> Result<()> {
            Ok(())
        }

        fn consume_into_column_and_move(
            &mut self,
            _direction: Direction,
            _original_tile_index: usize,
        ) -> Result<()> {
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

    impl WindowCycleProvider for FakeCycleProvider {}
}
