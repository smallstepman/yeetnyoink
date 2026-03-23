use anyhow::Result;

pub mod mmsg;
pub mod toplevel;

use crate::config::WmBackend;
use crate::engine::topology::Direction;
use crate::engine::wm::{
    validate_declared_capabilities, CapabilitySupport, ConfiguredWindowManager,
    DirectionalCapability, FocusedWindowRecord, PrimitiveWindowManagerCapabilities, ResizeIntent,
    WindowManagerCapabilities, WindowManagerCapabilityDescriptor, WindowManagerSession,
    WindowManagerSpec, WindowRecord,
};

pub struct MangowcAdapter;

pub struct MangowcSpec;

pub static MANGOWC_SPEC: MangowcSpec = MangowcSpec;

impl WindowManagerSpec for MangowcSpec {
    fn backend(&self) -> WmBackend {
        WmBackend::Mangowc
    }

    fn name(&self) -> &'static str {
        MangowcAdapter::NAME
    }

    fn connect(&self) -> Result<ConfiguredWindowManager> {
        anyhow::bail!("wm backend 'mangowc' is not yet supported at runtime")
    }
}

impl MangowcAdapter {
    pub fn connect() -> Result<Self> {
        validate_declared_capabilities::<Self>()?;
        Ok(Self)
    }
}

impl WindowManagerCapabilityDescriptor for MangowcAdapter {
    const NAME: &'static str = "mangowc";
    const CAPABILITIES: WindowManagerCapabilities = WindowManagerCapabilities {
        primitives: PrimitiveWindowManagerCapabilities {
            tear_out_right: false,
            move_column: false,
            consume_into_column_and_move: false,
            set_window_width: false,
            set_window_height: false,
        },
        tear_out: DirectionalCapability::uniform(CapabilitySupport::Unsupported),
        resize: DirectionalCapability::uniform(CapabilitySupport::Unsupported),
    };
}

impl WindowManagerSession for MangowcAdapter {
    fn adapter_name(&self) -> &'static str {
        Self::NAME
    }

    fn capabilities(&self) -> WindowManagerCapabilities {
        Self::CAPABILITIES
    }

    fn focused_window(&mut self) -> Result<FocusedWindowRecord> {
        anyhow::bail!("mangowc adapter not implemented")
    }

    fn windows(&mut self) -> Result<Vec<WindowRecord>> {
        anyhow::bail!("mangowc adapter not implemented")
    }

    fn focus_direction(&mut self, _direction: Direction) -> Result<()> {
        anyhow::bail!("mangowc adapter not implemented")
    }

    fn move_direction(&mut self, _direction: Direction) -> Result<()> {
        anyhow::bail!("mangowc adapter not implemented")
    }

    fn resize_with_intent(&mut self, _intent: ResizeIntent) -> Result<()> {
        anyhow::bail!("mangowc adapter not implemented")
    }

    fn spawn(&mut self, _command: Vec<String>) -> Result<()> {
        anyhow::bail!("mangowc adapter not implemented")
    }

    fn focus_window_by_id(&mut self, _id: u64) -> Result<()> {
        anyhow::bail!("mangowc adapter not implemented")
    }

    fn close_window_by_id(&mut self, _id: u64) -> Result<()> {
        anyhow::bail!("mangowc adapter not implemented")
    }
}
