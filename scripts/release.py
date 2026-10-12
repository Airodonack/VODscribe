"""Prepares a release for dl.vodscribe.com from the build workflow's artifacts.

    python3 scripts/release.py <artifacts dir> <version> <out dir>

<artifacts dir> holds one vodscribe-<target> directory per build (as
actions/download-artifact writes them). Writes into <out dir>:

  files/       each installer and update bundle under its stable name
               (VODscribe-windows-x64-setup.exe, ...), which the website links
               to under releases/latest/ and the updater under releases/<version>/
  latest.json  the updater's manifest (plugins.updater in tauri.conf.json):
               every platform's versioned URL and signature
  winget/      the winget-pkgs manifests for this version

Fails if an expected file is missing, so a release never goes out half-built.
"""

import datetime
import hashlib
import json
import subprocess
import sys
from pathlib import Path

BASE = "https://dl.vodscribe.com/releases"
WINGET_ID = "Airodonack.VODscribe"

# (artifact, file name suffix, stable name, updater platforms). The first platform
# for each OS without an installer suffix is the default the updater falls back to.
FILES = [
    ("x86_64-pc-windows-msvc", "_x64-setup.exe", "VODscribe-windows-x64-setup.exe", ["windows-x86_64-nsis", "windows-x86_64"]),
    ("x86_64-pc-windows-msvc", ".msi", "VODscribe-windows-x64.msi", ["windows-x86_64-msi"]),
    ("aarch64-apple-darwin", ".dmg", "VODscribe-macos-arm64.dmg", []),
    ("aarch64-apple-darwin", ".app.tar.gz", "VODscribe-macos-arm64.app.tar.gz", ["darwin-aarch64-app", "darwin-aarch64"]),
    ("x86_64-apple-darwin", ".dmg", "VODscribe-macos-x64.dmg", []),
    ("x86_64-apple-darwin", ".app.tar.gz", "VODscribe-macos-x64.app.tar.gz", ["darwin-x86_64-app", "darwin-x86_64"]),
    ("x86_64-unknown-linux-gnu", ".AppImage", "VODscribe-linux-x64.AppImage", ["linux-x86_64-appimage", "linux-x86_64"]),
    ("x86_64-unknown-linux-gnu", ".deb", "VODscribe-linux-x64.deb", ["linux-x86_64-deb"]),
    ("x86_64-unknown-linux-gnu", ".rpm", "VODscribe-linux-x64.rpm", ["linux-x86_64-rpm"]),
]


def find(artifacts: Path, target: str, suffix: str) -> Path:
    matches = [p for p in (artifacts / f"vodscribe-{target}").rglob(f"*{suffix}") if p.is_file()]
    if len(matches) != 1:
        sys.exit(f"expected one *{suffix} for {target}, found {[str(m) for m in matches]}")
    return matches[0]


def notes(version: str) -> str:
    """The tag's message, which becomes the release notes the app can show."""
    out = subprocess.run(
        ["git", "tag", "-l", "--format=%(contents)", f"v{version}"], capture_output=True, text=True, check=False
    )
    return out.stdout.strip()


def main(artifacts: Path, version: str, out: Path) -> None:
    files = out / "files"
    files.mkdir(parents=True)
    platforms: dict[str, dict[str, str]] = {}
    sha256: dict[str, str] = {}
    for target, suffix, name, keys in FILES:
        src = find(artifacts, target, suffix)
        data = src.read_bytes()
        (files / name).write_bytes(data)
        sha256[name] = hashlib.sha256(data).hexdigest()
        if keys:
            sig = src.with_name(src.name + ".sig")
            if not sig.is_file():
                sys.exit(f"{src} has no .sig: was the build run without TAURI_SIGNING_PRIVATE_KEY?")
            for key in keys:
                platforms[key] = {"url": f"{BASE}/{version}/{name}", "signature": sig.read_text().strip()}
        print(f"{name:<34} {len(data):>12,}  {sha256[name]}")

    manifest = {
        "version": version,
        "notes": notes(version),
        "pub_date": datetime.datetime.now(datetime.UTC).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platforms": platforms,
    }
    (out / "latest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    write_winget(out / "winget", version, sha256["VODscribe-windows-x64-setup.exe"])


def write_winget(dir: Path, version: str, sha256: str) -> None:
    """The three winget-pkgs manifests, laid out as wingetcreate submit expects."""
    dir.mkdir()
    head = f"PackageIdentifier: {WINGET_ID}\nPackageVersion: {version}\n"
    tail = "ManifestVersion: 1.12.0\n"
    schema = "# yaml-language-server: $schema=https://aka.ms/winget-manifest.{}.1.12.0.schema.json\n"
    (dir / f"{WINGET_ID}.yaml").write_text(
        schema.format("version") + head + "DefaultLocale: en-US\nManifestType: version\n" + tail
    )
    (dir / f"{WINGET_ID}.installer.yaml").write_text(
        schema.format("installer")
        + head
        # Tauri's NSIS installer installs for the current user and takes /S for silent installs.
        + "InstallerType: nullsoft\nScope: user\nUpgradeBehavior: install\n"
        + f"ReleaseDate: {datetime.date.today().isoformat()}\n"
        + "Installers:\n- Architecture: x64\n"
        + f"  InstallerUrl: {BASE}/{version}/VODscribe-windows-x64-setup.exe\n"
        + f"  InstallerSha256: {sha256.upper()}\n"
        + "ManifestType: installer\n"
        + tail
    )
    (dir / f"{WINGET_ID}.locale.en-US.yaml").write_text(
        schema.format("defaultLocale")
        + head
        + "PackageLocale: en-US\n"
        + "Publisher: Airodonack\n"
        + "PublisherUrl: https://vodscribe.com\n"
        + "PublisherSupportUrl: https://ko-fi.com/airodonack\n"
        + "PrivacyUrl: https://vodscribe.com/privacy\n"
        + "PackageName: VODscribe\n"
        + "PackageUrl: https://vodscribe.com\n"
        + "License: Freeware\n"
        + "ShortDescription: Searchable transcripts for Kick, Twitch and YouTube VODs\n"
        + "Description: VODscribe transcribes Kick, Twitch and YouTube VODs, and video files, on your computer,"
        + " so you can find any moment by what was said and jump straight to it.\n"
        + "Moniker: vodscribe\n"
        + "Tags:\n- transcription\n- speech-to-text\n- twitch\n- youtube\n- kick\n- vod\n"
        + "ManifestType: defaultLocale\n"
        + tail
    )


if __name__ == "__main__":
    main(Path(sys.argv[1]), sys.argv[2], Path(sys.argv[3]))
