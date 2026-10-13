"""Checks that every download VODscribe pins is still where it's pinned.

    python3 -I scripts/check-pinned-downloads.py

Setup and the GPU add-on download fixed URLs (ffmpeg and Deno builds, NVIDIA's
wheels, sherpa-onnx's CUDA release, the speech models on dl.vodscribe.com), and
the sidecar build downloads sherpa-onnx's prebuilt libraries. Each is pinned by
SHA-256, so a moved or replaced file is refused rather than used, but a vanished
one breaks setup for every new user, silently, until someone complains. This
sends a HEAD request for each (nothing is downloaded) and fails if any is
missing or, where its size is pinned, a different size. The Pinned downloads
workflow runs it weekly.
"""

import re
import sys
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CORE = ROOT / "crates" / "core" / "src"
SHERPA_RELEASES = "https://github.com/k2-fsa/sherpa-onnx/releases/download"
YT_DLP_LATEST = "https://github.com/yt-dlp/yt-dlp/releases/latest/download"
# A pinned SHA-256 as the sources write it; one per download.
HASH = re.compile(r'"[0-9a-f]{64}"')


def pinned() -> list[tuple[str, int | None]]:
    """Every pinned URL and its size, if one is pinned next to it.

    Each source's count is checked against its number of pinned SHA-256s, so a
    change to how the pins are written fails here rather than quietly shrinking
    the list.
    """
    pins: list[tuple[str, int | None]] = []
    # deps.rs and gpu.rs: `url: "...", size: N` or, in a tuple, `"...", N`.
    for file in ("deps.rs", "gpu.rs"):
        text = (CORE / file).read_text()
        found = re.findall(r'"(https://[^"]+)",\s*(?:size:\s*)?(\d+)', text)
        expect(file, len(found), len(HASH.findall(text)))
        pins += [(url, int(size)) for url, size in found]
    # model.rs: files under each model id, served from MODELS_BASE_URL.
    text = (CORE / "model.rs").read_text()
    base = re.findall(r'MODELS_BASE_URL: &str = "([^"]+)"', text)[0]
    count = 0
    # Each `id: "<model>"` starts a model; its files follow until the next.
    for block in re.split(r'\bid: (?=")', text)[1:]:
        model = block.split('"')[1]
        for name, size in re.findall(r'name: "([^"]+)",\s*size: (\d+)', block):
            pins.append((f"{base}/{model}/{name}", int(size)))
            count += 1
    expect("model.rs", count, len(HASH.findall(text)))
    # The sidecar build's sherpa-onnx prebuilts, from the pinned list.
    lines = (ROOT / "scripts" / "sherpa-onnx-prebuilt.sha256").read_text().splitlines()
    names = [line.split()[1] for line in lines if line.strip() and not line.startswith("#")]
    expect("sherpa-onnx-prebuilt.sha256", len(names), 6)
    for name in names:
        version = name.split("-v")[1].split("-")[0]
        pins.append((f"{SHERPA_RELEASES}/v{version}/{name}", None))
    # yt-dlp is installed from its latest release, so only its checksum list is fixed.
    pins.append((f"{YT_DLP_LATEST}/SHA2-256SUMS", None))
    return pins


def expect(source: str, found: int, pinned_hashes: int) -> None:
    if found != pinned_hashes or found == 0:
        sys.exit(f"{source}: found {found} download URLs but {pinned_hashes} pinned hashes; did its layout change?")


class HeadAcrossRedirects(urllib.request.HTTPRedirectHandler):
    """Keeps HEAD through redirects; urllib's default would follow them with GET,
    and GitHub and PyPI redirect every download to a storage host."""

    def redirect_request(self, req, fp, code, msg, headers, newurl):
        new = super().redirect_request(req, fp, code, msg, headers, newurl)
        if new is not None:
            new.method = "HEAD"
        return new


OPENER = urllib.request.build_opener(HeadAcrossRedirects)


def head(url: str) -> tuple[int, int | None]:
    """The final status after redirects and the Content-Length, if given and sane."""
    req = urllib.request.Request(url, method="HEAD", headers={"User-Agent": "vodscribe-pin-check"})
    try:
        with OPENER.open(req, timeout=30) as res:
            length = res.headers.get("Content-Length", "")
            return res.status, int(length) if length.isdigit() else None
    except urllib.error.HTTPError as e:
        return e.code, None


def main() -> None:
    urls = pinned()
    failures = []
    for url, size in urls:
        try:
            status, length = head(url)
        except (urllib.error.URLError, TimeoutError, OSError) as e:
            failures.append(f"{url}: {e}")
            print(f"ERR  {url}: {e}")
            continue
        problem = None
        if status != 200:
            problem = f"HTTP {status}"
        elif size is not None and length is not None and length != size:
            problem = f"{length} bytes, pinned {size}"
        if problem:
            failures.append(f"{url}: {problem}")
        print(f"{'FAIL' if problem else 'ok  '} {url}{f': {problem}' if problem else ''}")
    print(f"\n{len(urls) - len(failures)} of {len(urls)} pinned downloads are in place")
    if failures:
        sys.exit("missing or changed:\n  " + "\n  ".join(failures))


if __name__ == "__main__":
    main()
