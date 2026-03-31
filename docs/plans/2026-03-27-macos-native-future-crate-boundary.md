# macOS Native Future-Crate Boundary Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Reshape `macos_window_manager_api` into a future-crate-ready native backend while keeping it in-place, and move topology, policy, and WM-record conversion back into outer `macos_native.rs`.

**Architecture:** Introduce backend-owned snapshot, id, bounds, options, and diagnostics types, then drive the outer adapter from `desktop_snapshot()` plus explicit native actions. Treat the current semantic facade (`plan_focus_direction`, `execute_focus_plan`, backend-returned WM records) as transitional: migrate callers to yeetnyoink-owned topology/policy first, then delete the old facade surface once the new boundary is fully wired.

**Tech Stack:** Rust, `src/adapters/window_managers/macos_native.rs`, existing macOS-native test suite, `cargo test`, `rustfmt --edition 2024`

---

**Skill refs:** @test-driven-development @verification-before-completion @subagent-driven-development

**Worktree:** `/Users/m/Projects/yeetnyoink/.worktrees/macos-native-facade-boundary`

**Design doc:** `docs/plans/2026-03-27-macos-native-future-crate-boundary-design.md`

**Formatting note:** Format only the touched file with `rustfmt --edition 2024 src/adapters/window_managers/macos_native.rs`. Do not use `cargo fmt --all --check`; the repository currently has unrelated formatting drift outside this feature area.

### Task 1: Introduce backend-owned transport and options types

**Files:**
- Modify: `src/adapters/window_managers/macos_native.rs`
- Test: `src/adapters/window_managers/macos_native.rs`

**Step 1: Write the failing test**

Add source-shape tests near the existing boundary guards that require backend-owned shells for the new boundary:

```rust
#[test]
fn source_declares_backend_owned_native_transport_types() {
    let implementation = implementation_source();
    for required in [
        "pub(crate) struct NativeDesktopSnapshot",
        "pub(crate) struct NativeSpaceSnapshot",
        "pub(crate) struct NativeWindowSnapshot",
        "pub(crate) struct NativeBounds",
        "pub(crate) struct NativeBackendOptions",
        "pub(crate) trait NativeDiagnostics",
    ] {
        assert!(
            implementation.contains(required),
            "expected backend boundary to declare {required}"
        );
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib source_declares_backend_owned_native_transport_types -- --nocapture`

Expected: FAIL because those backend-owned boundary types do not exist yet.

**Step 3: Write minimal implementation**

Inside `mod macos_window_manager_api`, add shell types only. Do not migrate behavior yet.

```rust
pub(crate) struct NativeDesktopSnapshot {
    pub(crate) spaces: Vec<NativeSpaceSnapshot>,
    pub(crate) active_space_ids: HashSet<NativeSpaceId>,
    pub(crate) windows: Vec<NativeWindowSnapshot>,
    pub(crate) focused_window_id: Option<NativeWindowId>,
}

pub(crate) struct NativeSpaceSnapshot { /* id, display_index, active, kind */ }
pub(crate) struct NativeWindowSnapshot { /* id, pid, app_id, title, bounds, space_id, ... */ }
pub(crate) struct NativeBounds { pub(crate) x: i32, pub(crate) y: i32, pub(crate) width: i32, pub(crate) height: i32 }
pub(crate) struct NativeBackendOptions { pub(crate) mission_control: MissionControlModifiers, pub(crate) diagnostics: Option<Arc<dyn NativeDiagnostics>> }
pub(crate) trait NativeDiagnostics: Send + Sync { fn debug(&self, message: &str); }
```

Use backend-owned id aliases or newtypes now. Keep them private to the backend module until later tasks expose them through the trait.

**Step 4: Run test to verify it passes**

Run: `cargo test --lib source_declares_backend_owned_native_transport_types -- --nocapture`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/adapters/window_managers/macos_native.rs
git commit -m "refactor: add macos native boundary transport types" -m "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

### Task 2: Add snapshot-first backend query surface

**Files:**
- Modify: `src/adapters/window_managers/macos_native.rs`
- Test: `src/adapters/window_managers/macos_native.rs`

**Step 1: Write the failing test**

Add a source-shape test that requires the backend trait to expose the new snapshot query:

```rust
#[test]
fn source_exposes_desktop_snapshot_query() {
    let implementation = implementation_source();
    assert!(implementation.contains(
        "fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError>;"
    ));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib source_exposes_desktop_snapshot_query -- --nocapture`

Expected: FAIL because `MacosNativeApi` does not yet expose `desktop_snapshot()`.

**Step 3: Write minimal implementation**

Add a canonical query method to `MacosNativeApi` and implement it for `RealNativeApi` by translating existing raw topology assembly into the new DTOs.

```rust
pub(crate) trait MacosNativeApi {
    fn desktop_snapshot(&self) -> Result<NativeDesktopSnapshot, MacosNativeProbeError>;
    fn focused_window_id(&self) -> Result<Option<NativeWindowId>, MacosNativeProbeError>;
    fn onscreen_window_ids(&self) -> Result<HashSet<NativeWindowId>, MacosNativeProbeError>;
}
```

Preserve the old helpers temporarily if outer production code still depends on them, but make `desktop_snapshot()` the new canonical read path. Update fake APIs in tests to implement it directly.

**Step 4: Run test to verify it passes**

Run: `cargo test --lib source_exposes_desktop_snapshot_query -- --nocapture`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/adapters/window_managers/macos_native.rs
git commit -m "refactor: add macos desktop snapshot query" -m "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

### Task 3: Move WM-record translation into the outer adapter

**Files:**
- Modify: `src/adapters/window_managers/macos_native.rs`
- Test: `src/adapters/window_managers/macos_native.rs`

**Step 1: Write the failing test**

Add a focused outer-adapter behavior test that proves `focused_window()`, `focused_app()`, and `windows()` can be derived from `desktop_snapshot()` without backend record helpers:

```rust
#[test]
fn focused_window_and_windows_are_derived_from_native_snapshot() {
    let adapter = MacosNativeAdapter::connect_with_api(SnapshotOnlyApi::new(/* snapshot */)).unwrap();

    let focused = adapter.session().focused_window().unwrap();
    let windows = adapter.session().windows().unwrap();

    assert_eq!(focused.id, 101);
    assert_eq!(windows.len(), 2);
}
```

Build the fake so it implements `desktop_snapshot()` but leaves backend `focused_window_record()` / `window_records()` unavailable or panicking. The test should fail until outer code stops using those old helpers.

**Step 2: Run test to verify it fails**

Run: `cargo test --lib focused_window_and_windows_are_derived_from_native_snapshot -- --nocapture`

Expected: FAIL because outer production code still calls backend record helpers.

**Step 3: Write minimal implementation**

Add outer conversion helpers and switch `WindowManagerSession` methods to them:

```rust
fn process_id_from_native(pid: Option<u32>) -> Option<ProcessId> { /* ... */ }

fn window_records_from_native(snapshot: &NativeDesktopSnapshot) -> Vec<WindowRecord> { /* ... */ }

fn focused_window_record_from_native(snapshot: &NativeDesktopSnapshot) -> anyhow::Result<FocusedWindowRecord> { /* ... */ }

fn focused_app_record_from_native(snapshot: &NativeDesktopSnapshot) -> anyhow::Result<Option<FocusedAppRecord>> { /* ... */ }
```

Use `desktop_snapshot()` in the outer adapter and derive WM records there. Keep the old backend record helpers compiling for now only if other code still needs them temporarily.

**Step 4: Run test to verify it passes**

Run: `cargo test --lib focused_window_and_windows_are_derived_from_native_snapshot -- --nocapture`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/adapters/window_managers/macos_native.rs
git commit -m "refactor: derive wm records from macos snapshot" -m "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

### Task 4: Build yeetnyoink topology from backend DTOs in outer code

**Files:**
- Modify: `src/adapters/window_managers/macos_native.rs`
- Test: `src/adapters/window_managers/macos_native.rs`

**Step 1: Write the failing test**

Add an outer-policy test that requires focus target selection from DTO-derived geometry instead of backend planning:

```rust
#[test]
fn native_snapshot_can_drive_outer_directional_selection() {
    let snapshot = NativeDesktopSnapshot { /* two windows west/east with bounds */ };
    let topology = outer_topology_from_native_snapshot(&snapshot).unwrap();

    let target = select_closest_in_direction_with_strategy(
        &topology.rects,
        101,
        Direction::West,
        None,
    );

    assert_eq!(target, Some(100));
}
```

The test should fail until there is an outer helper that turns backend DTOs into yeetnyoink-owned geometry/topology input.

**Step 2: Run test to verify it fails**

Run: `cargo test --lib native_snapshot_can_drive_outer_directional_selection -- --nocapture`

Expected: FAIL because the outer adapter does not yet build topology from `NativeDesktopSnapshot`.

**Step 3: Write minimal implementation**

Add outer-only helpers that map backend DTOs into yeetnyoink geometry:

```rust
fn rect_from_native(bounds: NativeBounds) -> Rect { /* ... */ }

fn outer_topology_from_native_snapshot(
    snapshot: &NativeDesktopSnapshot,
) -> anyhow::Result<OuterMacosTopology> { /* ... */ }
```

Do not re-export those helpers from the backend. This task is where geometry ownership starts moving back to yeetnyoink in concrete code.

**Step 4: Run test to verify it passes**

Run: `cargo test --lib native_snapshot_can_drive_outer_directional_selection -- --nocapture`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/adapters/window_managers/macos_native.rs
git commit -m "refactor: build macos topology outside backend" -m "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

### Task 5: Move directional focus policy out of the backend

**Files:**
- Modify: `src/adapters/window_managers/macos_native.rs`
- Test: `src/adapters/window_managers/macos_native.rs`

**Step 1: Write the failing test**

Add an outer behavior test with a recording fake API that proves focus routing is outer-driven:

```rust
#[test]
fn focus_direction_uses_outer_policy_with_native_snapshot() {
    let api = RecordingFocusApi::from_snapshot(/* focused window + west target */);
    let mut session = MacosNativeAdapter::connect_with_api(api).unwrap().session();

    session.focus_direction(Direction::West).unwrap();

    assert_eq!(
        session.api_calls(),
        vec![NativeCall::DesktopSnapshot, NativeCall::FocusWindowWithPid(100, 2000)]
    );
}
```

The fake should panic if outer code calls `plan_focus_direction()` or `execute_focus_plan()`. The test should fail until focus is driven by `desktop_snapshot()` plus explicit actions.

**Step 2: Run test to verify it fails**

Run: `cargo test --lib focus_direction_uses_outer_policy_with_native_snapshot -- --nocapture`

Expected: FAIL because focus still flows through backend planning helpers.

**Step 3: Write minimal implementation**

In outer `WindowManagerSession::focus_direction(...)`:

```rust
let snapshot = self.api.desktop_snapshot()?;
let topology = outer_topology_from_native_snapshot(&snapshot)?;
let target = select_focus_target_from_outer_topology(&topology, direction, strategy)?;

match target {
    FocusTarget::SameSpace { window_id, pid } => self.api.focus_window_with_pid(window_id, pid),
    FocusTarget::CrossSpace { target_space_id, target_window_id, pid, method } => { /* switch, resnapshot if needed, then focus */ }
}
```

Keep native mechanics inside backend action methods, but move all target selection and policy branching into outer code.

**Step 4: Run test to verify it passes**

Run: `cargo test --lib focus_direction_uses_outer_policy_with_native_snapshot -- --nocapture`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/adapters/window_managers/macos_native.rs
git commit -m "refactor: move macos focus policy outside backend" -m "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

### Task 6: Move directional move and space-move policy out of the backend

**Files:**
- Modify: `src/adapters/window_managers/macos_native.rs`
- Test: `src/adapters/window_managers/macos_native.rs`

**Step 1: Write the failing test**

Add recording-fake tests for move routing:

```rust
#[test]
fn move_direction_uses_outer_geometry_and_backend_frame_actions() {
    let api = RecordingMoveApi::from_snapshot(/* focused + west neighbor bounds */);
    let mut session = MacosNativeAdapter::connect_with_api(api).unwrap().session();

    session.move_direction(Direction::West).unwrap();

    assert_eq!(
        session.api_calls(),
        vec![
            NativeCall::DesktopSnapshot,
            NativeCall::SwapWindowFrames { source: 101, target: 100 },
        ]
    );
}
```

Also add one test for cross-space move to ensure outer code chooses the target space from DTO-derived topology and then calls `move_window_to_space(...)`.

**Step 2: Run test to verify it fails**

Run: `cargo test --lib move_direction_uses_outer_geometry_and_backend_frame_actions -- --nocapture`

Expected: FAIL because move still relies on backend `swap_directional_neighbor()` / checked move helpers.

**Step 3: Write minimal implementation**

Move move-target selection into outer code and keep backend actions primitive:

```rust
let snapshot = self.api.desktop_snapshot()?;
let topology = outer_topology_from_native_snapshot(&snapshot)?;
let target = select_move_target_from_outer_topology(&topology, direction)?;

match target {
    MoveTarget::NeighborSwap { source_window_id, source_frame, target_window_id, target_frame } => {
        self.api.swap_window_frames(source_window_id, source_frame, target_window_id, target_frame)
    }
    MoveTarget::CrossSpace { window_id, target_space_id } => {
        self.api.move_window_to_space(window_id, target_space_id)
    }
}
```

Do not leave geometry neighbor selection inside backend defaults.

**Step 4: Run test to verify it passes**

Run: `cargo test --lib move_direction_uses_outer_geometry_and_backend_frame_actions -- --nocapture`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/adapters/window_managers/macos_native.rs
git commit -m "refactor: move macos move policy outside backend" -m "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

### Task 7: Inject backend-owned config and diagnostics, then remove repo-type imports

**Files:**
- Modify: `src/adapters/window_managers/macos_native.rs`
- Test: `src/adapters/window_managers/macos_native.rs`

**Step 1: Write the failing test**

Add source-shape guards that target the backend module slice instead of the whole file:

```rust
#[test]
fn source_backend_module_avoids_repo_config_and_logging_imports() {
    let backend = backend_module_source();
    for forbidden in [
        "use crate::config",
        "MissionControlShortcutConfig",
        "use crate::logging",
        "crate::logging::",
    ] {
        assert!(
            !backend.contains(forbidden),
            "backend module should not depend on {forbidden}"
        );
    }
}
```

The test should fail until the backend is constructed with backend-owned options and an optional diagnostics sink.

**Step 2: Run test to verify it fails**

Run: `cargo test --lib source_backend_module_avoids_repo_config_and_logging_imports -- --nocapture`

Expected: FAIL because `foundation` still imports repo config and the backend still logs directly.

**Step 3: Write minimal implementation**

Move config/logging ownership to the adapter edge:

```rust
pub(crate) struct MissionControlModifiers {
    pub(crate) control: bool,
    pub(crate) option: bool,
    pub(crate) command: bool,
    pub(crate) shift: bool,
    pub(crate) function: bool,
}

impl RealNativeApi {
    fn new(options: NativeBackendOptions) -> Self { /* store options */ }
}

impl NativeDiagnostics for TracingDiagnostics {
    fn debug(&self, message: &str) {
        logging::debug(message.to_owned());
    }
}
```

Read repo config in outer `MacosNativeContext` / adapter construction, translate it into `NativeBackendOptions`, and pass those into the backend. Replace direct backend logging with `self.options.diagnostics.as_ref().map(...)`.

**Step 4: Run test to verify it passes**

Run: `cargo test --lib source_backend_module_avoids_repo_config_and_logging_imports -- --nocapture`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/adapters/window_managers/macos_native.rs
git commit -m "refactor: inject macos backend options and diagnostics" -m "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

### Task 8: Delete the transitional semantic facade and lock the final boundary

**Files:**
- Modify: `src/adapters/window_managers/macos_native.rs`
- Test: `src/adapters/window_managers/macos_native.rs`

**Step 1: Write the failing test**

Add the final regression guard for the backend public surface and outer adapter region:

```rust
#[test]
fn source_backend_boundary_is_future_crate_ready() {
    let backend_public = backend_public_api_source();
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
        "swap_directional_neighbor(",
        "move_window_to_space_checked(",
    ] {
        assert!(
            !backend_public.contains(forbidden),
            "backend public api should not expose {forbidden}"
        );
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib source_backend_boundary_is_future_crate_ready -- --nocapture`

Expected: FAIL because the transitional semantic facade still exists.

**Step 3: Write minimal implementation**

Delete obsolete semantic API methods and dead helpers once all outer callers have been migrated. Remove now-unused imports and clean up the fake APIs so they only implement the snapshot-first boundary plus primitive actions.

After the deletions, run:

```bash
rustfmt --edition 2024 src/adapters/window_managers/macos_native.rs
```

**Step 4: Run targeted and full verification**

Run:

```bash
cargo test --lib source_backend_boundary_is_future_crate_ready -- --nocapture
cargo test macos_native --lib -- --nocapture
```

Expected: PASS for the new boundary guard and PASS for the full macOS-native test suite.

**Step 5: Commit**

```bash
git add src/adapters/window_managers/macos_native.rs
git commit -m "refactor: finalize macos future-crate boundary" -m "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

## Final verification checklist

Run these before reporting success:

```bash
rustfmt --edition 2024 src/adapters/window_managers/macos_native.rs
cargo test --lib source_declares_backend_owned_native_transport_types -- --nocapture
cargo test --lib source_exposes_desktop_snapshot_query -- --nocapture
cargo test --lib focused_window_and_windows_are_derived_from_native_snapshot -- --nocapture
cargo test --lib native_snapshot_can_drive_outer_directional_selection -- --nocapture
cargo test --lib focus_direction_uses_outer_policy_with_native_snapshot -- --nocapture
cargo test --lib move_direction_uses_outer_geometry_and_backend_frame_actions -- --nocapture
cargo test --lib source_backend_module_avoids_repo_config_and_logging_imports -- --nocapture
cargo test --lib source_backend_boundary_is_future_crate_ready -- --nocapture
cargo test macos_native --lib -- --nocapture
```

If any targeted test name changes during implementation, update this checklist immediately in the same commit that renames the test.
