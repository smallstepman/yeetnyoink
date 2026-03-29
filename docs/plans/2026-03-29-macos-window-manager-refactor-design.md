# macos_window_manager broad maintainability refactor

## Problem

`crates/macos_window_manager` is functionally solid but internally messy after extraction from
`src/adapters/window_managers/macos_native.rs`. The crate root is carrying too many concerns,
platform implementation details are mixed with crate-surface wiring, and tests are concentrated
into a giant `src/tests.rs` file with repeated fake-API boilerplate.

The goal of this refactor is to improve maintainability without changing behavior and without a
deep contract rewrite. Small public API cleanups are acceptable where they make the crate surface
clearer, but the main trait shape should remain recognizable.

## Constraints and decisions

- Use the conservative structural path rather than a trait-heavy redesign.
- Preserve behavior and existing verification expectations.
- Keep the outer adapter thin; backend mechanics stay in this crate.
- Allow small public API cleanups, but do not turn this into a gratuitous breaking rewrite.

## Approaches considered

### Approach A

Split the crate into many focused internal modules and also reshape the main trait into smaller
capability traits plus orchestration layers.

Pros:
- Cleanest long-term shape
- Stronger contracts

Cons:
- Higher churn
- Larger risk surface
- More adapter-facing fallout

### Approach B (approved)

Keep the main trait shape broadly intact, but aggressively improve structure around it: thin
`lib.rs`, move helpers into focused modules, isolate macOS/stub implementations, and reorganize
tests around concern-specific modules and shared support.

Pros:
- High maintainability payoff
- Lower behavioral risk than trait surgery
- Keeps adapter integration familiar

Cons:
- Some existing trait awkwardness remains
- Public surface may still be larger than ideal

### Approach C

Introduce a new backend service object and redesign call flow around it.

Pros:
- Potentially cleanest conceptual model

Cons:
- Effectively a rewrite
- Too much churn for the current goal

## Approved design

### Architecture

`lib.rs` becomes a thin crate root that primarily declares modules and re-exports the intended
public surface.

Internal logic is split into focused modules:

- `api` for public types and the `MacosNativeApi` trait
- `environment` for validation and environment/precondition logic
- `navigation` for cross-space switching and settling behavior
- `focus` for same-space target selection, retries, and AX-backed fallback logic
- `geometry` for native window comparison helpers
- `real_api/{macos,stub}` for platform-specific concrete implementations

Existing raw-topology modules (`desktop_topology_snapshot`, `ax`, `foundation`, `skylight`,
`window_server`, `error`) remain, but the crate root stops being the place where policy logic
accumulates.

### Components and data flow

The adapter-facing flow remains stable: callers work through `RealNativeApi` via
`MacosNativeApi`.

Default trait methods and crate-level helpers delegate into focused internal modules instead of
large inline helper blocks in `lib.rs`.

Typical flow:

1. Adapter invokes a high-level backend operation.
2. `api` routes to helper logic in `navigation` or `focus`.
3. Those helpers consume raw/topology data from `desktop_topology_snapshot`.
4. Spatial comparisons live in `geometry`.
5. Native probing and mutations stay in `real_api/macos`.

This keeps native execution separate from policy/orchestration while preserving the current public
usage model.

### Public API cleanup

Keep the major entry points and data types the adapter already depends on.

Tighten the facade by:

- moving internal-only helpers to `pub(crate)`
- reducing incidental crate-root exports
- keeping `lib.rs` readable as a curated surface instead of a mixed implementation file

The intent is API clarification, not API novelty.

### Error handling

Behavior and semantics stay the same:

- same error enums
- same propagation strategy
- no broad catches or silent fallback additions

The refactor only relocates logic so error-producing paths are easier to reason about and test.

### Testing

The current `src/tests.rs` monolith should be broken up by concern.

Target shape:

- module-scoped tests near `navigation`, `focus`, `geometry`, and `real_api`
- a shared test-support area for reusable fake APIs/builders/helpers
- less repeated `MacosNativeApi` boilerplate in test doubles

Coverage should stay at least equivalent. The reorganization is meant to make future tests easier
to add and easier to trust.

### Execution strategy

Refactor in behavior-preserving slices:

1. thin the crate root and introduce new internal modules
2. move `RealNativeApi` macOS/stub implementations out of `lib.rs`
3. move navigation/focus/geometry/environment helpers into dedicated modules
4. clean up crate-root exports
5. split tests and shared fixtures by concern
6. verify after each logical chunk with the existing macOS-focused commands

## Success criteria

- `lib.rs` is substantially smaller and reads like a facade
- helper logic lives in coherent modules instead of inline monolith blocks
- platform-specific implementation is isolated from crate-surface wiring
- tests are organized by concern with less fixture duplication
- existing macOS verification remains green
