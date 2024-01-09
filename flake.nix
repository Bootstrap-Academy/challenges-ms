{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
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
    eachDefaultSystem = f:
      builtins.listToAttrs (map (system: {
          name = system;
          value = f system;
        })
        defaultSystems);
  in {
    packages = eachDefaultSystem (system: let
      pkgs = import nixpkgs {inherit system;};
    in {
      default = self.packages.${system}.challenges;
      challenges = let
        toolchain = fenix.packages.${system}.stable;
        rustPlatform = pkgs.makeRustPlatform {
          inherit (toolchain) cargo rustc;
        };
      in
        rustPlatform.buildRustPackage {
          inherit ((fromTOML (builtins.readFile ./challenges/Cargo.toml)).package) version;
          pname = "academy-challenges";
          src = lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions [
              ./Cargo.toml
              ./Cargo.lock
              ./migration
              ./entity
              ./lib
              ./schemas
              ./challenges
            ];
          };
          cargoLock.lockFile = ./Cargo.lock;
          doCheck = false;
        };
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
            ];
          RUST_LOG = "info,difft=off,poem_ext,lib,entity,migration,challenges=trace";
        };
    in {
      default = devShell true;
      noRust = devShell false;
    });
  };
}
