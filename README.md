# yeet-and-yoink

`yeet-and-yoink` routes `focus`, `move`, and `resize` through a geometry-first orchestrator with
domain plugins (WM / terminal / editor), plus a transfer pipeline for cross-domain moves.

## Config discovery

`config.toml` is loaded in this order:

1. `NIRI_DEEP_CONFIG` (explicit file path)
2. Platform config dir from `etcetera` (typically `$XDG_CONFIG_HOME/yeet-and-yoink/config.toml` on Linux)

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

Foot, Alacritty, and Ghostty are also supported as external terminal hosts;
configure them under `[app.terminal.foot]`, `[app.terminal.alacritty]`, or
`[app.terminal.ghostty]` with `mux_backend = "tmux"` or `mux_backend = "zellij"`.
For kitty native mux (`mux_backend = "kitty"`), kitty itself must expose remote
control to detached callers; add this to `~/.config/kitty/kitty.conf` and restart kitty:

```conf
allow_remote_control socket-only
listen_on unix:@kitty-{kitty_pid}
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
