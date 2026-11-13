#!/usr/bin/env bash
# Installs the system packages for a source build of Sonori on Debian/Ubuntu,
# Fedora/RHEL and Arch, plus ONNX Runtime in /usr/local.
#   install-deps.sh          install everything (uses sudo when not root)
#   install-deps.sh --env    print KEY=VALUE build variables, for `export $(...)` or $GITHUB_ENV
set -euo pipefail

# The ort crate in Cargo.lock (2.0.0-rc.10) needs this ONNX Runtime API version.
ONNX_VERSION=1.22.0

family() {
  . /etc/os-release
  for id in $ID ${ID_LIKE:-}; do
    case $id in
      debian | ubuntu) echo debian; return ;;
      fedora | rhel) echo fedora; return ;;
      arch) echo arch; return ;;
    esac
  done
  echo "install-deps.sh: unsupported distro '$ID'; see the package lists in this script" >&2
  exit 1
}

FAMILY=$(family)

if [ "${1:-}" = --env ]; then
  case $FAMILY in
    debian) echo "BLAS_INCLUDE_DIRS=/usr/include/$(uname -m)-linux-gnu" ;;
    *) echo "BLAS_INCLUDE_DIRS=/usr/include/openblas" ;;
  esac
  echo OPENBLAS_PATH=/usr
  echo ORT_STRATEGY=system
  echo ORT_LIB_LOCATION=/usr/local/lib
  exit 0
fi

SUDO=
[ "$(id -u)" -eq 0 ] || SUDO=sudo

case $FAMILY in
  debian)
    $SUDO apt-get update
    $SUDO env DEBIAN_FRONTEND=noninteractive apt-get install -y \
      build-essential pkg-config cmake git curl ca-certificates libclang-dev \
      portaudio19-dev libasound2-dev libfftw3-dev libssl-dev libopenblas-dev \
      libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev libxi-dev libxrandr-dev \
      libvulkan-dev spirv-headers glslc
    ;;
  fedora)
    $SUDO dnf install -y \
      gcc gcc-c++ make pkgconf-pkg-config cmake git curl tar gzip clang clang-devel \
      portaudio-devel alsa-lib-devel fftw-devel openssl-devel openblas-devel \
      libxkbcommon-devel wayland-devel libX11-devel libXcursor-devel libXi-devel libXrandr-devel \
      vulkan-loader-devel vulkan-headers spirv-headers-devel glslc
    ;;
  arch)
    $SUDO pacman -Syu --needed --noconfirm \
      base-devel pkgconf cmake git curl clang \
      portaudio alsa-lib fftw openssl openblas cblas \
      libxkbcommon wayland libx11 libxcursor libxi libxrandr \
      vulkan-headers vulkan-icd-loader spirv-headers shaderc
    ;;
esac

# The upstream build on every distro keeps the ONNX Runtime version identical.
case $(uname -m) in
  x86_64) ort_arch=x64 ;;
  aarch64) ort_arch=aarch64 ;;
  *) echo "install-deps.sh: no ONNX Runtime build for $(uname -m)" >&2; exit 1 ;;
esac
ort_name=onnxruntime-linux-$ort_arch-$ONNX_VERSION
tmp=$(mktemp -d)
curl -fsSL "https://github.com/microsoft/onnxruntime/releases/download/v$ONNX_VERSION/$ort_name.tgz" |
  tar -xz -C "$tmp"
$SUDO mkdir -p /usr/local/include /usr/local/lib
$SUDO cp -r "$tmp/$ort_name/include/." /usr/local/include/
# Only the libraries: the tarball's pkg-config file names /usr/local/lib64, and
# ort-sys trusts pkg-config over ORT_LIB_LOCATION, so linking would fail.
$SUDO cp -P "$tmp/$ort_name/lib/"libonnxruntime*.so* /usr/local/lib/
rm -r "$tmp"
echo /usr/local/lib | $SUDO tee /etc/ld.so.conf.d/onnxruntime.conf >/dev/null
$SUDO ldconfig
