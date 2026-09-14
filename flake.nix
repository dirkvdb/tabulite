{
  description = "tabulite";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      nixpkgs,
      flake-utils,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
        };
        linuxRuntimeDeps = with pkgs; [
          fontconfig
          libxcb
          libxkbcommon
          wayland
          vulkan-loader
        ];
      in
      {
        packages = {
          # regular, host-native build (dynamic)
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "tabulite";
            version = "1.0.0";

            src = ./.;
            nativeBuildInputs = [
              pkgs.pkg-config
              pkgs.makeWrapper
            ];
            buildInputs = pkgs.lib.optionals pkgs.stdenv.isLinux linuxRuntimeDeps;

            postFixup = pkgs.lib.optionalString pkgs.stdenv.isLinux ''
              wrapProgram $out/bin/tabulite \
                --prefix LD_LIBRARY_PATH : ${pkgs.lib.makeLibraryPath linuxRuntimeDeps}
            '';

            postInstall = ''
              install -Dm644 tabulite.desktop $out/share/applications/tabulite.desktop
              install -Dm644 logo.svg $out/share/icons/hicolor/scalable/apps/tabulite.svg
            '';

            # assuming you have a Cargo.lock
            cargoLock = {
              lockFile = ./Cargo.lock;
              outputHashes = {
                "geo-2026.9.14" = "sha256-mr0kUqgcLAKGg8T3tB0Jw9PSeE1MxADWij9CqQ+FE+g=";
              };
            };
          };
        }
        // (
          # musl-static package, only on Linux
          if pkgs.stdenv.isLinux then
            {
              static = pkgs.pkgsStatic.rustPlatform.buildRustPackage {
                pname = "tabulite";
                version = "1.0.0";

                src = ./.;
                nativeBuildInputs = [ pkgs.pkg-config ];
                buildInputs = [
                  pkgs.fontconfig
                  pkgs.libxcb
                  pkgs.libxkbcommon
                ];

                postInstall = ''
                  install -Dm644 tabulite.desktop $out/share/applications/tabulite.desktop
                  install -Dm644 logo.svg $out/share/icons/hicolor/scalable/apps/tabulite.svg
                '';

                cargoLock = {
                  lockFile = ./Cargo.lock;
                  outputHashes = {
                    "geo-2026.9.2" = "sha256-mr0kUqgcLAKGg8T3tB0Jw9PSeE1MxADWij9CqQ+FE+g=";
                  };
                };
              };
            }
          else
            { }
        );
      }
    );
}
