## 1. Core architecture reset

- [x] 1.1 Replace `context/planner/executor` flow with a single orchestration pipeline for focus/move/resize intents
- [x] 1.2 Introduce core geometry primitives (`Rect`, `GlobalLeaf`) and separate direction types (`Cardinal`, `SplitAxis`)
- [x] 1.3 Add global topology state model (`GlobalDomainTree` + flattened leaf cache)

## 2. Plugin contracts and topology lifecycle

- [x] 2.1 Define new plugin trait stack (`TopologyProvider`, `TopologyModifierImpl`, `TopologyModifier`, `TilingDomain`)
- [x] 2.2 Implement sealed `TopologyChanged` token pattern and framework wrapper issuance
- [x] 2.3 Enforce post-mutation topology resync in orchestrator control flow

## 3. Geometry solver and routing

- [x] 3.1 Implement geometry-based neighbor solver with directional validity, perpendicular overlap, nearest-distance ranking, and deterministic tie-breakers
- [x] 3.2 Implement routing decision layer that executes same-domain internal action vs cross-domain transfer based on selected source/target leaves
- [x] 3.3 Remove legacy tree-walk and adapter-order fallback heuristics from movement/focus decision logic

## 4. Cross-domain transfer negotiation

- [x] 4.1 Implement open `PaneState` payload abstraction and runtime converter registry
- [x] 4.2 Implement tear-off/merge transfer pipeline with conversion negotiation and explicit fallback strategy
- [x] 4.3 Add structured error/reporting path for unsupported transfer conversions

## 5. Adapter migration

- [x] 5.1 Port niri WM integration to the new domain plugin contract
- [x] 5.2 Port terminal integrations (wezterm, tmux) to normalized topology snapshots and mutation APIs
- [x] 5.3 Port editor integrations (nvim, emacs) to normalized topology snapshots and mutation APIs
- [x] 5.4 Implement i3 integration as a first-class WM domain plugin

## 6. Command wiring, validation, and cleanup

- [x] 6.1 Rewire CLI `focus`, `move`, and `resize` commands to the new orchestrator entrypoint
- [x] 6.2 Add unit and integration tests for solver correctness and cross-domain Neovim -> WezTerm -> WM scenarios
- [x] 6.3 Remove obsolete legacy modules/config toggles and update docs to reflect the rewritten architecture
