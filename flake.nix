{
  description = "Rust development environment";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    # Rust
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs = {
        nixpkgs.follows = "nixpkgs";
        flake-utils.follows = "flake-utils";
      };
    };
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        # Pkgs for default shell
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };

        # Toolchain for default shell
        toolchain = (pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml).override {
          extensions = [ "rust-src" ];
        };

      in
      {
        devShells = {
          default = pkgs.mkShell {
            name = "rust-dev";
            nativeBuildInputs = with pkgs; [
              pkg-config
              cmake
              # Mold Linker for faster builds (only on Linux)
              (pkgs.lib.optionals stdenv.isLinux mold)
              clang
            ];
            buildInputs = with pkgs; [
              libxkbcommon
              libxkbcommon.dev
              wayland
              wayland.dev
              xorg.libX11.dev
              xorg.libX11
              xorg.libXcursor
              xorg.libXi
              xorg.libXrandr
              libiconv
              openssl.dev
              alsa-lib
              portaudio
              fftw
              curl
              ctranslate2
              sentencepiece
              rust-analyzer-unwrapped
              wtype
              vulkan-loader
              toolchain
            ];

            packages = [ ];

            # Environment variables
            RUST_SRC_PATH = "${toolchain}/lib/rustlib/src/rust/library";
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (with pkgs; [ 
              libxkbcommon 
              libxkbcommon.dev 
              wayland 
              wayland.dev 
              xorg.libX11 
              xorg.libX11.dev 
              xorg.libXcursor 
              xorg.libXi 
              xorg.libXrandr 
              libiconv 
              openssl.dev 
              vulkan-loader 
            ]);
            OPENSSL_STATIC = "0";
            OPENSSL_DIR = pkgs.openssl.dev;
            OPENSSL_INCLUDE_DIR = (
              pkgs.lib.makeSearchPathOutput "dev" "include" [ pkgs.openssl.dev ]
            ) + "/openssl";
          };
        };
      });
}
