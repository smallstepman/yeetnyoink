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
