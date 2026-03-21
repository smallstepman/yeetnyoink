# Hyprland window manager support

## Problem

`yeetnyoink` currently supports `niri` and `i3` on Linux, but not Hyprland. That leaves the common window-manager layer unavailable in Hyprland sessions even though Hyprland exposes stable CLI and JSON IPC through `hyprctl`.

## Goals

- Add a built-in `hyprland` WM backend selectable via `[wm].enabled_integration = "hyprland"`.
- Support the common `WindowManagerSession` surface:
  - `focused_window`
  - `windows`
  - `focus_direction`
  - `move_direction`
  - `resize_with_intent`
  - `spawn`
  - `focus_window_by_id`
  - `close_window_by_id`
- Update docs, examples, and tests so Hyprland is treated like the other Linux WM integrations.
- Verify the first cut against a live Hyprland session after implementation.

## Non-goals

- No runtime WM auto-detection. WM selection remains explicit through config.
- No Hyprland-specific `WindowCycleProvider` / `focus-or-cycle` support in the first cut.
- No Hyprland domain factory or WM tear-out composer in the first cut.
- No broad refactor of existing Linux WM adapters unless a tiny shared helper clearly reduces duplication.

## Recommended approach

Implement a new CLI-backed adapter in `src/adapters/window_managers/hyprland.rs` and wire it into the existing `WmBackend` / `WindowManagerSpec` flow.

This is the smallest change that matches the current architecture:

- it fits the existing `WindowManagerSession` trait cleanly,
- it matches the repository’s explicit-config model,
- it is easy to unit test from captured JSON / command construction,
- and it leaves room for a later Hyprland-native expansion without forcing extra abstractions now.

Alternatives considered:

1. **Richer Hyprland-native integration first**  
   Could add extra workspace/domain behavior or cycle features, but it increases scope and risk for the initial backend.

2. **Refactor Linux WM helpers before adding Hyprland**  
   Might reduce duplication eventually, but it delays the actual feature and touches already-working backends unnecessarily.

## Design

### Adapter boundary

Add a Linux-only `HyprlandAdapter` and `HyprlandSpec`, mirroring the existing adapter structure used by `i3` and `niri`.

Files touched:

- `src/adapters/window_managers/hyprland.rs`
- `src/adapters/window_managers/mod.rs`
- `src/config.rs`
- existing WM connector / config tests
- `README.md`
- CLI help text/examples that enumerate supported WMs

`WmBackend` gains a new `Hyprland` variant with:

- `snake_case` TOML name: `hyprland`
- Linux-only `supported_on_current_platform() == true`
- participation in `spec_for_backend(...)` and built-in spec tests

The first cut will keep Hyprland on the same contract boundary as `i3`: a WM session core only, without extra WM-owned features.

### Capabilities

Declare Hyprland capabilities conservatively, matching the common Linux WM contract:

- native directional focus
- native directional move
- native resize
- no WM tear-out support
- no composed tear-out support
- no special column primitives

That means the capability shape should start effectively i3-like:

- `tear_out`: unsupported in all directions
- `resize`: native in all directions
- `primitives.move_column = false`
- `primitives.consume_into_column_and_move = false`
- `primitives.tear_out_right = false`
- `primitives.set_window_width = true`
- `primitives.set_window_height = true`

### IPC and data mapping

The adapter uses `hyprctl` directly:

- `hyprctl -j activewindow`
- `hyprctl -j clients`
- `hyprctl dispatch ...`

Directional mapping:

- west → `l`
- east → `r`
- north → `u`
- south → `d`

Window identity mapping:

- Hyprland identifies windows by hex `address` strings such as `0xaaaae329c5d0`.
- The adapter will parse that value into the engine’s required `u64` id by trimming the `0x` prefix and decoding hex.
- When targeting a specific window again, the adapter formats the id back to Hyprland syntax as `address:0x{id:x}`.

Metadata mapping:

- `class` → `WindowRecord.app_id`
- `title` → `WindowRecord.title`
- `pid` → `WindowRecord.pid`
- focused state comes from the active window address
- `original_tile_index = 1` for all windows because Hyprland is not participating in the niri-style tear-out composition path

For `windows()`, start with mapped clients from `hyprctl -j clients`. If live verification shows that hidden mapped clients pollute routing, tighten the filter during implementation with an explicit test.

### Command mapping

- `focused_window()`  
  Read `hyprctl -j activewindow`, parse the focused client, and return a `FocusedWindowRecord`.

- `windows()`  
  Read `hyprctl -j clients`, convert each mapped client to `WindowRecord`, and mark the focused one by matching its address to the active window address.

- `focus_direction(direction)`  
  `hyprctl dispatch movefocus <l|r|u|d>`

- `move_direction(direction)`  
  `hyprctl dispatch movewindow <l|r|u|d>`

- `resize_with_intent(intent)`  
  `hyprctl dispatch resizeactive <dx> <dy>` where the sign is derived from both direction and grow/shrink intent.

- `spawn(command)`  
  `hyprctl dispatch exec <joined command>`

- `focus_window_by_id(id)`  
  `hyprctl dispatch focuswindow address:0x...`

- `close_window_by_id(id)`  
  `hyprctl dispatch closewindow address:0x...`

### Error handling

The adapter should fail loudly and specifically:

- if `hyprctl` is missing,
- if `hyprctl` returns a non-success status,
- if stderr/stdout indicates malformed output,
- if JSON deserialization fails,
- or if a window address cannot be parsed back and forth cleanly.

Do not silently skip invalid clients and do not add fallback behavior that pretends Hyprland succeeded. This backend should either produce a valid `WindowManagerSession` result or return a clear error.

## Testing plan

Add targeted tests instead of broad refactors:

1. **Config tests**
   - `enabled_integration = "hyprland"` deserializes correctly
   - `WmBackend::Hyprland.as_str() == "hyprland"`
   - `supported_on_current_platform()` matches Linux gating

2. **WM spec wiring tests**
   - `spec_for_backend(WmBackend::Hyprland)` returns a valid `WindowManagerSpec`
   - built-in spec coverage tests include Hyprland

3. **Capability validation tests**
   - declared capabilities validate successfully
   - tear-out / resize planning matches the intended conservative capability shape

4. **JSON parsing tests**
   - parse `activewindow` payloads
   - parse `clients` payloads
   - round-trip window address hex strings to/from `u64`
   - preserve `class`, `title`, `pid`, and focus state

5. **Command construction tests**
   - focus command uses `movefocus`
   - move command uses `movewindow`
   - resize command computes the expected signed deltas
   - focus-by-id and close-by-id target `address:0x...`
   - spawn uses `dispatch exec`

## Live verification after implementation

Because the current session is running inside Hyprland, verify the first implementation with:

1. read-only checks first:
   - `focused_window`
   - `windows`
   - address parsing against live IPC payloads

2. then a few real commands:
   - directional focus
   - directional move
   - resize grow/shrink on both axes
   - focus-by-id / close-by-id only if a safe test window is available

## Notes for implementation

- Keep the change surgical and consistent with the existing adapter style.
- Prefer small helper functions inside `hyprland.rs` over introducing a new cross-adapter abstraction unless duplication is clearly structural.
- Preserve the repository’s current explicit-config model: no runtime WM probing, no implicit fallback to another Linux WM.
