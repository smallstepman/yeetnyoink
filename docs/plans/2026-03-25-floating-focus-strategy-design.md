# floating-window focus strategy design

## Problem

Floating-window focus currently relies on a single implicit geometry heuristic. That makes behavior hard to
reason about, hard to compare, and hard to tune across window managers with floating layouts. We need to
codify floating-window directional focus as explicit strategies instead of baking one geometry policy into
the topology helper.

## Goals

- Codify floating-window directional **focus** as named strategies.
- Keep the strategy surface simple and user-facing: flat named presets, not a composable grammar.
- Store strategy selection per WM backend.
- Require an explicit floating focus strategy for `wm.macos_native`.
- Make the strategy field optional and disabled-by-default for tiling or mixed backends.
- Centralize the geometry policy in reusable engine helpers so multiple WMs can share it.

## Non-goals

- No directional move or rearrange strategy changes in this design.
- No global cross-WM floating focus knob.
- No user-facing composable strategy DSL.
- No requirement that every backend adopt floating focus strategies immediately.

## Strategy catalog

The approved surface is a flat preset enum.

- `radial_center`
  Focus candidates inside a directional cone/triangle anchored at the source window center, then choose the
  candidate with the smallest center-to-center distance.
- `trailing_edge_parallel`
  Compare the source window's edge opposite the requested direction to the candidate window's opposite edge,
  then prefer the smallest directional gap.
- `leading_edge_parallel`
  Compare the source window's edge facing the requested direction to the candidate window's edge facing the
  same direction, then prefer the smallest directional gap.
- `cross_edge_gap`
  Compare the source window's trailing edge to the candidate window's leading edge, then prefer the
  smallest directional gap.
- `overlap_then_gap`
  Prefer candidates with perpendicular overlap first, then choose the smallest directional gap.
- `ray_angle`
  Prefer the candidate whose anchor deviates the least from the exact cardinal ray, then break ties by
  distance.

Shared invariants across all strategies:

- candidates must still lie in the requested half-plane,
- non-focusable windows remain excluded by backend-specific filtering,
- and ties fall back to smaller perpendicular offset, then stable ordering.

Terminology:

- **Leading edge**: the edge facing the requested direction.
- **Trailing edge**: the opposite edge.

Examples:

- `focus west` → leading edge is the left edge, trailing edge is the right edge.
- `focus east` → leading edge is the right edge, trailing edge is the left edge.
- `focus north` → leading edge is the top edge, trailing edge is the bottom edge.
- `focus south` → leading edge is the bottom edge, trailing edge is the top edge.

## Config shape

The setting lives under each backend table.

Required for `macos_native`:

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

Optional for other backends:

```toml
[wm.yabai]
enabled = true
# floating_focus_strategy = "overlap_then_gap"
```

Validation rules:

- `wm.macos_native.floating_focus_strategy` is required.
- For other backends, omitting the field disables the strategy and preserves current behavior.
- Unknown strategy names fail during config preparation, not at focus time.

## Architecture

The geometry policy should stay centralized near the existing directional-selection helper.

Primary insertion point:

- `src/engine/topology.rs`

Proposed additions:

- `enum FloatingFocusStrategy`
- `select_closest_in_direction_with_strategy(...)`

The existing `select_closest_in_direction(...)` helper should remain as a wrapper around the current default
behavior so non-floating callers do not need to change all at once.

Backends remain responsible for:

- deciding which windows become directional candidates,
- building `DirectedRect` values,
- and deciding when floating-focus strategy selection is relevant.

The engine helper remains responsible for:

- applying the chosen strategy's geometric scoring,
- enforcing shared directional invariants,
- and returning the winning candidate.

## Data flow

1. Config parsing reads `floating_focus_strategy` from the selected backend table.
2. `src/config.rs` validates required vs optional backends and exposes a typed getter.
3. WM adapter code resolves its floating focus strategy from config.
4. The adapter gathers focusable floating candidates as `DirectedRect`s.
5. The adapter calls `select_closest_in_direction_with_strategy(...)`.
6. If no in-space candidate is found, existing off-space fallback behavior remains unchanged.

Initial adoption target:

- `src/adapters/window_managers/macos_native.rs`

Future adopters can opt in one at a time without changing the public strategy surface.

## Failure model

- Missing required `wm.macos_native.floating_focus_strategy` is a config error.
- Unknown strategy names are config errors.
- Optional backends with no strategy set continue to use existing focus behavior.
- If a strategy finds no candidate, the adapter's existing adjacent-space or WM-native fallback continues.

This keeps failures early and explicit while preserving current runtime fallbacks.

## Testing strategy

### Config coverage

In `src/config.rs`:

- require `floating_focus_strategy` for `wm.macos_native`,
- accept omission for optional backends,
- reject unknown strategy names,
- and verify the typed getter resolves the chosen preset.

### Engine coverage

In `src/engine/topology.rs`:

- add table-driven layouts that run the same candidate set through each preset,
- assert that each preset chooses the intended target,
- and verify shared tie-breaking and half-plane filtering rules.

### Adapter coverage

In `src/adapters/window_managers/macos_native.rs`:

- verify strategy-specific focus outcomes on floating window fixtures,
- preserve existing adjacent-space fallback when no in-space target exists,
- preserve overlay/layer exclusion behavior,
- and cover multi-display scoping.

### Edge cases

- partially overlapping windows,
- zero overlap,
- exact tie scores,
- stacked/overlapping floating windows,
- non-normal-layer overlays,
- and no candidate in the requested half-plane.

## Recommendation

Implement the flat preset enum first, require it for `macos_native`, and keep other backends opt-in. This
captures the approved design without overcommitting to a more abstract user-facing model, while still
keeping the underlying geometry logic reusable for future floating backends.
