use std::collections::{HashMap, HashSet};

use crate::{
    ActiveSpaceFocusTargetHint, MacosNativeConnectError, MacosNativeFastFocusError,
    MacosNativeOperationError, MacosNativeProbeError, MacosWindowManagerBackend,
    NativeBackendOptions, NativeBounds, NativeDesktopSnapshot, NativeDirection,
    NativeFastFocusContext, NativeWindowId,
    desktop_topology_snapshot::{
        DESKTOP_SPACE_TYPE, FULLSCREEN_SPACE_TYPE, RawSpaceRecord, RawTopologySnapshot, RawWindow,
        SpaceKind,
    },
    shim::SwiftBackendShim,
    transport::{
        MWM_STATUS_CONNECT_MISSING_REQUIRED_SYMBOL,
        MWM_STATUS_CONNECT_MISSING_ACCESSIBILITY_PERMISSION,
        MWM_STATUS_CONNECT_MISSING_TOPOLOGY_PRECONDITION, MWM_STATUS_PROBE_MISSING_TOPOLOGY,
    },
};

pub struct SwiftMacosBackend {
    swift_backend: Result<SwiftBackendShim, crate::MacosNativeBridgeError>,
    options: NativeBackendOptions,
}

impl SwiftMacosBackend {
    pub fn new(options: NativeBackendOptions) -> Self {
        Self {
            swift_backend: SwiftBackendShim::new(),
            options,
        }
    }

    pub(crate) fn debug(&self, message: impl AsRef<str>) {
        if let Some(diagnostics) = self.options.diagnostics.as_ref() {
            diagnostics.debug(message.as_ref());
        }
    }

    fn topology_native_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
        self.swift_backend
            .as_ref()
            .map_err(|_| MacosNativeProbeError::MissingTopology("swift macOS backend"))?
            .topology_snapshot_native()
    }

    fn topology_snapshot_from_swift(&self) -> Result<RawTopologySnapshot, MacosNativeProbeError> {
        let snapshot = self.topology_native_snapshot()?;
        let spaces = snapshot
            .spaces
            .iter()
            .map(space_record_from_snapshot)
            .collect();
        let active_space_ids = snapshot.active_space_ids.clone();
        let mut active_space_windows = HashMap::<u64, Vec<RawWindow>>::new();
        let mut inactive_space_window_ids = HashMap::<u64, Vec<u64>>::new();

        for window in snapshot.windows {
            if window.order_index.is_some() {
                active_space_windows
                    .entry(window.space_id)
                    .or_default()
                    .push(raw_window_from_snapshot(window));
            } else {
                inactive_space_window_ids
                    .entry(window.space_id)
                    .or_default()
                    .push(window.id);
            }
        }

        Ok(RawTopologySnapshot {
            spaces,
            active_space_ids,
            active_space_windows,
            inactive_space_window_ids,
            focused_window_id: snapshot.focused_window_id,
        })
    }

    fn swift_backend_for_action(&self) -> Result<&SwiftBackendShim, MacosNativeOperationError> {
        self.swift_backend
            .as_ref()
            .map_err(|_| MacosNativeOperationError::CallFailed("swift macOS backend"))
    }

    fn connect_state(&self) -> Result<(), MacosNativeConnectError> {
        self.swift_backend
            .as_ref()
            .map_err(|_| {
                MacosNativeConnectError::MissingTopologyPrecondition("swift macOS backend")
            })?
            .validate_environment()
    }

    pub(crate) fn prepare_fast_focus_context(
        &self,
    ) -> Result<NativeFastFocusContext, crate::MacosNativeBridgeError> {
        self.swift_backend
            .as_ref()
            .map_err(Clone::clone)?
            .prepare_fast_focus_context()
    }
}

fn fast_focus_error_from_bridge(error: crate::MacosNativeBridgeError) -> MacosNativeFastFocusError {
    match error {
        crate::MacosNativeBridgeError::BackendStatus { code, message } => match code {
            MWM_STATUS_CONNECT_MISSING_REQUIRED_SYMBOL => {
                MacosNativeFastFocusError::Connect(MacosNativeConnectError::MissingRequiredSymbol(
                    crate::shim::static_message(message),
                ))
            }
            MWM_STATUS_CONNECT_MISSING_ACCESSIBILITY_PERMISSION => {
                MacosNativeFastFocusError::Connect(
                    MacosNativeConnectError::MissingAccessibilityPermission,
                )
            }
            MWM_STATUS_CONNECT_MISSING_TOPOLOGY_PRECONDITION => {
                MacosNativeFastFocusError::Connect(
                    MacosNativeConnectError::MissingTopologyPrecondition(
                        crate::shim::static_message(message),
                    ),
                )
            }
            MWM_STATUS_PROBE_MISSING_TOPOLOGY => {
                MacosNativeFastFocusError::Probe(MacosNativeProbeError::MissingTopology(
                    crate::shim::static_message(message),
                ))
            }
            _ => MacosNativeFastFocusError::Bridge(crate::MacosNativeBridgeError::BackendStatus {
                code,
                message,
            }),
        },
        other => MacosNativeFastFocusError::Bridge(other),
    }
}

impl MacosWindowManagerBackend for SwiftMacosBackend {
    fn has_symbol(&self, _symbol: &'static str) -> bool {
        self.swift_backend.is_ok()
    }

    fn debug(&self, message: &str) {
        SwiftMacosBackend::debug(self, message);
    }

    fn ax_is_trusted(&self) -> bool {
        !matches!(
            self.connect_state(),
            Err(MacosNativeConnectError::MissingAccessibilityPermission)
        )
    }

    fn minimal_topology_ready(&self) -> bool {
        !matches!(
            self.connect_state(),
            Err(MacosNativeConnectError::MissingTopologyPrecondition(_))
        )
    }

    fn validate_environment(&self) -> Result<(), crate::MacosNativeConnectError> {
        self.connect_state()
    }

    fn prepare_fast_focus_context(
        &self,
    ) -> Result<NativeFastFocusContext, MacosNativeFastFocusError> {
        SwiftMacosBackend::prepare_fast_focus_context(self).map_err(fast_focus_error_from_bridge)
    }

    fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
        self.swift_backend
            .as_ref()
            .map_err(|_| MacosNativeProbeError::MissingTopology("swift macOS backend"))?
            .desktop_snapshot_native()
    }

    fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
        Ok(self.topology_snapshot_from_swift()?.spaces)
    }

    fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
        Ok(self.topology_native_snapshot()?.active_space_ids)
    }

    fn active_space_windows(&self, space_id: u64) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
        let topology = self.topology_snapshot_from_swift()?;
        Ok(topology
            .active_space_windows
            .get(&space_id)
            .cloned()
            .unwrap_or_default())
    }

    fn inactive_space_window_ids(&self) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
        Ok(self
            .topology_snapshot_from_swift()?
            .inactive_space_window_ids)
    }

    fn onscreen_window_ids(&self) -> Result<HashSet<NativeWindowId>, MacosNativeProbeError> {
        Ok(self
            .topology_native_snapshot()?
            .windows
            .into_iter()
            .filter(|window| window.order_index.is_some())
            .map(|window| window.id)
            .collect())
    }

    fn switch_space(&self, space_id: u64) -> Result<(), MacosNativeOperationError> {
        self.swift_backend_for_action()?.switch_space(space_id)
    }

    fn switch_adjacent_space(
        &self,
        direction: NativeDirection,
        space_id: u64,
    ) -> Result<(), MacosNativeOperationError> {
        self.swift_backend_for_action()?
            .switch_adjacent_space(direction, space_id)
    }

    fn focus_window(&self, window_id: u64) -> Result<(), MacosNativeOperationError> {
        self.swift_backend_for_action()?.focus_window(window_id)
    }

    fn focus_window_with_known_pid(
        &self,
        window_id: u64,
        pid: u32,
    ) -> Result<(), MacosNativeOperationError> {
        self.swift_backend_for_action()?
            .focus_window_with_known_pid(window_id, pid)
    }

    fn ax_window_ids_for_pid(&self, pid: u32) -> Result<Vec<u64>, MacosNativeOperationError> {
        self.topology_native_snapshot()
            .map(|snapshot| {
                snapshot
                    .windows
                    .into_iter()
                    .filter(|window| window.pid == Some(pid))
                    .map(|window| window.id)
                    .collect()
            })
            .map_err(MacosNativeOperationError::from)
    }

    fn focus_window_in_active_space_with_known_pid(
        &self,
        window_id: u64,
        pid: u32,
        target_hint: Option<ActiveSpaceFocusTargetHint>,
    ) -> Result<(), MacosNativeOperationError> {
        self.swift_backend_for_action()?
            .focus_window_in_active_space_with_known_pid(
                window_id,
                pid,
                target_hint.map(|hint| (hint.space_id, hint.bounds)),
            )
    }

    fn move_window_to_space(
        &self,
        window_id: u64,
        space_id: u64,
    ) -> Result<(), MacosNativeOperationError> {
        self.swift_backend_for_action()?
            .move_window_to_space(window_id, space_id)
    }

    fn swap_window_frames(
        &self,
        source_window_id: u64,
        source_frame: NativeBounds,
        target_window_id: u64,
        target_frame: NativeBounds,
    ) -> Result<(), MacosNativeOperationError> {
        self.swift_backend_for_action()?.swap_window_frames(
            source_window_id,
            source_frame,
            target_window_id,
            target_frame,
        )
    }

    fn focused_window_id(&self) -> Result<Option<NativeWindowId>, MacosNativeProbeError> {
        Ok(self.topology_native_snapshot()?.focused_window_id)
    }

    fn topology_snapshot(&self) -> Result<RawTopologySnapshot, MacosNativeProbeError> {
        self.topology_snapshot_from_swift()
    }

    fn topology_snapshot_without_focus(
        &self,
    ) -> Result<RawTopologySnapshot, MacosNativeProbeError> {
        let mut topology = self.topology_snapshot_from_swift()?;
        topology.focused_window_id = None;
        Ok(topology)
    }

    fn switch_space_in_snapshot(
        &self,
        snapshot: &NativeDesktopSnapshot,
        space_id: u64,
        adjacent_direction: Option<NativeDirection>,
    ) -> Result<(), MacosNativeOperationError> {
        self.swift_backend_for_action()?.switch_space_in_snapshot(
            snapshot,
            space_id,
            adjacent_direction,
        )
    }

    fn focus_same_space_target_in_snapshot(
        &self,
        snapshot: &NativeDesktopSnapshot,
        direction: NativeDirection,
        target_window_id: u64,
    ) -> Result<(), MacosNativeOperationError> {
        self.swift_backend_for_action()?
            .focus_same_space_target_in_snapshot(snapshot, direction, target_window_id)
    }
}

fn space_record_from_snapshot(space: &crate::NativeSpaceSnapshot) -> RawSpaceRecord {
    let (space_type, has_tile_layout_manager, stage_manager_managed) = match space.kind {
        SpaceKind::Desktop => (DESKTOP_SPACE_TYPE, false, false),
        SpaceKind::Fullscreen => (FULLSCREEN_SPACE_TYPE, false, false),
        SpaceKind::SplitView => (DESKTOP_SPACE_TYPE, true, false),
        SpaceKind::System => (-1, false, false),
        SpaceKind::StageManagerOpaque => (DESKTOP_SPACE_TYPE, false, true),
    };

    RawSpaceRecord {
        managed_space_id: space.id,
        display_index: space.display_index,
        space_type,
        tile_spaces: Vec::new(),
        has_tile_layout_manager,
        stage_manager_managed,
    }
}

fn raw_window_from_snapshot(window: crate::NativeWindowSnapshot) -> RawWindow {
    RawWindow {
        id: window.id,
        pid: window.pid,
        app_id: window.app_id,
        title: window.title,
        level: window.level,
        visible_index: window.order_index,
        frame: window.bounds,
    }
}

#[cfg(test)]
mod tests {
    use std::{ffi::CString, mem::ManuallyDrop};

    use super::SwiftMacosBackend;
    use crate::{
        MacosNativeBridgeError, MacosNativeFastFocusError, MacosWindowManagerBackend,
        MissionControlHotkey, MissionControlModifiers, NativeBackendOptions,
        shim::{self, SwiftBackendShim},
        transport::{
            MWM_STATUS_CONNECT_MISSING_REQUIRED_SYMBOL,
            MWM_STATUS_CONNECT_MISSING_ACCESSIBILITY_PERMISSION,
            MWM_STATUS_PROBE_MISSING_TOPOLOGY, MwmFastFocusContextAbi, MwmStatus,
        },
    };

    fn test_backend_with_bridge_error(error: MacosNativeBridgeError) -> SwiftMacosBackend {
        SwiftMacosBackend {
            swift_backend: Err(error),
            options: NativeBackendOptions {
                west_space_hotkey: MissionControlHotkey {
                    key_code: 123,
                    mission_control: MissionControlModifiers::default(),
                },
                east_space_hotkey: MissionControlHotkey {
                    key_code: 124,
                    mission_control: MissionControlModifiers::default(),
                },
                diagnostics: None,
            },
        }
    }

    fn test_backend_with_shim(shim: SwiftBackendShim) -> SwiftMacosBackend {
        SwiftMacosBackend {
            swift_backend: Ok(shim),
            options: NativeBackendOptions {
                west_space_hotkey: MissionControlHotkey {
                    key_code: 123,
                    mission_control: MissionControlModifiers::default(),
                },
                east_space_hotkey: MissionControlHotkey {
                    key_code: 124,
                    mission_control: MissionControlModifiers::default(),
                },
                diagnostics: None,
            },
        }
    }

    #[test]
    fn prepare_fast_focus_context_uses_swift_override_instead_of_trait_default() {
        let backend = test_backend_with_bridge_error(MacosNativeBridgeError::NullBackendHandle);

        let error = <SwiftMacosBackend as MacosWindowManagerBackend>::prepare_fast_focus_context(
            &backend,
        )
        .unwrap_err();

        assert_eq!(
            error,
            MacosNativeFastFocusError::Bridge(MacosNativeBridgeError::NullBackendHandle)
        );
    }

    #[test]
    fn prepare_fast_focus_context_rehydrates_swift_connect_and_probe_statuses() {
        let _guard = shim::ffi_test_guard();
        crate::ffi::test_support::reset();
        crate::ffi::test_support::set_fast_focus_context_response(
            crate::ffi::TestFastFocusContextResponse {
                code: MWM_STATUS_CONNECT_MISSING_ACCESSIBILITY_PERMISSION,
                context: MwmFastFocusContextAbi::empty(),
                status: MwmStatus {
                    code: MWM_STATUS_CONNECT_MISSING_ACCESSIBILITY_PERMISSION,
                    message_ptr: std::ptr::null_mut(),
                },
            },
        );
        let backend =
            ManuallyDrop::new(test_backend_with_shim(SwiftBackendShim::dangling_for_test()));
        let connect_error =
            <SwiftMacosBackend as MacosWindowManagerBackend>::prepare_fast_focus_context(&backend)
                .unwrap_err();
        assert_eq!(
            connect_error,
            MacosNativeFastFocusError::Connect(
                crate::MacosNativeConnectError::MissingAccessibilityPermission
            )
        );

        crate::ffi::test_support::reset();
        crate::ffi::test_support::set_fast_focus_context_response(
            crate::ffi::TestFastFocusContextResponse {
                code: MWM_STATUS_CONNECT_MISSING_REQUIRED_SYMBOL,
                context: MwmFastFocusContextAbi::empty(),
                status: MwmStatus {
                    code: MWM_STATUS_CONNECT_MISSING_REQUIRED_SYMBOL,
                    message_ptr: CString::new("SLSMainConnectionID").unwrap().into_raw(),
                },
            },
        );
        let backend =
            ManuallyDrop::new(test_backend_with_shim(SwiftBackendShim::dangling_for_test()));
        let required_symbol_error =
            <SwiftMacosBackend as MacosWindowManagerBackend>::prepare_fast_focus_context(&backend)
                .unwrap_err();
        assert_eq!(
            required_symbol_error,
            MacosNativeFastFocusError::Connect(
                crate::MacosNativeConnectError::MissingRequiredSymbol("SLSMainConnectionID")
            )
        );

        crate::ffi::test_support::reset();
        crate::ffi::test_support::set_fast_focus_context_response(
            crate::ffi::TestFastFocusContextResponse {
                code: MWM_STATUS_PROBE_MISSING_TOPOLOGY,
                context: MwmFastFocusContextAbi::empty(),
                status: MwmStatus {
                    code: MWM_STATUS_PROBE_MISSING_TOPOLOGY,
                    message_ptr: std::ptr::null_mut(),
                },
            },
        );
        let backend =
            ManuallyDrop::new(test_backend_with_shim(SwiftBackendShim::dangling_for_test()));
        let probe_error =
            <SwiftMacosBackend as MacosWindowManagerBackend>::prepare_fast_focus_context(&backend)
                .unwrap_err();
        assert_eq!(
            probe_error,
            MacosNativeFastFocusError::Probe(crate::MacosNativeProbeError::MissingTopology(
                "swift macOS backend"
            ))
        );
    }
}
