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

# sherpa-onnx's build script would download its prebuilt libraries unverified.
# Instead this downloads them, checks them against pinned hashes, unpacks them,
# and points each build at them with SHERPA_ONNX_LIB_DIR, so the build script
# never downloads anything. Unpacking here, every time it's missing, also keeps
# a cached build (CI's rust-cache) from linking against a folder that's gone.
version=1.13.8
cache="${CARGO_TARGET_DIR:-target}/sherpa-onnx-prebuilt"
# An absolute path for the build script; Git Bash's /d/... wouldn't do on Windows.
root=$(pwd -W 2>/dev/null || pwd)
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
  local dir="${name%.tar.bz2}"
  if [ "$(cat "$cache/$dir.verified" 2>/dev/null)" != "$want" ] || [ ! -d "$cache/$dir/lib" ]; then
    rm -rf "${cache:?}/$dir"
    # Windows' own tar reads .bz2 without needing bzip2 installed.
    if [ -x /c/Windows/System32/tar.exe ]; then
      /c/Windows/System32/tar.exe -xf "$cache/$name" -C "$cache"
    else
      tar -xjf "$cache/$name" -C "$cache"
    fi
    echo "$want" > "$cache/$dir.verified"
  fi
}
# The lib dir of the verified prebuilt `$1`, for SHERPA_ONNX_LIB_DIR.
lib_dir() {
  case "$cache" in
    /*) echo "$cache/sherpa-onnx-v$version-$1/lib" ;;
    *) echo "$root/$cache/sherpa-onnx-v$version-$1/lib" ;;
  esac
}
# The static libraries for the CPU worker, and the shared ones the GPU worker links against.
# The "no-tts" builds leave out text-to-speech, which would bring in espeak-ng (GPL-3.0);
# third_party/sherpa-onnx-sys links them.
shared=""
case "$triple" in
  x86_64-unknown-linux-gnu) static=linux-x64-static-no-tts-lib shared=linux-x64-shared-no-tts-lib ;;
  x86_64-pc-windows-msvc) static=win-x64-static-MT-Release-no-tts-lib shared=win-x64-shared-MT-Release-no-tts-lib ;;
  x86_64-apple-darwin) static=osx-x64-static-no-tts-lib ;;
  aarch64-apple-darwin) static=osx-arm64-static-no-tts-lib ;;
  *) echo "no pinned sherpa-onnx libraries for $triple" >&2; exit 1 ;;
esac
verified_prebuilt "$static"
[ -z "$shared" ] || verified_prebuilt "$shared"

# CPU worker: sherpa-onnx linked statically.
# ${flag[@]+...}: macOS's bash 3.2 treats an empty array as unset under set -u.
SHERPA_ONNX_LIB_DIR=$(lib_dir "$static") cargo build ${flag[@]+"${flag[@]}"} -p vodscribe-worker --target "$triple"
cp "${CARGO_TARGET_DIR:-target}/$triple/$profile/vodscribe-worker$exe" "$out/vodscribe-worker-$triple$exe"

# GPU worker: sherpa-onnx linked dynamically. The no-tts shared library ships
# with the app (apps/desktop/src-tauri/lib, a bundle resource) and the GPU pack
# supplies ONNX Runtime with CUDA at runtime. Built in its own target dir so
# its features don't mix with the static build.
case "$triple" in
  x86_64-unknown-linux-gnu | x86_64-pc-windows-msvc)
    lib=apps/desktop/src-tauri/lib
    SHERPA_ONNX_LIB_DIR=$(lib_dir "$shared") cargo build ${flag[@]+"${flag[@]}"} -p vodscribe-worker \
      --no-default-features --features shared \
      --target "$triple" --target-dir target/cuda
    cp "target/cuda/$triple/$profile/vodscribe-worker$exe" "$out/vodscribe-worker-cuda-$triple$exe"
    mkdir -p "$lib"
    case "$triple" in
      *linux*) cp "$(lib_dir "$shared")/libsherpa-onnx-c-api.so" "$lib/" ;;
      *) cp "$(lib_dir "$shared")/sherpa-onnx-c-api.dll" "$lib/" ;;
    esac
    # The build bakes this machine's sherpa-onnx lib dir into the RUNPATH. Release
    # binaries find the GPU pack's libs via LD_LIBRARY_PATH instead, so don't ship it.
    if [ "$profile" = release ] && [[ "$triple" == *linux* ]]; then
      patchelf --set-rpath '$ORIGIN' "$out/vodscribe-worker-cuda-$triple$exe"
    fi
    ;;
esac
ls -la "$out"
[ ! -d "${lib:-}" ] || ls -la "$lib"
