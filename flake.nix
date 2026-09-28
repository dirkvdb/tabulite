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
        linuxRuntimeDeps = with pkgs; [
          fontconfig
          libxcb
          libxkbcommon
          wayland
          vulkan-loader
        ];
        buildDeps =
          with pkgs;
          [
            fontconfig.dev
            vulkan-headers
          ]
          ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [ pkgs.apple-sdk_15 ];
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
              pkgs.imagemagick
              duckdb
            ];
            buildInputs = [
              duckdb.lib
              duckdb.dev
            ]
            ++ buildDeps
            ++ pkgs.lib.optionals pkgs.stdenv.isLinux linuxRuntimeDeps;
            DUCKDB_LIB_DIR = "${duckdb.lib}/lib";
            DUCKDB_INCLUDE_DIR = "${duckdb.dev}/include";

            preCheck = ''
              ${pkgs.python3.withPackages (pythonPackages: [ pythonPackages.openpyxl ])}/bin/python3 scripts/generate-test-data.py
            '';

            postFixup = pkgs.lib.optionalString pkgs.stdenv.isLinux ''
              wrapProgram $out/bin/tabulite \
                --prefix LD_LIBRARY_PATH : ${pkgs.lib.makeLibraryPath (linuxRuntimeDeps ++ [ duckdb.lib ])}
            '';

            postInstall = ''
              install -Dm644 tabulite.desktop $out/share/applications/tabulite.desktop
              install -d $out/share/icons/hicolor/512x512/apps
              magick logo.png -resize 512x512 $out/share/icons/hicolor/512x512/apps/tabulite.png
            '';

            cargoLock.lockFile = ./Cargo.lock;
          };
        };
      }
    );
}
