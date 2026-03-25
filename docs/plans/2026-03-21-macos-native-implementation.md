# macOS Native WM Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the research-only `src/adapters/window_managers/macos_native.rs` stub with a compiled, tested macOS-only shared support module that exposes private Spaces-aware snapshots and fail-fast focus/move operations for current macOS adapters to consume.

**Architecture:** Build `src/adapters/window_managers/macos_native.rs` as a macOS-only shared helper layered into unsafe FFI bindings, safe snapshot/classification types, topology probing, and explicit operations. Wire it into `src/adapters/window_managers/mod.rs`, classify fullscreen and Split View as first-class Spaces, keep Stage Manager intentionally opaque, and make initialization fail immediately when required private symbols or permissions are missing.

**Tech Stack:** Rust, macOS-only FFI crates (`accessibility-sys`, `objc2-core-foundation`, `objc2-core-graphics`, `libc` if needed), existing WM adapter module tree, `cargo build --target-dir target`, `cargo test --target-dir target`.

---

**Skill refs:** @test-driven-development @verification-before-completion @using-git-worktrees

**Worktree:** Execute from `/Users/m/Projects/yeetnyoink/.worktrees/macos-native-impl`

**Baseline:** In this worktree, `cargo build --target-dir target -q` passes. `cargo test --target-dir target -q` currently has 13 unrelated baseline failures (12 `vscode` tests failing to connect to `ws://127.0.0.1:3710`, plus 1 `zellij` assertion failure). Treat new `macos_native` failures as regressions; the existing 13 are pre-existing.

### Task 1: Wire the macOS-native module into the build

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/adapters/window_managers/mod.rs`
- Create: `src/adapters/window_managers/macos_native.rs`
- Test: `src/adapters/window_managers/mod.rs`

**Step 1: Write the failing test**

In `src/adapters/window_managers/mod.rs`, add a macOS-gated test that imports the new module boundary:

```rust
#[cfg(target_os = "macos")]
#[test]
fn macos_native_space_kind_symbols_are_exposed() {
    use crate::adapters::window_managers::macos_native::SpaceKind;

    assert_eq!(SpaceKind::Desktop.as_str(), "desktop");
    assert_eq!(
        SpaceKind::StageManagerOpaque.as_str(),
        "stage_manager_opaque"
    );
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --target-dir target -q macos_native_space_kind_symbols_are_exposed`

Expected: FAIL because `macos_native` is not in the adapter module tree yet and `SpaceKind` does not exist.

**Step 3: Write minimal implementation**

- In `Cargo.toml`, add the macOS-only dependencies needed to represent CF / CG / AX types cleanly.
- In `src/adapters/window_managers/mod.rs`, add:

```rust
#[cfg(target_os = "macos")]
pub(crate) mod macos_native;
```

- Create `src/adapters/window_managers/macos_native.rs` with the initial public surface:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceKind {
    Desktop,
    Fullscreen,
    SplitView,
    System,
    StageManagerOpaque,
}

impl SpaceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Fullscreen => "fullscreen",
            Self::SplitView => "split_view",
            Self::System => "system",
            Self::StageManagerOpaque => "stage_manager_opaque",
        }
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test --target-dir target -q macos_native_space_kind_symbols_are_exposed`

Expected: PASS.

**Step 5: Commit**

```bash
git add Cargo.toml src/adapters/window_managers/mod.rs src/adapters/window_managers/macos_native.rs
git commit -m "feat: wire macos native wm module" -m "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

### Task 2: Add safe space and window models plus classification helpers

**Files:**
- Modify: `src/adapters/window_managers/macos_native.rs`
- Test: `src/adapters/window_managers/macos_native.rs`

**Step 1: Write the failing test**

In `src/adapters/window_managers/macos_native.rs`, add pure-Rust classification tests:

```rust
#[test]
fn classify_space_distinguishes_desktop_fullscreen_split_and_stage_manager() {
    assert_eq!(classify_space(&raw_desktop_space(1)), SpaceKind::Desktop);
    assert_eq!(classify_space(&raw_fullscreen_space(2)), SpaceKind::Fullscreen);
    assert_eq!(classify_space(&raw_split_space(3, &[11, 12])), SpaceKind::SplitView);
    assert_eq!(
        classify_space(&raw_stage_manager_space(4)),
        SpaceKind::StageManagerOpaque
    );
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --target-dir target -q classify_space_distinguishes_desktop_fullscreen_split_and_stage_manager`

Expected: FAIL because the raw metadata model and classifier do not exist yet.

**Step 3: Write minimal implementation**

Add small, testable model types in `src/adapters/window_managers/macos_native.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
struct RawSpaceRecord {
    managed_space_id: u64,
    space_type: i32,
    tile_spaces: Vec<u64>,
    has_tile_layout_manager: bool,
    stage_manager_managed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceSnapshot {
    pub id: u64,
    pub kind: SpaceKind,
    pub is_active: bool,
    pub ordered_window_ids: Option<Vec<u64>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowSnapshot {
    pub id: u64,
    pub pid: Option<u32>,
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub space_id: u64,
    pub order_index: Option<usize>,
}
```

- Implement `classify_space(&RawSpaceRecord) -> SpaceKind`.
- Treat fullscreen and split metadata as first-class Spaces.
- Treat Stage Manager payloads as `StageManagerOpaque`.

**Step 4: Run test to verify it passes**

Run: `cargo test --target-dir target -q classify_space_distinguishes_desktop_fullscreen_split_and_stage_manager`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/adapters/window_managers/macos_native.rs
git commit -m "feat: add macos native space classification" -m "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

### Task 3: Implement best-effort active-Space ordering

**Files:**
- Modify: `src/adapters/window_managers/macos_native.rs`
- Test: `src/adapters/window_managers/macos_native.rs`

**Step 1: Write the failing test**

Add ordering tests that lock the approved semantics:

```rust
#[test]
fn active_space_ordering_prefers_frontmost_visible_windows() {
    let windows = vec![
        raw_window(11).with_level(10).with_visible_index(1),
        raw_window(12).with_level(20).with_visible_index(0),
    ];

    let ordered = order_active_space_windows(&windows);
    assert_eq!(ordered.iter().map(|w| w.id).collect::<Vec<_>>(), vec![12, 11]);
}

#[test]
fn non_active_space_windows_remain_unordered() {
    let snapshots = snapshots_for_inactive_space(99, &[21, 22]);
    assert!(snapshots.iter().all(|window| window.order_index.is_none()));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --target-dir target -q active_space_ordering_prefers_frontmost_visible_windows`

Expected: FAIL because there is no ordering helper or inactive-space policy yet.

**Step 3: Write minimal implementation**

- Add a helper that sorts only active-space windows using the best available signal order.
- Populate `WindowSnapshot.order_index` only for the active Space.
- Keep inactive-space windows as membership-only snapshots.

**Step 4: Run test to verify it passes**

Run:
- `cargo test --target-dir target -q active_space_ordering_prefers_frontmost_visible_windows`
- `cargo test --target-dir target -q non_active_space_windows_remain_unordered`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/adapters/window_managers/macos_native.rs
git commit -m "feat: add macos native window ordering policy" -m "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

### Task 4: Add fail-fast connection and permission checks

**Files:**
- Modify: `src/adapters/window_managers/macos_native.rs`
- Test: `src/adapters/window_managers/macos_native.rs`

**Step 1: Write the failing test**

Add connection tests behind a fake API shim:

```rust
#[test]
fn connect_with_api_rejects_missing_required_symbol() {
    let api = FakeNativeApi::default().without_symbol("SLSCopyManagedDisplaySpaces");
    let err = MacosNativeContext::connect_with_api(api).unwrap_err();
    assert!(err.to_string().contains("SLSCopyManagedDisplaySpaces"));
}

#[test]
fn connect_with_api_rejects_missing_accessibility_permission() {
    let api = FakeNativeApi::default().with_ax_trusted(false);
    let err = MacosNativeContext::connect_with_api(api).unwrap_err();
    assert!(err.to_string().contains("Accessibility"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --target-dir target -q connect_with_api_rejects_missing_required_symbol`

Expected: FAIL because `MacosNativeContext` and the injected API seam do not exist yet.

**Step 3: Write minimal implementation**

- Add `MacosNativeContext` plus a test seam such as `connect_with_api(api)`.
- Centralize the required symbol list.
- Check AX trust and any topology precondition that must exist for the module to be safe to use.
- Return explicit errors instead of silently degrading.

**Step 4: Run test to verify it passes**

Run:
- `cargo test --target-dir target -q connect_with_api_rejects_missing_required_symbol`
- `cargo test --target-dir target -q connect_with_api_rejects_missing_accessibility_permission`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/adapters/window_managers/macos_native.rs
git commit -m "feat: add macos native fail-fast connect checks" -m "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

### Task 5: Implement topology probing helpers

**Files:**
- Modify: `src/adapters/window_managers/macos_native.rs`
- Test: `src/adapters/window_managers/macos_native.rs`

**Step 1: Write the failing test**

Add probe tests using fake raw topology payloads:

```rust
#[test]
fn spaces_snapshot_includes_active_flags_and_classified_kinds() {
    let ctx = fake_context_with_spaces();
    let spaces = ctx.spaces().unwrap();

    assert!(spaces.iter().any(|space| space.kind == SpaceKind::Desktop && space.is_active));
    assert!(spaces.iter().any(|space| space.kind == SpaceKind::SplitView));
}

#[test]
fn focused_window_comes_from_active_space_snapshot() {
    let ctx = fake_context_with_active_window(42);
    let focused = ctx.focused_window().unwrap();
    assert_eq!(focused.id, 42);
    assert_eq!(focused.space_id, 1);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --target-dir target -q spaces_snapshot_includes_active_flags_and_classified_kinds`

Expected: FAIL because the probe pipeline does not produce snapshots yet.

**Step 3: Write minimal implementation**

- Add safe wrappers for the private topology queries.
- Convert raw results into `SpaceSnapshot` / `WindowSnapshot`.
- Enrich active-space windows with titles / app identity only where the approved semantics require it.
- Keep the parsing boundary separate from raw FFI calls so the tests stay pure.

**Step 4: Run test to verify it passes**

Run:
- `cargo test --target-dir target -q spaces_snapshot_includes_active_flags_and_classified_kinds`
- `cargo test --target-dir target -q focused_window_comes_from_active_space_snapshot`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/adapters/window_managers/macos_native.rs
git commit -m "feat: add macos native topology probes" -m "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

### Task 6: Implement focus, switch, and move operations

**Files:**
- Modify: `src/adapters/window_managers/macos_native.rs`
- Test: `src/adapters/window_managers/macos_native.rs`

**Step 1: Write the failing test**

Add operation tests around the explicit action ordering:

```rust
#[test]
fn focus_window_switches_to_target_space_before_fronting_window() {
    let mut ctx = fake_context_for_focus(77, 9);
    ctx.focus_window(77).unwrap();
    assert_eq!(ctx.take_calls(), vec!["switch_space:9", "focus_window:77"]);
}

#[test]
fn move_window_to_space_uses_space_move_primitive() {
    let mut ctx = fake_context_for_move();
    ctx.move_window_to_space(51, 12).unwrap();
    assert_eq!(ctx.take_calls(), vec!["move_window_to_space:51:12"]);
}

#[test]
fn stage_manager_targets_are_rejected_explicitly() {
    let mut ctx = fake_context_for_stage_manager_target();
    let err = ctx.focus_window(88).unwrap_err();
    assert!(err.to_string().contains("Stage Manager"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --target-dir target -q focus_window_switches_to_target_space_before_fronting_window`

Expected: FAIL because the operation helpers do not exist yet.

**Step 3: Write minimal implementation**

- Implement `switch_space`, `focus_window`, and `move_window_to_space`.
- Ensure cross-space focus switches first, then fronts the target window.
- Reject Stage Manager strip/topology requests explicitly instead of inventing behavior.

**Step 4: Run test to verify it passes**

Run:
- `cargo test --target-dir target -q focus_window_switches_to_target_space_before_fronting_window`
- `cargo test --target-dir target -q move_window_to_space_uses_space_move_primitive`
- `cargo test --target-dir target -q stage_manager_targets_are_rejected_explicitly`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/adapters/window_managers/macos_native.rs
git commit -m "feat: add macos native space operations" -m "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

### Task 7: Verify the module and record the known baseline

**Files:**
- Modify: `AGENTS.md`
- Test: `src/adapters/window_managers/macos_native.rs`

**Step 1: Write the failing test**

Add one focused regression test that exercises the end-to-end happy path via the fake API:

```rust
#[test]
fn context_happy_path_returns_active_space_and_focuses_window() {
    let mut ctx = fake_context_with_active_window(100);
    assert_eq!(ctx.focused_window().unwrap().id, 100);
    ctx.focus_window(100).unwrap();
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --target-dir target -q context_happy_path_returns_active_space_and_focuses_window`

Expected: FAIL until the probe and operation layers work together cleanly.

**Step 3: Write minimal implementation**

- Finish any glue needed so the regression test passes.
- Add an `AGENTS.md` note describing the surprising worktree behavior here: the original research stub was untracked in the main tree, so isolated worktrees start without `src/adapters/window_managers/macos_native.rs` and must create it from scratch.

**Step 4: Run tests to verify they pass**

Run:
- `cargo test --target-dir target -q macos_native`
- `cargo build --target-dir target -q`
- `cargo test --target-dir target -q`

Expected:
- `macos_native` targeted tests PASS.
- Build PASS.
- Full suite still reports only the same 13 baseline failures listed above and no new `macos_native` failures.

**Step 5: Commit**

```bash
git add AGENTS.md src/adapters/window_managers/macos_native.rs
git commit -m "feat: finalize macos native wm support" -m "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```
