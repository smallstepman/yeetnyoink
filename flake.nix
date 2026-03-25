{
  description = "yeetnyoink package and home-manager module";

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
        pname = "yeetnyoink";
        version = "0.1.0";
        src = ./.;
        cargoLock.lockFile = ./Cargo.lock;
        meta = with pkgs.lib; {
          description = "Deep focus/move integration between niri and apps";
          mainProgram = "yeetnyoink";
          platforms = platforms.all;
        };
      };

      hmModule = { config, lib, pkgs, ... }:
        let
          inherit (lib) literalExpression mkEnableOption mkOption mkIf;
          types = lib.types;
          cfg = config.programs.yeetnyoink;
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

          wmConfigModule =
            let
              floatingFocusStrategyType = types.enum [
                "radial_center"
                "trailing_edge_parallel"
                "leading_edge_parallel"
                "cross_edge_gap"
                "overlap_then_gap"
                "ray_angle"
              ];

              enabledWmBackendModule = backendName: {
                options = {
                  enabled = mkOption {
                    type = types.bool;
                    description = "Whether to enable the ${backendName} backend.";
                  };

                  floating_focus_strategy = mkOption {
                    type = types.nullOr floatingFocusStrategyType;
                    default = null;
                    description = "Floating-window directional-focus strategy for the ${backendName} backend. Current built-in tiling-only backends must leave this unset.";
                  };
                };
              };

              missionControlShortcutConfigModule = {
                options = {
                  keycode = mkOption {
                    type = types.str;
                    description = "0x-prefixed macOS virtual keycode for the shortcut.";
                  };

                  shift = mkOption {
                    type = types.bool;
                    default = false;
                    description = "Whether Shift is held for the shortcut.";
                  };

                  ctrl = mkOption {
                    type = types.bool;
                    default = false;
                    description = "Whether Control is held for the shortcut.";
                  };

                  option = mkOption {
                    type = types.bool;
                    default = false;
                    description = "Whether Option is held for the shortcut.";
                  };

                  command = mkOption {
                    type = types.bool;
                    default = false;
                    description = "Whether Command is held for the shortcut.";
                  };

                  fn = mkOption {
                    type = types.bool;
                    default = false;
                    description = "Whether Fn is held for the shortcut.";
                  };
                };
              };

              macosNativeWmConfigModule = {
                options = {
                  enabled = mkOption {
                    type = types.bool;
                    description = "Whether to enable the macOS-native Spaces-aware backend.";
                  };

                  floating_focus_strategy = mkOption {
                    type = types.nullOr floatingFocusStrategyType;
                    default = null;
                    description = "Floating-window directional-focus strategy for the macOS-native backend. Runtime validation requires this when `enabled = true`.";
                  };

                  mission_control_keyboard_shortcuts = mkOption {
                    type = types.submodule {
                      options = {
                        move_left_a_space = mkOption {
                          type = types.submodule missionControlShortcutConfigModule;
                          description = "Shortcut that macOS Mission Control uses to move left one space.";
                        };

                        move_right_a_space = mkOption {
                          type = types.submodule missionControlShortcutConfigModule;
                          description = "Shortcut that macOS Mission Control uses to move right one space.";
                        };
                      };
                    };
                    description = "Mission Control space-navigation shortcuts used by the macOS-native WM backend.";
                  };
                };
              };
            in {
              options = {
                macos_native = optionalSubmoduleOption macosNativeWmConfigModule "macOS-native Spaces-aware backend config. When enabled, runtime validation requires `floating_focus_strategy` plus both Mission Control adjacent-space shortcuts.";
                niri = optionalSubmoduleOption (enabledWmBackendModule "niri") "niri backend config. Current built-in backend is tiling-only, so leave `floating_focus_strategy` unset.";
                i3 = optionalSubmoduleOption (enabledWmBackendModule "i3") "i3 backend config. Current built-in backend is tiling-only, so leave `floating_focus_strategy` unset.";
                hyprland = optionalSubmoduleOption (enabledWmBackendModule "hyprland") "Hyprland backend config. Current built-in backend is tiling-only, so leave `floating_focus_strategy` unset.";
                paneru = optionalSubmoduleOption (enabledWmBackendModule "paneru") "Paneru backend config. Current built-in backend is tiling-only, so leave `floating_focus_strategy` unset.";
                yabai = optionalSubmoduleOption (enabledWmBackendModule "yabai") "yabai backend config. Current built-in backend is tiling-only, so leave `floating_focus_strategy` unset.";
              };
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
              tab_axis = optionalEnumOption [ "horizontal" "vertical" "vertical_flipped" ] ''
                Default browser tab axis. `horizontal` keeps west/east bound to tab actions;
                `vertical` maps north/south to previous/next tab; `vertical_flipped` maps
                north/south to next/previous; both leave west/east to the WM.
              '';
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
              ignore_tabs = optionalBoolOption "Legacy fallback for whether edge focus should stay inside the current terminal tab when host_tabs is unset. Rust defaults to true.";
            };
          };

          terminalMoveConfigModule = {
            options = {
              internal_panes = optionalSubmoduleOption internalPaneDirectionConfigModule "Internal-pane move behavior.";
              docking = optionalSubmoduleOption dockingConfigModule "Terminal pane docking behavior.";
              ignore_tabs = optionalBoolOption "Legacy fallback for whether edge moves should stay inside the current terminal tab when host_tabs is unset. Rust defaults to true.";
            };
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
              host_tabs = optionalEnumOption [
                "transparent"
                "focus"
                "native_full"
              ] "Whether terminal host tabs participate in edge focus/move routing.";
              tear_off_scope = optionalEnumOption [
                "mux_pane"
                "mux_window"
                "mux_session"
                "terminal_tab"
              ] "Default terminal tear-off scope.";
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

          generatedConfig = tomlFormat.generate "yeetnyoink-config.toml" (
            cleanToml (lib.removeAttrs cfg.config [ "raw" ])
          );
          configSource =
            if cfg.config.raw != null
            then pkgs.writeText "yeetnyoink-config.toml" cfg.config.raw
            else generatedConfig;
        in {
          options.programs.yeetnyoink = {
            enable = mkEnableOption "yeetnyoink";

            package = mkOption {
              type = types.package;
              default = self.packages.${pkgs.system}.default;
              defaultText = literalExpression "inputs.yeetnyoink.packages.${pkgs.system}.default";
              description = "yeetnyoink package to install.";
            };

            config = mkOption {
              type = types.submodule {
                options = {
                  raw = mkOption {
                    type = types.nullOr types.lines;
                    default = null;
                    description = ''
                      Raw TOML for yeetnyoink written as-is. When non-null, this value
                      overrides all other programs.yeetnyoink.config.* fields.
                    '';
                  };

                  wm = optionalSubmoduleOption wmConfigModule "Window-manager config.";
                  app = optionalSubmoduleOption appConfigModule "App integration config.";
                  runtime = optionalSubmoduleOption runtimeConfigModule "Runtime integration config.";
                };
              };
              default = {};
              description = "yeetnyoink runtime configuration.";
            };
          };

          config = mkIf cfg.enable {
            home.packages = [ cfg.package ];
            xdg.configFile."yeetnyoink/config.toml".source = configSource;
          };
        };
    in {
      packages = forAllSystems (pkgs:
        rec {
          yeetnyoink = mkPackage pkgs;
          default = yeetnyoink;
        });

      overlays.default = final: prev: {
        yeetnyoink = self.packages.${prev.system}.default;
      };

      homeManagerModules.default = hmModule;
    };
}
