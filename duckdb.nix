{ pkgs }:
let
  # duckdb-rs 1.10505.0 targets DuckDB 1.5.5. Extensions must be
  # compiled for precisely the same version as the library.
  duckdbSource = pkgs.fetchFromGitHub {
    owner = "duckdb";
    repo = "duckdb";
    tag = "v1.5.5";
    hash = "sha256-vFXrMcWF5KDYYRjWZb6iJdhGnCAb6SMlSgzlcr+FQ8Y=";
  };
  excelSource = pkgs.fetchFromGitHub {
    owner = "duckdb";
    repo = "duckdb-excel";
    rev = "f4c72b5ef04a03b3a78a95b5a2ee94ba93e3178d";
    hash = "sha256-hyHTiTfRR+hXJ7hZKt/h/Hu1zNgEYEbMozIv6WZbnfA=";
  };
  sqliteSource = pkgs.fetchFromGitHub {
    owner = "duckdb";
    repo = "duckdb-sqlite";
    rev = "f79b1db7d7730b18d0f8400d3650ffa6b45168d8";
    hash = "sha256-zQSB/dreOArPrrXV8KP6i/nOlSguRyOGWORvwZ5BsfI=";
  };
  extensions = pkgs.writeText "tabulite-duckdb-extensions.cmake" ''
    include("${duckdbSource}/.github/config/in_tree_extensions.cmake")
    duckdb_extension_load(excel
      SOURCE_DIR ${excelSource}
      INCLUDE_DIR ${excelSource}/src/excel/include
    )
    duckdb_extension_load(sqlite_scanner SOURCE_DIR ${sqliteSource})
  '';
in
pkgs.duckdb.overrideAttrs (old: {
  version = "1.5.5";
  rev = "d8cdaa33fda8df955cc76ef58a280f68f4cd43fa";
  src = duckdbSource;
  doInstallCheck = false;
  buildInputs = old.buildInputs ++ [
    pkgs.expat
    pkgs.zlib
    pkgs.minizip-ng
    pkgs.bzip2
    pkgs.xz
    pkgs.zstd
  ];
  cmakeFlags =
    pkgs.lib.filter (flag: !(pkgs.lib.hasPrefix "-DDUCKDB_EXTENSION_CONFIGS" flag)) old.cmakeFlags
    ++ [ (pkgs.lib.cmakeFeature "DUCKDB_EXTENSION_CONFIGS" "${extensions}") ];
})
