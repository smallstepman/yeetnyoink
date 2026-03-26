use anyhow::{Context, Result};

use crate::engine::runtime::ProcessId;
use crate::engine::topology::{Direction, DirectionalNeighbors, MoveSurface};

use super::adapter::unsupported_operation;
use super::merge::{MergeExecutionMode, MergePreparation, SourcePaneMerge};

fn legacy_pid(pid: Option<ProcessId>) -> u32 {
    pid.map(ProcessId::get).unwrap_or(0)
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

/// Result of tearing a buffer/pane out of an app.
pub struct TearResult {
    /// Command to spawn the torn-out content as a new window.
    /// None if the app already created the window itself.
    pub spawn_command: Option<Vec<String>>,
}

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

    fn focus_if_possible(&self, dir: Direction, pid: u32) -> Result<bool> {
        let _span = tracing::debug_span!(
            "topology.focus_if_possible",
            handler = std::any::type_name::<Self>(),
            pid,
            ?dir
        )
        .entered();
        let can_focus = {
            let _span = tracing::debug_span!("topology.focus_if_possible.can_focus").entered();
            self.can_focus(dir, pid)?
        };
        if can_focus {
            let _span = tracing::debug_span!("topology.focus_if_possible.focus").entered();
            self.focus(dir, pid)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

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

    fn prepare_source_pane_merge<Meta>(
        &self,
        source_pid: Option<ProcessId>,
        missing_source_pid: &'static str,
        capture: impl FnOnce(u32) -> Result<(u64, Meta)>,
    ) -> Result<MergePreparation>
    where
        Self: Sized,
        Meta: Send + 'static,
    {
        self.prepare_merge_payload(source_pid, missing_source_pid, |source_pid| {
            let (pane_id, meta) = capture(source_pid)?;
            Ok(SourcePaneMerge::new(pane_id, meta))
        })
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

    fn resolve_source_pane_merge<Meta>(
        &self,
        source_pid: Option<ProcessId>,
        target_pid: Option<ProcessId>,
        preparation: MergePreparation,
        missing_source_pid: &'static str,
        missing_target_pid: &'static str,
        missing_preparation: &'static str,
    ) -> Result<(u32, u32, SourcePaneMerge<Meta>)>
    where
        Self: Sized,
        Meta: Send + 'static,
    {
        self.resolve_target_focused_merge::<SourcePaneMerge<Meta>>(
            source_pid,
            target_pid,
            preparation,
            missing_source_pid,
            missing_target_pid,
            missing_preparation,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn topology_handler_can_focus_from_directional_neighbors_helper() {
        let mux = SnapshotMux;
        assert!(mux.can_focus(Direction::West, 0).expect("west focus"));
        assert!(!mux.can_focus(Direction::East, 0).expect("east focus"));
    }
}
