# Engine Simplification Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Reduce the cognitive load of `src/engine/` by replacing repeated action preambles and fragmented move/merge/tear-out helpers with a shared focused-app session and clearer engine-side orchestration helpers, while preserving behavior.

**Architecture:** Keep adapter contracts mostly stable and simplify the engine around one explicit app-first processing flow. Capture focused window metadata and adapter chain resolution once, then route focus/resize/move through small engine-only helpers that keep merge, probe, and tear-out details local to the action layer.

**Tech Stack:** Rust, Cargo unit tests, existing engine action/orchestrator tests

---

### Task 1: Shared focused-app session and app-first executor

**Files:**
- Modify: `src/engine/actions/context.rs`
- Modify: `src/engine/actions/focus.rs`
- Modify: `src/engine/actions/resize.rs`
- Modify: `src/engine/actions/orchestrator.rs`
- Test: `src/engine/actions/context.rs`
- Test: `src/engine/actions/orchestrator.rs`

**Step 1: Write the failing tests**

Add tests that describe the intended helper behavior before implementation:

```rust
#[test]
fn with_focused_app_session_returns_none_when_focused_window_has_no_pid() {
    // fake wm focused window with pid=None
    // expect helper to return Ok(None)
}

#[test]
fn execute_app_then_wm_fallback_skips_wm_when_app_handler_returns_true() {
    // track app handler + wm fallback calls
    // expect wm fallback not to run
}

#[test]
fn execute_app_then_wm_fallback_runs_wm_when_app_handler_returns_false() {
    // track app handler + wm fallback calls
    // expect wm fallback to run once
}
```

**Step 2: Run tests to verify they fail**

Run:

```bash
cd /Users/m/Projects/yeetnyoink/.worktrees/copilot-engine-simplify-20260315-060633 && cargo test --quiet with_focused_app_session_returns_none_when_focused_window_has_no_pid execute_app_then_wm_fallback_skips_wm_when_app_handler_returns_true execute_app_then_wm_fallback_runs_wm_when_app_handler_returns_false
```

Expected: FAIL because the shared focused-app helper / generic fallback helper does not exist yet.

**Step 3: Write minimal implementation**

Implement the focused-app session in `context.rs` and refactor orchestrator focus/resize flow around it:

```rust
pub(crate) struct FocusedAppSession {
    source_window_id: u64,
    source_tile_index: usize,
    pid: ProcessId,
    app_id: String,
    title: String,
    chain: Vec<Box<dyn AppAdapter>>,
}

pub(crate) fn with_focused_app_session<T>(
    wm: &mut ConfiguredWindowManager,
    f: impl FnOnce(FocusedAppSession) -> Result<T>,
) -> Result<Option<T>> {
    let focused = wm.focused_window()?;
    let Some(pid) = focused.pid else {
        return Ok(None);
    };
    let app_id = focused.app_id.unwrap_or_default();
    let title = focused.title.unwrap_or_default();
    let chain = crate::engine::resolution::resolve_app_chain(&app_id, pid.get(), &title);
    Ok(Some(f(FocusedAppSession { /* ... */ })?))
}

fn execute_app_then_wm_fallback<A, W>(
    &mut self,
    wm: &mut ConfiguredWindowManager,
    dir: Direction,
    app_handler: A,
    wm_fallback: W,
) -> Result<()>
where
    A: FnOnce(&mut ConfiguredWindowManager, Direction) -> Result<bool>,
    W: FnOnce(&mut ConfiguredWindowManager, Direction) -> Result<()>,
{
    if app_handler(wm, dir)? {
        Ok(())
    } else {
        wm_fallback(wm, dir)
    }
}
```

Then rewrite `focus.rs` and `resize.rs` to use the shared session rather than repeating the same capture/chain-resolution preamble.

**Step 4: Run tests to verify they pass**

Run:

```bash
cd /Users/m/Projects/yeetnyoink/.worktrees/copilot-engine-simplify-20260315-060633 && cargo test --quiet with_focused_app_session_returns_none_when_focused_window_has_no_pid execute_app_then_wm_fallback_skips_wm_when_app_handler_returns_true execute_app_then_wm_fallback_runs_wm_when_app_handler_returns_false
```

Expected: PASS.

**Step 5: Commit**

```bash
cd /Users/m/Projects/yeetnyoink/.worktrees/copilot-engine-simplify-20260315-060633 && git add src/engine/actions/context.rs src/engine/actions/focus.rs src/engine/actions/resize.rs src/engine/actions/orchestrator.rs && git commit -m "refactor(engine): share focused app action session"
```

### Task 2: Consolidate directional probe semantics

**Files:**
- Modify: `src/engine/actions/probe.rs`
- Modify: `src/engine/actions/merge.rs`
- Modify: `src/engine/actions/orchestrator.rs`
- Modify: `src/engine/actions/movement.rs`
- Test: `src/engine/actions/probe.rs`

**Step 1: Write the failing tests**

Add tests that pin the probe semantics you want to preserve while simplifying the helper surface:

```rust
#[test]
fn directional_probe_restore_source_returns_focus_to_source_window() {
    // focus moves to target during probe, then source focus is restored
}

#[test]
fn directional_probe_keep_target_leaves_focus_on_target_window() {
    // focus stays on target when keep-target mode is requested
}
```

**Step 2: Run tests to verify they fail**

Run:

```bash
cd /Users/m/Projects/yeetnyoink/.worktrees/copilot-engine-simplify-20260315-060633 && cargo test --quiet directional_probe_restore_source_returns_focus_to_source_window directional_probe_keep_target_leaves_focus_on_target_window
```

Expected: FAIL until the probe helper is reshaped around the new clearer API.

**Step 3: Write minimal implementation**

Refactor probe code so that one helper owns directional focus mutation/restoration and higher-level callers build on it:

```rust
pub(crate) struct DirectionalWindowProbe<'a> {
    wm: &'a mut ConfiguredWindowManager,
    source_window_id: u64,
}

impl<'a> DirectionalWindowProbe<'a> {
    pub(crate) fn window(
        &mut self,
        dir: Direction,
        focus_mode: DirectionalProbeFocusMode,
    ) -> Result<Option<WindowRecord>> { /* existing probe logic */ }

    pub(crate) fn window_matching_adapter(
        &mut self,
        dir: Direction,
        adapter_name: &str,
        focus_mode: DirectionalProbeFocusMode,
    ) -> Result<Option<WindowRecord>> { /* probe + adapter filter */ }
}
```

Update merge, movement, and orchestrator callers to use the clearer probe entry points.

**Step 4: Run tests to verify they pass**

Run:

```bash
cd /Users/m/Projects/yeetnyoink/.worktrees/copilot-engine-simplify-20260315-060633 && cargo test --quiet directional_probe_restore_source_returns_focus_to_source_window directional_probe_keep_target_leaves_focus_on_target_window
```

Expected: PASS.

**Step 5: Commit**

```bash
cd /Users/m/Projects/yeetnyoink/.worktrees/copilot-engine-simplify-20260315-060633 && git add src/engine/actions/probe.rs src/engine/actions/merge.rs src/engine/actions/orchestrator.rs src/engine/actions/movement.rs && git commit -m "refactor(engine): clarify directional probe helpers"
```

### Task 3: Make move / merge / tear-out read as linear action flow

**Files:**
- Modify: `src/engine/actions/movement.rs`
- Modify: `src/engine/actions/merge.rs`
- Modify: `src/engine/actions/tearout.rs`
- Modify: `src/engine/actions/mod.rs`
- Test: `src/engine/actions/orchestrator.rs`
- Test: `src/engine/actions/tearout.rs`

**Step 1: Write the failing tests**

Add tests that protect the main routing branches while you refactor the action layer:

```rust
#[test]
fn passthrough_move_prefers_merge_before_tear_out_or_wm_fallback() {
    // matching adapter neighbor exists, merge path should win
}

#[test]
fn tear_out_wait_and_focus_returns_new_window_when_it_appears_late() {
    // delayed wm snapshot should still resolve target window
}
```

**Step 2: Run tests to verify they fail**

Run:

```bash
cd /Users/m/Projects/yeetnyoink/.worktrees/copilot-engine-simplify-20260315-060633 && cargo test --quiet passthrough_move_prefers_merge_before_tear_out_or_wm_fallback tear_out_wait_and_focus_returns_new_window_when_it_appears_late
```

Expected: FAIL before the new action contexts/helpers exist.

**Step 3: Write minimal implementation**

Group the current loose arguments into action-local helpers:

```rust
struct MoveExecution<'a> {
    wm: &'a mut ConfiguredWindowManager,
    session: &'a FocusedAppSession,
    dir: Direction,
}

impl MoveExecution<'_> {
    fn run(self) -> Result<bool> {
        for (index, app) in self.session.chain().iter().enumerate() {
            if self.handle_app_decision(index, app.as_ref())? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

struct PassthroughMergeContext<'a> { /* app + session + outer_chain + dir */ }
struct TearOutRequest<'a> { /* app + session + dir + decision_label */ }
```

The implementation goal is that `movement.rs` reads as a small policy loop and `merge.rs` / `tearout.rs` contain the detailed mechanics.

**Step 4: Run tests to verify they pass**

Run:

```bash
cd /Users/m/Projects/yeetnyoink/.worktrees/copilot-engine-simplify-20260315-060633 && cargo test --quiet passthrough_move_prefers_merge_before_tear_out_or_wm_fallback tear_out_wait_and_focus_returns_new_window_when_it_appears_late
```

Expected: PASS.

**Step 5: Commit**

```bash
cd /Users/m/Projects/yeetnyoink/.worktrees/copilot-engine-simplify-20260315-060633 && git add src/engine/actions/movement.rs src/engine/actions/merge.rs src/engine/actions/tearout.rs src/engine/actions/mod.rs && git commit -m "refactor(engine): linearize move merge and tear-out flow"
```

### Task 4: Canonical engine imports and full verification

**Files:**
- Modify: `src/engine/**/*.rs` where internal engine imports still go through compatibility shims unnecessarily
- Test: `src/engine/actions/orchestrator.rs`
- Test: `src/engine/actions/probe.rs`
- Test: `src/engine/actions/tearout.rs`

**Step 1: Write the failing test**

If needed, add or update a structural assertion that internal engine code uses canonical imports in the touched action files, or skip a new structural test if the behavior tests already pin the refactor sufficiently.

Representative test:

```rust
#[test]
fn engine_action_modules_use_canonical_engine_imports() {
    let source = include_str!("movement.rs");
    assert!(!source.contains("crate::engine::contracts::"));
}
```

**Step 2: Run test to verify it fails**

Run:

```bash
cd /Users/m/Projects/yeetnyoink/.worktrees/copilot-engine-simplify-20260315-060633 && cargo test --quiet engine_action_modules_use_canonical_engine_imports
```

Expected: FAIL if you add the structural test.

**Step 3: Write minimal implementation**

Normalize touched engine files to use canonical engine layer imports where practical, then run the targeted action tests and the full suite:

```bash
cd /Users/m/Projects/yeetnyoink/.worktrees/copilot-engine-simplify-20260315-060633 && cargo test --quiet passthrough_move_prefers_merge_before_tear_out_or_wm_fallback tear_out_wait_and_focus_returns_new_window_when_it_appears_late directional_probe_restore_source_returns_focus_to_source_window directional_probe_keep_target_leaves_focus_on_target_window
```

Then:

```bash
cd /Users/m/Projects/yeetnyoink/.worktrees/copilot-engine-simplify-20260315-060633 && cargo test -q
```

Expected: PASS.

**Step 4: Commit**

```bash
cd /Users/m/Projects/yeetnyoink/.worktrees/copilot-engine-simplify-20260315-060633 && git add src/engine && git commit -m "refactor(engine): simplify action orchestration structure"
```
