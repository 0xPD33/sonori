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
        # Pkgs for default shell with sentencepiece override
        pkgs = import nixpkgs {
          inherit system;
          overlays = [
            (import rust-overlay)
            # Override sentencepiece to use system protobuf instead of embedded version
            # This prevents protobuf ABI conflicts with onnxruntime
            (final: prev: {
              sentencepiece = prev.sentencepiece.overrideAttrs (old: {
                buildInputs = (old.buildInputs or []) ++ [
                  final.protobuf
                  final.abseil-cpp
                ];
                cmakeFlags = (old.cmakeFlags or []) ++ [
                  # Use modern CMake provider flags (replaces legacy SPM_USE_BUILTIN_PROTOBUF)
                  "-DSPM_PROTOBUF_PROVIDER=package"
                  "-DSPM_ABSL_PROVIDER=package"
                  "-DSPM_BUILD_TEST=OFF"
                ];
              });
            })
          ];
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
              pipewire  # For ALSA-PipeWire plugin compatibility
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
              dotool
              vulkan-loader
              toolchain
            ];

            packages = [ ];

            # Environment variables
            RUST_SRC_PATH = "${toolchain}/lib/rustlib/src/rust/library";
            LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";  # For whisper-rs-sys bindgen
            ALSA_PLUGIN_DIR = "${pkgs.pipewire}/lib/alsa-lib";  # For ALSA-PipeWire integration

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
              openblas
              pipewire  # For ALSA-PipeWire plugin compatibility
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

            # Force vendored sentencepiece-sys to use system libsentencepiece
            # This prevents it from building its own copy with embedded protobuf
            postPatch = ''
              # Find all vendored sentencepiece crates and enable the system feature
              for manifest in $(find . -path "*/sentencepiece*/Cargo.toml" -o -path "*/sentencepiece-sys*/Cargo.toml"); do
                echo "Patching $manifest to use system sentencepiece"

                # If there's already a sentencepiece-sys dependency, add system feature
                if grep -q 'sentencepiece-sys.*version' "$manifest"; then
                  sed -i 's/sentencepiece-sys = { version = "\([^"]*\)"[^}]*/sentencepiece-sys = { version = "\1", features = ["system"]/' "$manifest"
                fi

                # If it's the sentencepiece-sys crate itself, ensure system feature exists and is default
                if echo "$manifest" | grep -q "sentencepiece-sys"; then
                  # Add system to default features if not already there
                  if grep -q '^\[features\]' "$manifest"; then
                    sed -i '/^\[features\]/a system = []' "$manifest"
                    sed -i 's/^default = \[\(.*\)\]/default = [\1, "system"]/' "$manifest"
                  fi
                fi
              done
            '';

            nativeBuildInputs = with pkgs; [
              pkg-config
              cmake
              (pkgs.lib.optionals stdenv.isLinux mold)
              clang
              llvmPackages.libclang.lib  # For whisper-rs-sys bindgen
              vulkan-headers
              shaderc
              git
              makeWrapper  # For wrapping binary with library paths
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
              pipewire  # For ALSA-PipeWire plugin compatibility
              portaudio
              fftw
              curl
              ctranslate2
              sentencepiece  # Overridden via overlay to use system protobuf
              abseil-cpp     # Required by sentencepiece with package provider
              wtype
              dotool
              vulkan-loader
              vulkan-headers
              openblas
              openblas.dev
              onnxruntime
              protobuf  # Ensure consistent protobuf version across all C++ libs
            ];

            # Use system ONNX Runtime for Silero VAD
            ORT_STRATEGY = "system";

            LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
            BLAS_INCLUDE_DIRS = "${pkgs.openblas.dev}/include";
            OPENBLAS_PATH = "${pkgs.openblas}";

            # Skip tests in Nix build (CI will run them)
            doCheck = false;

            postInstall = ''
              # Wrap binary with required library paths and Vulkan environment
              wrapProgram $out/bin/sonori \
                --prefix LD_LIBRARY_PATH : ${pkgs.lib.makeLibraryPath [
                  pkgs.libxkbcommon
                  pkgs.wayland
                  pkgs.xorg.libX11
                  pkgs.xorg.libXcursor
                  pkgs.xorg.libXi
                  pkgs.xorg.libXrandr
                  pkgs.vulkan-loader
                  pkgs.openblas
                  pkgs.onnxruntime  # Uses same protobuf as sentencepiece (unified via overlay)
                  pkgs.alsa-lib
                  pkgs.pipewire  # For ALSA-PipeWire plugin compatibility
                  pkgs.portaudio
                ]} \
                --set ALSA_PLUGIN_DIR ${pkgs.pipewire}/lib/alsa-lib \
                --prefix VK_DRIVER_FILES : /run/opengl-driver/share/vulkan/icd.d/nvidia_icd.x86_64.json:/run/opengl-driver/share/vulkan/icd.d/radeon_icd.x86_64.json:/run/opengl-driver/share/vulkan/icd.d/intel_icd.x86_64.json:/run/opengl-driver/share/vulkan/icd.d/intel_hasvk_icd.x86_64.json \
                --prefix VK_ICD_FILENAMES : /run/opengl-driver/share/vulkan/icd.d/nvidia_icd.x86_64.json:/run/opengl-driver/share/vulkan/icd.d/radeon_icd.x86_64.json:/run/opengl-driver/share/vulkan/icd.d/intel_icd.x86_64.json:/run/opengl-driver/share/vulkan/icd.d/intel_hasvk_icd.x86_64.json

              # Install desktop file
              mkdir -p $out/share/applications
              install -m 644 ${./desktop/dev.sonori.desktop} $out/share/applications/

              # Install AppStream metadata
              mkdir -p $out/share/metainfo
              install -m 644 ${./desktop/dev.sonori.metainfo.xml} $out/share/metainfo/

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
