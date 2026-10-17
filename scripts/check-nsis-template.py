#!/usr/bin/env python3
"""Fails when the Tauri CLI in use ships an installer template other than
the one our copy (apps/desktop/src-tauri/windows/installer.nsi) was made
from: then the copy must be taken again from the new template and its
"VODscribe:" change reapplied, or the installer would build from an old one.

Run after `pnpm install`. Reads the CLI version from node_modules and the
template it was made from (its sha256) from the copy's header.
"""

import hashlib
import json
import re
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
COPY = ROOT / "apps/desktop/src-tauri/windows/installer.nsi"
CLI = ROOT / "apps/desktop/node_modules/@tauri-apps/cli/package.json"
UPSTREAM = (
    "https://raw.githubusercontent.com/tauri-apps/tauri/tauri-cli-v{version}"
    "/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi"
)


def main() -> int:
    version = json.loads(CLI.read_text())["version"]
    header = COPY.read_text()[:1000]
    made_from = re.search(r"sha256 ([0-9a-f]{64})", header)
    if not made_from:
        print(f"{COPY}: no sha256 of the template it was made from in its header")
        return 1
    with urllib.request.urlopen(UPSTREAM.format(version=version), timeout=30) as response:
        upstream = hashlib.sha256(response.read()).hexdigest()
    if upstream != made_from.group(1):
        print(
            f"Tauri CLI {version}'s installer template (sha256 {upstream}) isn't the one "
            f"{COPY.relative_to(ROOT)} was made from ({made_from.group(1)}). Take the new "
            "template, reapply the changes marked 'VODscribe:', and update the header."
        )
        return 1
    print(f"installer.nsi matches Tauri CLI {version}'s template")
    return 0


if __name__ == "__main__":
    sys.exit(main())
