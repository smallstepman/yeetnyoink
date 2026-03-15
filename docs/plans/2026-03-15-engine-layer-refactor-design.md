# Engine Layer Refactor Design

**Date:** 2026-03-15
**Status:** Approved for implementation

## Problem

`src/engine/` absorbed all orchestration complexity while `src/adapters/` stayed clean. The
result is five large files where unrelated concerns are tangled together:

| File | Lines | Problems |
|---|---|---|
| `orchestrator.rs` | 2102 | God class: focus+move+resize+tearout+merge+WM-probing+domain-routing all in one struct |
| `domain.rs` | 1264 | Generic transfer machinery mixed with concrete WM/app bridge types |
| `contract.rs` | 942 | Five unrelated concerns: adapter identity, topology handler, mux provider, merge types, chain resolver |
| `window_manager.rs` | 922 | Fine but isolated — included in refactor for consistency |
| `chain_resolver.rs` | 847 | Reasonable but `domain_id_for_app_kind` duplicated here and in `domain.rs` |
| `app_policy.rs` | 318 | ~200 lines of identical delegation boilerplate for 11+ pass-through methods |

Additionally, three `adapters/*/mod.rs` files contain processing logic and type definitions
that belong in the engine:

| File | Items to move |
|---|---|
| `adapters/terminal_multiplexers/mod.rs` | `spawn_attach_command`, `prepend_terminal_launch_prefix` — command-composition functions that operate on `TearResult` (an engine type) |
| `adapters/apps/mod.rs` | `DirectAdapterSpec`, `TerminalHostSpec` struct definitions — shapes that only `engine::chain_resolver` consumes; the catalog constants stay in adapters |
| `adapters/window_managers/mod.rs` | `UnsupportedWindowManagerSpec` + platform statics — a null impl of an engine trait that belongs alongside the trait definition |

Structural bugs:
- `domain_id_for_app_kind` is a private copy in both `chain_resolver.rs` and `domain.rs`
- A blanket `impl ChainResolver for T where T: TopologyHandler` silently adds chain resolution
  methods to every adapter type; no adapter actually uses them (orchestrator calls the free
  functions directly), so this creates unexpected API surface noise
- `ConfiguredWindowManager` implements `WindowManagerSession` with 10 methods that all
  delegate verbatim to `self.core` — pure boilerplate

## Approach: Full Layer Refactor

Re-organize `src/engine/` around six clear responsibilities. No adapter code changes.
The module boundaries map directly to the dependency graph: each layer only imports from
layers listed above it.

```
topology.rs      (geometry primitives — no deps)
runtime/         (process introspection — no deps)
contracts/       (pure trait definitions — imports topology, runtime)
resolution/      (adapter discovery + policy — imports contracts)
transfer/        (payload pipeline + domain bridge — imports contracts, resolution)
wm/              (WM session + capabilities — imports topology, contracts)
actions/         (orchestration — imports all)
```

## New Module Structure

```
src/engine/
├── mod.rs
├── topology.rs              unchanged
├── runtime/                 unchanged
│
├── contracts/
│   ├── mod.rs               re-exports
│   ├── adapter.rs           AppAdapter, AppKind, AppCapabilities, AdapterCapabilities
│   ├── topology.rs          TopologyHandler, TopologySnapshot, MoveDecision, TearResult
│   ├── mux.rs               TerminalMultiplexerProvider, TerminalPaneSnapshot
│   └── merge.rs             MergePreparation, SourcePaneMerge, MergeExecutionMode
│
├── resolution/
│   ├── mod.rs               free fns: resolve_chain, default_adapters, domain_id_for_window
│   ├── chain.rs             RuntimeChainResolver + terminal chain resolution logic
│   ├── domain.rs            domain_id_for_app_kind (canonical, deduplicated)
│   └── policy.rs            PolicyBoundApp, bind_app_policy, delegate_to_inner! macro
│
├── transfer/
│   ├── mod.rs               re-exports
│   ├── registry.rs          PayloadRegistry, PaneState, TransferError
│   ├── pipeline.rs          TransferPipeline, TransferOutcome
│   └── bridge.rs            ErasedDomain, DomainSnapshot, DomainLeafSnapshot,
│                            AppDomainPlugin, UnsupportedDomainPlugin,
│                            NativeWindowRef, encode/decode_native_window_ref,
│                            AppMergePayload, runtime_domains_for_window_manager
│
├── wm/
│   ├── mod.rs               re-exports
│   ├── capabilities.rs      WindowManagerCapabilities, DirectionalCapability,
│                            PrimitiveWindowManagerCapabilities, CapabilitySupport,
│                            plan_tear_out, plan_resize, validate_declared_capabilities
│   ├── session.rs           WindowManagerSession, FocusedWindowRecord, WindowRecord,
│                            ResizeIntent, ResizeKind, WindowManagerDomainFactory,
│                            WindowCycleRequest, WindowCycleProvider, WindowTearOutComposer
│   └── configured.rs        ConfiguredWindowManager, WindowManagerFeatures,
│                            WindowManagerSpec, connect_selected_window_manager
│
└── actions/
    ├── mod.rs               Orchestrator, ActionRequest, ActionKind
    ├── context.rs           AppContext, ChainWalker
    ├── focus.rs             FocusAction
    ├── move.rs              MoveAction
    ├── resize.rs            ResizeAction
    ├── tearout.rs           TearOutExecutor
    ├── merge.rs             MergeExecutor
    └── probe.rs             WmProber (free functions)
```

## Key Design Decisions

### 1. Remove the ChainResolver blanket impl

`contract.rs` currently has:

```rust
impl<T> ChainResolver for T where T: TopologyHandler + ?Sized { ... }
```

This silently adds chain resolution methods to every adapter type, but no adapter calls
`self.resolve_chain(...)` on itself — only the orchestrator and chain resolver call the
free functions directly. The blanket impl is dead weight that adds unexpected API surface
to adapter types.

**Fix:** Remove `ChainResolver` as a supertrait of `AppAdapter`. Remove the blanket impl.
`AppAdapter` becomes: `pub trait AppAdapter: Send + TopologyHandler`.
Chain resolution uses the free functions in `resolution/mod.rs` directly.

### 2. AppContext and ChainWalker (eliminate repeated preamble)

Three orchestrator methods share an identical 8-line preamble + loop pattern:

```rust
// Repeated in attempt_focused_app_focus, attempt_focused_app_move, attempt_focused_app_resize:
let focused = wm.focused_window()?;
let source_window_id = focused.id;
let source_tile_index = focused.original_tile_index;
let app_id = focused.app_id.unwrap_or_default();
let title = focused.title.unwrap_or_default();
let source_pid = focused.pid;
let owner_pid = source_pid.map(ProcessId::get);
let Some(owner_pid) = owner_pid else { return Ok(false); };
for app in resolve_app_chain(&app_id, owner_pid, &title) { ... }
```

**Fix:**

```rust
/// Captures the focused window's context once. Returns None if no pid (app handlers need a pid).
pub struct AppContext {
    pub source_window_id: u64,
    pub source_tile_index: usize,
    pub source_pid: Option<ProcessId>,
    pub owner_pid: u32,
    pub app_id: String,
    pub title: String,
}

impl AppContext {
    pub fn from_focused(wm: &mut ConfiguredWindowManager) -> Result<Option<Self>>;
    pub fn resolve_chain(&self) -> Vec<Box<dyn AppAdapter>>;
}

/// Walks a resolved adapter chain and calls a visitor for each adapter that passes a
/// capability predicate. Returns the first Some(T) result.
pub fn walk_chain<T>(
    chain: &[Box<dyn AppAdapter>],
    mut visitor: impl FnMut(usize, &dyn AppAdapter) -> Result<Option<T>>,
) -> Result<Option<T>>;
```

Each action handler (focus, move, resize) creates an `AppContext` once and calls its
specific logic, eliminating the repeated preamble.

### 3. WmProber (extract probe methods from Orchestrator)

The four probe methods (`probe_directional_target`, `probe_directional_target_for_adapter`,
`probe_in_place_target_for_adapter`, `restore_in_place_target_focus`) are self-contained
helpers that take `wm` as an argument and don't use `&self` from `Orchestrator`. Extracting
them as free functions in `actions/probe.rs` reduces `Orchestrator` to pure orchestration.

### 4. Deduplicate domain_id_for_app_kind

A private copy exists in `chain_resolver.rs` (line 365) and `domain.rs` (line 329).
Both do the same mapping. The canonical copy lives in `resolution/domain.rs`.

### 5. delegate_to_inner! macro in policy.rs

`PolicyBoundApp` has 9 methods with real policy logic and ~11 methods that are verbatim:

```rust
fn merge_into(&self, dir, source_pid) -> Result<()> {
    TopologyHandler::merge_into(self.inner.as_ref(), dir, source_pid)
}
// (repeated for at_side, window_count, merge_execution_mode, prepare_merge,
//  augment_merge_preparation_for_target, merge_into_target, adapter_name,
//  config_aliases, kind, eval)
```

A `macro_rules! delegate_to_inner!` generates these from method signatures, reducing the
boilerplate to a single block. The 9 policy-gate methods remain explicit.

### 6. TearOutExecutor and MergeExecutor as separate structs

The tear-out lifecycle (`execute_app_tear_out` + 3 sub-methods) and merge lifecycle
(`attempt_passthrough_merge` + `cleanup_merged_source_window`) each form a coherent unit
that currently lives as methods on `Orchestrator`. Extracting them to `TearOutExecutor`
and `MergeExecutor` structs in `tearout.rs`/`merge.rs` makes each self-contained and
directly testable without constructing a full `Orchestrator`.

### 7. Action handler structs

Each of the three top-level actions becomes a struct in its own file:

```rust
pub struct FocusAction<'a> {
    orchestrator: &'a Orchestrator,
}
impl FocusAction<'_> {
    pub fn execute(wm: &mut ConfiguredWindowManager, dir: Direction) -> Result<()>;
}
```

`Orchestrator::execute_*` delegates to these, keeping `actions/mod.rs` as a thin
dispatcher with minimal state.

### 8. Adapter mod.rs items relocating to engine

**`prepend_terminal_launch_prefix` + `spawn_attach_command` → `engine/resolution/command.rs`**

Both are stateless command-composition helpers that build `Vec<String>` from a terminal
launch prefix plus mux-provided arguments. `prepend_terminal_launch_prefix` takes a
`TearResult` (an engine type) and mutates its `spawn_command` field — it should live
alongside `TearResult`. `spawn_attach_command` is the inverse: it assembles an attach
command for torn-out windows. Both move to `engine/resolution/command.rs`. The adapters
module re-exports them for the adapter-side callers that currently import them directly.

**`DirectAdapterSpec` + `TerminalHostSpec` struct definitions → `engine/resolution/catalog.rs`**

These structs define the shape of adapter catalog entries that `engine::chain_resolver`
consumes. Moving the *struct definitions* to `engine/resolution/catalog.rs` makes the
dependency point correctly: the engine owns the catalog schema, and `adapters/apps/mod.rs`
just populates `TERMINAL_HOSTS` and `DIRECT_ADAPTERS` with concrete values. The constants
themselves stay in `adapters/apps/mod.rs` since they reference adapter-specific constants
(`APP_IDS`, `ADAPTER_ALIASES`, etc.).

**`UnsupportedWindowManagerSpec` + platform statics → `engine/wm/configured.rs`**

This null implementation of `WindowManagerSpec` only exists to satisfy cross-platform
`spec_for_backend` dispatch. Since `WindowManagerSpec` is an engine trait defined in
`engine/wm/`, its null stub belongs there. The `spec_for_backend` function in
`adapters/window_managers/mod.rs` is pure enum-branch dispatch and stays as-is, but
imports `UnsupportedWindowManagerSpec` from the engine.

## What Does NOT Change

- Adapter *implementation* files (`adapters/**/*.rs` excluding the three mod.rs items above)
- All tests — only import paths change, behavior is identical
- The `Orchestrator` public API (`execute`, `execute_focus`, `execute_move`, `execute_resize`,
  `register_domain`, `route`) — same signatures, re-exported from `actions/mod.rs`
- `src/commands/`, `src/config.rs`, `src/main.rs` — import paths update, no logic changes

## Migration Notes

All existing public types are re-exported from `src/engine/mod.rs` to maintain backward
compatibility with `src/commands/` callers. The refactor is purely internal to `src/engine/`.

Existing re-export aliases to maintain:
- `engine::orchestrator::*` → `engine::actions::*`
- `engine::contracts::*` → split across `engine::contracts::*`
- `engine::domain::*` → split across `engine::transfer::*`
- `engine::window_manager::*` → `engine::wm::*`
- `engine::chain_resolver::*` → `engine::resolution::*`
- `engine::app_policy::*` → `engine::resolution::policy::*`
