#!/usr/bin/env bash
# Readies a Debian system (or container) for `run.mjs --profile`: perf, for
# this kernel; binutils, whose c++filt names Rust's symbols; libc's debug
# symbols (memcpy and its kin, by name); and the web engine's (WebKitGTK's
# and JavaScriptCore's), exactly the version installed, without which its
# functions show as addresses. The debug archive keeps only the newest
# build of each, so those come from snapshot.debian.org, by version. Needs
# sudo. A profile is read where it was recorded: the symbols are there.
#
#   tests/e2e/profile-setup.sh
set -euo pipefail

sudo apt-get install -y -qq linux-perf binutils libc6-dbg curl ca-certificates jq > /dev/null
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
for pkg in libwebkit2gtk-4.1-0 libjavascriptcoregtk-4.1-0; do
  version=$(dpkg-query -W -f '${Version}' "$pkg" 2> /dev/null) || {
    echo "$pkg isn't installed: no symbols for it"
    continue
  }
  if [ "$(dpkg-query -W -f '${Version}' "$pkg-dbgsym" 2> /dev/null || true)" = "$version" ]; then
    echo "$pkg-dbgsym $version: already there"
    continue
  fi
  hash=$(curl -fsS "https://snapshot.debian.org/mr/binary/$pkg-dbgsym/$version/binfiles" |
    jq -r '.result[] | select(.architecture == "amd64") | .hash')
  [ -n "$hash" ] || {
    echo "no $pkg-dbgsym $version on snapshot.debian.org"
    continue
  }
  curl -fsS -o "$work/$pkg-dbgsym.deb" "https://snapshot.debian.org/file/$hash"
  # The archive names a file by its SHA-1: what came must be that file.
  echo "$hash  $work/$pkg-dbgsym.deb" | sha1sum -c --quiet - || {
    echo "$pkg-dbgsym $version: the file fetched isn't the one asked for; not installed"
    exit 1
  }
  sudo dpkg -i "$work/$pkg-dbgsym.deb" > /dev/null
  echo "$pkg-dbgsym $version: installed"
done
perf --version
