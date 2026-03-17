const browserFocusValues = [
  "ignore",
  "focus_previous_tab",
  "focus_next_tab",
  "focus_first_tab",
  "focus_last_tab",
];

const browserMoveValues = [
  "ignore",
  "move_tab_backward",
  "move_tab_forward",
  "move_tab_to_first_position",
  "move_tab_to_last_position",
];

const tearOffStrategies = [
  "only_if_edgemost",
  "once_it_neighbors_with_window_edge",
  "always",
];

const terminalMuxValues = ["tmux", "zellij", "wezterm", "kitty"];
const terminalHostTabValues = ["transparent", "focus", "native_full"];
const terminalScopeValues = ["mux_pane", "mux_window", "mux_session", "terminal_tab"];
const editorTerminalMuxValues = ["inherit", "tmux", "zellij", "wezterm", "kitty"];
const editorScopeValues = ["buffer", "window", "workspace"];
const directionValues = ["W", "E", "N", "S"];

const pageOrder = ["terminal", "editor", "browser"];

const profileRoots = {
  browser: "[app.browser.<profile>]",
  terminal: "[app.terminal.<profile>]",
  editor: "[app.editor.<profile>]",
};

const kindLabels = {
  browser: "Browser",
  terminal: "Terminal",
  editor: "Editor",
};

const directionTitles = {
  left: "left",
  right: "right",
  up: "up",
  down: "down",
};

const directionWords = {
  left: "West",
  right: "East",
  up: "North",
  down: "South",
};

const field = (kind, groupId, slug, configPath, details) => ({
  kind,
  kindLabel: kindLabels[kind],
  groupId,
  slug,
  configPath,
  fullPath: `${profileRoots[kind]} ${configPath}`,
  compositionId: `${kind}-${slug}`,
  assetPath: `assets/generated/${kind}/${slug}.mp4`,
  ...details,
});

const anchorNote =
  "The field is part of the public config surface, but there is no obvious runtime consumer outside config.rs today, so the docs describe the declared policy rather than adapter-specific enforcement.";

const integrationField = (kind, groupId, surfaceLabel) =>
  field(kind, groupId, "enabled", "enabled", {
    title: "enabled",
    defaultValue: "false",
    summary: `Turns ${surfaceLabel} integration on for matching ${kind} profiles.`,
    behavior:
      "Until this is true, the matching profile stays out of directional routing and the orchestrator falls back to outer layers.",
    values: [
      "true: allow the profile to participate in focus/move/resize handling",
      "false: keep the profile inactive even if aliases match",
    ],
    scene: {
      template: "integration-toggle",
      surfaceLabel,
      activeLabel: `${surfaceLabel} integration active`,
      inactiveLabel: `${surfaceLabel} integration disabled`,
    },
  });

const anchorField = (kind, groupId, surfaceLabel) =>
  field(kind, groupId, "anchor-app-window", "anchor_app_window", {
    title: "anchor_app_window",
    defaultValue: "false",
    summary: `Declares that the surrounding window manager should keep the ${surfaceLabel} window anchored instead of moving it around during routing.`,
    behavior:
      "Use it when you want app-level routing to stay internal and you do not want the outer WM to reposition the host window as part of the policy.",
    values: [
      "true: prefer a pinned/anchored outer window posture",
      "false: allow normal WM-level movement policy",
    ],
    note: anchorNote,
    scene: {
      template: "anchor-window",
      surfaceLabel,
    },
  });

const internalEnabledField = (kind, groupId, actionKey, actionLabel) =>
  field(kind, groupId, `${actionKey}-internal-enabled`, `${actionKey}.internal_panes.enabled`, {
    title: `${actionKey}.internal_panes.enabled`,
    defaultValue: "true",
    summary: `Lets the ${kind} profile own internal ${actionLabel} before the command escapes to the next layer.`,
    behavior:
      "When this flag is off, the matching adapter does not expose that internal capability even if the profile itself is enabled.",
    values: [
      `true: keep ${actionLabel} inside the ${kind} pane/split graph when possible`,
      `false: skip straight to the surrounding layer for ${actionLabel}`,
    ],
    scene: {
      template: "internal-enabled",
      actionLabel,
      kind,
    },
  });

const allowedDirectionsField = (kind, groupId, actionKey, actionLabel) =>
  field(kind, groupId, `${actionKey}-allowed-directions`, `${actionKey}.internal_panes.allowed_directions`, {
    title: `${actionKey}.internal_panes.allowed_directions`,
    defaultValue: "all directions when unset",
    summary: `Restricts internal ${actionLabel} to the listed directions only.`,
    behavior:
      "The capability stays enabled, but directions that are not in the list behave as if the adapter declined the request and the orchestrator escalates outward.",
    values: [
      "unset: allow every cardinal direction",
      `set a subset like [${directionValues.map((value) => `"${value}"`).join(", ")}] to keep only those directions internal`,
    ],
    scene: {
      template: "allowed-directions",
      actionLabel,
      kind,
    },
  });

const tearOffEnabledField = (kind, groupId, unitLabel) =>
  field(kind, groupId, "tear-off-enabled", "move.docking.tear_off.enabled", {
    title: "move.docking.tear_off.enabled",
    defaultValue: "true",
    summary: `Allows edge moves to tear the focused ${unitLabel} into a new window.`,
    behavior:
      "Turn this off when a move should stop at the boundary or hand back to an outer layer instead of creating a new window.",
    values: [
      `true: ${unitLabel} can break out into a separate window`,
      `false: disable tear-out for ${unitLabel} moves`,
    ],
    scene: {
      template: "tear-off-toggle",
      kind,
      unitLabel,
    },
  });

const tearOffStrategyField = (kind, groupId, unitLabel) =>
  field(kind, groupId, "tear-off-strategy", "move.docking.tear_off.strategy", {
    title: "move.docking.tear_off.strategy",
    defaultValue: "only_if_edgemost",
    summary: `Chooses how aggressively the focused ${unitLabel} is allowed to tear out once a move hits the boundary.`,
    behavior:
      "The stricter strategies require a cleaner edge condition before the tear-out path is even considered.",
    values: tearOffStrategies,
    scene: {
      template: "tear-off-strategy",
      kind,
      unitLabel,
    },
  });

const snapBackField = (kind, groupId, unitLabel) =>
  field(kind, groupId, "snap-back", "move.docking.snap_back", {
    title: "move.docking.snap_back",
    defaultValue: "true",
    summary: `Lets a torn-out ${unitLabel} merge back into a compatible target instead of staying permanently detached.`,
    behavior:
      "With snap-back disabled, repeated moves toward a matching target stop short of the in-app merge path and leave the torn-out window independent.",
    values: [
      `true: allow merge-back for torn-out ${unitLabel}s`,
      "false: keep tear-outs as separate windows",
    ],
    scene: {
      template: "snap-back",
      kind,
      unitLabel,
    },
  });

const browserDirectionField = (actionKey, direction, actionValues, defaultNote) =>
  field(
    "browser",
    actionKey,
    `${actionKey}-${direction}`,
    `${actionKey}.${directionTitles[direction]}`,
    {
      title: `${actionKey}.${directionTitles[direction]}`,
      defaultValue: `unset (${defaultNote})`,
      summary: `Overrides the ${directionWords[direction]} ${actionKey === "focus" ? "focus" : "move"} result for the browser profile.`,
      behavior:
        actionKey === "focus"
          ? "If this field is unset, the browser falls back to tab_axis for that direction. Once it is set, the explicit per-direction action wins."
          : "If this field is unset, the browser falls back to tab_axis for that direction. Once it is set, the explicit per-direction tab-reordering action wins.",
      values: actionValues,
      scene: {
        template: "browser-direction-action",
        actionKind: actionKey,
        direction,
        defaultNote,
      },
    },
  );

const terminalScopeField = (slug, configPath, summary, note) =>
  field("terminal", "move", slug, configPath, {
    title: configPath,
    defaultValue: "unset",
    summary,
    behavior:
      "The chosen scope decides what the directional tear-out operation treats as the movable unit before it becomes a new window.",
    values: terminalScopeValues,
    note,
    scene: {
      template: "terminal-scope",
    },
  });

const terminalIgnoreTabsField = (actionKey, actionLabel) =>
  field("terminal", actionKey, `${actionKey}-ignore-tabs`, `${actionKey}.ignore_tabs`, {
    title: `${actionKey}.ignore_tabs`,
    defaultValue: "true",
    summary: `Legacy fallback for host-tab ${actionLabel} when host_tabs is not set.`,
    behavior:
      `Set this to false to let edge ${actionLabel} continue into the next host tab in the old pre-host_tabs style. If host_tabs is set explicitly, that newer mode wins instead.`,
    values: [
      `true: ignore host tabs for ${actionLabel} when host_tabs is unset`,
      `false: allow host-tab ${actionLabel} as the legacy fallback`,
    ],
    scene: {
      template: "legacy-ignore-tabs",
      actionLabel,
    },
  });

const docsCatalog = {
  terminal: {
    kind: "terminal",
    label: "Terminal profiles",
    pageTitle: "Terminal configuration",
    heroTitle: "Document terminal hosts, mux backends, host tabs, and pane policy.",
    heroIntro:
      "Terminal profiles are where yeetnyoink decides how much of the directional model belongs to the host emulator, how much belongs to the mux, and when a pane becomes its own window.",
    sampleConfig: `[app.terminal.wezterm]\nenabled = true\nmux_backend = "wezterm"\nhost_tabs = "focus"\nfocus.internal_panes.enabled = true\nmove.internal_panes.enabled = true\nresize.internal_panes.enabled = true\nmove.docking.tear_off.enabled = true\nmove.docking.tear_off.strategy = "only_if_edgemost"\nmove.docking.snap_back = true`,
    sections: [
      {
        id: "activation",
        title: "Activation, mux selection, and host tabs",
        blurb:
          "These fields decide whether the terminal profile participates at all, which multiplexer implementation it delegates to, and whether host tabs join the directional model.",
        fields: [
          integrationField("terminal", "activation", "terminal routing"),
          anchorField("terminal", "activation", "terminal"),
          field("terminal", "activation", "mux-backend", "mux_backend", {
            title: "mux_backend",
            defaultValue: "alias-based default when unset",
            summary:
              "Selects the multiplexer backend the terminal profile uses for internal pane control.",
            behavior:
              "When unset, defaults are inferred from the terminal alias: kitty -> kitty, foot/alacritty/ghostty/iTerm -> tmux, everything else -> wezterm.",
            values: terminalMuxValues,
            scene: {
              template: "terminal-mux-backend",
            },
          }),
          field("terminal", "activation", "host-tabs", "host_tabs", {
            title: "host_tabs",
            defaultValue: "transparent",
            summary:
              "Controls whether terminal host tabs participate in edge focus and move routing.",
            behavior:
              "transparent disables host-tab routing, focus allows tab-to-tab focus moves, and native_full allows both focus and move across host tabs. When set, it overrides the legacy ignore_tabs flags.",
            values: terminalHostTabValues,
            scene: {
              template: "terminal-host-tabs",
            },
          }),
        ],
      },
      {
        id: "focus",
        title: "Focus policy",
        blurb:
          "Focus settings decide whether the terminal keeps focus changes inside the pane grid and which directions count as internal rather than outer-window navigation.",
        fields: [
          internalEnabledField("terminal", "focus", "focus", "focus"),
          allowedDirectionsField("terminal", "focus", "focus", "focus"),
          terminalIgnoreTabsField("focus", "focus"),
        ],
      },
      {
        id: "move",
        title: "Move, tear-out, and merge policy",
        blurb:
          "Move settings cover internal pane moves, the tear-out threshold, the movable scope, and whether torn-out panes can merge back later.",
        fields: [
          internalEnabledField("terminal", "move", "move", "move"),
          allowedDirectionsField("terminal", "move", "move", "move"),
          tearOffEnabledField("terminal", "move", "pane or tab unit"),
          tearOffStrategyField("terminal", "move", "pane or tab unit"),
          terminalScopeField(
            "tear-off-scope-nested",
            "move.docking.tear_off.scope",
            "Inline form for choosing the terminal tear-out scope inside the move.docking block.",
            "This is the nested spelling of the scope field. It describes the same scope choices as the top-level terminal shortcut."
          ),
          terminalScopeField(
            "tear-off-scope-top-level",
            "tear_off_scope",
            "Top-level shortcut for picking the default terminal tear-out scope.",
            "This is a terminal-host shortcut for the same scope concept exposed under move.docking.tear_off.scope."
          ),
          snapBackField("terminal", "move", "pane or tab unit"),
          terminalIgnoreTabsField("move", "move"),
        ],
      },
      {
        id: "resize",
        title: "Resize policy",
        blurb:
          "Resize settings decide whether the current terminal owns directional resizing and which directions stay inside the pane graph.",
        fields: [
          internalEnabledField("terminal", "resize", "resize", "resize"),
          allowedDirectionsField("terminal", "resize", "resize", "resize"),
        ],
      },
    ],
  },
  editor: {
    kind: "editor",
    label: "Editor profiles",
    pageTitle: "Editor configuration",
    heroTitle: "Document editor split control, UI mapping, and tear-off scope.",
    heroIntro:
      "Editor profiles decide how deep directional routing goes inside split layouts, whether terminal surfaces are part of the editor model, and which UI surfaces an editor can bind to.",
    sampleConfig: `[app.editor.neovim]\nenabled = true\ntear_off_scope = "buffer"\nmove.docking.tear_off.enabled = true\nmove.docking.snap_back = true\n\n[app.editor.neovim.ui.terminal]\napp = "wezterm"\nmux_backend = "inherit"`,
    sections: [
      {
        id: "activation",
        title: "Activation and high-level editor scope",
        blurb:
          "These fields turn the editor on, describe whether terminal content should be treated as part of the editor model, and pick the editor tear-out unit.",
        fields: [
          integrationField("editor", "activation", "editor routing"),
          anchorField("editor", "activation", "editor"),
          field("editor", "activation", "manage-terminal", "manage_terminal", {
            title: "manage_terminal",
            defaultValue: "false",
            summary:
              "Lets the editor adapter treat integrated terminal surfaces as part of the editor workflow instead of always leaving terminal handling to the outer terminal host.",
            behavior:
              "When this is on, adapters that support it can route focus, tear-out, and merge preparation through terminal-like surfaces inside the editor itself.",
            values: [
              "true: include supported embedded terminals in the editor model",
              "false: keep terminal surfaces outside editor-owned routing",
            ],
            note: "This flag has an active runtime consumer in the VS Code adapter today.",
            scene: {
              template: "editor-manage-terminal",
            },
          }),
          field("editor", "activation", "tear-off-scope", "tear_off_scope", {
            title: "tear_off_scope",
            defaultValue: "buffer",
            summary:
              "Chooses what an editor tear-out move lifts out of the current editor surface.",
            behavior:
              "buffer keeps the operation focused on one file/buffer, window moves an editor window/group unit, and workspace escalates to the whole workspace when the adapter supports it.",
            values: editorScopeValues,
            scene: {
              template: "editor-scope",
            },
          }),
        ],
      },
      {
        id: "focus",
        title: "Focus policy",
        blurb:
          "Focus settings decide whether directional focus stays inside editor splits and which directions the editor is allowed to handle internally.",
        fields: [
          internalEnabledField("editor", "focus", "focus", "focus"),
          allowedDirectionsField("editor", "focus", "focus", "focus"),
        ],
      },
      {
        id: "move",
        title: "Move, tear-out, and merge policy",
        blurb:
          "Move settings decide whether panes move inside the editor, when they may leave the editor as a new window, and whether they can merge back in later.",
        fields: [
          internalEnabledField("editor", "move", "move", "move"),
          allowedDirectionsField("editor", "move", "move", "move"),
          tearOffEnabledField("editor", "move", "buffer/window/workspace unit"),
          tearOffStrategyField("editor", "move", "buffer/window/workspace unit"),
          snapBackField("editor", "move", "buffer/window/workspace unit"),
        ],
      },
      {
        id: "resize",
        title: "Resize policy",
        blurb:
          "Resize settings control whether the editor owns split resizing and which directions remain internal instead of escaping to the outer window manager.",
        fields: [
          internalEnabledField("editor", "resize", "resize", "resize"),
          allowedDirectionsField("editor", "resize", "resize", "resize"),
        ],
      },
      {
        id: "ui",
        title: "Terminal and graphical UI binding",
        blurb:
          "UI settings let an editor describe where it lives: inside a terminal host, on a graphical surface, or both.",
        fields: [
          field("editor", "ui", "ui-terminal-app", "ui.terminal.app", {
            title: "ui.terminal.app",
            defaultValue: "unset",
            summary:
              "Pins the editor's terminal-hosted UI to a specific terminal app alias.",
            behavior:
              "When set, the editor chain can target that host explicitly instead of relying on ambient discovery. The value is normalized to a lowercase alias before use.",
            values: [
              "set a terminal alias like wezterm, kitty, alacritty, foot, ghostty, or iterm2",
              "leave unset to avoid forcing a terminal host target",
            ],
            scene: {
              template: "editor-terminal-app",
            },
          }),
          field("editor", "ui", "ui-terminal-mux-backend", "ui.terminal.mux_backend", {
            title: "ui.terminal.mux_backend",
            defaultValue: "unset",
            summary:
              "Chooses how the editor resolves the multiplexer for its terminal-hosted UI.",
            behavior:
              "inherit asks the config layer to resolve from ui.terminal.app or from the single enabled terminal backend. If the environment is ambiguous, inherit resolves to none until you make the choice explicit.",
            values: editorTerminalMuxValues,
            scene: {
              template: "editor-terminal-mux",
            },
          }),
          field("editor", "ui", "ui-graphical-app", "ui.graphical.app", {
            title: "ui.graphical.app",
            defaultValue: "unset",
            summary:
              "Pins the editor's graphical UI to a specific graphical app alias.",
            behavior:
              "Use it when the editor has a dedicated graphical surface you want the config layer to match directly. The value is normalized to a lowercase alias before use.",
            values: [
              "set a graphical app alias like vscode or emacs",
              "leave unset when the editor does not need a graphical surface binding",
            ],
            scene: {
              template: "editor-graphical-app",
            },
          }),
        ],
      },
    ],
  },
  browser: {
    kind: "browser",
    label: "Browser profiles",
    pageTitle: "Browser configuration",
    heroTitle: "Document tab-axis defaults, per-direction overrides, and tab tear-out policy.",
    heroIntro:
      "Browser profiles are flatter than terminal or editor profiles, but they still expose an important set of directional decisions: how tabs map to axes, which directions get explicit overrides, and whether tabs can tear out and merge back.",
    sampleConfig: `[app.browser.librewolf]\nenabled = true\ntab_axis = "vertical"\n\n[app.browser.librewolf.focus]\nleft = "focus_first_tab"\n\n[app.browser.librewolf.move]\nright = "move_tab_to_last_position"\n[app.browser.librewolf.move.docking.tear_off]\nenabled = true\nstrategy = "only_if_edgemost"`,
    sections: [
      {
        id: "activation",
        title: "Activation and default tab axis",
        blurb:
          "These fields decide whether the browser adapter is active at all and how the default axis maps directional intent to tab navigation or tab movement.",
        fields: [
          integrationField("browser", "activation", "browser tab routing"),
          anchorField("browser", "activation", "browser"),
          field("browser", "activation", "tab-axis", "tab_axis", {
            title: "tab_axis",
            defaultValue: "horizontal",
            summary:
              "Selects the default axis that maps directional commands to browser tab actions.",
            behavior:
              "horizontal keeps west/east as previous/next tab. vertical moves that mapping to north/south. vertical_flipped keeps the vertical axis but swaps which direction means previous vs next.",
            values: ["horizontal", "vertical", "vertical_flipped"],
            scene: {
              template: "browser-tab-axis",
            },
          }),
        ],
      },
      {
        id: "focus",
        title: "Per-direction focus overrides",
        blurb:
          "Each focus direction can override tab_axis explicitly. If a field is unset, that direction falls back to the tab_axis default.",
        fields: [
          browserDirectionField("focus", "left", browserFocusValues, "tab_axis decides the default West behavior"),
          browserDirectionField("focus", "right", browserFocusValues, "tab_axis decides the default East behavior"),
          browserDirectionField("focus", "up", browserFocusValues, "tab_axis decides the default North behavior"),
          browserDirectionField("focus", "down", browserFocusValues, "tab_axis decides the default South behavior"),
        ],
      },
      {
        id: "move",
        title: "Per-direction move overrides and docking",
        blurb:
          "Move directions can override tab reordering explicitly, and the docking block controls whether tabs may tear out into new windows and later merge back.",
        fields: [
          browserDirectionField("move", "left", browserMoveValues, "tab_axis decides the default West move behavior"),
          browserDirectionField("move", "right", browserMoveValues, "tab_axis decides the default East move behavior"),
          browserDirectionField("move", "up", browserMoveValues, "tab_axis decides the default North move behavior"),
          browserDirectionField("move", "down", browserMoveValues, "tab_axis decides the default South move behavior"),
          tearOffEnabledField("browser", "move", "tab"),
          tearOffStrategyField("browser", "move", "tab"),
          snapBackField("browser", "move", "tab"),
        ],
      },
    ],
  },
};

const allDocFields = pageOrder.flatMap((kind) =>
  docsCatalog[kind].sections.flatMap((section) => section.fields),
);

const docsPages = [
  { path: "index.html", title: "Overview" },
  { path: "terminal.html", title: docsCatalog.terminal.pageTitle },
  { path: "editor.html", title: docsCatalog.editor.pageTitle },
  { path: "browser.html", title: docsCatalog.browser.pageTitle },
  { path: "404.html", title: "Not found" },
];

const staticAssets = [
  "assets/site.css",
  "assets/site.js",
  "assets/favicon.svg",
  "assets/docs-catalog.mjs",
];

const docsManifest = {
  pages: docsPages.map((page) => page.path),
  staticAssets,
  videoAssets: allDocFields.map((fieldEntry) => fieldEntry.assetPath),
};

export { allDocFields, docsCatalog, docsManifest, docsPages, pageOrder };
