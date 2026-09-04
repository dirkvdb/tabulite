{
  description = "tables";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    rust-overlay.url = "github:oxalica/rust-overlay/stable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      nixpkgs,
      rust-overlay,
      flake-utils,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
      in
      {
        packages = {
          # regular, host-native build (dynamic)
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "tables";
            version = "1.0.0";

            src = ./.;

            # assuming you have a Cargo.lock
            cargoLock.lockFile = ./Cargo.lock;
          };
        }
        // (
          # musl-static package, only on Linux
          if pkgs.stdenv.isLinux then
            {
              static = pkgs.pkgsStatic.rustPlatform.buildRustPackage {
                pname = "tables";
                version = "1.0.0";

                src = ./.;

                cargoLock.lockFile = ./Cargo.lock;
              };
            }
          else
            { }
        );
      }
    );
}
