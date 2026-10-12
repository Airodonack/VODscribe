"""Writes apps/desktop/src-tauri/THIRD-PARTY-NOTICES.txt: the licenses of
everything compiled into VODscribe. The installers include it, and
vodscribe.com/third-party-notices.txt serves it.

    python3 scripts/third-party-notices.py [--check]

Needs cargo-about 0.9 (`cargo install cargo-about --locked`, or $CARGO_ABOUT)
and `pnpm install`. Rust crates come from cargo-about, which also refuses any
license not accepted in licenses/about.toml. Then the JavaScript packages
bundled into the app's interface, and ONNX Runtime, which is linked into the
worker inside sherpa-onnx's static libraries. --check fails if the file is out
of date, so CI catches a dependency change that wasn't regenerated.
"""

import json
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "apps/desktop/src-tauri/THIRD-PARTY-NOTICES.txt"
RULE = "-" * 78

# The packages in the app's JavaScript, found from its source maps. Add any
# new runtime dependency of apps/desktop here.
JS_PACKAGES = [
    "@sveltejs/kit",
    "@tauri-apps/api",
    "@tauri-apps/plugin-clipboard-manager",
    "@tauri-apps/plugin-dialog",
    "@tauri-apps/plugin-opener",
    "@tauri-apps/plugin-process",
    "@tauri-apps/plugin-updater",
    "clsx",
    "hls.js",
    "svelte",
]
LICENSE_FILES = ["LICENSE", "LICENSE.md", "license", "LICENSE-MIT", "LICENSE.txt"]


def package_dir(name: str) -> Path:
    """The copy of `name` the app's build uses, found as Node finds it: in
    apps/desktop/node_modules, or for an indirect dependency, beside a package
    that depends on it (pnpm puts a package's dependencies next to it)."""
    direct = ROOT / "apps/desktop/node_modules" / name
    if direct.is_dir():
        return direct.resolve()
    for other in JS_PACKAGES:
        d = ROOT / "apps/desktop/node_modules" / other
        if d.is_dir():
            # .../node_modules/<other>, scoped or not, then its sibling `name`.
            store = d.resolve().parents[other.count("/")]
            if (store / name / "package.json").is_file():
                return store / name
    sys.exit(f"{name} isn't installed for apps/desktop; run pnpm install")


def js_section() -> str:
    # Every runtime dependency of the app ends up in its JavaScript, so each
    # must be listed (with whatever else the bundle pulls in, like svelte).
    deps = json.loads((ROOT / "apps/desktop/package.json").read_text()).get("dependencies", {})
    missing = sorted(set(deps) - set(JS_PACKAGES))
    if missing:
        sys.exit(f"add {', '.join(missing)} to JS_PACKAGES in scripts/third-party-notices.py")
    parts = ["\nJavaScript packages in the app's interface\n"]
    tauri_mit = (package_dir("@tauri-apps/api") / "LICENSE-MIT").read_text()
    for name in JS_PACKAGES:
        d = package_dir(name)
        meta = json.loads((d / "package.json").read_text())
        text = next(((d / f).read_text() for f in LICENSE_FILES if (d / f).is_file()), None)
        if text is None and name.startswith("@tauri-apps/"):
            # Tauri's plugins ship only an SPDX tag; they're under Tauri's MIT license.
            text = tauri_mit
        if text is None:
            sys.exit(f"no license text found for {name}")
        parts.append(f"{RULE}\n{name} {meta['version']} ({meta.get('license', '')})\n\n{text.strip()}\n")
    return "\n".join(parts)


def onnxruntime_section() -> str:
    text = (ROOT / "licenses/onnxruntime-LICENSE").read_text().strip()
    return f"\nLinked into the transcription worker\n\n{RULE}\nONNX Runtime (MIT), in sherpa-onnx's libraries\n\n{text}\n"


def main() -> None:
    cargo_about = os.environ.get("CARGO_ABOUT", "cargo-about")
    rust = subprocess.run(
        [cargo_about, "generate", "--config", "licenses/about.toml", "licenses/about.hbs"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    notices = rust.rstrip() + "\n" + js_section() + onnxruntime_section()
    if "--check" in sys.argv:
        if not OUT.is_file() or OUT.read_text() != notices:
            sys.exit(f"{OUT.relative_to(ROOT)} is out of date: run python3 scripts/third-party-notices.py")
        return
    OUT.write_text(notices)
    print(f"wrote {OUT.relative_to(ROOT)} ({len(notices):,} bytes)")


if __name__ == "__main__":
    main()
