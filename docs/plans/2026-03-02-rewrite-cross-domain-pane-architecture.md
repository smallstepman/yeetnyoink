## Why

The current `yeetnyoink` architecture is a niri-centric planner/executor pipeline that relies on per-adapter edge probes and layer-order heuristics. It does not build one global topology for nested domains, so cross-domain focus/move/tear-off behavior is not deterministic across heterogeneous tiling models.

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


## Context

`yeetnyoink` currently resolves actions by probing an ordered adapter chain (`resolve_chain`) and selecting the first supported operation in planner/executor fallback order. This model is tightly coupled to current app adapter contracts, only has one built-in WM adapter (`niri`), and does not maintain a global nested topology that can answer cross-domain routing deterministically.

The target architecture from `rectangularly dualizable graph vs binary space par.md` requires:
- geometry-based directional solving across all panes/windows,
- explicit domain hierarchy handling for nested tiling systems,
- capability negotiation for cross-domain pane transfer,
- type-safe mutation contracts that force topology resync.

## Goals / Non-Goals

**Goals:**
- Provide deterministic focus/move routing for heterogeneous layouts (BSP, grid, master-stack, columnar) using the same solver.
- Support nested domain orchestration (WM -> terminal mux -> editor) with explicit tear-off and merge semantics.
- Make plugin extension open (new WM/app domains without editing closed core enums).
- Enforce mutation correctness via non-forgeable topology-change tokens and mandatory resync points.

**Non-Goals:**
- Backward compatibility with current planner/executor APIs or old adapter trait shapes.
- Preserving current config surface when it conflicts with the new architecture.
- Implementing every possible app/WM integration in this change; initial set will prioritize niri + i3 and current terminal/editor adapters.

## Decisions

1. **Adopt geometry-first neighbor solving over global leaves**
   - We will flatten all visible leaves from all domains into one list with screen-space rectangles.
   - Neighbor selection is based on directional validity, perpendicular overlap, and nearest receiving edge.
   - **Alternative considered:** tree-walk traversal per domain; rejected because it is layout-shape dependent and fails across non-uniform trees.

2. **Use a forest-of-domains model, not a single tagged arena**
   - Each plugin owns its local layout representation and emits a normalized snapshot.
   - Core orchestrator maintains a global domain tree (containment) and a global leaf vector (geometry).
   - **Alternative considered:** one global arena with runtime `DomainLevel` tags; rejected due weak invariants and contributor error risk.

3. **Split movement and split-axis directions**
   - Introduce `Cardinal` for user intent (`N/S/E/W`) and `SplitAxis` for topology (`Horizontal/Vertical`).
   - **Alternative considered:** reuse one direction enum; rejected due semantic conflation and repeated bugs.

4. **Adopt open pane-state transfer with a registry**
   - Transfer payloads are trait objects (`PaneState`) keyed by type identity; converters are registered at startup.
   - Orchestrator negotiates direct transfer, conversion, or fallback spawn path.
   - **Alternative considered:** closed payload enum in core; rejected because every new integration would require core edits.

5. **Seal topology-change tokens via framework wrapper traits**
   - Contributors implement `*Impl` mutator traits only; framework wraps them and issues sealed `TopologyChanged` tokens.
   - Orchestrator consumes token and triggers mandatory resync.
   - **Alternative considered:** public mutation token type; rejected because it can be forged and breaks correctness guarantees.

6. **Replace planner/executor pipeline with a single orchestration engine**
   - `focus`, `move`, and `resize` commands will call one shared orchestrator with operation intent and focused leaf context.
   - Strategy selection moves from probe-order logic to solver + domain capability checks.

## Risks / Trade-offs

- **[Risk] Adapter migration is large and touches many modules** -> Mitigation: implement compatibility shims only during transition branch and remove before merge.
- **[Risk] Geometry snapshots may be stale under async WM/app updates** -> Mitigation: force resync after every topology mutation and include bounded retry for fresh snapshots.
- **[Risk] Conversion registry may introduce unclear fallback behavior** -> Mitigation: require explicit fallback strategy and structured error reporting when no conversion path exists.
- **[Risk] i3 integration IPC correctness and lifecycle handling are complex** -> Mitigation: isolate i3 transport + parsing module with dedicated integration tests and fixture replay.

## Migration Plan

1. Land new core modules (`geometry`, `domain_tree`, `orchestrator`, `pane_state_registry`, `plugin_contracts`).
2. Implement sealed mutation token flow and integrate mandatory resync points.
3. Port existing adapters to the new plugin contracts (niri, wezterm/tmux, nvim/emacs) with feature-complete focus/move first.
4. Add i3 domain plugin and cross-domain tear-off/merge orchestration paths.
5. Switch CLI commands to the new orchestrator and remove old planner/executor/context pipelines.
6. Run compliance/integration suites and archive legacy modules.

Rollback strategy during rollout: keep changes on the dedicated rewrite branch; if migration blocks execution, revert command dispatch to previous entrypoints until affected adapters are fully ported.

## Open Questions

- Which minimum adapter set is required for initial “rewrite complete” acceptance (niri+wezterm+nvim only vs full current set)?
- Should payload conversion support multi-hop conversion chains in v1, or only direct converters?
- Do we enforce per-workspace scoping in the first i3 implementation or allow global workspace candidate search initially?