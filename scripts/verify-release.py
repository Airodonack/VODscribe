"""Checks a published release from the outside, as the app and the website see it.

    python3 scripts/verify-release.py <version>

Run by the publish job once the upload is done, and by hand any time. Fails
(listing every problem) unless:

  latest.json  names <version>, with CHANGELOG.md's notes for it and every
               platform of release.py's FILES, each at its versioned URL
  updates      every file the updater fetches downloads, and its signature in
               latest.json verifies against the updater key in tauri.conf.json
  website      every releases/latest/ installer the website links to is served,
               saved under its versioned name, and is the same size as that file

Downloads about 1 GB (each update file once). Requests say "vodscribe-release-check"
as their user agent, so they can be told apart in the site's analytics.
"""

import base64
import hashlib
import json
import re
import sys
import urllib.request
import uuid
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import release  # noqa: E402

CONF = release.ROOT / "apps/desktop/src-tauri/tauri.conf.json"
AGENT = {"User-Agent": "vodscribe-release-check"}

# Ed25519 signature checking (RFC 8032), small and slow, as minisign's isn't
# installed everywhere this runs and a release has only a few files to check.
P = 2**255 - 19
L = 2**252 + 27742317777372353535851937790883648493
D = -121665 * pow(121666, P - 2, P) % P
I = pow(2, (P - 1) // 4, P)


def _x(y: int, sign: int) -> int | None:
    xx = (y * y - 1) * pow(D * y * y + 1, P - 2, P) % P
    x = pow(xx, (P + 3) // 8, P)
    if (x * x - xx) % P:
        x = x * I % P
    if (x * x - xx) % P:
        return None
    return P - x if x & 1 != sign else x


def _point(b: bytes):
    y = int.from_bytes(b, "little")
    sign, y = y >> 255, y & ((1 << 255) - 1)
    x = None if y >= P else _x(y, sign)
    return None if x is None else (x, y, 1, x * y % P)


def _add(a, b):
    A = (a[1] - a[0]) * (b[1] - b[0]) % P
    B = (a[1] + a[0]) * (b[1] + b[0]) % P
    C = 2 * a[3] * b[3] * D % P
    Dd = 2 * a[2] * b[2] % P
    E, F, G, H = B - A, Dd - C, Dd + C, B + A
    return (E * F % P, G * H % P, F * G % P, E * H % P)


def _mul(s: int, p):
    q = (0, 1, 1, 0)
    while s:
        if s & 1:
            q = _add(q, p)
        p, s = _add(p, p), s >> 1
    return q


def _same(a, b) -> bool:
    return (a[0] * b[2] - b[0] * a[2]) % P == 0 and (a[1] * b[2] - b[1] * a[2]) % P == 0


_GY = 4 * pow(5, P - 2, P) % P
_GX = _x(_GY, 0)
assert _GX is not None
G = (_GX, _GY, 1, _GX * _GY % P)


def ed25519_ok(public: bytes, message: bytes, sig: bytes) -> bool:
    a, r = _point(public), _point(sig[:32])
    s = int.from_bytes(sig[32:], "little")
    if a is None or r is None or s >= L:
        return False
    h = int.from_bytes(hashlib.sha512(sig[:32] + public + message).digest(), "little") % L
    return _same(_mul(s, G), _add(r, _mul(h, a)))


def minisign_ok(public_key_file: str, sig_file: str, file_hash: bytes) -> str | None:
    """None if `sig_file` (a minisign signature, as tauri signer writes) signs the
    file whose BLAKE2b-512 is `file_hash`; otherwise what's wrong."""
    pk = base64.b64decode(public_key_file.strip().splitlines()[1])
    lines = sig_file.strip().splitlines()
    sig = base64.b64decode(lines[1])
    if sig[:2] != b"ED":
        return f"unexpected signature algorithm {sig[:2]!r}"
    if sig[2:10] != pk[2:10]:
        return "signed with a different key"
    if not ed25519_ok(pk[10:], file_hash, sig[10:]):
        return "signature doesn't match the file"
    trusted = lines[2].removeprefix("trusted comment: ").encode()
    if not ed25519_ok(pk[10:], sig[10:] + trusted, base64.b64decode(lines[3])):
        return "trusted comment's signature doesn't match"
    return None


def get(url: str, method: str = "GET"):
    return urllib.request.urlopen(urllib.request.Request(url, headers=AGENT, method=method), timeout=60)


def main(version: str) -> None:
    problems: list[str] = []
    # A query the edge cache hasn't seen, so this reads what was just uploaded.
    with get(f"{release.BASE}/latest.json?check={uuid.uuid4().hex}") as r:
        manifest = json.load(r)
    if manifest.get("version") != version:
        problems.append(f"latest.json names {manifest.get('version')}, not {version}")
    if manifest.get("notes") != release.notes(release.CHANGELOG.read_text(), version):
        problems.append("latest.json's notes aren't CHANGELOG.md's for this version")
    platforms = manifest.get("platforms", {})
    expected = {p: release.versioned(stable, version) for _, _, stable, ps in release.FILES for p in ps}
    if set(platforms) != set(expected):
        problems.append(f"latest.json's platforms are {sorted(platforms)}, expected {sorted(expected)}")

    public_key = base64.b64decode(json.loads(CONF.read_text())["plugins"]["updater"]["pubkey"]).decode()
    checked: dict[str, str | None] = {}
    for platform, name in sorted(expected.items()):
        entry = platforms.get(platform)
        if not entry:
            continue
        url = f"{release.BASE}/{version}/{name}"
        if entry.get("url") != url:
            problems.append(f"{platform}: url is {entry.get('url')}, expected {url}")
            continue
        if url not in checked:
            print(f"checking {name}", flush=True)
            h = hashlib.blake2b(digest_size=64)
            try:
                with get(url) as r:
                    while chunk := r.read(1 << 20):
                        h.update(chunk)
                checked[url] = minisign_ok(public_key, base64.b64decode(entry.get("signature", "")).decode(), h.digest())
            except Exception as e:  # noqa: BLE001 (reported, with the others)
                checked[url] = f"couldn't download: {e}"
        if checked[url]:
            problems.append(f"{platform} ({name}): {checked[url]}")

    for _, _, stable, _ in release.FILES:
        if stable.endswith(".app.tar.gz"):
            continue  # Update bundles aren't in latest/.
        named = release.versioned(stable, version)
        try:
            with get(f"{release.BASE}/latest/{stable}?check={uuid.uuid4().hex}", "HEAD") as r:
                disposition, size = r.headers.get("Content-Disposition", ""), r.headers.get("Content-Length")
            with get(f"{release.BASE}/{version}/{named}", "HEAD") as r:
                expected_size = r.headers.get("Content-Length")
        except Exception as e:  # noqa: BLE001
            problems.append(f"latest/{stable}: {e}")
            continue
        if f'filename="{named}"' not in disposition:
            problems.append(f"latest/{stable} saves as {disposition!r}, not {named}")
        if size != expected_size:
            problems.append(f"latest/{stable} is {size} bytes, {named} is {expected_size}")

    if problems:
        sys.exit("The release has problems:\n  " + "\n  ".join(problems))
    print(f"{version}: latest.json, {len(checked)} signed update files and the website's downloads all check out")


if __name__ == "__main__":
    if len(sys.argv) != 2 or not re.fullmatch(r"\d+\.\d+\.\d+", sys.argv[1]):
        sys.exit(__doc__)
    main(sys.argv[1])
