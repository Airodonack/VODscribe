#!/usr/bin/env bash
# Builds the app for profiling (tests/e2e/README.md, "Profiling"): release
# code with its symbols, line tables and frame pointers (perf's cheapest
# way up the stack), and the page unminified. The binary lands in
# target/profiling/ (under CARGO_TARGET_DIR if that is set), with the
# transcription worker beside it, where the app looks for it, and is run
# directly: it isn't packaged.
#
#   scripts/build-profiling.sh
#
# Needs what a release build needs: the workers made first
# (scripts/prepare-sidecars.sh release).
set -euo pipefail
cd "$(dirname "$0")/.."

triple="${TARGET_TRIPLE:-$(rustc -vV | sed -n 's/^host: //p')}"
target="${CARGO_TARGET_DIR:-target}/profiling"
export RUSTFLAGS="${RUSTFLAGS:-} -C force-frame-pointers=yes"
export VODSCRIBE_PROFILING=1
pnpm --filter desktop tauri build --no-bundle -- --profile profiling
cp apps/desktop/src-tauri/binaries/vodscribe-worker-"$triple" "$target/vodscribe-worker"
echo "built: $target/vodscribe"
