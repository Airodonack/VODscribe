"""Tests for release.py's changelog and version parsing, verify-release.py's
signature checks, and ready-to-release.py's next development version:
python3 -I -m unittest scripts/test_release.py"""

import base64
import hashlib
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("release", Path(__file__).with_name("release.py"))
assert spec is not None and spec.loader is not None
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)

spec = importlib.util.spec_from_file_location("verify", Path(__file__).with_name("verify-release.py"))
assert spec is not None and spec.loader is not None
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

spec = importlib.util.spec_from_file_location("ready", Path(__file__).with_name("ready-to-release.py"))
assert spec is not None and spec.loader is not None
ready = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ready)

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
        # In development (0.4.1-dev), the entry is the release's (0.4.1).
        release.changelog_entry((root / "CHANGELOG.md").read_text(), version.split("-")[0])


def sign(secret: bytes, message: bytes) -> tuple[bytes, bytes]:
    """Ed25519 (RFC 8032): the public key for `secret`, and its signature of `message`."""
    h = hashlib.sha512(secret).digest()
    a = int.from_bytes(h[:32], "little") & ((1 << 254) - 8) | (1 << 254)
    encode = lambda p: (  # noqa: E731
        (p[1] * pow(p[2], verify.P - 2, verify.P) % verify.P)
        | ((p[0] * pow(p[2], verify.P - 2, verify.P) % verify.P) & 1) << 255
    ).to_bytes(32, "little")
    public = encode(verify._mul(a, verify.G))
    r = int.from_bytes(hashlib.sha512(h[32:] + message).digest(), "little") % verify.L
    big_r = encode(verify._mul(r, verify.G))
    k = int.from_bytes(hashlib.sha512(big_r + public + message).digest(), "little") % verify.L
    return public, big_r + ((r + k * a) % verify.L).to_bytes(32, "little")


class SignatureTest(unittest.TestCase):
    def test_rfc8032_vector(self):
        public = bytes.fromhex("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a")
        sig = bytes.fromhex(
            "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
        )
        self.assertTrue(verify.ed25519_ok(public, b"", sig))
        self.assertFalse(verify.ed25519_ok(public, b"x", sig))
        self.assertEqual(sign(bytes.fromhex("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60"), b"")[1], sig)

    def test_minisign(self):
        key_id = b"\x01" * 8
        public, _ = sign(b"\x07" * 32, b"")
        file_hash = hashlib.blake2b(b"installer", digest_size=64).digest()
        sig = sign(b"\x07" * 32, file_hash)[1]
        trusted = b"timestamp:1\tfile:installer"
        global_sig = sign(b"\x07" * 32, sig + trusted)[1]
        b64 = lambda b: base64.b64encode(b).decode()  # noqa: E731
        public_file = f"untrusted comment: key\n{b64(b'Ed' + key_id + public)}\n"
        sig_file = f"untrusted comment: sig\n{b64(b'ED' + key_id + sig)}\ntrusted comment: {trusted.decode()}\n{b64(global_sig)}\n"
        self.assertIsNone(verify.minisign_ok(public_file, sig_file, file_hash))
        other = hashlib.blake2b(b"installer, changed", digest_size=64).digest()
        self.assertEqual(verify.minisign_ok(public_file, sig_file, other), "signature doesn't match the file")
        wrong_key = public_file.replace(b64(b"Ed" + key_id + public), b64(b"Ed" + b"\x02" * 8 + public))
        self.assertEqual(verify.minisign_ok(wrong_key, sig_file, file_hash), "signed with a different key")
        forged = sig_file.replace("file:installer", "file:other")
        self.assertIn("trusted comment", verify.minisign_ok(public_file, forged, file_hash) or "")



class NextDevelopmentVersion(unittest.TestCase):
    def test_is_the_next_patch_in_development(self):
        self.assertEqual(ready.next_dev("0.4.0"), "0.4.1-dev")
        self.assertEqual(ready.next_dev("1.2.9"), "1.2.10-dev")

if __name__ == "__main__":
    unittest.main()
