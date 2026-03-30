use crate::api::MacosNativeApi;
use crate::error::MacosNativeConnectError;

#[cfg(target_os = "macos")]
const REQUIRED_PRIVATE_SYMBOLS: &[&str] = &[
    "SLSMainConnectionID",
    "SLSCopyManagedDisplaySpaces",
    "SLSManagedDisplayGetCurrentSpace",
    "SLSManagedDisplaySetCurrentSpace",
    "SLSCopyManagedDisplayForSpace",
    "SLSCopyWindowsWithOptionsAndTags",
    "SLSMoveWindowsToManagedSpace",
    "AXIsProcessTrusted",
    "_AXUIElementGetWindow",
    "_SLPSSetFrontProcessWithOptions",
    "GetProcessForPID",
];

pub(crate) fn validate_environment_with_api<A: MacosNativeApi + ?Sized>(
    api: &A,
) -> Result<(), MacosNativeConnectError> {
    for symbol in REQUIRED_PRIVATE_SYMBOLS {
        if !api.has_symbol(symbol) {
            return Err(MacosNativeConnectError::MissingRequiredSymbol(symbol));
        }
    }

    if !api.ax_is_trusted() {
        return Err(MacosNativeConnectError::MissingAccessibilityPermission);
    }

    if !api.minimal_topology_ready() {
        return Err(MacosNativeConnectError::MissingTopologyPrecondition(
            "main SkyLight connection",
        ));
    }

    Ok(())
}
