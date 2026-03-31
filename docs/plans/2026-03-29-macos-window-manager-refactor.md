# macos_window_manager Maintainability Refactor Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Refactor `crates/macos_window_manager` into a thinner facade plus focused internal modules and split test support, without changing behavior.

**Architecture:** Keep the existing `MacosNativeApi` shape broadly intact, but move public types/trait wiring into a dedicated `api` module, move macOS/stub concrete implementations into `real_api`, and extract navigation, environment, geometry, and focus helpers out of `lib.rs`. Reorganize the crate’s tests into concern-specific modules with shared support so behavior coverage remains strong while the structure becomes easier to maintain.

**Tech Stack:** Rust, Cargo, crate-local unit tests, integration source-shape tests in `crates/macos_window_manager/tests`, existing yeetnyoink macOS adapter verification commands.

---

## Ground rules

- Follow @test-driven-development for every refactor slice.
- Keep commits small and reviewable.
- Do not change behavior unless a test proves the existing behavior is broken.
- Prefer moving code before rewriting code.
- After each task, rerun the targeted crate test plus any touched regression coverage.

### Task 1: Thin the crate root and introduce `api` / `real_api` structure

**Files:**
- Create: `crates/macos_window_manager/src/api.rs`
- Create: `crates/macos_window_manager/src/real_api/mod.rs`
- Create: `crates/macos_window_manager/src/real_api/macos.rs`
- Create: `crates/macos_window_manager/src/real_api/stub.rs`
- Modify: `crates/macos_window_manager/src/lib.rs`
- Test: `crates/macos_window_manager/tests/boundary_source.rs`

**Step 1: Write the failing test**

Add a new source-shape test to `crates/macos_window_manager/tests/boundary_source.rs`:

```rust
#[test]
fn source_crate_root_is_a_thin_facade() {
    let lib = std::fs::read_to_string(crate_source("src/lib.rs")).unwrap();
    assert!(lib.contains("mod api;"));
    assert!(lib.contains("mod real_api;"));
    assert!(!lib.contains("pub trait MacosNativeApi {"));
    assert!(!lib.contains("pub struct RealNativeApi"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p macos_window_manager source_crate_root_is_a_thin_facade -- --exact --nocapture`

Expected: FAIL because `lib.rs` still contains the trait and `RealNativeApi` implementation.

**Step 3: Write minimal implementation**

Move the public type and trait surface into `src/api.rs`, for example:

```rust
pub type NativeSpaceId = u64;
pub type NativeWindowId = u64;

pub trait MacosNativeApi {
    fn has_symbol(&self, symbol: &'static str) -> bool;
    // existing methods stay here
}
```

Split `RealNativeApi` into `src/real_api/macos.rs` and `src/real_api/stub.rs`, then re-export from `src/real_api/mod.rs` and `src/lib.rs`:

```rust
mod api;
mod real_api;

pub use api::*;
pub use real_api::RealNativeApi;
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p macos_window_manager source_crate_root_is_a_thin_facade -- --exact --nocapture`

Expected: PASS

Then run: `cargo test -p macos_window_manager public_api_smoke_test --test public_api -- --exact --nocapture`

Expected: PASS

**Step 5: Commit**

```bash
git add crates/macos_window_manager/src/lib.rs \
        crates/macos_window_manager/src/api.rs \
        crates/macos_window_manager/src/real_api/mod.rs \
        crates/macos_window_manager/src/real_api/macos.rs \
        crates/macos_window_manager/src/real_api/stub.rs \
        crates/macos_window_manager/tests/boundary_source.rs
git commit -m "refactor: split macos backend facade root"
```

### Task 2: Extract environment and navigation helpers out of `lib.rs`

**Files:**
- Create: `crates/macos_window_manager/src/environment.rs`
- Create: `crates/macos_window_manager/src/navigation.rs`
- Modify: `crates/macos_window_manager/src/api.rs`
- Modify: `crates/macos_window_manager/src/lib.rs`
- Test: `crates/macos_window_manager/tests/boundary_source.rs`
- Test: `crates/macos_window_manager/src/tests.rs` or moved test module that covers switching/settling behavior

**Step 1: Write the failing test**

Add a new source-shape test:

```rust
#[test]
fn source_navigation_helpers_leave_lib_rs() {
    let lib = std::fs::read_to_string(crate_source("src/lib.rs")).unwrap();
    assert!(lib.contains("mod environment;"));
    assert!(lib.contains("mod navigation;"));
    assert!(!lib.contains("fn wait_for_space_presentation("));
    assert!(!lib.contains("fn switch_space_in_snapshot("));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p macos_window_manager source_navigation_helpers_leave_lib_rs -- --exact --nocapture`

Expected: FAIL because those helpers still live in `lib.rs`.

**Step 3: Write minimal implementation**

Move validation and navigation helpers into dedicated files:

```rust
// environment.rs
pub(crate) fn validate_environment_with_api<A: MacosNativeApi + ?Sized>(
    api: &A,
) -> Result<(), MacosNativeConnectError> { /* moved body */ }

// navigation.rs
pub(crate) fn switch_space_in_snapshot<A: MacosNativeApi + ?Sized>(
    api: &A,
    snapshot: &NativeDesktopSnapshot,
    space_id: u64,
    adjacent_direction: Option<NativeDirection>,
) -> Result<(), MacosNativeOperationError> { /* moved body */ }
```

Update `api.rs` trait defaults to delegate to these modules instead of inline helpers.

**Step 4: Run test to verify it passes**

Run: `cargo test -p macos_window_manager source_navigation_helpers_leave_lib_rs -- --exact --nocapture`

Expected: PASS

Then run a focused regression slice:

```bash
cargo test -p macos_window_manager \
  backend_focus_direction_can_switch_adjacent_space_without_direct_switch_primitive \
  -- --exact --nocapture

cargo test -p macos_window_manager \
  switch_space_waits_for_target_space_to_become_active_before_returning \
  -- --exact --nocapture
```

Expected: PASS for both tests.

**Step 5: Commit**

```bash
git add crates/macos_window_manager/src/lib.rs \
        crates/macos_window_manager/src/api.rs \
        crates/macos_window_manager/src/environment.rs \
        crates/macos_window_manager/src/navigation.rs \
        crates/macos_window_manager/tests/boundary_source.rs
git commit -m "refactor: extract macos backend navigation helpers"
```

### Task 3: Extract geometry and focus helper logic

**Files:**
- Create: `crates/macos_window_manager/src/geometry.rs`
- Create: `crates/macos_window_manager/src/focus.rs`
- Modify: `crates/macos_window_manager/src/api.rs`
- Modify: `crates/macos_window_manager/src/lib.rs`
- Test: `crates/macos_window_manager/tests/boundary_source.rs`
- Test: `crates/macos_window_manager/src/tests.rs` or moved focus test module

**Step 1: Write the failing test**

Add a new source-shape test:

```rust
#[test]
fn source_focus_and_geometry_helpers_leave_lib_rs() {
    let lib = std::fs::read_to_string(crate_source("src/lib.rs")).unwrap();
    assert!(lib.contains("mod geometry;"));
    assert!(lib.contains("mod focus;"));
    assert!(!lib.contains("fn native_overlap_len("));
    assert!(!lib.contains("fn focus_same_space_target_with_known_pid("));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p macos_window_manager source_focus_and_geometry_helpers_leave_lib_rs -- --exact --nocapture`

Expected: FAIL because the helper blocks are still in `lib.rs`.

**Step 3: Write minimal implementation**

Move the geometry helpers:

```rust
// geometry.rs
pub(crate) fn native_overlap_len(...) -> i32 { /* moved body */ }
pub(crate) fn native_center_distance_sq(...) -> i64 { /* moved body */ }
pub(crate) fn compare_native_windows_for_target_match(...) -> std::cmp::Ordering { /* moved body */ }
```

Move the focus helpers:

```rust
// focus.rs
pub(crate) fn focus_same_space_target_with_known_pid<A: MacosNativeApi + ?Sized>(
    api: &A,
    snapshot: &NativeDesktopSnapshot,
    direction: NativeDirection,
    target_window_id: u64,
    pid: u32,
) -> Result<(), MacosNativeOperationError> { /* moved body */ }
```

Update the default trait methods in `api.rs` to call into `focus` and `geometry`.

**Step 4: Run test to verify it passes**

Run: `cargo test -p macos_window_manager source_focus_and_geometry_helpers_leave_lib_rs -- --exact --nocapture`

Expected: PASS

Then run focused regression slices:

```bash
cargo test -p macos_window_manager \
  backend_focus_direction_preflights_same_pid_splitview_ax_target_before_focus_attempt \
  -- --exact --nocapture

cargo test -p macos_window_manager \
  backend_focus_direction_keeps_selected_target_when_next_snapshot_drops_it \
  -- --exact --nocapture
```

Expected: PASS for both tests.

**Step 5: Commit**

```bash
git add crates/macos_window_manager/src/lib.rs \
        crates/macos_window_manager/src/api.rs \
        crates/macos_window_manager/src/geometry.rs \
        crates/macos_window_manager/src/focus.rs \
        crates/macos_window_manager/tests/boundary_source.rs
git commit -m "refactor: extract macos backend focus helpers"
```

### Task 4: Replace the monolithic `src/tests.rs` with split concern-based test modules

**Files:**
- Delete: `crates/macos_window_manager/src/tests.rs`
- Create: `crates/macos_window_manager/src/tests/mod.rs`
- Create: `crates/macos_window_manager/src/tests/support.rs`
- Create: `crates/macos_window_manager/src/tests/navigation.rs`
- Create: `crates/macos_window_manager/src/tests/focus.rs`
- Create: `crates/macos_window_manager/src/tests/real_api.rs`
- Create: `crates/macos_window_manager/src/tests/topology.rs`
- Modify: `crates/macos_window_manager/src/lib.rs`
- Test: `crates/macos_window_manager/tests/boundary_source.rs`

**Step 1: Write the failing test**

Add a new source-shape test:

```rust
#[test]
fn source_tests_are_split_by_concern() {
    assert!(crate_source("src/tests/mod.rs").exists());
    assert!(crate_source("src/tests/support.rs").exists());
    assert!(!crate_source("src/tests.rs").exists());
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p macos_window_manager source_tests_are_split_by_concern -- --exact --nocapture`

Expected: FAIL because `src/tests.rs` still exists and the new files do not.

**Step 3: Write minimal implementation**

Create a directory-style test module:

```rust
// src/tests/mod.rs
mod support;
mod navigation;
mod focus;
mod real_api;
mod topology;
```

Move fake APIs, builders, and helpers into `support.rs`, and move concern-specific tests into the new files with the smallest diff possible.

**Step 4: Run test to verify it passes**

Run: `cargo test -p macos_window_manager source_tests_are_split_by_concern -- --exact --nocapture`

Expected: PASS

Then run the full crate test suite:

Run: `cargo test -p macos_window_manager`

Expected: PASS

**Step 5: Commit**

```bash
git add crates/macos_window_manager/src/lib.rs \
        crates/macos_window_manager/src/tests
git commit -m "test: split macos backend tests by concern"
```

### Task 5: Clean up crate-root exports and run end-to-end verification

**Files:**
- Modify: `crates/macos_window_manager/src/lib.rs`
- Modify: `crates/macos_window_manager/tests/public_api.rs`
- Modify: `src/adapters/window_managers/macos_native.rs` (only if import adjustments are required by public-surface cleanup)
- Test: `crates/macos_window_manager/tests/public_api.rs`
- Test: `crates/macos_window_manager/tests/boundary_source.rs`

**Step 1: Write the failing test**

If the facade is still noisy, add or tighten a source/public-surface test:

```rust
#[test]
fn source_backend_crate_stays_facade_focused() {
    let lib = std::fs::read_to_string(crate_source("src/lib.rs")).unwrap();
    assert!(lib.lines().count() < 400, "crate root should stay thin");
}
```

If line-count is too brittle, replace the numeric assertion with marker assertions against stray helper functions still present in `lib.rs`.

**Step 2: Run test to verify it fails**

Run: `cargo test -p macos_window_manager source_backend_crate_stays_facade_focused -- --exact --nocapture`

Expected: FAIL if helper leakage remains.

**Step 3: Write minimal implementation**

Prune `lib.rs` down to module declarations and curated re-exports such as:

```rust
mod api;
mod environment;
mod focus;
mod geometry;
mod navigation;
mod real_api;

pub use api::{ /* public types + trait */ };
pub use desktop_topology_snapshot::{ /* approved raw exports */ };
pub use error::{MacosNativeConnectError, MacosNativeOperationError, MacosNativeProbeError};
pub use real_api::RealNativeApi;
```

Adjust `public_api.rs` and outer adapter imports only if required by the facade cleanup.

**Step 4: Run test to verify it passes**

Run the full verification chain:

```bash
cargo test -p macos_window_manager
cargo test macos_native --lib -- --nocapture
cargo build --release
```

Expected: PASS for all three commands.

**Step 5: Commit**

```bash
git add crates/macos_window_manager/src/lib.rs \
        crates/macos_window_manager/tests/public_api.rs \
        crates/macos_window_manager/tests/boundary_source.rs \
        src/adapters/window_managers/macos_native.rs
git commit -m "refactor: finalize macos backend crate cleanup"
```

## Final verification checklist

- `cargo test -p macos_window_manager`
- `cargo test macos_native --lib -- --nocapture`
- `cargo build --release`
- `git --no-pager diff --stat`
- `git --no-pager status`

## Notes for the implementer

- Prefer moving code blocks verbatim before renaming anything.
- If a source-shape test becomes too brittle, replace it with marker assertions that prove the architectural move without depending on exact formatting.
- Keep raw-topology behavior in the backend crate and keep the outer adapter thin.
- Do not broaden scope into new behavior or unrelated warning cleanup unless the refactor forces it.
