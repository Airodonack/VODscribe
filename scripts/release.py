"""Prepares a release for dl.vodscribe.com from the build workflow's artifacts.

    python3 scripts/release.py <artifacts dir> <version> <out dir>

<artifacts dir> holds one vodscribe-<target> directory per build (as
actions/download-artifact writes them). Writes into <out dir>:

  files/       each installer and update bundle under its versioned name
               (VODscribe-<version>-windows-x64-setup.exe, ...), for releases/<version>/,
               where the updater and winget fetch them
  latest/      the installers under their stable names (VODscribe-windows-x64-setup.exe),
               for releases/latest/, which the website links to; not the macOS
               update bundles, which only the updater fetches
  latest.txt   one line per file in latest/: its stable name and versioned name.
               The publish job uploads each with a Content-Disposition naming the
               versioned file, so what lands in a Downloads folder says which version it is
  latest.json  the updater's manifest (plugins.updater in tauri.conf.json):
               every platform's versioned URL and signature, and the release
               notes from CHANGELOG.md's entry for the version
  winget/      the winget-pkgs manifests for this version

Fails if an expected file is missing, so a release never goes out half-built,
or if CHANGELOG.md has no entry for the version, or one still marked unreleased.

    python3 scripts/release.py --check        the Cargo.toml version has an entry (CI)
    python3 scripts/release.py --notes <ver>  print that version's notes
"""

import datetime
import hashlib
import json
import os
import re
import sys
from pathlib import Path

BASE = "https://dl.vodscribe.com/releases"
WINGET_ID = "Airodonack.VODscribe"

# (artifact, file name suffix, stable name, updater platforms). The versioned
# name puts the version after "VODscribe-" (`versioned`). Windows and macOS
# also get a plain platform the updater falls back to. Linux doesn't: each package
# finds its own format, and anything else (such as an AppImage from 0.1.0) gets no
# update rather than a package it can't install.
WINDOWS_SETUP = "VODscribe-windows-x64-setup.exe"
FILES = [
    ("x86_64-pc-windows-msvc", "_x64-setup.exe", WINDOWS_SETUP, ["windows-x86_64-nsis", "windows-x86_64"]),
    ("x86_64-pc-windows-msvc", ".msi", "VODscribe-windows-x64.msi", ["windows-x86_64-msi"]),
    ("aarch64-apple-darwin", ".dmg", "VODscribe-macos-arm64.dmg", []),
    ("aarch64-apple-darwin", ".app.tar.gz", "VODscribe-macos-arm64.app.tar.gz", ["darwin-aarch64-app", "darwin-aarch64"]),
    ("x86_64-apple-darwin", ".dmg", "VODscribe-macos-x64.dmg", []),
    ("x86_64-apple-darwin", ".app.tar.gz", "VODscribe-macos-x64.app.tar.gz", ["darwin-x86_64-app", "darwin-x86_64"]),
    ("x86_64-unknown-linux-gnu", ".deb", "VODscribe-linux-x64.deb", ["linux-x86_64-deb"]),
    ("x86_64-unknown-linux-gnu", ".rpm", "VODscribe-linux-x64.rpm", ["linux-x86_64-rpm"]),
]


def versioned(stable: str, version: str) -> str:
    """VODscribe-windows-x64-setup.exe -> VODscribe-<version>-windows-x64-setup.exe."""
    return stable.replace("VODscribe-", f"VODscribe-{version}-", 1)


def find(artifacts: Path, target: str, suffix: str) -> Path:
    matches = [p for p in (artifacts / f"vodscribe-{target}").rglob(f"*{suffix}") if p.is_file()]
    if len(matches) != 1:
        sys.exit(f"expected one *{suffix} for {target}, found {[str(m) for m in matches]}")
    return matches[0]


ROOT = Path(__file__).resolve().parent.parent
CHANGELOG = ROOT / "CHANGELOG.md"


def changelog_entry(changelog: str, version: str) -> tuple[str, bool]:
    """The changelog's entry for `version`: its text and whether it's marked unreleased.

    Entries start `## <version>`; the text runs to the next `## `. Exits if there's none.
    """
    for section in ("\n" + changelog).split("\n## ")[1:]:
        heading, _, body = section.partition("\n")
        if heading.split()[0] == version:
            return body.strip(), "(unreleased)" in heading
    sys.exit(f"CHANGELOG.md has no entry for {version}: add a '## {version}' section with what changed")


def notes(changelog: str, version: str) -> str:
    """The changelog entry, which becomes the release notes in latest.json."""
    body, unreleased = changelog_entry(changelog, version)
    if unreleased:
        sys.exit(f"CHANGELOG.md still marks {version} as unreleased: date the heading before tagging")
    if not body:
        sys.exit(f"CHANGELOG.md's entry for {version} is empty")
    return body


def cargo_version(cargo_toml: str) -> str:
    """The workspace version from the root Cargo.toml."""
    table = cargo_toml.split("[workspace.package]")[1]
    return re.search(r'^version = "([^"]+)"', table, re.M).group(1)


def main(artifacts: Path, version: str, out: Path) -> None:
    # Before any file is written, so a missing entry fails at once.
    release_notes = notes(CHANGELOG.read_text(), version)
    files, latest = out / "files", out / "latest"
    files.mkdir(parents=True)
    latest.mkdir()
    platforms: dict[str, dict[str, str]] = {}
    sha256: dict[str, str] = {}
    served: list[str] = []
    for target, suffix, name, keys in FILES:
        src = find(artifacts, target, suffix)
        data = src.read_bytes()
        named = versioned(name, version)
        (files / named).write_bytes(data)
        if not name.endswith(".app.tar.gz"):
            # The same bytes under the stable name, without a second copy on disk.
            os.link(files / named, latest / name)
            served.append(f"{name} {named}\n")
        sha256[name] = hashlib.sha256(data).hexdigest()
        if keys:
            sig = src.with_name(src.name + ".sig")
            if not sig.is_file():
                sys.exit(f"{src} has no .sig: was the build run without TAURI_SIGNING_PRIVATE_KEY?")
            for key in keys:
                platforms[key] = {"url": f"{BASE}/{version}/{named}", "signature": sig.read_text().strip()}
        print(f"{named:<40} {len(data):>12,}  {sha256[name]}")

    manifest = {
        "version": version,
        "notes": release_notes,
        "pub_date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platforms": platforms,
    }
    (out / "latest.txt").write_text("".join(served))
    (out / "latest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    write_winget(out / "winget", version, sha256[WINDOWS_SETUP])


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
        + f"  InstallerUrl: {BASE}/{version}/{versioned(WINDOWS_SETUP, version)}\n"
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
        + "PublisherSupportUrl: https://github.com/Airodonack/VODscribe/issues\n"
        + "PrivacyUrl: https://vodscribe.com/privacy\n"
        + "PackageName: VODscribe\n"
        + "PackageUrl: https://vodscribe.com\n"
        + "License: FSL-1.1-ALv2\n"
        + "LicenseUrl: https://github.com/Airodonack/VODscribe/blob/main/LICENSE.md\n"
        + "Copyright: Copyright 2026 Airodonack\n"
        + "ShortDescription: Searchable transcripts for Kick, Twitch, YouTube and other VODs\n"
        + "Description: VODscribe transcribes Kick, Twitch and YouTube VODs, videos from other sites, and video files,"
        + " on your computer, so you can find any moment by what was said and jump straight to it.\n"
        + "Moniker: vodscribe\n"
        + "Tags:\n- transcription\n- speech-to-text\n- twitch\n- youtube\n- kick\n- vod\n"
        + "ManifestType: defaultLocale\n"
        + tail
    )


if __name__ == "__main__":
    if sys.argv[1:] == ["--check"]:
        # For CI: the version being worked on has a changelog entry (unreleased or dated).
        changelog_entry(CHANGELOG.read_text(), cargo_version((ROOT / "Cargo.toml").read_text()))
    elif len(sys.argv) == 3 and sys.argv[1] == "--notes":
        print(notes(CHANGELOG.read_text(), sys.argv[2]))
    elif len(sys.argv) == 4:
        main(Path(sys.argv[1]), sys.argv[2], Path(sys.argv[3]))
    else:
        sys.exit(__doc__)
