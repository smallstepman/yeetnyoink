#![cfg(target_os = "macos")]

use std::sync::Arc;

use macos_window_manager::{
    ActiveSpaceFocusTargetHint, MacosNativeApi, MacosNativeConnectError, MacosNativeOperationError,
    MacosNativeProbeError, MissionControlHotkey, MissionControlModifiers, NativeBackendOptions,
    NativeBounds, NativeDesktopSnapshot, NativeDiagnostics, NativeDirection, NativeWindowSnapshot,
    RawSpaceRecord, RawTopologySnapshot, RawWindow, RealNativeApi, SpaceKind, WindowSnapshot,
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
    let _space_kind = SpaceKind::Desktop;

    let raw_space = RawSpaceRecord {
        managed_space_id: 7,
        display_index: 0,
        space_type: 0,
        tile_spaces: Vec::new(),
        has_tile_layout_manager: false,
        stage_manager_managed: false,
    };
    let raw_window = RawWindow {
        id: 11,
        pid: Some(22),
        app_id: Some("com.example.smoke".to_string()),
        title: Some("Smoke".to_string()),
        level: 0,
        visible_index: Some(0),
        frame: Some(hint.bounds),
    };
    let window_snapshot = WindowSnapshot {
        id: raw_window.id,
        pid: raw_window.pid,
        app_id: raw_window.app_id.clone(),
        title: raw_window.title.clone(),
        space_id: raw_space.managed_space_id,
        order_index: Some(0),
    };
    let _raw_topology_snapshot = RawTopologySnapshot {
        spaces: vec![raw_space],
        active_space_ids: [hint.space_id].into_iter().collect(),
        active_space_windows: [(hint.space_id, vec![raw_window])].into_iter().collect(),
        inactive_space_window_ids: std::collections::HashMap::new(),
        focused_window_id: Some(window_snapshot.id),
    };

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
