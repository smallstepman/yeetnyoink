use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
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

fn crate_source(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

#[test]
fn source_crate_root_is_a_thin_facade() {
    let lib = std::fs::read_to_string(crate_source("src/lib.rs")).unwrap();
    let macos_real_api = std::fs::read_to_string(crate_source("src/real_api/macos.rs")).unwrap();
    let stub_real_api = std::fs::read_to_string(crate_source("src/real_api/stub.rs")).unwrap();

    assert!(lib.contains("mod api;"));
    assert!(lib.contains("mod real_api;"));
    assert!(!lib.contains("pub trait MacosNativeApi {"));
    assert!(!lib.contains("pub struct RealNativeApi"));
    assert!(!lib.contains("pub use api::*;"));
    assert!(!macos_real_api.contains("use crate::*;"));
    assert!(!stub_real_api.contains("use crate::*;"));
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

    let exports =
        std::fs::read_to_string(crate_source("swift/Sources/MacosWindowManagerFFI/Exports.swift"))
            .unwrap();
    assert!(exports.contains("@_cdecl(\"mwm_backend_new\")"));
    assert!(exports.contains("@_cdecl(\"mwm_backend_free\")"));
    assert!(exports.contains("@_cdecl(\"mwm_backend_desktop_snapshot\")"));
}
