# window-manager floating-focus contract design

## Problem

`floating_focus_strategy` currently mixes two concerns:

- whether a backend *can* use floating-window focus strategy selection in `yeetnyoink`, and
- whether that setting is required, optional, or invalid for the selected backend.

Today that rule is partly encoded in config shape itself: `wm.macos_native.floating_focus_strategy` is
required by type, while other backends accept an optional field. That works for the current set of
backends, but it is not expressed as a reusable window-manager contract, and it does not scale cleanly
once we want to distinguish:

- floating-only backends,
- mixed tiling+floating backends,
- and tiling-only backends.

At the same time, the WM config surface should become stricter: only one `wm.<backend>` table should be
present at a time, rather than allowing multiple tables and then inferring selection only from enabled
flags.

## Goals

- Express floating-focus config rules as a backend contract owned by `yeetnyoink`.
- Keep the contract tied to adapter behavior in this project, not the WM's theoretical full feature set.
- Require `floating_focus_strategy` for floating-only backends.
- Allow `floating_focus_strategy` for mixed tiling+floating backends.
- Reject `floating_focus_strategy` for tiling-only backends.
- Enforce that exactly one `wm.<backend>` table is present and that it is enabled.
- Preserve backend-specific nested config such as macOS Mission Control shortcuts.

## Non-goals

- No change to the geometry strategies themselves.
- No change to directional move/rearrange behavior.
- No attempt to infer WM policy from runtime probing.
- No plugin/extensibility mechanism for third-party WM contracts.

## Chosen contract shape

Use a closed enum on the WM contract:

```rust
pub enum FloatingFocusMode {
    FloatingOnly,
    TilingAndFloating,
    TilingOnly,
}
```

This is a better fit than trait-backed policy types because the codebase has:

- a fixed set of built-in backends,
- a closed set of three policy states,
- and config validation that benefits from simple backend-to-policy lookup plus clear error messages.

Trait-backed policy types were considered, but they would encode the same three states indirectly while
adding type indirection without a concrete benefit for the current architecture.

## Contract placement

The contract should live in the engine-owned WM contract layer, next to existing capability declarations.

Primary additions:

- `src/engine/wm/capabilities.rs`
  - add `FloatingFocusMode`
  - extend `WindowManagerCapabilityDescriptor` with
    `const FLOATING_FOCUS_MODE: FloatingFocusMode;`
- `src/engine/wm/configured.rs`
  - extend `WindowManagerSpec` with
    `fn floating_focus_mode(&self) -> FloatingFocusMode;`

Concrete adapters declare the mode once, and their corresponding specs expose it through the runtime
`WindowManagerSpec` boundary.

This keeps the policy engine-owned while still letting config validation consult the selected backend's
declared contract.

## Initial backend mapping

The contract describes current adapter behavior in `yeetnyoink`, not everything the underlying WM could
support in theory.

Initial built-in mapping:

- `macos_native` → `FloatingOnly`
- `niri` → `TilingOnly`
- `i3` → `TilingOnly`
- `hyprland` → `TilingOnly`
- `paneru` → `TilingOnly`
- `yabai` → `TilingOnly`

There is intentionally no built-in `TilingAndFloating` backend yet. The enum still includes that variant
so future mixed-mode adapters can opt in without changing the config model again.

## Config shape

Keep the current per-backend WM tables, but shift floating-focus requirement rules out of serde shape and
into validation.

That means:

- `EnabledWmConfig` continues to hold
  `floating_focus_strategy: Option<FloatingFocusStrategy>`
- `MacosNativeWmConfig` changes from
  `floating_focus_strategy: FloatingFocusStrategy`
  to
  `floating_focus_strategy: Option<FloatingFocusStrategy>`
- macOS-native-specific Mission Control shortcut config remains unchanged

Example:

```toml
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
```

```toml
[wm.niri]
enabled = true
# floating_focus_strategy is invalid here because the adapter is TilingOnly
```

## Validation model

Validation should remain centralized in `WmConfig::validate()`, because it needs to reason across:

- which backend table is present,
- whether it is enabled,
- what contract that backend declares,
- and any backend-specific nested validation rules.

Validation order:

1. Collect which `wm.<backend>` tables are present (`Option::is_some()`), not just which ones are enabled.
2. Require exactly one backend table to be present.
3. Require that single present table to have `enabled = true`.
4. Resolve the backend's declared `FloatingFocusMode`.
5. Validate `floating_focus_strategy` against that mode:
   - `FloatingOnly` → required
   - `TilingAndFloating` → optional
   - `TilingOnly` → forbidden
6. Run backend-specific nested validation such as Mission Control shortcut parsing.

This is stricter than the current "exactly one enabled backend" rule and matches the intended config
shape more closely.

## Error model

Misconfiguration should fail during config preparation with backend-specific messages.

Examples:

- `config must contain exactly one window manager table`
- `wm.yabai must set enabled = true`
- `wm.niri.floating_focus_strategy is invalid because backend 'niri' is TilingOnly in yeetnyoink`
- `wm.macos_native.floating_focus_strategy is required because backend 'macos_native' is FloatingOnly in yeetnyoink`

The important property is that the error explains both the field and the contract reason, so the user can
fix the config without guessing.

## Runtime behavior

This design does not change focus-routing behavior by itself. It only changes how config validity is
declared and enforced.

Runtime implications:

- `macos_native` can continue to read a floating focus strategy from config, but after successful config
  validation it may treat that value as guaranteed for the selected backend.
- tiling-only backends keep their current routing unchanged.
- future mixed backends can opt into floating strategy support without needing another schema redesign.

## Testing strategy

### Config tests

In `src/config.rs`:

- reject zero WM tables present,
- reject multiple WM tables present,
- reject a single present WM table with `enabled = false`,
- require strategy for `FloatingOnly`,
- allow omission or presence for `TilingAndFloating`,
- reject presence for `TilingOnly`,
- preserve unknown-strategy parse failures,
- preserve Mission Control shortcut validation.

### Contract tests

In the WM contract/adapter boundary:

- verify each built-in adapter declares the expected `FloatingFocusMode`,
- verify built-in specs expose the same mode as their adapter contract.

### Integration surface tests

- update `config.example.toml` parsing coverage to reflect the stricter single-table rule,
- update help/docs tests that describe which backends may set `floating_focus_strategy`,
- preserve existing macOS-native focus tests after config fixture updates.

## Recommendation

Adopt the closed enum contract now and make config validation consult it. This keeps the rule explicit,
matches the requested "adapter behavior in yeetnyoink" semantics, and avoids baking one special case
(`macos_native`) permanently into config type shape.
