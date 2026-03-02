I need to computationally solve in rust the problem of focusing and moving the panes. 

Here is detailed problem description: I need to manage panes inside tiled apps (tmux, emacs, vscode, neovim, wezterm etc) and integrate them with different window managers (which may be using different tiling schemes (master-slave, or srcolling columnar, or bsp, or grid or whatever)). 
I'm looking to solve this problem completely, therefore I'd like to have an algorith that creates a map of of layout of nested tiles and be able to manage calculate what happens if id like to for example execute `move west` while focusing left-edgemost neovim buffer inside neovim workspace, inside vsplit wezterm tab which is a terminal app window managed by i3 tiling window manager which also has its workspaces - i need to be able to understand where the tearoff buffer should appear (should it be new wezterm split, or should it be new i3-managed window), similarly, we need to solve the problems of:
- merging window back into the app 
- moving the window across the WM space 
- changing focus between apps/internal tiles

The end goal is to have: TopologyQuery + UserConfig + Capabilities = Action,
where the implementation of Action (for each app or wm) is as minimal and as clean
as they can be.

The role of this file is to describe common mistakes and confusion points that agents might encounter as they work in this project. If you ever encounter something in the project that surprises you, please alert the developer working with you and indicate that this is the case in the `plugins/niri-deep/AGENTS.md` file to help prevent future agents from having the same issue. 




# Agent's notes

- Surprise encountered: `src/config.rs` currently appears inconsistent with the runtime call sites (`config::prepare`, `wm_adapter_override`, `app_adapter_override`, etc.) and includes schema/test content that does not obviously match the compiled API surface. Before doing architecture work, verify whether config code is mid-migration or partially generated to avoid debugging the wrong interface. After probing the user, I understood that the src/config.rs has recently been rebuilt and it's shape is more-or-less correct and stable, while the rest of the code needs to adapt to current state of config.rs
- Surprise encountered: after the orchestrator rewrite, app adapters' `move_decision/move_internal/move_out` were not being invoked by `move` at all in normal routing, so edge moves in terminal/editor panes silently fell back to WM moves (for niri this triggers `ConsumeOrExpelWindow*`). If `move` behavior seems WM-only, inspect `src/engine/orchestrator.rs` first.
- Surprise encountered: same-domain routing (`source.domain == target.domain`) originally skipped transfer/merge logic entirely and always delegated to WM move, which prevented expected "merge back" behavior for app windows of the same adapter (e.g. wezterm↔wezterm). If merge-back does nothing, inspect `execute_move` same-domain branch in `src/engine/orchestrator.rs`.
- Surprise encountered: wezterm `get-pane-direction` can be inconsistent across immediate back-to-back queries (decision says perpendicular neighbor exists, action query returns none). Rearrange needs a fallback target selection from same-tab pane list instead of assuming direction probes are stable.
- Surprise encountered: successful same-domain merge logs can still result in no visible merge if the wezterm mux bridge is "ready" but no consumer is actually processing `merge.cmd`. When an explicit target window is known, prefer direct CLI merge over bridge enqueue.
- Surprise encountered: app-level tear-out can produce the new window but still place it with default WM geometry unless orchestrator explicitly applies WM tear-out composition (`plan_tear_out`) after `move_out`. Directional placement (W/E/N/S) must be applied as a post-tearout WM step.
- Surprise encountered: `wezterm cli move-pane-to-new-tab --new-window` may leave WM focus on the source window immediately after tearout. If post-tearout placement logic assumes focus switched to new window, it will silently no-op; detect/focus the new window from WM snapshots first.
- Surprise encountered: new WM windows from tearout may not be visible in the very first post-action snapshot. A short retry/poll loop around `wm.windows()` is needed before concluding no new window exists.
- Surprise encountered: `execute_focus` can silently bypass app adapters if it only routes via WM topology. For terminal/editor internal pane focus to work, focus needs the same app-first handling pattern used by move.
