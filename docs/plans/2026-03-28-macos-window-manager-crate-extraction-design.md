# macOS window manager crate extraction design

## Problem

The macOS native backend boundary was already reshaped to be future-crate-ready, but the backend is
still duplicated in the wrong place:

- `src/adapters/window_managers/macos_native.rs` still contains the production
  `mod macos_window_manager_api`
- `crates/macos_window_manager/` now exists, but extraction is only partial
- the new crate is not yet the single source of truth for backend production code
- backend-focused tests and source-boundary checks still mostly live in the main crate

That means the conceptual boundary is right, but the physical code ownership is not finished.

## Approved direction

The target for this round is **full production extraction**:

- `crates/macos_window_manager` becomes the only home for macOS-native backend production code
- `src/adapters/window_managers/macos_native.rs` becomes an outer adapter only
- backend-focused tests move into the new crate as part of the same pass
- no production shim or inline compatibility module remains in `macos_native.rs`

## Goals

- Make `crates/macos_window_manager` the sole source of macOS-native backend production logic.
- Remove the production inline `mod macos_window_manager_api` from
  `src/adapters/window_managers/macos_native.rs`.
- Keep the previously approved boundary intact:
  - yeetnyoink owns geometry, topology, directional policy, and WM record construction
  - the extracted crate owns native DTOs, native state collection, and native action execution
- Move backend-focused tests to the extracted crate.
- Preserve current macOS behavior and the recent Split View regressions/fixes during the extraction.

## Non-goals

- Do not move yeetnyoink geometry or directional policy into the new crate.
- Do not redesign the macOS behavior model while extracting.
- Do not introduce a transitional production shim just to reduce churn.
- Do not broaden the extraction into unrelated adapter cleanup.

## Current state

The repository already contains the start of the extracted crate:

- `crates/macos_window_manager/Cargo.toml`
- `crates/macos_window_manager/src/lib.rs`
- partially split backend modules such as:
  - `ax.rs`
  - `desktop_topology_snapshot.rs`
  - `error.rs`
  - `foundation.rs`
  - `skylight.rs`
  - `window_server.rs`

However:

- the root crate is not yet using the extracted crate as the production backend surface
- `macos_native.rs` still contains the old inline backend module
- backend tests and source-shape assertions are still centered on the monolithic main-crate file

## Target architecture

### `crates/macos_window_manager` owns

- backend-native DTOs and ids
  - `NativeDesktopSnapshot`
  - `NativeSpaceSnapshot`
  - `NativeWindowSnapshot`
  - `NativeBounds`
  - `NativeDirection`
  - `ActiveSpaceFocusTargetHint`
- backend options/diagnostics
  - `NativeBackendOptions`
  - `MissionControlHotkey`
  - `MissionControlModifiers`
  - `NativeDiagnostics`
- backend errors
  - `MacosNativeConnectError`
  - `MacosNativeProbeError`
  - `MacosNativeOperationError`
- backend-native traits and runtime state
  - `MacosNativeApi`
  - `RealNativeApi`
- backend-native modules
  - AX access
  - Core Foundation / dylib / raw interop support
  - SkyLight queries/actions
  - CG window-server and front/raise helpers
  - raw desktop-topology snapshot parsing
- backend-native execution helpers
  - native focus helpers
  - pid-assisted focus recovery
  - active-space stale-target recovery
  - settle loops
  - native move/switch primitives

### `src/adapters/window_managers/macos_native.rs` owns

- yeetnyoink config translation into backend-owned options
- diagnostics hookup from repo logging into backend diagnostics
- conversion between repo geometry/types and backend-native types
- topology construction from backend DTOs
- WM record construction
- directional focus/move policy
- integration of backend actions into the window-manager adapter contract
- outer integration/regression tests that validate adapter behavior

## Public API shape

The extracted crate should expose a narrow production surface tailored to what the outer adapter
actually consumes. The adapter should not reach into raw CF/SkyLight/AX implementation details.

The public surface should therefore be:

- backend DTOs/options/errors
- `MacosNativeApi`
- `RealNativeApi`
- backend-native helper entry points that the outer adapter calls directly

Implementation details should remain private to crate modules unless they are explicitly needed by
the outer adapter or by crate-local tests.

## Test ownership after extraction

### Tests that move into `crates/macos_window_manager`

- backend parser/unit tests in extracted modules
- backend-native focus/switch/move behavior tests
- same-pid AX fallback tests
- stale-target remap tests that are backend-mechanics tests
- source/boundary checks whose purpose is now to ensure the extracted crate stays self-contained

### Tests that stay in `macos_native.rs`

- outer adapter topology/WM-record conversion tests
- directional selection and policy tests
- adapter integration/regression tests that validate yeetnyoink-owned behavior
- tests that intentionally exercise the adapter boundary rather than backend internals

### Source-shape tests

The existing source-string tests that inspect the inline backend module need to be reworked because
their current premise disappears once the backend is physically extracted.

The replacement strategy is:

- remove tests that only prove the inline module still exists
- keep or rewrite tests that prove the real architectural contract still holds
  - no yeetnyoink production imports inside the extracted crate backend
  - no backend production module embedded in `macos_native.rs`
  - the adapter still owns outer geometry/topology/policy

## Migration plan

1. Make the new crate a real workspace member and add the path dependency from the root crate.
2. Finish moving backend production code from the inline module into crate modules.
3. Define the crate's final public surface in `crates/macos_window_manager/src/lib.rs`.
4. Rewrite `macos_native.rs` to import the extracted crate instead of the inline module.
5. Delete the production inline `mod macos_window_manager_api`.
6. Move backend-focused tests into the new crate.
7. Replace obsolete inline-source boundary tests with extracted-crate boundary checks.
8. Run focused backend tests, adapter tests, and release build verification.

## Verification

At minimum, the completed extraction should verify with:

- `cargo test -p macos_window_manager`
- `cargo test macos_native --lib -- --nocapture`
- `cargo build --release`

If crate integration changes require broader confidence, run the relevant broader repo slice after
the focused extraction checks are green.

## Success criteria

The extraction is complete when all of the following are true:

- `macos_native.rs` no longer contains production backend implementation code
- the extracted crate has no production dependency on yeetnyoink `crate::...` modules
- backend-focused tests live with the extracted crate
- the outer adapter still owns geometry, topology, and routing policy
- the verified macOS Split View and Space-navigation fixes remain intact after the split
