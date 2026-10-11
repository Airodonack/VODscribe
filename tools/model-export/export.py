#!/usr/bin/env python3
"""Exports NVIDIA's Parakeet TDT 0.6B v3 to ONNX in sherpa-onnx's layout, in fp32.

Adapted from sherpa-onnx's scripts/nemo/parakeet-tdt-0.6b-v3/export_onnx.py
(Apache-2.0, Xiaomi Corp.). Unlike that script it stops at fp32; quantize.py
builds the smaller variants from this output.

usage: export.py OUT_DIR
"""

import shutil
import sys
from pathlib import Path

import nemo.collections.asr as nemo_asr
import onnx
import torch

MODEL = "nvidia/parakeet-tdt-0.6b-v3"


def set_metadata(path: Path, meta: dict, source: Path | None = None) -> None:
    """Writes `source` (default: `path`) to `path` with `meta` as its metadata,
    keeping the encoder's weights in one external file."""
    model = onnx.load(str(source or path))
    del model.metadata_props[:]
    for key, value in meta.items():
        prop = model.metadata_props.add()
        prop.key, prop.value = key, str(value)
    external = path.stem == "encoder"
    onnx.save(
        model,
        str(path),
        save_as_external_data=external,
        all_tensors_to_one_file=True,
        location="encoder.weights" if external else None,
    )


@torch.no_grad()
def main() -> None:
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)

    model = nemo_asr.models.ASRModel.from_pretrained(model_name=MODEL)
    model.eval()

    with open(out / "tokens.txt", "w", encoding="utf-8") as f:
        vocab = model.joint.vocabulary
        for i, token in enumerate(vocab):
            f.write(f"{token} {i}\n")
        f.write(f"<blk> {len(vocab)}\n")

    # The encoder is over protobuf's 2 GB limit, so the exporter scatters its
    # weights over many files; export to a scratch dir and repack below.
    scratch = out / "encoder-export"
    scratch.mkdir(exist_ok=True)
    model.encoder.export(str(scratch / "encoder.onnx"))
    model.decoder.export(str(out / "decoder.onnx"))
    model.joint.export(str(out / "joiner.onnx"))

    normalize = model.cfg.preprocessor.normalize
    meta = {
        "vocab_size": model.decoder.vocab_size,
        "normalize_type": "" if normalize == "NA" else normalize,
        "pred_rnn_layers": model.decoder.pred_rnn_layers,
        "pred_hidden": model.decoder.pred_hidden,
        "subsampling_factor": 8,
        "model_type": "EncDecRNNTBPEModel",
        "version": "2",
        "model_author": "NeMo",
        "url": f"https://huggingface.co/{MODEL}",
        "comment": "Only the transducer branch is exported",
        "feat_dim": 128,
    }
    # sherpa-onnx reads the metadata from the encoder.
    set_metadata(out / "encoder.onnx", meta, source=scratch / "encoder.onnx")
    shutil.rmtree(scratch)
    print("exported to", out, "with", meta)


if __name__ == "__main__":
    main()
