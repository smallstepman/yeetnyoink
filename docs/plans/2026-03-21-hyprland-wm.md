# Hyprland WM Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add first-cut Hyprland window-manager support so `yeetnyoink` can use `hyprctl` for WM-level `focus`, `move`, `resize`, window enumeration, spawn, focus-by-id, and close-by-id when `[wm].enabled_integration = "hyprland"`.

**Architecture:** Add a Linux-only `HyprlandAdapter` that implements the existing `WindowManagerSession` contract through `hyprctl -j activewindow`, `hyprctl -j clients`, and `hyprctl dispatch ...`. Keep the capability shape conservative and i3-like—native focus/move/resize only, no domain factory, no WM tear-out composer—and wire it through the current config/spec registry before updating docs and smoke-testing it against a live Hyprland session.

**Tech Stack:** Rust, `serde`, `anyhow`, existing `engine::runtime` command helpers, `hyprctl`, `cargo test`, `cargo build --release --target-dir target -q`

---

## File map

- Create: `src/adapters/window_managers/hyprland.rs`
  - Owns Hyprland JSON payload structs, address parsing/formatting helpers, direction/resize command helpers, `HyprlandAdapter`, `HyprlandSpec`, and focused unit tests.
- Modify: `src/adapters/window_managers/mod.rs`
  - Registers the Linux-only Hyprland module and routes `WmBackend::Hyprland` through `spec_for_backend(...)`.
- Modify: `src/config.rs`
  - Adds `WmBackend::Hyprland`, string/platform helpers, and backend parsing coverage.
- Modify: `src/engine/wm/configured.rs`
  - Adds `UNSUPPORTED_HYPRLAND_SPEC` for non-Linux builds.
- Modify: `src/engine/wm/mod.rs`
  - Extends built-in connector/capability tests to include Hyprland.
- Modify: `flake.nix`
  - Adds `hyprland` to the typed Home Manager `wm.enabled_integration` enum.
- Modify: `README.md`
  - Adds `hyprland` to Linux WM docs/examples and fixes the stale auto-detection wording in the same section.
- Modify: `config.example.toml`
  - Adds `hyprland` to the documented backend list.

## Task 1: Build Hyprland helper foundations

**Files:**
- Create: `src/adapters/window_managers/hyprland.rs`
- Modify: `src/adapters/window_managers/mod.rs`
- Test: `src/adapters/window_managers/hyprland.rs`

- [ ] **Step 1: Add the Linux-only module declaration and write failing helper tests**

```rust
#[test]
fn hyprland_parses_window_address_hex() {
    assert_eq!(parse_window_address("0x2a").unwrap(), 0x2a);
}

#[test]
fn hyprland_formats_window_selector() {
    assert_eq!(format_window_selector(0x2a), "address:0x2a");
}

#[test]
fn hyprland_resize_delta_matches_direction_and_intent() {
    assert_eq!(resize_delta(Direction::East, true, 40), (40, 0));
    assert_eq!(resize_delta(Direction::North, false, 40), (0, 40));
}
```

- [ ] **Step 2: Run the helper tests and confirm they fail**

Run:

```bash
cargo test -q hyprland_parses_window_address_hex
```

Expected: FAIL because `src/adapters/window_managers/hyprland.rs` and its helpers do not exist yet.

- [ ] **Step 3: Implement the pure helpers and minimal JSON payload types**

```rust
fn parse_window_address(raw: &str) -> Result<u64> {
    let trimmed = raw.trim().strip_prefix("0x").unwrap_or(raw.trim());
    u64::from_str_radix(trimmed, 16).context("invalid Hyprland window address")
}

fn format_window_selector(id: u64) -> String {
    format!("address:0x{id:x}")
}

fn resize_delta(direction: Direction, grow: bool, step: i32) -> (i32, i32) {
    let step = step.abs().max(1);
    match (direction, grow) {
        (Direction::West, true) => (-step, 0),
        (Direction::West, false) => (step, 0),
        (Direction::East, true) => (step, 0),
        (Direction::East, false) => (-step, 0),
        (Direction::North, true) => (0, -step),
        (Direction::North, false) => (0, step),
        (Direction::South, true) => (0, step),
        (Direction::South, false) => (0, -step),
    }
}
```

- [ ] **Step 4: Add JSON parsing coverage for live-shaped Hyprland payloads**

```rust
#[test]
fn hyprland_parses_activewindow_json() {
    let sample = r#"{"address":"0x2a","class":"foot","title":"shell","pid":123,"mapped":true}"#;
    let window: HyprlandClient = serde_json::from_str(sample).unwrap();
    assert_eq!(window.address, "0x2a");
    assert_eq!(window.class.as_deref(), Some("foot"));
}
```

- [ ] **Step 5: Run the focused helper suite and confirm it passes**

Run:

```bash
cargo test -q hyprland_
```

Expected: PASS for the new Hyprland helper tests.

- [ ] **Step 6: Commit the helper foundation**

```bash
git add src/adapters/window_managers/hyprland.rs src/adapters/window_managers/mod.rs
git commit -m "test: add Hyprland WM helper coverage"
```

## Task 2: Wire Hyprland into backend selection

**Files:**
- Modify: `src/config.rs:52-107,1845-1859`
- Modify: `src/adapters/window_managers/mod.rs:14-87,99-107`
- Modify: `src/engine/wm/configured.rs:192-238`
- Modify: `src/engine/wm/mod.rs:180-188`
- Modify: `flake.nix:131-137`
- Modify: `src/adapters/window_managers/hyprland.rs`
- Test: `src/config.rs`, `src/adapters/window_managers/mod.rs`, `src/engine/wm/mod.rs`, `src/adapters/window_managers/hyprland.rs`

- [ ] **Step 1: Write the failing backend-wiring tests**

```rust
assert_eq!(
    toml::from_str::<WmConfig>("enabled_integration = \"hyprland\"")
        .unwrap()
        .enabled_integration,
    WmBackend::Hyprland
);

assert_spec(super::spec_for_backend(WmBackend::Hyprland));
```

- [ ] **Step 2: Run the backend-wiring tests and confirm they fail**

Run:

```bash
cargo test -q wm_backend_deserializes_any_builtin_name && \
cargo test -q built_in_specs_match_window_manager_contract && \
cargo test -q built_in_connectors_are_typed_as_configured_window_managers
```

Expected: FAIL because `Hyprland` is not yet part of `WmBackend`, `spec_for_backend(...)`, or the unsupported-spec table.

- [ ] **Step 3: Implement the enum/spec plumbing**

```rust
pub enum WmBackend {
    Niri,
    I3,
    Hyprland,
    Paneru,
    Yabai,
}

#[cfg(not(target_os = "linux"))]
pub(crate) static UNSUPPORTED_HYPRLAND_SPEC: UnsupportedWindowManagerSpec =
    UnsupportedWindowManagerSpec {
        backend: WmBackend::Hyprland,
        name: "hyprland",
    };
```

```nix
options.enabled_integration = optionalEnumOption [
  "niri"
  "i3"
  "hyprland"
  "paneru"
  "yabai"
] "Window-manager backend. Rust defaults to niri on Linux and yabai on macOS.";
```

- [ ] **Step 4: Declare conservative Hyprland capabilities and expose `HYPRLAND_SPEC`**

```rust
impl WindowManagerCapabilityDescriptor for HyprlandAdapter {
    const NAME: &'static str = "hyprland";
    const CAPABILITIES: WindowManagerCapabilities = WindowManagerCapabilities {
        primitives: PrimitiveWindowManagerCapabilities {
            tear_out_right: false,
            move_column: false,
            consume_into_column_and_move: false,
            set_window_width: true,
            set_window_height: true,
        },
        tear_out: DirectionalCapability::uniform(CapabilitySupport::Unsupported),
        resize: DirectionalCapability::uniform(CapabilitySupport::Native),
    };
}
```

- [ ] **Step 5: Add capability validation coverage**

```rust
#[test]
fn hyprland_capabilities_are_valid() {
    validate_declared_capabilities::<HyprlandAdapter>()
        .expect("hyprland capability descriptor should be valid");
}
```

- [ ] **Step 6: Re-run the targeted wiring tests and confirm they pass**

Run:

```bash
cargo test -q wm_backend_deserializes_any_builtin_name && \
cargo test -q built_in_specs_match_window_manager_contract && \
cargo test -q built_in_connectors_are_typed_as_configured_window_managers && \
cargo test -q hyprland_capabilities_are_valid
```

Expected: PASS.

- [ ] **Step 7: Commit the backend wiring**

```bash
git add src/config.rs src/adapters/window_managers/mod.rs src/engine/wm/configured.rs src/engine/wm/mod.rs flake.nix src/adapters/window_managers/hyprland.rs
git commit -m "feat: register Hyprland WM backend"
```

## Task 3: Implement the Hyprland session surface

**Files:**
- Modify: `src/adapters/window_managers/hyprland.rs`
- Test: `src/adapters/window_managers/hyprland.rs`

- [ ] **Step 1: Write the failing session/command tests**

```rust
#[test]
fn hyprland_marks_focused_client_from_activewindow_address() {
    let active = r#"{"address":"0x20","class":"foot","title":"shell","pid":200}"#;
    let clients = r#"[{"address":"0x10","class":"firefox","title":"docs","pid":100,"mapped":true},{"address":"0x20","class":"foot","title":"shell","pid":200,"mapped":true}]"#;

    let windows = parse_clients_with_focus(active, clients).unwrap();
    assert!(windows.iter().any(|window| window.id == 0x20 && window.is_focused));
}

#[test]
fn hyprland_close_window_by_id_dispatches_expected_command() {
    let mut adapter = test_adapter();
    adapter.close_window_by_id(0x2a).unwrap();
    assert_eq!(
        adapter.take_status_calls(),
        vec![vec!["dispatch", "closewindow", "address:0x2a"]]
    );
}

#[test]
fn hyprland_resize_with_intent_dispatches_signed_resizeactive_delta() {
    let mut adapter = test_adapter();
    adapter
        .resize_with_intent(ResizeIntent::new(Direction::East, ResizeKind::Grow, 40))
        .unwrap();
    assert_eq!(
        adapter.take_status_calls(),
        vec![vec!["dispatch", "resizeactive", "40", "0"]]
    );
}

#[test]
fn hyprland_move_and_spawn_dispatch_expected_commands() {
    let mut adapter = test_adapter();
    adapter.move_direction(Direction::East).unwrap();
    adapter.spawn(vec!["foot".into(), "--app-id".into(), "smoke".into()]).unwrap();
    assert_eq!(
        adapter.take_status_calls(),
        vec![
            vec!["dispatch", "movewindow", "r"],
            vec!["dispatch", "exec", "foot --app-id smoke"],
        ]
    );
}
```

- [ ] **Step 2: Run the focused Hyprland tests and confirm they fail**

Run:

```bash
cargo test -q hyprland_marks_focused_client_from_activewindow_address && \
cargo test -q hyprland_close_window_by_id_dispatches_expected_command && \
cargo test -q hyprland_resize_with_intent_dispatches_signed_resizeactive_delta && \
cargo test -q hyprland_move_and_spawn_dispatch_expected_commands
```

Expected: FAIL because the public adapter methods and their recorded command transport are not implemented yet.

- [ ] **Step 3: Add a tiny testable command transport and implement the trait methods**

```rust
fn command_output(action: &'static str, args: &[&str]) -> Result<std::process::Output> {
    runtime::run_command_output("hyprctl", args, &CommandContext::new(Self::NAME, action))
}

#[cfg(test)]
struct RecordingTransport {
    status_calls: Vec<Vec<String>>,
}

fn focus_direction(&mut self, direction: Direction) -> Result<()> {
    Self::command_status("focus", &["dispatch", "movefocus", direction_name(direction)])
}

fn move_direction(&mut self, direction: Direction) -> Result<()> {
    Self::command_status("move", &["dispatch", "movewindow", direction_name(direction)])
}

fn focus_window_by_id(&mut self, id: u64) -> Result<()> {
    let selector = format_window_selector(id);
    Self::command_status("focus_window_by_id", &["dispatch", "focuswindow", &selector])
}

fn spawn(&mut self, command: Vec<String>) -> Result<()> {
    let joined = command.join(" ");
    Self::command_status("spawn", &["dispatch", "exec", &joined])
}

fn close_window_by_id(&mut self, id: u64) -> Result<()> {
    let selector = format_window_selector(id);
    Self::command_status("close_window_by_id", &["dispatch", "closewindow", &selector])
}
```

- [ ] **Step 4: Implement `focused_window()` and `windows()` from Hyprland JSON**

```rust
fn focused_window(&mut self) -> Result<FocusedWindowRecord> {
    let client = self.active_window()?;
    Ok(FocusedWindowRecord {
        id: parse_window_address(&client.address)?,
        app_id: client.class.and_then(non_empty),
        title: client.title.and_then(non_empty),
        pid: client.pid.and_then(ProcessId::new),
        original_tile_index: 1,
    })
}
```

- [ ] **Step 5: Re-run the Hyprland-focused suite and confirm it passes**

Run:

```bash
cargo test -q hyprland_
```

Expected: PASS for the Hyprland adapter tests, including focus, move, resize, spawn, and address-targeted command helpers.

- [ ] **Step 6: Commit the session implementation**

```bash
git add src/adapters/window_managers/hyprland.rs
git commit -m "feat: implement Hyprland WM session"
```

## Task 4: Update docs and examples

**Files:**
- Modify: `README.md:38-170`
- Modify: `config.example.toml:19-20`
- Test: `src/config.rs`, `src/main.rs`

- [ ] **Step 1: Update the README and example config**

```toml
[wm]
enabled_integration = "niri" # niri | i3 | hyprland | paneru | yabai
```

Update the README’s Linux WM lists/examples to mention `hyprland`, and replace the stale sentence about auto-detection with wording that matches `src/main.rs`: WM selection is explicit and no runtime probing occurs.

- [ ] **Step 2: Mirror the task list into the session-local `plan.md`**
- [ ] **Step 2: Re-run the doc-adjacent tests**

Run:

```bash
cargo test -q repo_config_example_toml_parses && \
cargo test -q cli_help_describes_configured_wm_selection
```

Expected: PASS.

- [ ] **Step 3: Commit the docs/examples**

```bash
git add README.md config.example.toml
git commit -m "docs: document Hyprland WM support"
```

## Task 5: Run full verification and live Hyprland smoke tests

**Files:**
- Modify: none unless verification exposes a real bug
- Test: repository root, live Hyprland session

- [ ] **Step 1: Run the full Rust test suite**

Run:

```bash
cargo test -q
```

Expected: PASS.

- [ ] **Step 2: Build the release binary used by this repo**

Run:

```bash
cargo build --release --target-dir target -q
```

Expected: PASS and refresh `./target/release/yny`.

- [ ] **Step 3: Prepare a disposable Hyprland config file for manual smoke tests**

Run:

```bash
cat >/tmp/yny-hyprland.toml <<'EOF'
[wm]
enabled_integration = "hyprland"
EOF
```

Expected: `/tmp/yny-hyprland.toml` exists.

- [ ] **Step 4: Run read-only smoke checks first**

Run:

```bash
hyprctl -j activewindow | python -m json.tool | sed -n '1,40p'
hyprctl -j clients | python -m json.tool | sed -n '1,80p'
```

Expected: both commands return valid JSON shaped like the parser tests expect.

- [ ] **Step 5: Run active CLI smoke checks in a disposable two-window layout**

```bash
RUST_LOG=debug ./target/release/yny --config /tmp/yny-hyprland.toml focus west
```

Expected: focus shifts west through Hyprland instead of erroring or mentioning another WM backend.

- [ ] **Step 6: Run active move/resize smoke tests only in a disposable layout**

Run:

```bash
./target/release/yny --config /tmp/yny-hyprland.toml move east
./target/release/yny --config /tmp/yny-hyprland.toml resize east grow
./target/release/yny --config /tmp/yny-hyprland.toml resize north shrink
```

Expected: Hyprland moves/resizes the focused tiled window in the requested direction.

- [ ] **Step 7: If verification is clean, commit the finished feature**

```bash
git add src/config.rs src/adapters/window_managers/mod.rs src/adapters/window_managers/hyprland.rs src/engine/wm/configured.rs src/engine/wm/mod.rs flake.nix README.md config.example.toml
git commit -m "feat: add Hyprland WM support"
```
