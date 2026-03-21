use anyhow::{anyhow, Context, Result};

use crate::config::WmBackend;
use crate::engine::topology::Direction;
use crate::engine::wm::capabilities::{
    plan_tear_out, CapabilitySupport, WindowManagerCapabilities,
};
use crate::engine::wm::session::{
    FocusedWindowRecord, ResizeIntent, WindowCycleProvider, WindowManagerDomainFactory,
    WindowManagerSession, WindowRecord, WindowTearOutComposer,
};

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

// ---------------------------------------------------------------------------
// UnsupportedWindowManagerSpec — moved from adapters::window_managers
// ---------------------------------------------------------------------------

pub(crate) struct UnsupportedWindowManagerSpec {
    pub(crate) backend: WmBackend,
    pub(crate) name: &'static str,
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
pub(crate) static UNSUPPORTED_NIRI_SPEC: UnsupportedWindowManagerSpec =
    UnsupportedWindowManagerSpec {
        backend: WmBackend::Niri,
        name: "niri",
    };
#[cfg(not(target_os = "linux"))]
pub(crate) static UNSUPPORTED_I3_SPEC: UnsupportedWindowManagerSpec =
    UnsupportedWindowManagerSpec {
        backend: WmBackend::I3,
        name: "i3",
    };
#[cfg(not(target_os = "linux"))]
pub(crate) static UNSUPPORTED_HYPRLAND_SPEC: UnsupportedWindowManagerSpec =
    UnsupportedWindowManagerSpec {
        backend: WmBackend::Hyprland,
        name: "hyprland",
    };
#[cfg(not(target_os = "macos"))]
pub(crate) static UNSUPPORTED_PANERU_SPEC: UnsupportedWindowManagerSpec =
    UnsupportedWindowManagerSpec {
        backend: WmBackend::Paneru,
        name: "paneru",
    };
#[cfg(not(target_os = "macos"))]
pub(crate) static UNSUPPORTED_YABAI_SPEC: UnsupportedWindowManagerSpec =
    UnsupportedWindowManagerSpec {
        backend: WmBackend::Yabai,
        name: "yabai",
    };
