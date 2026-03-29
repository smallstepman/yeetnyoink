use std::{
    collections::{HashMap, HashSet},
    ffi::{CString, c_void},
    time::Instant,
};

use crate::{
    AX_RAISE_RETRY_INTERVAL, AX_RAISE_SETTLE_TIMEOUT, ActiveSpaceFocusTargetHint, MacosNativeApi,
    MacosNativeOperationError, MacosNativeProbeError, NativeBackendOptions, NativeBounds,
    NativeDesktopSnapshot, NativeDirection, NativeWindowId, active_space_ax_backed_same_pid_target,
    ax,
    desktop_topology_snapshot::{
        RawSpaceRecord, RawTopologySnapshot, RawWindow, WindowSnapshot, active_window_snapshot,
        focused_window_from_active_space_windows, native_desktop_snapshot_from_topology,
        stable_app_id_from_real_window,
    },
    focus_window_via_make_key_and_raise, focus_window_via_process_and_raise,
    foundation::{
        CFArrayRef, DylibHandle, HISERVICES_FRAMEWORK_PATH, SKYLIGHT_FRAMEWORK_PATH,
        SlsMainConnectionIdFn, switch_adjacent_space_via_hotkey,
    },
    skylight::{
        self, parse_active_space_ids, parse_display_identifiers, parse_managed_spaces,
        parse_window_ids,
    },
    window_server::{
        self, assemble_real_active_space_windows, copy_onscreen_window_descriptions_raw,
        onscreen_window_ids_from_descriptions, query_visible_window_order,
    },
};

pub struct RealNativeApi {
    skylight: Option<DylibHandle>,
    hiservices: Option<DylibHandle>,
    options: NativeBackendOptions,
}

#[cfg(target_os = "macos")]
impl RealNativeApi {
    pub fn new(options: NativeBackendOptions) -> Self {
        Self {
            skylight: DylibHandle::open(SKYLIGHT_FRAMEWORK_PATH),
            hiservices: DylibHandle::open(HISERVICES_FRAMEWORK_PATH),
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

    fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
        Ok(native_desktop_snapshot_from_topology(
            &self.topology_snapshot()?,
        ))
    }

    fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
        let payload = skylight::copy_managed_display_spaces_raw(self)?;
        parse_managed_spaces(payload.as_type_ref() as CFArrayRef)
    }

    fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
        let payload = skylight::copy_managed_display_spaces_raw(self)?;
        let display_identifiers = parse_display_identifiers(payload.as_type_ref() as CFArrayRef)?;
        let active_space_ids = display_identifiers
            .into_iter()
            .map(|display_identifier| {
                skylight::current_space_for_display(self, &display_identifier)
            })
            .collect::<Result<HashSet<_>, _>>()?;

        (!active_space_ids.is_empty())
            .then_some(active_space_ids)
            .ok_or(MacosNativeProbeError::MissingTopology(
                "SLSManagedDisplayGetCurrentSpace",
            ))
    }

    fn active_space_windows(&self, space_id: u64) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
        let payload = skylight::copy_windows_for_space_raw(self, space_id)?;
        let visible_order =
            query_visible_window_order(&parse_window_ids(payload.as_type_ref() as CFArrayRef)?)?;
        let descriptions =
            window_server::copy_window_descriptions_raw(self, payload.as_type_ref() as CFArrayRef)?;

        assemble_real_active_space_windows(descriptions.as_type_ref() as CFArrayRef, &visible_order)
    }

    fn inactive_space_window_ids(&self) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
        let spaces = self.managed_spaces()?;
        let active_space_ids = self.active_space_ids()?;
        let mut inactive_space_window_ids = HashMap::new();

        for space in spaces {
            if active_space_ids.contains(&space.managed_space_id) {
                continue;
            }

            let payload = skylight::copy_windows_for_space_raw(self, space.managed_space_id)?;
            inactive_space_window_ids.insert(
                space.managed_space_id,
                parse_window_ids(payload.as_type_ref() as CFArrayRef)?,
            );
        }

        Ok(inactive_space_window_ids)
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
                let deadline = Instant::now() + AX_RAISE_SETTLE_TIMEOUT;
                loop {
                    if self.focused_window_id().ok() == Some(Some(window_id)) {
                        self.debug(&format!(
                            "macos_native: treating missing AX raise target {window_id} as success after focus confirmation"
                        ));
                        return Ok(());
                    }
                    if Instant::now() >= deadline {
                        break;
                    }
                    std::thread::sleep(AX_RAISE_RETRY_INTERVAL);
                }
                self.debug(&format!(
                    "macos_native: AX raise still missing target {window_id} after retries; focused_window_id={:?}",
                    self.focused_window_id().ok().flatten()
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
                let deadline = Instant::now() + AX_RAISE_SETTLE_TIMEOUT;
                loop {
                    if self.focused_window_id().ok() == Some(Some(window_id)) {
                        self.debug(&format!(
                            "macos_native: treating missing active-space AX raise target {window_id} as success after focus confirmation"
                        ));
                        return Ok(());
                    }
                    if Instant::now() >= deadline {
                        break;
                    }
                    std::thread::sleep(AX_RAISE_RETRY_INTERVAL);
                }
                if let Some(remapped_target_id) = active_space_ax_backed_same_pid_target(
                    self,
                    &self.desktop_snapshot()?,
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
                    self.focused_window_id().ok().flatten()
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

    fn focused_window_snapshot(&self) -> Result<WindowSnapshot, MacosNativeProbeError> {
        let focused_window_id = ax::probe_focused_window_id(self)?;
        let active_space_ids = self.active_space_ids()?;
        let mut active_space_windows = HashMap::new();

        for space_id in active_space_ids {
            let windows = window_server::active_space_windows_without_app_ids(self, space_id)?;
            if let Some(target_window_id) = focused_window_id {
                if let Some(mut snapshot) =
                    active_window_snapshot(space_id, &windows, target_window_id)
                {
                    snapshot.app_id = snapshot
                        .app_id
                        .or_else(|| stable_app_id_from_real_window(snapshot.pid, None));
                    return Ok(snapshot);
                }
            }
            active_space_windows.insert(space_id, windows);
        }

        let mut snapshot =
            focused_window_from_active_space_windows(&active_space_windows, focused_window_id)?;
        snapshot.app_id = snapshot
            .app_id
            .or_else(|| stable_app_id_from_real_window(snapshot.pid, None));
        Ok(snapshot)
    }

    fn topology_snapshot(&self) -> Result<RawTopologySnapshot, MacosNativeProbeError> {
        let mut topology = self.topology_snapshot_without_focus()?;
        topology.focused_window_id = self.focused_window_id()?;
        Ok(topology)
    }

    fn topology_snapshot_without_focus(
        &self,
    ) -> Result<RawTopologySnapshot, MacosNativeProbeError> {
        let payload = skylight::copy_managed_display_spaces_raw(self)?;
        let payload = payload.as_type_ref() as CFArrayRef;
        let spaces = parse_managed_spaces(payload)?;
        let active_space_ids = parse_active_space_ids(payload)?;
        let mut active_space_windows = HashMap::new();
        let mut inactive_space_window_ids = HashMap::new();

        for space in &spaces {
            let payload = skylight::copy_windows_for_space_raw(self, space.managed_space_id)?;
            let raw_window_ids = parse_window_ids(payload.as_type_ref() as CFArrayRef)?;

            if active_space_ids.contains(&space.managed_space_id) {
                let visible_order = query_visible_window_order(&raw_window_ids)?;
                let descriptions = window_server::copy_window_descriptions_raw(
                    self,
                    payload.as_type_ref() as CFArrayRef,
                )?;
                let windows = assemble_real_active_space_windows(
                    descriptions.as_type_ref() as CFArrayRef,
                    &visible_order,
                )?;

                active_space_windows.insert(space.managed_space_id, windows);
            } else {
                inactive_space_window_ids.insert(space.managed_space_id, raw_window_ids);
            }
        }

        Ok(RawTopologySnapshot {
            spaces,
            active_space_ids,
            active_space_windows,
            inactive_space_window_ids,
            focused_window_id: None,
        })
    }
}
