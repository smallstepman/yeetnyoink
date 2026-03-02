<img src="https://r2cdn.perplexity.ai/pplx-full-logo-primary-dark%402x.png" style="height:64px;margin-right:32px"/>

# rectangularly dualizable graph vs binary space partitioning

Rectangularly dualizable graphs (RDGs) are a graph theory concept used to map logical adjacencies into physical rectangular layouts, whereas binary space partitioning (BSP) is a computational geometry method that recursively cuts space into convex sets to create a searchable tree. While an orthogonal BSP strictly generates simple "slicing" geometric partitions, an RDG can model both slicing and highly complex "non-slicing" floorplan structures.

## Core Definitions

A rectangularly dualizable graph is a planar graph whose geometrical dual can be drawn as a set of non-overlapping rectangles filling a bounding box, where graph edges dictate that two rectangles must share a physical boundary. In contrast, binary space partitioning is a top-down algorithmic process that recursively subdivides a Euclidean space using hyperplanes (such as lines or planes). This hierarchical decomposition creates strictly convex spatial cells and is fundamentally designed to accelerate spatial querying rather than strictly dictating geometric adjacency.[^1_1][^1_2][^1_3][^1_4][^1_5]

## Topological Capabilities

The core geometric distinction lies in how these concepts are mathematically capable of partitioning two-dimensional space. When BSP is applied to a 2D bounding box using horizontal and vertical cuts, it strictly produces a guillotine partition, frequently referred to in mathematics as a slicing floorplan. Conversely, RDGs can model slicing floorplans as well as complex non-slicing floorplans—such as a "pinwheel" structure—which are geometrically impossible to generate via BSP because no single line cuts completely across the layout.[^1_6][^1_7][^1_8]

## Field Applications

RDGs are predominantly utilized in VLSI microchip floorplanning, network drawing, and architectural design to successfully pack modules into an optimal rectangular area while guaranteeing critical logical connections. BSP was engineered primarily for 3D environments to facilitate rapid visibility ordering, efficient collision detection, and real-time rendering in applications like video game engines.[^1_2][^1_3][^1_4][^1_8]

## Foundational Differences

The underlying mechanisms of both approaches cater to fundamentally different mathematical and practical needs. Their structural frameworks heavily dictate how they process spatial relationships and layout limitations.[^1_4][^1_7][^1_8][^1_2]


| Feature | Rectangularly Dualizable Graph | Binary Space Partitioning |
| :-- | :-- | :-- |
| **Primary Domain** | Graph theory and VLSI circuit design [^1_1][^1_8] | Computer graphics and computational geometry [^1_3][^1_4] |
| **Data Structure** | Planar graphs and geometric duals [^1_1][^1_2] | Hierarchical binary tree known as a BSP tree [^1_3][^1_4] |
| **Layout Capability** | Slicing and non-slicing (pinwheel) partitions [^1_7][^1_8] | Strictly slicing (guillotine) orthogonal partitions [^1_6][^1_7] |
| **Execution Flow** | Bottom-up mapping of logic to physical space [^1_2] | Top-down recursive subdivision of existing space [^1_3][^1_4] |

<span style="display:none">[^1_10][^1_11][^1_12][^1_13][^1_14][^1_15][^1_16][^1_17][^1_18][^1_9]</span>

<div align="center">⁂</div>

[^1_1]: https://arxiv.org/abs/2102.05304

[^1_2]: https://staff.cdms.westernsydney.edu.au/~anton/Publications/AIMI.pdf

[^1_3]: https://en.wikipedia.org/wiki/Binary_space_partitioning

[^1_4]: https://www.geeksforgeeks.org/dsa/binary-space-partitioning/

[^1_5]: https://ics.uci.edu/~eppstein/junkyard/euler/bsp.html

[^1_6]: https://en.wikipedia.org/wiki/Guillotine_partition

[^1_7]: https://barequet.cs.technion.ac.il/theses/eyal-ackerman-phd-thesis.pdf

[^1_8]: https://researchscript.in/wp-content/uploads/2015/04/IJRCS020406.pdf

[^1_9]: http://www.arxiv.org/abs/2102.05304

[^1_10]: http://www.pphmj.com/abstract/14004.htm

[^1_11]: https://comb-opt.azaruniv.ac.ir/article_14444_9a3a14f51e2556463bdcb0f92c53612e.pdf

[^1_12]: https://math.ucsd.edu/seminar/binary-space-partitions-orthogonal-fat-rectangles

[^1_13]: https://toc.ui.ac.ir/article_26808_c7fc0941f103f715082931044c68cdd4.pdf

[^1_14]: https://proceedings.neurips.cc/paper/2020/file/6271faadeedd7626d661856b7a004e27-Paper.pdf

[^1_15]: https://proceedings.mlr.press/v157/nakano21a/nakano21a.pdf

[^1_16]: https://dl.acm.org/doi/pdf/10.1145/267665.267706

[^1_17]: https://pure.tue.nl/ws/files/90999005/sliceable_gd_r0075.pdf

[^1_18]: https://arxiv.org/pdf/2102.05304.pdf


---

# is any of the popular terminal multiplexers, or code/text editors, or window managers use RDG? which ones are not based on bsp

None of the popular terminal multiplexers, code editors, or window managers use Rectangularly Dualizable Graphs (RDG). Instead, software interfaces rely heavily on slicing floorplans, utilizing layout algorithms like Binary Space Partitioning (BSP), Master-Stack, N-ary trees, or explicit Grid models.

### Why RDG is Absent in User Interfaces

RDG is mathematically capable of modeling "non-slicing" geometries, such as a pinwheel layout where rectangles spiral around a center and no single straight line cuts entirely across the overall space. While this is highly efficient for automated VLSI microchip design, it is impractical for software user interfaces. If a window manager or code editor used a non-slicing layout, a user would not be able to click and drag a continuous window border to resize multiple adjacent panels simultaneously. To maintain intuitive resizing and snapping, graphical interfaces strictly limit themselves to "slicing" (guillotine) partitions that can be manipulated via straight vertical or horizontal lines.[^2_1][^2_2][^2_3]

### Tools Based on BSP

BSP is primarily found in a specific subset of tiling window managers that recursively divide screen real estate into halves. The most prominent example is `bspwm` (Binary Space Partitioning Window Manager), which strictly organizes windows as the leaves of a binary tree.[^2_4]

### Tools Not Based on BSP

Most other popular productivity tools actively avoid strict binary partitioning in favor of alternative layout engines that offer different paradigms for window organization:

- **Master-Stack Window Managers:** Tools like `dwm`, `Xmonad`, and `AwesomeWM` use dynamic tiling algorithms. Instead of requiring the user to manually cut spaces in half, these managers automatically designate a large "master" area for the primary application and mathematically stack all remaining open windows into a smaller secondary column.[^2_5][^2_6][^2_7]
- **N-ary Trees (Containers):** Window managers like `i3` and `Sway` do not restrict splits to strictly binary relationships. They use "explicit layouts" built on container nodes that can hold an arbitrary number of windows side-by-side, vertically stacked, or tabbed together.[^2_6][^2_4]
- **Grid Models:** Code editors like VS Code and window snapping utilities like Windows PowerToys' `FancyZones` or macOS `Rectangle` rely on a 2D grid matrix or predefined canvas zones. Users assign windows to occupy specific fractions of the screen (like exact thirds or quarters) rather than recursively splitting an existing space.[^2_8][^2_9][^2_10]
- **Terminal Multiplexers:** Multiplexers like `tmux` and `Zellij` allow for arbitrary horizontal and vertical pane splitting. While you can manually split panes in a way that *resembles* a binary tree, `tmux` does not enforce an automatic BSP layout natively. If a user wants `tmux` to automatically halve the screen with every new pane, they must write custom shell scripts to force that specific behavior.[^2_11][^2_12]
<span style="display:none">[^2_13][^2_14][^2_15][^2_16][^2_17][^2_18][^2_19][^2_20]</span>

<div align="center">⁂</div>

[^2_1]: https://hpi.de/fileadmin/user_upload/fachgebiete/rabl/publications/2019/GeneralStreamSlicingEDBT2019.pdf

[^2_2]: https://dl.acm.org/doi/pdf/10.1145/37888.37945

[^2_3]: https://en.wikipedia.org/wiki/Wmii

[^2_4]: https://www.youtube.com/watch?v=0xrUVjFKD6E

[^2_5]: https://github.com/klesh/JigsawWM

[^2_6]: https://www.reddit.com/r/linux/comments/87argz/really_nice_tiling_window_manager_presentation/

[^2_7]: https://github.com/pguerin3/cwmrc

[^2_8]: https://www.youtube.com/watch?v=6gANKlnuVA8

[^2_9]: https://github.com/rxhanson/Rectangle

[^2_10]: https://windowsforum.com/threads/windows-tiling-window-managers-four-standout-options-for-power-users.380077/

[^2_11]: https://www.reddit.com/r/tmux/comments/ogxhto/tmux_and_binary_space_partitioning_bsp/

[^2_12]: https://www.youtube.com/watch?v=zDz8nlUflbc

[^2_13]: https://rectangleapp.com

[^2_14]: https://www.reddit.com/r/macapps/comments/u2019b/window_managerdual_screen_workflow_app/

[^2_15]: https://bbs.archlinux.org/viewtopic.php?id=55648

[^2_16]: https://dl.acm.org/doi/fullHtml/10.1145/3433675

[^2_17]: https://www.scribd.com/document/702375977/Proceedings

[^2_18]: https://en.wikipedia.org/wiki/Terminal_multiplexer

[^2_19]: https://glazewm.com

[^2_20]: https://news.ycombinator.com/item?id=22843705


---

# so what is the name of the overarching topology strcuture that could be used to encapsulate different layout of the tools youve mentioned

The overarching topology structure that encapsulates all the UI layout models used by these tools is the **Slicing Tree**, which is the topological representation of a **Guillotine Partition** (also known as a **Slicing Floorplan**).

In graph theory, the geometric dual of this layout is known as a **Series-Parallel Graph**. This topology guarantees that a layout can be perfectly subdivided using only straight, edge-to-edge horizontal or vertical cuts, without ever creating the "pinwheel" shapes found in unrestricted Rectangularly Dualizable Graphs (RDGs).[^3_1][^3_2]

### How the Slicing Tree Unifies the Tools

A slicing tree is a hierarchical data structure where every internal node represents a directional split (either horizontal or vertical), and every leaf node represents a physical window on your screen.[^3_3][^3_1]

Even though popular window managers and editors feel different to the user, they simply traverse or modify a slicing tree in different ways:

- **Binary Trees (`bspwm`):** Uses a strict binary slicing tree. Every internal node splits a rectangle exactly in half, resulting in a parent node with exactly two children.[^3_4][^3_1]
- **N-ary Trees / Containers (`i3`, `Sway`):** Uses an unconstrained slicing tree. A single internal node (like a horizontal split) can have three, four, or more child leaves right next to each other, rather than strictly pairing them off by twos.
- **Master-Stack (`dwm`, `Xmonad`):** Uses a highly asymmetrical slicing tree. The root node splits the screen vertically into two halves; the left leaf is permanently reserved as the "master" window, while the right node becomes a new internal node that continuously creates horizontal slices to "stack" the remaining leaves.[^3_5][^3_4]
- **Grids (VS Code, `Rectangle`, `FancyZones`):** Uses a static, shallow slicing tree. The root node makes a predetermined number of vertical cuts (columns), and each column makes predetermined horizontal cuts (rows) to create a fixed grid array.[^3_4]
- **Manual Splitting (`tmux`, `Zellij`):** The user manually acts as the layout algorithm, deciding whether to insert a vertical or horizontal node into the slicing tree on the fly.[^3_6]


### Why This Topology is Universal for UI

The slicing tree topology completely dominates human-computer interfaces because it can be represented by **1D resizable borders**. Because every cut in a guillotine partition goes from one boundary completely to the other, dragging a single line between two windows will predictably resize them without breaking the geometry or causing other windows to unexpectedly stretch or overlap.[^3_2]
<span style="display:none">[^3_10][^3_11][^3_12][^3_13][^3_14][^3_15][^3_16][^3_17][^3_18][^3_19][^3_20][^3_21][^3_22][^3_23][^3_24][^3_25][^3_26][^3_27][^3_28][^3_29][^3_7][^3_8][^3_9]</span>

<div align="center">⁂</div>

[^3_1]: https://github.com/baskerville/bspwm

[^3_2]: https://en.wikipedia.org/wiki/Guillotine_partition

[^3_3]: https://es.cs.rptu.de/publications/datarsg/ScZi96.pdf

[^3_4]: https://github.com/yazeed1s/zwm

[^3_5]: https://www.reddit.com/r/tilingwindowmanagers/comments/wlbcmd/representation_of_windows_in_tiling_window/

[^3_6]: https://www.reddit.com/r/cpp_questions/comments/1opvkcb/designing_a_tilingwindowmanager/

[^3_7]: https://github.com/TanKaiYi0220/Online-Judge-Problems

[^3_8]: https://vjudge.net/article/4705

[^3_9]: https://patents.google.com/patent/US10620780B2/en

[^3_10]: https://www.udebug.com/?search_string=%2A

[^3_11]: https://mathweb.ucsd.edu/~ronspubs/01_08_floorplan.pdf

[^3_12]: https://citeseerx.ist.psu.edu/document?repid=rep1\&type=pdf\&doi=5e3fa15dafc5c1658711674715c548ea239e4ec4

[^3_13]: https://limsk.ece.gatech.edu/course/ece6133/slides/floorplanning.pdf

[^3_14]: https://www.sciencedirect.com/science/article/pii/S240584402303921X

[^3_15]: https://arxiv.org/pdf/1904.10646.pdf

[^3_16]: https://www.youtube.com/watch?v=xysISs0mcj8

[^3_17]: https://vagnerinacio.com/blog/whybspwm/

[^3_18]: https://en.wikipedia.org/wiki/Guillotine_cutting

[^3_19]: https://bbs.archlinux.org/viewtopic.php?id=149444

[^3_20]: https://sar.ac.id/stmik_ebook/prog_file_file/cfzZ0F4nAl.pdf

[^3_21]: https://www.treemap.com/download/changelog/

[^3_22]: https://dl.ifip.org/db/conf/interact/interact2011-1/TakSGC11.pdf

[^3_23]: https://news.ycombinator.com/item?id=10069521

[^3_24]: https://www.reddit.com/r/golang/comments/uf6if1/pure_go_wasm_tool_to_visualise_code_coverage/

[^3_25]: https://github.com/RobSis/treesome

[^3_26]: https://en.wikipedia.org/wiki/Tiling_window_manager

[^3_27]: https://qtile.org

[^3_28]: https://www.youtube.com/watch?v=y_njYmwKkos

[^3_29]: https://www.reddit.com/r/DistroHopping/comments/ef1l2m/getting_started_with_tilling_window_managers/


---

# alright perfect, `slicing trees` then!

Now, i need to computationally solve in rust the problem of focusing and moving the panes.

Here is detailed problem description: I need to manage panes inside tiled apps (tmux, emacs, vscode, neovim, wezterm etc) and integrate them with different window managers (which may be using different tiling schemes (master-slave, or srcolling columnar, or bsp, or grid or whatever). I'm looking to solve this problem completely, therefore I'd like to have an algorith that creates a map of of layout of nested tiles and be able to manage calculate what happens if id like to for example execute `move west` while focusing left-edgemost neovim buffer inside neovim workspace, inside vsplit wezterm tab which is a terminal app window managed by i3 tiling window manager which also has its workspaces - i need to be able to understand where the tearoff buffer should appear (should it be new wezterm split, or should it be new i3-managed window), similarly, we need to solve the problems of:

- merging window back into the app
- moving the window across the WM space
- changing focus between apps/internal tiles

To computationally solve this in Rust, you must model the entire software stack (Window Manager -> Multiplexer -> Editor) as a single, unified **Slicing Tree**.

Because Rust’s ownership model strictly prohibits self-referential structs with parent pointers, the standard and safest approach for modeling mutable trees is an **Arena Allocator**. You represent the Slicing Tree as a flat `Vec<Node>`, and edges are `usize` indices rather than memory pointers.

Here is the complete algorithmic architecture to map, focus, extract, and merge panes across heterogeneous tiling environments.

## 1. The Rust Arena Data Model

First, define the topology using indices. Every entity (i3, Wezterm, Neovim) manages its layout differently, but computationally, all slicing layouts—whether Binary Space Partitioning (BSP), Master-Stack, or Grid—can be reduced to a tree of Horizontal and Vertical splits.

```rust
type NodeId = usize;

#[derive(Clone, PartialEq)]
enum Direction { Horizontal, Vertical } // Horizontal = side-by-side (West/East)

#[derive(Clone, PartialEq)]
enum DomainLevel {
    Wm(String),       // e.g., "i3" or "leftwm"
    Terminal(String), // e.g., "wezterm"
    Editor(String),   // e.g., "neovim"
}

enum NodeData {
    Split { dir: Direction, children: Vec<NodeId> },
    Leaf { app_id: String, process_state: String }, // e.g., Neovim buffer ID or Wezterm PID
}

struct Node {
    id: NodeId,
    parent: Option<NodeId>,
    domain: DomainLevel,
    data: NodeData,
}

struct LayoutTree {
    arena: Vec<Node>,
    active_focus: NodeId,
}
```


### Abstraction of Tiling Schemes

The brilliance of the Slicing Tree is that it makes your algorithm agnostic to the visual layout:

- **BSP (bspwm):** Nodes always have exactly 2 children.
- **Grid (VS Code):** A root node splits vertically into $N$ children, which each split horizontally into $M$ children.
- **Master-Slave (dwm):** The root node splits horizontally. Child 0 is a `Leaf` (Master). Child 1 is a `Split` containing the rest of the stack.
Your algorithm does not need to know which scheme is being used; it only needs to read the hierarchical arrangement of the nodes.


## 2. The Focus Traversal Algorithm (`move_west`)

To move focus "West" from a Neovim buffer nested deeply inside Wezterm, the algorithm must traverse the tree, searching for a leftward sibling. If it hits the boundary of a domain without finding one, it "escalates."

```rust
impl LayoutTree {
    pub fn focus_west(&mut self, current: NodeId) -> Option<NodeId> {
        let mut curr_id = current;
        let start_domain = self.arena[curr_id].domain.clone();

        // 1. Traverse UP to find an ancestor with a path to the West
        loop {
            let parent_id = match self.arena[curr_id].parent {
                Some(id) => id,
                None => return None, // Hit the absolute edge of the screen
            };

            let parent = &self.arena[parent_id];
            
            // Domain Escalation Check
            if parent.domain != start_domain {
                // We hit the left edge of Neovim. The algorithm must now emit an IPC 
                // command to Wezterm to shift focus west at the multiplexer level.
                self.emit_ipc_focus(parent.domain.clone(), "west");
                return Some(parent_id); // Yield control to outer domain
            }

            if let NodeData::Split { dir: Direction::Horizontal, ref children } = parent.data {
                let curr_idx = children.iter().position(|&x| x == curr_id).unwrap();
                if curr_idx > 0 {
                    // Success! We found a sibling to our West.
                    let target_sibling = children[curr_idx - 1];
                    // 2. Traverse DOWN to find the right-most leaf of the left sibling
                    return Some(self.find_rightmost_leaf(target_sibling));
                }
            }
            // Move up a level
            curr_id = parent_id;
        }
    }

    fn find_rightmost_leaf(&self, start: NodeId) -> NodeId {
        let mut curr = start;
        loop {
            match &self.arena[curr].data {
                NodeData::Leaf { .. } => return curr,
                NodeData::Split { children, .. } => {
                    // Always take the last child when traversing down from the East
                    curr = *children.last().unwrap(); 
                }
            }
        }
    }
}
```


## 3. The Tear-off Algorithm (Moving Windows Outward)

If the user commands `move west` (moving the actual window, not just focus), and the algorithm hits a domain boundary, a **Tear-off Event** is triggered.

If a Neovim buffer reaches the left-most edge of Neovim's layout, it must escape Neovim and become a Wezterm pane.

1. **Extraction:** The central daemon sends an RPC command to Neovim: `:q` or equivalent to unmount the buffer internally, saving the buffer's URI/file path.
2. **Re-parenting:** The daemon evaluates the Slicing Tree to find the Wezterm `Node` that acts as the parent of the Neovim domain.
3. **Spawning:** The daemon sends a CLI command to Wezterm: `wezterm cli split-pane --left -- program neovim <saved_buffer_uri>`.
4. **Tree Update:** The daemon updates the Rust Arena, removing the Neovim `Leaf` and inserting a new Wezterm `Split` containing the newly spawned Neovim instance.

If the Wezterm pane hits the left edge of the Wezterm domain, the escalation continues. Wezterm closes the pane, and the daemon sends an IPC command to `i3` to spawn a brand new Alacritty/Wezterm graphical window running Neovim.

## 4. The Merge Algorithm (Moving Windows Inward)

Merging occurs when an outer-domain object (an i3 window) is pushed "East" into the space occupied by a multiplexer or editor domain.

1. **Collision Detection:** The daemon calculates that the i3 `Leaf` node's designated new location structurally intersects with a Wezterm `Node`.
2. **State Serialization:** The daemon requests the internal state of the moving pane (e.g., extracting the Neovim buffer URI and cursor line, or detaching a `tmux` session).
3. **Window Destruction:** The daemon instructs `i3` to destroy the graphical window (`i3-msg kill`).
4. **Injection:** The daemon sends a command to the target Wezterm domain (`wezterm cli split-pane --right`) and passes the serialized state so the target application opens the exact same buffer/process natively inside its own Slicing Tree.
<span style="display:none">[^4_1][^4_10][^4_2][^4_3][^4_4][^4_5][^4_6][^4_7][^4_8][^4_9]</span>

<div align="center">⁂</div>

[^4_1]: https://www.reddit.com/r/neovim/comments/1j442su/multiplexernvim_enables_seamless_navigation_and/

[^4_2]: https://github.com/stevalkr/multiplexer.nvim

[^4_3]: https://news.ycombinator.com/item?id=40669337

[^4_4]: https://github.com/cdleon/awesome-terminals

[^4_5]: https://www.reddit.com/r/neovim/comments/1b03nh9/awesome_projectsession_navigation_with_wezterm_i3/

[^4_6]: https://bbs.archlinux.org/viewtopic.php?id=292467

[^4_7]: http://way-cooler.org/blog/2016/08/14/designing-a-bi-mutable-directional-tree-safely-in-rust.html

[^4_8]: https://news.ycombinator.com/item?id=44754492

[^4_9]: https://hackaday.com/2025/11/11/waylands-never-ending-opposition-to-multi-window-positioning/

[^4_10]: https://github.com/leftwm/leftwm


---

# how would you solve this in rust typesystem? such that the typesystem enforces the correctness of implementation in case an external cotributor would like to implement a new WM or new App. Im thinking the contributor would have to implement a few traits such as:

- IPC (how to connect and interface with the app/wm)
- TopologyProvider (methods you execute to fetch and convert from app-native topology description into our Rust Arena Data Model, think emacs's `(window-tree)` or tmux's (`tmux list-panes`)
- TopologyModifier (methods you execute to change focus directionally, move pane inside the app, or tear off / merge back a pane)

To enforce correctness using Rust’s type system, you must design the API so that **invalid states are unrepresentable**. If an external contributor writes a plugin for a new Window Manager (WM) or App, the compiler should physically prevent them from returning disconnected graphs, leaking memory panes, or successfully tearing off a pane without extracting its transferable state.

Here is the type-driven architecture to achieve this, using **Associated Types**, **State Enums**, and **Must-Use Tokens**.

### 1. The Cross-Domain Currency (`PanePayload`)

Before defining the traits, we need a strongly typed "currency" for tear-offs and merges. A pane cannot just be a raw window handle, because an `i3` window handle means nothing to `Neovim`. The type system must enforce exhaustive matching on what is actually moving.

```rust
#[derive(Clone, Debug)]
pub enum PanePayload {
    /// Moving a raw text buffer (e.g., Neovim tearing off)
    EditorBuffer { file_uri: String, line: usize, col: usize },
    /// Moving a multiplexer session (e.g., Tmux pane tearing off)
    TerminalSession { session_name: String, pty_id: String },
    /// Moving an entire graphical application (e.g., i3 window)
    GuiApp { x11_window_id: u32, class_name: String },
}
```


### 2. Topology Provider: Preventing Dangling Graphs

If contributors interact directly with the `Vec<Node>` Arena, they could easily introduce dangling `NodeId`s or cyclic graphs.
To enforce correctness, the `TopologyProvider` must not return Arena IDs. It must return a localized, guaranteed-acyclic **Abstract Syntax Tree (AST)** of their layout. The core engine will then safely map this AST into the global Arena.

```rust
#[derive(Debug, Clone)]
pub enum Direction { North, South, East, West }

/// A strictly tree-shaped, acyclic representation of the app's local layout.
pub enum AppLayout<T> {
    Split { dir: Direction, children: Vec<AppLayout<T>> },
    Pane { native_id: T, is_focused: bool },
}

pub trait TopologyProvider {
    type NativeId: Clone + PartialEq; // e.g., `usize` for Neovim, `String` for Tmux
    type Error: std::error::Error;

    /// Forces the contributor to return a mathematically sound slicing tree.
    /// It is impossible to return a cyclic graph with this enum.
    fn fetch_layout(&self) -> Result<AppLayout<Self::NativeId>, Self::Error>;
}
```


### 3. Topology Modifier: Enforcing State Synchronization

When a contributor implements `TopologyModifier`, they mutate the native app (e.g., executing `tmux swap-pane`). If they do this, our central Rust Arena becomes instantly out-of-sync.

To solve this, we use the **Type-State Pattern** and `#[must_use]`. Mutating functions do not return `()`. They return a `TopologyChanged` token that the core engine *must* consume to rebuild the Arena.

```rust
/// A zero-cost token that proves the app's internal layout was modified.
#[must_use = "You must sync the Arena after modifying the topology"]
pub struct TopologyChanged;

pub trait TopologyModifier: TopologyProvider {
    /// Moves focus internally.
    fn focus_pane(&mut self, id: &Self::NativeId) -> Result<TopologyChanged, Self::Error>;

    /// Moves a pane internally (e.g., swapping two sibling splits).
    fn move_pane_internal(
        &mut self, 
        id: &Self::NativeId, 
        dir: Direction
    ) -> Result<TopologyChanged, Self::Error>;

    /// TEAR OFF: Enforces that the app *must* return a generic PanePayload.
    /// You cannot drop a pane from the app without extracting its context.
    fn tear_off(
        &mut self, 
        id: &Self::NativeId
    ) -> Result<(PanePayload, TopologyChanged), Self::Error>;

    /// MERGE: Enforces that the app *must* accept a generic PanePayload.
    /// The target app receives the payload and returns the ID of the newly spawned native pane.
    fn merge_in(
        &mut self, 
        target_id: &Self::NativeId, 
        dir: Direction, 
        payload: PanePayload
    ) -> Result<(Self::NativeId, TopologyChanged), Self::Error>;
}
```


### 4. IPC Adapter: Managing Lifecycles

The IPC trait guarantees that we can connect to the app and gracefully handle disconnections. We use associated types to ensure the configuration passed to the plugin matches the plugin's requirements.

```rust
pub trait IpcAdapter {
    type Config;
    type Error: std::error::Error;

    fn connect(config: &Self::Config) -> Result<Self, Self::Error> where Self: Sized;
    fn ping(&self) -> Result<(), Self::Error>;
}
```


### 5. The Unified Plugin Trait

Finally, we bind these together into a single `TilingDomain` trait. Any external contributor simply implements this master trait.

```rust
pub trait TilingDomain: IpcAdapter + TopologyProvider + TopologyModifier {
    fn domain_name(&self) -> &'static str;
}
```


### How the Compiler Enforces Correctness in Action

Imagine a user executing `move west` on a Neovim buffer, and it hits the Neovim boundary, needing to tear off into `i3`.

The core orchestrator daemon will execute this algorithm, fully checked by the Rust compiler:

1. **Extraction:** The daemon calls `neovim.tear_off(buffer_id)`.
    * *Compiler guarantee:* Neovim cannot just delete the buffer; the type signature forces Neovim to return a `PanePayload` (yielding `PanePayload::EditorBuffer { uri: "main.rs", line: 42 }`).
    * *Compiler guarantee:* It also returns `TopologyChanged`. Because of `#[must_use]`, the daemon gets a compiler warning/error if it forgets to call `neovim.fetch_layout()` to update the Arena afterwards.
2. **Translation via Exhaustive Match:** The daemon passes the `PanePayload` to `i3.merge_in(target_window, Direction::West, payload)`.
    * *Compiler guarantee:* The i3 plugin implements `merge_in`. Inside that method, the contributor must `match` on `PanePayload`. The Rust compiler forces them to handle `EditorBuffer`. The i3 plugin logic will read the `uri` and spawn `alacritty -e nvim +42 main.rs`, successfully mapping the editor state into a GUI window state.

---

# i want you to look at all of this second time, and think deeply what is the best way to solve this issue

There are five fundamental design flaws in the previous approach that compound together. Fixing them requires rethinking the model from the ground up, not just patching the traits. Here is the full critique and the correct redesign.

***

## The 5 Fundamental Flaws

### Flaw 1: Tree Traversal is the Wrong Navigation Algorithm

The previous approach walked the tree upward to find a "Western sibling." This completely breaks in a Grid layout (where a pane can have 5 siblings) or a Master-Stack layout (where the tree shape is highly asymmetric). The real answer, confirmed by how production window managers actually implement directional focus, is **physical geometry**. Every leaf pane occupies a `Rect { x, y, w, h }`. "Move West" means: find the leaf whose right edge is closest to my left edge. This algorithm works identically regardless of whether the WM uses BSP, Grid, or Master-Stack, and naturally crosses domain boundaries.[^6_1]

### Flaw 2: `Direction` Was a Single, Conflated Enum

The previous design used `Direction { Horizontal, Vertical }` for both split axes and movement. These are orthogonal concepts. A `Horizontal` split axis means children are arranged East/West — so movement `East` or `West` is an *intra-split* movement, while `North`/`South` is *cross-split*. Using a single type for both means neither is correct.

### Flaw 3: A Single Flat Arena is the Wrong Data Model

Tagging every node with `DomainLevel` at runtime is fragile. A contributor could accidentally tag a node with the wrong domain. The structural nesting of domains (i3 contains Wezterm contains Neovim) should be a **structural property of the type system**, not a runtime field.

### Flaw 4: `PanePayload` is a Closed Enum

If a contributor adds JetBrains or Emacs, they must modify `PanePayload` in the core crate. This violates the Open/Closed Principle and makes the plugin system impossible. The correct solution is a **capability registry** using `TypeId`.

### Flaw 5: `TopologyChanged` is Publicly Constructible

Any contributor can write `TopologyChanged` as a literal in their code to satisfy the `#[must_use]` requirement without actually syncing the Arena. The token needs to be **impossible to construct** outside the module that defines the trait.

***

## The Correct Redesign

### Step 1: Fix the Two `Direction` Types

```rust
/// The axis along which a split node divides its children.
pub enum SplitAxis { Horizontal, Vertical }

/// The cardinal direction a user intends to move.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Cardinal { North, South, East, West }

impl Cardinal {
    /// The SplitAxis whose children are adjacently arranged in this direction.
    pub fn axis(self) -> SplitAxis {
        match self {
            Cardinal::East | Cardinal::West  => SplitAxis::Horizontal,
            Cardinal::North | Cardinal::South => SplitAxis::Vertical,
        }
    }
}
```


### Step 2: Fix Navigation — Use Geometry, Not Tree Walking

Every leaf node stores a physical `Rect`. The navigator is now a flat geometric query over **all registered leaf panes across all domains**:[^6_1]

```rust
#[derive(Clone, Copy)]
pub struct Rect { pub x: i32, pub y: i32, pub w: i32, pub h: i32 }

impl Rect {
    /// Returns the "leading edge" in a given direction.
    pub fn leading_edge(self, dir: Cardinal) -> i32 {
        match dir {
            Cardinal::East  => self.x + self.w,
            Cardinal::West  => self.x,
            Cardinal::South => self.y + self.h,
            Cardinal::North => self.y,
        }
    }
    
    /// Returns the "receiving edge" — what a neighbor sees when approaching from dir.
    pub fn receiving_edge(self, dir: Cardinal) -> i32 {
        self.leading_edge(dir.opposite())
    }
    
    /// Checks overlap in the perpendicular axis to avoid selecting non-overlapping panes.
    pub fn perp_overlap(self, other: Rect, dir: Cardinal) -> bool {
        match dir.axis() {
            SplitAxis::Horizontal => 
                self.y < other.y + other.h && self.y + self.h > other.y,
            SplitAxis::Vertical   => 
                self.x < other.x + other.w && self.x + self.w > other.x,
        }
    }
}
```

The navigation function itself becomes trivially correct and domain-agnostic:

```rust
pub fn find_neighbor<'a>(
    all_leaves: &'a [GlobalLeaf],
    focused: &GlobalLeaf,
    dir: Cardinal,
) -> Option<&'a GlobalLeaf> {
    let my_edge = focused.rect.leading_edge(dir);
    
    all_leaves.iter()
        .filter(|l| l.id != focused.id)
        // Must be strictly in the target direction
        .filter(|l| l.rect.receiving_edge(dir) > my_edge)
        // Must have perpendicular overlap (actually adjacent, not diagonally far)
        .filter(|l| focused.rect.perp_overlap(l.rect, dir))
        // Closest edge wins
        .min_by_key(|l| l.rect.receiving_edge(dir) - my_edge)
}
```

The **domain hierarchy** is only consulted *after* finding a neighbor, to decide the consequence: same domain → send a focus IPC command; different domain → evaluate tear-off or cross-domain focus.

### Step 3: Fix the Data Model — A Forest of Arenas

Instead of one flat Arena with domain tags, each domain plugin owns its own private Arena. A separate, lightweight `GlobalDomainTree` tracks only the nesting of domains:

```rust
/// The global tree only knows about domain containment, not pane details.
pub struct GlobalDomainTree {
    /// Flat list of all registered domains in DFS order.
    domains: Vec<DomainNode>,
}

pub struct DomainNode {
    pub id: DomainId,
    pub parent: Option<DomainId>,
    pub rect: Rect,          // The screen region this domain occupies
    pub plugin: Box<dyn TilingDomain>,
}

/// A flat list of ALL leaf panes across all domains, maintained by the daemon.
/// This is the input to `find_neighbor`.
pub struct GlobalLeaf {
    pub id: LeafId,
    pub domain: DomainId,
    pub native_id_blob: Vec<u8>, // Opaque: each domain plugin interprets this
    pub rect: Rect,
}
```

The daemon rebuilds `Vec<GlobalLeaf>` after any `TopologyChanged` event by calling `fetch_layout()` on the relevant domain and mapping the resulting `AppLayout` tree (which carries `Rect` coordinates) into the flat list.

### Step 4: Fix `PanePayload` — Open Capability Registry

Replace the closed enum with a trait and a runtime registry. This is essentially typed content-negotiation, similar to HTTP `Accept` headers:

```rust
use std::any::{Any, TypeId};

/// Any pane's transferable state must implement this.
pub trait PaneState: Any + Send + 'static {
    fn type_id(&self) -> TypeId { TypeId::of::<Self>() }
    fn as_any(self: Box<Self>) -> Box<dyn Any>;
}

/// The central broker that finds converters between payload types.
/// Plugins register converters on startup.
pub struct PayloadRegistry {
    // (FromTypeId, ToTypeId) -> converter function
    converters: HashMap<(TypeId, TypeId), Box<dyn Fn(Box<dyn Any>) -> Box<dyn Any>>>,
}

impl PayloadRegistry {
    /// Register: "I know how to convert an EditorBufferState into a TerminalSessionState."
    pub fn register<From: PaneState, To: PaneState>(
        &mut self,
        f: impl Fn(From) -> To + 'static
    ) { ... }

    pub fn convert(
        &self,
        payload: Box<dyn PaneState>,
        target_type: TypeId,
    ) -> Option<Box<dyn PaneState>> { ... }
}
```

Each plugin declares what it can *emit* and what it can *absorb* as a `TypeId`. The registry finds the conversion path between them. If no path exists, the daemon falls back to a generic "spawn a new shell process" strategy.

### Step 5: Fix `TopologyChanged` — Make it Unforgiveable to Fake

Use a module-private constructor with a `PhantomData` lifetime to prevent construction outside the crate:[^6_2][^6_3]

```rust
mod sealed {
    use std::marker::PhantomData;

    /// Zero-size token. Cannot be constructed by external code.
    /// The lifetime `'sync` ensures it cannot outlive the borrow of the modifier.
    #[must_use = "Arena is stale until you call fetch_layout() to resync"]
    pub struct TopologyChanged(pub(super) PhantomData<()>);
    
    /// Only constructible inside this module.
    impl TopologyChanged {
        pub(super) fn new() -> Self { Self(PhantomData) }
    }
}
pub use sealed::TopologyChanged;

pub trait TopologyModifier: TopologyProvider {
    fn tear_off(
        &mut self,
        id: &Self::NativeId,
    ) -> Result<(Box<dyn PaneState>, sealed::TopologyChanged), Self::Error>;
}
```

A contributor implementing `tear_off` literally cannot return `TopologyChanged::new()` because `new()` is `pub(super)` — scoped to the `sealed` module only. The only way to get a `TopologyChanged` token is for the framework to inject it via a provided blanket implementation, ensuring the token is always issued by trusted orchestration code.

***

## The Unified Plugin Trait

With all five fixes in place, an external contributor implementing a new WM/App plugin must implement exactly three traits. The compiler refuses to compile an incomplete or incorrect implementation:

```rust
pub trait TilingDomain: IpcAdapter + TopologyProvider + TopologyModifier {
    fn domain_name(&self) -> &'static str;
    fn rect(&self) -> Rect; // The screen area this domain manages
    
    /// Declare which PaneState TypeIds this domain can absorb.
    fn supported_payload_types(&self) -> &[TypeId];
}
```

The flow for `move west` across the Neovim → Wezterm → i3 boundary is now:

1. **Geometric query** → `find_neighbor(&all_leaves, &focused_neovim_buf, West)` → finds an `i3` leaf
2. **Domain comparison** → source domain is `Neovim`, target domain is `i3` → tear-off path triggered
3. **Capability negotiation** → `neovim.tear_off(id)` returns `Box<EditorBufferState>` + `TopologyChanged`; registry checks if i3 can absorb `EditorBufferState` directly or needs a converter
4. **Arena resync** → `TopologyChanged` token consumed, `neovim.fetch_layout()` is called; compiler warns if you forget
5. **Spawn** → `i3.merge_in(target, West, converted_payload)` spawns a new window; returns `TopologyChanged`; Arena resynced again
<span style="display:none">[^6_10][^6_11][^6_12][^6_13][^6_14][^6_15][^6_4][^6_5][^6_6][^6_7][^6_8][^6_9]</span>

<div align="center">⁂</div>

[^6_1]: https://github.com/leftwm/leftwm/issues/771

[^6_2]: https://blog.yoshuawuyts.com/state-machines-3/

[^6_3]: https://rust-lang.github.io/api-guidelines/future-proofing.html

[^6_4]: https://github.com/ntrrgc/right-window

[^6_5]: https://www.reddit.com/r/suckless/comments/orf4fd/directional_focus_dwm/

[^6_6]: https://learn.microsoft.com/pt-br/windows/apps/design/input/focus-navigation-programmatic

[^6_7]: https://krshrimali.github.io/posts/2022/07/porting-a-tiling-window-manager-extenstion-to-c-bismuth-part-2-getting-closest-relative-window/

[^6_8]: https://www.reddit.com/r/rust/comments/1gaby42/update_use_typestate_pattern_without_the_ugly/

[^6_9]: https://bennett.dev/rust/downcast-trait-object/

[^6_10]: https://learn.microsoft.com/it-it/windows/apps/develop/input/focus-navigation-programmatic

[^6_11]: https://www.reddit.com/r/rust/comments/1c3un60/downcasting_trait_objects_with_minimal_support/

[^6_12]: https://roboflow.com/convert/supervisely-json-to-yolov5-obb

[^6_13]: https://github.com/rust-lang/rust-clippy/issues/12427

[^6_14]: https://www.reddit.com/r/Windows10/comments/vww48c/change_windowsfocus_by_directional_arrow_key_kb/

[^6_15]: https://www.reddit.com/r/rust/comments/f0l7cw/is_this_typetagging_sealed_trait_pattern_sound/


---

# \# Obtaining app's window/pane topology

## Emacs

In Emacs, the 2D layout of windows (which contain buffers) is formally represented as a binary space partitioning (BSP) tree. Emacs exposes this topology through built-in Lisp functions, allowing you to extract it hierarchically, geometrically, or relationally.

You can evaluate any of these functions in a running instance using `M-:` (eval-expression) or inside the `*scratch*` buffer.

### 1. The Hierarchical Topology (The Window Tree)

The most direct way to get the exact layout structure is to query the window tree. Emacs windows are either "internal" (splits) or "live" (leaf nodes displaying buffers).

* **`(window-tree)`**
This function returns a list representing the full split-tree of the selected frame.[221]
The return format is `(ROOT MINIBUFFER-WINDOW)`.
If `ROOT` is a split, it looks like `(DIR EDGES CHILD1 CHILD2 ...)`.
    * `DIR` is `t` for a vertical split (windows stacked top-to-bottom) and `nil` for a horizontal split (windows side-by-side).
    * Leaf nodes are the actual live window objects (e.g., `#<window 3 on *scratch*>`).
* **`(window-state-get)`**
If you need a highly serialized, declarative DSL representation of the tree (for saving and restoring layouts), use this. It returns a deeply nested alist representing the topology, buffer names, point positions, and exact pixel sizes.[222]


### 2. The Geometric Topology (Edges \& Coordinates)

If you want to calculate spatial adjacency manually, you can query the exact bounding boxes of windows.

* **`(window-edges)`**
Returns a list `(LEFT TOP RIGHT BOTTOM)` representing the grid-character coordinates of the current window.[223][224]
* **`(window-pixel-edges)`**
Returns the exact same `(LEFT TOP RIGHT BOTTOM)` bounding box, but measured in screen pixels instead of character cells.[224][225]

By mapping the coordinates of all live windows—retrieved via `(window-list)`—you can mathematically reconstruct the 2D grid and identify which windows are sharing identical X or Y boundaries.

### 3. The Relational Topology (Neighbors)

If you just want to know what is immediately adjacent to a specific window without parsing the whole tree, Emacs provides directional querying:

* **`(window-in-direction DIRECTION)`**
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

-------

given the above, provide and example how would you implement emacs app using the reasoning done so far

This is a fully design-driven answer with no ambiguity to look up. Let me synthesize everything we've established into a complete, concrete Emacs implementation.

The implementation has three non-obvious design decisions before writing a single line of code:

1. **Do not parse S-expressions in Rust.** Emacs has `json-encode` built in. Inject multi-line Lisp that serializes `window-tree` directly to JSON, then use `serde_json` to deserialize into clean Rust types.
2. **`NativeId` = pixel bounding box.** Emacs windows have no stable integer ID exposed across IPC calls. Pixel edges `(left top right bottom)` are unique per window within a layout snapshot, and can be used to reconstruct a Lisp selector that resolves back to a live window object.
3. **The sealed token pattern — `_impl` + blanket wrapper.** The `TopologyChanged` token must be issued only by the framework, never by the contributor. This is achieved by having contributors implement `tear_off_impl` (returns only the payload), while a blanket wrapper in the core crate's private module appends the token.

***

## The Sealed Token Pattern (Core Crate)

First, the final version of the sealed token, using the `_impl` / blanket wrapper split:

```rust
// core/src/modifier.rs

mod sealed {
    use std::marker::PhantomData;

    #[must_use = "Arena is stale: call fetch_layout() to resync after topology changes"]
    pub struct TopologyChanged(pub(super) PhantomData<()>);
}
pub use sealed::TopologyChanged;

/// What CONTRIBUTORS implement — no token involved.
pub trait TopologyModifierImpl: TopologyProvider {
    fn focus_impl(&mut self, id: &Self::NativeId) -> Result<(), Self::Error>;
    fn move_impl(&mut self, id: &Self::NativeId, dir: Cardinal) -> Result<(), Self::Error>;

    /// Must destroy the pane in the native app and return its transferable state.
    fn tear_off_impl(&mut self, id: &Self::NativeId) -> Result<Box<dyn PaneState>, Self::Error>;

    /// Must create a new pane in the native app from the payload and return its new NativeId.
    fn merge_in_impl(
        &mut self,
        target: &Self::NativeId,
        dir: Cardinal,
        payload: Box<dyn PaneState>,
    ) -> Result<Self::NativeId, Self::Error>;
}

/// Blanket wrapper — lives in the core crate, has access to the sealed constructor.
/// Contributors get this for FREE and CANNOT override it.
pub trait TopologyModifier: TopologyModifierImpl {
    fn focus(&mut self, id: &Self::NativeId) -> Result<TopologyChanged, Self::Error> {
        self.focus_impl(id)?;
        Ok(sealed::TopologyChanged(std::marker::PhantomData))
    }
    fn tear_off(&mut self, id: &Self::NativeId) -> Result<(Box<dyn PaneState>, TopologyChanged), Self::Error> {
        let state = self.tear_off_impl(id)?;
        Ok((state, sealed::TopologyChanged(std::marker::PhantomData)))
    }
    fn merge_in(&mut self, target: &Self::NativeId, dir: Cardinal, payload: Box<dyn PaneState>)
        -> Result<(Self::NativeId, TopologyChanged), Self::Error>
    {
        let new_id = self.merge_in_impl(target, dir, payload)?;
        Ok((new_id, sealed::TopologyChanged(std::marker::PhantomData)))
    }
}

// The blanket impl — anyone implementing TopologyModifierImpl gets TopologyModifier for free.
impl<T: TopologyModifierImpl> TopologyModifier for T {}
```

A contributor implementing `EmacsAdapter` implements only `TopologyModifierImpl`. They get `TopologyModifier` automatically. They cannot override `tear_off` because the token constructor is `pub(super)` — inaccessible outside `sealed`.

***

## The Emacs Plugin

```rust
// plugins/emacs/src/lib.rs

use std::any::TypeId;
use std::process::Command;
use serde::Deserialize;
use core::{
    IpcAdapter, AppLayout, SplitAxis, Rect, Cardinal, PaneState,
    TopologyProvider, TopologyModifierImpl, TilingDomain,
};

// ── Config ────────────────────────────────────────────────────────────────────

pub struct EmacsConfig {
    pub emacsclient_bin: String,
    pub server_name: String,
}
impl Default for EmacsConfig {
    fn default() -> Self {
        Self { emacsclient_bin: "emacsclient".into(), server_name: "server".into() }
    }
}

pub struct EmacsAdapter { config: EmacsConfig }

// ── NativeId ──────────────────────────────────────────────────────────────────
// Pixel edges are unique per window within a single layout snapshot.

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct EmacsWindowId { pub left: i32, pub top: i32, pub right: i32, pub bottom: i32 }

impl EmacsWindowId {
    fn to_rect(&self) -> Rect {
        Rect { x: self.left, y: self.top, w: self.right - self.left, h: self.bottom - self.top }
    }

    /// Emits Lisp that resolves this ID back to a live window object at runtime.
    /// This is the bridge between our static snapshot ID and the live Emacs session.
    fn as_lisp_selector(&self) -> String {
        format!(
            "(car (seq-filter \
               (lambda (w) (equal (window-pixel-edges w) (list {} {} {} {}))) \
               (window-list nil 'no-mini)))",
            self.left, self.top, self.right, self.bottom
        )
    }
}

// ── PaneState ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct EmacsBufferState {
    pub buffer_name: String,
    pub file_path:   Option<String>,
    pub point:       u64,   // byte offset of the cursor
    pub window_start: u64,  // byte offset of the top-of-window scroll position
}
impl PaneState for EmacsBufferState {
    fn as_any(self: Box<Self>) -> Box<dyn std::any::Any> { self }
}

// ── Error ─────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum EmacsError {
    ProcessFailed(std::io::Error),
    EvalFailed(String),
    ParseFailed(String),
    WindowNotFound(EmacsWindowId),
}
impl std::fmt::Display for EmacsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{:?}", self) }
}
impl std::error::Error for EmacsError {}

// ── IPC layer ─────────────────────────────────────────────────────────────────

impl EmacsAdapter {
    fn eval(&self, lisp: &str) -> Result<String, EmacsError> {
        let out = Command::new(&self.config.emacsclient_bin)
            .args(["--socket-name", &self.config.server_name, "--eval", lisp])
            .output()
            .map_err(EmacsError::ProcessFailed)?;

        if !out.status.success() {
            return Err(EmacsError::EvalFailed(
                String::from_utf8_lossy(&out.stderr).into_owned()
            ));
        }
        // emacsclient wraps string output in quotes; strip them
        let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Ok(raw.trim_matches('"').to_string())
    }
}

impl IpcAdapter for EmacsAdapter {
    type Config = EmacsConfig;
    type Error  = EmacsError;

    fn connect(cfg: &Self::Config) -> Result<Self, Self::Error> {
        let a = EmacsAdapter { config: EmacsConfig {
            emacsclient_bin: cfg.emacsclient_bin.clone(),
            server_name:     cfg.server_name.clone(),
        }};
        a.ping()?;
        Ok(a)
    }

    fn ping(&self) -> Result<(), Self::Error> { self.eval("t").map(|_| ()) }
}

// ── Lisp → JSON serializer ────────────────────────────────────────────────────
//
// This is the critical bridge. Rather than writing an S-expression parser in
// Rust, we inject Lisp that uses Emacs's own json-encode to produce clean JSON.
// window-tree returns: (DIR EDGES CHILD...) for splits, or a window-object for leaves.
// DIR: t = vertical split (children stacked N/S), nil = horizontal (children E/W).

const SERIALIZE_TREE_LISP: &str = r#"
(cl-labels
  ((ser (node)
     (if (windowp node)
       ;; Leaf node: capture full state needed for both topology and tear-off
       (let* ((e   (window-pixel-edges node))
              (buf (window-buffer node)))
         `((type    . "leaf")
           (left    . ,(nth 0 e))
           (top     . ,(nth 1 e))
           (right   . ,(nth 2 e))
           (bottom  . ,(nth 3 e))
           (buf     . ,(buffer-name buf))
           (file    . ,(or (buffer-file-name buf) :null))
           (point   . ,(window-point node))
           (wstart  . ,(window-start node))
           (focused . ,(eq node (selected-window)))))
       ;; Internal split node: recurse on all children (cddr skips DIR and EDGES)
       `((type     . "split")
         (dir      . ,(if (car node) "v" "h"))
         (children . ,(apply #'vector (mapcar #'ser (cddr node))))))))
  (json-encode (ser (car (window-tree)))))
"#;

// ── Serde shapes ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum EmacsNode {
    Leaf {
        left: i32, top: i32, right: i32, bottom: i32,
        buf: String,
        file: Option<String>,
        point: u64,
        wstart: u64,
        focused: bool,
    },
    Split {
        dir: String,             // "v" = SplitAxis::Vertical, "h" = Horizontal
        children: Vec<EmacsNode>,
    },
}

impl EmacsNode {
    fn into_app_layout(self) -> AppLayout<EmacsWindowId> {
        match self {
            EmacsNode::Leaf { left, top, right, bottom, focused, .. } =>
                AppLayout::Pane {
                    native_id:  EmacsWindowId { left, top, right, bottom },
                    rect:       Rect { x: left, y: top, w: right - left, h: bottom - top },
                    is_focused: focused,
                },
            EmacsNode::Split { dir, children } =>
                AppLayout::Split {
                    axis:     if dir == "v" { SplitAxis::Vertical } else { SplitAxis::Horizontal },
                    children: children.into_iter().map(EmacsNode::into_app_layout).collect(),
                },
        }
    }
}

// ── TopologyProvider ──────────────────────────────────────────────────────────

impl TopologyProvider for EmacsAdapter {
    type NativeId = EmacsWindowId;
    type Error    = EmacsError;

    fn fetch_layout(&self) -> Result<AppLayout<Self::NativeId>, Self::Error> {
        let json = self.eval(SERIALIZE_TREE_LISP)?;
        let node: EmacsNode = serde_json::from_str(&json)
            .map_err(|e| EmacsError::ParseFailed(e.to_string()))?;
        Ok(node.into_app_layout())
    }
}

// ── TopologyModifierImpl ──────────────────────────────────────────────────────

impl TopologyModifierImpl for EmacsAdapter {
    fn focus_impl(&mut self, id: &Self::NativeId) -> Result<(), Self::Error> {
        self.eval(&format!("(select-window {})", id.as_lisp_selector()))?;
        Ok(())
    }

    fn move_impl(&mut self, id: &Self::NativeId, dir: Cardinal) -> Result<(), Self::Error> {
        let emacs_dir = match dir {
            Cardinal::West  => "left",
            Cardinal::East  => "right",
            Cardinal::North => "above",
            Cardinal::South => "below",
        };
        // window-swap-states exchanges buffer + point + scroll between two windows
        self.eval(&format!(
            "(let ((w {}))
               (when-let ((n (window-in-direction '{} w)))
                 (window-swap-states w n)))",
            id.as_lisp_selector(), emacs_dir
        ))?;
        Ok(())
    }

    fn tear_off_impl(&mut self, id: &Self::NativeId) -> Result<Box<dyn PaneState>, Self::Error> {
        let sel = id.as_lisp_selector();

        // Capture state BEFORE destroying the window.
        let json = self.eval(&format!(
            r#"(let* ((w   {sel})
                      (buf (window-buffer w)))
                 (json-encode
                   `((buf    . ,(buffer-name buf))
                     (file   . ,(or (buffer-file-name buf) :null))
                     (point  . ,(window-point w))
                     (wstart . ,(window-start w)))))"#
        ))?;

        #[derive(Deserialize)]
        struct Raw { buf: String, file: Option<String>, point: u64, wstart: u64 }
        let raw: Raw = serde_json::from_str(&json)
            .map_err(|e| EmacsError::ParseFailed(e.to_string()))?;

        // Now destroy. If this is the last window in the frame, error before destroying.
        self.eval(&format!(
            "(if (one-window-p t) \
               (error \"Cannot tear off last window\") \
               (delete-window {sel}))"
        ))?;

        Ok(Box::new(EmacsBufferState {
            buffer_name:  raw.buf,
            file_path:    raw.file,
            point:        raw.point,
            window_start: raw.wstart,
        }))
    }

    fn merge_in_impl(
        &mut self,
        target: &Self::NativeId,
        dir: Cardinal,
        payload: Box<dyn PaneState>,
    ) -> Result<Self::NativeId, Self::Error> {

        // Capability negotiation: prefer a native EmacsBufferState, fall back to shell.
        let open_buffer_lisp = if let Ok(s) = payload.as_any().downcast::<EmacsBufferState>() {
            if let Some(path) = &s.file_path {
                // Restore exact cursor and scroll position
                format!(
                    r#"(let ((b (find-file-noselect "{}")))
                         (with-current-buffer b
                           (goto-char {})
                           b))"#,
                    path.replace('"', "\\\""), s.point
                )
            } else {
                format!(r#"(get-buffer-create "{}")"#, s.buffer_name.replace('"', "\\\""))
            }
        } else {
            // Generic fallback: open an ansi-term shell
            r#"(ansi-term (getenv "SHELL"))"#.to_string()
        };

        // In Emacs: split-window nil = horizontal (side-by-side), t = vertical (stacked)
        let split_dir_lisp = match dir {
            Cardinal::West | Cardinal::East  => "nil",
            Cardinal::North | Cardinal::South => "t",
        };

        let sel = target.as_lisp_selector();
        let new_win_json = self.eval(&format!(
            r#"(let* ((target  {sel})
                      (buf     {open_buffer_lisp})
                      (new-win (split-window target nil {split_dir_lisp})))
                 (set-window-buffer new-win buf)
                 (let ((e (window-pixel-edges new-win)))
                   (json-encode
                     `((left   . ,(nth 0 e)) (top    . ,(nth 1 e))
                       (right  . ,(nth 2 e)) (bottom . ,(nth 3 e))))))"#
        ))?;

        #[derive(Deserialize)]
        struct NewWin { left: i32, top: i32, right: i32, bottom: i32 }
        let nw: NewWin = serde_json::from_str(&new_win_json)
            .map_err(|e| EmacsError::ParseFailed(e.to_string()))?;

        Ok(EmacsWindowId { left: nw.left, top: nw.top, right: nw.right, bottom: nw.bottom })
    }
}

// ── TilingDomain ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct FrameRect { x: i32, y: i32, w: i32, h: i32 }

impl TilingDomain for EmacsAdapter {
    fn domain_name(&self) -> &'static str { "emacs" }

    fn rect(&self) -> Rect {
        self.eval(
            r#"(json-encode `((x . 0) (y . 0)
                               (w . ,(frame-pixel-width))
                               (h . ,(frame-pixel-height))))"#
        )
        .ok()
        .and_then(|j| serde_json::from_str::<FrameRect>(&j).ok())
        .map(|f| Rect { x: f.x, y: f.y, w: f.w, h: f.h })
        .unwrap_or_default()
    }

    fn supported_payload_types(&self) -> &[TypeId] {
        static TYPES: &[TypeId] = &[TypeId::of::<EmacsBufferState>()]; // evaluated at compile time
        TYPES
    }
}
```


***

## What the Compiler Now Enforces for the Next Contributor

If someone adds a Neovim plugin following this exact contract, the compiler will:

- **Refuse to compile** if `tear_off_impl` returns `()` instead of `Box<dyn PaneState>` — a Neovim buffer cannot silently disappear without yielding state.
- **Refuse to compile** if `merge_in_impl` omits the `Cardinal` direction parameter — a pane cannot be injected without specifying where to split.
- **Emit a `#[must_use]` warning at the call site** if the orchestrator daemon calls `emacs.tear_off(id)` and ignores the returned `TopologyChanged` token — the Arena will never silently go stale.
- **Refuse to compile** if the contributor tries to construct `TopologyChanged` manually — they implement only `_impl` variants, which return no token at all.

---

# alright, now im curious how would you implement it a i3 integration

Three design challenges arise in i3 that did not exist in the Emacs integration, and they shape the entire implementation:

1. **`NativeId` is a stable integer**, not a bounding box. i3 container IDs (`con_id`) are stable for the session's lifetime, unlike Emacs window objects which have no persistent integer identity across IPC calls.
2. **Tear-off must not kill the process.** When you "tear off" an i3 window to hand it to another domain, you cannot `close` it. The correct i3 primitive is `move scratchpad` — the window goes invisible but the process stays alive and can be recalled by `merge_in`.
3. **i3's tree has non-slicing nodes.** `stacked` and `tabbed` layouts are structurally overlapping — not part of the Slicing Tree model. The converter must handle these explicitly rather than silently mapping them to the wrong axis.

***

## The i3 IPC Protocol

i3 uses a Unix domain socket with a custom framing protocol. Rather than depending on an external crate, the raw protocol is simple enough to implement directly. This keeps the plugin self-contained.

```
┌──────────────────────────────────────────────────────────────────┐
│  "i3-ipc"  │  payload_len (u32 LE)  │  msg_type (u32 LE)  │ ... │
│  6 bytes   │       4 bytes          │      4 bytes         │     │
└──────────────────────────────────────────────────────────────────┘
```


***

## Full Implementation

```rust
// plugins/i3/src/lib.rs

use std::any::TypeId;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::Command;
use serde::Deserialize;
use core::{
    AppLayout, Cardinal, IpcAdapter, PaneState, Rect, SplitAxis,
    TilingDomain, TopologyModifierImpl, TopologyProvider,
};

// ── Config ────────────────────────────────────────────────────────────────────

pub struct I3Config {
    /// If None, auto-detect via $I3SOCK then `i3 --get-socketpath`.
    pub socket_path:  Option<PathBuf>,
    /// Spawned when merging a non-GUI payload (e.g. EditorBufferState) into i3.
    pub terminal_cmd: String,
}
impl Default for I3Config {
    fn default() -> Self {
        Self { socket_path: None, terminal_cmd: "wezterm".into() }
    }
}

// ── NativeId ──────────────────────────────────────────────────────────────────
// i3 container IDs are i64s, stable for the full session lifetime.
// This is the key difference from Emacs, where we had to use pixel edges.
pub type I3ConId = i64;

// ── PaneState ─────────────────────────────────────────────────────────────────

/// The "currency" emitted by tear_off_impl when handing an i3 window
/// to another domain — or back to i3 via merge_in_impl.
#[derive(Debug, Clone)]
pub struct I3WindowState {
    /// The scratchpad con_id — the process is still alive, just hidden.
    pub con_id:        I3ConId,
    pub x11_window_id: Option<u32>,
    pub title:         String,
    pub class:         Option<String>,
    pub instance:      Option<String>,
    pub workspace:     String,
    pub rect:          Rect,
}
impl PaneState for I3WindowState {
    fn as_any(self: Box<Self>) -> Box<dyn std::any::Any> { self }
}

// ── Error ─────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum I3Error {
    Io(std::io::Error),
    SocketNotFound,
    ParseFailed(String),
    CommandFailed(String),
    WindowNotFound(I3ConId),
    EmptyTree,
}
impl std::fmt::Display for I3Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{:?}", self) }
}
impl std::error::Error for I3Error {}

// ── Raw i3 IPC socket ─────────────────────────────────────────────────────────

const MAGIC: &[u8; 6] = b"i3-ipc";

#[repr(u32)]
#[derive(Clone, Copy)]
enum I3MsgType { RunCommand = 0, GetTree = 4 }

struct I3Socket(UnixStream);

impl I3Socket {
    fn open(path: &std::path::Path) -> Result<Self, I3Error> {
        UnixStream::connect(path).map(I3Socket).map_err(I3Error::Io)
    }

    fn send(&mut self, msg: I3MsgType, payload: &[u8]) -> Result<Vec<u8>, I3Error> {
        // ── Write request ──────────────────────────────────────────────────
        let mut req = Vec::with_capacity(14 + payload.len());
        req.extend_from_slice(MAGIC);
        req.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        req.extend_from_slice(&(msg as u32).to_le_bytes());
        req.extend_from_slice(payload);
        self.0.write_all(&req).map_err(I3Error::Io)?;

        // ── Read response header ───────────────────────────────────────────
        let mut hdr = [0u8; 14];
        self.0.read_exact(&mut hdr).map_err(I3Error::Io)?;
        // Bytes [6..10] = response body length
        let body_len = u32::from_le_bytes(hdr[6..10].try_into().unwrap()) as usize;

        // ── Read response body ─────────────────────────────────────────────
        let mut body = vec![0u8; body_len];
        self.0.read_exact(&mut body).map_err(I3Error::Io)?;
        Ok(body)
    }
}

// ── Adapter ───────────────────────────────────────────────────────────────────

pub struct I3Adapter {
    socket_path:  PathBuf,
    terminal_cmd: String,
}

impl I3Adapter {
    fn socket(&self) -> Result<I3Socket, I3Error> {
        I3Socket::open(&self.socket_path)
    }

    fn run_command(&self, cmd: &str) -> Result<(), I3Error> {
        let body = self.socket()?.send(I3MsgType::RunCommand, cmd.as_bytes())?;
        #[derive(Deserialize)]
        struct CmdResult { success: bool, error: Option<String> }
        let results: Vec<CmdResult> = serde_json::from_slice(&body)
            .map_err(|e| I3Error::ParseFailed(e.to_string()))?;
        let errors: Vec<_> = results.iter()
            .filter(|r| !r.success)
            .filter_map(|r| r.error.as_deref())
            .collect();
        if !errors.is_empty() {
            return Err(I3Error::CommandFailed(errors.join("; ")));
        }
        Ok(())
    }

    fn get_tree(&self) -> Result<I3Node, I3Error> {
        let body = self.socket()?.send(I3MsgType::GetTree, b"")?;
        serde_json::from_slice(&body).map_err(|e| I3Error::ParseFailed(e.to_string()))
    }
}

// ── Serde shapes for GET_TREE ─────────────────────────────────────────────────

#[derive(Deserialize, Clone)]
struct I3Rect { x: i32, y: i32, width: i32, height: i32 }

#[derive(Deserialize, Clone)]
struct I3WindowProps {
    class:    Option<String>,
    instance: Option<String>,
    title:    Option<String>,
}

#[derive(Deserialize, Clone)]
struct I3Node {
    id:                 i64,
    #[serde(rename = "type")]
    node_type:          Option<String>,   // "root"|"output"|"workspace"|"con"|"floating_con"
    name:               Option<String>,
    layout:             Option<String>,   // "splith"|"splitv"|"stacked"|"tabbed"
    rect:               Option<I3Rect>,
    focused:            bool,
    #[serde(default)]
    focus:              Vec<i64>,          // MRU-ordered list of focused child con_ids
    window:             Option<u32>,       // X11 window ID; only present on leaf containers
    window_properties:  Option<I3WindowProps>,
    #[serde(default)]
    nodes:              Vec<I3Node>,
    #[serde(default)]
    floating_nodes:     Vec<I3Node>,
}

impl I3Node {
    fn to_rect(&self) -> Rect {
        self.rect.as_ref()
            .map(|r| Rect { x: r.x, y: r.y, w: r.width, h: r.height })
            .unwrap_or_default()
    }

    /// Recursively convert the i3 tree into our AppLayout.
    ///
    /// Key edge cases:
    /// - `root`/`output`/`dockarea` nodes: structural wrappers, collapse transparently.
    /// - `workspace`: the real root of meaningful i3 tiling.
    /// - `stacked`/`tabbed`: overlapping layouts — not Slicing Trees.
    ///   We still emit them as Vertical splits so the daemon has SOMETHING to
    ///   work with, but tag them so the Navigator knows not to offer
    ///   East/West movement between stacked children.
    fn into_app_layout(self) -> Option<AppLayout<I3ConId>> {
        match self.node_type.as_deref() {

            // ── Transparent wrappers ──────────────────────────────────────
            Some("root") | Some("output") | Some("dockarea") => {
                let children: Vec<_> = self.nodes.into_iter()
                    .filter_map(|n| n.into_app_layout())
                    .collect();
                match children.len() {
                    0 => None,
                    1 => children.into_iter().next(),
                    _ => Some(AppLayout::Split {
                        axis: SplitAxis::Horizontal, // outputs are side-by-side
                        children,
                    }),
                }
            }

            // ── Workspace: collapse its direct children ───────────────────
            Some("workspace") => {
                let children: Vec<_> = self.nodes.iter()
                    .filter_map(|n| n.clone().into_app_layout())
                    .collect();
                match children.len() {
                    0 => None,
                    1 => children.into_iter().next(),
                    _ => Some(AppLayout::Split {
                        axis: SplitAxis::Horizontal,
                        children,
                    }),
                }
            }

            _ => {
                // ── Leaf container: has an actual X11 window ─────────────
                if self.nodes.is_empty() {
                    return self.window.map(|_| AppLayout::Pane {
                        native_id:  self.id,
                        rect:       self.to_rect(),
                        is_focused: self.focused,
                    });
                }

                // ── Internal split container ──────────────────────────────
                let axis = match self.layout.as_deref() {
                    Some("splitv")  => SplitAxis::Vertical,
                    Some("stacked") => SplitAxis::Vertical,   // overlapping; treated as column
                    Some("tabbed")  => SplitAxis::Horizontal, // overlapping; treated as row
                    _               => SplitAxis::Horizontal, // splith is the default
                };
                let children: Vec<_> = self.nodes.into_iter()
                    .filter_map(|n| n.into_app_layout())
                    .collect();
                if children.is_empty() { return None; }
                Some(AppLayout::Split { axis, children })
            }
        }
    }

    // ── Tree query helpers ────────────────────────────────────────────────────

    fn find_node(&self, target: I3ConId) -> Option<&I3Node> {
        if self.id == target { return Some(self); }
        self.nodes.iter().find_map(|n| n.find_node(target))
    }

    fn find_workspace_for(&self, target: I3ConId) -> Option<String> {
        fn search(node: &I3Node, target: I3ConId, ws: Option<&str>) -> Option<String> {
            let current_ws = if node.node_type.as_deref() == Some("workspace") {
                node.name.as_deref()
            } else { ws };
            if node.id == target { return current_ws.map(str::to_owned); }
            node.nodes.iter().find_map(|n| search(n, target, current_ws))
        }
        search(self, target, None)
    }

    /// Walk the focus-order arrays to find the deepest currently-focused leaf.
    fn focused_leaf(&self) -> Option<I3ConId> {
        if self.nodes.is_empty() { return self.window.map(|_| self.id); }
        let focused_child_id = self.focus.first()?;
        let child = self.nodes.iter().find(|n| n.id == *focused_child_id)?;
        child.focused_leaf()
    }
}

// ── IpcAdapter ────────────────────────────────────────────────────────────────

impl IpcAdapter for I3Adapter {
    type Config = I3Config;
    type Error  = I3Error;

    fn connect(cfg: &Self::Config) -> Result<Self, Self::Error> {
        let path = match &cfg.socket_path {
            Some(p) => p.clone(),
            None => {
                if let Ok(p) = std::env::var("I3SOCK") { PathBuf::from(p) }
                else {
                    let out = Command::new("i3").arg("--get-socketpath")
                        .output().map_err(I3Error::Io)?;
                    let s = String::from_utf8_lossy(&out.stdout).trim().to_owned();
                    if s.is_empty() { return Err(I3Error::SocketNotFound); }
                    PathBuf::from(s)
                }
            }
        };
        let adapter = I3Adapter { socket_path: path, terminal_cmd: cfg.terminal_cmd.clone() };
        adapter.ping()?;
        Ok(adapter)
    }

    fn ping(&self) -> Result<(), Self::Error> { self.get_tree().map(|_| ()) }
}

// ── TopologyProvider ──────────────────────────────────────────────────────────

impl TopologyProvider for I3Adapter {
    type NativeId = I3ConId;
    type Error    = I3Error;

    fn fetch_layout(&self) -> Result<AppLayout<Self::NativeId>, Self::Error> {
        self.get_tree()?
            .into_app_layout()
            .ok_or(I3Error::EmptyTree)
    }
}

// ── TopologyModifierImpl ──────────────────────────────────────────────────────

impl TopologyModifierImpl for I3Adapter {
    fn focus_impl(&mut self, id: &I3ConId) -> Result<(), I3Error> {
        self.run_command(&format!("[con_id={}] focus", id))
    }

    fn move_impl(&mut self, id: &I3ConId, dir: Cardinal) -> Result<(), I3Error> {
        let d = match dir {
            Cardinal::West  => "left",
            Cardinal::East  => "right",
            Cardinal::North => "up",
            Cardinal::South => "down",
        };
        // i3 move commands act on the focused container — focus first, then move.
        self.run_command(&format!("[con_id={}] focus", id))?;
        self.run_command(&format!("move {}", d))
    }

    fn tear_off_impl(&mut self, id: &I3ConId) -> Result<Box<dyn PaneState>, I3Error> {
        let tree = self.get_tree()?;
        let node = tree.find_node(*id).ok_or(I3Error::WindowNotFound(*id))?;
        let props = node.window_properties.clone().unwrap_or(I3WindowProps {
            class: None, instance: None, title: None
        });

        let state = Box::new(I3WindowState {
            con_id:        *id,
            x11_window_id: node.window,
            title:         props.title.unwrap_or_default(),
            class:         props.class,
            instance:      props.instance,
            workspace:     tree.find_workspace_for(*id).unwrap_or_default(),
            rect:          node.to_rect(),
        });

        // Critical: do NOT kill the window. Move it to the scratchpad.
        // The process stays alive; it is simply detached from the visible tiling layout.
        // merge_in_impl can recall it from the scratchpad by con_id.
        self.run_command(&format!("[con_id={}] move scratchpad", id))?;

        Ok(state)
    }

    fn merge_in_impl(
        &mut self,
        target: &I3ConId,
        dir: Cardinal,
        payload: Box<dyn PaneState>,
    ) -> Result<I3ConId, I3Error> {
        // Set the split direction on the target container before inserting.
        self.run_command(&format!("[con_id={}] focus", target))?;
        self.run_command(match dir {
            Cardinal::West | Cardinal::East  => "split h",
            Cardinal::North | Cardinal::South => "split v",
        })?;

        // ── Case 1: Native I3WindowState — recall from scratchpad ─────────
        // This is a zero-copy operation: the original process reappears in place.
        if let Ok(state) = payload.as_any().downcast::<I3WindowState>() {
            self.run_command(&format!("[con_id={}] scratchpad show", state.con_id))?;
            self.run_command(&format!("[con_id={}] move window to mark __current",
                                      state.con_id))?;
            return Ok(state.con_id);
        }

        // ── Case 2: EditorBufferState — spawn emacsclient -c ─────────────
        // The PayloadRegistry in the core daemon converts this before calling merge_in,
        // but as a fallback we can handle it inline here.
        if let Ok(state) = payload.as_any().downcast::<core::EditorBufferState>() {
            let file  = state.file_uri.replace('"', "\\\"");
            let point = state.line; // approximate: Emacs line, not byte offset
            Command::new("emacsclient")
                .args(["-c", "-n", &format!("+{}", point), &file])
                .spawn().map_err(I3Error::Io)?;
            return self.wait_for_new_focused_leaf();
        }

        // ── Case 3: TerminalSessionState — attach in terminal ─────────────
        if let Ok(state) = payload.as_any().downcast::<core::TerminalSessionState>() {
            let attach_cmd = format!("tmux attach-session -t {}", state.session_name);
            Command::new(&self.terminal_cmd)
                .args(["start", "--", "sh", "-c", &attach_cmd])
                .spawn().map_err(I3Error::Io)?;
            return self.wait_for_new_focused_leaf();
        }

        // ── Case 4: Unknown payload — spawn a bare terminal ───────────────
        Command::new(&self.terminal_cmd).spawn().map_err(I3Error::Io)?;
        self.wait_for_new_focused_leaf()
    }
}

impl I3Adapter {
    /// Poll for the new focused leaf after spawning an external process.
    /// In production this should use i3's SUBSCRIBE / window::new event instead.
    fn wait_for_new_focused_leaf(&self) -> Result<I3ConId, I3Error> {
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            let tree = self.get_tree()?;
            if let Some(id) = tree.focused_leaf() { return Ok(id); }
        }
        Err(I3Error::ParseFailed("no focused leaf appeared after 2s".into()))
    }
}

// ── TilingDomain ──────────────────────────────────────────────────────────────

impl TilingDomain for I3Adapter {
    fn domain_name(&self) -> &'static str { "i3" }

    fn rect(&self) -> Rect {
        self.get_tree().ok()
            .and_then(|t| t.rect)
            .map(|r| Rect { x: r.x, y: r.y, w: r.width, h: r.height })
            .unwrap_or_default()
    }

    fn supported_payload_types(&self) -> &[TypeId] {
        static TYPES: &[TypeId] = &[TypeId::of::<I3WindowState>()];
        TYPES
    }
}
```


***

## Key Differences from the Emacs Plugin

| Concern | Emacs | i3 |
| :-- | :-- | :-- |
| **NativeId type** | `EmacsWindowId { left, top, right, bottom }` — bounding box as identity because Emacs has no stable integer window ID across IPC | `i64` — i3 container IDs are stable for the full session lifetime |
| **Tear-off strategy** | `delete-window` — buffer stays alive in Emacs's buffer list, only the window display is destroyed | `move scratchpad` — the X11 process stays alive, merely hidden; `merge_in` recalls it by `con_id` |
| **Tree shape quirk** | Binary splits only; Emacs `window-tree` maps cleanly to `SplitAxis` | Has `stacked`/`tabbed` layouts that are overlapping, not slicing; must be gracefully degraded |
| **IPC transport** | `emacsclient --eval` (subprocess) — a new process per call | Unix socket with a binary framing protocol — one persistent connection per message |
| **Spawn → NativeId** | N/A (Emacs creates windows internally via Lisp) | Must poll `focused_leaf()` after spawning an external process, since the new `con_id` is not known until i3 assigns it |

The `wait_for_new_focused_leaf` polling is the one honest rough edge. In production it should be replaced with a subscription to i3's `SUBSCRIBE` event stream (`window` events, specifically the `new` type), which delivers the new `con_id` as a push notification rather than requiring a spin-wait.

---

# im curious to now see the solver code

Here is the implementation of the geometry-based solver code for directional movement.

The fundamental insight of this algorithm is that **we do not traverse trees** to find the next window. Trees differ wildly between apps (Grid, BSP, Stacked), making cross-domain tree traversal almost impossible. Instead, we project every window from every application onto a flat 2D coordinate plane and perform a purely mathematical spatial query.[^9_1][^9_2]

This is the exact heuristic used by production game engines and advanced tiling window managers like `leftwm` and `komorebi`.[^9_2][^9_1]

### The Core Algorithm

The solver must answer this question: *"Given a currently focused window $W$ and a direction $D$, out of all other windows on the screen, which one should receive focus?"*

To solve this, we filter the list of all windows through three strict rules:

1. **Directional Validity:** The candidate's "receiving edge" must be strictly in the requested direction relative to the focused window's "leading edge" (e.g., if moving East, the target's left edge must be $\ge$ my right edge).
2. **Perpendicular Overlap:** The candidate must horizontally or vertically align with the focused window. If moving East, a window that is far above or below us is not a valid neighbor; it must share some Y-axis space.
3. **Proximity:** Out of all candidates that survived the first two filters, pick the one with the shortest physical distance.

### The Rust Implementation

```rust
use std::collections::HashMap;

/// Represents the cardinal direction the user pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cardinal { North, South, East, West }

/// A simple 2D bounding box representing physical screen pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    /// The physical X/Y coordinate of the edge pointing toward `dir`.
    /// For example, if we are moving East, our leading edge is our Right side (x + w).
    pub fn leading_edge(&self, dir: Cardinal) -> i32 {
        match dir {
            Cardinal::East  => self.x + self.w,
            Cardinal::West  => self.x,
            Cardinal::South => self.y + self.h,
            Cardinal::North => self.y,
        }
    }

    /// The physical X/Y coordinate of the edge receiving an incoming focus.
    /// For example, if focus is moving East, the target window receives it on its Left side (x).
    pub fn receiving_edge(&self, dir: Cardinal) -> i32 {
        match dir {
            Cardinal::East  => self.x,
            Cardinal::West  => self.x + self.w,
            Cardinal::South => self.y,
            Cardinal::North => self.y + self.h,
        }
    }

    /// Checks if this Rect shares spatial alignment with `other` along the axis 
    /// perpendicular to the movement direction.
    /// If moving East/West, we check if their Y-boundaries overlap.
    pub fn perp_overlap(&self, other: &Rect, dir: Cardinal) -> bool {
        match dir {
            Cardinal::East | Cardinal::West => {
                // Y-axis overlap
                self.y < other.y + other.h && self.y + self.h > other.y
            }
            Cardinal::North | Cardinal::South => {
                // X-axis overlap
                self.x < other.x + other.w && self.x + self.w > other.x
            }
        }
    }
}

/// A flat, domain-agnostic representation of a pane anywhere on the screen.
#[derive(Clone, Debug)]
pub struct GlobalLeaf {
    pub id: String,         // Opaque ID (e.g., "i3:12345" or "emacs:win_3")
    pub domain_id: String,  // Which plugin owns this ("i3", "wezterm", "emacs")
    pub rect: Rect,
}

pub struct FocusSolver {
    pub all_leaves: Vec<GlobalLeaf>,
}

impl FocusSolver {
    /// Solves for the next window to focus based purely on geometry.
    pub fn find_neighbor(&self, focused_id: &str, dir: Cardinal) -> Option<&GlobalLeaf> {
        // 1. Find the currently focused window
        let focused = self.all_leaves.iter().find(|l| l.id == focused_id)?;
        let my_edge = focused.rect.leading_edge(dir);

        let mut candidates: Vec<&GlobalLeaf> = self.all_leaves
            .iter()
            .filter(|l| l.id != focused.id) // Don't focus ourselves
            .filter(|l| {
                // 2. Directional Validity: 
                // The target's receiving edge must be physically past our leading edge.
                let target_edge = l.rect.receiving_edge(dir);
                match dir {
                    Cardinal::East | Cardinal::South => target_edge >= my_edge,
                    Cardinal::West | Cardinal::North => target_edge <= my_edge,
                }
            })
            .filter(|l| {
                // 3. Perpendicular Overlap: 
                // We only want windows that are actually adjacent, not diagonally across the screen.
                focused.rect.perp_overlap(&l.rect, dir)
            })
            .collect();

        if candidates.is_empty() {
            return None;
        }

        // 4. Proximity: Sort candidates by how close they are to our leading edge
        candidates.sort_by_key(|l| {
            let distance = (l.rect.receiving_edge(dir) - my_edge).abs();
            distance
        });

        // 5. Tie-breaker: If multiple windows are equidistant (e.g., a vertical stack of windows 
        // directly to our right), pick the one that aligns closest to our top/left corner.
        let closest_distance = (candidates[^9_0].rect.receiving_edge(dir) - my_edge).abs();
        
        candidates.into_iter()
            .filter(|l| (l.rect.receiving_edge(dir) - my_edge).abs() == closest_distance)
            .min_by_key(|l| match dir {
                // If moving horizontally, tie-break by Y proximity
                Cardinal::East | Cardinal::West => (l.rect.y - focused.rect.y).abs(),
                // If moving vertically, tie-break by X proximity
                Cardinal::North | Cardinal::South => (l.rect.x - focused.rect.x).abs(),
            })
    }
}
```


### How the Daemon Orchestrates This

Once the geometric solver yields a target `GlobalLeaf`, the orchestrator daemon takes that result and dictates the cross-application behavior:

```rust
pub fn execute_move_west(focused_id: &str, solver: &FocusSolver, registry: &PluginRegistry) {
    let current_leaf = solver.all_leaves.iter().find(|l| l.id == focused_id).unwrap();
    
    // 1. Run the geometric solver
    if let Some(target_leaf) = solver.find_neighbor(focused_id, Cardinal::West) {
        
        // 2. Cross-Domain Routing
        if current_leaf.domain_id == target_leaf.domain_id {
            // Case A: The target window is inside the same application.
            // Action: Send an IPC command to just move focus normally.
            let plugin = registry.get(&current_leaf.domain_id);
            plugin.focus_impl(&target_leaf.id);
            
        } else {
            // Case B: We hit the boundary of the app, and the target is in a different app.
            // Action: Tell the new application to focus its specific window.
            let target_plugin = registry.get(&target_leaf.domain_id);
            target_plugin.focus_impl(&target_leaf.id);
        }
    } else {
        // Case C: The solver returned None. We are at the absolute edge of the monitor.
        // Action: Do nothing, or wrap around to the other side of the screen.
        println!("Hit the edge of the screen.");
    }
}
```

<span style="display:none">[^9_10][^9_11][^9_12][^9_13][^9_14][^9_15][^9_16][^9_17][^9_18][^9_3][^9_4][^9_5][^9_6][^9_7][^9_8][^9_9]</span>

<div align="center">⁂</div>

[^9_1]: https://github.com/leftwm/leftwm/issues/771

[^9_2]: https://www.youtube.com/watch?v=VqdUIVxWTlE

[^9_3]: https://edwardwibowo.com/blog/making-a-window-manager/

[^9_4]: https://krshrimali.github.io/posts/2022/07/porting-a-tiling-window-manager-extenstion-to-c-bismuth-part-2-getting-closest-relative-window/

[^9_5]: https://bbs.archlinux.org/viewtopic.php?id=64100

[^9_6]: https://github.com/ntrrgc/right-window

[^9_7]: https://rust-windowing.github.io/winit/winit/index.html

[^9_8]: https://www.reddit.com/r/archlinux/comments/jtmnk/techniques_for_using_tiling_window_managers/

[^9_9]: https://www.youtube.com/watch?v=N_chPAN8ibI

[^9_10]: https://github.com/drewcrawford/app_window

[^9_11]: https://lib.rs/config

[^9_12]: https://news.ycombinator.com/item?id=14482988

[^9_13]: http://www.fox-toolkit.org

[^9_14]: https://lwn.net/Articles/1005297/

[^9_15]: https://dev.to/siph/building-a-tiling-window-manager-with-rust-and-penrose-5863

[^9_16]: https://www.youtube.com/watch?v=E2cyTTaJzfY

[^9_17]: https://www.youtube.com/watch?v=dIwpTK6fyLk\&lc=UgxD-A_WVP0GrDRjwnZ4AaABAg

[^9_18]: https://www.youtube.com/watch?v=48DidRy_2vQ

