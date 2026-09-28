{ pkgs, lib, ... }:
let
  duckdb = import ./duckdb.nix { inherit pkgs; };
in
{
  languages.rust = {
    enable = true;
    channel = "stable";
    version = "1.95.0";
    components = [
      "rustc"
      "cargo"
      "rust-src"
      "rustfmt"
      "clippy"
    ];
  };

  packages = with pkgs; [
    cargo-nextest
    duckdb.lib
    duckdb.dev
    duckdb
    (python3.withPackages (pythonPackages: [ pythonPackages.openpyxl ]))
    just
    sccache
    pkg-config
    fontconfig
    fontconfig.dev
    vulkan-headers
    libxkbcommon
    xorg.libxcb
  ] ++ lib.optionals pkgs.stdenv.isDarwin [
    apple-sdk_15
  ];

  enterShell = ''
    python3 "$DEVENV_ROOT/scripts/generate-test-data.py"
  '';

  env = {
    RUSTC_WRAPPER = "${pkgs.sccache}/bin/sccache";
    DUCKDB_LIB_DIR = "${duckdb.lib}/lib";
    DUCKDB_INCLUDE_DIR = "${duckdb.dev}/include";
  } // lib.optionalAttrs pkgs.stdenv.isLinux {
    LD_LIBRARY_PATH = lib.makeLibraryPath [
      pkgs.wayland
      pkgs.libxkbcommon
      pkgs.xorg.libxcb
      pkgs.vulkan-loader
      duckdb.lib
    ];
  };
}
