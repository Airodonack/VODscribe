# Model export

vodscribe uses NVIDIA's [Parakeet TDT 0.6B v3](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3), converted to ONNX in [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx)'s layout. We build the model files ourselves instead of using sherpa-onnx's published int8 release, because that release silently drops stretches of speech on stream audio.

## Steps

The scripts are a [uv](https://docs.astral.sh/uv/) project: `pyproject.toml` pins Python 3.12 and CPU-only PyTorch, and uv resolves NeMo, ONNX and ONNX Runtime together. NeMo's build dependencies need a few system packages, so the export runs in a throwaway Distrobox container to keep the host clean. Your home directory is shared with it, so uv and the repo are available inside.

```sh
distrobox create --name vodscribe-export --image docker.io/library/ubuntu:24.04
distrobox enter vodscribe-export -- sudo apt-get install -y build-essential ffmpeg libsndfile1

W=~/.cache/vodscribe-export
cd tools/model-export
run() { distrobox enter vodscribe-export -- bash -c "cd $PWD && uv run $*"; }

# 1. fp32 export, straight from NVIDIA's checkpoint.
run export.py $W/models/fp32

# 2. Smaller variants built from the fp32 export.
run quantize.py $W/models/fp32 $W/models int8-pc int8-pc-safe fp16

# 3. Score each variant against fp32 with vodscribe's own workers. Runs on the
#    host; build the workers first (scripts/prepare-sidecars.sh).
python3 evaluate.py $W/audio $W/models/fp32 \
  int8-pc=$W/models/int8-pc int8-pc-safe=$W/models/int8-pc-safe fp16-gpu=$W/models/fp16:cuda:4
```

`evaluate.py` reports the word error rate against fp32, the number of words produced, and the speed relative to realtime. Use a mix of clips that matches real use (stream audio from each platform, plus a few non-English samples). WER against fp32 measures how much a variant loses, not its absolute accuracy.

## Results

Accuracy was measured on 11 minutes of Kick, Twitch, YouTube, German, French and Spanish audio. Speed was measured on 34 minutes of the same audio, on a Ryzen with 32 threads and an RTX 3080 (10 GB). WER is measured against fp32 on the CPU.

| Variant | Device | WER vs fp32 | Speed | VRAM | Size |
|---|---|---|---|---|---|
| sherpa-onnx int8 release | CPU | 21.8% | 53x | | 650 MB |
| **int8-wo-fast** (shipped for CPU) | CPU | **1.3%** | **35x** | | 906 MB |
| int8-wo | CPU | 0.2% | 19x | | 906 MB |
| int4-wo | CPU | 34.0% | | | 649 MB |
| int8-pc, int8-pc-safe | CPU | 95%+ | | | 640 MB |
| fp32 | CPU | 0.0% | 34x | | 2.5 GB |
| fp32, batch 4 | GPU | 0.0% | 155x | 8.1 GB | 2.5 GB |
| int8-wo-fast, batch 4 | GPU | 0.2% | 155x | 7.6 GB | 906 MB |
| **fp16, batch 1** (shipped for GPU) | GPU | **0.6%** | 92x | 2.4 GB | 1.2 GB |
| **fp16, batch 4** | GPU | 0.6% | 155x | 4.5 GB | |
| **fp16, batch 8** | GPU | 0.6% | 176x | 5.5 GB | |

Findings:

- **sherpa-onnx's int8 release** drops whole stretches of speech on stream audio, 43% of the words on one Kick clip. It uses one scale per weight tensor and quantizes activations dynamically.
- **Per-channel dynamic int8** produced near-empty transcripts with ONNX Runtime's CPU kernels.
- **Weight-only int8** (`MatMulNBits`) keeps fp32's accuracy. Its fp32-math mode is slow on CPU; accuracy level 4 (int8 math on activations quantized per block of 128) matches fp32's speed at 1.3% WER, with no dropped speech. **4-bit** loses too much.
- **fp16** for the GPU needs ONNX Runtime's converter (`onnxruntime.transformers.float16`) with overflow-prone ops kept in fp32. sherpa-onnx's straight fp16 conversion produced NaNs that blanked out whole chunks. It halves VRAM against fp32 at the same speed.
- **fp8** wasn't tried: it needs an Ada (RTX 40-series) or newer GPU, and ONNX Runtime's support for it is limited.
