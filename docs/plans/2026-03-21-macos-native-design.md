# macOS native window-manager support design

## Problem

`src/adapters/window_managers/macos_native.rs` is currently a research dump rather than an implementation.
The macOS adapters that exist today (`paneru` and `yabai`) do not share a common native Spaces/window
surface, so each adapter either shells out to its own CLI or falls back to incomplete macOS-specific
helpers. We want `macos_native.rs` to become a shared support module that gives current macOS adapters
direct access to private Spaces-aware primitives.

## Goals

- Implement `macos_native.rs` as a shared macOS-native support module, not a new `WmBackend`.
- Use private SkyLight/CGS primitives, with Accessibility/public metadata as supporting layers.
- Fail fast when the required private symbols, permissions, or topology prerequisites are unavailable.
- Treat fullscreen apps and Split View as first-class Spaces.
- Expose best-effort ordering only for the active Space.

## Non-goals

- No new standalone `macos_native` backend in config or WM registry.
- No claim of strict front-to-back ordering outside the active Space.
- No attempt to model Stage Manager's strip as ordinary Space topology.

## High-level shape

`macos_native.rs` should stay a single shared module with four internal layers:

1. `ffi`
   Unsafe SkyLight/CGS/AX bindings and symbol-resolution helpers.
2. `model`
   Safe Rust types such as `SpaceKind`, `SpaceSnapshot`, `WindowSnapshot`, and error enums.
3. `probe`
   Topology enumeration and metadata enrichment.
4. `ops`
   Focus, Space switching, and window-move actions.

The module should present a narrow safe API upward. A caller such as `paneru` or `yabai` should not
need to reason about raw Core Foundation pointers or private framework details.

## Primary API surface

The shared module should expose a reusable context object, for example `MacosNativeContext`, that owns
the resolved private API surface and any validated preconditions.

The context should provide helpers along these lines:

- `focused_window() -> Result<WindowSnapshot>`
- `windows_in_active_space() -> Result<Vec<WindowSnapshot>>`
- `spaces() -> Result<Vec<SpaceSnapshot>>`
- `focus_window(window_id) -> Result<()>`
- `switch_space(space_id) -> Result<()>`
- `move_window_to_space(window_id, space_id) -> Result<()>`

This is intentionally a support surface rather than a `WindowManagerSession`. Existing adapters can call
it selectively where native macOS topology or operations are needed.

## Private primitives

The implementation should follow the same general direction proven out in Paneru's manager layer:

- `SLSMainConnectionID`
- `SLSCopyManagedDisplaySpaces`
- `SLSManagedDisplayGetCurrentSpace`
- `SLSCopyWindowsWithOptionsAndTags`
- `_AXUIElementGetWindow`
- `_SLPSSetFrontProcessWithOptions`

Other private calls can be added as required for switching or moving windows between Spaces. The unsafe
surface should stay centralized so the rest of the module only deals with safe Rust data.

## Topology semantics

Spaces should be modeled explicitly:

- `Desktop`
- `Fullscreen`
- `SplitView`
- `System`
- `StageManagerOpaque`

Fullscreen apps and Split View should be treated as first-class Spaces because they share the same
managed-space machinery and expose metadata that can be parsed from the private topology layer. Split
View must not assume one app per Space; tile metadata can describe multiple participants.

Stage Manager should be treated differently. The current active stage may still be observable through the
same underlying window/space machinery, but the left-hand strip should not be modeled as ordinary Space
topology. If a caller asks for behavior that depends on strip membership or ordering, the module should
return an explicit unsupported error.

## Ordering policy

Window ordering is best-effort only for the active Space.

The implementation should combine active-space membership with currently visible window information and
window-level hints where available. Other Spaces should expose membership without claiming stable
front-to-back order.

## Data flow

Connection and probing should work like this:

1. Resolve or link the required private framework entry points.
2. Validate required permissions and core topology prerequisites.
3. Enumerate managed displays and Spaces.
4. Classify each Space into `Desktop`, `Fullscreen`, `SplitView`, `System`, or `StageManagerOpaque`.
5. Query windows for relevant Spaces.
6. Enrich active-space windows with AX/public metadata when titles, focus, or application identity are
   needed.

The important boundary is that parsing and classification should be testable without private calls. Raw
framework output should be converted into small internal structs first, then classified by pure Rust
logic.

## Failure model

The module should fail fast during initialization when:

- required private symbols are unavailable,
- required user-granted permissions are missing,
- a required operation surface cannot be validated,
- or the current topology mode cannot be represented safely.

Operational calls should return explicit unsupported errors for things like Stage Manager strip routing
instead of pretending support exists and degrading silently.

## Testing strategy

Most tests should be unit tests around safe logic rather than live private API calls.

Key coverage areas:

- classification of desktop/fullscreen/split/stage-manager-like spaces,
- ordering heuristics for the active Space,
- missing-symbol and missing-permission failures,
- operation routing decisions,
- and conversion from raw topology payloads into repo-facing snapshots.

Any live macOS-specific validation should remain optional and narrowly scoped. The core module should be
designed so its behavior is mostly verified through mocked FFI shims and parser fixtures.

## Adoption plan

Implementation should land in `src/adapters/window_managers/macos_native.rs` first, then current macOS
adapters can adopt it incrementally for focused-window lookup, window enumeration, cross-Space focus,
and Space-aware movement.
