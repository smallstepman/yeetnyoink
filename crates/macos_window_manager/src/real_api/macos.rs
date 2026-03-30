use std::{
    collections::{HashMap, HashSet},
    ffi::{c_void, CString},
};

use crate::{
    active_space_ax_backed_same_pid_target, ax, confirm_focus_after_missing_ax_target,
    desktop_topology_snapshot::{
        self, RawSpaceRecord, RawTopologySnapshot, RawWindow, SpaceKind, DESKTOP_SPACE_TYPE,
        FULLSCREEN_SPACE_TYPE,
    },
    focus_window_via_make_key_and_raise, focus_window_via_process_and_raise,
    foundation::{
        switch_adjacent_space_via_hotkey, CFArrayRef, DylibHandle, SlsMainConnectionIdFn,
        HISERVICES_FRAMEWORK_PATH, SKYLIGHT_FRAMEWORK_PATH,
    },
    shim::SwiftBackendShim,
    skylight,
    window_server::{
        self, copy_onscreen_window_descriptions_raw, onscreen_window_ids_from_descriptions,
    },
    ActiveSpaceFocusTargetHint, MacosNativeApi, MacosNativeOperationError, MacosNativeProbeError,
    NativeBackendOptions, NativeBounds, NativeDesktopSnapshot, NativeDirection, NativeWindowId,
};

pub struct RealNativeApi {
    skylight: Option<DylibHandle>,
    hiservices: Option<DylibHandle>,
    swift_backend: Result<SwiftBackendShim, crate::MacosNativeBridgeError>,
    options: NativeBackendOptions,
}

#[cfg(target_os = "macos")]
impl RealNativeApi {
    pub fn new(options: NativeBackendOptions) -> Self {
        Self {
            skylight: DylibHandle::open(SKYLIGHT_FRAMEWORK_PATH),
            hiservices: DylibHandle::open(HISERVICES_FRAMEWORK_PATH),
            swift_backend: SwiftBackendShim::new(),
            options,
        }
    }

    pub(crate) fn resolve_symbol(&self, symbol: &'static str) -> Option<*mut c_void> {
        let symbol = CString::new(symbol).expect("required symbol names should not contain NULs");

        self.skylight
            .as_ref()
            .and_then(|handle| handle.resolve(symbol.as_c_str()))
            .or_else(|| {
                self.hiservices
                    .as_ref()
                    .and_then(|handle| handle.resolve(symbol.as_c_str()))
            })
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
}

#[cfg(target_os = "macos")]
impl MacosNativeApi for RealNativeApi {
    fn has_symbol(&self, symbol: &'static str) -> bool {
        self.resolve_symbol(symbol).is_some()
    }

    fn debug(&self, message: &str) {
        RealNativeApi::debug(self, message);
    }

    fn ax_is_trusted(&self) -> bool {
        ax::is_process_trusted(self)
    }

    fn minimal_topology_ready(&self) -> bool {
        let Some(symbol) = self.resolve_symbol("SLSMainConnectionID") else {
            return false;
        };

        let main_connection_id: SlsMainConnectionIdFn = unsafe { std::mem::transmute(symbol) };
        unsafe { main_connection_id() != 0 }
    }

    fn validate_environment(&self) -> Result<(), crate::MacosNativeConnectError> {
        self.swift_backend
            .as_ref()
            .map_err(|_| {
                crate::MacosNativeConnectError::MissingTopologyPrecondition("swift macOS backend")
            })?
            .validate_environment()
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
        let descriptions = copy_onscreen_window_descriptions_raw()?;
        onscreen_window_ids_from_descriptions(descriptions.as_type_ref() as CFArrayRef)
    }

    fn switch_space(&self, space_id: u64) -> Result<(), MacosNativeOperationError> {
        skylight::switch_space(self, space_id)
    }

    fn switch_adjacent_space(
        &self,
        direction: NativeDirection,
        _space_id: u64,
    ) -> Result<(), MacosNativeOperationError> {
        self.debug(&format!(
            "macos_native: switching adjacent space via mission-control hotkey direction={direction}"
        ));
        switch_adjacent_space_via_hotkey(&self.options, direction, |key_code, key_down, flags| {
            window_server::post_keyboard_event(self, key_code, key_down, flags)
        })
    }

    fn focus_window(&self, window_id: u64) -> Result<(), MacosNativeOperationError> {
        window_server::focus_window(self, window_id)
    }

    fn focus_window_with_known_pid(
        &self,
        window_id: u64,
        pid: u32,
    ) -> Result<(), MacosNativeOperationError> {
        match focus_window_via_process_and_raise(
            window_id,
            |_| Ok(pid),
            |resolved_pid| window_server::process_serial_number_for_pid(self, resolved_pid),
            |psn, target_window_id| {
                window_server::front_process_window(self, psn, target_window_id)
            },
            |psn, target_window_id| window_server::make_key_window(self, psn, target_window_id),
            |target_window_id, resolved_pid| {
                ax::raise_window_via_ax(self, target_window_id, resolved_pid)
            },
        ) {
            Err(MacosNativeOperationError::MissingWindow(missing_window_id))
                if missing_window_id == window_id =>
            {
                if confirm_focus_after_missing_ax_target(window_id, || {
                    ax::probe_focused_window_id(self)
                }) {
                    self.debug(&format!(
                        "macos_native: treating missing AX raise target {window_id} as success after focus confirmation"
                    ));
                    return Ok(());
                }
                self.debug(&format!(
                    "macos_native: AX raise still missing target {window_id} after retries; focused_window_id={:?}",
                    ax::probe_focused_window_id(self).ok().flatten()
                ));
                Err(MacosNativeOperationError::MissingWindow(window_id))
            }
            other => other,
        }
    }

    fn ax_window_ids_for_pid(&self, pid: u32) -> Result<Vec<u64>, MacosNativeOperationError> {
        ax::ax_window_ids_for_pid(self, pid)
    }

    fn focus_window_in_active_space_with_known_pid(
        &self,
        window_id: u64,
        pid: u32,
        target_hint: Option<ActiveSpaceFocusTargetHint>,
    ) -> Result<(), MacosNativeOperationError> {
        match focus_window_via_make_key_and_raise(
            window_id,
            |_| Ok(pid),
            |resolved_pid| window_server::process_serial_number_for_pid(self, resolved_pid),
            |psn, target_window_id| window_server::make_key_window(self, psn, target_window_id),
            |target_window_id, resolved_pid| {
                ax::raise_window_via_ax(self, target_window_id, resolved_pid)
            },
        ) {
            Err(MacosNativeOperationError::MissingWindow(missing_window_id))
                if missing_window_id == window_id =>
            {
                if confirm_focus_after_missing_ax_target(window_id, || {
                    ax::probe_focused_window_id(self)
                }) {
                    self.debug(&format!(
                        "macos_native: treating missing active-space AX raise target {window_id} as success after focus confirmation"
                    ));
                    return Ok(());
                }
                if let Some(remapped_target_id) = active_space_ax_backed_same_pid_target(
                    self,
                    &desktop_topology_snapshot::native_desktop_snapshot_from_topology(
                        &self.topology_snapshot()?,
                    ),
                    window_id,
                    pid,
                    target_hint,
                )? {
                    self.debug(&format!(
                        "macos_native: active-space focus remapped stale same-pid target {} to {}",
                        window_id, remapped_target_id
                    ));
                    return self.focus_window_with_known_pid(remapped_target_id, pid);
                }
                self.debug(&format!(
                    "macos_native: active-space AX raise still missing target {window_id} after retries; focused_window_id={:?}",
                    ax::probe_focused_window_id(self).ok().flatten()
                ));
                Err(MacosNativeOperationError::MissingWindow(window_id))
            }
            other => other,
        }
    }

    fn move_window_to_space(
        &self,
        window_id: u64,
        space_id: u64,
    ) -> Result<(), MacosNativeOperationError> {
        skylight::move_window_to_space(self, window_id, space_id)
    }

    fn swap_window_frames(
        &self,
        source_window_id: u64,
        source_frame: NativeBounds,
        target_window_id: u64,
        target_frame: NativeBounds,
    ) -> Result<(), MacosNativeOperationError> {
        ax::swap_window_frames(
            self,
            source_window_id,
            source_frame,
            target_window_id,
            target_frame,
        )
    }

    fn focused_window_id(&self) -> Result<Option<NativeWindowId>, MacosNativeProbeError> {
        ax::probe_focused_window_id(self)
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
}

#[cfg(target_os = "macos")]
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

#[cfg(target_os = "macos")]
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
