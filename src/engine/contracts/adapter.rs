use anyhow::Result;

use crate::engine::runtime::ProcessId;

use super::topology::TopologyHandler;

pub fn unsupported_operation(adapter: &str, operation: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "adapter '{}' does not support operation '{}'",
        adapter,
        operation
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppKind {
    Browser,
    Editor,
    Terminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppCapabilities {
    pub probe: bool,
    pub focus: bool,
    pub move_internal: bool,
    pub resize_internal: bool,
    pub rearrange: bool,
    pub tear_out: bool,
    pub merge: bool,
}

impl AppCapabilities {
    pub const fn none() -> Self {
        Self {
            probe: false,
            focus: false,
            move_internal: false,
            resize_internal: false,
            rearrange: false,
            tear_out: false,
            merge: false,
        }
    }

    pub const fn terminal_mux_defaults() -> Self {
        Self {
            probe: true,
            focus: true,
            move_internal: true,
            resize_internal: false,
            rearrange: false,
            tear_out: true,
            merge: true,
        }
    }

    pub const fn with_resize_internal(mut self, resize_internal: bool) -> Self {
        self.resize_internal = resize_internal;
        self
    }

    pub const fn with_rearrange(mut self, rearrange: bool) -> Self {
        self.rearrange = rearrange;
        self
    }

    pub const fn with_merge(mut self, merge: bool) -> Self {
        self.merge = merge;
        self
    }
}

pub type AdapterCapabilities = AppCapabilities;

/// Metadata/capabilities contract for app adapters.
pub trait AppAdapter: Send + TopologyHandler {
    /// Human-readable adapter name used in diagnostics.
    fn adapter_name(&self) -> &'static str;

    /// Optional config aliases used to bind policy for this adapter.
    fn config_aliases(&self) -> Option<&'static [&'static str]> {
        None
    }

    /// High-level app category used by domain resolution policy.
    fn kind(&self) -> AppKind;

    /// Explicit capability declaration used by orchestrator routing.
    fn capabilities(&self) -> AdapterCapabilities;

    /// Optional adapter-native expression evaluator.
    fn eval(&self, _expression: &str, _pid: Option<ProcessId>) -> Result<String> {
        Err(unsupported_operation(self.adapter_name(), "eval"))
    }
}
