## Why

The current `niri-deep` architecture is a niri-centric planner/executor pipeline that relies on per-adapter edge probes and layer-order heuristics. It does not build one global topology for nested domains, so cross-domain focus/move/tear-off behavior is not deterministic across heterogeneous tiling models.

## What Changes

- **BREAKING** Replace the existing `context -> planner -> executor` decision pipeline with a geometry-first orchestrator over a global leaf set.
- **BREAKING** Replace current `DeepApp`/WM action contracts with topology provider/modifier contracts that expose layout snapshots and explicit topology mutations.
- **BREAKING** Remove runtime-only domain tagging and tree-walk neighbor logic; movement and focus decisions will be geometry-driven.
- Add an open pane-state transfer registry for tear-off/merge conversions between domains.
- Add sealed topology-mutation tokens so topology-changing operations cannot be faked by plugin contributors.
- Expand integration architecture to support multiple WM/app domain plugins without core enum edits.

## Capabilities

### New Capabilities
- `cross-domain-topology`: Build and maintain a global domain tree plus flattened leaf map from nested WM/app layouts.
- `geometry-navigation`: Resolve directional focus/move neighbors using geometry instead of tree traversal.
- `pane-transfer-registry`: Negotiate tear-off/merge payload transfer using open pane-state types and converters.
- `plugin-contract-enforcement`: Enforce safe plugin mutation and resync semantics through sealed tokens and trait layering.

### Modified Capabilities
- None.

## Impact

- Replaces core architecture in `src/context.rs`, `src/planner.rs`, `src/executor.rs`, `src/apps/mod.rs`, and `src/window_managers/mod.rs`.
- Requires migration of app adapters (`emacs`, `nvim`, `tmux`, `wezterm`, `vscode`) and WM adapters (current `niri`, new `i3` target).
- Changes CLI command behavior for `focus`, `move`, and `resize` to route through a new orchestrator.
- Introduces new test fixtures and scenarios for geometry solver correctness and cross-domain tear-off/merge flows.
