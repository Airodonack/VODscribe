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

# CPU worker: sherpa-onnx linked statically.
cargo build "${flag[@]}" -p vodscribe-worker --target "$triple"
cp "target/$triple/$profile/vodscribe-worker$exe" "$out/vodscribe-worker-$triple$exe"

# GPU worker: sherpa-onnx linked dynamically; the GPU pack supplies the CUDA
# build at runtime. Built in its own target dir so its features don't mix
# with the static build.
case "$triple" in
  x86_64-unknown-linux-gnu | x86_64-pc-windows-msvc)
    cargo build "${flag[@]}" -p vodscribe-worker --no-default-features --features shared \
      --target "$triple" --target-dir target/cuda
    cp "target/cuda/$triple/$profile/vodscribe-worker$exe" "$out/vodscribe-worker-cuda-$triple$exe"
    ;;
esac
ls -la "$out"
