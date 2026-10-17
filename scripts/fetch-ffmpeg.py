#!/usr/bin/env python3
"""Puts the ffmpeg the app ships for Linux x86_64 (the build pinned in
crates/core/src/deps.rs, checked by its SHA-256) at <dir>/ffmpeg. For CI's
player tests: Ubuntu 22.04's own ffmpeg (4.4) lacks options the player uses.

The archive is kept in <dir> (CI caches it) and checked against the pin at
every run, and the binary is taken from it each time: nothing that only
claims to be it, cached or not, is run.

    python3 -I scripts/fetch-ffmpeg.py <dir>
"""

import hashlib
import re
import sys
import tarfile
import tempfile
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DEPS = ROOT / "crates/core/src/deps.rs"


def pin() -> tuple[str, str]:
    """The Linux x86_64 ffmpeg's URL and SHA-256, from deps.rs."""
    text = DEPS.read_text()
    body = text[text.index("pub fn ffmpeg()") :]
    block = body[body.index('("linux", "x86_64")') :]
    block = block[: block.index("},")]
    url = re.search(r'url: "([^"]+)"', block)
    sha = re.search(r'sha256: "([0-9a-f]{64})"', block)
    if not url or not sha:
        sys.exit(f"{DEPS}: no Linux x86_64 ffmpeg URL and SHA-256 found")
    return url.group(1), sha.group(1)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as f:
        while chunk := f.read(1 << 20):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> None:
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    archive = out / "ffmpeg.tar.xz"
    target = out / "ffmpeg"
    url, sha = pin()
    if archive.is_file() and sha256(archive) == sha:
        print(f"{archive} is the pinned one")
    else:
        with tempfile.TemporaryDirectory(dir=out) as tmp:
            fetched = Path(tmp) / "ffmpeg.tar.xz"
            with (
                urllib.request.urlopen(url, timeout=120) as response,
                fetched.open("wb") as f,
            ):
                while chunk := response.read(1 << 20):
                    f.write(chunk)
            if (got := sha256(fetched)) != sha:
                sys.exit(f"{url}: SHA-256 {got}, not {sha}")
            fetched.replace(archive)
        print(f"{archive} from {url}")
    with tarfile.open(archive) as tar:
        member = next(m for m in tar.getmembers() if m.name.endswith("/bin/ffmpeg"))
        source = tar.extractfile(member)
        if source is None:
            sys.exit(f"{url}: bin/ffmpeg isn't a file")
        target.unlink(missing_ok=True)
        target.write_bytes(source.read())
    target.chmod(0o755)
    print(f"{target} from {archive.name}")


if __name__ == "__main__":
    main()
