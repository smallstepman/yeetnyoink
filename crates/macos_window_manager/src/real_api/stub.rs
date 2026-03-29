use std::collections::HashMap;

use crate::*;

pub struct RealNativeApi {
    options: NativeBackendOptions,
}

#[cfg(not(target_os = "macos"))]
impl RealNativeApi {
    pub fn new(options: NativeBackendOptions) -> Self {
        Self { options }
    }

    fn debug(&self, message: impl AsRef<str>) {
        if let Some(diagnostics) = self.options.diagnostics.as_ref() {
            diagnostics.debug(message.as_ref());
        }
    }
}

#[cfg(not(target_os = "macos"))]
impl MacosNativeApi for RealNativeApi {
    fn has_symbol(&self, _symbol: &'static str) -> bool {
        false
    }

    fn ax_is_trusted(&self) -> bool {
        false
    }

    fn minimal_topology_ready(&self) -> bool {
        false
    }

    fn debug(&self, message: &str) {
        RealNativeApi::debug(self, message);
    }

    fn validate_environment(&self) -> Result<(), MacosNativeConnectError> {
        Err(MacosNativeConnectError::MissingTopologyPrecondition(
            UNSUPPORTED_PLATFORM_MESSAGE,
        ))
    }

    fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError> {
        Err(MacosNativeProbeError::MissingTopology(
            UNSUPPORTED_PLATFORM_MESSAGE,
        ))
    }

    fn managed_spaces(&self) -> Result<Vec<RawSpaceRecord>, MacosNativeProbeError> {
        Err(MacosNativeProbeError::MissingTopology(
            UNSUPPORTED_PLATFORM_MESSAGE,
        ))
    }

    fn active_space_ids(&self) -> Result<HashSet<u64>, MacosNativeProbeError> {
        Err(MacosNativeProbeError::MissingTopology(
            UNSUPPORTED_PLATFORM_MESSAGE,
        ))
    }

    fn active_space_windows(
        &self,
        _space_id: u64,
    ) -> Result<Vec<RawWindow>, MacosNativeProbeError> {
        Err(MacosNativeProbeError::MissingTopology(
            UNSUPPORTED_PLATFORM_MESSAGE,
        ))
    }

    fn inactive_space_window_ids(&self) -> Result<HashMap<u64, Vec<u64>>, MacosNativeProbeError> {
        Err(MacosNativeProbeError::MissingTopology(
            UNSUPPORTED_PLATFORM_MESSAGE,
        ))
    }

    fn switch_space(&self, _space_id: u64) -> Result<(), MacosNativeOperationError> {
        Err(MacosNativeOperationError::CallFailed(
            UNSUPPORTED_PLATFORM_MESSAGE,
        ))
    }

    fn focus_window(&self, _window_id: u64) -> Result<(), MacosNativeOperationError> {
        Err(MacosNativeOperationError::CallFailed(
            UNSUPPORTED_PLATFORM_MESSAGE,
        ))
    }

    fn move_window_to_space(
        &self,
        _window_id: u64,
        _space_id: u64,
    ) -> Result<(), MacosNativeOperationError> {
        Err(MacosNativeOperationError::CallFailed(
            UNSUPPORTED_PLATFORM_MESSAGE,
        ))
    }

    fn swap_window_frames(
        &self,
        _source_window_id: u64,
        _source_frame: NativeBounds,
        _target_window_id: u64,
        _target_frame: NativeBounds,
    ) -> Result<(), MacosNativeOperationError> {
        Err(MacosNativeOperationError::CallFailed(
            UNSUPPORTED_PLATFORM_MESSAGE,
        ))
    }
}
