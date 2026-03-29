// Backend-internal adapter coverage lives in a separate include file so
// macos_native.rs keeps only outer-policy and source-structure tests.

#[derive(Debug, Clone)]
struct FakeNativeApi {
    topology: RawTopologySnapshot,
    space_windows: HashMap<u64, Vec<RawWindow>>,
    calls: Rc<RefCell<Vec<String>>>,
}

impl Default for FakeNativeApi {
    fn default() -> Self {
        Self {
            topology: Self::topology_fixture(41),
            space_windows: HashMap::new(),
            calls: Rc::new(RefCell::new(Vec::new())),
        }
    }
}

impl FakeNativeApi {
    fn topology_fixture(active_window_id: u64) -> RawTopologySnapshot {
        RawTopologySnapshot {
            spaces: vec![raw_desktop_space(1), raw_split_space(2, &[21, 22])],
            active_space_ids: HashSet::from([1]),
            active_space_windows: HashMap::from([(
                1,
                vec![raw_window(active_window_id)
                    .with_visible_index(0)
                    .with_pid(4242)
                    .with_app_id("com.example.focused")
                    .with_title("Focused window")],
            )]),
            inactive_space_window_ids: HashMap::from([(2, vec![21, 22])]),
            focused_window_id: Some(active_window_id),
        }
    }

    fn with_topology(mut self, topology: RawTopologySnapshot) -> Self {
        self.topology = topology;
        self
    }

    fn with_calls(mut self, calls: Rc<RefCell<Vec<String>>>) -> Self {
        self.calls = calls;
        self
    }
}

impl MacosNativeApi for FakeNativeApi {
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
        Ok(self.topology.spaces.clone())
    }

    fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
        Ok(self.topology.active_space_ids.clone())
    }

    fn active_space_windows(&self, space_id: u64) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
        Ok(self
            .space_windows
            .get(&space_id)
            .cloned()
            .or_else(|| self.topology.active_space_windows.get(&space_id).cloned())
            .unwrap_or_default())
    }

    fn inactive_space_window_ids(&self) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
        Ok(self.topology.inactive_space_window_ids.clone())
    }

    fn switch_space(&self, space_id: u64) -> Result<(), MacosNativeOperationError> {
        self.calls
            .borrow_mut()
            .push(format!("switch_space:{space_id}"));
        Ok(())
    }

    fn focus_window(&self, window_id: u64) -> Result<(), MacosNativeOperationError> {
        self.calls
            .borrow_mut()
            .push(format!("focus_window:{window_id}"));
        Ok(())
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
        Ok(self.topology.clone())
    }
}

#[derive(Debug, Clone)]
struct PostSwitchSelectionDriftApi {
    initial_topology: RawTopologySnapshot,
    switched_topology: RawTopologySnapshot,
    drifted_windows: Vec<RawWindow>,
    calls: Rc<RefCell<Vec<String>>>,
    current_space_id: Rc<RefCell<u64>>,
}

impl PostSwitchSelectionDriftApi {
    fn new(
        initial_topology: RawTopologySnapshot,
        switched_topology: RawTopologySnapshot,
        drifted_windows: Vec<RawWindow>,
        calls: Rc<RefCell<Vec<String>>>,
    ) -> Self {
        Self {
            initial_topology,
            switched_topology,
            drifted_windows,
            calls,
            current_space_id: Rc::new(RefCell::new(1)),
        }
    }
}

impl MacosNativeApi for PostSwitchSelectionDriftApi {
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
        Ok(self.initial_topology.spaces.clone())
    }

    fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
        Ok(HashSet::from([*self.current_space_id.borrow()]))
    }

    fn active_space_windows(&self, space_id: u64) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
        if *self.current_space_id.borrow() == 2 && space_id == 2 {
            return Ok(self.drifted_windows.clone());
        }
        Ok(self
            .initial_topology
            .active_space_windows
            .get(&space_id)
            .cloned()
            .unwrap_or_default())
    }

    fn inactive_space_window_ids(&self) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
        Ok(self.initial_topology.inactive_space_window_ids.clone())
    }

    fn focused_window_id(&self) -> Result<Option<u64>, MacosNativeProbeError> {
        Ok(if *self.current_space_id.borrow() == 2 {
            self.switched_topology.focused_window_id
        } else {
            self.initial_topology.focused_window_id
        })
    }

    fn onscreen_window_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
        Ok(if *self.current_space_id.borrow() == 2 {
            self.switched_topology
                .active_space_windows
                .values()
                .flat_map(|windows| windows.iter().map(|window| window.id))
                .collect()
        } else {
            self.initial_topology
                .active_space_windows
                .values()
                .flat_map(|windows| windows.iter().map(|window| window.id))
                .collect()
        })
    }

    fn switch_space(&self, space_id: u64) -> Result<(), MacosNativeOperationError> {
        self.calls
            .borrow_mut()
            .push(format!("switch_space:{space_id}"));
        *self.current_space_id.borrow_mut() = space_id;
        Ok(())
    }

    fn focus_window(&self, window_id: u64) -> Result<(), MacosNativeOperationError> {
        self.calls
            .borrow_mut()
            .push(format!("focus_window:{window_id}"));
        Ok(())
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

    fn topology_snapshot(&self) -> Result<RawTopologySnapshot, MacosNativeProbeError> {
        Ok(if *self.current_space_id.borrow() == 2 {
            self.switched_topology.clone()
        } else {
            self.initial_topology.clone()
        })
    }
}

#[derive(Debug, Clone)]
struct SamePidAxFallbackApi {
    topology: RawTopologySnapshot,
    ax_backed_window_ids: Vec<u64>,
    calls: Rc<RefCell<Vec<String>>>,
}

impl MacosNativeApi for SamePidAxFallbackApi {
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
        Ok(self.topology.spaces.clone())
    }

    fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
        Ok(self.topology.active_space_ids.clone())
    }

    fn active_space_windows(&self, space_id: u64) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
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
        Ok(self.topology.focused_window_id)
    }

    fn ax_window_ids_for_pid(&self, _pid: u32) -> Result<Vec<u64>, MacosNativeOperationError> {
        Ok(self.ax_backed_window_ids.clone())
    }

    fn switch_space(&self, _space_id: u64) -> Result<(), MacosNativeOperationError> {
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

#[derive(Debug, Clone)]
struct SwitchThenFocusApi {
    topology: RawTopologySnapshot,
    switched_space_windows: HashMap<u64, Vec<RawWindow>>,
    current_space_id: Rc<RefCell<u64>>,
    calls: Rc<RefCell<Vec<String>>>,
}

impl MacosNativeApi for SwitchThenFocusApi {
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
        if !self
            .switched_space_windows
            .contains_key(&*self.current_space_id.borrow())
        {
            return Err(MacosNativeOperationError::MissingWindow(window_id));
        }
        self.calls
            .borrow_mut()
            .push(format!("focus_window:{window_id}"));
        Ok(())
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

#[derive(Debug, Clone)]
struct AdjacentHotkeyOnlyApi {
    topology: RawTopologySnapshot,
    switched_space_windows: HashMap<u64, Vec<RawWindow>>,
    current_space_id: Rc<RefCell<u64>>,
    calls: Rc<RefCell<Vec<String>>>,
}

impl MacosNativeApi for AdjacentHotkeyOnlyApi {
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
        Err(MacosNativeOperationError::CallFailed(
            "direct_switch_for_adjacent_space",
        ))
    }

    fn switch_adjacent_space(
        &self,
        _direction: NativeDirection,
        space_id: u64,
    ) -> Result<(), MacosNativeOperationError> {
        *self.current_space_id.borrow_mut() = space_id;
        Ok(())
    }

    fn focus_window(&self, window_id: u64) -> Result<(), MacosNativeOperationError> {
        if !self
            .switched_space_windows
            .contains_key(&*self.current_space_id.borrow())
        {
            return Err(MacosNativeOperationError::MissingWindow(window_id));
        }
        self.calls
            .borrow_mut()
            .push(format!("focus_window:{window_id}"));
        Ok(())
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

#[derive(Debug, Clone)]
struct EmptySpaceSkippingAdjacentHotkeyApi {
    topology: RawTopologySnapshot,
    switched_space_windows: HashMap<u64, Vec<RawWindow>>,
    current_space_id: Rc<RefCell<u64>>,
    adjacent_hotkey_skip_target_space_id: u64,
    calls: Rc<RefCell<Vec<String>>>,
}

impl MacosNativeApi for EmptySpaceSkippingAdjacentHotkeyApi {
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

    fn switch_adjacent_space(
        &self,
        direction: NativeDirection,
        space_id: u64,
    ) -> Result<(), MacosNativeOperationError> {
        self.calls
            .borrow_mut()
            .push(format!("switch_adjacent_space:{direction}:{space_id}"));
        *self.current_space_id.borrow_mut() = self.adjacent_hotkey_skip_target_space_id;
        Ok(())
    }

    fn focus_window(&self, window_id: u64) -> Result<(), MacosNativeOperationError> {
        self.calls
            .borrow_mut()
            .push(format!("focus_window:{window_id}"));
        Ok(())
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

#[test]
fn backend_focus_direction_selects_closest_neighbor_by_geometry() {
    let _config = install_macos_native_focus_config("radial_center");
    let calls = Rc::new(RefCell::new(Vec::new()));
    let topology = RawTopologySnapshot {
        spaces: vec![raw_desktop_space(1)],
        active_space_ids: HashSet::from([1]),
        active_space_windows: HashMap::from([(
            1,
            vec![
                raw_window(10)
                    .with_pid(1010)
                    .with_app_id("com.example.left")
                    .with_title("left")
                    .with_frame(crate::engine::topology::Rect {
                        x: 0,
                        y: 0,
                        w: 100,
                        h: 100,
                    }),
                raw_window(20)
                    .with_visible_index(0)
                    .with_pid(2020)
                    .with_app_id("com.example.center")
                    .with_title("center")
                    .with_frame(crate::engine::topology::Rect {
                        x: 120,
                        y: 0,
                        w: 100,
                        h: 100,
                    }),
                raw_window(30)
                    .with_pid(3030)
                    .with_app_id("com.example.right")
                    .with_title("right")
                    .with_frame(crate::engine::topology::Rect {
                        x: 240,
                        y: 0,
                        w: 100,
                        h: 100,
                    }),
            ],
        )]),
        inactive_space_window_ids: HashMap::new(),
        focused_window_id: Some(20),
    };
    let api = FakeNativeApi::default()
        .with_topology(topology)
        .with_calls(calls.clone());
    let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

    adapter.focus_direction_inner(Direction::West).unwrap();

    assert_eq!(take_calls(&calls), vec!["focus_window:10"]);
}

#[test]
fn backend_focus_direction_uses_radial_center_strategy() {
    let _config = install_macos_native_focus_config("radial_center");
    let calls = Rc::new(RefCell::new(Vec::new()));
    let topology = RawTopologySnapshot {
        spaces: vec![raw_desktop_space(1)],
        active_space_ids: HashSet::from([1]),
        active_space_windows: HashMap::from([(
            1,
            vec![
                raw_window(10)
                    .with_visible_index(0)
                    .with_pid(1010)
                    .with_app_id("com.example.source")
                    .with_title("source")
                    .with_frame(Rect {
                        x: 200,
                        y: 100,
                        w: 100,
                        h: 100,
                    }),
                raw_window(20)
                    .with_pid(2020)
                    .with_app_id("com.example.radial-target")
                    .with_title("radial-target")
                    .with_frame(Rect {
                        x: 40,
                        y: 80,
                        w: 60,
                        h: 60,
                    }),
                raw_window(30)
                    .with_pid(3030)
                    .with_app_id("com.example.cross-edge-target")
                    .with_title("cross-edge-target")
                    .with_frame(Rect {
                        x: 90,
                        y: 150,
                        w: 130,
                        h: 130,
                    }),
            ],
        )]),
        inactive_space_window_ids: HashMap::new(),
        focused_window_id: Some(10),
    };
    let api = FakeNativeApi::default()
        .with_topology(topology)
        .with_calls(calls.clone());
    let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

    adapter.focus_direction_inner(Direction::West).unwrap();

    assert_eq!(take_calls(&calls), vec!["focus_window:20"]);
}

#[test]
fn backend_focus_direction_uses_cross_edge_gap_strategy() {
    let _config = install_macos_native_focus_config("cross_edge_gap");
    let calls = Rc::new(RefCell::new(Vec::new()));
    let topology = RawTopologySnapshot {
        spaces: vec![raw_desktop_space(1)],
        active_space_ids: HashSet::from([1]),
        active_space_windows: HashMap::from([(
            1,
            vec![
                raw_window(10)
                    .with_visible_index(0)
                    .with_pid(1010)
                    .with_app_id("com.example.source")
                    .with_title("source")
                    .with_frame(Rect {
                        x: 200,
                        y: 100,
                        w: 100,
                        h: 100,
                    }),
                raw_window(20)
                    .with_pid(2020)
                    .with_app_id("com.example.radial-target")
                    .with_title("radial-target")
                    .with_frame(Rect {
                        x: 40,
                        y: 80,
                        w: 60,
                        h: 60,
                    }),
                raw_window(30)
                    .with_pid(3030)
                    .with_app_id("com.example.cross-edge-target")
                    .with_title("cross-edge-target")
                    .with_frame(Rect {
                        x: 90,
                        y: 150,
                        w: 130,
                        h: 130,
                    }),
            ],
        )]),
        inactive_space_window_ids: HashMap::new(),
        focused_window_id: Some(10),
    };
    let api = FakeNativeApi::default()
        .with_topology(topology)
        .with_calls(calls.clone());
    let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

    adapter.focus_direction_inner(Direction::West).unwrap();

    assert_eq!(take_calls(&calls), vec!["focus_window:30"]);
}

#[test]
fn backend_focus_direction_prefers_opposite_split_pane_over_interior_same_app_window() {
    let _config = install_macos_native_focus_config("overlap_then_gap");
    let calls = Rc::new(RefCell::new(Vec::new()));
    let topology = RawTopologySnapshot {
        spaces: vec![raw_split_space(1, &[11, 12])],
        active_space_ids: HashSet::from([1]),
        active_space_windows: HashMap::from([(
            1,
            vec![
                raw_window(10)
                    .with_visible_index(0)
                    .with_pid(3350)
                    .with_app_id("com.github.wez.wezterm")
                    .with_title("left-pane")
                    .with_frame(Rect {
                        x: 0,
                        y: 0,
                        w: 120,
                        h: 120,
                    }),
                raw_window(15)
                    .with_visible_index(1)
                    .with_pid(926)
                    .with_app_id("ai.perplexity.mac")
                    .with_title("interior-helper")
                    .with_frame(Rect {
                        x: 150,
                        y: 0,
                        w: 60,
                        h: 120,
                    }),
                raw_window(20)
                    .with_visible_index(2)
                    .with_pid(926)
                    .with_app_id("ai.perplexity.mac")
                    .with_title("right-pane")
                    .with_frame(Rect {
                        x: 220,
                        y: 0,
                        w: 120,
                        h: 120,
                    }),
            ],
        )]),
        inactive_space_window_ids: HashMap::new(),
        focused_window_id: Some(20),
    };
    let api = FakeNativeApi::default()
        .with_topology(topology)
        .with_calls(calls.clone());
    let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

    adapter.focus_direction_inner(Direction::West).unwrap();

    assert_eq!(take_calls(&calls), vec!["focus_window:10"]);
}

#[test]
fn backend_split_view_focus_ignores_non_normal_layer_overlay_targets() {
    let _config = install_macos_native_focus_config("radial_center");
    let calls = Rc::new(RefCell::new(Vec::new()));
    let topology = RawTopologySnapshot {
        spaces: vec![raw_split_space(1, &[11, 12])],
        active_space_ids: HashSet::from([1]),
        active_space_windows: HashMap::from([(
            1,
            vec![
                raw_window(100)
                    .with_visible_index(0)
                    .with_pid(4001)
                    .with_app_id("com.example.source")
                    .with_title("source")
                    .with_frame(Rect {
                        x: 0,
                        y: 120,
                        w: 500,
                        h: 900,
                    }),
                raw_window(159)
                    .with_visible_index(1)
                    .with_pid(946)
                    .with_app_id("com.example.target")
                    .with_title("target")
                    .with_frame(Rect {
                        x: 1200,
                        y: 120,
                        w: 500,
                        h: 900,
                    }),
                raw_window(52)
                    .with_visible_index(2)
                    .with_level(25)
                    .with_pid(950)
                    .with_app_id("com.apple.controlcenter")
                    .with_title("Control Center")
                    .with_frame(Rect {
                        x: 1739,
                        y: 0,
                        w: 63,
                        h: 39,
                    }),
            ],
        )]),
        inactive_space_window_ids: HashMap::new(),
        focused_window_id: Some(100),
    };
    let api = FakeNativeApi::default()
        .with_topology(topology)
        .with_calls(calls.clone());
    let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

    adapter.focus_direction_inner(Direction::East).unwrap();

    assert_eq!(take_calls(&calls), vec!["focus_window:159"]);
}

#[test]
fn backend_focus_direction_preflights_same_pid_splitview_ax_target_before_focus_attempt() {
    let _config = install_macos_native_focus_config("overlap_then_gap");
    let calls = Rc::new(RefCell::new(Vec::new()));
    let topology = RawTopologySnapshot {
        spaces: vec![raw_split_space(1, &[11, 12])],
        active_space_ids: HashSet::from([1]),
        active_space_windows: HashMap::from([(
            1,
            vec![
                raw_window(998)
                    .with_visible_index(0)
                    .with_pid(4613)
                    .with_app_id("com.github.wez.wezterm")
                    .with_title("stale-left")
                    .with_frame(Rect {
                        x: 0,
                        y: 0,
                        w: 120,
                        h: 120,
                    }),
                raw_window(999)
                    .with_visible_index(1)
                    .with_pid(4613)
                    .with_app_id("com.github.wez.wezterm")
                    .with_title("actual-left")
                    .with_frame(Rect {
                        x: 12,
                        y: 0,
                        w: 108,
                        h: 120,
                    }),
                raw_window(410)
                    .with_visible_index(2)
                    .with_pid(4613)
                    .with_app_id("com.github.wez.wezterm")
                    .with_title("focused-right")
                    .with_frame(Rect {
                        x: 220,
                        y: 0,
                        w: 120,
                        h: 120,
                    }),
            ],
        )]),
        inactive_space_window_ids: HashMap::new(),
        focused_window_id: Some(410),
    };
    let api = SamePidAxFallbackApi {
        topology,
        ax_backed_window_ids: vec![999, 410],
        calls: calls.clone(),
    };
    let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

    adapter.focus_direction_inner(Direction::West).unwrap();

    assert_eq!(
        take_calls(&calls),
        vec!["focus_window_with_known_pid:999:4613"]
    );
}

#[test]
fn backend_focus_direction_switches_to_adjacent_split_space_when_desktop_helper_does_not_extend_west(
) {
    let _config = install_macos_native_focus_config("overlap_then_gap");
    let calls = Rc::new(RefCell::new(Vec::new()));
    let topology = RawTopologySnapshot {
        spaces: vec![raw_split_space(1, &[11, 12]), raw_desktop_space(2)],
        active_space_ids: HashSet::from([2]),
        active_space_windows: HashMap::from([(
            2,
            vec![
                raw_window(203)
                    .with_visible_index(0)
                    .with_pid(898)
                    .with_app_id("com.apple.Safari")
                    .with_title("frontmost")
                    .with_frame(Rect {
                        x: 0,
                        y: 0,
                        w: 240,
                        h: 120,
                    }),
                raw_window(201)
                    .with_visible_index(1)
                    .with_pid(898)
                    .with_app_id("com.apple.Safari")
                    .with_title("helper")
                    .with_frame(Rect {
                        x: 40,
                        y: 0,
                        w: 80,
                        h: 120,
                    }),
            ],
        )]),
        inactive_space_window_ids: HashMap::from([(1, vec![10, 20])]),
        focused_window_id: Some(203),
    };
    let api = SwitchThenFocusApi {
        topology,
        switched_space_windows: HashMap::from([(
            1,
            vec![
                raw_window(10)
                    .with_visible_index(0)
                    .with_pid(3350)
                    .with_app_id("com.github.wez.wezterm")
                    .with_title("left-pane")
                    .with_frame(Rect {
                        x: 0,
                        y: 0,
                        w: 120,
                        h: 120,
                    }),
                raw_window(20)
                    .with_visible_index(1)
                    .with_pid(926)
                    .with_app_id("ai.perplexity.mac")
                    .with_title("right-pane")
                    .with_frame(Rect {
                        x: 220,
                        y: 0,
                        w: 120,
                        h: 120,
                    }),
            ],
        )]),
        current_space_id: Rc::new(RefCell::new(2)),
        calls: calls.clone(),
    };
    let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

    adapter.focus_direction_inner(Direction::West).unwrap();

    assert_eq!(
        take_calls(&calls),
        vec!["switch_space:1", "focus_window:20"]
    );
}

#[test]
fn backend_focus_direction_switches_to_adjacent_space_when_desktop_helper_ties_west_edge_despite_visible_order(
) {
    let _config = install_macos_native_focus_config("overlap_then_gap");
    let calls = Rc::new(RefCell::new(Vec::new()));
    let topology = RawTopologySnapshot {
        spaces: vec![raw_split_space(1, &[11, 12]), raw_desktop_space(2)],
        active_space_ids: HashSet::from([2]),
        active_space_windows: HashMap::from([(
            2,
            vec![
                raw_window(203)
                    .with_visible_index(1)
                    .with_pid(898)
                    .with_app_id("com.apple.Safari")
                    .with_title("frontmost")
                    .with_frame(Rect {
                        x: 0,
                        y: 0,
                        w: 240,
                        h: 120,
                    }),
                raw_window(201)
                    .with_visible_index(0)
                    .with_pid(898)
                    .with_app_id("com.apple.Safari")
                    .with_title("helper")
                    .with_frame(Rect {
                        x: 0,
                        y: 0,
                        w: 80,
                        h: 120,
                    }),
            ],
        )]),
        inactive_space_window_ids: HashMap::from([(1, vec![10, 20])]),
        focused_window_id: Some(203),
    };
    let api = SwitchThenFocusApi {
        topology,
        switched_space_windows: HashMap::from([(
            1,
            vec![
                raw_window(10)
                    .with_visible_index(0)
                    .with_pid(3350)
                    .with_app_id("com.github.wez.wezterm")
                    .with_title("left-pane")
                    .with_frame(Rect {
                        x: 0,
                        y: 0,
                        w: 120,
                        h: 120,
                    }),
                raw_window(20)
                    .with_visible_index(1)
                    .with_pid(926)
                    .with_app_id("ai.perplexity.mac")
                    .with_title("right-pane")
                    .with_frame(Rect {
                        x: 220,
                        y: 0,
                        w: 120,
                        h: 120,
                    }),
            ],
        )]),
        current_space_id: Rc::new(RefCell::new(2)),
        calls: calls.clone(),
    };
    let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

    adapter.focus_direction_inner(Direction::West).unwrap();

    assert_eq!(
        take_calls(&calls),
        vec!["switch_space:1", "focus_window:20"]
    );
}

#[test]
fn backend_focus_direction_uses_same_post_switch_snapshot_for_selection_and_focus() {
    let _config = install_macos_native_focus_config("radial_center");
    let calls = Rc::new(RefCell::new(Vec::new()));
    let initial_topology = RawTopologySnapshot {
        spaces: vec![raw_desktop_space(1), raw_desktop_space(2)],
        active_space_ids: HashSet::from([1]),
        active_space_windows: HashMap::from([(
            1,
            vec![raw_window(10)
                .with_visible_index(0)
                .with_pid(1010)
                .with_app_id("com.example.source")
                .with_title("source")
                .with_frame(Rect {
                    x: 0,
                    y: 0,
                    w: 100,
                    h: 100,
                })],
        )]),
        inactive_space_window_ids: HashMap::from([(2, vec![21, 22])]),
        focused_window_id: Some(10),
    };
    let switched_topology = RawTopologySnapshot {
        spaces: vec![raw_desktop_space(1), raw_desktop_space(2)],
        active_space_ids: HashSet::from([2]),
        active_space_windows: HashMap::from([(
            2,
            vec![raw_window(21)
                .with_visible_index(0)
                .with_pid(2121)
                .with_app_id("com.example.visible")
                .with_title("visible")
                .with_frame(Rect {
                    x: 0,
                    y: 0,
                    w: 100,
                    h: 100,
                })],
        )]),
        inactive_space_window_ids: HashMap::new(),
        focused_window_id: Some(21),
    };
    let drifted_topology = RawTopologySnapshot {
        spaces: vec![raw_desktop_space(1), raw_desktop_space(2)],
        active_space_ids: HashSet::from([2]),
        active_space_windows: HashMap::from([(
            2,
            vec![raw_window(22)
                .with_visible_index(0)
                .with_pid(2222)
                .with_app_id("com.example.drifted")
                .with_title("drifted")
                .with_frame(Rect {
                    x: 240,
                    y: 0,
                    w: 100,
                    h: 100,
                })],
        )]),
        inactive_space_window_ids: HashMap::new(),
        focused_window_id: Some(22),
    };
    let api = PostSwitchSelectionDriftApi::new(
        initial_topology,
        switched_topology,
        drifted_topology
            .active_space_windows
            .get(&2)
            .cloned()
            .unwrap_or_default(),
        calls.clone(),
    );
    let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

    adapter.focus_direction_inner(Direction::East).unwrap();

    assert_eq!(
        take_calls(&calls),
        vec!["switch_space:2", "focus_window:21"]
    );
}

#[test]
fn backend_focus_direction_switches_then_focuses_rightmost_window_in_previous_space_when_no_west_window_exists(
) {
    let _config = install_macos_native_focus_config("radial_center");
    let calls = Rc::new(RefCell::new(Vec::new()));
    let topology = RawTopologySnapshot {
        spaces: vec![
            raw_desktop_space(1),
            raw_desktop_space(2),
            raw_desktop_space(3),
        ],
        active_space_ids: HashSet::from([2]),
        active_space_windows: HashMap::from([(
            2,
            vec![raw_window(20)
                .with_visible_index(0)
                .with_pid(2020)
                .with_app_id("com.example.center")
                .with_title("center")
                .with_frame(crate::engine::topology::Rect {
                    x: 120,
                    y: 0,
                    w: 100,
                    h: 100,
                })],
        )]),
        inactive_space_window_ids: HashMap::from([(1, vec![11, 12]), (3, vec![30])]),
        focused_window_id: Some(20),
    };
    let api = SwitchThenFocusApi {
        topology,
        switched_space_windows: HashMap::from([(
            1,
            vec![
                raw_window(11)
                    .with_visible_index(0)
                    .with_pid(1010)
                    .with_app_id("com.example.left")
                    .with_title("left")
                    .with_frame(Rect {
                        x: 0,
                        y: 0,
                        w: 100,
                        h: 100,
                    }),
                raw_window(12)
                    .with_visible_index(1)
                    .with_pid(1212)
                    .with_app_id("com.example.right")
                    .with_title("right")
                    .with_frame(Rect {
                        x: 240,
                        y: 0,
                        w: 100,
                        h: 100,
                    }),
            ],
        )]),
        current_space_id: Rc::new(RefCell::new(2)),
        calls: calls.clone(),
    };
    let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

    adapter.focus_direction_inner(Direction::West).unwrap();

    assert_eq!(
        take_calls(&calls),
        vec!["switch_space:1", "focus_window:12"]
    );
}

#[test]
fn backend_focus_direction_switches_then_focuses_window_in_previous_space_on_same_display_only() {
    let _config = install_macos_native_focus_config("radial_center");
    let calls = Rc::new(RefCell::new(Vec::new()));
    let topology = RawTopologySnapshot {
        spaces: vec![
            raw_desktop_space_on_display(1, 0),
            raw_desktop_space_on_display(2, 0),
            raw_desktop_space_on_display(10, 1),
            raw_desktop_space_on_display(11, 1),
        ],
        active_space_ids: HashSet::from([2, 11]),
        active_space_windows: HashMap::from([
            (
                2,
                vec![raw_window(200)
                    .with_pid(2200)
                    .with_app_id("com.example.left-display")
                    .with_title("left display")
                    .with_frame(crate::engine::topology::Rect {
                        x: 0,
                        y: 0,
                        w: 100,
                        h: 100,
                    })],
            ),
            (
                11,
                vec![raw_window(1100)
                    .with_visible_index(0)
                    .with_pid(1111)
                    .with_app_id("com.example.right-display")
                    .with_title("right display")
                    .with_frame(crate::engine::topology::Rect {
                        x: 120,
                        y: 0,
                        w: 100,
                        h: 100,
                    })],
            ),
        ]),
        inactive_space_window_ids: HashMap::from([(1, vec![100]), (10, vec![1000])]),
        focused_window_id: Some(1100),
    };
    let api = SwitchThenFocusApi {
        topology,
        switched_space_windows: HashMap::from([(
            10,
            vec![raw_window(1000)
                .with_visible_index(0)
                .with_pid(1001)
                .with_app_id("com.example.other-display")
                .with_title("other display")
                .with_frame(Rect {
                    x: 0,
                    y: 0,
                    w: 100,
                    h: 100,
                })],
        )]),
        current_space_id: Rc::new(RefCell::new(11)),
        calls: calls.clone(),
    };
    let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

    adapter.focus_direction_inner(Direction::West).unwrap();

    assert_eq!(
        take_calls(&calls),
        vec!["switch_space:10", "focus_window:1000"]
    );
}

#[test]
fn backend_focus_direction_switches_then_focuses_leftmost_window_in_next_space_when_no_east_window_exists(
) {
    let _config = install_macos_native_focus_config("radial_center");
    let calls = Rc::new(RefCell::new(Vec::new()));
    let topology = RawTopologySnapshot {
        spaces: vec![raw_desktop_space(1), raw_desktop_space(2)],
        active_space_ids: HashSet::from([1]),
        active_space_windows: HashMap::from([(
            1,
            vec![raw_window(10)
                .with_visible_index(0)
                .with_pid(1010)
                .with_app_id("com.example.source")
                .with_title("source")
                .with_frame(Rect {
                    x: 0,
                    y: 0,
                    w: 100,
                    h: 100,
                })],
        )]),
        inactive_space_window_ids: HashMap::from([(2, vec![21, 22])]),
        focused_window_id: Some(10),
    };
    let api = SwitchThenFocusApi {
        topology,
        switched_space_windows: HashMap::from([(
            2,
            vec![
                raw_window(21)
                    .with_pid(2121)
                    .with_app_id("com.example.left")
                    .with_title("left")
                    .with_frame(Rect {
                        x: 0,
                        y: 0,
                        w: 100,
                        h: 100,
                    }),
                raw_window(22)
                    .with_pid(2222)
                    .with_app_id("com.example.right")
                    .with_title("right")
                    .with_frame(Rect {
                        x: 240,
                        y: 0,
                        w: 100,
                        h: 100,
                    }),
            ],
        )]),
        current_space_id: Rc::new(RefCell::new(1)),
        calls: calls.clone(),
    };
    let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

    adapter.focus_direction_inner(Direction::East).unwrap();

    assert_eq!(
        take_calls(&calls),
        vec!["switch_space:2", "focus_window:21"]
    );
}

#[test]
fn backend_focus_direction_switches_then_focuses_edge_window_when_offspace_metadata_is_missing() {
    let _config = install_macos_native_focus_config("radial_center");
    let calls = Rc::new(RefCell::new(Vec::new()));
    let topology = RawTopologySnapshot {
        spaces: vec![raw_desktop_space(1), raw_desktop_space(2)],
        active_space_ids: HashSet::from([1]),
        active_space_windows: HashMap::from([(
            1,
            vec![raw_window(10)
                .with_visible_index(0)
                .with_pid(1010)
                .with_app_id("com.example.source")
                .with_title("source")
                .with_frame(Rect {
                    x: 0,
                    y: 0,
                    w: 100,
                    h: 100,
                })],
        )]),
        inactive_space_window_ids: HashMap::from([(2, vec![21, 22])]),
        focused_window_id: Some(10),
    };
    let api = SwitchThenFocusApi {
        topology,
        switched_space_windows: HashMap::from([(
            2,
            vec![
                raw_window(21)
                    .with_visible_index(1)
                    .with_pid(2121)
                    .with_app_id("com.example.left")
                    .with_title("left")
                    .with_frame(Rect {
                        x: 0,
                        y: 0,
                        w: 100,
                        h: 100,
                    }),
                raw_window(22)
                    .with_visible_index(0)
                    .with_pid(2222)
                    .with_app_id("com.example.right")
                    .with_title("right")
                    .with_frame(Rect {
                        x: 240,
                        y: 0,
                        w: 100,
                        h: 100,
                    }),
            ],
        )]),
        current_space_id: Rc::new(RefCell::new(1)),
        calls: calls.clone(),
    };
    let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

    adapter.focus_direction_inner(Direction::East).unwrap();

    assert_eq!(
        take_calls(&calls),
        vec!["switch_space:2", "focus_window:21"]
    );
}

#[test]
fn backend_focus_direction_can_switch_adjacent_space_without_direct_switch_primitive() {
    let _config = install_macos_native_focus_config("radial_center");
    let calls = Rc::new(RefCell::new(Vec::new()));
    let topology = RawTopologySnapshot {
        spaces: vec![raw_desktop_space(1), raw_desktop_space(2)],
        active_space_ids: HashSet::from([1]),
        active_space_windows: HashMap::from([(
            1,
            vec![raw_window(10)
                .with_visible_index(0)
                .with_pid(1010)
                .with_app_id("com.example.source")
                .with_title("source")
                .with_frame(Rect {
                    x: 0,
                    y: 0,
                    w: 100,
                    h: 100,
                })],
        )]),
        inactive_space_window_ids: HashMap::from([(2, vec![21, 22])]),
        focused_window_id: Some(10),
    };
    let api = AdjacentHotkeyOnlyApi {
        topology,
        switched_space_windows: HashMap::from([(
            2,
            vec![
                raw_window(21)
                    .with_visible_index(1)
                    .with_pid(2121)
                    .with_app_id("com.example.left")
                    .with_title("left")
                    .with_frame(Rect {
                        x: 0,
                        y: 0,
                        w: 100,
                        h: 100,
                    }),
                raw_window(22)
                    .with_visible_index(0)
                    .with_pid(2222)
                    .with_app_id("com.example.right")
                    .with_title("right")
                    .with_frame(Rect {
                        x: 240,
                        y: 0,
                        w: 100,
                        h: 100,
                    }),
            ],
        )]),
        current_space_id: Rc::new(RefCell::new(1)),
        calls: calls.clone(),
    };
    let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

    adapter.focus_direction_inner(Direction::East).unwrap();

    assert_eq!(take_calls(&calls), vec!["focus_window:21"]);
}

#[test]
fn backend_focus_direction_uses_exact_switch_for_empty_adjacent_space_when_hotkey_would_skip_it() {
    let _config = install_macos_native_focus_config("radial_center");
    let calls = Rc::new(RefCell::new(Vec::new()));
    let topology = RawTopologySnapshot {
        spaces: vec![
            raw_desktop_space(1),
            raw_desktop_space(2),
            raw_desktop_space(3),
        ],
        active_space_ids: HashSet::from([3]),
        active_space_windows: HashMap::from([(
            3,
            vec![raw_window(30)
                .with_visible_index(0)
                .with_pid(3030)
                .with_app_id("com.example.center")
                .with_title("center")
                .with_frame(crate::engine::topology::Rect {
                    x: 240,
                    y: 0,
                    w: 100,
                    h: 100,
                })],
        )]),
        inactive_space_window_ids: HashMap::from([(1, vec![10]), (2, vec![])]),
        focused_window_id: Some(30),
    };
    let api = EmptySpaceSkippingAdjacentHotkeyApi {
        topology,
        switched_space_windows: HashMap::from([(
            1,
            vec![raw_window(10)
                .with_visible_index(0)
                .with_pid(1010)
                .with_app_id("com.example.left")
                .with_title("left")
                .with_frame(crate::engine::topology::Rect {
                    x: 0,
                    y: 0,
                    w: 100,
                    h: 100,
                })],
        )]),
        current_space_id: Rc::new(RefCell::new(3)),
        adjacent_hotkey_skip_target_space_id: 1,
        calls: calls.clone(),
    };
    let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

    adapter.focus_direction_inner(Direction::West).unwrap();

    assert_eq!(take_calls(&calls), vec!["switch_space:2"]);
}

#[test]
fn backend_focus_direction_ignores_ghost_inactive_window_ids_for_empty_adjacent_space() {
    let _config = install_macos_native_focus_config("radial_center");
    let calls = Rc::new(RefCell::new(Vec::new()));
    let topology = RawTopologySnapshot {
        spaces: vec![
            raw_desktop_space(1),
            raw_desktop_space(2),
            raw_desktop_space(3),
        ],
        active_space_ids: HashSet::from([1]),
        active_space_windows: HashMap::from([(
            1,
            vec![raw_window(10)
                .with_visible_index(0)
                .with_pid(1010)
                .with_app_id("com.example.source")
                .with_title("source")
                .with_frame(crate::engine::topology::Rect {
                    x: 0,
                    y: 0,
                    w: 100,
                    h: 100,
                })],
        )]),
        inactive_space_window_ids: HashMap::from([(2, vec![31, 32]), (3, vec![])]),
        focused_window_id: Some(10),
    };
    let api = EmptySpaceSkippingAdjacentHotkeyApi {
        topology,
        switched_space_windows: HashMap::from([(
            3,
            vec![
                raw_window(31)
                    .with_visible_index(1)
                    .with_pid(3131)
                    .with_app_id("com.example.skip-left")
                    .with_title("skip-left")
                    .with_frame(crate::engine::topology::Rect {
                        x: 240,
                        y: 0,
                        w: 100,
                        h: 100,
                    }),
                raw_window(32)
                    .with_visible_index(0)
                    .with_pid(3232)
                    .with_app_id("com.example.skip-right")
                    .with_title("skip-right")
                    .with_frame(crate::engine::topology::Rect {
                        x: 360,
                        y: 0,
                        w: 100,
                        h: 100,
                    }),
            ],
        )]),
        current_space_id: Rc::new(RefCell::new(1)),
        adjacent_hotkey_skip_target_space_id: 3,
        calls: calls.clone(),
    };
    let adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

    adapter.focus_direction_inner(Direction::East).unwrap();

    assert_eq!(
        take_calls(&calls),
        vec!["switch_adjacent_space:east:2", "switch_space:2"]
    );
}

#[test]
fn backend_move_direction_swaps_with_directional_neighbor() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let topology = RawTopologySnapshot {
        spaces: vec![raw_desktop_space(1)],
        active_space_ids: HashSet::from([1]),
        active_space_windows: HashMap::from([(
            1,
            vec![
                raw_window(10)
                    .with_pid(1010)
                    .with_title("left")
                    .with_frame(Rect {
                        x: 0,
                        y: 0,
                        w: 100,
                        h: 100,
                    }),
                raw_window(20)
                    .with_pid(2020)
                    .with_title("center")
                    .with_frame(Rect {
                        x: 120,
                        y: 0,
                        w: 100,
                        h: 100,
                    }),
                raw_window(30)
                    .with_pid(3030)
                    .with_title("right")
                    .with_frame(Rect {
                        x: 240,
                        y: 0,
                        w: 100,
                        h: 100,
                    }),
            ],
        )]),
        inactive_space_window_ids: HashMap::new(),
        focused_window_id: Some(20),
    };
    let api = SendRecordingApi {
        topology,
        calls: calls.clone(),
    };
    let mut adapter = MacosNativeAdapter::connect_with_api(api).unwrap();

    adapter.move_direction(Direction::East).unwrap();

    assert_eq!(
        std::mem::take(&mut *calls.lock().unwrap()),
        vec!["swap_window_frames:20:30"]
    );
}
