{ pkgs, lib, ... }:
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
    just
  ] ++ lib.optionals pkgs.stdenv.isLinux [
    pkg-config
    fontconfig
    fontconfig.dev
    vulkan-headers
    libxkbcommon
    xorg.libxcb
  ] ++ lib.optionals pkgs.stdenv.isDarwin [
    apple-sdk_15
  ];

  env = lib.optionalAttrs pkgs.stdenv.isLinux {
    LD_LIBRARY_PATH = lib.makeLibraryPath [
      pkgs.wayland
      pkgs.libxkbcommon
      pkgs.xorg.libxcb
      pkgs.vulkan-loader
    ];
  };
}
