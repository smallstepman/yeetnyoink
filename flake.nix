{
  description = "yeet-and-yoink package and home-manager module";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-25.11";
  };

  outputs = { self, nixpkgs }:
    let
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-darwin"
        "x86_64-linux"
      ];

      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f (import nixpkgs { inherit system; }));

      mkPackage = pkgs: pkgs.rustPlatform.buildRustPackage {
        pname = "yeet-and-yoink";
        version = "0.1.0";
        src = ./.;
        cargoLock.lockFile = ./Cargo.lock;
        meta = with pkgs.lib; {
          description = "Deep focus/move integration between niri and apps";
          mainProgram = "yeet-and-yoink";
          platforms = platforms.all;
        };
      };

      hmModule = { config, lib, pkgs, ... }:
        let
          inherit (lib) literalExpression mkEnableOption mkOption mkIf;
          types = lib.types;
          cfg = config.programs.yeet-and-yoink;
          tomlFormat = pkgs.formats.toml {};
          pathStringType = types.coercedTo types.path toString types.str;

          optionalBoolOption = description: mkOption {
            type = types.nullOr types.bool;
            default = null;
            inherit description;
          };

          optionalEnumOption = values: description: mkOption {
            type = types.nullOr (types.enum values);
            default = null;
            inherit description;
          };

          optionalListOption = type: description: mkOption {
            type = types.nullOr (types.listOf type);
            default = null;
            inherit description;
          };

          optionalPathOption = description: mkOption {
            type = types.nullOr pathStringType;
            default = null;
            inherit description;
          };

          optionalStrOption = description: mkOption {
            type = types.nullOr types.str;
            default = null;
            inherit description;
          };

          optionalUnsignedOption = description: mkOption {
            type = types.nullOr types.ints.unsigned;
            default = null;
            inherit description;
          };

          optionalPortOption = description: mkOption {
            type = types.nullOr types.port;
            default = null;
            inherit description;
          };

          optionalSubmoduleOption = module: description: mkOption {
            type = types.nullOr (types.submodule module);
            default = null;
            inherit description;
          };

          attrsOfSubmoduleOption = module: description: mkOption {
            type = types.attrsOf (types.submodule module);
            default = {};
            inherit description;
          };

          directionType = types.enum [
            "west"
            "east"
            "north"
            "south"
            "left"
            "right"
            "up"
            "down"
            "above"
            "below"
            "W"
            "E"
            "N"
            "S"
            "Left"
            "Right"
            "Up"
            "Down"
            "Above"
            "Below"
          ];

          isEmptyAttrs = value:
            builtins.isAttrs value && builtins.length (builtins.attrNames value) == 0;

          cleanToml = value:
            if builtins.isAttrs value then
              let
                cleaned = lib.mapAttrs (_: cleanToml) value;
                withoutNulls = lib.filterAttrs (_: v: v != null) cleaned;
              in
              lib.filterAttrs (_: v: !(isEmptyAttrs v)) withoutNulls
            else if builtins.isList value then
              map cleanToml value
            else
              value;

          wmConfigModule = {
            options.enabled_integration = optionalEnumOption [
              "niri"
              "i3"
              "paneru"
              "yabai"
            ] "Window-manager backend. Rust defaults to niri on Linux and yabai on macOS.";
          };

          loggingRuntimeConfigModule = {
            options.debug = optionalBoolOption "Enable debug logging.";
          };

          browserNativeRuntimeConfigModule = {
            options = {
              chromium_socket_path = optionalPathOption "Socket path for the Chromium-family browser bridge.";
              firefox_socket_path = optionalPathOption "Socket path for the Firefox-family browser bridge.";
            };
          };

          vscodeRuntimeConfigModule = {
            options = {
              remote_control_host = optionalStrOption "VS Code remote-control host.";
              remote_control_port = optionalPortOption "VS Code remote-control port.";
              state_file = optionalPathOption "Path to the persisted VS Code state file.";
              focus_settle_ms = optionalUnsignedOption "Settle window for repeated VS Code focus commands, in milliseconds.";
              test_clipboard_file = optionalPathOption "Clipboard file used by tests.";
            };
          };

          zellijRuntimeConfigModule = {
            options.break_plugin = optionalPathOption "Path to the zellij break-pane plugin artifact.";
          };

          runtimeConfigModule = {
            options = {
              logging = optionalSubmoduleOption loggingRuntimeConfigModule "Runtime logging settings.";
              browser_native = optionalSubmoduleOption browserNativeRuntimeConfigModule "Runtime browser bridge settings.";
              vscode = optionalSubmoduleOption vscodeRuntimeConfigModule "Runtime VS Code integration settings.";
              zellij = optionalSubmoduleOption zellijRuntimeConfigModule "Runtime zellij integration settings.";
            };
          };

          tearOffConfigModule = {
            options = {
              enabled = optionalBoolOption "Whether tearing off into a new window is allowed. Rust defaults to true.";
              strategy = optionalEnumOption [
                "only_if_edgemost"
                "once_it_neighbors_with_window_edge"
                "always"
              ] "Tear-off strategy.";
              scope = optionalEnumOption [
                "mux_pane"
                "mux_window"
                "mux_session"
                "terminal_tab"
              ] "Optional tear-off scope.";
            };
          };

          dockingConfigModule = {
            options = {
              tear_off = optionalSubmoduleOption tearOffConfigModule "Tear-off behavior.";
              snap_back = optionalBoolOption "Whether snapping a torn-out pane back is allowed. Rust defaults to true.";
            };
          };

          directionalBrowserFocusModule = {
            options = {
              left = optionalEnumOption [
                "ignore"
                "focus_previous_tab"
                "focus_next_tab"
                "focus_first_tab"
                "focus_last_tab"
              ] "Browser focus action when moving left.";
              right = optionalEnumOption [
                "ignore"
                "focus_previous_tab"
                "focus_next_tab"
                "focus_first_tab"
                "focus_last_tab"
              ] "Browser focus action when moving right.";
              up = optionalEnumOption [
                "ignore"
                "focus_previous_tab"
                "focus_next_tab"
                "focus_first_tab"
                "focus_last_tab"
              ] "Browser focus action when moving up.";
              down = optionalEnumOption [
                "ignore"
                "focus_previous_tab"
                "focus_next_tab"
                "focus_first_tab"
                "focus_last_tab"
              ] "Browser focus action when moving down.";
            };
          };

          directionalBrowserMoveModule = {
            options = {
              left = optionalEnumOption [
                "ignore"
                "move_tab_backward"
                "move_tab_forward"
                "move_tab_to_first_position"
                "move_tab_to_last_position"
              ] "Browser move action when moving left.";
              right = optionalEnumOption [
                "ignore"
                "move_tab_backward"
                "move_tab_forward"
                "move_tab_to_first_position"
                "move_tab_to_last_position"
              ] "Browser move action when moving right.";
              up = optionalEnumOption [
                "ignore"
                "move_tab_backward"
                "move_tab_forward"
                "move_tab_to_first_position"
                "move_tab_to_last_position"
              ] "Browser move action when moving up.";
              down = optionalEnumOption [
                "ignore"
                "move_tab_backward"
                "move_tab_forward"
                "move_tab_to_first_position"
                "move_tab_to_last_position"
              ] "Browser move action when moving down.";
              docking = optionalSubmoduleOption dockingConfigModule "Browser docking behavior.";
            };
          };

          browserAppConfigModule = {
            options = {
              enabled = optionalBoolOption "Enable this browser integration profile.";
              anchor_app_window = optionalBoolOption "Prevent the window manager from moving this browser window.";
              focus = optionalSubmoduleOption directionalBrowserFocusModule "Directional browser focus behavior.";
              move = optionalSubmoduleOption directionalBrowserMoveModule "Directional browser move behavior.";
            };
          };

          internalPaneDirectionConfigModule = {
            options = {
              enabled = optionalBoolOption "Whether internal-pane handling is enabled. Rust defaults to true.";
              allowed_directions = optionalListOption directionType "Allowed directions for internal-pane handling.";
            };
          };

          paneFocusConfigModule = {
            options.internal_panes = optionalSubmoduleOption internalPaneDirectionConfigModule "Internal-pane focus behavior.";
          };

          paneResizeConfigModule = {
            options.internal_panes = optionalSubmoduleOption internalPaneDirectionConfigModule "Internal-pane resize behavior.";
          };

          paneMoveConfigModule = {
            options = {
              internal_panes = optionalSubmoduleOption internalPaneDirectionConfigModule "Internal-pane move behavior.";
              docking = optionalSubmoduleOption dockingConfigModule "Pane docking behavior.";
            };
          };

          terminalFocusConfigModule = {
            options = {
              internal_panes = optionalSubmoduleOption internalPaneDirectionConfigModule "Internal-pane focus behavior.";
              ignore_tabs = optionalBoolOption "Whether edge focus should stay inside the current terminal tab. Rust defaults to true.";
            };
          };

          terminalMoveConfigModule = {
            options = {
              internal_panes = optionalSubmoduleOption internalPaneDirectionConfigModule "Internal-pane move behavior.";
              docking = optionalSubmoduleOption dockingConfigModule "Terminal pane docking behavior.";
              ignore_tabs = optionalBoolOption "Whether edge moves should stay inside the current terminal tab. Rust defaults to true.";
            };
          };

          terminalMuxControlConfigModule = {
            options.enable = optionalBoolOption "Override whether mux bridge/control support is enabled.";
          };

          terminalAppConfigModule = {
            options = {
              enabled = optionalBoolOption "Enable this terminal integration profile.";
              anchor_app_window = optionalBoolOption "Prevent the window manager from moving this terminal window.";
              focus = optionalSubmoduleOption terminalFocusConfigModule "Terminal focus behavior.";
              move = optionalSubmoduleOption terminalMoveConfigModule "Terminal move behavior.";
              resize = optionalSubmoduleOption paneResizeConfigModule "Terminal resize behavior.";
              mux_backend = optionalEnumOption [
                "tmux"
                "zellij"
                "wezterm"
                "kitty"
              ] "Terminal mux backend.";
              tear_off_scope = optionalEnumOption [
                "mux_pane"
                "mux_window"
                "mux_session"
                "terminal_tab"
              ] "Default terminal tear-off scope.";
              mux = optionalSubmoduleOption terminalMuxControlConfigModule "Terminal mux control overrides.";
            };
          };

          editorTerminalUiConfigModule = {
            options = {
              mux_backend = optionalEnumOption [
                "inherit"
                "inherited"
                "tmux"
                "zellij"
                "wezterm"
                "kitty"
              ] "Mux backend for terminal-hosted editor UIs.";
              app = optionalStrOption "Terminal host application alias for this editor.";
            };
          };

          editorGraphicalUiConfigModule = {
            options.app = optionalStrOption "Graphical host application alias for this editor.";
          };

          editorUiConfigModule = {
            options = {
              terminal = optionalSubmoduleOption editorTerminalUiConfigModule "Terminal-hosted UI settings.";
              graphical = optionalSubmoduleOption editorGraphicalUiConfigModule "Graphical UI settings.";
            };
          };

          editorAppConfigModule = {
            options = {
              enabled = optionalBoolOption "Enable this editor integration profile.";
              anchor_app_window = optionalBoolOption "Prevent the window manager from moving this editor window.";
              focus = optionalSubmoduleOption paneFocusConfigModule "Editor focus behavior.";
              resize = optionalSubmoduleOption paneResizeConfigModule "Editor resize behavior.";
              move = optionalSubmoduleOption paneMoveConfigModule "Editor move behavior.";
              manage_terminal = optionalBoolOption "Allow the editor adapter to manage editor-owned terminal panes.";
              ui = optionalSubmoduleOption editorUiConfigModule "Editor UI host settings.";
              tear_off_scope = optionalEnumOption [
                "buffer"
                "window"
                "workspace"
              ] "Editor tear-off scope.";
            };
          };

          appConfigModule = {
            options = {
              browser = attrsOfSubmoduleOption browserAppConfigModule "Browser integration profiles keyed by alias.";
              terminal = attrsOfSubmoduleOption terminalAppConfigModule "Terminal integration profiles keyed by alias.";
              editor = attrsOfSubmoduleOption editorAppConfigModule "Editor integration profiles keyed by alias.";
            };
          };

          generatedConfig = tomlFormat.generate "yeet-and-yoink-config.toml" (
            cleanToml (lib.removeAttrs cfg.config [ "raw" ])
          );
          configSource =
            if cfg.config.raw != null
            then pkgs.writeText "yeet-and-yoink-config.toml" cfg.config.raw
            else generatedConfig;
        in {
          options.programs.yeet-and-yoink = {
            enable = mkEnableOption "yeet-and-yoink";

            package = mkOption {
              type = types.package;
              default = self.packages.${pkgs.system}.default;
              defaultText = literalExpression "inputs.yeet-and-yoink.packages.${pkgs.system}.default";
              description = "yeet-and-yoink package to install.";
            };

            config = mkOption {
              type = types.submodule {
                options = {
                  raw = mkOption {
                    type = types.nullOr types.lines;
                    default = null;
                    description = ''
                      Raw TOML for yeet-and-yoink written as-is. When non-null, this value
                      overrides all other programs.yeet-and-yoink.config.* fields.
                    '';
                  };

                  wm = optionalSubmoduleOption wmConfigModule "Window-manager config.";
                  app = optionalSubmoduleOption appConfigModule "App integration config.";
                  runtime = optionalSubmoduleOption runtimeConfigModule "Runtime integration config.";
                };
              };
              default = {};
              description = "yeet-and-yoink runtime configuration.";
            };
          };

          config = mkIf cfg.enable {
            home.packages = [ cfg.package ];
            xdg.configFile."yeet-and-yoink/config.toml".source = configSource;
          };
        };
    in {
      packages = forAllSystems (pkgs:
        rec {
          yeet-and-yoink = mkPackage pkgs;
          default = yeet-and-yoink;
        });

      overlays.default = final: prev: {
        yeet-and-yoink = self.packages.${prev.system}.default;
      };

      homeManagerModules.default = hmModule;
    };
}
