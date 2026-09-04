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
      in
      {
        packages = {
          # regular, host-native build (dynamic)
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "tabulite";
            version = "1.0.0";

            src = ./.;
            nativeBuildInputs = [ pkgs.pkg-config ];
            buildInputs = [
              pkgs.fontconfig
              pkgs.libxcb
              pkgs.libxkbcommon
            ];

            # assuming you have a Cargo.lock
            cargoLock = {
              lockFile = ./Cargo.lock;
              outputHashes = {
                "geo-2026.9.2" = "sha256-mr0kUqgcLAKGg8T3tB0Jw9PSeE1MxADWij9CqQ+FE+g=";
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
