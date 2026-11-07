#!/bin/bash
# The desktop app on Servo: `tauri dev` or `tauri build` with the `servo`
# feature instead of the system web view, inside Servo's Nix shell.
#
#   [VODSCRIBE_SERVO_DIR=<folder>] pnpm tauri:servo dev|build [tauri args]
#
# Cargo takes the Servo runtime and the Servo fork from GitHub, at the
# revisions Cargo.lock pins, and the Nix shell comes from that Servo
# revision: its shell.nix and the files it reads, fetched once into
# $XDG_CACHE_HOME/vodscribe/servo-shell/<rev>. The app then runs on the
# system's GStreamer.
#
# VODSCRIBE_SERVO_DIR names local checkouts to build from instead: servo/ (the
# fork, whose shell.nix is used), tauri-servo/tauri-runtime-servo/ and
# scripts/gst-128-env.sh (GStreamer 1.28 with the patched mselib), which the
# app is started with. Cargo builds in CARGO_TARGET_DIR, else
# $VODSCRIBE_SERVO_DIR/target, else target/. Linux on X11 only. Both commands
# build Servo with the release profile (dev runs a release build: a debug
# Servo is too slow to use), without LTO, so one build serves both.
set -euo pipefail

cmd=${1:-}
case $cmd in
  dev) extra=(--release) ;;
  build) extra=() ;;
  *) echo "usage: pnpm tauri:servo dev|build [tauri args]" >&2; exit 2 ;;
esac
shift
root=$(cd "$(dirname "$0")/.." && pwd)
desktop=$root/apps/desktop
export NIX_PATH=nixpkgs=flake:nixpkgs
cargo_args=(--no-default-features)
gst=

if [ -n "${VODSCRIBE_SERVO_DIR:-}" ]; then
  shell_dir=$VODSCRIBE_SERVO_DIR/servo
  runtime=$VODSCRIBE_SERVO_DIR/tauri-servo/tauri-runtime-servo
  gst=$VODSCRIBE_SERVO_DIR/scripts/gst-128-env.sh
  [ -f "$shell_dir/shell.nix" ] || { echo "no $shell_dir/shell.nix: VODSCRIBE_SERVO_DIR should hold the Servo fork in servo/" >&2; exit 1; }
  [ -f "$runtime/Cargo.toml" ] || { echo "no $runtime/Cargo.toml: VODSCRIBE_SERVO_DIR should hold the runtime in tauri-servo/tauri-runtime-servo/" >&2; exit 1; }
  [ -f "$gst" ] || { echo "no $gst: VODSCRIBE_SERVO_DIR should hold scripts/gst-128-env.sh" >&2; exit 1; }
  # Cargo makes a path dependency inside this workspace one of its members,
  # and Servo's crates then can't inherit from Servo's own workspace.
  case $(cd "$VODSCRIBE_SERVO_DIR" && pwd -P)/ in
    "$(cd "$root" && pwd -P)"/*) echo "VODSCRIBE_SERVO_DIR should be outside this repository" >&2; exit 1 ;;
  esac
  export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$VODSCRIBE_SERVO_DIR/target}
  # The runtime, and each Servo crate it depends on, from the checkouts.
  # Servo's other crates follow through libservo's own path dependencies.
  cargo_args+=(--config "patch.\"https://github.com/Airodonack/tauri-runtime-servo\".tauri-runtime-servo.path=\"$runtime\"")
  for crate in servo=components/servo servo-net-traits=components/shared/net servo-webdriver-server=components/webdriver_server; do
    cargo_args+=(--config "patch.\"https://github.com/Airodonack/servo\".${crate%%=*}.path=\"$shell_dir/${crate#*=}\"")
  done
  # Cargo writes the patched sources into Cargo.lock: put the pinned one back
  # when the command ends.
  lock_copy=$(mktemp)
  cp "$root/Cargo.lock" "$lock_copy"
  trap 'cp "$lock_copy" "$root/Cargo.lock"; rm -f "$lock_copy"' EXIT
else
  rev=$(sed -n 's|^source = "git+https://github.com/Airodonack/servo?rev=[0-9a-f]*#\([0-9a-f]*\)"$|\1|p' "$root/Cargo.lock" | head -n 1)
  [ -n "$rev" ] || { echo "no Airodonack/servo revision in Cargo.lock" >&2; exit 1; }
  shell_dir=${XDG_CACHE_HOME:-$HOME/.cache}/vodscribe/servo-shell/$rev
  if [ ! -f "$shell_dir/shell.nix" ]; then
    mkdir -p "$(dirname "$shell_dir")"
    fetch=$(mktemp -d "$shell_dir.XXXXXX")
    git -C "$fetch" init -q
    git -C "$fetch" sparse-checkout set --no-cone /shell.nix /rust-toolchain.toml /support/crown/
    git -C "$fetch" fetch -q --depth 1 --filter=blob:none https://github.com/Airodonack/servo "$rev"
    git -C "$fetch" checkout -q FETCH_HEAD
    mv "$fetch" "$shell_dir"
  fi
fi

# Inside the shell: Servo's build environment as its mach sets it (the
# toolchain and RUSTC_BOOTSTRAP list of the fork's rust-toolchain.toml and
# .cargo/config.toml, clang, Debian's GTK .pc files beside Nix's), the
# release profile the Servo runtime builds with, then GStreamer and X11 for
# the app that `tauri dev` starts.
args=$(printf '%q ' "$cmd" --features servo "${extra[@]}" "$@" -- "${cargo_args[@]}")
# Started from the folder of Servo's shell.nix, which keeps its Rust
# toolchains there (.rustup), not in this repository.
cd "$shell_dir"
nix-shell shell.nix --run "
  set -e
  export PKG_CONFIG_PATH_FOR_TARGET=\${PKG_CONFIG_PATH_FOR_TARGET:-}:/usr/lib/x86_64-linux-gnu/pkgconfig:/usr/share/pkgconfig
  export CC=clang CXX=clang++ SERVO_STYLE_THREAD_STACK_SIZE_KB=8192 RUSTFLAGS=
  export RUSTUP_TOOLCHAIN=1.99.1
  export RUSTC_BOOTSTRAP=crown,script,script_webcrypto,script_webgpu,script_bindings,style_tests,mozjs,mozjs_sys
  export CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 CARGO_PROFILE_RELEASE_PANIC=unwind CARGO_PROFILE_RELEASE_STRIP=false
  ${gst:+source $(printf '%q' "$gst")}
  export GDK_BACKEND=x11
  unset WAYLAND_DISPLAY
  cd $(printf '%q' "$desktop")
  pnpm tauri $args"
