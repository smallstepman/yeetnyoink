
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
