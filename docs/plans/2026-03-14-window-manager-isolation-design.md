# Window manager isolation design

## Problem

Window-manager-specific logic currently leaks outside `src/adapters/window_managers/` in several directions:

- WM selection in `src/adapters/window_managers/mod.rs` probes the host environment and hardcodes detector priority.
- `src/engine/domain.rs` string-matches `"niri"` to attach a special WM domain plugin.
- `src/commands/focus_or_cycle.rs` imports `niri::Niri` directly and embeds Niri workspace/monitor behavior in command code.
- `src/adapters/window_managers/mod.rs` owns a large `SelectedWindowManager` enum and match-based forwarding layer, so shared WM plumbing knows every concrete WM type.
- Shared WM execution traits currently expose Niri-shaped primitives such as `move_column` and `consume_into_column_and_move`.

This makes the WM layer harder to extend, keeps command/engine code coupled to concrete compositors, and pushes architectural knowledge into the wrong modules.

## Approved constraints

- `config.toml` is the source of truth for WM selection.
- Keep `WmBackend` as a closed enum of built-in WMs.
- If the configured WM is unavailable or fails to connect, fail fast with a clear error.
- WM-specific UX commands should stay generic in the CLI and dispatch through optional WM capability traits rather than concrete WM types.

## Goals

- Move WM-specific connection, feature wiring, and domain wiring into the WM-specific modules.
- Make shared runtime code depend on uniform contracts and optional capabilities, not concrete WM names.
- Remove runtime WM detection/probing from adapter selection.
- Keep generic CLI commands generic even when only some WMs support them.

## Non-goals

- Turning WM integrations into a dynamic plugin system.
- Changing app/mux orchestrator semantics beyond what is required to isolate WM concerns.
- Expanding unsupported WM features as part of this refactor.

## Proposed architecture

### 1. Built-in WM registry driven by config

Keep `WmBackend` in `src/config.rs`, but replace the current detector/priority registry with a built-in spec registry keyed directly by that enum.

Each WM file owns one spec/factory object that knows how to assemble the runtime pieces for that WM. Conceptually:

- `niri.rs` owns `NiriSpec`
- `i3.rs` owns `I3Spec`
- `paneru.rs` owns `PaneruSpec`
- `yabai.rs` owns `YabaiSpec`

`connect_selected()` becomes:

1. Read `WmBackend` from config.
2. Resolve the matching built-in WM spec.
3. Ask that spec to connect.
4. Return a configured WM handle or a direct error.

No detector functions, process probing, or fallback priority remain in the selection path.

### 2. Replace the giant selected-WM enum with an object-safe runtime handle

The current runtime-facing WM trait surface is not object-safe because `WindowManagerIntrospection` uses a GAT (`FocusedWindow<'a>`). That is why `mod.rs` currently needs `SelectedWindowManager` and `SelectedFocusedWindow` enums plus large match-forwarding impls.

The shared/runtime-facing WM handle should switch to owned snapshot-based data so it can be type-erased. For example:

- focused-window queries return an owned `FocusedWindowRecord`
- window listing returns `Vec<WindowRecord>`
- execution methods stay uniform and object-safe

This yields a runtime shape closer to:

- `ConfiguredWindowManager`
  - `core`: object-safe WM session
  - `domain_factory`: optional WM domain provider
  - `cycle_provider`: optional `focus-or-cycle` capability
  - other optional WM feature providers as needed

`mod.rs` should keep shared contracts, shared value types, and generic capability math, but it should stop naming every WM in runtime dispatch code.

### 3. Keep only uniform operations in the shared core contract

The shared core WM contract should expose only operations that make sense across WMs, such as:

- adapter identity and capabilities
- focused window snapshot
- window list snapshot
- directional focus
- directional move
- resize intent
- spawn
- focus-by-id
- close-by-id

Niri-shaped execution primitives such as `move_column` and `consume_into_column_and_move` should not remain part of the shared runtime interface.

If a WM needs extra internal steps to realize a higher-level plan, those steps belong behind the WM-specific module boundary.

### 4. Model WM-specific features as optional capabilities

Features that are not universal should be modeled as optional capability traits provided by the selected WM spec, rather than added to the shared core contract.

Examples:

- `WindowManagerDomainFactory`
- `WindowCycleProvider` for `focus-or-cycle` / summon behavior
- WM-specific placement/composition helper for composed tear-out behavior if still needed

Shared code asks whether the selected WM exposes the capability. If it does, it uses it. If not, it returns a precise unsupported error.

This keeps the CLI generic while still allowing rich WM-specific behavior to live in the corresponding WM module.

## Responsibility split

### Shared WM layer (`src/adapters/window_managers/mod.rs`)

- shared contracts and shared value types
- capability declarations and validation
- generic capability planning
- built-in spec lookup by `WmBackend`
- object-safe configured WM handle
- generic unsupported-capability errors

### WM-specific files (`src/adapters/window_managers/<wm>.rs`)

- WM connection details
- WM-specific command encoding / IPC wiring
- WM-specific optional capability implementations
- WM-specific domain factory wiring
- WM-specific higher-level composition steps

### Command/engine layers

- consume only the object-safe WM handle and optional capabilities
- never import concrete WM types directly
- never string-match on WM names for behavior routing

## Data flow changes

### Selection

Current flow:

- config override -> optional detector-based registry lookup -> connect concrete selected enum

Proposed flow:

- config `WmBackend` -> built-in spec lookup -> `spec.connect()` -> `ConfiguredWindowManager`

### Domain wiring

Current flow:

- `engine/domain.rs` checks `wm.adapter_name()`
- `"niri"` gets a special domain plugin
- every other WM gets a generic unsupported plugin

Proposed flow:

- the selected WM spec exposes an optional domain factory/provider
- `engine/domain.rs` asks the configured WM for that capability
- if absent, shared code produces a generic unsupported-domain path without naming a specific WM

This keeps the actual Niri domain implementation in `niri.rs` while removing Niri branching from engine code.

### Generic WM UX commands

Current flow:

- `focus_or_cycle.rs` imports `niri::Niri`
- command code calls Niri workspace and monitor APIs directly

Proposed flow:

- the CLI command stays generic
- the command resolves the selected WM once
- the command asks for `WindowCycleProvider`
- Niri implements the provider internally using its existing workspace/monitor behavior
- unsupported WMs return a clean capability error

This isolates Niri-specific summon behavior without making the CLI itself Niri-shaped.

### Core movement/resize

Shared code can continue to perform generic capability planning, but it must not rely on public Niri-only primitives. If composed WM behavior still needs WM-specific substeps, the plan should terminate in a WM-owned helper/capability rather than extra shared trait methods.

## Error handling

- Invalid or unsupported configured WM backend: clear startup error naming the backend and platform mismatch.
- Connection failure: clear startup error naming the backend and the failed connection step.
- Unsupported optional feature: explicit error such as `wm 'yabai' does not support focus-or-cycle`.
- No silent fallback from the configured WM to any other WM.
- No name-based special cases in shared command/engine code.

## Testing strategy

- Add selection tests proving WM choice comes only from config and not from detector/probing behavior.
- Add spec-level tests for each built-in WM:
  - connects or surfaces a deterministic connection error
  - advertises the expected core capabilities
  - reports presence/absence of optional capabilities
- Add command tests proving generic WM commands dispatch through optional capabilities instead of importing concrete WMs.
- Add domain tests proving `engine/domain.rs` no longer branches on WM names.
- Keep capability validation tests for shared capability math.
- Final verification for implementation work should include both `cargo test` and `cargo build --release`, because the runtime setup uses the release binary.

## Migration order

1. Introduce the new built-in WM spec abstraction and the object-safe configured WM handle alongside the current code.
2. Port Niri first, including its domain factory and `focus-or-cycle` capability.
3. Port i3, paneru, and yabai to the same spec shape.
4. Switch `connect_selected()` to config-only built-in spec resolution and delete detector/probing code.
5. Move command/domain wiring over to optional capabilities and remove direct concrete-WM imports from shared code.
6. Remove `SelectedWindowManager`, `SelectedFocusedWindow`, and other name-based branching that becomes obsolete.

## Expected outcome

After this refactor, the WM-specific files remain the only place that understand compositor-specific connection details, optional WM-only features, and WM-specific domain behavior. Shared runtime code becomes smaller and more uniform: it selects the configured WM, talks to it through stable contracts, and uses optional capabilities when a feature is not universal.
