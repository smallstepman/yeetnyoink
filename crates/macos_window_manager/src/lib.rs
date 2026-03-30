mod api;
#[cfg(target_os = "macos")]
mod backend;
mod desktop_topology_snapshot;
mod environment;
mod error;
mod ffi;
mod navigation;
mod shim;
mod transport;

pub use api::{
    ActiveSpaceFocusTargetHint, MacosWindowManagerBackend, MissionControlHotkey,
    MissionControlModifiers, NativeBackendOptions, NativeBounds, NativeDesktopSnapshot,
    NativeDiagnostics, NativeDirection, NativeFastFocusContext, NativeFastFocusEnvironment,
    NativeSpaceId, NativeSpaceSnapshot, NativeWindowId, NativeWindowSnapshot,
};
#[cfg(target_os = "macos")]
pub use backend::SwiftMacosBackend;
pub use desktop_topology_snapshot::SpaceKind;
pub use desktop_topology_snapshot::{
    RawSpaceRecord, RawTopologySnapshot, RawWindow, WindowSnapshot,
};
pub use error::{
    MacosNativeBridgeError, MacosNativeConnectError, MacosNativeFastFocusError,
    MacosNativeOperationError, MacosNativeProbeError,
};

#[cfg(test)]
#[test]
fn snapshot_wrapper_converts_ffi_snapshot() {
    let snapshot = shim::test_snapshot_from_ffi();

    assert_eq!(snapshot.spaces.len(), 2);
    assert_eq!(snapshot.windows.len(), 3);
    assert_eq!(snapshot.focused_window_id, Some(9003));
}

#[cfg(test)]
#[test]
fn switch_space_in_snapshot_maps_swift_operation_error() {
    let err = shim::test_switch_space_error_from_ffi();
    assert!(matches!(err, MacosNativeOperationError::MissingWindow(_)));
}
