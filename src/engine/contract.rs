use std::any::Any;

use anyhow::{anyhow, Result};

use crate::engine::runtime::ProcessId;
use crate::engine::topology::{Direction, DomainId};

pub trait ChainResolver {
    fn resolve_chain(&self, app_id: &str, pid: u32, title: &str) -> Vec<Box<dyn AppAdapter>>;
    fn default_domain_adapters(&self) -> Vec<Box<dyn AppAdapter>>;
    fn domain_id_for_window(
        &self,
        app_id: Option<&str>,
        pid: Option<ProcessId>,
        title: Option<&str>,
    ) -> DomainId;
}

pub trait TerminalMultiplexerProvider: TopologyHandler + ChainResolver {
    /// Capabilities this mux backend supports (pane focus, move, resize, etc).
    fn capabilities(&self) -> AdapterCapabilities;
    fn focused_pane_for_pid(&self, pid: u32) -> Result<u64>;
    fn pane_neighbor_for_pid(&self, pid: u32, pane_id: u64, dir: Direction) -> Result<u64>;
    fn send_text_to_pane(&self, pid: u32, pane_id: u64, text: &str) -> Result<()>;
    /// Returns the mux-specific attach arguments (e.g. `["tmux", "attach", "-t", target]`),
    /// or `None` if the mux manages windows directly (built-in mux).
    /// The terminal host composes these with its own launch prefix.
    fn mux_attach_args(&self, target: String) -> Option<Vec<String>>;
    fn merge_source_pane_into_focused_target(
        &self,
        source_pid: u32,
        source_pane_id: u64,
        target_pid: u32,
        target_window_id: Option<u64>,
        dir: Direction,
    ) -> Result<()>;
    fn active_foreground_process(&self, pid: u32) -> Option<String>;
}
/// What the app wants to do for a move operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveDecision {
    /// Swap/move internally within the app.
    Internal,
    /// Rearrange panes: no neighbor in move direction, but panes exist
    /// in other directions. Reorganize layout (e.g. horizontal → vertical).
    Rearrange,
    /// At the edge with multiple splits along the move axis — tear the buffer out.
    TearOut,
    /// Nothing to do internally, fall through to the compositor.
    Passthrough,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppKind {
    Browser,
    Editor,
    Terminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeExecutionMode {
    SourceFocused,
    TargetFocused,
}

pub struct MergePreparation {
    payload: Option<Box<dyn Any + Send>>,
}

impl MergePreparation {
    pub fn none() -> Self {
        Self { payload: None }
    }

    pub fn with_payload<T>(payload: T) -> Self
    where
        T: Send + 'static,
    {
        Self {
            payload: Some(Box::new(payload)),
        }
    }

    pub fn into_payload<T>(self) -> Option<T>
    where
        T: Send + 'static,
    {
        self.payload
            .and_then(|payload| payload.downcast::<T>().ok())
            .map(|typed| *typed)
    }

    pub fn map_payload<T>(self, update: impl FnOnce(T) -> T) -> Self
    where
        T: Send + 'static,
    {
        let Some(payload) = self.payload else {
            return self;
        };
        match payload.downcast::<T>() {
            Ok(typed) => Self::with_payload(update(*typed)),
            Err(payload) => Self {
                payload: Some(payload),
            },
        }
    }
}

impl Default for MergePreparation {
    fn default() -> Self {
        Self::none()
    }
}

/// Result of tearing a buffer/pane out of an app.
pub struct TearResult {
    /// Command to spawn the torn-out content as a new window.
    /// None if the app already created the window itself.
    pub spawn_command: Option<Vec<String>>,
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
}

pub type AdapterCapabilities = AppCapabilities;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TopologySnapshot {
    /// Directions where the focused pane has in-app neighbors.
    pub internal_neighbors: Vec<Direction>,
    /// Directions where leaving the app domain is possible/expected.
    pub cross_domain_neighbors: Vec<Direction>,
}

/// Unified query + mutation contract for app topology.
pub trait TopologyHandler {
    /// Snapshot of the current in-app and cross-domain neighbor surface.
    fn topology_snapshot(&self, pid: u32) -> Result<TopologySnapshot> {
        let mut snapshot = TopologySnapshot::default();
        for direction in [
            Direction::West,
            Direction::East,
            Direction::North,
            Direction::South,
        ] {
            if self.can_focus(direction, pid)? {
                snapshot.internal_neighbors.push(direction);
                continue;
            }
            match self.move_decision(direction, pid)? {
                MoveDecision::Passthrough | MoveDecision::TearOut => {
                    snapshot.cross_domain_neighbors.push(direction);
                }
                MoveDecision::Internal | MoveDecision::Rearrange => {}
            }
        }
        Ok(snapshot)
    }

    fn can_focus(&self, dir: Direction, pid: u32) -> Result<bool>;
    fn focus(&self, dir: Direction, pid: u32) -> Result<()>;
    fn move_decision(&self, dir: Direction, pid: u32) -> Result<MoveDecision>;
    fn move_internal(&self, dir: Direction, pid: u32) -> Result<()>;

    /// Optional primitive for "edge in a direction" queries.
    fn at_side(&self, dir: Direction, pid: u32) -> Result<bool> {
        Ok(!self.can_focus(dir, pid)?)
    }

    /// Optional primitive for visible pane/window count in the app scope.
    fn window_count(&self, _pid: u32) -> Result<u32> {
        Ok(1)
    }

    fn can_resize(&self, _dir: Direction, _grow: bool, _pid: u32) -> Result<bool> {
        Ok(false)
    }

    fn resize_internal(&self, _dir: Direction, _grow: bool, _step: i32, _pid: u32) -> Result<()> {
        Err(unsupported_operation(
            std::any::type_name::<Self>(),
            "resize_internal",
        ))
    }

    fn rearrange(&self, _dir: Direction, _pid: u32) -> Result<()> {
        Err(unsupported_operation(
            std::any::type_name::<Self>(),
            "rearrange",
        ))
    }

    fn move_out(&self, dir: Direction, pid: u32) -> Result<TearResult>;

    fn merge_into(&self, _dir: Direction, _source_pid: u32) -> Result<()> {
        Err(unsupported_operation(
            std::any::type_name::<Self>(),
            "merge_into",
        ))
    }

    fn merge_execution_mode(&self) -> MergeExecutionMode {
        MergeExecutionMode::SourceFocused
    }

    fn prepare_merge(&self, _source_pid: Option<ProcessId>) -> Result<MergePreparation> {
        Ok(MergePreparation::none())
    }

    fn augment_merge_preparation_for_target(
        &self,
        preparation: MergePreparation,
        _target_window_id: Option<u64>,
    ) -> MergePreparation {
        preparation
    }

    fn merge_into_target(
        &self,
        dir: Direction,
        source_pid: Option<ProcessId>,
        _target_pid: Option<ProcessId>,
        _preparation: MergePreparation,
    ) -> Result<()> {
        self.merge_into(dir, legacy_pid(source_pid))
    }
}

/// Metadata/capabilities contract for app adapters.
pub trait AppAdapter: Send + TopologyHandler + ChainResolver {
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

impl<T> ChainResolver for T
where
    T: TopologyHandler + ?Sized,
{
    fn resolve_chain(&self, app_id: &str, pid: u32, title: &str) -> Vec<Box<dyn AppAdapter>> {
        crate::engine::chain_resolver::runtime_chain_resolver().resolve_chain(app_id, pid, title)
    }

    fn default_domain_adapters(&self) -> Vec<Box<dyn AppAdapter>> {
        crate::engine::chain_resolver::runtime_chain_resolver().default_domain_adapters()
    }

    fn domain_id_for_window(
        &self,
        app_id: Option<&str>,
        pid: Option<ProcessId>,
        title: Option<&str>,
    ) -> DomainId {
        crate::engine::chain_resolver::runtime_chain_resolver()
            .domain_id_for_window(app_id, pid, title)
    }
}

pub fn unsupported_operation(adapter: &str, operation: &str) -> anyhow::Error {
    anyhow!(
        "adapter '{}' does not support operation '{}'",
        adapter,
        operation
    )
}

fn legacy_pid(pid: Option<ProcessId>) -> u32 {
    pid.map(ProcessId::get).unwrap_or(0)
}
