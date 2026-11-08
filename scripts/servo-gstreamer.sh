#!/bin/bash
# GStreamer 1.28.8 with the patched mselib, for the Servo AppImage: core,
# base, good and bad from Airodonack/gstreamer (branch mse-fixes-1.28),
# built with meson against the system's own GLib, with only the plugins the
# app plays with. Then mselib's unit tests. Run it outside Nix, on the
# system the AppImage is built on (Ubuntu 24.04 for the published one):
#
#   scripts/servo-gstreamer.sh [prefix, default target/servo-gstreamer]
#
# scripts/servo-appimage.sh takes it from that prefix. Needs meson 1.4 or
# newer (Ubuntu 24.04 has 1.3: `pip install meson==1.6.1` in a venv), git,
# and on Ubuntu 24.04:
#
#   apt install libgtk-3-dev ninja-build flex bison libglib2.0-dev \
#     liborc-0.4-dev libgl-dev libegl-dev libgles-dev libx11-dev \
#     libx11-xcb-dev libxext-dev libasound2-dev libpulse-dev libogg-dev \
#     libvorbis-dev libtheora-dev libopus-dev libvpx-dev libfdk-aac-dev \
#     libopenh264-dev libaom-dev zlib1g-dev libbz2-dev
#
# (GTK's for the app, the rest for GStreamer; libfdk-aac-dev is in
# multiverse.)
set -euo pipefail

rev=5ce1c32454cf2ac6195ee8fc717ca3d3e1c8070c
root=$(cd "$(dirname "$0")/.." && pwd)
prefix=$(mkdir -p "${1:-$root/target/servo-gstreamer}" && cd "${1:-$root/target/servo-gstreamer}" && pwd)
build=${CARGO_TARGET_DIR:-$root/target}/servo-gstreamer-build
src=$build/src

if [ "$(git -C "$src" rev-parse HEAD 2>/dev/null)" != "$rev" ]; then
  rm -rf "$src"
  git init -q "$src"
  git -C "$src" fetch -q --depth 1 https://github.com/Airodonack/gstreamer "$rev"
  git -C "$src" checkout -q FETCH_HEAD
fi

export PKG_CONFIG_PATH=$prefix/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}
# bad's tests ask for core's gst-tester-1.0, which only a monorepo build has,
# for its validate tests: a stand-in that skips them (libs_mse doesn't use it).
mkdir -p "$build/bin"
printf '#!/bin/sh\nexit 77\n' >"$build/bin/gst-tester-1.0"
chmod +x "$build/bin/gst-tester-1.0"
export PATH=$build/bin:$PATH
# Each module on its own, every optional feature off but what's named.
module() {
  local name=$1
  shift
  local again=()
  [ ! -d "$build/$name/meson-private" ] || again=(--reconfigure)
  meson setup "${again[@]}" "$build/$name" "$src/subprojects/$name" \
    --prefix="$prefix" --libdir=lib --buildtype=release \
    -Dauto_features=disabled -Dexamples=disabled -Dnls=disabled -Ddoc=disabled "$@"
  meson compile -C "$build/$name"
  meson install -C "$build/$name" --quiet
}
# libgstcheck for the unit tests, and the tools, which bring the plugin
# scanner; no libunwind or libdw for backtraces.
module gstreamer -Dintrospection=disabled -Dcheck=enabled -Dtools=enabled -Dtests=disabled -Dbenchmarks=disabled
# The plugins: the pages' media (MP4 and WebM through Media Source
# Extensions, H.264, AAC, VP8/9, AV1, Opus, Vorbis, Theora), Servo's player and
# Web Audio pipelines, and the sound output.
module gst-plugins-base -Dintrospection=disabled -Dtests=disabled -Dorc=enabled \
  -Dapp=enabled -Daudioconvert=enabled -Daudioresample=enabled -Dgio=enabled -Dogg=enabled \
  -Dopus=enabled -Dpbtypes=enabled -Dplayback=enabled -Dtheora=enabled -Dtypefind=enabled \
  -Dvideoconvertscale=enabled -Dvolume=enabled -Dvorbis=enabled -Dalsa=enabled \
  -Dx11=enabled -Dgl=enabled -Dgl_api=opengl,gles2 -Dgl_platform=glx,egl -Dgl_winsys=x11,egl
module gst-plugins-good -Dtests=disabled -Dorc=enabled \
  -Daudiofx=enabled -Daudioparsers=enabled -Dautodetect=enabled -Ddeinterlace=enabled \
  -Did3demux=enabled -Dinterleave=enabled -Disomp4=enabled -Dmatroska=enabled -Dpulse=enabled \
  -Dvideofilter=enabled -Dvpx=enabled -Dwavparse=enabled
module gst-plugins-bad -Dintrospection=disabled -Dtests=enabled -Dorc=enabled \
  -Dmse=enabled -Daudiobuffersplit=enabled -Dfdkaac=enabled -Did3tag=enabled \
  -Dopenh264=enabled -Dproxy=enabled -Dvideoparsers=enabled -Daom=enabled

# mselib's tests, with the demuxers of good on the plugin path (meson's test
# environment lacks them).
meson compile -C "$build/gst-plugins-bad" tests/check/libs_mse
registry=$(mktemp)
LD_LIBRARY_PATH=$prefix/lib GST_REGISTRY=$registry GST_REGISTRY_FORK=no GST_PLUGIN_SYSTEM_PATH_1_0='' \
  GST_PLUGIN_PATH_1_0=$prefix/lib/gstreamer-1.0 CK_DEFAULT_TIMEOUT=60 GST_STATE_IGNORE_ELEMENTS='' \
  "$build/gst-plugins-bad/tests/check/libs_mse"
rm -f "$registry"
