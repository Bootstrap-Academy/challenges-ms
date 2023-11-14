{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    fenix.url = "github:nix-community/fenix";
  };

  outputs = {
    nixpkgs,
    fenix,
    ...
  }: let
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
    devShells = eachDefaultSystem (system: let
      inherit (nixpkgs) lib;
      pkgs = import nixpkgs {inherit system;};
      rust = fenix.packages.${system}.stable.toolchain;
      devShell = withRust:
        pkgs.mkShell {
          packages = with pkgs;
            lib.optional withRust rust
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
