{
  description = "tables";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
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
              ] ++ lib.optionals stdenv.isLinux [
                fontconfig
                vulkan-headers
                libxkbcommon
                xorg.libxcb
              ] ++ lib.optionals stdenv.isDarwin [
                apple-sdk_15
              ];

              LD_LIBRARY_PATH = lib.optionalString stdenv.isLinux (
                lib.makeLibraryPath [
                  wayland
                  libxkbcommon
                  xorg.libxcb
                  vulkan-loader
                ]
              );
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
