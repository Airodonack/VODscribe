#!/usr/bin/env python3
"""Builds smaller variants of the fp32 export, each in its own model dir.

usage: quantize.py FP32_DIR OUT_ROOT VARIANT...

Variants:
  int8-pc       int8 weights with a scale per output channel (sherpa's release
                uses one scale per tensor, which drops whole stretches of speech)
  int8-pc-safe  int8-pc, but the convolutional front end and the joiner stay
                fp32, as those are the layers most sensitive to quantization
  int8-wo       weight-only int8: MatMul weights stored in 8 bits, math in fp32
  int8-wo-fast  int8-wo, but with int8 math on activations quantized per block
                (ONNX Runtime's accuracy level 4), its fast CPU path
  int4-wo       weight-only int4, likewise
  fp16          fp16 for the GPU, with overflow-prone ops kept in fp32
"""

import shutil
import sys
from pathlib import Path

import onnx
from onnxruntime.quantization import QuantType, quantize_dynamic

PARTS = ("encoder", "decoder", "joiner")


def copy_metadata(src: onnx.ModelProto, dst_path: Path, external: bool) -> None:
    """Quantizing drops metadata; sherpa-onnx needs it on the encoder."""
    model = onnx.load(str(dst_path))
    del model.metadata_props[:]
    model.metadata_props.extend(src.metadata_props)
    save(model, dst_path, external)


def save(model: onnx.ModelProto, path: Path, external: bool) -> None:
    onnx.save(
        model,
        str(path),
        save_as_external_data=external,
        all_tensors_to_one_file=True,
        location=f"{path.stem}.weights" if external else None,
    )


def int8(fp32: Path, out: Path, safe: bool) -> None:
    for part in PARTS:
        src = fp32 / f"{part}.onnx"
        dst = out / f"{part}.int8.onnx"
        if part == "joiner" and safe:
            # Stays fp32; the name is what sherpa-onnx's loader looks for.
            shutil.copy(src, dst)
            continue
        model = onnx.load(str(src), load_external_data=False)
        exclude = []
        if safe and part == "encoder":
            # The convolutional subsampling at the input sees raw features with
            # the widest dynamic range.
            exclude = [n.name for n in model.graph.node if n.name.startswith("/pre_encode/")]
        quantize_dynamic(
            model_input=str(src),
            model_output=str(dst),
            per_channel=True,
            weight_type=QuantType.QInt8,
            nodes_to_exclude=exclude,
            # Needed for models over 2 GB.
            use_external_data_format=part == "encoder",
        )
        if part == "encoder":
            # int8 brings the encoder under 2 GB: store it as a single file.
            copy_metadata(model, dst, external=False)
            for leftover in out.glob(f"{dst.name}.data"):
                leftover.unlink()
    print("built", out)


def weight_only(fp32: Path, out: Path, bits: int, accuracy_level: int | None = None) -> None:
    from onnxruntime.quantization.matmul_nbits_quantizer import (
        DefaultWeightOnlyQuantConfig,
        MatMulNBitsQuantizer,
    )

    for part in PARTS:
        model = onnx.load(str(fp32 / f"{part}.onnx"))
        # The quantizer edits the model in place, so keep the metadata first.
        metadata = [(p.key, p.value) for p in model.metadata_props]
        config = DefaultWeightOnlyQuantConfig(
            block_size=128, is_symmetric=True, bits=bits, accuracy_level=accuracy_level
        )
        quantizer = MatMulNBitsQuantizer(
            model, bits=bits, block_size=128, is_symmetric=True, accuracy_level=accuracy_level, algo_config=config
        )
        quantizer.process()
        quantized = quantizer.model.model
        del quantized.metadata_props[:]
        for key, value in metadata:
            quantized.metadata_props.add(key=key, value=value)
        save(quantized, out / f"{part}.int8.onnx", external=part == "encoder")
    print("built", out)


def fp16(fp32: Path, out: Path) -> None:
    # ONNX Runtime's converter inserts the casts needed around the ops kept in
    # fp32; onnxconverter-common's left mixed fp16/fp32 inputs on some nodes.
    from onnxruntime.transformers.float16 import convert_float_to_float16

    # Ops whose fp16 results overflow or lose too much precision.
    keep_fp32 = [
        "LayerNormalization", "ReduceMean", "Pow", "Sqrt", "Div", "Softmax",
        "Exp", "Log", "Erf", "Sigmoid", "Tanh",
    ]
    for part in PARTS:
        # The converter needs inferred shapes, and in-memory inference can't
        # handle the 2.4 GB encoder; infer from the file instead.
        # Written next to the source, where its external weights file lives.
        inferred = fp32 / f"{part}.inferred.onnx"
        onnx.shape_inference.infer_shapes_path(str(fp32 / f"{part}.onnx"), str(inferred))
        model = onnx.load(str(inferred))
        inferred.unlink()
        # The converter edits the model in place, so keep the metadata first.
        metadata = [(p.key, p.value) for p in model.metadata_props]
        converted = convert_float_to_float16(
            model, keep_io_types=True, op_block_list=keep_fp32, disable_shape_infer=True
        )
        del converted.metadata_props[:]
        for key, value in metadata:
            converted.metadata_props.add(key=key, value=value)
        # fp16 halves the encoder to ~1.2 GB, under protobuf's limit, but keep
        # weights external so all variants share the same layout.
        save(converted, out / f"{part}.fp16.onnx", external=part == "encoder")
    print("built", out)


def main() -> None:
    fp32, root, variants = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3:]
    for variant in variants:
        out = root / variant
        out.mkdir(parents=True, exist_ok=True)
        shutil.copy(fp32 / "tokens.txt", out / "tokens.txt")
        if variant == "int8-pc":
            int8(fp32, out, safe=False)
        elif variant == "int8-pc-safe":
            int8(fp32, out, safe=True)
        elif variant == "int8-wo":
            weight_only(fp32, out, bits=8)
        elif variant == "int8-wo-fast":
            weight_only(fp32, out, bits=8, accuracy_level=4)
        elif variant == "int4-wo":
            weight_only(fp32, out, bits=4)
        elif variant == "fp16":
            fp16(fp32, out)
        else:
            sys.exit(f"unknown variant {variant}")


if __name__ == "__main__":
    main()
