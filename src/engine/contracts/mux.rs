use anyhow::{anyhow, Context, Result};
use serde::de::DeserializeOwned;

use crate::engine::runtime::{self};
use crate::engine::topology::{Direction, DirectionalNeighbors};

use super::adapter::{AdapterCapabilities, unsupported_operation};
use super::topology::{MoveDecision, TopologyHandler};

pub trait TerminalMultiplexerProvider: TopologyHandler + crate::engine::contract::ChainResolver {
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

    fn focused_pane_arg_for_pid(&self, pid: u32) -> Result<(u64, String)> {
        let pane_id = self.focused_pane_for_pid(pid)?;
        Ok((pane_id, pane_id.to_string()))
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
    pub tty_name: Option<String>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::contracts::topology::{MoveDecision, TearResult};
    use crate::engine::runtime::ProcessId;
    use crate::engine::topology::{Direction, DirectionalNeighbors};

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
                    tty_name: None,
                },
                TerminalPaneSnapshot {
                    pane_id: 20,
                    tab_id: Some(2),
                    window_id: Some(200),
                    is_active: true,
                    foreground_process_name: Some("nvim".into()),
                    tty_name: None,
                },
                TerminalPaneSnapshot {
                    pane_id: 30,
                    tab_id: Some(2),
                    window_id: Some(200),
                    is_active: false,
                    foreground_process_name: Some("python".into()),
                    tty_name: None,
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
    fn mux_provider_merge_helpers_wrap_source_pane_payloads() {
        let mux = SnapshotMux;
        let source_pid = Some(ProcessId::new(10).expect("nonzero"));
        let target_pid = Some(ProcessId::new(20).expect("nonzero"));
        let preparation = mux
            .prepare_source_pane_merge(source_pid, "missing source", |_pid| {
                Ok((30, "detached-session".to_string()))
            })
            .expect("preparation");
        let (resolved_source, resolved_target, preparation) = mux
            .resolve_source_pane_merge::<String>(
                source_pid,
                target_pid,
                preparation,
                "missing source",
                "missing target",
                "missing preparation",
            )
            .expect("resolved merge");
        assert_eq!(resolved_source, 10);
        assert_eq!(resolved_target, 20);
        assert_eq!(preparation.pane_id, 30);
        assert_eq!(preparation.meta, "detached-session");
    }

    #[test]
    fn mux_provider_focused_pane_arg_helper_formats_id() {
        let mux = SnapshotMux;
        assert_eq!(
            mux.focused_pane_arg_for_pid(1).expect("focused pane arg"),
            (20, "20".to_string())
        );
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
                tty_name: None,
            },
            TerminalPaneSnapshot {
                pane_id: 10,
                tab_id: None,
                window_id: None,
                is_active: true,
                foreground_process_name: None,
                tty_name: None,
            },
            TerminalPaneSnapshot {
                pane_id: 10,
                tab_id: None,
                window_id: None,
                is_active: false,
                foreground_process_name: None,
                tty_name: None,
            },
        ];
        assert_eq!(
            TerminalPaneSnapshot::active_or_first(panes.iter()).map(|pane| pane.pane_id),
            Some(10)
        );
        assert_eq!(TerminalPaneSnapshot::unique_ids(panes.iter()), vec![10, 30]);
    }
}
