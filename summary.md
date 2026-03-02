# Implementation Spec (Extracted for Future Work)

## 1) Problem statement and target behavior
Build a Rust orchestration layer that can:
- Move focus directionally across panes/windows across nested domains (editor inside terminal multiplexer inside WM).
- Move/tear-off panes across domain boundaries (e.g., Neovim pane becomes WezTerm pane or i3 window).
- Merge panes back inward into another domain while preserving pane state.
- Work across heterogeneous layout engines (BSP, master-stack, grid, container-based).

Primary example discussed: `move west` while focused on the left-most Neovim pane inside WezTerm inside i3.

---

## 2) Final architectural conclusions (superseding earlier approach)

1. **Topology abstraction:** Use slicing-tree semantics for native layouts, but **do not** use tree-walking as the global navigation algorithm.
2. **Global navigation:** Use a **geometry-first solver** over a flat list of leaf rectangles (`Rect`) across all domains.
3. **Type correctness:** Split layout axis and movement direction:
   - `SplitAxis { Horizontal, Vertical }` for topology.
   - `Cardinal { North, South, East, West }` for user intent.
4. **Plugin extensibility:** Replace closed `PanePayload` enum with open `PaneState` + `TypeId` capability negotiation/conversion.
5. **Sync correctness:** Use a sealed, non-forgeable `TopologyChanged` token so topology-changing calls force resync.
6. **Data model:** Prefer **forest-of-arenas** (one per domain plugin) + daemon-maintained global flat leaf projection.

---

## 3) Core type and trait contracts

### 3.1 Direction and geometry
```rust
pub enum SplitAxis { Horizontal, Vertical }
pub enum Cardinal { North, South, East, West }

pub struct Rect { pub x: i32, pub y: i32, pub w: i32, pub h: i32 }
```

Required geometry helpers:
- `leading_edge(dir)`
- `receiving_edge(dir)`
- `perp_overlap(other, dir)`

### 3.2 Plugin surface
```rust
pub trait IpcAdapter {
    type Config;
    type Error: std::error::Error;
    fn connect(config: &Self::Config) -> Result<Self, Self::Error> where Self: Sized;
    fn ping(&self) -> Result<(), Self::Error>;
}

pub trait TopologyProvider {
    type NativeId: Clone + PartialEq;
    type Error: std::error::Error;
    fn fetch_layout(&self) -> Result<AppLayout<Self::NativeId>, Self::Error>;
}
```

Topology returned by providers must be acyclic and local (tree-shaped AST), not raw global arena IDs.

### 3.3 Modifier and sync-token pattern
Use `_impl` trait for plugin contributors and a blanket wrapper in core that injects sealed `TopologyChanged`.

Contributor-implemented methods:
- `focus_impl`
- `move_impl`
- `tear_off_impl -> Box<dyn PaneState>`
- `merge_in_impl -> NativeId`

Framework wrapper methods return `TopologyChanged` and are `#[must_use]`.

### 3.4 Unified domain trait
```rust
pub trait TilingDomain: IpcAdapter + TopologyProvider + TopologyModifier {
    fn domain_name(&self) -> &'static str;
    fn rect(&self) -> Rect;
    fn supported_payload_types(&self) -> &[TypeId];
}
```

---

## 4) Global runtime model (daemon-owned)

### 4.1 Domain containment
- Maintain `GlobalDomainTree` for domain nesting/ownership only.
- Each node references plugin instance and its domain rectangle.

### 4.2 Flattened global leaf map
- Maintain `Vec<GlobalLeaf>` with:
  - global leaf ID
  - `domain_id`
  - opaque native identifier blob/typed representation
  - `Rect`

### 4.3 Resync rule (strict)
After any call returning `TopologyChanged`, daemon must refresh affected domain layout(s) and rebuild global leaves before next solve step.

---

## 5) Directional focus/move solver spec (authoritative)

Given `(focused_leaf, direction)`:
1. Exclude self.
2. Keep candidates that are directionally valid (`receiving_edge` is beyond focused `leading_edge` in requested direction).
3. Keep only candidates with perpendicular overlap.
4. Select nearest by edge distance.
5. Tie-break by perpendicular offset alignment.
6. If none found: edge-of-screen behavior (no-op or optional wrap policy).

Important conclusion: this solver is domain-agnostic and robust across BSP/grid/master-stack/tabbed mixes.

---

## 6) Orchestration policy for `move <dir>`

1. Run geometry solver on `all_leaves`.
2. If target is in same domain:
   - Route to `focus_impl`/`move_impl` as applicable.
3. If target domain differs:
   - Trigger tear-off/merge flow with payload negotiation.
4. Consume sync tokens and rebuild topology snapshots.

### Cross-domain tear-off / merge flow
1. `source.tear_off_impl(id)` yields `Box<dyn PaneState>`.
2. Registry checks if target domain supports payload type directly.
3. If not direct, run converter path (`TypeId -> TypeId`).
4. `target.merge_in_impl(target_native_id, dir, payload)` creates native pane/window.
5. Resync source + target domains using returned topology-change tokens.

Fallback from discussion: when no conversion exists, spawn generic terminal/shell behavior in target domain.

---

## 7) Adapter-specific implementation spec captured

## 7.1 Emacs adapter
Decisions:
- IPC via `emacsclient --eval`.
- Ask Emacs Lisp to `json-encode` layout/state; parse in Rust with `serde_json`.
- `NativeId` represented by pixel edges `(left, top, right, bottom)` for snapshot identity.
- Re-select window from ID by generated Lisp selector (`window-pixel-edges` match).

Topology/state behaviors:
- Tear-off captures buffer/file/point/window-start first, then deletes window (reject last-window deletion).
- Merge-in splits target window according to direction, restores buffer/file/point as possible.
- Domain rect from frame pixel dimensions.

## 7.2 i3 adapter
Decisions:
- `NativeId` is stable `con_id` (`i64`).
- Tear-off should use `move scratchpad` (preserve process), not kill.
- Handle `stacked`/`tabbed` as non-slicing quirks explicitly during conversion.
- IPC over i3 Unix socket framing protocol.

Known rough edge from discussion:
- Spawned window ID recovery via focus polling is acceptable initial behavior.
- Future replacement target: i3 `SUBSCRIBE` events (`window::new`) for push-based con_id discovery.

---

## 8) Data-source capabilities discovered per tool

- **tmux:** full layout string + per-pane geometry available.
- **Zellij:** full KDL layout dump available.
- **WezTerm:** good pane/tab/window metadata and geometry helpers, but no official full internal tree export.
- **Kitty:** hierarchy + neighbor matching available; full layout internals partially exposed.
- **iTerm2:** hierarchy and size metadata available; no direct BSP tree API.
- **Neovim:** full window tree (`winlayout`) + geometry (`getwininfo`) available.
- **VS Code:** tab groups visible, but no official API for full split-grid topology readback.

Implication: provider implementations must support partial-native-data cases and reconstruct best-effort topology where APIs are incomplete.

---

## 9) Future-work plan (execution-ready)

### Phase A — Core contracts and daemon
1. Implement `SplitAxis`, `Cardinal`, `Rect`, and geometry helper methods.
2. Implement sealed `TopologyChanged` and `_impl` + blanket-wrapper modifier pattern.
3. Implement `PaneState` trait and `PayloadRegistry` (`TypeId` converters).
4. Implement daemon state: domain tree + global flattened leaves + resync pipeline.

### Phase B — Solver and routing
1. Implement geometry solver with strict filtering and tie-break rules.
2. Implement command router:
   - same-domain focus/move
   - cross-domain tear-off/merge
   - edge-of-screen policy path
3. Implement post-mutation resync enforcement.

### Phase C — Initial adapters
1. Emacs adapter per spec above.
2. i3 adapter per spec above.
3. At least one multiplexer/editor adapter pair (Neovim/WezTerm or tmux) to validate cross-domain flows.

### Phase D — Hardening
1. Replace i3 polling with event subscription.
2. Add converter coverage for common payload types.
3. Add behavior for tools with partial topology APIs (notably VS Code/WezTerm constraints).

---

## 10) Acceptance criteria (from conversation intent)

The implementation is considered aligned when:
1. Directional focus works correctly across mixed layout styles without tree-shape assumptions.
2. Cross-domain `move west/east/north/south` performs deterministic tear-off/merge with payload preservation.
3. Compiler prevents silent topology desync (`TopologyChanged` is non-forgeable and must be consumed).
4. Adding a new plugin does not require core enum edits for payloads.
5. Emacs and i3 adapters satisfy their domain-specific constraints above.

---

## 11) Conversation note relevant to execution
- `src/config.rs` was described as recently rebuilt/stable; surrounding runtime call sites should be adapted to current config shape, and surprises should be documented in `AGENTS.md`.
