# Window Manager Isolation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Move WM selection, domain wiring, and WM-only UX behavior behind WM-local specs/capabilities so shared code no longer imports concrete window managers or probes the host to decide which WM to use.

**Architecture:** Introduce a config-driven built-in WM spec registry that returns an object-safe `ConfiguredWindowManager` handle composed of a core WM session plus optional capabilities such as domain factories, window-cycling providers, and WM-specific tear-out composers. Port Niri first to prove the shape, then switch the engine/command call sites to the new handle, remove detector/probing code, and finally delete the legacy enum/match forwarding layer.

**Tech Stack:** Rust, clap, anyhow, serde/serde_json, existing WM adapters, `cargo test`, `cargo build --release`.

---

**Skill refs:** @test-driven-development @verification-before-completion

### Task 1: Introduce the object-safe WM runtime handle

**Files:**
- Modify: `src/adapters/window_managers/mod.rs`
- Test: `src/adapters/window_managers/mod.rs`

**Step 1: Write the failing test**

In `src/adapters/window_managers/mod.rs`, add tests around a fake boxed core session and optional feature bundle:

```rust
#[test]
fn configured_window_manager_delegates_to_object_safe_core() {
    let mut wm = fake_configured_wm();
    assert_eq!(wm.adapter_name(), "fake");
    assert_eq!(wm.focused_window().unwrap().id, 42);
    wm.focus_direction(Direction::West).unwrap();
    assert_eq!(wm.take_calls(), vec!["focus_direction:west"]);
}

#[test]
fn configured_window_manager_exposes_optional_capabilities_independently() {
    let wm = fake_configured_wm_with_cycle_provider();
    assert!(wm.window_cycle().is_some());
    assert!(wm.domain_factory().is_none());
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -q configured_window_manager_delegates_to_object_safe_core`
Expected: FAIL because `ConfiguredWindowManager` / boxed WM session traits do not exist yet.

**Step 3: Write minimal implementation**

In `src/adapters/window_managers/mod.rs`:

- Add an owned `FocusedWindowRecord` snapshot type (parallel to `WindowRecord`).
- Add an object-safe `WindowManagerSession` trait:

```rust
pub trait WindowManagerSession: Send {
    fn adapter_name(&self) -> &'static str;
    fn capabilities(&self) -> WindowManagerCapabilities;
    fn focused_window(&mut self) -> Result<FocusedWindowRecord>;
    fn windows(&mut self) -> Result<Vec<WindowRecord>>;
    fn focus_direction(&mut self, direction: Direction) -> Result<()>;
    fn move_direction(&mut self, direction: Direction) -> Result<()>;
    fn resize_with_intent(&mut self, intent: ResizeIntent) -> Result<()>;
    fn spawn(&mut self, command: Vec<String>) -> Result<()>;
    fn focus_window_by_id(&mut self, id: u64) -> Result<()>;
    fn close_window_by_id(&mut self, id: u64) -> Result<()>;
}
```

- Add optional capability traits and a bundle:

```rust
pub trait WindowManagerDomainFactory: Send { /* create_domain(...) */ }
pub trait WindowCycleProvider: Send { /* focus_or_cycle(...) */ }
pub trait WindowTearOutComposer: Send { /* compose_tear_out(...) */ }

pub struct WindowManagerFeatures {
    pub domain_factory: Option<Box<dyn WindowManagerDomainFactory>>,
    pub window_cycle: Option<Box<dyn WindowCycleProvider>>,
    pub tear_out_composer: Option<Box<dyn WindowTearOutComposer>>,
}
```

- Add `ConfiguredWindowManager { core, features }` plus ergonomic accessors such as `adapter_name()`, `capabilities()`, `focused_window()`, `window_cycle()`, and `domain_factory()`.
- Keep the old GAT-based traits in place for now if that reduces churn, but make the new boxed handle the new target interface for downstream call sites.

**Step 4: Run test to verify it passes**

Run: `cargo test -q configured_window_manager_delegates_to_object_safe_core`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/adapters/window_managers/mod.rs
git commit -m "refactor: add object-safe window manager handle" -m "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

### Task 2: Replace WM probing with a config-driven built-in spec registry

**Files:**
- Modify: `src/config.rs`
- Modify: `src/adapters/window_managers/mod.rs`
- Modify: `src/adapters/window_managers/niri.rs`
- Modify: `src/adapters/window_managers/i3.rs`
- Modify: `src/adapters/window_managers/paneru.rs`
- Modify: `src/adapters/window_managers/yabai.rs`
- Test: `src/config.rs`
- Test: `src/adapters/window_managers/mod.rs`

**Step 1: Write the failing test**

Add config + registry tests:

```rust
#[test]
fn wm_backend_deserializes_any_builtin_name() {
    assert_eq!(
        toml::from_str::<WmConfig>("enabled_integration = \"niri\"").unwrap().enabled_integration,
        WmBackend::Niri
    );
    assert_eq!(
        toml::from_str::<WmConfig>("enabled_integration = \"yabai\"").unwrap().enabled_integration,
        WmBackend::Yabai
    );
}

#[test]
fn connect_selected_reports_configured_backend_failure_without_fallback() {
    let err = connect_backend_for_test(WmBackend::Niri, failing_spec("niri"))
        .unwrap_err();
    assert!(err.to_string().contains("niri"));
    assert!(!err.to_string().contains("i3"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -q connect_selected_reports_configured_backend_failure_without_fallback`
Expected: FAIL because selection still uses detector/probing code and string overrides.

**Step 3: Write minimal implementation**

In `src/config.rs`:

- Make `WmBackend` represent all built-in names directly (do not rely on `#[cfg]`-gated deserialization for platform errors).
- Replace `wm_adapter_override() -> Option<String>` with a typed accessor such as:

```rust
pub fn selected_wm_backend() -> WmBackend {
    read_config().wm.enabled_integration
}
```

- Add a helper like `supported_on_current_platform()` if needed so platform mismatch errors happen in WM connection, not TOML parsing.

In `src/adapters/window_managers/mod.rs`:

- Add a built-in spec trait:

```rust
pub trait WindowManagerSpec: Sync {
    fn backend(&self) -> WmBackend;
    fn name(&self) -> &'static str;
    fn connect(&self) -> Result<ConfiguredWindowManager>;
}
```

- Add `spec_for_backend(backend: WmBackend) -> &'static dyn WindowManagerSpec`.
- Rewrite `connect_selected()` to:
  1. read `selected_wm_backend()`
  2. resolve the matching spec
  3. connect it
  4. surface backend-specific errors directly
- Delete `detect_*`, `connect_*`, `REGISTRY`, priority selection, and normalization code tied to string overrides.

In each WM file:

- Export one built-in spec (`NiriSpec`, `I3Spec`, `PaneruSpec`, `YabaiSpec`) that knows how to build the boxed core session and optional features for that WM.

**Step 4: Run test to verify it passes**

Run:
- `cargo test -q wm_backend_deserializes_any_builtin_name`
- `cargo test -q connect_selected_reports_configured_backend_failure_without_fallback`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/config.rs src/adapters/window_managers/mod.rs src/adapters/window_managers/niri.rs src/adapters/window_managers/i3.rs src/adapters/window_managers/paneru.rs src/adapters/window_managers/yabai.rs
git commit -m "refactor: make wm selection config-driven" -m "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

### Task 3: Move WM domain wiring behind an optional domain factory

**Files:**
- Modify: `src/engine/domain.rs`
- Modify: `src/adapters/window_managers/mod.rs`
- Modify: `src/adapters/window_managers/niri.rs`
- Test: `src/engine/domain.rs`

**Step 1: Write the failing test**

In `src/engine/domain.rs`, add tests driven by fake configured WMs:

```rust
#[test]
fn runtime_domains_uses_wm_domain_factory_when_present() {
    let mut wm = fake_wm_with_domain_factory(fake_domain("wm-test"));
    let domains = runtime_domains_for_window_manager(&mut wm).unwrap();
    assert!(domains.iter().any(|domain| domain.domain_name() == "wm-test"));
}

#[test]
fn runtime_domains_uses_generic_unsupported_domain_when_factory_absent() {
    let mut wm = fake_wm_without_domain_factory("fake");
    let domains = runtime_domains_for_window_manager(&mut wm).unwrap();
    assert!(domains.iter().any(|domain| domain.domain_name() == "fake"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -q runtime_domains_uses_wm_domain_factory_when_present`
Expected: FAIL because `runtime_domains_for_window_manager` still depends on `WindowManagerAdapter` and hardcodes `"niri"`.

**Step 3: Write minimal implementation**

In `src/adapters/window_managers/mod.rs`:

- Flesh out `WindowManagerDomainFactory` with a method like:

```rust
fn create_domain(&self, domain_id: DomainId) -> Result<Box<dyn ErasedDomain>>;
```

In `src/adapters/window_managers/niri.rs`:

- Keep `NiriDomainPlugin` in `niri.rs`.
- Wrap it in a Niri-owned domain factory object exposed by `NiriSpec`.

In `src/engine/domain.rs`:

- Change `runtime_domains_for_window_manager` to accept the new `ConfiguredWindowManager`.
- Replace:

```rust
match wm.adapter_name() {
    "niri" => { ... }
    other => { ... }
}
```

with:

```rust
if let Some(factory) = wm.domain_factory() {
    domains.push(factory.create_domain(WM_DOMAIN_ID)?);
} else {
    domains.push(Box::new(UnsupportedDomainPlugin::new(WM_DOMAIN_ID, wm.adapter_name())));
}
```

- Keep the rest of the app-domain resolution flow unchanged.

**Step 4: Run test to verify it passes**

Run:
- `cargo test -q runtime_domains_uses_wm_domain_factory_when_present`
- `cargo test -q runtime_domains_uses_generic_unsupported_domain_when_factory_absent`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/engine/domain.rs src/adapters/window_managers/mod.rs src/adapters/window_managers/niri.rs
git commit -m "refactor: route wm domains through optional factories" -m "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

### Task 4: Move `focus-or-cycle` behind a generic WM capability

**Files:**
- Modify: `src/adapters/window_managers/mod.rs`
- Modify: `src/adapters/window_managers/niri.rs`
- Modify: `src/commands/focus_or_cycle.rs`
- Modify: `src/main.rs`
- Test: `src/commands/focus_or_cycle.rs`

**Step 1: Write the failing test**

In `src/commands/focus_or_cycle.rs`, extract a WM-injectable execution helper and test it:

```rust
#[test]
fn focus_or_cycle_dispatches_through_window_cycle_provider() {
    let mut wm = fake_wm_with_cycle_provider();
    run_with_window_manager(sample_request(), &mut wm).unwrap();
    assert_eq!(wm.take_cycle_calls(), vec![sample_request()]);
}

#[test]
fn focus_or_cycle_returns_clear_error_when_capability_is_missing() {
    let mut wm = fake_wm_without_cycle_provider();
    let err = run_with_window_manager(sample_request(), &mut wm).unwrap_err();
    assert!(err.to_string().contains("does not support focus-or-cycle"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -q focus_or_cycle_dispatches_through_window_cycle_provider`
Expected: FAIL because the command still imports `niri::Niri` directly.

**Step 3: Write minimal implementation**

In `src/adapters/window_managers/mod.rs`:

- Add a shared request model so the capability trait does not depend on the command module:

```rust
pub struct WindowCycleRequest {
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub spawn: Option<String>,
    pub new: bool,
    pub summon: bool,
}
```

- Define:

```rust
pub trait WindowCycleProvider: Send {
    fn focus_or_cycle(&mut self, request: &WindowCycleRequest) -> Result<()>;
}
```

In `src/commands/focus_or_cycle.rs`:

- Convert `FocusOrCycleArgs` into `WindowCycleRequest`.
- Replace `Niri::connect()` and all direct Niri calls with `connect_selected()` plus a capability lookup on the configured WM.
- Surface unsupported-capability errors directly.

In `src/adapters/window_managers/niri.rs`:

- Move the current summon/workspace/monitor implementation behind a Niri-owned `WindowCycleProvider`.
- Keep state-file handling in `niri.rs`, not in shared command code.

In `src/main.rs`:

- Remove Niri-specific CLI text such as `(Linux/Niri only)` and the top-level `"for niri"` description.

**Step 4: Run test to verify it passes**

Run:
- `cargo test -q focus_or_cycle_dispatches_through_window_cycle_provider`
- `cargo test -q focus_or_cycle_returns_clear_error_when_capability_is_missing`

Expected: PASS.

**Step 5: Commit**

```bash
git add src/adapters/window_managers/mod.rs src/adapters/window_managers/niri.rs src/commands/focus_or_cycle.rs src/main.rs
git commit -m "refactor: route focus-or-cycle through wm capability" -m "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

### Task 5: Switch engine + command call sites to the new handle and remove legacy WM-only shared primitives

**Files:**
- Modify: `src/adapters/window_managers/mod.rs`
- Modify: `src/engine/orchestrator.rs`
- Modify: `src/commands/mod.rs`
- Modify: `src/commands/resize.rs`
- Modify: `src/engine/domain.rs`
- Test: `src/engine/orchestrator.rs`
- Test: `src/adapters/window_managers/mod.rs`

**Step 1: Write the failing test**

Rewrite the orchestrator fake WM test double to target `ConfiguredWindowManager`, then add targeted tests for the remaining architectural seams:

```rust
#[test]
fn composed_tearout_routes_through_wm_specific_composer() {
    let mut wm = fake_wm_with_tearout_composer();
    let mut orchestrator = Orchestrator::default();
    orchestrator.place_tearout_window(&mut wm, Direction::North, 3).unwrap();
    assert_eq!(wm.take_composer_calls(), vec![("north", 3)]);
}

#[test]
fn orchestrator_uses_object_safe_wm_core_snapshots() {
    let mut wm = fake_configured_wm();
    let mut orchestrator = Orchestrator::default();
    // exercise one existing focus/move path with the boxed WM handle
    orchestrator.execute(&mut wm, ActionRequest::new(ActionKind::Focus, Direction::East)).unwrap();
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -q composed_tearout_routes_through_wm_specific_composer`
Expected: FAIL because the orchestrator and command entry points still require the legacy trait stack and shared `move_column` / `consume_into_column_and_move` methods.

**Step 3: Write minimal implementation**

In `src/engine/orchestrator.rs`:

- Replace generic `W: WindowManagerAdapter` call sites with `ConfiguredWindowManager`.
- Replace `with_focused_window(...)` calls with `focused_window()`.
- When `plan_tear_out(...)` returns `CapabilitySupport::Composed`, dispatch through the optional `WindowTearOutComposer` capability instead of calling public Niri-shaped primitives on the shared core trait.

In `src/commands/mod.rs` and `src/commands/resize.rs`:

- Update command entry points to pass `ConfiguredWindowManager` through to the orchestrator/domain layer.

In `src/adapters/window_managers/mod.rs`:

- Delete the legacy runtime path once all call sites compile:
  - `FocusedWindowView`
  - `WindowManagerIntrospection`
  - `WindowManagerExecution`
  - `WindowManagerAdapter`
  - `SelectedWindowManager`
  - `SelectedFocusedWindow`
  - shared `move_column` / `consume_into_column_and_move` requirements

- Keep shared capability planning, `WindowRecord`, and the new object-safe types.

**Step 4: Run test to verify it passes**

Run:
- `cargo test -q composed_tearout_routes_through_wm_specific_composer`
- `cargo test -q orchestrator_uses_object_safe_wm_core_snapshots`
- `cargo test -q place_tearout_window_moves_column_for_composed_west`

Expected: PASS, with composed tear-out behavior now routed through WM-local helpers instead of shared Niri-shaped primitives.

**Step 5: Commit**

```bash
git add src/adapters/window_managers/mod.rs src/engine/orchestrator.rs src/commands/mod.rs src/commands/resize.rs src/engine/domain.rs
git commit -m "refactor: switch engine to configured wm handle" -m "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

---

### Task 6: Update architecture docs and run full verification

**Files:**
- Modify: `ARCHITECTURE.md`
- Modify: `src/main.rs`

**Step 1: Update the architecture docs**

In `ARCHITECTURE.md`, update the WM section and “How to add a window manager” instructions so they describe:

- config-driven built-in WM selection
- per-WM spec objects
- optional WM capabilities (`domain_factory`, `window_cycle`, `tear_out_composer`)
- no runtime WM probing/detection

Replace guidance like “add detector/connect code in `mod.rs`” with “export a spec from `src/adapters/window_managers/<name>.rs` and add it to the built-in backend mapping.”

**Step 2: Run the targeted WM tests**

Run:

```bash
cargo test -q configured_window_manager_delegates_to_object_safe_core && \
cargo test -q connect_selected_reports_configured_backend_failure_without_fallback && \
cargo test -q runtime_domains_uses_wm_domain_factory_when_present && \
cargo test -q focus_or_cycle_dispatches_through_window_cycle_provider && \
cargo test -q composed_tearout_routes_through_wm_specific_composer
```

Expected: PASS.

**Step 3: Run the full test suite**

Run: `cargo test -q`
Expected: PASS.

**Step 4: Build the release binary**

Run: `cargo build --release`
Expected: PASS.

**Step 5: Commit**

```bash
git add ARCHITECTURE.md src/main.rs
git commit -m "docs: describe capability-driven wm architecture" -m "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```
