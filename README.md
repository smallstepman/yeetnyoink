# niri-deep

`niri-deep` routes `focus`, `move`, and `resize` through a geometry-first orchestrator with
domain plugins (WM / terminal / editor), plus a transfer pipeline for cross-domain moves.

## Config discovery

`config.toml` is loaded in this order:

1. `NIRI_DEEP_CONFIG` (explicit file path)
2. `$XDG_CONFIG_DIR/niri-deep/config.toml`
3. `$XDG_CONFIG_HOME/niri-deep/config.toml`
4. `~/.config/niri-deep/config.toml`

If no file is present, defaults from `src/config.rs` are used.

## Minimal config example

```toml
[wm]
enabled_integration = "niri" # or "i3" on Linux

[app.terminal.wezterm]
enabled = true
mux_backend = "wezterm"
focus.internal_panes.enabled = true
move.internal_panes.enabled = true
resize.internal_panes.enabled = true
move.docking.tear_off.enabled = true

[app.editor.emacs]
enabled = true
focus.internal_panes.enabled = true
move.internal_panes.enabled = true
resize.internal_panes.enabled = true
move.docking.tear_off.enabled = true
```

## Runtime architecture

- Command entrypoints: `src/commands/{focus,move_win,resize}.rs`
- Orchestration core: `src/orchestrator.rs`
- Geometry solver: `src/topology.rs`
- Domain/plugin contracts: `src/domain.rs`
- Runtime domain plugins + native-id bridge: `src/domain_plugins.rs`
- Transfer negotiation/conversion: `src/transfer.rs`, `src/pane_state.rs`

## Window manager adapters

Current built-in WM adapters:

- `niri`
- `i3`

Adapter selection is driven by `wm.enabled_integration` (or auto-detection when not overridden).

## Notes

- Legacy planner/context/executor and benchmark-command code paths were removed; runtime command
  dispatch is orchestrator-only.
- WezTerm mux bridge control is config-driven (`app.terminal.wezterm.*`);
  env-var-only bridge toggles are no longer part of the runtime contract.
