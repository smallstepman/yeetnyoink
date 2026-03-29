#[allow(unused_imports)]
pub(crate) use super::desktop_topology_snapshot::tests::{
    SpaceSnapshot, space_snapshots_from_topology,
};
#[cfg(target_os = "macos")]
use super::foundation::ProcessSerialNumber;
#[cfg(target_os = "macos")]
#[allow(unused_imports)]
pub(crate) use super::foundation::tests::dictionary_from_type_refs;
use super::*;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    rc::Rc,
};

#[cfg(target_os = "macos")]
pub(crate) fn focused_window_id_via_ax<App, Window, FocusedApplication, FocusedWindow, WindowId>(
    focused_application: FocusedApplication,
    focused_window: FocusedWindow,
    window_id: WindowId,
) -> Result<Option<u64>, MacosNativeProbeError>
where
    FocusedApplication: FnMut() -> Result<Option<App>, MacosNativeProbeError>,
    FocusedWindow: FnMut(&App) -> Result<Option<Window>, MacosNativeProbeError>,
    WindowId: FnMut(&Window) -> Result<u64, MacosNativeProbeError>,
{
    ax::focused_window_id(focused_application, focused_window, window_id)
}

#[derive(Debug, Clone)]
struct SequencedTopologyApi {
    snapshots: Rc<RefCell<VecDeque<RawTopologySnapshot>>>,
    calls: Rc<RefCell<Vec<String>>>,
}

impl SequencedTopologyApi {
    fn new(snapshots: Vec<RawTopologySnapshot>, calls: Rc<RefCell<Vec<String>>>) -> Self {
        Self {
            snapshots: Rc::new(RefCell::new(VecDeque::from(snapshots))),
            calls,
        }
    }

    fn current_topology(&self) -> RawTopologySnapshot {
        self.snapshots
            .borrow()
            .front()
            .cloned()
            .expect("sequenced topology api must retain at least one snapshot")
    }
}

impl MacosNativeApi for SequencedTopologyApi {
    fn has_symbol(&self, _symbol: &'static str) -> bool {
        true
    }

    fn ax_is_trusted(&self) -> bool {
        true
    }

    fn minimal_topology_ready(&self) -> bool {
        true
    }

    fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
        Ok(native_desktop_snapshot_from_topology(
            &self.topology_snapshot()?,
        ))
    }

    fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
        Ok(self.current_topology().spaces)
    }

    fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
        Ok(self.current_topology().active_space_ids)
    }

    fn active_space_windows(&self, space_id: u64) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
        Ok(self
            .current_topology()
            .active_space_windows
            .get(&space_id)
            .cloned()
            .unwrap_or_default())
    }

    fn inactive_space_window_ids(&self) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
        Ok(self.current_topology().inactive_space_window_ids)
    }

    fn focused_window_id(&self) -> Result<Option<u64>, MacosNativeProbeError> {
        Ok(self.current_topology().focused_window_id)
    }

    fn onscreen_window_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
        Ok(self
            .current_topology()
            .active_space_windows
            .values()
            .flat_map(|windows| windows.iter().map(|window| window.id))
            .collect())
    }

    fn switch_space(&self, space_id: u64) -> Result<(), MacosNativeOperationError> {
        self.calls
            .borrow_mut()
            .push(format!("switch_space:{space_id}"));
        let mut snapshots = self.snapshots.borrow_mut();
        if snapshots.len() > 1 {
            snapshots.pop_front();
        }
        Ok(())
    }

    fn focus_window(&self, window_id: u64) -> Result<(), MacosNativeOperationError> {
        self.calls
            .borrow_mut()
            .push(format!("focus_window:{window_id}"));
        Ok(())
    }

    fn focus_window_with_known_pid(
        &self,
        window_id: u64,
        pid: u32,
    ) -> Result<(), MacosNativeOperationError> {
        self.calls
            .borrow_mut()
            .push(format!("focus_window_with_known_pid:{window_id}:{pid}"));
        let topology = self
            .topology_snapshot()
            .map_err(MacosNativeOperationError::from)?;
        if topology_contains_window(&topology, window_id) {
            Ok(())
        } else {
            Err(MacosNativeOperationError::MissingWindow(window_id))
        }
    }

    fn move_window_to_space(
        &self,
        window_id: u64,
        space_id: u64,
    ) -> Result<(), MacosNativeOperationError> {
        self.calls
            .borrow_mut()
            .push(format!("move_window_to_space:{window_id}:{space_id}"));
        Ok(())
    }

    fn swap_window_frames(
        &self,
        source_window_id: u64,
        _source_frame: NativeBounds,
        target_window_id: u64,
        _target_frame: NativeBounds,
    ) -> Result<(), MacosNativeOperationError> {
        self.calls.borrow_mut().push(format!(
            "swap_window_frames:{source_window_id}:{target_window_id}"
        ));
        Ok(())
    }

    fn topology_snapshot(&self) -> Result<RawTopologySnapshot, MacosNativeProbeError> {
        let mut snapshots = self.snapshots.borrow_mut();
        let snapshot = snapshots
            .front()
            .cloned()
            .expect("sequenced topology api must retain at least one snapshot");
        if snapshots.len() > 1 {
            snapshots.pop_front();
        }
        Ok(snapshot)
    }
}

#[derive(Debug, Clone)]
struct SwitchThenFocusSamePidAxFallbackApi {
    topology: RawTopologySnapshot,
    switched_space_windows: HashMap<u64, Vec<RawWindow>>,
    post_switch_snapshot_topologies: Rc<RefCell<VecDeque<RawTopologySnapshot>>>,
    current_space_id: Rc<RefCell<u64>>,
    calls: Rc<RefCell<Vec<String>>>,
    ax_backed_window_ids: Vec<u64>,
}

impl MacosNativeApi for SwitchThenFocusSamePidAxFallbackApi {
    fn has_symbol(&self, _symbol: &'static str) -> bool {
        true
    }

    fn ax_is_trusted(&self) -> bool {
        true
    }

    fn minimal_topology_ready(&self) -> bool {
        true
    }

    fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
        if !self
            .topology
            .active_space_ids
            .contains(&*self.current_space_id.borrow())
        {
            let mut queued = self.post_switch_snapshot_topologies.borrow_mut();
            if queued.len() > 1 {
                return Ok(native_desktop_snapshot_from_topology(
                    &queued
                        .pop_front()
                        .expect("post-switch snapshot queue should be non-empty"),
                ));
            }
            if let Some(snapshot) = queued.front() {
                return Ok(native_desktop_snapshot_from_topology(snapshot));
            }
        }
        Ok(native_desktop_snapshot_from_topology(
            &self.topology_snapshot()?,
        ))
    }

    fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
        Ok(self.topology.spaces.clone())
    }

    fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
        Ok(HashSet::from([*self.current_space_id.borrow()]))
    }

    fn active_space_windows(&self, space_id: u64) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
        if *self.current_space_id.borrow() == space_id {
            if let Some(windows) = self.switched_space_windows.get(&space_id) {
                return Ok(windows.clone());
            }
        }
        Ok(self
            .topology
            .active_space_windows
            .get(&space_id)
            .cloned()
            .unwrap_or_default())
    }

    fn inactive_space_window_ids(&self) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
        Ok(self.topology.inactive_space_window_ids.clone())
    }

    fn focused_window_id(&self) -> Result<Option<u64>, MacosNativeProbeError> {
        Ok(self
            .active_space_windows(*self.current_space_id.borrow())?
            .first()
            .map(|window| window.id))
    }

    fn onscreen_window_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
        Ok(self
            .active_space_windows(*self.current_space_id.borrow())?
            .into_iter()
            .map(|window| window.id)
            .collect())
    }

    fn switch_space(&self, space_id: u64) -> Result<(), MacosNativeOperationError> {
        self.calls
            .borrow_mut()
            .push(format!("switch_space:{space_id}"));
        *self.current_space_id.borrow_mut() = space_id;
        Ok(())
    }

    fn focus_window(&self, window_id: u64) -> Result<(), MacosNativeOperationError> {
        Err(MacosNativeOperationError::MissingWindow(window_id))
    }

    fn focus_window_with_known_pid(
        &self,
        window_id: u64,
        pid: u32,
    ) -> Result<(), MacosNativeOperationError> {
        self.calls
            .borrow_mut()
            .push(format!("focus_window_with_known_pid:{window_id}:{pid}"));
        if self.ax_backed_window_ids.contains(&window_id) {
            Ok(())
        } else {
            Err(MacosNativeOperationError::MissingWindow(window_id))
        }
    }

    fn ax_window_ids_for_pid(&self, _pid: u32) -> Result<Vec<u64>, MacosNativeOperationError> {
        Ok(self.ax_backed_window_ids.clone())
    }

    fn move_window_to_space(
        &self,
        _window_id: u64,
        _space_id: u64,
    ) -> Result<(), MacosNativeOperationError> {
        Ok(())
    }

    fn swap_window_frames(
        &self,
        _source_window_id: u64,
        _source_frame: NativeBounds,
        _target_window_id: u64,
        _target_frame: NativeBounds,
    ) -> Result<(), MacosNativeOperationError> {
        Ok(())
    }
}

fn topology_contains_window(topology: &RawTopologySnapshot, window_id: u64) -> bool {
    topology
        .active_space_windows
        .values()
        .any(|windows| windows.iter().any(|window| window.id == window_id))
        || topology
            .inactive_space_window_ids
            .values()
            .any(|window_ids| window_ids.contains(&window_id))
}

fn take_calls(calls: &Rc<RefCell<Vec<String>>>) -> Vec<String> {
    std::mem::take(&mut *calls.borrow_mut())
}

fn raw_window(id: u64) -> RawWindow {
    RawWindow {
        id,
        pid: None,
        app_id: None,
        title: None,
        level: 0,
        visible_index: None,
        frame: None,
    }
}

impl RawWindow {
    fn with_level(mut self, level: i32) -> Self {
        self.level = level;
        self
    }

    fn with_visible_index(mut self, visible_index: usize) -> Self {
        self.visible_index = Some(visible_index);
        self
    }

    fn with_pid(mut self, pid: u32) -> Self {
        self.pid = Some(pid);
        self
    }

    fn with_app_id(mut self, app_id: &str) -> Self {
        self.app_id = Some(app_id.to_string());
        self
    }

    fn with_title(mut self, title: &str) -> Self {
        self.title = Some(title.to_string());
        self
    }

    fn with_frame(mut self, frame: NativeBounds) -> Self {
        self.frame = Some(frame);
        self
    }
}

fn raw_desktop_space(managed_space_id: u64) -> RawSpaceRecord {
    RawSpaceRecord {
        managed_space_id,
        display_index: 0,
        space_type: desktop_topology_snapshot::DESKTOP_SPACE_TYPE,
        tile_spaces: Vec::new(),
        has_tile_layout_manager: false,
        stage_manager_managed: false,
    }
}

fn raw_split_space(managed_space_id: u64, tile_spaces: &[u64]) -> RawSpaceRecord {
    RawSpaceRecord {
        managed_space_id,
        display_index: 0,
        space_type: desktop_topology_snapshot::DESKTOP_SPACE_TYPE,
        tile_spaces: tile_spaces.to_vec(),
        has_tile_layout_manager: true,
        stage_manager_managed: false,
    }
}

#[test]
fn enrich_real_window_app_ids_resolves_bundle_ids_after_parsing() {
    let windows = vec![raw_window(11).with_pid(42), raw_window(12)];

    let enriched = enrich_real_window_app_ids_with(windows, |pid| match pid {
        42 => Some("com.example.test".to_string()),
        _ => None,
    });

    assert_eq!(
        enriched,
        vec![
            raw_window(11).with_pid(42).with_app_id("com.example.test"),
            raw_window(12)
        ]
    );
}

#[test]
fn enrich_real_window_app_ids_reuses_pid_lookups_within_single_pass() {
    let windows = vec![
        raw_window(11).with_pid(42),
        raw_window(12).with_pid(42),
        raw_window(13).with_pid(7),
        raw_window(14).with_pid(42),
    ];
    let mut resolved_pids = Vec::new();

    let enriched = enrich_real_window_app_ids_with(windows, |pid| {
        resolved_pids.push(pid);
        Some(format!("com.example.{pid}"))
    });

    assert_eq!(resolved_pids, vec![42, 7]);
    assert_eq!(
        enriched,
        vec![
            raw_window(11).with_pid(42).with_app_id("com.example.42"),
            raw_window(12).with_pid(42).with_app_id("com.example.42"),
            raw_window(13).with_pid(7).with_app_id("com.example.7"),
            raw_window(14).with_pid(42).with_app_id("com.example.42"),
        ]
    );
}

#[test]
fn non_active_space_windows_remain_unordered() {
    let snapshots = snapshots_for_inactive_space(99, &[21, 22]);
    assert!(snapshots.iter().all(|window| window.order_index.is_none()));
}

#[test]
fn best_window_id_from_windows_ignores_non_normal_layer_targets() {
    let windows = vec![
        raw_window(159)
            .with_pid(946)
            .with_level(0)
            .with_frame(NativeBounds {
                x: 1200,
                y: 120,
                width: 500,
                height: 900,
            }),
        raw_window(52)
            .with_pid(950)
            .with_level(25)
            .with_frame(NativeBounds {
                x: 1739,
                y: 0,
                width: 63,
                height: 39,
            }),
    ];

    assert_eq!(
        best_window_id_from_windows(NativeDirection::East, &windows),
        Some(159)
    );
}

#[test]
fn focused_window_prefers_frontmost_window_across_active_spaces() {
    let topology = RawTopologySnapshot {
        spaces: vec![
            RawSpaceRecord {
                managed_space_id: 1,
                display_index: 0,
                space_type: desktop_topology_snapshot::DESKTOP_SPACE_TYPE,
                tile_spaces: Vec::new(),
                has_tile_layout_manager: false,
                stage_manager_managed: false,
            },
            RawSpaceRecord {
                managed_space_id: 3,
                display_index: 1,
                space_type: desktop_topology_snapshot::FULLSCREEN_SPACE_TYPE,
                tile_spaces: Vec::new(),
                has_tile_layout_manager: false,
                stage_manager_managed: false,
            },
        ],
        active_space_ids: HashSet::from([1, 3]),
        active_space_windows: HashMap::from([
            (
                1,
                vec![
                    raw_window(11)
                        .with_visible_index(2)
                        .with_pid(1111)
                        .with_app_id("com.example.left")
                        .with_title("Left display"),
                ],
            ),
            (
                3,
                vec![
                    raw_window(31)
                        .with_visible_index(0)
                        .with_pid(3333)
                        .with_app_id("com.example.right")
                        .with_title("Right display"),
                ],
            ),
        ]),
        inactive_space_window_ids: HashMap::from([(2, vec![21, 22])]),
        focused_window_id: Some(31),
    };

    let focused = focused_window_from_topology(&topology).unwrap();

    assert_eq!(focused.id, 31);
    assert_eq!(focused.space_id, 3);
    assert_eq!(focused.order_index, Some(0));
}

#[test]
fn focused_window_prefers_explicit_window_id_over_visible_order_heuristic() {
    let topology = RawTopologySnapshot {
        spaces: vec![raw_desktop_space(1)],
        active_space_ids: HashSet::from([1]),
        active_space_windows: HashMap::from([(
            1,
            vec![
                raw_window(10).with_visible_index(0),
                raw_window(20).with_visible_index(1),
            ],
        )]),
        inactive_space_window_ids: HashMap::new(),
        focused_window_id: Some(20),
    };

    let focused = focused_window_from_topology(&topology).unwrap();

    assert_eq!(focused.id, 20);
}

#[test]
fn active_space_snapshot_ordered_window_ids_match_window_ordering_contract() {
    let topology = RawTopologySnapshot {
        spaces: vec![raw_desktop_space(1)],
        active_space_ids: HashSet::from([1]),
        active_space_windows: HashMap::from([(
            1,
            vec![
                raw_window(11).with_visible_index(1),
                raw_window(12).with_visible_index(0),
                raw_window(13).with_level(5),
            ],
        )]),
        inactive_space_window_ids: HashMap::new(),
        focused_window_id: Some(12),
    };

    let spaces = space_snapshots_from_topology(&topology);
    let active = spaces.iter().find(|space| space.is_active).unwrap();
    let windows = window_snapshots_from_topology(&topology);
    let ordered_window_ids_from_windows = windows
        .iter()
        .filter(|window| topology.active_space_ids.contains(&window.space_id))
        .map(|window| (window.id, window.order_index.unwrap()))
        .collect::<Vec<_>>();

    assert_eq!(
        active.ordered_window_ids.as_deref(),
        Some(&[12, 11, 13][..])
    );
    assert_eq!(
        ordered_window_ids_from_windows,
        vec![(12, 0), (11, 1), (13, 2)]
    );
}

#[test]
fn focus_window_via_process_and_raise_fronts_makes_key_then_raises_target_window() {
    let calls = Rc::new(RefCell::new(Vec::new()));

    focus_window_via_process_and_raise(
        77,
        |_| Ok(5151),
        |pid| {
            assert_eq!(pid, 5151);
            Ok(ProcessSerialNumber {
                high_long_of_psn: 1,
                low_long_of_psn: 2,
            })
        },
        {
            let calls = calls.clone();
            move |psn, window_id| {
                calls.borrow_mut().push(format!(
                    "front:{}:{}:{}",
                    psn.high_long_of_psn, psn.low_long_of_psn, window_id
                ));
                Ok(())
            }
        },
        {
            let calls = calls.clone();
            move |psn, window_id| {
                calls.borrow_mut().push(format!(
                    "make_key:{}:{}:{}",
                    psn.high_long_of_psn, psn.low_long_of_psn, window_id
                ));
                Ok(())
            }
        },
        {
            let calls = calls.clone();
            move |window_id, pid| {
                calls.borrow_mut().push(format!("raise:{window_id}:{pid}"));
                Ok(())
            }
        },
    )
    .unwrap();

    assert_eq!(
        take_calls(&calls),
        vec!["front:1:2:77", "make_key:1:2:77", "raise:77:5151"]
    );
}

#[test]
fn focus_window_via_make_key_and_raise_skips_front_process() {
    let calls = Rc::new(RefCell::new(Vec::new()));

    focus_window_via_make_key_and_raise(
        77,
        |_| Ok(5151),
        |pid| {
            assert_eq!(pid, 5151);
            Ok(ProcessSerialNumber {
                high_long_of_psn: 1,
                low_long_of_psn: 2,
            })
        },
        {
            let calls = calls.clone();
            move |psn, window_id| {
                calls.borrow_mut().push(format!(
                    "make_key:{}:{}:{}",
                    psn.high_long_of_psn, psn.low_long_of_psn, window_id
                ));
                Ok(())
            }
        },
        {
            let calls = calls.clone();
            move |window_id, pid| {
                calls.borrow_mut().push(format!("raise:{window_id}:{pid}"));
                Ok(())
            }
        },
    )
    .unwrap();

    assert_eq!(take_calls(&calls), vec!["make_key:1:2:77", "raise:77:5151"]);
}

#[test]
fn focus_window_via_make_key_and_raise_retries_missing_ax_window_during_raise() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let attempts = Rc::new(RefCell::new(0usize));

    focus_window_via_make_key_and_raise(
        77,
        |_| Ok(5151),
        |_| {
            Ok(ProcessSerialNumber {
                high_long_of_psn: 1,
                low_long_of_psn: 2,
            })
        },
        {
            let calls = calls.clone();
            move |psn, window_id| {
                calls.borrow_mut().push(format!(
                    "make_key:{}:{}:{}",
                    psn.high_long_of_psn, psn.low_long_of_psn, window_id
                ));
                Ok(())
            }
        },
        {
            let calls = calls.clone();
            let attempts = attempts.clone();
            move |window_id, pid| {
                let mut attempts = attempts.borrow_mut();
                *attempts += 1;
                calls
                    .borrow_mut()
                    .push(format!("raise:{window_id}:{pid}:{}", *attempts));
                if *attempts == 1 {
                    Err(MacosNativeOperationError::MissingWindow(window_id))
                } else {
                    Ok(())
                }
            }
        },
    )
    .unwrap();

    assert_eq!(
        take_calls(&calls),
        vec!["make_key:1:2:77", "raise:77:5151:1", "raise:77:5151:2"]
    );
}

#[test]
fn focus_window_via_process_and_raise_retries_missing_ax_window_during_raise() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let attempts = Rc::new(RefCell::new(0usize));

    focus_window_via_process_and_raise(
        77,
        |_| Ok(5151),
        |_| {
            Ok(ProcessSerialNumber {
                high_long_of_psn: 1,
                low_long_of_psn: 2,
            })
        },
        {
            let calls = calls.clone();
            move |psn, window_id| {
                calls.borrow_mut().push(format!(
                    "front:{}:{}:{}",
                    psn.high_long_of_psn, psn.low_long_of_psn, window_id
                ));
                Ok(())
            }
        },
        {
            let calls = calls.clone();
            move |psn, window_id| {
                calls.borrow_mut().push(format!(
                    "make_key:{}:{}:{}",
                    psn.high_long_of_psn, psn.low_long_of_psn, window_id
                ));
                Ok(())
            }
        },
        {
            let calls = calls.clone();
            let attempts = attempts.clone();
            move |window_id, pid| {
                let mut attempts = attempts.borrow_mut();
                *attempts += 1;
                calls
                    .borrow_mut()
                    .push(format!("raise:{window_id}:{pid}:{}", *attempts));
                if *attempts == 1 {
                    Err(MacosNativeOperationError::MissingWindow(window_id))
                } else {
                    Ok(())
                }
            }
        },
    )
    .unwrap();

    assert_eq!(
        take_calls(&calls),
        vec![
            "front:1:2:77",
            "make_key:1:2:77",
            "raise:77:5151:1",
            "raise:77:5151:2",
        ]
    );
}

#[test]
fn focus_window_via_process_and_raise_waits_past_three_missing_ax_retries() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let attempts = Rc::new(RefCell::new(0usize));

    focus_window_via_process_and_raise(
        77,
        |_| Ok(5151),
        |_| {
            Ok(ProcessSerialNumber {
                high_long_of_psn: 1,
                low_long_of_psn: 2,
            })
        },
        {
            let calls = calls.clone();
            move |psn, window_id| {
                calls.borrow_mut().push(format!(
                    "front:{}:{}:{}",
                    psn.high_long_of_psn, psn.low_long_of_psn, window_id
                ));
                Ok(())
            }
        },
        {
            let calls = calls.clone();
            move |psn, window_id| {
                calls.borrow_mut().push(format!(
                    "make_key:{}:{}:{}",
                    psn.high_long_of_psn, psn.low_long_of_psn, window_id
                ));
                Ok(())
            }
        },
        {
            let calls = calls.clone();
            let attempts = attempts.clone();
            move |window_id, pid| {
                let mut attempts = attempts.borrow_mut();
                *attempts += 1;
                calls
                    .borrow_mut()
                    .push(format!("raise:{window_id}:{pid}:{}", *attempts));
                if *attempts < 4 {
                    Err(MacosNativeOperationError::MissingWindow(window_id))
                } else {
                    Ok(())
                }
            }
        },
    )
    .unwrap();

    assert_eq!(
        take_calls(&calls),
        vec![
            "front:1:2:77",
            "make_key:1:2:77",
            "raise:77:5151:1",
            "raise:77:5151:2",
            "raise:77:5151:3",
            "raise:77:5151:4",
        ]
    );
}

#[test]
fn backend_focus_direction_keeps_selected_target_when_next_snapshot_drops_it() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let first_topology = RawTopologySnapshot {
        spaces: vec![raw_desktop_space(1)],
        active_space_ids: HashSet::from([1]),
        active_space_windows: HashMap::from([(
            1,
            vec![
                raw_window(20)
                    .with_visible_index(0)
                    .with_pid(2020)
                    .with_app_id("com.example.source")
                    .with_title("source")
                    .with_frame(NativeBounds {
                        x: 0,
                        y: 0,
                        width: 100,
                        height: 100,
                    }),
                raw_window(51)
                    .with_visible_index(1)
                    .with_pid(5151)
                    .with_app_id("com.example.target")
                    .with_title("target")
                    .with_frame(NativeBounds {
                        x: 120,
                        y: 0,
                        width: 100,
                        height: 100,
                    }),
            ],
        )]),
        inactive_space_window_ids: HashMap::new(),
        focused_window_id: Some(20),
    };
    let second_topology = RawTopologySnapshot {
        spaces: vec![raw_desktop_space(1)],
        active_space_ids: HashSet::from([1]),
        active_space_windows: HashMap::from([(
            1,
            vec![
                raw_window(20)
                    .with_visible_index(0)
                    .with_pid(2020)
                    .with_app_id("com.example.source")
                    .with_title("source")
                    .with_frame(NativeBounds {
                        x: 0,
                        y: 0,
                        width: 100,
                        height: 100,
                    }),
            ],
        )]),
        inactive_space_window_ids: HashMap::new(),
        focused_window_id: Some(20),
    };
    let api = SequencedTopologyApi::new(vec![second_topology.clone()], calls.clone());
    let snapshot = native_desktop_snapshot_from_topology(&first_topology);

    api.focus_same_space_target_in_snapshot(&snapshot, NativeDirection::East, 51)
        .unwrap();

    assert_eq!(
        take_calls(&calls),
        vec!["focus_window_with_known_pid:51:5151", "focus_window:51"]
    );
}

#[test]
fn backend_focus_direction_remaps_post_switch_same_pid_splitview_target_before_active_space_focus()
{
    let calls = Rc::new(RefCell::new(Vec::new()));
    let initial_topology = RawTopologySnapshot {
        spaces: vec![raw_split_space(2, &[11, 12]), raw_desktop_space(1)],
        active_space_ids: HashSet::from([1]),
        active_space_windows: HashMap::from([(
            1,
            vec![
                raw_window(20)
                    .with_visible_index(0)
                    .with_pid(2020)
                    .with_app_id("com.apple.Safari")
                    .with_title("source")
                    .with_frame(NativeBounds {
                        x: 360,
                        y: 0,
                        width: 100,
                        height: 100,
                    }),
            ],
        )]),
        inactive_space_window_ids: HashMap::from([(2, vec![998, 1019])]),
        focused_window_id: Some(20),
    };
    let post_switch_selection_topology = RawTopologySnapshot {
        spaces: initial_topology.spaces.clone(),
        active_space_ids: HashSet::from([2]),
        active_space_windows: HashMap::from([(
            2,
            vec![
                raw_window(985)
                    .with_visible_index(0)
                    .with_pid(4613)
                    .with_app_id("com.github.wez.wezterm")
                    .with_title("actual-left")
                    .with_frame(NativeBounds {
                        x: 12,
                        y: 0,
                        width: 108,
                        height: 120,
                    }),
                raw_window(998)
                    .with_visible_index(1)
                    .with_pid(4613)
                    .with_app_id("com.github.wez.wezterm")
                    .with_title("stale-left")
                    .with_frame(NativeBounds {
                        x: 0,
                        y: 0,
                        width: 120,
                        height: 120,
                    }),
                raw_window(1019)
                    .with_visible_index(2)
                    .with_pid(4613)
                    .with_app_id("com.github.wez.wezterm")
                    .with_title("stale-right")
                    .with_frame(NativeBounds {
                        x: 220,
                        y: 0,
                        width: 121,
                        height: 120,
                    }),
                raw_window(410)
                    .with_visible_index(3)
                    .with_pid(4613)
                    .with_app_id("com.github.wez.wezterm")
                    .with_title("actual-right")
                    .with_frame(NativeBounds {
                        x: 228,
                        y: 0,
                        width: 112,
                        height: 120,
                    }),
            ],
        )]),
        inactive_space_window_ids: HashMap::from([(1, vec![20])]),
        focused_window_id: Some(985),
    };
    let post_switch_remap_topology = RawTopologySnapshot {
        spaces: initial_topology.spaces.clone(),
        active_space_ids: HashSet::from([2]),
        active_space_windows: HashMap::from([(
            2,
            vec![
                raw_window(985)
                    .with_visible_index(0)
                    .with_pid(4613)
                    .with_app_id("com.github.wez.wezterm")
                    .with_title("actual-left")
                    .with_frame(NativeBounds {
                        x: 12,
                        y: 0,
                        width: 108,
                        height: 120,
                    }),
                raw_window(410)
                    .with_visible_index(1)
                    .with_pid(4613)
                    .with_app_id("com.github.wez.wezterm")
                    .with_title("actual-right")
                    .with_frame(NativeBounds {
                        x: 228,
                        y: 0,
                        width: 112,
                        height: 120,
                    }),
            ],
        )]),
        inactive_space_window_ids: HashMap::from([(1, vec![20])]),
        focused_window_id: Some(985),
    };
    let api = SwitchThenFocusSamePidAxFallbackApi {
        topology: initial_topology,
        switched_space_windows: HashMap::from([(
            2,
            post_switch_selection_topology
                .active_space_windows
                .get(&2)
                .cloned()
                .unwrap(),
        )]),
        post_switch_snapshot_topologies: Rc::new(RefCell::new(VecDeque::from([
            post_switch_selection_topology,
            post_switch_remap_topology,
        ]))),
        current_space_id: Rc::new(RefCell::new(1)),
        calls: calls.clone(),
        ax_backed_window_ids: vec![985, 410],
    };

    api.switch_space(2).unwrap();
    api.focus_window_in_active_space_with_known_pid(
        1019,
        4613,
        Some(ActiveSpaceFocusTargetHint {
            space_id: 2,
            bounds: NativeBounds {
                x: 220,
                y: 0,
                width: 121,
                height: 120,
            },
        }),
    )
    .unwrap();

    assert_eq!(
        take_calls(&calls),
        vec![
            "switch_space:2",
            "focus_window_with_known_pid:1019:4613",
            "focus_window_with_known_pid:410:4613",
        ]
    );
}
