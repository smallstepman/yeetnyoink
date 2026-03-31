# macOS Test Support Removal Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Delete `src/adapters/window_managers/macos_window_manager_test_support.rs` by moving backend-leaning tests next to `crates/macos_window_manager` and leaving only true outer-policy tests in `src/adapters/window_managers/macos_native.rs`.

**Architecture:** `crates/macos_window_manager` already owns the real backend contract (`RealNativeApi`, native snapshot helpers, space-switch helpers, raw topology parsing). The remaining `macos_window_manager_test_support.rs` file is a transitional facade that duplicates backend-facing helpers for adapter tests. The follow-up should migrate low-level/backend-leaning tests into crate-local module tests, then remove the adapter-side support file entirely so `macos_native.rs` keeps only outer-policy logic and adapter-facing regressions.

**Tech Stack:** Rust workspace, Cargo unit tests, crate-local `#[cfg(test)]` modules, source-shape assertions, `cargo build --release`

---

> **Execution note:** the worktree is already dirty (`AGENTS.md`, the session plan doc, and a formatting-only diff in `crates/macos_window_manager/src/tests.rs`). Use path-limited `git add` / `git commit` so this follow-up only stages the files listed below.

> **Important constraint:** do **not** move production backend code out of `macos_window_manager_test_support.rs` into the crate again. The crate already owns the real backend implementation. This follow-up is about rerouting tests off the duplicate facade and deleting the facade, not re-extracting production code.

### Task 1: Move Foundation and AX unit tests into the crate

**Files:**
- Modify: `src/adapters/window_managers/macos_native.rs`
- Modify: `crates/macos_window_manager/src/foundation.rs`
- Modify: `crates/macos_window_manager/src/ax.rs`
- Modify: `crates/macos_window_manager/src/tests.rs`

**Tests to move out of `macos_native.rs`:**
- `servo_cf_array_from_u64s_returns_numbers_in_order`
- `servo_cf_dictionary_accessors_read_string_and_i32_values`
- `switch_adjacent_space_via_hotkey_posts_configured_shortcut_for_east`
- `switch_adjacent_space_via_hotkey_rejects_vertical_directions`
- `focused_window_id_via_ax_queries_focused_app_then_window`

**Step 1: Write the failing test**

Copy one low-level test first into the crate module it actually exercises. Start with:

```rust
#[test]
fn servo_cf_array_from_u64s_returns_numbers_in_order() {
    let array = array_from_u64s(&[11, 22])
        .expect("servo-backed helper should build a CFArray of numbers");

    let values = array_iter(array.as_concrete_TypeRef())
        .map(|value| number_to_i64(value).expect("each value should decode to i64"))
        .collect::<Vec<_>>();

    assert_eq!(values, vec![11, 22]);
}
```

Put it in `crates/macos_window_manager/src/foundation.rs` under the existing `#[cfg(test)]` section (or create that section if it does not yet exist).

**Step 2: Run test to verify it fails**

Run: `cargo test -p macos_window_manager servo_cf_array_from_u64s_returns_numbers_in_order -- --nocapture`

Expected: FAIL because the crate test module does not yet have the exact helper imports / fixture setup.

**Step 3: Write minimal implementation**

- Move the five low-level Foundation / AX tests listed above out of `macos_native.rs`.
- Put Foundation-specific tests in `crates/macos_window_manager/src/foundation.rs`.
- Put the AX-specific test in `crates/macos_window_manager/src/ax.rs` (or `src/tests.rs` only if it truly must span multiple backend modules).
- If multiple moved tests need a tiny shared helper, add it to `crates/macos_window_manager/src/tests.rs` instead of recreating another adapter-side facade.
- Delete the moved test bodies from `macos_native.rs`.

**Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p macos_window_manager servo_cf_array_from_u64s_returns_numbers_in_order -- --nocapture
cargo test -p macos_window_manager focused_window_id_via_ax_queries_focused_app_then_window -- --nocapture
cargo test servo_cf_array_from_u64s_returns_numbers_in_order --lib -- --nocapture
```

Expected:
- the two crate-local tests PASS
- the adapter-side filter shows `running 0 tests` for `servo_cf_array_from_u64s_returns_numbers_in_order`

**Step 5: Commit**

```bash
git add src/adapters/window_managers/macos_native.rs \
  crates/macos_window_manager/src/foundation.rs \
  crates/macos_window_manager/src/ax.rs \
  crates/macos_window_manager/src/tests.rs
git commit -m "test: move macos foundation and ax tests into crate"
```

### Task 2: Move raw topology, parser, and window-description tests into the crate

**Files:**
- Modify: `src/adapters/window_managers/macos_native.rs`
- Modify: `crates/macos_window_manager/src/desktop_topology_snapshot.rs`
- Modify: `crates/macos_window_manager/src/skylight.rs`
- Modify: `crates/macos_window_manager/src/window_server.rs`
- Modify: `crates/macos_window_manager/src/tests.rs`

**Tests to move out of `macos_native.rs`:**
- `classify_space_distinguishes_desktop_fullscreen_split_and_stage_manager`
- `real_path_app_id_ignores_owner_name_display_label`
- `parse_lsappinfo_bundle_identifier_extracts_stable_app_id`
- `active_space_ordering_prefers_frontmost_visible_windows`
- `active_space_ordering_uses_window_level_when_visible_order_is_missing`
- `active_space_ordering_prefers_visible_windows_over_fallback_ordering`
- `spaces_snapshot_includes_active_flags_and_classified_kinds`
- `spaces_snapshot_marks_all_active_display_spaces_active`
- `matching_onscreen_window_descriptions_preserve_target_window_metadata`
- `parse_raw_space_record_ignores_non_dictionary_tile_space_entries`
- `parse_managed_spaces_preserves_display_grouping`

**Step 1: Write the failing test**

Move one raw-topology test first into the crate module it targets. Start with:

```rust
#[test]
fn classify_space_distinguishes_desktop_fullscreen_split_and_stage_manager() {
    assert_eq!(classify_space(DESKTOP_SPACE_TYPE, false), SpaceKind::Desktop);
    assert_eq!(classify_space(FULLSCREEN_SPACE_TYPE, false), SpaceKind::Fullscreen);
    assert_eq!(classify_space(FULLSCREEN_SPACE_TYPE, true), SpaceKind::SplitView);
}
```

Put it in `crates/macos_window_manager/src/desktop_topology_snapshot.rs`.

**Step 2: Run test to verify it fails**

Run: `cargo test -p macos_window_manager classify_space_distinguishes_desktop_fullscreen_split_and_stage_manager -- --nocapture`

Expected: FAIL until the crate module has the right test imports and the moved test no longer depends on adapter-side scaffolding.

**Step 3: Write minimal implementation**

- Move the topology and parser tests listed above into the crate modules they exercise:
  - `desktop_topology_snapshot.rs`
  - `skylight.rs`
  - `window_server.rs`
- Keep only cross-module backend helper code in `crates/macos_window_manager/src/tests.rs`.
- Delete the moved test bodies from `macos_native.rs`.
- Do **not** add new adapter-side test support to replace them.

**Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p macos_window_manager classify_space_distinguishes_desktop_fullscreen_split_and_stage_manager -- --nocapture
cargo test -p macos_window_manager parse_managed_spaces_preserves_display_grouping -- --nocapture
cargo test -p macos_window_manager matching_onscreen_window_descriptions_preserve_target_window_metadata -- --nocapture
cargo test parse_managed_spaces_preserves_display_grouping --lib -- --nocapture
```

Expected:
- the three crate-local tests PASS
- the adapter-side filter for `parse_managed_spaces_preserves_display_grouping` shows `running 0 tests`

**Step 5: Commit**

```bash
git add src/adapters/window_managers/macos_native.rs \
  crates/macos_window_manager/src/desktop_topology_snapshot.rs \
  crates/macos_window_manager/src/skylight.rs \
  crates/macos_window_manager/src/window_server.rs \
  crates/macos_window_manager/src/tests.rs
git commit -m "test: move macos parser and topology tests into crate"
```

### Task 3: Remove all remaining adapter-test imports of `macos_window_manager_test_support`

**Files:**
- Modify: `src/adapters/window_managers/macos_native.rs`
- Modify: `crates/macos_window_manager/src/tests.rs`

**Step 1: Write the failing test**

Add a source-shape test in `src/adapters/window_managers/macos_native.rs` asserting the adapter test module no longer imports from the external support file:

```rust
#[test]
fn source_adapter_tests_do_not_import_macos_test_support() {
    let source = include_str!("macos_native.rs");
    assert!(!source.contains("use super::macos_window_manager_test_support::"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test source_adapter_tests_do_not_import_macos_test_support --lib -- --nocapture`

Expected: FAIL because the current test module still imports Foundation/window_server helpers and shared reexports from `macos_window_manager_test_support`.

**Step 3: Write minimal implementation**

- Rewrite the remaining `macos_native.rs` tests so they use only:
  - `super::*`
  - local outer-only helpers already in the adapter test module
  - public DTOs / traits imported from `macos_window_manager`
- If a remaining test is still backend-leaning, move it into `crates/macos_window_manager/src/tests.rs` instead of recreating adapter-side support.
- By the end of this task, `macos_native.rs` should not contain any `use super::macos_window_manager_test_support::...` lines.

**Step 4: Run test to verify it passes**

Run:

```bash
cargo test source_adapter_tests_do_not_import_macos_test_support --lib -- --nocapture
cargo test focus_direction_escapes_overlay_only_space_into_adjacent_real_space --lib -- --nocapture
cargo test focus_direction_uses_outer_policy_with_native_snapshot --lib -- --nocapture
```

Expected: all three PASS

**Step 5: Commit**

```bash
git add src/adapters/window_managers/macos_native.rs \
  crates/macos_window_manager/src/tests.rs
git commit -m "refactor: detach macos adapter tests from support facade"
```

### Task 4: Delete `macos_window_manager_test_support.rs`

**Files:**
- Delete: `src/adapters/window_managers/macos_window_manager_test_support.rs`
- Modify: `src/adapters/window_managers/macos_native.rs`

**Step 1: Write the failing test**

Add the final structural guard in `macos_native.rs`:

```rust
#[test]
fn source_adapter_has_no_macos_test_support_module() {
    let implementation = implementation_source();
    assert!(!implementation.contains("mod macos_window_manager_test_support;"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test source_adapter_has_no_macos_test_support_module --lib -- --nocapture`

Expected: FAIL because `macos_native.rs` still declares the sibling module.

**Step 3: Write minimal implementation**

- Remove:
  - `#[cfg(test)]`
  - `#[path = "macos_window_manager_test_support.rs"]`
  - `mod macos_window_manager_test_support;`
- Delete `src/adapters/window_managers/macos_window_manager_test_support.rs`.
- Keep only outer-policy tests and source-shape tests in `macos_native.rs`.

**Step 4: Run test to verify it passes**

Run:

```bash
cargo test source_adapter_has_no_macos_test_support_module --lib -- --nocapture
cargo test macos_native --lib -- --nocapture
```

Expected:
- the structural guard PASS
- the full `macos_native` lib suite PASS without the support file

**Step 5: Commit**

```bash
git add src/adapters/window_managers/macos_native.rs
git rm src/adapters/window_managers/macos_window_manager_test_support.rs
git commit -m "refactor: remove macos adapter test support facade"
```

### Task 5: Full verification and cleanup

**Files:**
- Modify only if verification uncovers a real extraction/test-split defect
- Modify: `AGENTS.md` only if removing the support file exposes a durable new gotcha

**Step 1: Run verification to find failures**

Run:

```bash
cargo test -p macos_window_manager
cargo test macos_native --lib -- --nocapture
cargo build --release
```

Expected: at least one failure if any backend-leaning test/support still implicitly depends on the deleted facade.

**Step 2: Write minimal implementation**

- Fix only support-removal regressions.
- If the failure shows that a test is still backend-leaning, move it into the crate rather than recreating adapter-side support.
- Do not broaden into unrelated cleanup.

**Step 3: Run verification to verify it passes**

Run:

```bash
cargo test -p macos_window_manager
cargo test macos_native --lib -- --nocapture
cargo build --release
```

Expected:
- extracted crate tests PASS
- `macos_native` lib tests PASS
- release build succeeds

**Step 4: Commit**

```bash
git add crates/macos_window_manager/src \
  src/adapters/window_managers/macos_native.rs \
  AGENTS.md
git commit -m "refactor: finalize macos test support removal"
```
