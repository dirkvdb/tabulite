{ pkgs, lib, ... }:
{
  languages.rust = {
    enable = true;
    channel = "stable";
    version = "1.97.1";
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
    pkg-config
    fontconfig
    fontconfig.dev
    vulkan-headers
    libxkbcommon
    xorg.libxcb
  ] ++ lib.optionals pkgs.stdenv.isDarwin [
    apple-sdk_15
  ];

  env.LD_LIBRARY_PATH = lib.mkIf pkgs.stdenv.isLinux (lib.makeLibraryPath [
    pkgs.wayland
    pkgs.libxkbcommon
    pkgs.xorg.libxcb
    pkgs.vulkan-loader
  ]);
}
