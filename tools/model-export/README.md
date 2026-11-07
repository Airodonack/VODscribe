# Model export

Builds the speech model VODscribe ships: NVIDIA's [Parakeet TDT 0.6B v3](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3), as ONNX in [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx)'s layout. sherpa-onnx's own int8 release drops stretches of speech on stream audio, so the files are made here.

- `export.py`: exports the fp32 model from NVIDIA's checkpoint.
- `quantize.py`: builds the smaller variants from the fp32 export.
- `evaluate.py`: scores variants against fp32 with VODscribe's own worker.
- `stage.py`: lays out models for upload and prints their pins for `crates/core/src/model.rs`.
- `publish.py`: uploads staged models to the download bucket.
- `pyproject.toml`, `uv.lock`: a [uv](https://docs.astral.sh/uv/) project with Python 3.12, CPU-only PyTorch, NeMo, ONNX and ONNX Runtime.

## Steps

NeMo's build needs a few system packages, so the export runs in a throwaway Distrobox container that shares your home folder.

```sh
distrobox create --name vodscribe-export --image docker.io/library/ubuntu:24.04
distrobox enter vodscribe-export -- sudo apt-get install -y build-essential ffmpeg libsndfile1

W=~/.cache/vodscribe-export
cd tools/model-export
run() { distrobox enter vodscribe-export -- bash -c "cd $PWD && uv run $*"; }

# 1. fp32 export, straight from NVIDIA's checkpoint.
run export.py $W/models/fp32

# 2. Smaller variants built from the fp32 export.
run quantize.py $W/models/fp32 $W/models int8-wo-fast fp16

# 3. Score each variant against fp32 with VODscribe's own workers. Runs on the
#    host; build the workers first (scripts/prepare-sidecars.sh).
python3 evaluate.py $W/audio $W/models/fp32 \
  int8-wo-fast=$W/models/int8-wo-fast fp16-gpu=$W/models/fp16:cuda:4
```

To ship new models, stage them, paste the printed pins into `crates/core/src/model.rs`, and upload:

```sh
python3 stage.py $W/models $W/dist
CLOUDFLARE_ACCOUNT_ID=... uv run --with boto3 publish.py $W/dist
```

The app downloads from `https://dl.vodscribe.com/models` (`MODELS_BASE_URL` in `model.rs`). Never change or delete a file under `models/`: released versions pin them by hash, and the edge caches them for a year. A bucket lock rule refuses changes, and `publish.py` skips files already there with the same content and refuses ones with different content. Ship a changed model under a new model id.

`evaluate.py` reports the word error rate against fp32, the number of words, and the speed against real time. Score a mix like real use: stream audio from each platform and a few non-English samples. WER against fp32 measures what a variant loses, not its accuracy.

## Results

Accuracy was measured on 11 minutes of Kick, Twitch, YouTube, German, French and Spanish audio. Speed was measured on 34 minutes of the same audio, on a Ryzen with 32 threads and an RTX 3080 (10 GB). WER is measured against fp32 on the CPU.

| Variant                             | Device | WER vs fp32 | Speed   | VRAM   | Size   |
| ----------------------------------- | ------ | ----------- | ------- | ------ | ------ |
| sherpa-onnx int8 release            | CPU    | 21.8%       | 53x     |        | 650 MB |
| **int8-wo-fast** (shipped for CPU)  | CPU    | **1.3%**    | **35x** |        | 906 MB |
| int8-wo                             | CPU    | 0.2%        | 19x     |        | 906 MB |
| int4-wo                             | CPU    | 34.0%       |         |        | 649 MB |
| int8-pc, int8-pc-safe               | CPU    | 95%+        |         |        | 640 MB |
| fp32                                | CPU    | 0.0%        | 34x     |        | 2.5 GB |
| fp32, batch 4                       | GPU    | 0.0%        | 155x    | 8.1 GB | 2.5 GB |
| int8-wo-fast, batch 4               | GPU    | 0.2%        | 155x    | 7.6 GB | 906 MB |
| **fp16, batch 1** (shipped for GPU) | GPU    | **0.6%**    | 92x     | 2.4 GB | 1.2 GB |
| **fp16, batch 4**                   | GPU    | 0.6%        | 155x    | 4.5 GB |        |
| **fp16, batch 8**                   | GPU    | 0.6%        | 176x    | 5.5 GB |        |

Findings:

- **sherpa-onnx's int8 release** drops whole stretches of speech on stream audio, 43% of the words on one Kick clip.
- **Per-channel dynamic int8** gives near-empty transcripts with ONNX Runtime's CPU kernels.
- **Weight-only int8** at accuracy level 4 matches fp32's speed at 1.3% WER, with no dropped speech; 4-bit loses too much.
- **fp16** for the GPU needs ONNX Runtime's converter with overflow-prone ops kept in fp32, since sherpa-onnx's straight conversion blanks whole chunks.
- **fp8** wasn't tried: it needs an RTX 40-series or newer card.
