use std::any::Any;

use anyhow::{anyhow, Context, Result};
use serde::de::DeserializeOwned;

use crate::engine::runtime::{self, ProcessId};
use crate::engine::topology::{Direction, DirectionalNeighbors, DomainId, MoveSurface};

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
    fn cli_output_for_pid(&self, _pid: u32, _args: &[&str]) -> Result<std::process::Output> {
        Err(unsupported_operation(
            std::any::type_name::<Self>(),
            "cli_output_for_pid",
        ))
    }

    fn command_error_for_pid(&self, _pid: u32, args: &[&str], stderr: &str) -> anyhow::Error {
        anyhow!(
            "terminal multiplexer command {:?} failed: {}",
            args,
            stderr.trim()
        )
    }

    fn cli_status_for_pid(&self, pid: u32, args: &[&str]) -> Result<()> {
        let output = self.cli_output_for_pid(pid, args)?;
        if !output.status.success() {
            let stderr = runtime::stderr_text(&output);
            return Err(self.command_error_for_pid(pid, args, &stderr));
        }
        Ok(())
    }

    fn cli_stdout_for_pid(&self, pid: u32, args: &[&str]) -> Result<String> {
        let output = self.cli_output_for_pid(pid, args)?;
        if !output.status.success() {
            let stderr = runtime::stderr_text(&output);
            return Err(self.command_error_for_pid(pid, args, &stderr));
        }
        Ok(runtime::stdout_text(&output))
    }

    fn cli_json_for_pid<T>(&self, pid: u32, args: &[&str], parse_context: &'static str) -> Result<T>
    where
        Self: Sized,
        T: DeserializeOwned,
    {
        let stdout = self.cli_stdout_for_pid(pid, args)?;
        serde_json::from_str(&stdout).context(parse_context)
    }

    fn list_panes_for_pid(&self, _pid: u32) -> Result<Vec<TerminalPaneSnapshot>> {
        Err(unsupported_operation(
            std::any::type_name::<Self>(),
            "list_panes_for_pid",
        ))
    }

    /// Capabilities this mux backend supports (pane focus, move, resize, etc).
    fn capabilities(&self) -> AdapterCapabilities;
    fn focused_pane_for_pid(&self, pid: u32) -> Result<u64>;

    fn pane_in_direction_for_pid(
        &self,
        _pid: u32,
        _pane_id: u64,
        _dir: Direction,
    ) -> Result<Option<u64>> {
        Err(unsupported_operation(
            std::any::type_name::<Self>(),
            "pane_in_direction_for_pid",
        ))
    }

    fn pane_neighbor_for_pid(&self, pid: u32, pane_id: u64, dir: Direction) -> Result<u64> {
        self.pane_in_direction_for_pid(pid, pane_id, dir)?
            .context("no terminal multiplexer pane exists in requested direction")
    }

    fn directional_neighbors_from_pane_lookup(&self, pid: u32) -> Result<DirectionalNeighbors> {
        let pane_id = self.focused_pane_for_pid(pid)?;
        let mut neighbors = DirectionalNeighbors::default();
        for direction in Direction::ALL {
            neighbors.set(
                direction,
                self.pane_in_direction_for_pid(pid, pane_id, direction)?
                    .is_some(),
            );
        }
        Ok(neighbors)
    }

    fn can_focus_from_pane_lookup(&self, dir: Direction, pid: u32) -> Result<bool> {
        let pane_id = self.focused_pane_for_pid(pid)?;
        Ok(self.pane_in_direction_for_pid(pid, pane_id, dir)?.is_some())
    }

    fn axis_neighbors_exist_from_pane_lookup(&self, pid: u32, dir: Direction) -> Result<bool> {
        let pane_id = self.focused_pane_for_pid(pid)?;
        let [first, second] = dir.axis_directions();
        Ok(self
            .pane_in_direction_for_pid(pid, pane_id, first)?
            .is_some()
            || self
                .pane_in_direction_for_pid(pid, pane_id, second)?
                .is_some())
    }

    fn perpendicular_pane_for_pid(
        &self,
        pid: u32,
        pane_id: u64,
        dir: Direction,
    ) -> Result<Option<u64>> {
        let [first, second] = dir.perpendicular_directions();
        Ok(self
            .pane_in_direction_for_pid(pid, pane_id, first)?
            .or(self.pane_in_direction_for_pid(pid, pane_id, second)?))
    }

    fn move_decision_from_pane_lookup(
        &self,
        dir: Direction,
        pid: u32,
        supports_rearrange: bool,
    ) -> Result<MoveDecision> {
        let pane_count = self.active_scope_pane_count_for_pid(pid)?;
        if pane_count <= 1 {
            return Ok(MoveDecision::Passthrough);
        }
        let pane_id = self.focused_pane_for_pid(pid)?;
        if self.pane_in_direction_for_pid(pid, pane_id, dir)?.is_some() {
            return Ok(MoveDecision::Internal);
        }
        if supports_rearrange
            && self
                .perpendicular_pane_for_pid(pid, pane_id, dir)?
                .is_some()
        {
            return Ok(MoveDecision::Rearrange);
        }
        Ok(MoveDecision::TearOut)
    }

    fn focused_pane_from_snapshots(
        &self,
        panes: &[TerminalPaneSnapshot],
        missing_pane_message: &'static str,
    ) -> Result<u64> {
        TerminalPaneSnapshot::active_or_first(panes.iter())
            .map(|pane| pane.pane_id)
            .context(missing_pane_message)
    }

    fn active_scope_panes_for_pid(&self, pid: u32) -> Result<Vec<TerminalPaneSnapshot>> {
        let panes = self.list_panes_for_pid(pid)?;
        let focused_pane_id = self.focused_pane_for_pid(pid)?;
        let focused = panes
            .iter()
            .find(|pane| pane.pane_id == focused_pane_id)
            .context("focused pane is not present in terminal pane list")?;
        let active_tab_id = focused.tab_id;
        let active_window_id = focused.window_id;
        Ok(panes
            .into_iter()
            .filter(|pane| {
                if let Some(tab_id) = active_tab_id {
                    pane.tab_id == Some(tab_id)
                } else if let Some(window_id) = active_window_id {
                    pane.window_id == Some(window_id)
                } else {
                    true
                }
            })
            .collect())
    }

    fn active_scope_pane_count_for_pid(&self, pid: u32) -> Result<u32> {
        Ok(self.active_scope_panes_for_pid(pid)?.len() as u32)
    }

    fn active_foreground_process_from_snapshots(&self, pid: u32) -> Option<String> {
        if pid == 0 {
            return None;
        }
        let pane_id = self.focused_pane_for_pid(pid).ok()?;
        let panes = self.active_scope_panes_for_pid(pid).ok()?;
        panes
            .into_iter()
            .find(|pane| pane.pane_id == pane_id)
            .and_then(|pane| pane.foreground_process_name)
            .and_then(|name| {
                let normalized = name.trim().to_string();
                (!normalized.is_empty()).then_some(normalized)
            })
    }

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
    fn active_foreground_process(&self, pid: u32) -> Option<String> {
        self.active_foreground_process_from_snapshots(pid)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TerminalPaneSnapshot {
    pub pane_id: u64,
    pub tab_id: Option<u64>,
    pub window_id: Option<u64>,
    pub is_active: bool,
    pub foreground_process_name: Option<String>,
}

impl TerminalPaneSnapshot {
    pub fn active_or_first<'a>(
        panes: impl IntoIterator<Item = &'a TerminalPaneSnapshot>,
    ) -> Option<&'a TerminalPaneSnapshot> {
        let mut panes: Vec<_> = panes.into_iter().collect();
        panes.sort_by_key(|pane| pane.pane_id);
        panes
            .iter()
            .copied()
            .find(|pane| pane.is_active)
            .or_else(|| panes.first().copied())
    }

    pub fn unique_ids<'a>(panes: impl IntoIterator<Item = &'a TerminalPaneSnapshot>) -> Vec<u64> {
        let mut pane_ids: Vec<u64> = panes.into_iter().map(|pane| pane.pane_id).collect();
        pane_ids.sort_unstable();
        pane_ids.dedup();
        pane_ids
    }
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

#[derive(Debug, Clone)]
pub struct SourcePaneMerge<Meta = ()> {
    pub pane_id: u64,
    pub meta: Meta,
}

impl<Meta> SourcePaneMerge<Meta> {
    pub fn new(pane_id: u64, meta: Meta) -> Self {
        Self { pane_id, meta }
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

    fn can_focus_from_directional_neighbors(&self, dir: Direction, pid: u32) -> Result<bool> {
        Ok(self.directional_neighbors(pid)?.in_direction(dir))
    }

    fn focus(&self, dir: Direction, pid: u32) -> Result<()>;
    fn move_decision(&self, dir: Direction, pid: u32) -> Result<MoveDecision> {
        Ok(self.move_surface(pid)?.decision_for(dir))
    }
    fn move_internal(&self, dir: Direction, pid: u32) -> Result<()>;

    fn directional_neighbors(&self, pid: u32) -> Result<DirectionalNeighbors> {
        let mut neighbors = DirectionalNeighbors::default();
        for direction in Direction::ALL {
            neighbors.set(direction, self.can_focus(direction, pid)?);
        }
        Ok(neighbors)
    }

    fn supports_rearrange_decision(&self) -> bool {
        true
    }

    fn move_surface(&self, pid: u32) -> Result<MoveSurface> {
        Ok(MoveSurface {
            pane_count: self.window_count(pid)?,
            neighbors: self.directional_neighbors(pid)?,
            supports_rearrange: self.supports_rearrange_decision(),
        })
    }

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

    fn prepare_merge_payload<T>(
        &self,
        source_pid: Option<ProcessId>,
        missing_source_pid: &'static str,
        capture: impl FnOnce(u32) -> Result<T>,
    ) -> Result<MergePreparation>
    where
        Self: Sized,
        T: Send + 'static,
    {
        let source_pid = source_pid.context(missing_source_pid)?;
        Ok(MergePreparation::with_payload(capture(source_pid.get())?))
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

    fn resolve_target_focused_merge<T>(
        &self,
        source_pid: Option<ProcessId>,
        target_pid: Option<ProcessId>,
        preparation: MergePreparation,
        missing_source_pid: &'static str,
        missing_target_pid: &'static str,
        missing_preparation: &'static str,
    ) -> Result<(u32, u32, T)>
    where
        Self: Sized,
        T: Send + 'static,
    {
        let source_pid = source_pid.context(missing_source_pid)?;
        let target_pid = target_pid.context(missing_target_pid)?;
        let preparation = preparation
            .into_payload::<T>()
            .context(missing_preparation)?;
        Ok((source_pid.get(), target_pid.get(), preparation))
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

#[cfg(test)]
mod tests {
    use super::{
        AdapterCapabilities, MoveDecision, TearResult, TerminalMultiplexerProvider,
        TerminalPaneSnapshot, TopologyHandler,
    };
    use crate::engine::topology::{Direction, DirectionalNeighbors};

    struct SharedDecisionAdapter;

    impl TopologyHandler for SharedDecisionAdapter {
        fn directional_neighbors(&self, _pid: u32) -> anyhow::Result<DirectionalNeighbors> {
            Ok(DirectionalNeighbors {
                west: false,
                east: false,
                north: true,
                south: false,
            })
        }

        fn window_count(&self, _pid: u32) -> anyhow::Result<u32> {
            Ok(2)
        }

        fn can_focus(&self, _dir: Direction, _pid: u32) -> anyhow::Result<bool> {
            Ok(false)
        }

        fn focus(&self, _dir: Direction, _pid: u32) -> anyhow::Result<()> {
            Ok(())
        }

        fn move_internal(&self, _dir: Direction, _pid: u32) -> anyhow::Result<()> {
            Ok(())
        }

        fn move_out(&self, _dir: Direction, _pid: u32) -> anyhow::Result<TearResult> {
            Ok(TearResult {
                spawn_command: None,
            })
        }
    }

    #[test]
    fn topology_handler_move_surface_classifies_decisions() {
        let adapter = SharedDecisionAdapter;
        assert!(matches!(
            adapter.move_decision(Direction::West, 0).expect("decision"),
            MoveDecision::Rearrange
        ));
    }

    struct SnapshotMux;

    impl TopologyHandler for SnapshotMux {
        fn directional_neighbors(&self, _pid: u32) -> anyhow::Result<DirectionalNeighbors> {
            Ok(DirectionalNeighbors {
                west: true,
                east: false,
                north: false,
                south: false,
            })
        }

        fn can_focus(&self, dir: Direction, pid: u32) -> anyhow::Result<bool> {
            self.can_focus_from_directional_neighbors(dir, pid)
        }

        fn focus(&self, _dir: Direction, _pid: u32) -> anyhow::Result<()> {
            Ok(())
        }

        fn move_internal(&self, _dir: Direction, _pid: u32) -> anyhow::Result<()> {
            Ok(())
        }

        fn move_out(&self, _dir: Direction, _pid: u32) -> anyhow::Result<TearResult> {
            Ok(TearResult {
                spawn_command: None,
            })
        }
    }

    impl TerminalMultiplexerProvider for SnapshotMux {
        fn capabilities(&self) -> AdapterCapabilities {
            AdapterCapabilities::terminal_mux_defaults()
        }

        fn list_panes_for_pid(&self, _pid: u32) -> anyhow::Result<Vec<TerminalPaneSnapshot>> {
            Ok(vec![
                TerminalPaneSnapshot {
                    pane_id: 10,
                    tab_id: Some(1),
                    window_id: Some(100),
                    is_active: false,
                    foreground_process_name: Some("bash".into()),
                },
                TerminalPaneSnapshot {
                    pane_id: 20,
                    tab_id: Some(2),
                    window_id: Some(200),
                    is_active: true,
                    foreground_process_name: Some("nvim".into()),
                },
                TerminalPaneSnapshot {
                    pane_id: 30,
                    tab_id: Some(2),
                    window_id: Some(200),
                    is_active: false,
                    foreground_process_name: Some("python".into()),
                },
            ])
        }

        fn focused_pane_for_pid(&self, _pid: u32) -> anyhow::Result<u64> {
            Ok(20)
        }

        fn pane_in_direction_for_pid(
            &self,
            _pid: u32,
            pane_id: u64,
            dir: Direction,
        ) -> anyhow::Result<Option<u64>> {
            if pane_id != 20 {
                return Ok(None);
            }
            Ok(match dir {
                Direction::West => Some(10),
                Direction::North => Some(30),
                Direction::East | Direction::South => None,
            })
        }

        fn send_text_to_pane(&self, _pid: u32, _pane_id: u64, _text: &str) -> anyhow::Result<()> {
            Ok(())
        }

        fn mux_attach_args(&self, _target: String) -> Option<Vec<String>> {
            None
        }

        fn merge_source_pane_into_focused_target(
            &self,
            _source_pid: u32,
            _source_pane_id: u64,
            _target_pid: u32,
            _target_window_id: Option<u64>,
            _dir: Direction,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn topology_handler_can_focus_from_directional_neighbors_helper() {
        let mux = SnapshotMux;
        assert!(mux.can_focus(Direction::West, 0).expect("west focus"));
        assert!(!mux.can_focus(Direction::East, 0).expect("east focus"));
    }

    #[test]
    fn mux_provider_helpers_scope_panes_and_foreground_process() {
        let mux = SnapshotMux;
        let panes = mux
            .active_scope_panes_for_pid(1)
            .expect("active scope panes");
        assert_eq!(panes.len(), 2);
        assert!(panes.iter().all(|pane| pane.tab_id == Some(2)));
        assert_eq!(mux.active_foreground_process(1), Some("nvim".to_string()));
    }

    #[test]
    fn mux_provider_pane_lookup_helpers_share_axis_and_decision_logic() {
        let mux = SnapshotMux;
        assert!(mux
            .axis_neighbors_exist_from_pane_lookup(1, Direction::West)
            .expect("axis neighbors"));
        assert_eq!(
            mux.perpendicular_pane_for_pid(1, 20, Direction::East)
                .expect("perpendicular pane"),
            Some(30)
        );
        assert!(matches!(
            mux.move_decision_from_pane_lookup(Direction::West, 1, false)
                .expect("internal decision"),
            MoveDecision::Internal
        ));
        assert!(matches!(
            mux.move_decision_from_pane_lookup(Direction::East, 1, true)
                .expect("rearrange decision"),
            MoveDecision::Rearrange
        ));
        assert!(matches!(
            mux.move_decision_from_pane_lookup(Direction::East, 1, false)
                .expect("tear-out decision"),
            MoveDecision::TearOut
        ));
    }

    #[test]
    fn terminal_pane_snapshot_helpers_prefer_active_and_dedup_ids() {
        let panes = vec![
            TerminalPaneSnapshot {
                pane_id: 30,
                tab_id: None,
                window_id: None,
                is_active: false,
                foreground_process_name: None,
            },
            TerminalPaneSnapshot {
                pane_id: 10,
                tab_id: None,
                window_id: None,
                is_active: true,
                foreground_process_name: None,
            },
            TerminalPaneSnapshot {
                pane_id: 10,
                tab_id: None,
                window_id: None,
                is_active: false,
                foreground_process_name: None,
            },
        ];
        assert_eq!(
            TerminalPaneSnapshot::active_or_first(panes.iter()).map(|pane| pane.pane_id),
            Some(10)
        );
        assert_eq!(TerminalPaneSnapshot::unique_ids(panes.iter()), vec![10, 30]);
    }
}
