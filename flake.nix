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
              llvmPackages.libclang.lib  # For whisper-rs-sys bindgen
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
              openblas  # For whisper.cpp optimization
              vulkan-headers  # For whisper.cpp Vulkan compilation
              shaderc  # Provides glslc shader compiler for Vulkan
              rust-analyzer-unwrapped
              wtype
              vulkan-loader
              toolchain
            ];

            packages = [ ];

            # Environment variables
            RUST_SRC_PATH = "${toolchain}/lib/rustlib/src/rust/library";
            LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";  # For whisper-rs-sys bindgen

            # OpenBLAS configuration for whisper.cpp acceleration
            BLAS_INCLUDE_DIRS = "${pkgs.openblas.dev}/include";
            OPENBLAS_PATH = "${pkgs.openblas}";

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
              openblas  # Add OpenBLAS to library path
            ]);
            OPENSSL_STATIC = "0";
            OPENSSL_DIR = pkgs.openssl.dev;
            OPENSSL_INCLUDE_DIR = (
              pkgs.lib.makeSearchPathOutput "dev" "include" [ pkgs.openssl.dev ]
            ) + "/openssl";
          };
        };

        packages = let
          cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
          sonoriPkg = pkgs.rustPlatform.buildRustPackage rec {
            pname = "sonori";
            version = cargoToml.package.version;

            src = ./.;

            cargoLock = {
              lockFile = ./Cargo.lock;
              outputHashes = {
                "dpi-0.1.2" = "sha256-7DW0eaqJ5S0ixl4aio+cAE8qnq77tT9yzbemJJOGDX0=";
              };
            };

            nativeBuildInputs = with pkgs; [
              pkg-config
              cmake
              (pkgs.lib.optionals stdenv.isLinux mold)
              clang
              llvmPackages.libclang.lib  # For whisper-rs-sys bindgen
              vulkan-headers
              shaderc
              git
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
              wtype
              vulkan-loader
              vulkan-headers
              openblas
              openblas.dev
              onnxruntime
            ];

            # Environment variable to point ort-sys to system ONNX Runtime
            ORT_STRATEGY = "system";

            LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
            BLAS_INCLUDE_DIRS = "${pkgs.openblas.dev}/include";
            OPENBLAS_PATH = "${pkgs.openblas}";

            # Skip tests in Nix build (CI will run them)
            doCheck = false;

            postInstall = ''
              # Install desktop file
              mkdir -p $out/share/applications
              install -m 644 ${./desktop/sonori.desktop} $out/share/applications/

              # Install AppStream metadata
              mkdir -p $out/share/metainfo
              install -m 644 ${./desktop/com.github.0xPD33.sonori.metainfo.xml} $out/share/metainfo/

              # Install icon
              mkdir -p $out/share/icons/hicolor/scalable/apps
              install -m 644 ${./desktop/icons/hicolor/scalable/apps/sonori.svg} $out/share/icons/hicolor/scalable/apps/
            '';

            meta = with pkgs.lib; {
              description = "Local AI-powered speech transcription for Linux";
              homepage = "https://github.com/0xPD33/sonori";
              license = licenses.mit;
              maintainers = [ ];
              platforms = [ "x86_64-linux" "aarch64-linux" ];
              mainProgram = "sonori";
            };
          };
        in {
          sonori = sonoriPkg;
          default = sonoriPkg;
        };

        apps = {
          sonori = {
            type = "app";
            program = "${self.packages.${system}.sonori}/bin/sonori";
          };
          default = {
            type = "app";
            program = "${self.packages.${system}.sonori}/bin/sonori";
          };
        };
      });
}
