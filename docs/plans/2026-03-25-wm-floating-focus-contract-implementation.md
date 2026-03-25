# WM Floating-Focus Contract Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Express `floating_focus_strategy` as a window-manager contract, enforce the single-`wm.<backend>` config rule, and make config validation require/allow/forbid the field based on adapter behavior in `yeetnyoink`.

**Architecture:** Add an engine-owned `FloatingFocusMode` contract next to existing WM capability declarations, expose it through `WindowManagerSpec`, and give every built-in adapter/spec an explicit mode. Then move `floating_focus_strategy` requirement logic out of serde type shape and into `WmConfig::validate()`, so config can enforce `FloatingOnly`, `TilingAndFloating`, and `TilingOnly` uniformly while keeping macOS-native shortcut parsing intact.

**Tech Stack:** Rust, serde, existing WM contract types in `src/engine/wm/`, config parsing in `src/config.rs`, built-in WM adapters under `src/adapters/window_managers/`, Cargo tests, CLI help tests, and the Nix Home Manager surface in `flake.nix`.

---

### Task 1: Add the engine-owned floating-focus contract and expose it through built-in specs

**Files:**
- Modify: `src/engine/wm/capabilities.rs:5-160`
- Modify: `src/engine/wm/configured.rs:182-253`
- Modify: `src/engine/wm/mod.rs:13-52`
- Modify: `src/engine/wm/mod.rs:180-210`
- Modify: `src/adapters/window_managers/niri.rs`
- Modify: `src/adapters/window_managers/i3.rs`
- Modify: `src/adapters/window_managers/hyprland.rs`
- Modify: `src/adapters/window_managers/paneru.rs`
- Modify: `src/adapters/window_managers/yabai.rs`
- Modify: `src/adapters/window_managers/macos_native.rs`
- Test: `src/engine/wm/mod.rs`
- Test: `src/adapters/window_managers/mod.rs`

**Step 1: Write the failing contract test**

Add a focused test in `src/engine/wm/mod.rs` that proves the spec boundary exposes the new contract and that the current built-in mapping is explicit:

```rust
#[test]
fn built_in_specs_expose_expected_floating_focus_modes() {
    use crate::engine::wm::FloatingFocusMode;

    assert_eq!(
        spec_for_backend(WmBackend::MacosNative).floating_focus_mode(),
        FloatingFocusMode::FloatingOnly,
    );
    assert_eq!(
        spec_for_backend(WmBackend::Niri).floating_focus_mode(),
        FloatingFocusMode::TilingOnly,
    );
    assert_eq!(
        spec_for_backend(WmBackend::Yabai).floating_focus_mode(),
        FloatingFocusMode::TilingOnly,
    );
}
```

Also extend the existing built-in spec contract test in `src/adapters/window_managers/mod.rs` so it asserts that adapter-declared modes and spec-exposed modes stay in sync.

**Step 2: Run the focused test to verify it fails**

Run:

```bash
cargo test --lib built_in_specs_expose_expected_floating_focus_modes -- --nocapture
```

Expected: FAIL to compile because `FloatingFocusMode` and/or `floating_focus_mode()` do not exist yet.

**Step 3: Write the minimal contract implementation**

Add the engine-owned enum in `src/engine/wm/capabilities.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatingFocusMode {
    FloatingOnly,
    TilingAndFloating,
    TilingOnly,
}
```

Extend the contract traits:

```rust
pub trait WindowManagerCapabilityDescriptor {
    const NAME: &'static str;
    const CAPABILITIES: WindowManagerCapabilities;
    const FLOATING_FOCUS_MODE: FloatingFocusMode;
}

pub trait WindowManagerSpec: Sync {
    fn backend(&self) -> WmBackend;
    fn name(&self) -> &'static str;
    fn connect(&self) -> Result<ConfiguredWindowManager>;
    fn floating_focus_mode(&self) -> FloatingFocusMode;
}
```

In `src/engine/wm/mod.rs`, add a tiny engine-owned helper so config code does not need to import the adapter module directly:

```rust
pub fn floating_focus_mode_for_backend(backend: WmBackend) -> FloatingFocusMode {
    spec_for_backend(backend).floating_focus_mode()
}
```

Then set `FLOATING_FOCUS_MODE` on every built-in adapter:

- `MacosNativeAdapter` → `FloatingOnly`
- `NiriAdapter` → `TilingOnly`
- `I3Adapter` → `TilingOnly`
- `HyprlandAdapter` → `TilingOnly`
- `PaneruAdapter` → `TilingOnly`
- `YabaiAdapter` → `TilingOnly`

Finally, update each concrete spec and `UnsupportedWindowManagerSpec` to return the correct mode so the contract remains visible even when a backend is unavailable on the current OS.

**Step 4: Run the contract tests to verify they pass**

Run:

```bash
cargo test --lib built_in_specs_expose_expected_floating_focus_modes -- --nocapture
cargo test --lib built_in_specs_match_window_manager_contract -- --nocapture
```

Expected: PASS.

**Step 5: Commit**

```bash
git add src/engine/wm/capabilities.rs src/engine/wm/configured.rs src/engine/wm/mod.rs \
  src/adapters/window_managers/niri.rs src/adapters/window_managers/i3.rs \
  src/adapters/window_managers/hyprland.rs src/adapters/window_managers/paneru.rs \
  src/adapters/window_managers/yabai.rs src/adapters/window_managers/macos_native.rs
git commit -m "feat: add WM floating focus contract"
```

### Task 2: Rework WM config validation to use the contract instead of hardcoded field shape

**Files:**
- Modify: `src/config.rs:95-203`
- Modify: `src/config.rs:1262-1272`
- Test: `src/config.rs:2085-2385`

**Step 1: Write the failing config tests**

Add focused tests in `src/config.rs` for the new rules:

```rust
#[test]
fn wm_config_rejects_multiple_wm_tables_even_if_only_one_is_enabled() {
    let err = prepare_with_path(write_temp_config(r#"
[wm.niri]
enabled = true

[wm.yabai]
enabled = false
"#)).unwrap_err();

    assert!(format!("{err:#}").contains("exactly one window manager table"));
}

#[test]
fn wm_config_rejects_floating_focus_strategy_for_tiling_only_backend() {
    let err = prepare_with_path(write_temp_config(r#"
[wm.niri]
enabled = true
floating_focus_strategy = "ray_angle"
"#)).unwrap_err();

    assert!(format!("{err:#}").contains("TilingOnly"));
}
```

Also add tests for:

- zero WM tables present,
- one WM table present with `enabled = false`,
- `wm.macos_native` missing `floating_focus_strategy`,
- `wm.macos_native` succeeding when the strategy is present,
- direct helper coverage for `FloatingFocusMode::TilingAndFloating` so the optional case is tested without inventing a fake backend.

**Step 2: Run the focused config test to verify it fails**

Run:

```bash
cargo test --lib wm_config_rejects_floating_focus_strategy_for_tiling_only_backend -- --nocapture
```

Expected: FAIL because config still allows the field on generic enabled backends.

**Step 3: Write the minimal validation implementation**

First, make `macos_native` participate in the same contract-driven shape by changing its field to optional:

```rust
pub struct MacosNativeWmConfig {
    pub enabled: bool,
    pub floating_focus_strategy: Option<FloatingFocusStrategy>,
    pub mission_control_keyboard_shortcuts: MissionControlKeyboardShortcutsConfig,
}
```

Then add a small helper that can be unit-tested directly:

```rust
fn validate_floating_focus_strategy_mode(
    backend: WmBackend,
    mode: FloatingFocusMode,
    strategy: Option<FloatingFocusStrategy>,
) -> Result<()> {
    match (mode, strategy) {
        (FloatingFocusMode::FloatingOnly, None) => bail!(...),
        (FloatingFocusMode::TilingOnly, Some(_)) => bail!(...),
        _ => Ok(()),
    }
}
```

Tighten `WmConfig::validate()` in this order:

1. collect which WM tables are present (`Option::is_some()`),
2. require exactly one present table,
3. require that table to have `enabled = true`,
4. look up `floating_focus_mode_for_backend(backend)`,
5. validate the backend's `floating_focus_strategy`,
6. run backend-specific nested validation (Mission Control shortcuts).

Do **not** silently default or drop invalid `floating_focus_strategy` values. Fail fast with field-specific errors.

Keep `macos_native_floating_focus_strategy()` returning `Option<FloatingFocusStrategy>`, but make it read from the now-optional field. The macOS adapter can rely on validated config instead of inventing a fallback.

**Step 4: Run the config tests to verify they pass**

Run:

```bash
cargo test --lib config::tests -- --nocapture
```

Expected: PASS, including the new single-table and require/allow/forbid cases.

**Step 5: Commit**

```bash
git add src/config.rs
git commit -m "feat: validate floating focus config by WM contract"
```

### Task 3: Update user-facing config surfaces to match the contract-driven rules

**Files:**
- Modify: `config.example.toml:19-41`
- Modify: `README.md:41-79`
- Modify: `ARCHITECTURE.md:110-112`
- Modify: `src/main.rs:18-22`
- Modify: `flake.nix:149-217`
- Test: `src/main.rs:173-179`

**Step 1: Write the failing user-surface test**

Update the CLI help test in `src/main.rs` so it asserts the new wording:

```rust
#[test]
fn cli_help_describes_configured_wm_selection() {
    let help = Cli::command().render_long_help().to_string();

    assert!(help.contains("exactly one [wm.<backend>] table"));
    assert!(help.contains("wm.macos_native"));
    assert!(help.contains("TilingOnly") || help.contains("tiling-only"));
}
```

If there is already a parse test for `config.example.toml`, leave it in place and let it go red once the example comment block becomes outdated.

**Step 2: Run the focused docs/help test to verify it fails**

Run:

```bash
cargo test --bin yny cli_help_describes_configured_wm_selection -- --nocapture
```

Expected: FAIL because the help text still says non-macOS backends may set `floating_focus_strategy` optionally.

**Step 3: Write the minimal user-surface updates**

Update:

- `config.example.toml`
  - keep exactly one live WM table
  - make the macOS-native example show the required strategy
  - remove comments implying today's tiling-only backends may set the field optionally
- `README.md`
  - explain the contract rule in user terms:
    - `macos_native` requires the field
    - current tiling-only backends must not set it
    - there is no built-in mixed backend yet
- `ARCHITECTURE.md`
  - describe the new contract and stricter single-table validation
- `src/main.rs`
  - update help strings accordingly
- `flake.nix`
  - make the macOS-native option type optional at the schema level
  - update option descriptions to mirror the runtime validation rule
  - if the module already has a natural assertion hook, use it; do **not** add a large new Nix validation subsystem just for this

**Step 4: Run the user-surface checks**

Run:

```bash
cargo test --bin yny cli_help_describes_configured_wm_selection -- --nocapture
cargo test --lib repo_config_example_toml_parses -- --nocapture
nix eval .#packages.aarch64-darwin.default.pname --raw --no-write-lock-file
```

Expected: PASS, and `nix eval` returns `yeetnyoink`.

**Step 5: Commit**

```bash
git add config.example.toml README.md ARCHITECTURE.md src/main.rs flake.nix
git commit -m "docs: describe WM floating focus contract"
```

### Task 4: Run focused verification and fix any regressions before review

**Files:**
- Re-run only; modify files only if verification exposes a regression

**Step 1: Run the focused verification sweep**

Run:

```bash
cargo test --lib built_in_specs_expose_expected_floating_focus_modes -- --nocapture
cargo test --lib config::tests -- --nocapture
cargo test --lib backend_focus_direction_ -- --nocapture
cargo test --lib switch_adjacent_space_via_hotkey_ -- --nocapture
cargo test --lib repo_config_example_toml_parses -- --nocapture
cargo test --bin yny cli_help_describes_configured_wm_selection -- --nocapture
cargo build --release
nix eval .#packages.aarch64-darwin.default.pname --raw --no-write-lock-file
```

Expected: PASS across the sweep.

**Step 2: If anything fails, fix the smallest correct thing**

Rules:

- fix only the regression the verification exposed,
- rerun the failing command first,
- then rerun the full verification sweep,
- do not fold unrelated cleanup into the fix.

**Step 3: Request review before merge**

Use `@superpowers:requesting-code-review` after the sweep is green so the implementation is checked against this plan and the WM contract semantics.

**Step 4: Commit verification fixes only if needed**

```bash
git add <only-the-files-you-fixed>
git commit -m "fix: align WM floating focus contract verification"
```

If the verification sweep is green without code changes, skip this commit.
