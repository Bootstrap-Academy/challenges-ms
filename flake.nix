{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    fenix.url = "github:nix-community/fenix";
  };

  outputs = {
    self,
    nixpkgs,
    fenix,
    ...
  }: let
    inherit (nixpkgs) lib;

    defaultSystems = [
      "x86_64-linux"
      "x86_64-darwin"
      "aarch64-linux"
      "aarch64-darwin"
    ];
    eachDefaultSystem = lib.genAttrs defaultSystems;
  in {
    packages = eachDefaultSystem (system: let
      pkgs = import nixpkgs {inherit system;};

      toolchain = fenix.packages.${system}.stable;

      cargoNix = pkgs.callPackage ./Cargo.nix {
        pkgs = pkgs.extend (final: prev: {
          inherit (toolchain) cargo;
          # workaround for https://github.com/NixOS/nixpkgs/blob/d80a3129b239f8ffb9015473c59b09ac585b378b/pkgs/build-support/rust/build-rust-crate/default.nix#L19-L23
          rustc = toolchain.rustc // {unwrapped.configureFlags = ["--target="];};
        });
      };
    in {
      default = self.packages.${system}.challenges;
      challenges = pkgs.symlinkJoin {
        name = "academy-challenges";
        paths = [
          cargoNix.workspaceMembers.challenges.build
          cargoNix.workspaceMembers.migration.build
        ];
      };
      generate = pkgs.writeShellScriptBin "generate" ''
        ${lib.getExe pkgs.crate2nix} generate
      '';
    });

    nixosModules.default = {
      config,
      lib,
      pkgs,
      ...
    }: let
      settingsFormat = pkgs.formats.toml {};
    in {
      options.academy.backend.challenges = with lib; {
        enable = mkEnableOption "Bootstrap Academy Challenges Microservice";
        RUST_LOG = mkOption {
          type = types.str;
          default = "info";
        };
        environmentFiles = mkOption {
          type = types.listOf types.path;
        };
        settings = mkOption {
          inherit (settingsFormat) type;
        };
      };

      config = let
        cfg = config.academy.backend.challenges;
      in
        lib.mkIf cfg.enable {
          systemd.services = {
            academy-challenges = {
              wantedBy = ["multi-user.target"];
              serviceConfig = {
                User = "academy-challenges";
                Group = "academy-challenges";
                DynamicUser = true;
                EnvironmentFile = cfg.environmentFiles;
              };
              environment = {
                inherit (cfg) RUST_LOG;
                CONFIG_PATH = settingsFormat.generate "config.toml" cfg.settings;
              };
              preStart = ''
                ${self.packages.${pkgs.system}.default}/bin/migration
              '';
              script = ''
                ${self.packages.${pkgs.system}.default}/bin/challenges
              '';
            };
          };
        };
    };

    devShells = eachDefaultSystem (system: let
      inherit (nixpkgs) lib;
      pkgs = import nixpkgs {inherit system;};
      devShell = withRust:
        pkgs.mkShell {
          packages = with pkgs;
            lib.optionals withRust [rustc cargo clippy rust-analyzer]
            ++ [
              just
              postgresql
              redis
              bacon
              sea-orm-cli
              yq
              gnused
              crate2nix
              self.packages.${system}.generate
            ];
          RUST_LOG = "info,difft=off,poem_ext,lib,entity,migration,challenges=trace";
        };
    in {
      default = devShell true;
      noRust = devShell false;
    });
  };
}
