#!/bin/bash
# The desktop app on Servo: `tauri dev` or `tauri build` with the `servo`
# feature instead of the system web view, inside Servo's Nix shell.
#
#   VODSCRIBE_SERVO_DIR=<folder> pnpm tauri:servo dev|build [tauri args]
#
# VODSCRIBE_SERVO_DIR holds servo/ (the fork, with its shell.nix), stylo/ and
# scripts/gst-128-env.sh (GStreamer 1.28 with the patched mselib). Cargo
# builds in CARGO_TARGET_DIR, else $VODSCRIBE_SERVO_DIR/target. Linux on X11
# only. Both build Servo with the release profile (dev runs a release build:
# a debug Servo is too slow to use), without LTO, so one build serves both.
set -euo pipefail

: "${VODSCRIBE_SERVO_DIR:?set VODSCRIBE_SERVO_DIR to the folder holding servo/, stylo/ and scripts/gst-128-env.sh}"
cmd=${1:-}
case $cmd in
  dev) extra=(--release) ;;
  build) extra=() ;;
  *) echo "usage: pnpm tauri:servo dev|build [tauri args]" >&2; exit 2 ;;
esac
shift
servo=$VODSCRIBE_SERVO_DIR/servo
gst=$VODSCRIBE_SERVO_DIR/scripts/gst-128-env.sh
[ -f "$servo/shell.nix" ] || { echo "no $servo/shell.nix: VODSCRIBE_SERVO_DIR should hold the Servo fork in servo/" >&2; exit 1; }
[ -f "$gst" ] || { echo "no $gst: VODSCRIBE_SERVO_DIR should hold scripts/gst-128-env.sh" >&2; exit 1; }
desktop=$(cd "$(dirname "$0")/../apps/desktop" && pwd)
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$VODSCRIBE_SERVO_DIR/target}
export NIX_PATH=nixpkgs=flake:nixpkgs

# Inside the shell: Servo's build environment as its mach sets it (the
# toolchain and RUSTC_BOOTSTRAP list of the fork's rust-toolchain.toml and
# .cargo/config.toml, clang, Debian's GTK .pc files beside Nix's), the
# release profile the Servo runtime builds with, then GStreamer 1.28 and X11
# for the app that `tauri dev` starts.
args=$(printf '%q ' "$cmd" --features servo "${extra[@]}" "$@" -- --no-default-features)
# Started from the fork, whose shell keeps its Rust toolchains in the
# checkout it starts in (servo/.rustup), not in this repository.
cd "$servo"
exec nix-shell shell.nix --run "
  set -e
  export PKG_CONFIG_PATH_FOR_TARGET=\${PKG_CONFIG_PATH_FOR_TARGET:-}:/usr/lib/x86_64-linux-gnu/pkgconfig:/usr/share/pkgconfig
  export CC=clang CXX=clang++ SERVO_STYLE_THREAD_STACK_SIZE_KB=8192 RUSTFLAGS=
  export RUSTUP_TOOLCHAIN=1.99.1
  export RUSTC_BOOTSTRAP=crown,script,script_webcrypto,script_webgpu,script_bindings,style_tests,mozjs,mozjs_sys
  export CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 CARGO_PROFILE_RELEASE_PANIC=unwind CARGO_PROFILE_RELEASE_STRIP=false
  source $(printf '%q' "$gst")
  export GDK_BACKEND=x11
  unset WAYLAND_DISPLAY
  cd $(printf '%q' "$desktop")
  pnpm tauri $args"
