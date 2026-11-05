#!/usr/bin/env bash
# Downloads cargo-about 0.9.2 (Linux x86_64), checks it against its pinned
# SHA-256, unpacks it into <dir> and prints the binary's path, for
# scripts/third-party-notices.py's $CARGO_ABOUT. CI's notices check and the
# Dependabot notices workflow both use it, so the pin lives in one place.
#
#   CARGO_ABOUT=$(bash scripts/fetch-cargo-about.sh <dir>)
set -euo pipefail

dir=${1:?usage: fetch-cargo-about.sh <dir>}
version=0.9.2
name=cargo-about-$version-x86_64-unknown-linux-musl
sha256=9099a59e820c38a68b9d65f300662a567d56562f9a10f6aa4c7e86c17c2566af

mkdir -p "$dir"
cd "$dir"
curl -fsSLO "https://github.com/EmbarkStudios/cargo-about/releases/download/$version/$name.tar.gz"
# sha256sum's own report goes to stderr, so stdout holds only the path.
echo "$sha256  $name.tar.gz" | sha256sum -c - >&2
tar xzf "$name.tar.gz"
echo "$PWD/$name/cargo-about"
