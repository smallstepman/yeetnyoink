{
  description = "niri-deep package and home-manager module";

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
        pname = "niri-deep";
        version = "0.1.0";
        src = ./.;
        cargoLock.lockFile = ./Cargo.lock;
        meta = with pkgs.lib; {
          description = "Deep focus/move integration between niri and apps";
          mainProgram = "niri-deep";
          platforms = platforms.all;
        };
      };

      hmModule = { config, lib, pkgs, ... }:
        let
          cfg = config.programs.niri-deep;
          tomlFormat = pkgs.formats.toml {};
          generatedConfig = tomlFormat.generate "niri-deep-config.toml" (lib.filterAttrs (name: _: name != "raw") cfg.config);
          configSource =
            if cfg.config.raw != null
            then pkgs.writeText "niri-deep-config.toml" cfg.config.raw
            else generatedConfig;
        in {
          options.programs.niri-deep = {
            enable = lib.mkEnableOption "niri-deep";

            package = lib.mkOption {
              type = lib.types.package;
              default = self.packages.${pkgs.system}.default;
              defaultText = lib.literalExpression "inputs.niri-deep.packages.${pkgs.system}.default";
              description = "niri-deep package to install.";
            };

            config = lib.mkOption {
              type = lib.types.submodule ({ ... }: {
                freeformType = tomlFormat.type;
                options.raw = lib.mkOption {
                  type = lib.types.nullOr lib.types.lines;
                  default = null;
                  description = ''
                    Raw TOML for niri-deep written as-is. When non-null, this value
                    overrides all other programs.niri-deep.config.* fields.
                  '';
                };
              });
              default = {};
              description = "niri-deep runtime configuration.";
            };
          };

          config = lib.mkIf cfg.enable {
            home.packages = [ cfg.package ];
            xdg.configFile."niri-deep/config.toml".source = configSource;
          };
        };
    in {
      packages = forAllSystems (pkgs:
        rec {
          niri-deep = mkPackage pkgs;
          default = niri-deep;
        });

      overlays.default = final: prev: {
        niri-deep = self.packages.${prev.system}.default;
      };

      homeManagerModules.default = hmModule;
    };
}
