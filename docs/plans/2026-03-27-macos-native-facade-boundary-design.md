# macOS native facade boundary design

## Problem

`src/adapters/window_managers/macos_native.rs` currently reaches through `macos_window_manager_api`
and directly imports raw topology types, helper algorithms, settle constants, and symbol-validation
details. That makes the outer adapter own too much macOS-specific reasoning.

This is the opposite of the shape we want. A file like `src/adapters/window_managers/niri.rs` reads as
adapter glue around a backend surface; `macos_native.rs` should move in that direction even though the
macOS backend has to build more functionality itself.

## Goals

- Make `macos_native.rs` a thin adapter layer that mainly implements repo WM traits and config handoff.
- Make `macos_window_manager_api` the real adapter-facing macOS backend facade.
- Keep raw topology structs, helper functions, settle loops, and FFI/symbol details private to the
  backend module.
- Return engine-facing records where possible instead of forcing outer code to translate raw macOS
  snapshots manually.
- Preserve current runtime behavior while improving the architectural boundary.

## Non-goals

- No rewrite of the actual focus/move/switch-space behavior in this design.
- No requirement that `macos_native.rs` become textually identical to `niri.rs`.
- No immediate cross-WM abstraction shared with non-macOS backends.
- No public user-facing config changes.

## Approved direction

The approved direction is a **trait-first facade**.

`MacosNativeApi` / `RealNativeApi` should become the boundary that outer `macos_native.rs` talks to.
The outer adapter should not need to know about:

- `RawTopologySnapshot`
- `WindowSnapshot`
- `SpaceKind`
- `REQUIRED_PRIVATE_SYMBOLS`
- `SPACE_SWITCH_*` settle constants
- helper algorithms like `best_window_id_from_windows(...)`

Those are still useful, but only as backend implementation details.

## Boundary rule

Production code outside `mod macos_window_manager_api` should depend only on:

- the backend trait / concrete type,
- backend error types,
- engine-facing records,
- and a small set of semantic adapter-facing plan/result types when a plain record is not enough.

If the outer adapter needs a macOS-specific detail, that detail should be exposed as a semantic value
such as a focus or move plan, not as raw topology plus helper functions.

## Public backend surface

The facade should be organized around adapter jobs instead of macOS implementation details.

### 1. Readiness / connection

The backend should own environment validation:

- required symbol presence,
- accessibility trust,
- and minimal topology availability.

Outer code should not iterate `REQUIRED_PRIVATE_SYMBOLS` itself.

### 2. Engine-facing queries

The backend should directly expose the record shapes the engine needs:

- `FocusedWindowRecord`
- `FocusedAppRecord`
- `Vec<WindowRecord>`

This keeps record conversion inside the backend instead of re-deriving it from raw window snapshots in
outer code.

### 3. Semantic planning

For logic that is still driven by outer adapter policy, the backend should return semantic plan/result
types rather than raw topology. Examples:

- directional focus resolution,
- adjacent-space transition decisions,
- chosen target window plus execution strategy,
- or explicit "no target" / "unsupported" outcomes.

The important property is that these values describe **what should happen**, not **how the topology was
parsed**.

### 4. Explicit operations

Execution helpers such as focus, switch-space, move-to-space, and frame swapping should remain backend
operations. Their retry/settle behavior belongs inside the backend.

## Internal backend layers

`macos_window_manager_api` can keep its current internal layering, but those internals should stay
private:

1. low-level FFI / Core Foundation / SkyLight / AX bindings,
2. raw parsing and topology assembly,
3. pure Rust topology / classification helpers,
4. operation execution and settle logic,
5. test-only visibility helpers inside `mod tests`.

The design does **not** require deleting those helpers. It requires stopping their leakage into the outer
adapter.

## Responsibility split

### Outer `macos_native.rs`

Owns:

- `WindowManagerSession` / spec / capability trait implementations,
- reading config and selecting repo-level policy inputs,
- handing `Direction` / strategy values into the backend,
- mapping backend failures into adapter-returned errors.

Does **not** own:

- topology parsing,
- target selection over raw macOS snapshots,
- settle loops,
- symbol validation,
- or space classification.

### `macos_window_manager_api`

Owns:

- all macOS-specific state collection,
- target-selection helpers,
- cross-space presentation settling,
- fast-path vs fallback focus behavior,
- and conversion from macOS-native state into stable adapter-facing results.

## Data flow

The target flow for a directional action is:

1. outer adapter receives a repo-level request such as `focus_direction(Direction::West)`,
2. outer adapter reads repo config inputs such as floating-focus strategy,
3. outer adapter asks the backend to plan or execute that request,
4. backend snapshots current macOS state, chooses the target, and handles any switch/settle behavior,
5. backend returns success or an explicit semantic failure,
6. outer adapter returns the repo-facing result without touching raw topology.

The same principle applies to `windows()` and `focused_window()`: outer code should ask the backend for
records, not reconstruct them from raw snapshots.

## Error model

Failures should remain explicit at the facade boundary.

Examples:

- missing focused window,
- missing target space / window,
- unsupported Stage Manager target,
- adjacent-space switch failed to settle,
- required private symbol unavailable.

The backend should surface those directly. Outer code should not need to infer them from raw state or
retry loops.

## Testing strategy

### Backend tests

Keep most behavioral tests in `src/adapters/window_managers/macos_native.rs` near the backend internals:

- topology classification,
- target selection,
- space-switch settling,
- pid fast-path vs fallback behavior,
- and macOS-specific operation semantics.

### Outer adapter tests

Keep only adapter-glue tests in the outer layer:

- trait wiring,
- config handoff,
- and a small set of source-shape assertions that ensure the outer file does not drift back into raw
  helper imports.

### Boundary regression tests

Add source-shape guards that fail if the outer import prelude starts depending again on raw backend
implementation symbols such as `RawTopologySnapshot`, `WindowSnapshot`, `SpaceKind`, `REQUIRED_PRIVATE_SYMBOLS`,
or `SPACE_SWITCH_*`.

## Success criteria

This design is successful when all of the following are true:

- top-level production imports in `macos_native.rs` shrink to a small facade surface,
- raw topology/helper symbols can change internally without forcing outer adapter edits,
- `macos_native.rs` reads as adapter glue rather than a second macOS backend,
- and the existing macOS-native test suite still proves behavior at the backend layer.

## Incremental adoption plan

1. Lock the desired boundary with source-shape tests.
2. Introduce semantic adapter-facing facade types and trait methods.
3. Move connection validation behind the facade.
4. Move query/record construction behind the facade.
5. Move directional planning / settle logic behind the facade.
6. Remove raw helper imports from outer production code.
7. Keep any necessary raw/helper visibility only inside backend-local `mod tests`.
