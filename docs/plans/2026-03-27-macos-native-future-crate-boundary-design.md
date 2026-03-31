# macOS native future-crate boundary design

## Problem

The current `macos_window_manager_api` boundary is cleaner than before, but it is still shaped like
an intermediate facade for `macos_native.rs`, not like a future standalone backend crate.

Today the backend still depends on yeetnyoink-owned concepts such as:

- `crate::engine::runtime::ProcessId`
- `crate::engine::topology::{Direction, FloatingFocusStrategy, Rect, ...}`
- `crate::engine::wm::{FocusedWindowRecord, FocusedAppRecord, WindowRecord}`
- `crate::config::MissionControlShortcutConfig`
- `crate::logging`

That is the wrong long-term split.

The desired future boundary is:

- yeetnyoink owns geometry, topology construction, focus/move policy, and WM records
- `macos_window_manager_api` owns native state collection and native action execution

This round keeps the code in-place on the `macos-native-facade-boundary` branch, but reshapes the
boundary so the backend could later be extracted into its own crate without bringing `crate::*`
dependencies with it.

## Goals

- Make `macos_window_manager_api` future-crate-ready while keeping it in-place for now.
- Remove backend dependencies on `crate::engine::*`, `crate::config`, and `crate::logging`.
- Keep yeetnyoink as the owner of `Rect`, topology building, directional selection, and WM record
  construction.
- Expose enough native data for yeetnyoink to build its own topology and choose targets.
- Keep native focus/move/switch execution details hidden behind backend operations.

## Non-goals

- Do not physically extract a new crate in this round.
- Do not move geometry algorithms or focus policy into the backend.
- Do not redesign user-facing WM behavior as part of this boundary change.
- Do not mix structural file-splitting with boundary cleanup unless it directly supports the new API.

## Approved boundary

`macos_window_manager_api` becomes a native backend, not a policy engine.

### Backend owns

- macOS-native desktop, space, window, and focus state collection
- raw/native identifiers and backend-owned DTOs
- native action execution:
  - focus
  - focus-with-pid / pid-assisted focus
  - space switching
  - move-window-to-space
  - frame move/swap primitives
- native fallback mechanics:
  - AX raise handling
  - same-pid remap logic
  - settle loops
  - private symbol usage

### Yeetnyoink owns

- `Rect` and other repo geometry types
- topology construction from backend DTOs
- directional target selection
- floating-focus strategy
- move/focus policy decisions
- `FocusedWindowRecord`, `FocusedAppRecord`, `WindowRecord`, and `ProcessId`

The rule is:

> The backend exposes facts and explicit native actions. Yeetnyoink derives meaning and chooses what
> to do.

## Boundary data contracts

The backend should stop returning yeetnyoink record types and instead return backend-owned raw DTOs.

### Canonical backend types

- `NativeDesktopSnapshot`
- `NativeSpaceSnapshot`
- `NativeWindowSnapshot`
- typed ids such as `NativeWindowId` and `NativeSpaceId`
- backend-owned pid storage
- `NativeBounds { x, y, width, height }`

`NativeBounds` is transport data, not the canonical geometry model. The outer adapter converts it
into yeetnyoink's `Rect` before any topology or directional logic runs.

### Translation happens outside the backend

Outer `macos_native.rs` should translate:

- `NativeDesktopSnapshot` -> yeetnyoink topology input
- backend pid -> `ProcessId`
- backend window/app data -> WM records

This removes `FocusedWindowRecord`, `FocusedAppRecord`, `WindowRecord`, and `ProcessId` from the
backend surface entirely.

## Backend API shape

The long-term boundary should remove semantic methods like:

- `plan_focus_direction`
- `execute_focus_plan`
- backend-returned WM record helpers

Those were useful for the intermediate facade step, but they are too yeetnyoink-shaped for a future
backend crate.

### Query surface

The backend should expose explicit native-state queries such as:

- `validate_environment(...)`
- `desktop_snapshot()`
- focused-window / focused-id probes when useful
- native verification probes like `onscreen_window_ids()` or `ax_window_ids_for_pid(pid)` when they
  support robust execution

### Action surface

The backend should expose explicit native operations such as:

- `switch_space(space_id)`
- `switch_adjacent_space(direction, space_id)` if the hotkey path remains useful
- `focus_window(window_id)`
- `focus_window_with_pid(window_id, pid)`
- `move_window_to_space(window_id, space_id)`
- frame move/swap primitives

The backend still owns the native mechanics inside those actions. For example, if a chosen target
requires same-pid AX remapping or a settle loop, that remains backend behavior. What moves out is
the choice of *which* target to focus or *when* to switch spaces.

## Control flow after the refactor

Directional actions should become outer-driven:

1. ask the backend for `desktop_snapshot()`
2. build yeetnyoink topology from backend DTOs
3. run directional selection and WM policy in yeetnyoink
4. call one or more explicit backend actions
5. resnapshot only when policy needs confirmation after a switch or move

That means "focus west" is no longer delegated to backend planning. Instead:

- yeetnyoink chooses the west target using its own topology and focus strategy
- the backend executes the chosen native step reliably

## Config and diagnostics

The backend should stop importing repo config and repo logging directly.

### Backend-owned options

Construct the backend with a small backend-owned options struct, for example:

- `NativeBackendOptions`
- `MissionControlModifiers { control, option, command, shift, function }`
- `diagnostics: Option<Arc<dyn NativeDiagnostics>>`

The exact names can change, but the shape matters:

- outer `macos_native.rs` reads repo config
- outer code translates repo config once into backend-owned primitive options
- the backend sees only native-relevant values

This replaces direct use of `MissionControlShortcutConfig` with plain modifier flags or a tiny
backend-owned settings type.

### Optional diagnostics hook

The backend should accept an optional injected diagnostics sink rather than depending on
`crate::logging`.

Production can provide a thin adapter into yeetnyoink logging. Tests can provide a recorder. `None`
means no-op.

This keeps the backend self-contained without losing traceability during tricky focus/switch flows.

## Migration strategy

This round should be implemented as an in-place boundary migration, not a file-move rewrite.

1. Add source-shape tests that lock the desired boundary.
2. Introduce backend-owned DTOs, options, and diagnostics types.
3. Add backend snapshot and primitive action methods alongside the current facade surface.
4. Move outer `macos_native.rs` to build topology and WM records from backend DTOs.
5. Move directional focus/move policy out of backend defaults into outer yeetnyoink code.
6. Delete obsolete semantic facade methods and yeetnyoink-owned return types from the backend API.
7. Consider a later file-tree or crate extraction only after the boundary is clean.

## Testing strategy

### Backend tests

Keep backend-focused tests close to the backend code:

- native snapshot parsing
- raw DTO correctness
- focus/move/switch execution behavior
- same-pid AX fallback behavior
- settle-loop behavior

### Outer adapter tests

Keep outer tests focused on translation and policy:

- DTO -> topology conversion
- DTO -> WM record conversion
- directional target selection using yeetnyoink geometry
- config handoff into backend-owned options

### Boundary regression tests

Add or extend source-shape tests so the backend no longer imports:

- `crate::engine::runtime::ProcessId`
- `crate::engine::wm::*` record types
- `crate::config::MissionControlShortcutConfig`
- `crate::logging`

and so the outer adapter no longer depends on backend planning helpers like
`plan_focus_direction` / `execute_focus_plan`.

## Success criteria

This design is successful when:

- `macos_window_manager_api` can be reasoned about as a self-contained native backend
- yeetnyoink owns all geometry/topology/policy decisions again
- backend imports no longer reach into repo-owned record/config/logging types
- the current in-place implementation can later be extracted with mostly path/module changes rather
  than another semantic redesign
