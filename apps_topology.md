# Obtaining app's window/pane topology 


## Emacs

In Emacs, the 2D layout of windows (which contain buffers) is formally represented as a binary space partitioning (BSP) tree. Emacs exposes this topology through built-in Lisp functions, allowing you to extract it hierarchically, geometrically, or relationally. 

You can evaluate any of these functions in a running instance using `M-:` (eval-expression) or inside the `*scratch*` buffer.

### 1. The Hierarchical Topology (The Window Tree)
The most direct way to get the exact layout structure is to query the window tree. Emacs windows are either "internal" (splits) or "live" (leaf nodes displaying buffers).

*   **`(window-tree)`**
    This function returns a list representing the full split-tree of the selected frame.[221]
    The return format is `(ROOT MINIBUFFER-WINDOW)`. 
    If `ROOT` is a split, it looks like `(DIR EDGES CHILD1 CHILD2 ...)`. 
    *   `DIR` is `t` for a vertical split (windows stacked top-to-bottom) and `nil` for a horizontal split (windows side-by-side).
    *   Leaf nodes are the actual live window objects (e.g., `#<window 3 on *scratch*>`).

*   **`(window-state-get)`**
    If you need a highly serialized, declarative DSL representation of the tree (for saving and restoring layouts), use this. It returns a deeply nested alist representing the topology, buffer names, point positions, and exact pixel sizes.[222]

### 2. The Geometric Topology (Edges & Coordinates)
If you want to calculate spatial adjacency manually, you can query the exact bounding boxes of windows.

*   **`(window-edges)`**
    Returns a list `(LEFT TOP RIGHT BOTTOM)` representing the grid-character coordinates of the current window.[223][224]
*   **`(window-pixel-edges)`**
    Returns the exact same `(LEFT TOP RIGHT BOTTOM)` bounding box, but measured in screen pixels instead of character cells.[224][225]

By mapping the coordinates of all live windows—retrieved via `(window-list)`—you can mathematically reconstruct the 2D grid and identify which windows are sharing identical X or Y boundaries.

### 3. The Relational Topology (Neighbors)
If you just want to know what is immediately adjacent to a specific window without parsing the whole tree, Emacs provides directional querying:

*   **`(window-in-direction DIRECTION)`**
    Where `DIRECTION` is `'above`, `'below`, `'left`, or `'right`. This returns the window object immediately adjacent to the currently selected window in that direction (or `nil` if it touches the frame edge).[221]

### Mapping Windows to Buffers
Any live window object you extract from the topology can be queried to find out what buffer it is currently displaying:

```elisp
(window-buffer (selected-window))      ;; Returns the buffer object
(buffer-name (window-buffer window))   ;; Returns the string name (e.g., "*scratch*")
```

**Example: A quick layout crawler**
If you want to iterate through all visible windows and print their buffer names alongside their bounding boxes to map the topology, you could run:

```elisp
(mapcar (lambda (w) 
          (list (buffer-name (window-buffer w)) 
                (window-edges w))) 
        (window-list))
```


## tmux

### 1. Get the split tree (“layout string”)

Each tmux window has a layout string that encodes the full BSP-style split tree and pane geometry.

From inside tmux, for the current window:

```sh
tmux display-message -p "#{window_layout}"
```

Or for all windows in the session:

```sh
tmux list-windows -F "#{window_index} #{window_layout}"
```

The `window_layout` value looks like:

```text
294a,186x52,0,0{93x52,0,0,185,92x52,94,0,186}
```

which encodes window size and a recursive tree of pane rectangles and IDs; there are libraries that parse this into a tree (example in Rust: `parse_window_layout` in `tmux-lib`).[1][2]

### 2. Get per-pane geometry (for adjacency / edges)

You can query each pane’s bounding box:

```sh
tmux list-panes -F "#{pane_id} #{pane_left} #{pane_top} #{pane_right} #{pane_bottom}"
```

This is explicitly recommended in the tmux issue tracker for reasoning about whether a pane is at an edge or not. From this you can:[3][4]

- Infer which panes share an edge (same `pane_top/pane_bottom` or `pane_left/pane_right` plus touching coordinates).
- Detect which panes touch the window borders (e.g. `pane_left == 0` means touching left window edge).

Directional navigation (`select-pane -L/-R/-U/-D`) is also implemented in terms of this geometry.[5]

***

## Zellij

### 1. Dump the full layout as a DSL (KDL)

Zellij exposes the complete layout of the current session via a CLI “action”:

```sh
zellij action dump-layout
```

This writes the current layout to stdout in Zellij’s KDL-based layout language. People routinely capture it to a file:[6]

```sh
zellij action dump-layout > layout.kdl
```

That KDL structure encodes:

- Tabs
- Panes within each tab
- Split directions (`split_direction="vertical"` / `"horizontal"`)
- Nested `pane { children { ... } }` blocks, etc.[7][6]

You can then parse that KDL to reconstruct the full 2D topology and adjacency.

Zellij currently dumps the whole session; there are open issues about adding flags for “only current tab/pane” and JSON output for easier scripting.[8]

***

## WezTerm

WezTerm has two main ways to introspect the multiplexer state: the CLI and the Lua mux API.

### 1. Structured list of windows/tabs/panes

From outside (or inside) WezTerm:

```sh
wezterm cli list --format json
```

This returns JSON records like:

```json
{
  "window_id": 0,
  "tab_id": 0,
  "pane_id": 0,
  "workspace": "default",
  "size": { "rows": 24, "cols": 80 },
  "title": "...",
  "cwd": "file://..."
}
```

for each pane. This gives you:[9]

- Window / tab / pane IDs and workspace
- Size of each pane in rows/cols (but not its x/y offset inside the tab)

### 2. Lua mux API (per-pane dimensions, neighbors)

Inside your `wezterm.lua` you can use the mux API:

- Get panes and their dimensions:

  ```lua
  local wezterm = require 'wezterm'
  local mux = wezterm.mux

  wezterm.on('dump-layout', function(window, pane)
    local mwin = window:mux_window()
    for _, tab in ipairs(mwin:tabs_with_info()) do
      for _, info in ipairs(tab:panes_with_info()) do
        local p = info.pane
        local dims = p:get_dimensions()  -- cols, viewport_rows, physical_top, etc.
        -- info also includes left, top, width, height per discussions
      end
    end
  end)
  ```

- There is also a CLI helper to get neighbors:

  ```sh
  wezterm cli get-pane-direction right
  wezterm cli get-pane-direction up --pane-id <ID>
  ```

  which attempts to return the pane ID in the given direction.[10][11]

Crucial limitation: the internal pane **binary tree** is *not* exposed on purpose. The maintainer explicitly states they will not expose the pane tree because it’s considered an implementation detail and may change. So you can get:[12][13]

- A flat list of panes with their bounding boxes
- Directional neighbors via `get-pane-direction`

…and from those you can reconstruct adjacency and edge-touching, but not the exact split tree (vsplit/hsplit nesting).

***

## Kitty

Kitty exposes a JSON tree of windows/tabs/panes via its remote-control interface.

### 1. JSON tree of Kitty windows/tabs/panes

Run:

```sh
kitty @ ls --output-format json
```

You get a JSON tree:

- Top level: list of OS “kitty windows”
- Each window: `id`, list of `tabs`
- Each tab: `id`, `title`, list of `windows` (these are the panes)
- Each pane: `id`, title, cwd, PID, commandline, environment, etc.[14][15][16]

This gives you the hierarchical topology (OS window → tab → pane list), but not exact pane geometry.

### 2. Adjacency via “neighbor” matcher

Kitty’s remote control has a `neighbor` matcher for directions (`left`, `right`, `top`, `bottom`). For example:[17][18]

```sh
kitty @ resize-window --match neighbor:right
```

selects the window that is a right-neighbor of the active pane and resizes it, implying Kitty internally knows adjacency and directions; you can use matchers like `neighbor:left` / `neighbor:top` etc. So:

- Global layout mode (tall, grid, etc.) is not directly queryable as a tree.
- But you can:
  - Discover all panes via `kitty @ ls`
  - Walk to directional neighbors using `--match neighbor:...`
  - Combine that with tab/window ordering to reconstruct a relational topology graph.

***

## iTerm2

For iTerm2 you use either AppleScript or the Python API.

### 1. Tabs and sessions (panes) hierarchy

iTerm2’s model is: **Window → Tabs → Sessions**; a “session” corresponds to a split pane.

In AppleScript, you can address:

```applescript
tell application "iTerm2"
  tell current window
    set t to current tab
    set sessList to sessions of t
  end tell
end tell
```

Tabs have an array of sessions, which represent all split panes in that tab.[19][20][21]

In the Python API:

```python
import iterm2

async def main(connection):
    app = await iterm2.async_get_app(connection)
    win = app.current_terminal_window
    tab = win.current_tab
    for session in tab.sessions:   # sessions == split panes in this tab
        # inspect this session
        ...
iterm2.run_until_complete(main)
```

`Tab.sessions` gives a list of split panes in the tab.[22][23]

### 2. Layout / geometry

iTerm2’s Python Tab API has `async_update_layout()` that uses each session’s preferred size (`Session.preferred_size`) to arrange panes. So:[22]

- You can iterate `tab.sessions` and inspect each session’s size/position metadata (via `preferred_size` and other session properties).
- There is not a single public “BSP tree” data structure; instead layout is implicitly determined from sessions plus their size hints and split operations.

So you can reconstruct an approximate topology (which sessions coexist in which tab, their sizes), but you will likely have to rely on conventions / your own bookkeeping at split time for an exact tree.

***

## Neovim

Neovim (and Vim) actually has the cleanest analogue to Emacs’ `window-tree` and `window-edges`.

### 1. Full window split tree

Use:

```vim
:echo winlayout()
```

`winlayout()` returns a nested list representing the window tree in the current tab page:

- `['leaf', winid]` – a leaf window
- `['row', [sub-layouts...]]` – windows arranged left–right
- `['col', [sub-layouts...]]` – windows arranged top–bottom

This is exactly the “row/column/leaf” tree structure discussed in Neovim’s issue about exposing the window layout. Plugins like `ventana.nvim` build on this to manage layouts.[24][25][26][27]

### 2. Per-window geometry / edges

For coordinates and size, use `getwininfo()`:

```vim
:echo getwininfo()
```

Each entry includes fields such as:

- `winid` – window id
- `winrow` – top screen line of the window
- `wincol` – left screen column of the window
- `width`, `height` – size of the window in cells[28][29]

From this you can:

- Determine which windows share vertical/horizontal edges.
- Identify windows that touch the outer editor edge (e.g. `wincol == 1` for left edge, `winrow == 1` for top, etc.).

Together, `winlayout()` + `getwininfo()` give you roughly the same capabilities as Emacs’s `window-tree` + `window-edges`.

***

## VS Code

For VS Code, the extension API exposes **editor groups** and **tabs**, but *not* the full 2D grid layout or split tree.

### 1. Tab groups (editor groups) and tabs

From an extension:

```ts
const groups = vscode.window.tabGroups.all;  // TabGroup[]
for (const group of groups) {
  const tabs = group.tabs;                  // vscode.Tab[]
  const isActive = group.isActive;
}
```

This gives you a list of “tab groups” (VS Code’s notion of editor groups) and, for each, the tabs (editors) inside.[30][31]

You can also inspect:

- `vscode.window.visibleTextEditors` for the editors visible in each group.[32]
- `TextEditor.viewColumn` (One, Two, Three, …) for a coarse “column index” per editor.[33]

### 2. What you cannot get: true grid topology

There is currently no official API to retrieve the *grid layout structure* (how groups are nested, which splits are vertical vs horizontal, etc.):

- There is a long-standing feature request to expose the editor layout, analogous to `vscode.setEditorLayout`, but for reading instead of writing.[34]
- Recent questions and discussions in 2025 still state that you cannot get the full group structure, nesting, or splitting topology via the extension API.[35][36]

So in VS Code you can:

- Know which groups exist and which tabs are in each group.
- Know an approximate left-to-right ordering via `viewColumn`.
- But you **cannot** reconstruct the exact 2D topology the way you can in Emacs/Neovim/tmux.

***

## Summary

- **tmux:** Use `#{window_layout}` for the BSP tree, `list-panes` with `pane_left/pane_top/...` for geometry and adjacency.[2][3]
- **Zellij:** `zellij action dump-layout` prints the current session layout in KDL, which encodes the full split tree.[37][6]
- **WezTerm:** Use `wezterm cli list --format json` plus Lua mux APIs (`pane:get_dimensions()`, `tabs_with_info()`, `panes_with_info()`) and `get-pane-direction`. No official access to the internal pane tree.[38][9][12]
- **Kitty:** `kitty @ ls --output-format json` for OS window → tab → pane tree and `--match neighbor:<dir>` to walk adjacency; layout type/geometry isn’t fully exposed.[16][14][17]
- **iTerm2:** Use Python API (App → Window → Tab → Session) or AppleScript; you get a tab/sessions hierarchy and per-session sizing via `preferred_size`, but not a direct BSP tree.[20][21][22]
- **Neovim:** `winlayout()` for the row/col/leaf tree, `getwininfo()` for window rectangles (winrow/wincol/width/height).[27][29][28]
- **VS Code:** Only high-level tab groups and tabs via `vscode.window.tabGroups.all` and `visibleTextEditors`; there is currently no API to read the full editor grid topology.[31][36][30]

If you want, the next step could be: pick one of these (e.g. tmux or Neovim) and sketch a concrete algorithm for turning their native representation into an explicit graph of panes + neighbor/edge relations.

Sources
[1] Module layoutCopy item path https://docs.rs/tmux-lib/latest/tmux_lib/layout/index.html
[2] What is tmux layout string format? - Stack Overflow https://stackoverflow.com/questions/59439821/what-is-tmux-layout-string-format
[3] ability to set xterm title escape sequence globally by tmux · Issue #764 https://github.com/tmux/tmux/issues/764
[4] Seamless Vim-Tmux-WindowManager-Monitor navigation - Silly Bytes https://sillybytes.net/2016/06/seamlessly-vim-tmux-windowmanager_24.html
[5] tmux(1) - Linux manual page - Michael Kerrisk - man7.org https://man7.org/linux/man-pages/man1/tmux.1.html
[6] Zellij Action - Zellij User Guide https://zellij.dev/documentation/cli-actions
[7] Set start layout based on `default` layout · zellij-org zellij · Discussion #2887 https://github.com/zellij-org/zellij/discussions/2887
[8] action dump-layout: select a specific tab/pane & serialize to JSON https://github.com/zellij-org/zellij/issues/4212
[9] list - Wez's Terminal Emulator https://wezterm.org/cli/cli/list.html
[10] adjust-pane-size - Wez's Terminal Emulator https://wezterm.org/cli/cli/adjust-pane-size.html
[11] get-pane-direction --pane-id x doesn't return expected result · Issue #5088 · wez/wezterm https://github.com/wez/wezterm/issues/5088
[12] How to traverse pane tree · wezterm wezterm · Discussion #5394 https://github.com/wezterm/wezterm/discussions/5394
[13] How to traverse pane tree #5394 - GitHub https://github.com/wez/wezterm/discussions/5394
[14] Controlling kitty from scripts or the shell https://www.ericksonfamily.com/Control4/doc/kitty/html/remote-control.html
[15] kitten-@-ls: List tabs/windows | Man Page | Commands | kitty https://www.mankier.com/1/kitten-@-ls
[16] kitten-@-ls(1) - Arch manual pages https://man.archlinux.org/man/extra/kitty/kitten-@-ls.1.en
[17] Ubuntu Manpage: kitten-@-resize-window - Resize the specified windows https://manpages.ubuntu.com/manpages/noble/man1/kitten-@-resize-window.1.html
[18] kitten-@-resize-window(1) - Arch manual pages https://man.archlinux.org/man/extra/kitty/kitten-@-resize-window.1.en
[19] Applescript (osascript): opening split panes in iTerm 2 and ... https://stackoverflow.com/questions/22339250/applescript-osascript-opening-split-panes-in-iterm-2-and-performing-commands
[20] Scripting - Documentation https://iterm2.com/documentation-scripting.html
[21] Scripting - Documentation - iTerm2 - macOS Terminal Replacement https://iterm2.com/3.2/documentation-scripting.html
[22] Tab — iTerm2 Python API 0.26 documentation https://iterm2.com/python-api/tab.html
[23] Session — iTerm2 Python API 0.26 documentation https://iterm2.com/python-api/session.html
[24] Expose window layout · Issue #3933 · neovim/neovim - GitHub https://github.com/neovim/neovim/issues/3933
[25] jyscao/ventana.nvim https://neovimcraft.com/plugin/jyscao/ventana.nvim/
[26] jyscao/ventana.nvim: Convenient flips & shifts ... https://github.com/jyscao/ventana.nvim
[27] Vim: usr_41.txt https://vimhelp.org/usr_41.txt.html
[28] Windows - Neovim docs https://neovim.io/doc/user/windows.html
[29] [vim/vim] winrow is reported as a 'column' in :help getwininfo() (#7994) https://groups.google.com/g/vim_dev/c/mDZJMzyG4Tw
[30] How do I get all the tabs in a Visual Studio Code window as an array of filenames? https://stackoverflow.com/questions/72772981/how-do-i-see-all-tabs-in-a-visual-studio-code-editor/72774627
[31] VS Code API | Visual Studio Code Extension API https://code.visualstudio.com/api/references/vscode-api
[32] How can I get all TextEditors,which are open at the top of Editor ... https://stackoverflow.com/questions/48223282/how-can-i-get-all-texteditors-which-are-open-at-the-top-of-editor-groups
[33] enum abstract ViewColumn(Int) https://vshaxe.github.io/vscode-extern/vscode/ViewColumn.html
[34] Allow extensions to get the current editor layout · Issue #94817 · microsoft/vscode https://github.com/microsoft/vscode/issues/94817
[35] Need help with vscode plugin api for getting the current window/group structure https://www.reddit.com/r/vscode/comments/1jn8qje/need_help_with_vscode_plugin_api_for_getting_the/
[36] How to use vscode plugin api to get the current window/group structure https://stackoverflow.com/questions/79552037/how-to-use-vscode-plugin-api-to-get-the-current-window-group-structure
[37] Generating a layout for already opened sessions, panes and tabs https://www.reddit.com/r/zellij/comments/1cf6i9k/generating_a_layout_for_already_opened_sessions/
[38] get_dimensions - Wez's Terminal Emulator https://wezterm.org/config/lua/pane/get_dimensions.html
[39] Tmux Cheat Sheet & Quick Reference | Session, window, pane and ... https://tmuxcheatsheet.com
[40] get_pane - Wez's Terminal Emulator https://wezterm.org/config/lua/wezterm.mux/get_pane.html
[41] Documentation - iTerm2 - Mac OS Terminal Replacement https://iterm2.com/documentation/2.1/documentation-one-page.html
[42] nvim-neo-tree: Neovim plugin to manage the file system https://github.com/nvim-neo-tree/neo-tree.nvim
[43] Tmux: How to filter the current session windows using ... https://stackoverflow.com/questions/72937438/tmux-how-to-filter-the-current-session-windows-using-choose-tree-and-format-the
[44] get_pane - Wez's Terminal Emulator https://wezfurlong.org/wezterm/config/lua/wezterm.mux/get_pane.html
[45] Highlights for New Users - Documentation - iTerm2 https://iterm2.com/3.3/documentation-highlights.html
[46] User interface https://code.visualstudio.com/docs/getstarted/userinterface
[47] Display all windows side by side : r/tmux https://www.reddit.com/r/tmux/comments/1dhsqsg/display_all_windows_side_by_side/
[48] Improve dump Screen Action · Issue #1446 · zellij-org/zellij https://github.com/zellij-org/zellij/issues/1446
[49] get_dimensions - Wez's Terminal Emulator https://wezterm.org/config/lua/window/get_dimensions.html
[50] How to get all of the properties of window in NeoVim? https://stackoverflow.com/questions/73850771/how-to-get-all-of-the-properties-of-window-in-neovim
[51] list-clients - Wez's Terminal Emulator https://wezterm.org/cli/cli/list-clients.html
[52] changing marked icon from "M" · Issue #1887 · tmux/tmux - GitHub https://github.com/tmux/tmux/issues/1887
[53] Is there a VS Code API function to return all open text editors and ... https://stackoverflow.com/questions/70989176/is-there-a-vs-code-api-function-to-return-all-open-text-editors-and-their-viewco
[54] Need help with creating and manipulating floating windows in neovim https://www.reddit.com/r/neovim/comments/k4wzs5/need_help_with_creating_and_manipulating_floating/
[55] How to find pane running specific process? https://www.reddit.com/r/tmux/comments/mxbf8v/how_to_find_pane_running_specific_process/
[56] How do I get the number of ViewColumns the user has open from a VSCode extension? https://stackoverflow.com/questions/56961523/how-do-i-get-the-number-of-viewcolumns-the-user-has-open-from-a-vscode-extension/58896667
[57] What is tmux layout string format? https://stackoverflow.com/questions/59439821/what-is-tmux-layout-string-format/71324581
[58] Tmux Cheat Sheet & Quick Reference | Session, window, pane and more http://tmuxcheatsheet.com
[59] Using Neovim floating window to break bad habits - The stuff I do https://www.statox.fr/posts/2021/03/breaking_habits_floating_window/
[60] Choose ViewColumn (editor group) when selecting from quick ... https://github.com/microsoft/vscode-discussions/discussions/1736
[61] Feature: enable -F format string option to show custom information for panes in display-panes command · Issue #3074 · tmux/tmux https://github.com/tmux/tmux/issues/3074
[62] How to use predefined layouts in tmux? - TmuxAI https://tmuxai.dev/tmux-layouts/
[63] Pane Title in Tmux https://stackoverflow.com/questions/9747952/pane-title-in-tmux
[64] How to Adjust Pane Size in tmux https://hatchjs.com/tmux-adjust-pane-size/
[65] [Help] Getting the pane id of all panes by location in window https://groups.google.com/g/tmux-users/c/rWpiSoqG1AE
[66] 将tmux窗格标题设置为用户定义(如果存在)，否则当前工作文件或目录。-腾讯云开发者社区-腾讯云 https://cloud.tencent.com/developer/ask/sof/116320535
[67] In tmux can I resize a pane to an absolute value https://stackoverflow.com/questions/16145078/in-tmux-can-i-resize-a-pane-to-an-absolute-value
[68] set pane title · Issue #680 · tmux/tmux https://github.com/tmux/tmux/issues/680
[69] Support old tmux version (<1.9) when querying height value https://docs.yoctoproject.org/pipermail/yocto/2015-November/027262.html
[70] Formats · tmux/tmux Wiki - GitHub https://github.com/tmux/tmux/wiki/Formats
[71] Looking for a way to format pane-border-status via a script https://www.reddit.com/r/tmux/comments/8mfc2d/looking_for_a_way_to_format_paneborderstatus_via/
[72] resize pane with percent · Issue #383 · tmux/tmux https://github.com/tmux/tmux/issues/383
[73] wezterm/lua-api-crates/mux/src/lib.rs at main - GitHub https://github.com/wez/wezterm/blob/main/lua-api-crates/mux/src/lib.rs
[74] How to get pane object for the newly created pane #3190 - GitHub https://github.com/wez/wezterm/discussions/3190
[75] get_title - WezTerm - Wez's Terminal Emulator https://wezterm.org/config/lua/pane/get_title.html
[76] Attach new tab/pane to existing window? · wezterm wezterm · Discussion #1017 https://github.com/wezterm/wezterm/discussions/1017
[77] How can i create a new tab in the current active wezterm window? https://www.reddit.com/r/wezterm/comments/1ftl8xz/how_can_i_create_a_new_tab_in_the_current_active/
[78] How to get full pane.title? https://www.reddit.com/r/wezterm/comments/1kt5csm/how_to_get_full_panetitle/
[79] I want to configure wezterm such that i dont have to split the panes ... https://www.reddit.com/r/wezterm/comments/1gq8bpv/i_want_to_configure_wezterm_such_that_i_dont_have/
[80] active_pane - Wez's Terminal Emulator https://wezterm.org/config/lua/mux-window/active_pane.html
[81] How I use Wezterm - mwop.net https://mwop.net/blog/2024-07-04-how-i-use-wezterm.html
[82] tabs - Wez's Terminal Emulator https://wezterm.org/config/lua/mux-window/tabs.html
[83] Displaying the Current Running Process and its Arguments in the ... https://softkube.com/blog/displaying-current-running-process-and-its-arguments-window-title-bar-wezterm
[84] Launch iTerm2 and Set Session Title https://iterm2.com/python-api/examples/set_title_forever.html
[85] AppleScript splitting terminal session doesn't work as expected (#3697) · Issues · George Nachman / iterm2 · GitLab https://gitlab.com/gnachman/iterm2/-/issues/3697
[86] Python API: not clear how to stream the content of a session (#8245) · Issues · George Nachman / iterm2 · GitLab https://gitlab.com/gnachman/iterm2/-/issues/8245
[87] AppleScript - iTerm2 https://iterm2.com/3.4/documentation-scripting.html
[88] iTerm2 3.3 Python API -- how to create a 2x2 grid of sessions? https://stackoverflow.com/questions/57681039/iterm2-3-3-python-api-how-to-create-a-2x2-grid-of-sessions
[89] Scripting Fundamentals - Documentation - iTerm2 https://iterm2.com/documentation-scripting-fundamentals.html
[90] Change Session's Profile — iTerm2 Python API 0.26 documentation https://iterm2.com/python-api/examples/setprofile.html
[91] An AppleScript for opening iTerm2 in fullscreen and splitting windows (and starting SSH session). iTerm2で画面分割してフルスクリーンにしつつSSH sessionを始めるスクリプト。 https://gist.github.com/pn11/864703b55f8ae7262527
[92] How does the iterm2 Python API determine which tab a script is ... https://stackoverflow.com/questions/78752372/how-does-the-iterm2-python-api-determine-which-tab-a-script-is-running-in
[93] How can I run some command in iTerm session using the Python API https://stackoverflow.com/questions/62169789/how-can-i-run-some-command-in-iterm-session-using-the-python-api
[94] Custom Layout Tutorial needed! (cols, rows, cells) https://forum.sublimetext.com/t/custom-layout-tutorial-needed-cols-rows-cells/10510
[95] GitHub - tnakaz/window-layout-manager.nvim: WindowLayoutManager is a Neovim plugin that provides commands to manage window layouts in a simple and efficient way. https://github.com/tnakaz/window-layout-manager.nvim
[96] Layout 布局 https://element-plus.org/zh-CN/component/layout
[97] Really confused????? different types of UILayouts https://blenderartists.org/t/really-confused-different-types-of-uilayouts/529053
[98] reformat in vim for a nice column layout https://stackoverflow.com/questions/1229900/reformat-in-vim-for-a-nice-column-layout
[99] yorickpeterse/nvim-window: Easily jump between NeoVim ... - GitHub https://github.com/yorickpeterse/nvim-window
[100] 12.3 Arranging plots with par(mfrow) and layout() https://bookdown.org/ndphillips/YaRrr/arranging-plots-with-parmfrow-and-layout.html
[101] Get position of splitted windows on a vim editor - Stack Overflow https://stackoverflow.com/questions/8070850/get-position-of-splitted-windows-on-a-vim-editor
[102] Gui - Neovim docs https://neovim.io/doc/user/gui/
[103] builtin - Vim Documentation https://vim-jp.org/vimdoc-en/builtin.html
[104] Are there no ways to manage multiple window layouts with plugin ... https://www.reddit.com/r/neovim/comments/1dtga4v/are_there_no_ways_to_manage_multiple_window/
[105] Language Model Tool API | Visual Studio Code Extension API https://code.visualstudio.com/api/extension-guides/ai/tools
[106] Preserve editor group order when applying layout · Issue #130392 · microsoft/vscode https://github.com/microsoft/vscode/issues/130392
[107] Open selected tab via extension API · Issue #188572 - GitHub https://github.com/microsoft/vscode/issues/188572
[108] Test: Grid Editor Layout · Issue #50458 · microsoft/vscode https://github.com/microsoft/vscode/issues/50458
[109] How can I drag and drop editor groups to customize the layout in VS Code? https://stackoverflow.com/questions/76558769/how-can-i-drag-and-drop-editor-groups-to-customize-the-layout-in-vs-code
[110] Calling vscode.window.tabGroups.close in the deactivate function is ... https://github.com/microsoft/vscode/issues/242867
[111] Tabs that group editor grid layouts (Vim-style tabs) · Issue #143024 · microsoft/vscode https://github.com/microsoft/vscode/issues/143024
[112] Tab groups for Visual Studio Code - Stack Overflow https://stackoverflow.com/questions/63170718/tab-groups-for-visual-studio-code
[221] Windows and Frames (GNU Emacs Lisp Reference Manual) https://www.gnu.org/software/emacs/manual/html_node/elisp/Windows-and-Frames.html
[222] Intro https://bmag.github.io/2016/02/24/window-state.html
[223] GNU Emacs Lisp Reference Manual - Windows https://www.math.utah.edu/docs/info/elisp_26.html
[224] 28.24 Coordinates and Windows | Emacs Docs https://emacsdocs.org/docs/elisp/Coordinates-and-Windows
[225] gnu.org https://gnu.huihoo.com/emacs/24.4/emacs-lisp/Coordinates-and-Windows.html
[226] gnu.org https://www.gnu.org/software/emacs/manual/html_node/elisp/Window-Parameters.html
[227] GNU Emacs Lisp Reference Manual: Window Parameters https://ayatakesi.github.io/lispref/27.1/html/Window-Parameters.html
[228] Emacs - Buffers, Windows, and Frames https://www.youtube.com/watch?v=zn5d21VDenw
[229] Window Parameters (ELISP Manual) http://xahlee.info/emacs/emacs_manual/elisp/Window-Parameters.html
[210] 28.11 Buffers and Windows - Emacs Docs https://emacsdocs.org/docs/elisp/Buffers-and-Windows