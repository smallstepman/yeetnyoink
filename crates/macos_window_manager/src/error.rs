use crate::NativeDirection;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum MacosNativeConnectError {
    #[error("required macOS private symbol is unavailable: {0}")]
    MissingRequiredSymbol(&'static str),
    #[error("Accessibility permission is required for macOS native support")]
    MissingAccessibilityPermission,
    #[error("macOS native topology precondition is unavailable: {0}")]
    MissingTopologyPrecondition(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum MacosNativeProbeError {
    #[error("macOS native topology query is unavailable: {0}")]
    MissingTopology(&'static str),
    #[error("no focused window was found for any active Space")]
    MissingFocusedWindow,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MacosNativeFastFocusError {
    #[error(transparent)]
    Connect(#[from] MacosNativeConnectError),
    #[error(transparent)]
    Probe(#[from] MacosNativeProbeError),
    #[error(transparent)]
    Bridge(#[from] MacosNativeBridgeError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum MacosNativeOperationError {
    #[error(transparent)]
    Probe(#[from] MacosNativeProbeError),
    #[error("macOS native space {0} was not found in the current topology")]
    MissingSpace(u64),
    #[error("macOS native window {0} was not found in the current topology")]
    MissingWindow(u64),
    #[error("macOS native window {0} has no frame")]
    MissingWindowFrame(u64),
    #[error("macOS native window {0} does not expose an owner pid")]
    MissingWindowPid(u64),
    #[error("macOS native Stage Manager space {0} is intentionally unsupported")]
    UnsupportedStageManagerSpace(u64),
    #[error("macos_native: no window to focus {0}")]
    NoDirectionalFocusTarget(NativeDirection),
    #[error("macos_native: no window to move {0}")]
    NoDirectionalMoveTarget(NativeDirection),
    #[error("macOS native operation failed: {0}")]
    CallFailed(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MacosNativeBridgeError {
    #[error("swift macOS backend returned status {code} ({message:?})")]
    BackendStatus { code: i32, message: Option<String> },
    #[error("swift macOS backend returned a null handle")]
    NullBackendHandle,
    #[error("swift macOS backend returned invalid desktop snapshot transport: {0}")]
    InvalidDesktopSnapshotTransport(&'static str),
}

impl MacosNativeOperationError {
    pub(crate) fn from_swift_status(code: i32, message: Option<&str>) -> Self {
        match code {
            30 => parse_u64(message)
                .map(Self::MissingSpace)
                .unwrap_or(Self::CallFailed("swift macOS backend")),
            31 => parse_u64(message)
                .map(Self::MissingWindow)
                .unwrap_or(Self::CallFailed("swift macOS backend")),
            32 => parse_u64(message)
                .map(Self::MissingWindowFrame)
                .unwrap_or(Self::CallFailed("swift macOS backend")),
            33 => parse_u64(message)
                .map(Self::MissingWindowPid)
                .unwrap_or(Self::CallFailed("swift macOS backend")),
            34 => parse_u64(message)
                .map(Self::UnsupportedStageManagerSpace)
                .unwrap_or(Self::CallFailed("swift macOS backend")),
            35 => parse_direction(message)
                .map(Self::NoDirectionalFocusTarget)
                .unwrap_or(Self::CallFailed("swift macOS backend")),
            36 => parse_direction(message)
                .map(Self::NoDirectionalMoveTarget)
                .unwrap_or(Self::CallFailed("swift macOS backend")),
            37 => Self::CallFailed(static_operation_message(message)),
            _ => Self::CallFailed("swift macOS backend"),
        }
    }
}

fn parse_u64(message: Option<&str>) -> Option<u64> {
    message?.parse().ok()
}

fn parse_direction(message: Option<&str>) -> Option<NativeDirection> {
    match message {
        Some("west") => Some(NativeDirection::West),
        Some("east") => Some(NativeDirection::East),
        Some("north") => Some(NativeDirection::North),
        Some("south") => Some(NativeDirection::South),
        _ => None,
    }
}

fn static_operation_message(message: Option<&str>) -> &'static str {
    match message {
        Some("switch_space") => "switch_space",
        Some("switch_adjacent_space") => "switch_adjacent_space",
        Some("switch_space_in_snapshot") => "switch_space_in_snapshot",
        Some("focus_window") => "focus_window",
        Some("focus_window_with_known_pid") => "focus_window_with_known_pid",
        Some("focus_window_in_active_space_with_known_pid") => {
            "focus_window_in_active_space_with_known_pid"
        }
        Some("focus_same_space_target_in_snapshot") => "focus_same_space_target_in_snapshot",
        Some("move_window_to_space") => "move_window_to_space",
        Some("swap_window_frames") => "swap_window_frames",
        Some("backend_action_system") => "backend_action_system",
        _ => "swift macOS backend",
    }
}
