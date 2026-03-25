# Floating Focus Strategy Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add named floating-window focus strategies, require `floating_focus_strategy` for `wm.macos_native`, and route macOS-native directional in-space focus through the configured strategy without changing move behavior.

**Architecture:** Keep floating-focus geometry policy centralized in `src/engine/topology.rs` behind a new `FloatingFocusStrategy` enum and strategy-aware selector. Parse the strategy per WM backend in `src/config.rs`, require it for `macos_native`, and thread only the selected backend's strategy into `src/adapters/window_managers/macos_native.rs`, while preserving existing display scoping, overlay filtering, and adjacent-space fallback behavior.

**Tech Stack:** Rust, serde, clap `ValueEnum`, repo config helpers in `src/config.rs`, macOS-native adapter tests, Cargo, Nix/flake config module.

---

### Task 1: Add the shared floating-focus strategy enum and selector

**Files:**
- Modify: `src/engine/topology.rs:23-46`
- Modify: `src/engine/topology.rs:209-285`
- Test: `src/engine/topology.rs:352-535`

**Step 1: Write the failing engine tests**

Add table-driven unit tests that use the same source window and candidate layout but assert different winners for different strategies.

```rust
#[test]
fn select_closest_in_direction_with_strategy_distinguishes_radial_and_cross_edge() {
    let rects = vec![
        DirectedRect { id: 1_u64, rect: Rect { x: 200, y: 100, w: 100, h: 100 } },
        DirectedRect { id: 2_u64, rect: Rect { x: 40, y: 80, w: 60, h: 60 } },
        DirectedRect { id: 3_u64, rect: Rect { x: 90, y: 150, w: 130, h: 130 } },
    ];

    assert_eq!(
        select_closest_in_direction_with_strategy(
            &rects,
            1,
            Direction::West,
            FloatingFocusStrategy::RadialCenter,
        ),
        Some(2),
    );
    assert_eq!(
        select_closest_in_direction_with_strategy(
            &rects,
            1,
            Direction::West,
            FloatingFocusStrategy::CrossEdgeGap,
        ),
        Some(3),
    );
}
```

Also add focused tests for:

- `TrailingEdgeParallel`
- `LeadingEdgeParallel`
- `OverlapThenGap`
- `RayAngle`
- shared tie-breaking when scores are equal

**Step 2: Run the new engine tests to verify they fail**

Run:

```bash
cargo test --lib select_closest_in_direction_with_strategy_ -- --nocapture
```

Expected: FAIL with missing enum/function or wrong winner assertions.

**Step 3: Write the minimal shared implementation**

Add the new enum near `Direction`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ValueEnum, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FloatingFocusStrategy {
    RadialCenter,
    TrailingEdgeParallel,
    LeadingEdgeParallel,
    CrossEdgeGap,
    OverlapThenGap,
    RayAngle,
}
```

Add a strategy-aware selector:

```rust
pub fn select_closest_in_direction_with_strategy<T>(
    rects: &[DirectedRect<T>],
    source_id: T,
    dir: Direction,
    strategy: FloatingFocusStrategy,
) -> Option<T>
where
    T: Copy + Eq,
{
    // 1. Find source rect
    // 2. Filter candidates into requested half-plane
    // 3. Score candidates with strategy-specific metric
    // 4. Break ties by perpendicular offset, then stable ordering
}
```

Keep `select_closest_in_direction(...)` as a wrapper around the current behavior:

```rust
pub fn select_closest_in_direction<T>(...) -> Option<T> {
    select_closest_in_direction_with_strategy(
        rects,
        source_id,
        dir,
        FloatingFocusStrategy::OverlapThenGap,
    )
}
```

Add small private helpers instead of duplicating math:

- `center_point`
- `directional_half_plane`
- `directional_gap`
- `edge_for(dir, rect, leading_or_trailing)`
- `angular_deviation_from_ray`

Do **not** add move logic here.

**Step 4: Run the engine tests to verify they pass**

Run:

```bash
cargo test --lib select_closest_in_direction_ -- --nocapture
```

Expected: PASS for old selector tests and new strategy-specific tests.

**Step 5: Commit**

```bash
git add src/engine/topology.rs
git commit -m "feat: add floating focus strategies"
```

### Task 2: Extend WM config to parse and validate floating focus strategy

**Files:**
- Modify: `src/config.rs:53-60`
- Modify: `src/config.rs:186-195`
- Modify: `src/config.rs:1251-1269`
- Test: `src/config.rs:2063-2179`

**Step 1: Write the failing config tests**

Add tests for all validation rules:

```rust
#[test]
fn wm_config_requires_macos_native_floating_focus_strategy() {
    let err = prepare_with_path(write_temp_config(r#"
[wm.macos_native]
enabled = true

[wm.macos_native.mission_control_keyboard_shortcuts.move_left_a_space]
keycode = "0x7B"
ctrl = true
option = false
command = false
shift = false
fn = true

[wm.macos_native.mission_control_keyboard_shortcuts.move_right_a_space]
keycode = "0x7C"
ctrl = true
option = false
command = false
shift = false
fn = true
"#)).unwrap_err();

    assert!(err.to_string().contains("floating_focus_strategy"));
}
```

Also add tests for:

- `wm.macos_native.floating_focus_strategy = "radial_center"` parses successfully
- unknown strategy names fail
- `[wm.niri] enabled = true` still parses without a strategy
- optional backends can set `floating_focus_strategy` if present
- getter returns the selected macOS-native strategy

**Step 2: Run the config tests to verify they fail**

Run:

```bash
cargo test --lib config::tests::wm_config_requires_macos_native_floating_focus_strategy
```

Expected: FAIL because the field is not yet modeled or enforced.

**Step 3: Write the minimal config implementation**

Import the shared enum from `src/engine/topology.rs`.

Update the shared backend config:

```rust
pub struct EnabledWmConfig {
    pub enabled: bool,
    pub floating_focus_strategy: Option<FloatingFocusStrategy>,
}
```

Update the macOS-native backend config:

```rust
pub struct MacosNativeWmConfig {
    pub enabled: bool,
    pub floating_focus_strategy: FloatingFocusStrategy,
    pub mission_control_keyboard_shortcuts: MissionControlKeyboardShortcutsConfig,
}
```

Extend validation:

```rust
impl MacosNativeWmConfig {
    fn validate(&self) -> Result<()> {
        self.mission_control_keyboard_shortcuts.validate()
    }
}
```

The type-level required field should make missing `floating_focus_strategy` fail at parse time; do not add a second silent default.

Expose a getter:

```rust
pub fn macos_native_floating_focus_strategy() -> Option<FloatingFocusStrategy> {
    read_config()
        .wm
        .macos_native
        .as_ref()
        .filter(|cfg| cfg.enabled)
        .map(|cfg| cfg.floating_focus_strategy)
}
```

Do **not** make this a global WM setting.

**Step 4: Run the config tests to verify they pass**

Run:

```bash
cargo test --lib config::tests
```

Expected: PASS, including the new parse/validation coverage.

**Step 5: Commit**

```bash
git add src/config.rs
git commit -m "feat: configure wm floating focus strategy"
```

### Task 3: Wire `macos_native` focus to the configured floating focus strategy

**Files:**
- Modify: `src/adapters/window_managers/macos_native.rs:438-471`
- Modify: `src/adapters/window_managers/macos_native.rs:3000-3078`
- Modify: `src/adapters/window_managers/macos_native.rs:3304-3306`
- Test: `src/adapters/window_managers/macos_native.rs:6289-7067`

**Step 1: Write the failing macOS-native adapter tests**

Add two end-to-end focus tests that use the same floating layout but different config snapshots:

```rust
#[test]
fn backend_focus_direction_uses_radial_center_strategy() {
    let _guard = install_config(r#"
[wm.macos_native]
enabled = true
floating_focus_strategy = "radial_center"

[wm.macos_native.mission_control_keyboard_shortcuts.move_left_a_space]
keycode = "0x7B"
ctrl = true
option = false
command = false
shift = false
fn = true

[wm.macos_native.mission_control_keyboard_shortcuts.move_right_a_space]
keycode = "0x7C"
ctrl = true
option = false
command = false
shift = false
fn = true
"#);

    // Same fixture as cross_edge test, but expect a different target window id.
}
```

Add at least:

- one test where `radial_center` and `cross_edge_gap` pick different targets
- one test proving overlays/non-normal-layer windows stay excluded regardless of strategy
- one test proving “no in-space candidate” still falls through to adjacent-space logic

**Step 2: Run the adapter tests to verify they fail**

Run:

```bash
cargo test --lib backend_focus_direction_uses_radial_center_strategy -- --nocapture
```

Expected: FAIL because the adapter still uses the implicit selector.

**Step 3: Write the minimal adapter implementation**

Resolve the strategy from config and pass it into the engine helper:

```rust
let strategy = config::macos_native_floating_focus_strategy()
    .expect("macos_native floating focus strategy should be validated at config load");

let Some(target_id) = select_closest_in_direction_with_strategy(
    &rects,
    focused.id,
    direction,
    strategy,
) else {
    // existing adjacent-space fallback
};
```

Keep these behaviors unchanged:

- `active_directed_rects_for_display(...)` still scopes to the focused display first
- `is_directional_focus_window(...)` still excludes non-normal-layer windows
- existing adjacent-space fallback remains intact

Do **not** add strategy branching inside `macos_native.rs` beyond choosing the enum value. The geometry logic belongs in `src/engine/topology.rs`.

**Step 4: Run the adapter tests to verify they pass**

Run:

```bash
cargo test --lib backend_focus_direction_ -- --nocapture
```

Expected: PASS for the new strategy-specific tests and all pre-existing focus-direction tests.

**Step 5: Commit**

```bash
git add src/adapters/window_managers/macos_native.rs
git commit -m "feat: apply floating focus strategy in macos native"
```

### Task 4: Update config surfaces, docs, and Nix module

**Files:**
- Modify: `config.example.toml:19-35`
- Modify: `README.md:74-77`
- Modify: `src/main.rs` command help block near the CLI config summary
- Modify: `flake.nix` WM module definitions for `macos_native` and shared backend tables
- Optional docs sync: `ARCHITECTURE.md` if it summarizes WM config requirements

**Step 1: Write the failing example/config verification**

Update or add checks that the example config still parses with the new required field.

If the repo already has the example parse test, make it fail first by updating the example fixture expectation:

```rust
#[test]
fn repo_config_example_toml_parses() {
    // Expect macos_native example to include floating_focus_strategy
}
```

**Step 2: Run the example parse check to verify it fails**

Run:

```bash
cargo test --lib repo_config_example_toml_parses
```

Expected: FAIL until the example and docs are updated.

**Step 3: Update the docs and module surface**

Add the new field to the commented macOS-native example:

```toml
# [wm.macos_native]
# enabled = true
# floating_focus_strategy = "overlap_then_gap"
```

Update README prose to explain:

- the field is required for `wm.macos_native`
- optional elsewhere
- current strategy names

Update `flake.nix` so the Home Manager / Nix module matches the runtime schema. Example shape:

```nix
floating_focus_strategy = lib.mkOption {
  type = lib.types.nullOr (lib.types.enum [
    "radial_center"
    "trailing_edge_parallel"
    "leading_edge_parallel"
    "cross_edge_gap"
    "overlap_then_gap"
    "ray_angle"
  ]);
  default = null;
};
```

Add the macOS-native requirement via module assertions or submodule typing; do not silently inject a default.

**Step 4: Run docs/module verification**

Run:

```bash
cargo test --lib repo_config_example_toml_parses
nix eval .#packages.aarch64-darwin.default.pname --raw --no-write-lock-file
```

Expected: both commands succeed.

**Step 5: Commit**

```bash
git add config.example.toml README.md src/main.rs flake.nix ARCHITECTURE.md
git commit -m "docs: document floating focus strategy"
```

### Task 5: Final verification and integration pass

**Files:**
- Verify: `src/engine/topology.rs`
- Verify: `src/config.rs`
- Verify: `src/adapters/window_managers/macos_native.rs`
- Verify: `config.example.toml`
- Verify: `README.md`
- Verify: `flake.nix`

**Step 1: Run the focused verification suite**

Run:

```bash
cargo test --lib select_closest_in_direction_
cargo test --lib config::tests
cargo test --lib repo_config_example_toml_parses
cargo test --lib backend_focus_direction_
cargo test --lib switch_adjacent_space_via_hotkey_
```

Expected: all commands pass.

**Step 2: Build the release binary used in real repros**

Run:

```bash
cargo build --release
```

Expected: release build succeeds.

**Step 3: Spot-check repo status**

Run:

```bash
git --no-pager status --short
git --no-pager diff -- src/engine/topology.rs src/config.rs src/adapters/window_managers/macos_native.rs config.example.toml README.md flake.nix
```

Expected: only intended files are changed.

**Step 4: Request code review**

Run the repo's review flow or use the configured code-review subagent before merging.

**Step 5: Commit the final verification-safe state**

```bash
git add src/engine/topology.rs src/config.rs src/adapters/window_managers/macos_native.rs config.example.toml README.md src/main.rs flake.nix ARCHITECTURE.md
git commit -m "feat: add wm floating focus strategies"
```
