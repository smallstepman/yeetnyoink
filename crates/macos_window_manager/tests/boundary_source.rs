fn backend_crate_source() -> String {
    [
        include_str!("../src/lib.rs"),
        include_str!("../src/ax.rs"),
        include_str!("../src/desktop_topology_snapshot.rs"),
        include_str!("../src/error.rs"),
        include_str!("../src/foundation.rs"),
        include_str!("../src/skylight.rs"),
        include_str!("../src/window_server.rs"),
    ]
    .join("\n")
}

#[test]
fn source_backend_crate_avoids_repo_imports() {
    let backend = backend_crate_source();
    for forbidden in [
        "use crate::config",
        "MissionControlShortcutConfig",
        "use crate::logging",
        "crate::logging::",
        "crate::engine::",
    ] {
        assert!(
            !backend.contains(forbidden),
            "extracted backend crate should not depend on {forbidden}"
        );
    }
}

#[test]
fn source_backend_crate_stays_facade_focused() {
    let backend = backend_crate_source();
    for forbidden in [
        "FocusedWindowRecord",
        "FocusedAppRecord",
        "WindowRecord",
        "ProcessId",
        "plan_focus_direction",
        "execute_focus_plan",
        "focused_window_record(",
        "focused_app_record(",
        "window_records(",
        "windows_in_space(",
        "swap_directional_neighbor(",
        "move_window_to_space_checked(",
        "fn directional_focus_target_in_active_topology(",
        "fn adjacent_space_in_direction(",
        "fn focus_direction_target_with_ax_fallback(",
    ] {
        assert!(
            !backend.contains(forbidden),
            "extracted backend crate should not expose {forbidden}"
        );
    }
}
