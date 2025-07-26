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
        mkShellFor = { pkgs, name, toolchain, withCuda ? false, cudaPackages ? pkgs.cudaPackages }:
          let
            compilerPkgs = with pkgs; if withCuda then [ gcc13 ] else [ clang ];

            shellAttrs = {
              inherit name;
              nativeBuildInputs = (with pkgs; [
                pkg-config
                cmake
                # Mold Linker for faster builds (only on Linux)
                (pkgs.lib.optionals stdenv.isLinux mold)
              ]) ++ compilerPkgs;
              buildInputs = (with pkgs; [
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
              ]) ++ (pkgs.lib.optionals withCuda [
                cudaPackages.cudatoolkit
                cudaPackages.cudnn
                cudaPackages.cuda_cudart
                cudaPackages.libcublas
                pkgs.mkl
                pkgs.stdenv.cc.cc.lib
              ])
              ++ [ toolchain ];

              packages = [ ];

              # Environment variables
              RUST_SRC_PATH = "${toolchain}/lib/rustlib/src/rust/library";
              LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath ((with pkgs; [ libxkbcommon libxkbcommon.dev wayland wayland.dev xorg.libX11 xorg.libX11.dev xorg.libXcursor pkgs.xorg.libXi pkgs.xorg.libXrandr pkgs.libiconv pkgs.openssl.dev pkgs.vulkan-loader ])
                ++ (pkgs.lib.optionals withCuda [
                  cudaPackages.cudatoolkit
                  cudaPackages.cudnn
                  cudaPackages.cuda_cudart
                  cudaPackages.libcublas
                  pkgs.mkl
                  pkgs.stdenv.cc.cc.lib
                ]));
              OPENSSL_STATIC = "0";
              OPENSSL_DIR = pkgs.openssl.dev;
              OPENSSL_INCLUDE_DIR = (
                pkgs.lib.makeSearchPathOutput "dev" "include" [ pkgs.openssl.dev ]
              ) + "/openssl";
            };
            cudaAttrs = {
              CUDA_TOOLKIT_ROOT_DIR = "${pkgs.cudaPackages_12_8.cudatoolkit}";
              MKLROOT = "${pkgs.mkl}";
            };
          in
            pkgs.mkShell (shellAttrs // pkgs.lib.optionalAttrs withCuda cudaAttrs);

        # Pkgs for default shell
        pkgs-default = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };

        # Toolchain for default shell
        toolchain-default = (pkgs-default.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml).override {
          extensions = [ "rust-src" ];
        };

        # Pkgs for cuda shell
        pkgs-cuda = import nixpkgs {
          inherit system;
          overlays = [
            (import rust-overlay)
            (final: prev: {
              # Forcefully override ctranslate2 to be built with a consistent CUDA package set
              # and with dynamic loading enabled.
              ctranslate2 = (prev.ctranslate2.override {
                stdenv = prev.gcc13Stdenv;
                withCUDA = true;
                withCuDNN = true;
                # Ensure it uses the exact same package set as the rest of the shell
                cudaPackages = final.cudaPackages_12_8;
              }).overrideAttrs (oldAttrs: {
                cmakeFlags = oldAttrs.cmakeFlags ++ [ "-DCUDA_DYNAMIC_LOADING=ON" ];
              });
            })
          ];
          config.allowUnfree = true;
        };
        # Toolchain for cuda shell
        toolchain-cuda = (pkgs-cuda.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml).override {
          extensions = [ "rust-src" ];
        };

      in
      {
        devShells = {
          default = mkShellFor { pkgs = pkgs-default; name = "rust-dev"; toolchain = toolchain-default; };
          cuda =
            let
              # Define a single, unambiguous CUDA package set
              cuda_pkgs = pkgs-cuda.cudaPackages_12_8;
            in
            (mkShellFor {
              pkgs = pkgs-cuda;
              name = "rust-dev-cuda";
              toolchain = toolchain-cuda;
              withCuda = true;
              # Override the cudaPackages in the shell with our specific version
              cudaPackages = cuda_pkgs;
            }).overrideAttrs (old: {
              RUSTFLAGS = (old.RUSTFLAGS or "") + " -C link-args=-Wl,-rpath,${cuda_pkgs.cudatoolkit}/lib -Wl,-rpath,${cuda_pkgs.cudnn}/lib -Wl,-rpath,${cuda_pkgs.libcublas}/lib -Wl,-rpath,${pkgs-cuda.onnxruntime}/lib";
            });
        };
      });
}
