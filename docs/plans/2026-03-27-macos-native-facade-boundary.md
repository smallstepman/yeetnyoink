# macOS Native Facade Boundary Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make `src/adapters/window_managers/macos_native.rs` depend only on a small adapter-facing
macOS backend facade instead of raw helper functions, constants, and topology primitives from
`macos_window_manager_api`.

**Architecture:** Keep the existing `mod macos_window_manager_api` as the place where macOS-specific
state collection, classification, and execution live, but change the public boundary so outer
`macos_native.rs` consumes only facade methods plus semantic plan/result types. Move connection
validation, record construction, directional planning, and settle loops behind that facade so the outer
adapter reads more like `src/adapters/window_managers/niri.rs`.

**Tech Stack:** Rust, existing `src/adapters/window_managers/macos_native.rs` test suite, existing
WM engine record types in `src/engine/wm`, `cargo test`, `rustfmt`.

---

**Skill refs:** @test-driven-development @verification-before-completion @using-git-worktrees

**Worktree:** Create a fresh worktree from `/Users/m/Projects/yeetnyoink` before executing this plan.

### Task 1: Lock the boundary with failing source-shape tests

**Files:**
- Modify: `src/adapters/window_managers/macos_native.rs`
- Test: `src/adapters/window_managers/macos_native.rs`

**Step 1: Write the failing test**

Add source-shape tests that explicitly reject raw backend imports in the root production prelude:

```rust
#[test]
fn source_keeps_raw_macos_backend_items_private() {
    let implementation = implementation_source();
    let api_module_idx = implementation
        .find("mod macos_window_manager_api {")
        .expect("implementation should define mod macos_window_manager_api");
    let root_prefix = &implementation[..api_module_idx];

    for forbidden in [
        "RawTopologySnapshot",
        "WindowSnapshot",
        "SpaceKind",
        "REQUIRED_PRIVATE_SYMBOLS",
        "SPACE_SWITCH_SETTLE_TIMEOUT",
        "SPACE_SWITCH_POLL_INTERVAL",
        "SPACE_SWITCH_STABLE_TARGET_POLLS",
        "active_window_pid_from_topology",
        "best_window_id_from_windows",
        "directional_focus_target_in_active_topology",
    ] {
        assert!(
            !root_prefix.contains(forbidden),
            "root production prelude should not import raw backend item {forbidden}"
        );
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib source_keeps_raw_macos_backend_items_private -- --nocapture`

Expected: FAIL because the current root import block still pulls raw topology/helper items from
`macos_window_manager_api`.

**Step 3: Write minimal implementation**

Keep the new test in place and add only the smallest facade-contract placeholders needed for later tasks,
for example:

```rust
pub(crate) enum DirectionalFocusPlan {
    None,
}
```

Do not move behavior yet; just establish the red test and any compile scaffolding needed for the next
tasks.

**Step 4: Run test to verify it still fails for the right reason**

Run: `cargo test --lib source_keeps_raw_macos_backend_items_private -- --nocapture`

Expected: FAIL because the raw imports still exist, not because the new facade placeholders fail to
compile.

**Step 5: Commit**

```bash
git add src/adapters/window_managers/macos_native.rs
git commit -m "test: lock macos native facade boundary" -m "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

### Task 2: Define the adapter-facing facade contract

**Files:**
- Modify: `src/adapters/window_managers/macos_native.rs`
- Test: `src/adapters/window_managers/macos_native.rs`

**Step 1: Write the failing test**

Add compile-time / source-shape tests that require the new semantic facade types to exist and be used by
the outer adapter:

```rust
#[test]
fn source_imports_semantic_backend_plan_types() {
    let implementation = implementation_source();
    assert!(implementation.contains("DirectionalFocusPlan"));
    assert!(!implementation.contains("use macos_window_manager_api::{\n    RawTopologySnapshot"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib source_imports_semantic_backend_plan_types -- --nocapture`

Expected: FAIL because the semantic plan/result types do not exist yet.

**Step 3: Write minimal implementation**

Inside `mod macos_window_manager_api`, define small adapter-facing types and trait methods:

```rust
pub(crate) enum DirectionalFocusPlan {
    InSpace {
        target_window_id: u64,
        target_pid: Option<u32>,
    },
    AdjacentSpace {
        target_space_id: u64,
        direction: Direction,
    },
    None,
}

pub(crate) trait MacosNativeApi {
    fn validate_environment(&self) -> Result<(), MacosNativeConnectError>;
    fn plan_focus_direction(
        &self,
        direction: Direction,
        strategy: FloatingFocusStrategy,
    ) -> Result<DirectionalFocusPlan, MacosNativeOperationError>;
}
```

Keep implementations stubbed if needed; the goal here is to create the public contract, not move all
logic yet.

**Step 4: Run test to verify it passes**

Run: `cargo test --lib source_imports_semantic_backend_plan_types -- --nocapture`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/adapters/window_managers/macos_native.rs
git commit -m "refactor: define macos native facade contract" -m "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

### Task 3: Move connection validation behind the facade

**Files:**
- Modify: `src/adapters/window_managers/macos_native.rs`
- Test: `src/adapters/window_managers/macos_native.rs`

**Step 1: Write the failing test**

Add a source-shape test that forbids the outer context from iterating raw symbol lists:

```rust
#[test]
fn source_keeps_required_private_symbols_inside_backend() {
    let implementation = implementation_source();
    let api_module_idx = implementation.find("mod macos_window_manager_api {").unwrap();
    let root_prefix = &implementation[..api_module_idx];
    assert!(!root_prefix.contains("REQUIRED_PRIVATE_SYMBOLS"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib source_keeps_required_private_symbols_inside_backend -- --nocapture`

Expected: FAIL because `MacosNativeContext::connect_with_api(...)` still inspects
`REQUIRED_PRIVATE_SYMBOLS` directly.

**Step 3: Write minimal implementation**

Move that validation into the trait implementation:

```rust
impl MacosNativeApi for RealNativeApi {
    fn validate_environment(&self) -> Result<(), MacosNativeConnectError> {
        for symbol in REQUIRED_PRIVATE_SYMBOLS {
            if !self.has_symbol(symbol) {
                return Err(MacosNativeConnectError::MissingRequiredSymbol(symbol));
            }
        }
        if !self.ax_is_trusted() {
            return Err(MacosNativeConnectError::MissingAccessibilityPermission);
        }
        if !self.minimal_topology_ready() {
            return Err(MacosNativeConnectError::MissingTopologyPrecondition("main SkyLight connection"));
        }
        Ok(())
    }
}
```

Then make `MacosNativeContext::connect_with_api(...)` call only `api.validate_environment()?`.

**Step 4: Run test to verify it passes**

Run: `cargo test --lib source_keeps_required_private_symbols_inside_backend -- --nocapture`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/adapters/window_managers/macos_native.rs
git commit -m "refactor: hide macos connection validation behind facade" -m "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

### Task 4: Move record construction behind the facade

**Files:**
- Modify: `src/adapters/window_managers/macos_native.rs`
- Test: `src/adapters/window_managers/macos_native.rs`

**Step 1: Write the failing test**

Add a source-shape test that forbids the outer adapter from importing raw snapshot structs:

```rust
#[test]
fn source_keeps_raw_snapshot_types_inside_backend() {
    let implementation = implementation_source();
    let api_module_idx = implementation.find("mod macos_window_manager_api {").unwrap();
    let root_prefix = &implementation[..api_module_idx];
    assert!(!root_prefix.contains("RawTopologySnapshot"));
    assert!(!root_prefix.contains("WindowSnapshot"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib source_keeps_raw_snapshot_types_inside_backend -- --nocapture`

Expected: FAIL because outer production code still imports and manipulates raw snapshot types.

**Step 3: Write minimal implementation**

Push query/record conversion behind the facade:

- add or expand backend methods that return `FocusedWindowRecord`, `FocusedAppRecord`, and
  `Vec<WindowRecord>`,
- update outer `focused_window()` / `windows()` call sites to use those directly,
- stop importing `RawTopologySnapshot` and `WindowSnapshot` in root production code.

Prefer code like:

```rust
fn focused_window(&mut self) -> anyhow::Result<FocusedWindowRecord> {
    self.ctx.api.focused_window_record().map_err(map_probe_error)
}
```

instead of reconstructing records from raw snapshots in the outer layer.

**Step 4: Run test to verify it passes**

Run: `cargo test --lib source_keeps_raw_snapshot_types_inside_backend -- --nocapture`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/adapters/window_managers/macos_native.rs
git commit -m "refactor: move macos record construction behind facade" -m "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

### Task 5: Move directional focus planning and settle logic behind the facade

**Files:**
- Modify: `src/adapters/window_managers/macos_native.rs`
- Test: `src/adapters/window_managers/macos_native.rs`

**Step 1: Write the failing test**

Extend the source-shape tests to forbid directional helper leakage:

```rust
#[test]
fn source_keeps_directional_selection_helpers_inside_backend() {
    let implementation = implementation_source();
    let api_module_idx = implementation.find("mod macos_window_manager_api {").unwrap();
    let root_prefix = &implementation[..api_module_idx];

    for forbidden in [
        "active_window_pid_from_topology",
        "adjacent_space_in_direction",
        "best_window_id_from_windows",
        "classify_space",
        "directional_focus_target_in_active_topology",
        "ensure_supported_target_space",
        "space_transition_window_ids",
        "window_ids_for_space",
        "SPACE_SWITCH_SETTLE_TIMEOUT",
        "SPACE_SWITCH_POLL_INTERVAL",
        "SPACE_SWITCH_STABLE_TARGET_POLLS",
    ] {
        assert!(!root_prefix.contains(forbidden));
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib source_keeps_directional_selection_helpers_inside_backend -- --nocapture`

Expected: FAIL because directional focus in outer production code still depends on those helpers.

**Step 3: Write minimal implementation**

Move that behavior behind semantic facade methods:

```rust
fn plan_focus_direction(
    &self,
    direction: Direction,
    strategy: FloatingFocusStrategy,
) -> Result<DirectionalFocusPlan, MacosNativeOperationError>;

fn execute_focus_plan(&self, plan: &DirectionalFocusPlan) -> Result<(), MacosNativeOperationError>;
```

Then update outer `focus_direction(...)` to:

1. resolve the repo-level strategy,
2. ask the backend for the plan,
3. ask the backend to execute it,
4. return the backend result.

Keep all space-switch settling and pid fast-path decisions inside the backend implementation.

**Step 4: Run test to verify it passes**

Run: `cargo test --lib source_keeps_directional_selection_helpers_inside_backend -- --nocapture`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/adapters/window_managers/macos_native.rs
git commit -m "refactor: move macos directional focus behind facade" -m "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

### Task 6: Move direct focus / move helpers behind the facade

**Files:**
- Modify: `src/adapters/window_managers/macos_native.rs`
- Test: `src/adapters/window_managers/macos_native.rs`

**Step 1: Write the failing test**

Add a regression test that ensures the outer import prelude no longer needs raw backend helper items:

```rust
#[test]
fn source_limits_root_macos_imports_to_facade_contract() {
    let implementation = implementation_source();
    let api_module_idx = implementation.find("mod macos_window_manager_api {").unwrap();
    let root_prefix = &implementation[..api_module_idx];

    assert!(root_prefix.contains("use macos_window_manager_api::{"));
    assert!(!root_prefix.contains("space_id_for_window"));
    assert!(!root_prefix.contains("active_directed_rects"));
    assert!(!root_prefix.contains("window_snapshots_from_topology"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib source_limits_root_macos_imports_to_facade_contract -- --nocapture`

Expected: FAIL because outer code still imports some helper items directly.

**Step 3: Write minimal implementation**

Finish moving direct operations behind facade methods such as:

```rust
fn focus_window_by_id(&self, window_id: u64) -> Result<(), MacosNativeOperationError>;
fn move_window_to_space_checked(
    &self,
    window_id: u64,
    space_id: u64,
) -> Result<(), MacosNativeOperationError>;
fn swap_directional_neighbor(&self, direction: Direction) -> Result<(), MacosNativeOperationError>;
```

Then shrink the root production import block to only the facade contract items the outer adapter truly
uses.

**Step 4: Run test to verify it passes**

Run: `cargo test --lib source_limits_root_macos_imports_to_facade_contract -- --nocapture`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/adapters/window_managers/macos_native.rs
git commit -m "refactor: finish macos facade boundary cleanup" -m "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

### Task 7: Re-run the behavioral macOS-native test slice

**Files:**
- Modify: `src/adapters/window_managers/macos_native.rs` (if test fixes are needed)
- Test: `src/adapters/window_managers/macos_native.rs`

**Step 1: Run the focused boundary tests**

Run:

```bash
cargo test --lib source_keeps_raw_macos_backend_items_private -- --nocapture
cargo test --lib source_keeps_required_private_symbols_inside_backend -- --nocapture
cargo test --lib source_keeps_raw_snapshot_types_inside_backend -- --nocapture
cargo test --lib source_keeps_directional_selection_helpers_inside_backend -- --nocapture
cargo test --lib source_limits_root_macos_imports_to_facade_contract -- --nocapture
```

Expected: PASS.

**Step 2: Run the existing macOS-native slice**

Run: `cargo test macos_native --lib -- --nocapture`

Expected: PASS. Any failure here is a regression from the boundary refactor.

**Step 3: Format the touched files**

Run: `rustfmt --edition 2024 src/adapters/window_managers/macos_native.rs`

Expected: success with no diff afterward other than the intended refactor.

**Step 4: Review the final diff**

Run:

```bash
git --no-pager diff -- src/adapters/window_managers/macos_native.rs
```

Expected: outer `macos_native.rs` reads as adapter glue, while raw topology/helper churn stays inside
`mod macos_window_manager_api`.

**Step 5: Commit**

```bash
git add src/adapters/window_managers/macos_native.rs
git commit -m "refactor: enforce macos native facade boundary" -m "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```
