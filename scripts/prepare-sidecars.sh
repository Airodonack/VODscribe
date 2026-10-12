#!/usr/bin/env bash
# Builds the transcription workers and places them where Tauri's `externalBin`
# expects them: apps/desktop/src-tauri/binaries/<name>-<target-triple>[.exe]. Tauri copies
# them next to the app executable, where the app looks for them.
#
#   scripts/prepare-sidecars.sh [debug|release]
#
# The GPU worker is built on Linux and Windows (x86_64) only; macOS has no CUDA.
set -euo pipefail
cd "$(dirname "$0")/.."

profile="${1:-debug}"
flag=()
[ "$profile" = release ] && flag=(--release)
triple="${TARGET_TRIPLE:-$(rustc -vV | sed -n 's/^host: //p')}"
exe=""
case "$triple" in *windows*) exe=".exe" ;; esac
out=apps/desktop/src-tauri/binaries
mkdir -p "$out"

# sherpa-onnx's build script downloads its prebuilt libraries unverified and
# reuses whatever it finds in its cache. Put a verified archive there first, and
# drop any extraction not made from it, so the build unpacks the verified one.
version=1.13.8
# sherpa-onnx's build script keeps its downloads under the Cargo target dir.
cache="${CARGO_TARGET_DIR:-target}/sherpa-onnx-prebuilt"
sha256() { { sha256sum "$1" 2>/dev/null || shasum -a 256 "$1"; } | awk '{print $1}'; }
verified_prebuilt() {
  local name="sherpa-onnx-v$version-$1.tar.bz2"
  local want
  want=$(awk -v n="$name" '$2 == n {print $1}' scripts/sherpa-onnx-prebuilt.sha256)
  [ -n "$want" ] || { echo "no pinned hash for $name" >&2; exit 1; }
  mkdir -p "$cache"
  if [ ! -f "$cache/$name" ] || [ "$(sha256 "$cache/$name")" != "$want" ]; then
    curl -fsSL -o "$cache/$name" "https://github.com/k2-fsa/sherpa-onnx/releases/download/v$version/$name"
    if [ "$(sha256 "$cache/$name")" != "$want" ]; then
      rm -f "$cache/$name"
      echo "$name doesn't match its pinned SHA-256" >&2
      exit 1
    fi
  fi
  if [ "$(cat "$cache/${name%.tar.bz2}.verified" 2>/dev/null)" != "$want" ]; then
    rm -rf "${cache:?}/${name%.tar.bz2}"
    echo "$want" > "$cache/${name%.tar.bz2}.verified"
  fi
}
case "$triple" in
  x86_64-unknown-linux-gnu) verified_prebuilt linux-x64-static-lib; verified_prebuilt linux-x64-shared-lib ;;
  x86_64-pc-windows-msvc) verified_prebuilt win-x64-static-MT-Release-lib; verified_prebuilt win-x64-shared-MT-Release-lib ;;
  x86_64-apple-darwin) verified_prebuilt osx-x64-static-lib ;;
  aarch64-apple-darwin) verified_prebuilt osx-arm64-static-lib ;;
  *) echo "no pinned sherpa-onnx libraries for $triple" >&2; exit 1 ;;
esac

# CPU worker: sherpa-onnx linked statically.
# ${flag[@]+...}: macOS's bash 3.2 treats an empty array as unset under set -u.
cargo build ${flag[@]+"${flag[@]}"} -p vodscribe-worker --target "$triple"
cp "${CARGO_TARGET_DIR:-target}/$triple/$profile/vodscribe-worker$exe" "$out/vodscribe-worker-$triple$exe"

# GPU worker: sherpa-onnx linked dynamically; the GPU pack supplies the CUDA
# build at runtime. Built in its own target dir so its features don't mix
# with the static build.
case "$triple" in
  x86_64-unknown-linux-gnu | x86_64-pc-windows-msvc)
    cargo build ${flag[@]+"${flag[@]}"} -p vodscribe-worker --no-default-features --features shared \
      --target "$triple" --target-dir target/cuda
    cp "target/cuda/$triple/$profile/vodscribe-worker$exe" "$out/vodscribe-worker-cuda-$triple$exe"
    # The build bakes this machine's sherpa-onnx lib dir into the RUNPATH. Release
    # binaries find the GPU pack's libs via LD_LIBRARY_PATH instead, so don't ship it.
    if [ "$profile" = release ] && [[ "$triple" == *linux* ]]; then
      patchelf --set-rpath '$ORIGIN' "$out/vodscribe-worker-cuda-$triple$exe"
    fi
    ;;
esac
ls -la "$out"
