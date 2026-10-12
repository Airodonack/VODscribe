#!/usr/bin/env python3
"""Scores model variants by running VODscribe's own worker over a set of clips.

usage: evaluate.py AUDIO_DIR REFERENCE_DIR CANDIDATE...

A candidate is NAME=MODEL_DIR[:cpu|:cuda[:BATCH]]. The reference model runs on
the CPU; every candidate's transcript is compared with it. Reports word error
rate against the reference (lower is better), words produced, and speed.

Set WORKER / CUDA_WORKER to the worker binaries and GPU_LIBS to the GPU pack's
library dir. Uses only the standard library.
"""

import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
# Where scripts/prepare-sidecars.sh puts the workers (Linux x86_64 names).
BINARIES = REPO / "apps/desktop/src-tauri/binaries"
WORKER = os.environ.get("WORKER", str(BINARIES / "vodscribe-worker-x86_64-unknown-linux-gnu"))
CUDA_WORKER = os.environ.get("CUDA_WORKER", str(BINARIES / "vodscribe-worker-cuda-x86_64-unknown-linux-gnu"))
# The app's GPU pack, on Linux.
GPU_LIBS = os.environ.get("GPU_LIBS", str(Path.home() / ".local/share/app.vodscribe/gpu/lib"))
FFMPEG = os.environ.get("FFMPEG", "ffmpeg")


def transcribe(model: str, provider: str, batch: int, wav: Path) -> tuple[list[str], float]:
    cuda = provider == "cuda"
    env = dict(os.environ)
    if cuda:
        env["LD_LIBRARY_PATH"] = GPU_LIBS
    cmd = [
        CUDA_WORKER if cuda else WORKER, "--model", model, "--provider", provider,
        "--batch", str(batch if cuda else 1), "--ffmpeg", FFMPEG, str(wav),
    ]
    start = time.monotonic()
    out = subprocess.run(cmd, env=env, capture_output=True, text=True)
    elapsed = time.monotonic() - start
    if out.returncode != 0:
        raise RuntimeError(f"{' '.join(cmd)} failed: {out.stderr.strip()[-300:]}")
    words = [w["text"] for line in out.stdout.splitlines() for w in json.loads(line).get("words", [])]
    return words, elapsed


def normalize(words: list[str]) -> list[str]:
    """Lowercase and strip punctuation, so only word choice counts."""
    out = []
    for w in words:
        out.extend(t for t in re.split(r"[^\w']+", w.lower()) if t)
    return out


def errors(ref: list[str], hyp: list[str]) -> int:
    """Word-level edit distance."""
    prev = list(range(len(hyp) + 1))
    for i, r in enumerate(ref, 1):
        cur = [i] + [0] * len(hyp)
        for j, h in enumerate(hyp, 1):
            cur[j] = min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (r != h))
        prev = cur
    return prev[-1]


def parse(spec: str) -> tuple[str, str, str, int]:
    name, rest = spec.split("=", 1)
    parts = rest.split(":")
    model = parts[0]
    provider = parts[1] if len(parts) > 1 else "cpu"
    batch = int(parts[2]) if len(parts) > 2 else 4
    return name, model, provider, batch


def duration(wav: Path) -> float:
    out = subprocess.run(
        ["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", str(wav)],
        capture_output=True, text=True, check=True,
    )
    return float(out.stdout)


def main() -> None:
    audio, reference, candidates = Path(sys.argv[1]), sys.argv[2], [parse(c) for c in sys.argv[3:]]
    clips = sorted(audio.glob("*.wav"))
    total = sum(duration(c) for c in clips)
    refs = {clip: normalize(transcribe(reference, "cpu", 1, clip)[0]) for clip in clips}
    ref_words = sum(len(r) for r in refs.values())
    print(f"{len(clips)} clips, {total / 60:.1f} min, reference has {ref_words} words\n")
    print(f"{'variant':<22} {'WER':>6} {'words':>6} {'speed':>8}   per clip WER")
    for name, model, provider, batch in candidates:
        errs, words, elapsed, per_clip = 0, 0, 0.0, []
        try:
            results = [(clip, *transcribe(model, provider, batch, clip)) for clip in clips]
        except RuntimeError as e:
            print(f"{name:<22} failed: {str(e).splitlines()[-1][-160:]}")
            continue
        for clip, hyp, secs in results:
            hyp = normalize(hyp)
            e = errors(refs[clip], hyp)
            errs, words, elapsed = errs + e, words + len(hyp), elapsed + secs
            per_clip.append(f"{clip.stem} {100 * e / max(1, len(refs[clip])):.1f}")
        print(f"{name:<22} {100 * errs / ref_words:5.1f}% {words:>6} {total / elapsed:7.1f}x   {', '.join(per_clip)}")


if __name__ == "__main__":
    main()
