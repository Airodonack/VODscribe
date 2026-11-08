#!/bin/bash
# The app on Servo as an AppImage, with GStreamer 1.28.8 and the patched
# mselib inside it. GStreamer comes from scripts/servo-gstreamer.sh, built
# on this system (Ubuntu 24.04 in CI) against its GLib; the app from this
# repository's flake (flake.nix), which the script enters unless it runs in
# it already:
#
#   scripts/servo-gstreamer.sh            # outside Nix
#   scripts/servo-appimage.sh [out folder, default target/servo-appimage]
#
# VODSCRIBE_GST_PREFIX names another GStreamer prefix than
# target/servo-gstreamer. The script builds the workers and the app (pnpm
# tauri:servo build) with tauri.servo-appimage.conf.json over the app's
# config: an AppImage only, with the CPU worker. The bundler takes in that
# GStreamer's libraries, the plugins listed below and the plugin scanner, and
# every other library from this system, GTK's among them, but GLib, which
# stays the user's system's: GStreamer and GTK were built against Ubuntu
# 24.04's 2.80 and run on it or newer. A launcher step
# (apprun-hooks/vodscribe-servo.sh) then sets X11, a GStreamer registry of
# the app's own, and no update check (the releases it would update to have no
# Servo), and the folder is packed again. The out folder gets the AppImage
# and its .sha256. Linux x86_64 on X11 only.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
if [ "${VODSCRIBE_SERVO_SHELL:-}" != flake ]; then
  exec nix --extra-experimental-features "nix-command flakes" develop "$root" -c bash "$0" "$@"
fi
out=$(mkdir -p "${1:-$root/target/servo-appimage}" && cd "${1:-$root/target/servo-appimage}" && pwd)
target=${CARGO_TARGET_DIR:-$root/target}
gst=${VODSCRIBE_GST_PREFIX:-$target/servo-gstreamer}
[ -x "$gst/libexec/gstreamer-1.0/gst-plugin-scanner" ] || { echo "no GStreamer in $gst: run scripts/servo-gstreamer.sh first" >&2; exit 1; }
work=$target/servo-appimage-work
rm -rf "$work"
mkdir -p "$work/plugins" "$work/helpers" "$work/bin"

# The plugins the app plays with: its pages' media (MP4 and WebM through
# Media Source Extensions, H.264, AAC, VP8/9, AV1, Opus, Vorbis, Theora), Servo's
# player and Web Audio pipelines, and the sound output.
plugins=(
  coreelements
  app audioconvert audioresample gio ogg opengl opus pbtypes playback theora
  typefindfunctions videoconvertscale volume vorbis alsa
  audiofx audioparsers autodetect deinterlace id3demux interleave isomp4
  matroska pulseaudio videofilter vpx wavparse
  mse aom audiobuffersplit fdkaac id3tag openh264 proxy videoparsersbad
)
for name in "${plugins[@]}"; do
  [ -f "$gst/lib/gstreamer-1.0/libgst$name.so" ] || { echo "no GStreamer plugin $name in $gst" >&2; exit 1; }
  ln -s "$gst/lib/gstreamer-1.0/libgst$name.so" "$work/plugins/"
done
ln -s "$gst/libexec/gstreamer-1.0/gst-plugin-scanner" "$work/helpers/"

# The bundler finds libraries with `ldd` and GTK's with pkg-config, and runs
# GLib's tools: the system's, not the shell's, which would find Nix's.
for tool in ldd pkg-config pkgconf glib-compile-schemas gio-querymodules; do
  if [ -x "/usr/bin/$tool" ]; then
    ln -s "/usr/bin/$tool" "$work/bin/"
  fi
done

cd "$root"
pnpm install --frozen-lockfile
scripts/prepare-sidecars.sh release
appimage=(--config "$root/apps/desktop/src-tauri/tauri.servo-appimage.conf.json")
bash scripts/tauri-servo.sh build --no-bundle "${appimage[@]}"
PATH=$work/bin:$PATH LD_LIBRARY_PATH=$gst/lib GSTREAMER_PLUGINS_DIR=$work/plugins GSTREAMER_HELPERS_DIR=$work/helpers \
  bash scripts/tauri-servo.sh build --bundles appimage "${appimage[@]}"

bundle=$target/release/bundle/appimage
appdir=$(ls -d "$bundle"/*.AppDir)
built=$(ls "$bundle"/*.AppImage)

cat >"$appdir/apprun-hooks/vodscribe-servo.sh" <<'EOF'
# Servo's window is X11 only (XWayland on Wayland). GStreamer 1.28 keeps a
# registry apart from the system GStreamer's. The releases the updater offers
# have no Servo in them.
export GDK_BACKEND=x11
export GST_REGISTRY_1_0="${XDG_CACHE_HOME:-$HOME/.cache}/vodscribe-servo/gstreamer-1.28.8-registry.bin"
export VODSCRIBE_NO_UPDATE_CHECK=1
EOF
grep -q 'apprun-hooks/"linuxdeploy-plugin-gstreamer.sh"' "$appdir/AppRun" || { echo "AppRun doesn't source the GStreamer hook" >&2; exit 1; }
# shellcheck disable=SC2016 # $this_dir is AppRun's own variable.
sed -i 's|^\(source .*apprun-hooks/"linuxdeploy-plugin-gstreamer.sh"\)$|\1\nsource "$this_dir"/apprun-hooks/"vodscribe-servo.sh"|' "$appdir/AppRun"

# GLib is the user's system's (see above), and so are the libraries it
# needs, which a newer GLib needs newer versions of (Debian 13's GIO needs
# libmount 2.40, Ubuntu 24.04 has 2.39).
rm -f "$appdir"/usr/lib/lib{glib,gobject,gio,gmodule,gthread}-2.0.so* \
  "$appdir"/usr/lib/lib{mount,blkid,selinux,ffi,pcre2-8}.so*

# Nothing in the folder may need Nix or WebKitGTK, and every library, glibc
# version and GLib or GStreamer function it needs is in it or on this system.
if find "$appdir" -name '*webkit*' | grep .; then
  echo "WebKitGTK files in the AppImage" >&2
  exit 1
fi
while IFS= read -r -d '' elf; do
  file -b "$elf" | grep -q '^ELF' || continue
  if patchelf --print-interpreter "$elf" 2>/dev/null | grep -q /nix/store ||
    patchelf --print-rpath "$elf" 2>/dev/null | grep -q /nix/store; then
    echo "$elf still points into /nix/store" >&2
    exit 1
  fi
  if LD_LIBRARY_PATH=$appdir/usr/lib /usr/bin/ldd -r "$elf" 2>&1 | grep -E 'not found|undefined symbol: g'; then
    echo "in $elf" >&2
    exit 1
  fi
done < <(find "$appdir" -type f -print0)

# Packed again with the AppImage tool Tauri's bundler downloaded.
packer=${XDG_CACHE_HOME:-$HOME/.cache}/tauri/linuxdeploy-plugin-appimage.AppImage
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
name=vodscribe-servo_${version}_x86_64.AppImage
rm -f "$built" "$out/$name" "$out/$name.sha256"
(cd "$bundle" && ARCH=x86_64 OUTPUT="$out/$name" APPIMAGE_EXTRACT_AND_RUN=1 "$packer" --appimage-extract-and-run --appdir "$appdir")
(cd "$out" && sha256sum "$name" >"$name.sha256")
ls -l "$out/$name"
cat "$out/$name.sha256"
