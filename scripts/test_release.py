"""Tests for release.py's changelog and version parsing: python3 -I -m unittest scripts/test_release.py"""

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("release", Path(__file__).with_name("release.py"))
assert spec is not None and spec.loader is not None
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)

CHANGELOG = """# Changelog

Intro text with ## inside a sentence, which isn't a heading.

## 0.1.10 (unreleased)

- Ten.

## 0.1.1 — 2026-10-12

- One.
- Two.

## 0.1.0 — 2026-10-12

First release.
"""


class ChangelogEntry(unittest.TestCase):
    def test_matches_the_exact_version(self):
        self.assertEqual(release.changelog_entry(CHANGELOG, "0.1.1"), ("- One.\n- Two.", False))
        self.assertEqual(release.changelog_entry(CHANGELOG, "0.1.0"), ("First release.", False))

    def test_tells_unreleased(self):
        self.assertEqual(release.changelog_entry(CHANGELOG, "0.1.10"), ("- Ten.", True))

    def test_exits_without_an_entry(self):
        with self.assertRaises(SystemExit):
            release.changelog_entry(CHANGELOG, "0.1")
        with self.assertRaises(SystemExit):
            release.changelog_entry(CHANGELOG, "9.9.9")

    def test_works_without_a_title_line(self):
        self.assertEqual(release.changelog_entry("## 1.0.0 — 2026-01-01\n\nText\n", "1.0.0"), ("Text", False))


class Notes(unittest.TestCase):
    def test_gives_a_dated_entry(self):
        self.assertEqual(release.notes(CHANGELOG, "0.1.1"), "- One.\n- Two.")

    def test_refuses_unreleased_or_empty(self):
        with self.assertRaises(SystemExit):
            release.notes(CHANGELOG, "0.1.10")
        with self.assertRaises(SystemExit):
            release.notes("## 2.0.0 — 2026-01-01\n\n## 1.0.0 — 2025-01-01\n\nOld\n", "2.0.0")


class Versioned(unittest.TestCase):
    def test_puts_the_version_after_the_name(self):
        self.assertEqual(release.versioned("VODscribe-windows-x64-setup.exe", "9.9.9"), "VODscribe-9.9.9-windows-x64-setup.exe")
        self.assertEqual(release.versioned("VODscribe-macos-arm64.app.tar.gz", "1.0.0"), "VODscribe-1.0.0-macos-arm64.app.tar.gz")


class Main(unittest.TestCase):
    def test_lays_out_the_release(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            for target, suffix, name, keys in release.FILES:
                bundle = tmp / "artifacts" / f"vodscribe-{target}" / "bundle"
                bundle.mkdir(parents=True, exist_ok=True)
                (bundle / f"VODscribe_9.9.9{suffix}").write_text(name)
                if keys:
                    (bundle / f"VODscribe_9.9.9{suffix}.sig").write_text(f"sig of {name}\n")
            changelog = tmp / "CHANGELOG.md"
            changelog.write_text(CHANGELOG.replace("0.1.10 (unreleased)", "9.9.9 — 2026-01-01"))
            release.CHANGELOG, saved = changelog, release.CHANGELOG
            try:
                release.main(tmp / "artifacts", "9.9.9", tmp / "out")
            finally:
                release.CHANGELOG = saved
            out = tmp / "out"

            files = sorted(p.name for p in (out / "files").iterdir())
            self.assertEqual(files, sorted(release.versioned(name, "9.9.9") for _, _, name, _ in release.FILES))
            # latest/ has the installers, the same files as their versioned copies, and latest.txt names both.
            lines = (out / "latest.txt").read_text().splitlines()
            self.assertEqual(sorted(p.name for p in (out / "latest").iterdir()), sorted(l.split()[0] for l in lines))
            self.assertEqual(len(lines), 6)
            for line in lines:
                name, named = line.split()
                self.assertEqual(named, release.versioned(name, "9.9.9"))
                self.assertTrue((out / "latest" / name).samefile(out / "files" / named))

            manifest = json.loads((out / "latest.json").read_text())
            self.assertEqual(manifest["notes"], "- Ten.")
            self.assertEqual(
                manifest["platforms"]["windows-x86_64"],
                {"url": f"{release.BASE}/9.9.9/VODscribe-9.9.9-windows-x64-setup.exe", "signature": "sig of VODscribe-windows-x64-setup.exe"},
            )
            for entry in manifest["platforms"].values():
                self.assertIn(entry["url"].rsplit("/", 1)[1], files)
            installer = (out / "winget" / f"{release.WINGET_ID}.installer.yaml").read_text()
            self.assertIn(f"InstallerUrl: {release.BASE}/9.9.9/VODscribe-9.9.9-windows-x64-setup.exe\n", installer)


class CargoVersion(unittest.TestCase):
    def test_reads_the_workspace_version(self):
        toml = '[package]\nversion = "9.9.9"\n\n[workspace.package]\nrust-version = "1.99.0"\nversion = "0.1.3"\n'
        self.assertEqual(release.cargo_version(toml), "0.1.3")

    def test_the_real_files_agree(self):
        root = Path(__file__).resolve().parent.parent
        version = release.cargo_version((root / "Cargo.toml").read_text())
        release.changelog_entry((root / "CHANGELOG.md").read_text(), version)


if __name__ == "__main__":
    unittest.main()
