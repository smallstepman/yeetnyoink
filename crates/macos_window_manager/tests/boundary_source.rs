use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn boundary_test_source() -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/boundary_source.rs"))
        .expect("boundary source test should be readable")
}

fn production_source_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut entries = fs::read_dir(root)
        .expect("backend src directory should be readable")
        .collect::<Result<Vec<_>, _>>()
        .expect("backend src directory entries should be readable");
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            files.extend(production_source_files(&path));
            continue;
        }

        if path.extension() != Some(OsStr::new("rs")) {
            continue;
        }

        if path.file_name() == Some(OsStr::new("tests.rs")) {
            continue;
        }

        files.push(path);
    }

    files
}

fn backend_crate_source() -> String {
    production_source_files(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"))
        .into_iter()
        .map(|path| fs::read_to_string(path).expect("backend source file should be readable"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn boundary_test_discovers_backend_source_dynamically() {
    let boundary_test = boundary_test_source();

    assert!(
        !boundary_test.contains("include_str!(\"../src/"),
        "boundary source corpus should be discovered dynamically instead of hard-coded include_str! entries"
    );
}

#[test]
fn boundary_test_explicitly_blocks_direct_repo_crate_imports() {
    let boundary_test = boundary_test_source();

    assert!(
        boundary_test.contains("\"yeetnyoink::\""),
        "boundary test should explicitly reject direct yeetnyoink:: references"
    );
}

#[test]
fn source_backend_crate_avoids_repo_imports() {
    let backend = backend_crate_source();
    for forbidden in [
        "yeetnyoink::",
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

#[test]
fn source_backend_surface_is_coarse_and_macos_only() {
    let api = std::fs::read_to_string(crate_source("src/api.rs")).unwrap();
    let backend = std::fs::read_to_string(crate_source("src/backend.rs")).unwrap();
    let environment = std::fs::read_to_string(crate_source("src/environment.rs")).unwrap();
    let lib = std::fs::read_to_string(crate_source("src/lib.rs")).unwrap();

    for forbidden in [
        "fn has_symbol(&self, symbol: &'static str) -> bool;",
        "fn ax_is_trusted(&self) -> bool;",
        "fn minimal_topology_ready(&self) -> bool;",
    ] {
        assert!(
            !api.contains(forbidden),
            "shared backend trait should not expose legacy probe-style method {forbidden}"
        );
    }

    for forbidden in [
        "fn has_symbol(&self",
        "fn ax_is_trusted(&self)",
        "fn minimal_topology_ready(&self)",
    ] {
        assert!(
            !backend.contains(forbidden),
            "Swift backend should not implement legacy probe-style helper {forbidden}"
        );
    }

    for forbidden in [
        "REQUIRED_PRIVATE_SYMBOLS",
        "AXIsProcessTrusted",
        "\"main SkyLight connection\"",
        "api.ax_is_trusted()",
        "api.minimal_topology_ready()",
    ] {
        assert!(
            !environment.contains(forbidden),
            "environment boundary should not retain deleted Rust fallback probe artifact {forbidden}"
        );
    }

    assert!(
        !lib.contains("use crate::{\n    active_space_ax_backed_same_pid_target"),
        "lib.rs should not read like a behavior engine importing legacy helper orchestration"
    );
}

fn crate_source(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

fn unique_test_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&dir).expect("temporary test directory should be creatable");
    dir
}

#[test]
fn source_crate_root_is_a_thin_facade() {
    let lib = std::fs::read_to_string(crate_source("src/lib.rs")).unwrap();
    let macos_real_api = std::fs::read_to_string(crate_source("src/backend.rs")).unwrap();

    assert!(lib.contains("mod api;"));
    assert!(lib.contains("mod backend;"));
    assert!(!lib.contains("mod stub;"));
    assert!(!lib.contains("mod ax;"));
    assert!(!lib.contains("mod foundation;"));
    assert!(!lib.contains("mod skylight;"));
    assert!(!lib.contains("mod window_server;"));
    assert!(!lib.contains("mod real_api;"));
    assert!(!lib.contains("pub trait MacosNativeApi {"));
    assert!(!lib.contains("pub struct RealNativeApi"));
    assert!(!lib.contains("pub use api::*;"));
    assert!(!macos_real_api.contains("use crate::*;"));
}

#[test]
fn source_backend_crate_is_macos_only() {
    let lib = std::fs::read_to_string(crate_source("src/lib.rs")).unwrap();

    assert!(
        !lib.contains("mod stub;"),
        "lib.rs should not declare a non-macOS stub module"
    );

    for path in production_source_files(&crate_source("src")) {
        let source = std::fs::read_to_string(&path).unwrap();
        assert!(
            !source.contains("cfg(not(target_os = \"macos\"))"),
            "{} should not contain non-macOS cfg guards",
            path.strip_prefix(crate_source(".")).unwrap().display()
        );
    }

    assert!(
        !crate_source("src/stub.rs").exists(),
        "src/stub.rs should be removed once the crate becomes macOS-only"
    );
}

#[test]
fn source_build_script_does_not_silently_return_for_non_macos_targets() {
    let build = std::fs::read_to_string(crate_source("build.rs")).unwrap();
    assert!(
        !build.contains(
            "if env::var(\"CARGO_CFG_TARGET_OS\").as_deref() != Ok(\"macos\") {\n        return;"
        ),
        "build.rs should fail clearly for non-macOS targets instead of returning early"
    );
}

#[test]
fn build_script_fails_clearly_for_non_macos_targets() {
    let temp_dir = unique_test_dir("macos-window-manager-build-script");
    let build_script_bin = temp_dir.join("build-script-test");

    let rustc_status = Command::new("rustc")
        .arg("--edition=2024")
        .arg(crate_source("build.rs"))
        .arg("-o")
        .arg(&build_script_bin)
        .status()
        .expect("rustc should compile build.rs for boundary test");
    assert!(
        rustc_status.success(),
        "build.rs should compile as a standalone program for testing"
    );

    let output = Command::new(&build_script_bin)
        .env("CARGO_MANIFEST_DIR", env!("CARGO_MANIFEST_DIR"))
        .env("OUT_DIR", temp_dir.join("out"))
        .env("PROFILE", "debug")
        .env("TARGET", "x86_64-unknown-linux-gnu")
        .env("CARGO_CFG_TARGET_ARCH", "x86_64")
        .env("CARGO_CFG_TARGET_OS", "linux")
        .output()
        .expect("compiled build.rs should run for boundary test");

    assert!(
        !output.status.success(),
        "build.rs should reject non-macOS targets instead of succeeding"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("macOS") || stderr.contains("macos"),
        "non-macOS failure should explain the macOS-only boundary, stderr was: {stderr}"
    );
}

#[test]
fn source_navigation_helpers_leave_lib_rs() {
    let lib = std::fs::read_to_string(crate_source("src/lib.rs")).unwrap();
    assert!(lib.contains("mod environment;"));
    assert!(lib.contains("mod navigation;"));
    assert!(!lib.contains("fn wait_for_space_presentation("));
    assert!(!lib.contains("fn switch_space_in_snapshot("));
}

#[test]
fn source_swift_backend_scaffold_exists() {
    assert!(crate_source("build.rs").exists());
    assert!(crate_source("swift/Package.swift").exists());

    let cargo = std::fs::read_to_string(crate_source("Cargo.toml")).unwrap();
    assert!(cargo.contains("build = \"build.rs\""));
}

#[test]
fn source_build_script_uses_cargo_target_and_out_dir_for_swiftpm() {
    let build = std::fs::read_to_string(crate_source("build.rs")).unwrap();

    assert!(
        build.contains("TARGET"),
        "build script should derive the Swift build target from Cargo target env"
    );
    assert!(
        build.contains("CARGO_CFG_TARGET_ARCH"),
        "build script should read Cargo target architecture instead of assuming the host arch"
    );
    assert!(
        build.contains("--triple"),
        "build script should pass an explicit target triple to swift build"
    );
    assert!(
        build.contains("OUT_DIR"),
        "build script should place SwiftPM scratch output under Cargo OUT_DIR"
    );
    assert!(
        build.contains("--scratch-path"),
        "build script should keep SwiftPM artifacts out of the source tree"
    );
    assert!(
        !build.contains(".build"),
        "build script should not rely on SwiftPM's in-tree .build directory"
    );
}

#[test]
fn source_swift_ffi_contract_is_explicit() {
    let lib = std::fs::read_to_string(crate_source("src/lib.rs")).unwrap();
    assert!(lib.contains("mod ffi;"));
    assert!(lib.contains("mod transport;"));
    assert!(lib.contains("mod shim;"));

    let exports = std::fs::read_to_string(crate_source(
        "swift/Sources/MacosWindowManagerFFI/Exports.swift",
    ))
    .unwrap();
    assert!(exports.contains("@_cdecl(\"mwm_backend_new\")"));
    assert!(exports.contains("@_cdecl(\"mwm_backend_free\")"));
    assert!(exports.contains("@_cdecl(\"mwm_backend_desktop_snapshot\")"));
    assert!(exports.contains("@_cdecl(\"mwm_backend_prepare_fast_focus_context\")"));
    assert!(exports.contains("@_cdecl(\"mwm_status_release\")"));
    assert!(exports.contains("@_cdecl(\"mwm_desktop_snapshot_release\")"));

    let rust_transport = std::fs::read_to_string(crate_source("src/transport.rs")).unwrap();
    assert!(
        rust_transport.contains("mwm_status_release")
            && rust_transport.contains("mwm_desktop_snapshot_release"),
        "Rust transport contract should document how owned FFI payloads are released"
    );
    assert!(
        rust_transport.contains("ABI layout assertions"),
        "Rust transport should pin the Swift ABI with explicit layout assertions"
    );
}

#[test]
fn source_swift_fast_focus_context_export_exists() {
    let exports = std::fs::read_to_string(crate_source(
        "swift/Sources/MacosWindowManagerFFI/Exports.swift",
    ))
    .unwrap();
    assert!(
        exports.contains("@_cdecl(\"mwm_backend_prepare_fast_focus_context\")"),
        "Swift FFI should expose a coarse fast-focus context export"
    );

    let api = std::fs::read_to_string(crate_source("src/api.rs")).unwrap();
    assert!(
        api.contains("fn prepare_fast_focus_context("),
        "the shared backend trait should expose the coarse fast-focus context method"
    );

    let backend = std::fs::read_to_string(crate_source("src/backend.rs")).unwrap();
    assert!(
        !backend.contains("pub fn prepare_fast_focus_context("),
        "SwiftMacosBackend should keep the bridge-typed fast-focus helper crate-private"
    );
}

fn source_window_around(source: &str, needle: &str, radius: usize) -> String {
    let needle_start = source.find(needle).expect("needle should exist in source");
    let start = needle_start.saturating_sub(radius);
    let end = (needle_start + needle.len() + radius).min(source.len());
    source[start..end].to_string()
}

#[test]
fn source_pointer_owning_transport_types_are_not_copy() {
    let rust_transport = std::fs::read_to_string(crate_source("src/transport.rs")).unwrap();

    for type_name in [
        "MwmStatus",
        "MwmWindowAbi",
        "MwmDesktopSnapshotAbi",
        "MwmFastFocusContextAbi",
    ] {
        let window = source_window_around(&rust_transport, &format!("pub struct {type_name}"), 120);
        assert!(
            !window.contains("Copy"),
            "{type_name} owns FFI pointers and should not derive Copy"
        );
    }
}

#[test]
fn source_owned_snapshot_does_not_deref_to_raw_transport() {
    let shim = std::fs::read_to_string(crate_source("src/shim.rs")).unwrap();
    assert!(
        !shim.contains("impl Deref for OwnedDesktopSnapshot"),
        "OwnedDesktopSnapshot should keep the raw transport behind an explicit ownership boundary"
    );
}

#[test]
fn source_rust_backend_no_longer_owns_private_macos_bindings() {
    assert!(!crate_source("src/foundation.rs").exists());
    assert!(!crate_source("src/ax.rs").exists());
    assert!(!crate_source("src/skylight.rs").exists());
    assert!(!crate_source("src/window_server.rs").exists());
}

#[test]
fn source_swift_backend_actions_delegate_production_calls_to_swift_backend() {
    let macos_real_api = std::fs::read_to_string(crate_source("src/backend.rs")).unwrap();

    for (method, expected_call_parts, forbidden_rust_impl) in [
        (
            "fn switch_space(&self, space_id: u64)",
            &[".switch_space(", "space_id"][..],
            "skylight::switch_space(self, space_id)",
        ),
        (
            "fn switch_adjacent_space(",
            &[".switch_adjacent_space(", "direction", "space_id"][..],
            "switch_adjacent_space_via_hotkey",
        ),
        (
            "fn focus_window(&self, window_id: u64)",
            &[".focus_window(", "window_id"][..],
            "window_server::focus_window(self, window_id)",
        ),
        (
            "fn focus_window_with_known_pid(",
            &[".focus_window_with_known_pid(", "window_id", "pid"][..],
            "focus_window_via_process_and_raise",
        ),
        (
            "fn focus_window_in_active_space_with_known_pid(",
            &[
                ".focus_window_in_active_space_with_known_pid(",
                "window_id",
                "pid",
                "target_hint",
            ][..],
            "focus_window_via_make_key_and_raise",
        ),
        (
            "fn move_window_to_space(",
            &[".move_window_to_space(", "window_id", "space_id"][..],
            "skylight::move_window_to_space(self, window_id, space_id)",
        ),
        (
            "fn swap_window_frames(",
            &[
                ".swap_window_frames(",
                "source_window_id",
                "target_window_id",
                "target_frame",
            ][..],
            "ax::swap_window_frames(",
        ),
    ] {
        let window = source_window_around(&macos_real_api, method, 700);
        assert!(
            window.contains("self.swift_backend_for_action()?"),
            "{method} should delegate through the Swift production backend"
        );
        for expected_part in expected_call_parts {
            assert!(
                window.contains(expected_part),
                "{method} should contain {expected_part}"
            );
        }
        assert!(
            !window.contains(forbidden_rust_impl),
            "{method} should not keep the Rust-owned production implementation"
        );
    }
}

#[test]
fn source_swift_backend_overrides_semantic_helpers_to_use_swift_backend() {
    let macos_real_api = std::fs::read_to_string(crate_source("src/backend.rs")).unwrap();

    for (method, expected_call_parts) in [
        (
            "fn switch_space_in_snapshot(",
            &[
                ".switch_space_in_snapshot(",
                "snapshot",
                "space_id",
                "adjacent_direction",
            ][..],
        ),
        (
            "fn switch_space_and_refresh(",
            &[
                ".switch_space_and_refresh(",
                "snapshot",
                "space_id",
                "adjacent_direction",
            ][..],
        ),
        (
            "fn focus_same_space_target_in_snapshot(",
            &[
                ".focus_same_space_target_in_snapshot(",
                "snapshot",
                "direction",
                "target_window_id",
            ][..],
        ),
    ] {
        let window = source_window_around(&macos_real_api, method, 500);
        assert!(
            window.contains("self.swift_backend_for_action()?"),
            "{method} should override the trait default and delegate to Swift"
        );
        for expected_part in expected_call_parts {
            assert!(
                window.contains(expected_part),
                "{method} should contain {expected_part}"
            );
        }
    }
}

#[test]
fn source_live_system_action_primitives_are_concrete() {
    let focus = std::fs::read_to_string(crate_source(
        "swift/Sources/MacosWindowManagerCore/Focus.swift",
    ))
    .unwrap();
    assert!(
        !focus.contains("extension LiveSystem: BackendActionSystem {}"),
        "LiveSystem should not rely on an empty BackendActionSystem conformance"
    );

    let live_system_actions = std::fs::read_to_string(crate_source(
        "swift/Sources/MacosWindowManagerCore/LiveSystemActions.swift",
    ))
    .unwrap();
    let live_system_actions = live_system_actions
        .split("extension LiveSystem: BackendActionSystem")
        .nth(1)
        .expect("LiveSystem BackendActionSystem conformance should exist");

    for (signature, marker) in [
        (
            "func switchSpace(_ spaceID: UInt64) throws",
            "SLSManagedDisplaySetCurrentSpace",
        ),
        (
            "func switchAdjacentSpace(_ direction: NativeDirection, targetSpaceID: UInt64) throws",
            "adjacentSpaceHotkey(direction)",
        ),
        (
            "func focusWindow(_ windowID: UInt64) throws",
            "windowDescription(id: windowID)",
        ),
        (
            "func focusWindowWithKnownPID(_ windowID: UInt64, pid: UInt32) throws",
            "focusWindow(windowID, pid: pid, frontsProcess: true)",
        ),
        (
            "func axWindowIDs(for pid: UInt32) throws -> [UInt64]",
            "axWindows(for: pid)",
        ),
        (
            "func moveWindowToSpace(_ windowID: UInt64, spaceID: UInt64) throws",
            "SLSMoveWindowsToManagedSpace",
        ),
        (
            "func swapWindowFrames(",
            "setWindowFrame(windowID: sourceWindowID, pid: sourcePID, frame: targetFrame)",
        ),
    ] {
        assert!(
            live_system_actions.contains(signature),
            "LiveSystem BackendActionSystem conformance should implement {signature}"
        );
        assert!(
            live_system_actions.contains(marker),
            "{signature} should contain production-only marker {marker}"
        );
    }

    for forbidden in [
        "_ = direction\n        try switchSpace(targetSpaceID)",
        "try focusWindow(windowID)",
        "[]\n    }",
    ] {
        assert!(
            !live_system_actions.contains(forbidden),
            "LiveSystem action implementations should not fall back to protocol-default body {forbidden:?}"
        );
    }
}
