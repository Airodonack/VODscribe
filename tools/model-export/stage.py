#!/usr/bin/env python3
"""Lays out built models for upload, as they will appear in the bucket:

    DIST/<model id>/<file>

and prints each file's size and SHA-256 for crates/core/src/model.rs, which
pins them. Upload DIST's contents to the root of the bucket the app's
MODELS_BASE_URL points at.

usage: stage.py MODELS_DIR DIST
"""

import hashlib
import shutil
import sys
from pathlib import Path

# Model id -> the quantize.py variant it ships.
MODELS = {
    "parakeet-tdt-0.6b-v3-int8wo": "int8-wo-fast",
    "parakeet-tdt-0.6b-v3-fp16": "fp16",
}


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        while chunk := f.read(1 << 20):
            h.update(chunk)
    return h.hexdigest()


def main() -> None:
    models, dist = Path(sys.argv[1]), Path(sys.argv[2])
    for model_id, variant in MODELS.items():
        src, dst = models / variant, dist / model_id
        dst.mkdir(parents=True, exist_ok=True)
        print(f"// {model_id} (from the {variant} variant)")
        for f in sorted(p for p in src.iterdir() if p.is_file()):
            shutil.copy2(f, dst / f.name)
            print(f'ModelFile {{ name: "{f.name}", size: {f.stat().st_size}, sha256: "{sha256(f)}" }},')
        print()


if __name__ == "__main__":
    main()
