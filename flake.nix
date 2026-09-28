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
            ];
            buildInputs = [
              duckdb.lib
              duckdb.dev
            ]
            ++ buildDeps
            ++ pkgs.lib.optionals pkgs.stdenv.isLinux linuxRuntimeDeps;
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

            cargoLock.lockFile = ./Cargo.lock;
          };
        };
      }
    );
}
