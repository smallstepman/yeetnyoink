use std::sync::Arc;

use macos_window_manager::{
    ActiveSpaceFocusTargetHint, MacosNativeApi, MacosNativeConnectError,
    MacosNativeOperationError, MacosNativeProbeError, MissionControlHotkey,
    MissionControlModifiers, NativeBackendOptions, NativeBounds, NativeDesktopSnapshot,
    NativeDiagnostics, NativeDirection, NativeWindowSnapshot, RealNativeApi,
};

struct SmokeDiagnostics;

impl NativeDiagnostics for SmokeDiagnostics {
    fn debug(&self, _message: &str) {}
}

fn takes_native_api(_api: &dyn MacosNativeApi) {}

#[test]
fn public_api_smoke_test() {
    let west_space_hotkey = MissionControlHotkey {
        key_code: 123,
        mission_control: MissionControlModifiers {
            control: true,
            ..MissionControlModifiers::default()
        },
    };
    let east_space_hotkey = MissionControlHotkey {
        key_code: 124,
        mission_control: MissionControlModifiers {
            option: true,
            shift: true,
            ..MissionControlModifiers::default()
        },
    };
    let diagnostics: Arc<dyn NativeDiagnostics> = Arc::new(SmokeDiagnostics);
    diagnostics.debug("smoke");

    let api = RealNativeApi::new(NativeBackendOptions {
        west_space_hotkey,
        east_space_hotkey,
        diagnostics: Some(Arc::clone(&diagnostics)),
    });
    takes_native_api(&api);

    let hint = ActiveSpaceFocusTargetHint {
        space_id: 7,
        bounds: NativeBounds {
            x: 1,
            y: 2,
            width: 3,
            height: 4,
        },
    };
    let _desktop_snapshot: Option<NativeDesktopSnapshot> = None;
    let _window_snapshot: Option<NativeWindowSnapshot> = None;

    let connect_error = MacosNativeConnectError::MissingRequiredSymbol("SLSMainConnectionID");
    let probe_error = MacosNativeProbeError::MissingTopology("CGWindowListCopyWindowInfo");
    let operation_error = MacosNativeOperationError::Probe(probe_error);

    assert_eq!(hint.space_id, 7);
    assert_eq!(hint.bounds.width, 3);
    assert!(matches!(NativeDirection::West, NativeDirection::West));
    assert!(matches!(
        connect_error,
        MacosNativeConnectError::MissingRequiredSymbol("SLSMainConnectionID")
    ));
    assert!(matches!(
        operation_error,
        MacosNativeOperationError::Probe(MacosNativeProbeError::MissingTopology(
            "CGWindowListCopyWindowInfo"
        ))
    ));
}
