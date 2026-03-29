# macOS Window Manager Crate Extraction Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Finish extracting the macOS native backend into `crates/macos_window_manager` so `src/adapters/window_managers/macos_native.rs` is outer-adapter-only and backend-focused tests live in the new crate.

**Architecture:** Treat `crates/macos_window_manager` as the single production backend implementation and reduce `macos_native.rs` to config translation, DTO-to-topology conversion, WM-record building, and directional policy. Move backend-native tests and boundary checks next to the new crate, while keeping outer adapter regressions in the main crate.

**Tech Stack:** Rust workspace, Cargo path dependency, macOS AX/CoreFoundation/CoreGraphics/SkyLight FFI, existing unit/source-shape tests

---

> **Execution note:** this worktree is already dirty. Use path-limited `git add` / `git commit` so each task only commits the files touched for that task.

> **TDD note:** when a task is mostly structural, the “red” step can be a failing targeted source-shape assertion or a failing `cargo test -p ...` compile/test command.

### Task 1: Make the extracted crate a real workspace package with a usable public surface

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `crates/macos_window_manager/Cargo.toml`
- Modify: `crates/macos_window_manager/src/lib.rs`
- Create: `crates/macos_window_manager/tests/public_api.rs`

**Step 1: Write the failing test**

Create `crates/macos_window_manager/tests/public_api.rs` with a minimal smoke test that imports the intended public surface:

```rust
use macos_window_manager::{
    ActiveSpaceFocusTargetHint, NativeBounds, NativeDirection,
};

#[test]
fn public_api_smoke_test() {
    let hint = ActiveSpaceFocusTargetHint {
        space_id: 7,
        bounds: NativeBounds { x: 1, y: 2, width: 3, height: 4 },
    };

    assert_eq!(hint.space_id, 7);
    assert!(matches!(NativeDirection::West, NativeDirection::West));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p macos_window_manager public_api_smoke_test -- --nocapture`

Expected: FAIL because the crate is not yet wired as a workspace member and/or the needed items are not public exports.

**Step 3: Write minimal implementation**

- Add a `[workspace]` section in the root `Cargo.toml` and include `crates/macos_window_manager`
- Add a normal path dependency from the root package to `crates/macos_window_manager`
- Make `crates/macos_window_manager/src/lib.rs` declare the extracted modules and `pub use` the backend surface needed by the outer adapter
- Make the public DTOs/options/errors/traits visible from the crate root

Representative shape:

```rust
mod ax;
mod desktop_topology_snapshot;
mod error;
mod foundation;
mod skylight;
mod window_server;

pub use desktop_topology_snapshot::{
    ActiveSpaceFocusTargetHint, NativeBounds, NativeDesktopSnapshot, NativeDirection,
};
pub use error::{MacosNativeConnectError, MacosNativeOperationError, MacosNativeProbeError};
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p macos_window_manager public_api_smoke_test -- --nocapture`

Expected: PASS

**Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/macos_window_manager/Cargo.toml \
  crates/macos_window_manager/src/lib.rs \
  crates/macos_window_manager/tests/public_api.rs
git commit -m "refactor: wire macos window manager crate"
```

### Task 2: Move the remaining production backend implementation into the new crate

**Files:**
- Modify: `crates/macos_window_manager/src/lib.rs`
- Modify: `crates/macos_window_manager/src/ax.rs`
- Modify: `crates/macos_window_manager/src/desktop_topology_snapshot.rs`
- Modify: `crates/macos_window_manager/src/error.rs`
- Modify: `crates/macos_window_manager/src/foundation.rs`
- Modify: `crates/macos_window_manager/src/skylight.rs`
- Modify: `crates/macos_window_manager/src/window_server.rs`
- Modify: `src/adapters/window_managers/macos_native.rs`

**Step 1: Write the failing test**

Add or update a main-crate source-shape test in `src/adapters/window_managers/macos_native.rs` that asserts:

```rust
#[test]
fn source_adapter_uses_extracted_macos_backend() {
    let implementation = implementation_source();
    assert!(implementation.contains("use macos_window_manager::{"));
    assert!(!implementation.contains("mod macos_window_manager_api {"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test source_adapter_uses_extracted_macos_backend --lib -- --nocapture`

Expected: FAIL because the production inline backend module still exists.

**Step 3: Write minimal implementation**

- Move any remaining production code from the inline `mod macos_window_manager_api` into the extracted crate
- Make `crates/macos_window_manager/src/lib.rs` the crate root for the production backend API
- Replace the inline backend imports in `macos_native.rs` with `use macos_window_manager::{ ... }`
- Delete the production inline `mod macos_window_manager_api`

Representative target import:

```rust
use macos_window_manager::{
    ActiveSpaceFocusTargetHint, MacosNativeApi, MacosNativeConnectError,
    MacosNativeOperationError, MacosNativeProbeError, NativeBackendOptions,
    NativeBounds, NativeDesktopSnapshot, NativeDiagnostics, NativeDirection,
    NativeWindowSnapshot, RealNativeApi,
};
```

**Step 4: Run test to verify it passes**

Run: `cargo test source_adapter_uses_extracted_macos_backend --lib -- --nocapture`

Expected: PASS

**Step 5: Commit**

```bash
git add crates/macos_window_manager/src/lib.rs \
  crates/macos_window_manager/src/ax.rs \
  crates/macos_window_manager/src/desktop_topology_snapshot.rs \
  crates/macos_window_manager/src/error.rs \
  crates/macos_window_manager/src/foundation.rs \
  crates/macos_window_manager/src/skylight.rs \
  crates/macos_window_manager/src/window_server.rs \
  src/adapters/window_managers/macos_native.rs
git commit -m "refactor: extract macos backend production code"
```

### Task 3: Move backend-native tests into the extracted crate

**Files:**
- Modify: `crates/macos_window_manager/src/lib.rs`
- Modify: `crates/macos_window_manager/src/ax.rs`
- Modify: `crates/macos_window_manager/src/desktop_topology_snapshot.rs`
- Modify: `crates/macos_window_manager/src/foundation.rs`
- Create or modify: `crates/macos_window_manager/src/tests.rs`
- Modify: `src/adapters/window_managers/macos_native.rs`

**Step 1: Write the failing test**

Move one real backend regression test first, for example:

```rust
#[test]
fn backend_focus_direction_remaps_post_switch_same_pid_splitview_target_before_active_space_focus() {
    // existing regression body moved from macos_native.rs
}
```

and keep a second targeted backend regression ready to move next, such as:

```rust
#[test]
fn backend_focus_direction_keeps_selected_target_when_next_snapshot_drops_it() {
    // existing regression body moved from macos_native.rs
}
```

**Step 2: Run test to verify it fails**

Run:
`cargo test -p macos_window_manager backend_focus_direction_remaps_post_switch_same_pid_splitview_target_before_active_space_focus -- --nocapture`

Expected: FAIL because crate-local test helpers/fakes/visibility are not fully wired yet.

**Step 3: Write minimal implementation**

- Move backend-only test fakes/helpers into the extracted crate
- Keep outer adapter tests in `macos_native.rs`
- Adjust `pub(crate)` / test-module visibility only where necessary for crate-local tests
- Remove the moved backend tests from the main crate once their crate-local copies are green

**Step 4: Run test to verify it passes**

Run:
`cargo test -p macos_window_manager backend_focus_direction_remaps_post_switch_same_pid_splitview_target_before_active_space_focus -- --nocapture`

Expected: PASS

**Step 5: Commit**

```bash
git add crates/macos_window_manager/src/lib.rs \
  crates/macos_window_manager/src/ax.rs \
  crates/macos_window_manager/src/desktop_topology_snapshot.rs \
  crates/macos_window_manager/src/foundation.rs \
  crates/macos_window_manager/src/tests.rs \
  src/adapters/window_managers/macos_native.rs
git commit -m "test: move macos backend tests into crate"
```

### Task 4: Replace inline-source boundary assertions with extracted-crate boundary checks

**Files:**
- Create: `crates/macos_window_manager/tests/boundary_source.rs`
- Modify: `src/adapters/window_managers/macos_native.rs`

**Step 1: Write the failing test**

Create `crates/macos_window_manager/tests/boundary_source.rs` with extracted-crate checks such as:

```rust
#[test]
fn source_backend_crate_avoids_repo_imports() {
    let lib = include_str!("../src/lib.rs");
    assert!(!lib.contains("use crate::config"));
    assert!(!lib.contains("crate::engine::"));
}
```

and keep/add a main-crate check:

```rust
#[test]
fn source_adapter_has_no_inline_macos_backend_module() {
    let implementation = implementation_source();
    assert!(!implementation.contains("mod macos_window_manager_api {"));
}
```

**Step 2: Run test to verify it fails**

Run:
`cargo test -p macos_window_manager source_backend_crate_avoids_repo_imports -- --nocapture`

Expected: FAIL until the replacement boundary checks point at the right extracted-crate surface and the old inline-boundary checks are updated.

**Step 3: Write minimal implementation**

- Remove obsolete tests that only make sense when the backend lives inline
- Add crate-local source/boundary assertions that protect the extracted backend from regressing back toward yeetnyoink-owned imports
- Keep a small main-crate assertion that ensures the adapter does not grow a new inline backend module again

**Step 4: Run test to verify it passes**

Run:
`cargo test -p macos_window_manager source_backend_crate_avoids_repo_imports -- --nocapture`

Expected: PASS

**Step 5: Commit**

```bash
git add crates/macos_window_manager/tests/boundary_source.rs \
  src/adapters/window_managers/macos_native.rs
git commit -m "test: update macos extraction boundary checks"
```

### Task 5: Clean up the outer adapter after backend removal

**Files:**
- Modify: `src/adapters/window_managers/macos_native.rs`
- Modify: `AGENTS.md` (only if the extraction exposes a new lasting gotcha)

**Step 1: Write the failing test**

Pick one outer adapter regression that must still pass after the split, for example:

```rust
#[test]
fn focus_direction_escapes_overlay_only_space_into_adjacent_real_space() {
    // existing outer regression body remains in macos_native.rs
}
```

**Step 2: Run test to verify it fails**

Run:
`cargo test focus_direction_escapes_overlay_only_space_into_adjacent_real_space --lib -- --nocapture`

Expected: FAIL if the adapter still depends on removed inline helpers or imports after the backend extraction.

**Step 3: Write minimal implementation**

- Remove dead inline-backend-only helper code from `macos_native.rs`
- Keep only outer adapter logic: config translation, DTO conversion, topology/policy, adapter tests
- Fix imports, aliases, and helper ownership so the adapter compiles cleanly against the new crate

**Step 4: Run test to verify it passes**

Run:
`cargo test focus_direction_escapes_overlay_only_space_into_adjacent_real_space --lib -- --nocapture`

Expected: PASS

**Step 5: Commit**

```bash
git add src/adapters/window_managers/macos_native.rs AGENTS.md
git commit -m "refactor: reduce macos adapter to outer policy layer"
```

### Task 6: Full verification and release build

**Files:**
- Modify only if verification uncovers a real defect in touched extraction files

**Step 1: Write the failing test**

Use the full verification set as the final red/green gate for the extraction:

```text
cargo test -p macos_window_manager
cargo test macos_native --lib -- --nocapture
cargo build --release
```

**Step 2: Run verification to find failures**

Run:

```bash
cargo test -p macos_window_manager
cargo test macos_native --lib -- --nocapture
cargo build --release
```

Expected: at least one failure if the extraction is incomplete.

**Step 3: Write minimal implementation**

- Fix only extraction-caused failures
- Do not broaden into unrelated cleanup
- If verification exposes a new macOS-specific surprise, add a short durable note to `AGENTS.md`

**Step 4: Run verification to verify it passes**

Run:

```bash
cargo test -p macos_window_manager
cargo test macos_native --lib -- --nocapture
cargo build --release
```

Expected:

- extracted crate tests PASS
- macOS adapter tests PASS
- release build succeeds

**Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/macos_window_manager src/adapters/window_managers/macos_native.rs AGENTS.md
git commit -m "refactor: finalize macos window manager crate extraction"
```
