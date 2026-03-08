# yeet-and-yoink Architecture Guide

## High-Level Architecture

### Key Design Patterns

#### 1. App-First Routing

The orchestrator always attempts **in-app operations first** before falling back to WM-level operations. This ensures:
- Terminal multiplexers handle their own pane navigation
- Editors manage their own buffer focus
- Only edge transitions (pane at window edge) cross into WM space

Example flow for `focus west`:
1. Is focused pane a terminal/editor with internal panes? → Try internal focus
2. Is pane at left edge? → Fall back to WM focus
3. WM finds window to the west → Focus it

#### 2. Domain-Based Architecture

Each component exists in a **domain** with clear boundaries:
- **App domains**: neovim, emacs, vscode, wezterm, kitty, etc.
- **Mux domains**: tmux, zellij, wezterm mux, kitty mux
- **WM domains**: niri, i3

Domains declare their capabilities (focus, move, resize, tear_out, merge) and the orchestrator negotiates cross-domain operations.

#### 3. Transfer Pipeline

Cross-domain moves (e.g., moving a neovim buffer to a new window) use a transfer pipeline:
1. **Prepare**: Capture source identity (pane ID, buffer path, etc.)
2. **Move Out**: Tear out from source domain (creates new window)
3. **Placement**: WM positions the new window directionally
4. **Merge**: (Optional) Merge back into target domain

#### 4. Geometry-First Navigation

Navigation uses **screen-space rectangles** rather than tree traversal:
- All visible leaves are flattened into a list with (x, y, w, h)
- Neighbor selection uses directional validity + perpendicular overlap
- Works uniformly across BSP, grid, master-stack, and columnar layouts

## Core Components Deep Dive

### 1. Orchestrator (`src/engine/orchestrator.rs`)

The orchestrator is the brain of the system. It:
- Builds topology snapshots from all domains
- Determines operation routing (internal vs cross-domain)
- Executes action plans
- Handles post-action composition (cleanup, focus management)

Key methods:
- `execute_focus()`: Route focus with app-first semantics
- `execute_move()`: Handle internal moves, tear-outs, and merges
- `execute_resize()`: Prefer in-app resize before WM fallback

### 2. Topology (`src/engine/topology.rs`)

Geometry solver that:
- Maintains screen-space rectangles for all leaves
- Computes directional neighbors using overlap + distance heuristics
- Supports cardinal directions (N/S/E/W) and split axes (Horizontal/Vertical)

Key insight: The solver treats all leaves uniformly regardless of domain, enabling consistent navigation across nested tiling systems.

### 3. Domain Contracts (`src/engine/domain.rs`)

Defines the interface all adapters implement:

```rust
trait AppAdapter {
    fn adapter_name(&self) -> &str;
    fn config_aliases(&self) -> &[&str];
    fn kind(&self) -> AppKind;
    fn capabilities(&self) -> Capabilities;
    
    // Core operations
    fn focus(&self, direction: Direction) -> Result<()>;
    fn move_pane(&self, direction: Direction) -> Result<MoveResult>;
    fn resize(&self, direction: Direction, delta: i32) -> Result<()>;
    fn tear_out(&self, scope: TearOffScope) -> Result<TearOutResult>;
    fn merge_into(&self, target: &dyn AppAdapter, prep: MergePreparation) -> Result<()>;
}
```

### 4. Configuration (`src/config.rs`)

Hierarchical TOML-based configuration:

```toml
[wm]
enabled_integration = "niri"

[app.terminal.wezterm]
enabled = true
mux_backend = "wezterm"
focus.internal_panes.enabled = true
move.internal_panes.enabled = true
move.docking.tear_off.enabled = true

[app.editor.emacs]
enabled = true
tear_off_scope = "buffer"
```

Config discovery order:
1. `NIRI_DEEP_CONFIG` environment variable (explicit path)
2. Platform config dir (`$XDG_CONFIG_HOME/yeet-and-yoink/config.toml`)
3. Defaults (all integrations disabled)

### 5. Chain Resolver (`src/engine/chain_resolver.rs`)

Responsible for building the **adapter chain** for a given window:
- Detects if window is a terminal host → Assembles terminal + mux adapters
- Detects editors directly
- Applies config overrides

Example chains:
- WezTerm window → `[wezterm_backend, wezterm_mux]`
- Foot + tmux → `[foot_backend, tmux_mux]`
- Emacs → `[emacs_backend]`

## Adapter Architecture

### Terminal Hosts (`src/adapters/apps/`)

Terminal adapters handle:
1. **Process detection**: Identify terminal emulator by PID/app_id
2. **Mux delegation**: Route mux operations to configured backend
3. **Launch prefix**: Prepare command prefix for spawning new terminals

Supported terminals:
- **wezterm**: Native mux support, rich CLI
- **kitty**: Native remote control (requires socket config)
- **foot, alacritty, ghostty, iTerm2**: External mux only (tmux/zellij)

### Terminal Multiplexers (`src/adapters/terminal_multiplexers/`)

Mux adapters implement pane operations:
- **tmux**: Uses CLI (`tmux list-panes`, `swap-pane`, `break-pane`, `join-pane`)
- **zellij**: Uses CLI + plugin API (`dump-layout`, pipe-triggered break-pane plugin)
- **wezterm mux**: Uses CLI (`wezterm cli list`, `move-pane-to-new-tab`)
- **kitty mux**: Uses remote control (`kitty @ ls`, `detach-window`)

Key mux operations:
- Internal pane movement (swap-pane)
- Tear-out (break-pane → new window)
- Merge (join-pane → target window)

### Window Managers (`src/adapters/window_managers/`)

WM adapters handle:
- Window listing with geometry
- Directional window focus
- Window movement/placement
- Resize operations

Supported WMs:
- **niri**: IPC via `niri-ipc` crate
- **i3**: IPC via Unix socket

## Data Flow Examples

### Example 1: Focus West (Internal)

```
User: yny focus west
│
├─ CLI parses → commands::focus::run(West)
│
├─ Orchestrator builds topology
│  ├─ WM snapshot: [Window A, Window B, ...]
│  ├─ Focused window = Window A (WezTerm)
│  └─ Chain resolver: [wezterm_backend, wezterm_mux]
│
├─ Orchestrator checks app-first
│  ├─ Wezterm adapter: focus(West)
│  ├─ Wezterm mux: Has neighbor? Yes
│  └─ Executes: select-pane -L
│
└─ Result: Focus moves within terminal, WM not involved
```

### Example 2: Move West (Tear-Out)

```
User: yny move west
│
├─ Orchestrator builds topology
│  ├─ Focused window = Window A (WezTerm)
│  └─ Focused pane is at LEFT EDGE
│
├─ Decision: Edge move → TearOut
│
├─ Execute tear-out
│  ├─ wezterm_mux.move_out(PaneScope)
│  ├─ Spawns: wezterm cli move-pane-to-new-tab --new-window
│  └─ Returns new window ID
│
├─ WM post-placement
│  ├─ Poll for new WM window
│  ├─ Focus new window
│  └─ Apply directional placement (West)
│
└─ Result: Pane becomes new window to the west
```

### Example 3: Merge Back

```
User: yny move west (from torn-out window)
│
├─ Orchestrator detects same-domain target
│  ├─ Source: wezterm window (pane X)
│  └─ Target: wezterm window (pane Y)
│
├─ Prepare merge
│  ├─ Capture source pane ID
│  └─ Identify target pane
│
├─ Execute merge
│  ├─ wezterm_mux.merge_into(target, prep)
│  ├─ join-pane -s <source> -t <target>
│  └─ Kill source WM window
│
└─ Result: Pane merged back, empty window closed
```

## Key Capabilities

Adapters declare capabilities that control orchestrator behavior:

### FocusCapability
- `internal_panes`: Can focus within app (neovim windows, tmux panes)
- `allowed_directions`: Which directions are supported internally

### MoveCapability
- `internal_panes`: Can move panes within app
- `tear_out`: Can pop out panes to new windows
- `snap_back`: Can merge panes back

### ResizeCapability
- `internal_panes`: Can resize internal splits

## Configuration-Driven Behavior

The system is heavily configuration-driven:

### Policies
- **PanePolicy**: Per-app focus/move/resize settings
- **MuxPolicy**: Which mux backend to use for terminals
- **TearOffStrategy**: When to allow tear-out (edgemost, always, etc.)

### Override Points
- `app_adapter_override()`: Pin to specific app
- `wm_adapter_override()`: Pin to specific WM
- `NIRI_DEEP_CONFIG`: Explicit config file path

## Error Handling Philosophy

1. **Fail fast on config errors**: Explicit config path must exist
2. **Graceful degradation**: If app adapter fails, fall back to WM
3. **Explicit errors over silent failures**: No-op with error message beats silent failure
4. **Retry for async operations**: WM window creation may need polling

## Testing Strategy

### Unit Tests
- Config parsing (src/config.rs)
- Topology calculations (src/engine/topology.rs)
- Chain resolution (src/engine/chain_resolver.rs)

### Integration Tests
- Adapter capability tests (with explicit config)
- Orchestrator routing tests
- End-to-end flow tests

### Test Isolation
- Set `NIRI_DEEP_CONFIG` explicitly per test
- Use `--test-threads=1` if tests flap (shared config state)

## Common Pitfalls (from AGENTS.md)

1. **App adapters not invoked**: Check orchestrator routing logic
2. **Same-domain merge skipped**: Verify `execute_move` same-domain branch
3. **Tear-out placement ignored**: Ensure WM `plan_tear_out` is called after `move_out`
4. **Focus not switching after tear-out**: Poll for new window, then focus
5. **Stale WM snapshots**: Retry loop around `wm.windows()` needed
6. **Config-sensitive tests**: Always pin config in capability tests

## Adding New Adapters

### Terminal Host
1. Create `src/adapters/apps/<name>.rs`
2. Implement `AppAdapter` trait
3. Add to chain resolver detection
4. Add config section in `src/config.rs`

### Mux Provider
1. Create `src/adapters/terminal_multiplexers/<name>.rs`
2. Implement `TerminalMultiplexerProvider` trait
3. Add to mux policy resolution

### Window Manager
1. Create `src/adapters/window_managers/<name>.rs`
2. Implement `WindowManager` trait
3. Add to `WmBackend` enum in `src/config.rs`

## Dependencies

Key external crates:
- `niri-ipc`: Niri WM IPC
- `clap`: CLI parsing
- `serde` + `toml`: Configuration
- `tracing`: Structured logging
- `etcetera`: Config directory resolution
- `tungstenite`: WebSocket (VS Code bridge)

## Build & Run

```bash
# Development
cargo build
cargo test

# Release (important for testing actual behavior)
cargo build --release

# Run with config
NIRI_DEEP_CONFIG=./my-config.toml cargo run -- focus west

# With logging
yny --log-file /tmp/yny.log focus west
```

## Architecture Evolution

This codebase represents a rewrite from an earlier planner/executor model to the current orchestrator-based architecture. Key changes:

- **Removed**: Legacy planner/context/executor pipelines
- **Added**: Unified orchestrator with geometry solver
- **Added**: Transfer pipeline for cross-domain moves
- **Added**: Chain resolver for adapter assembly
- **Retained**: Adapter implementations (with contract updates)

The new architecture prioritizes:
1. Deterministic routing via topology queries
2. Open plugin system (no closed enums)
3. Type-safe mutation contracts
4. Unified handling of all tiling layouts

## Further Reading

- `README.md`: Quick start and basic config
- `AGENTS.md`: Detailed debugging notes and surprises
- `apps_topology.md`: How apps expose their layout
- `docs/plans/`: Design documents for major features
- `openspec/changes/`: Architecture evolution specs
