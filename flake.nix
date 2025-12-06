{
  description = "tables";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    rust-overlay.url = "github:oxalica/rust-overlay";
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

        rustChannel = pkgs.rust-bin.stable.latest;
        rustToolchain = rustChannel.default.override {
          extensions = [
            "rust-src"
          ];
        };
        rustAnalyzer = rustChannel.rust-analyzer;
      in
      {
        devShells = {
          default =
            with pkgs;
            mkShell {
              buildInputs = [
                cargo-nextest
                nil
                nixfmt-rfc-style
                just
                rustAnalyzer
                rustToolchain
                xorg.libxcb
                libxkbcommon
                wayland
                wayland-protocols
                vulkan-loader
                mesa
              ];

              LD_LIBRARY_PATH = lib.makeLibraryPath [
                mesa
                wayland
                libxkbcommon
                xorg.libxcb
                vulkan-loader
              ];

              VK_ICD_FILENAMES = "${mesa}/share/vulkan/icd.d/radeon_icd.x86_64.json";
            };
        };

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
