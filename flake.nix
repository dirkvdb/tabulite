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
        duckdb = import ./duckdb.nix { inherit pkgs; };
        staticDuckdb = import ./duckdb.nix { pkgs = pkgs.pkgsStatic; };
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
            buildInputs = [ duckdb.lib duckdb.dev ] ++ pkgs.lib.optionals pkgs.stdenv.isLinux linuxRuntimeDeps;
            DUCKDB_LIB_DIR = "${duckdb.lib}/lib";
            DUCKDB_INCLUDE_DIR = "${duckdb.dev}/include";

            postFixup = pkgs.lib.optionalString pkgs.stdenv.isLinux ''
              wrapProgram $out/bin/tabulite \
                --prefix LD_LIBRARY_PATH : ${pkgs.lib.makeLibraryPath (linuxRuntimeDeps ++ [ duckdb.lib ])}
            '';

            postInstall = ''
              install -Dm644 tabulite.desktop $out/share/applications/tabulite.desktop
              install -Dm644 logo.svg $out/share/icons/hicolor/scalable/apps/tabulite.svg
            '';

            # assuming you have a Cargo.lock
            cargoLock = {
              lockFile = ./Cargo.lock;
              outputHashes = {
                "geo-2026.9.14" = "sha256-Gxf8s6zCalGUNitQODAXSgKu3Jai0ALY4QhwzbBJTVw=";
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
                  staticDuckdb.lib
                  staticDuckdb.dev
                  pkgs.fontconfig
                  pkgs.libxcb
                  pkgs.libxkbcommon
                ];

                DUCKDB_LIB_DIR = "${staticDuckdb.lib}/lib";
                DUCKDB_INCLUDE_DIR = "${staticDuckdb.dev}/include";
                DUCKDB_STATIC = "1";

                postInstall = ''
                  install -Dm644 tabulite.desktop $out/share/applications/tabulite.desktop
                  install -Dm644 logo.svg $out/share/icons/hicolor/scalable/apps/tabulite.svg
                '';

                cargoLock = {
                  lockFile = ./Cargo.lock;
                  outputHashes = {
                    "geo-2026.9.14" = "sha256-Gxf8s6zCalGUNitQODAXSgKu3Jai0ALY4QhwzbBJTVw=";
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
