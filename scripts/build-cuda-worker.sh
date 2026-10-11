#!/bin/sh
# Builds the GPU transcription worker. It links sherpa-onnx dynamically against
# the CPU shared build (same C API); at runtime the app puts the GPU pack's CUDA
# build first on the library path. No CUDA toolkit is needed to build.
set -eu
cd "$(dirname "$0")/.."
profile="${1:-debug}"
flag=""
[ "$profile" = release ] && flag="--release"
cargo build $flag -p vodscribe-worker --no-default-features --features shared --target-dir target/cuda
cp "target/cuda/$profile/vodscribe-worker" "target/$profile/vodscribe-worker-cuda"
echo "Built target/$profile/vodscribe-worker-cuda"
