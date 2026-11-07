# vodscribe-worker

The transcription process. It transcribes one stretch of one audio file and prints the words as JSON lines, so a GPU crash or running out of memory fails only that job. `scripts/prepare-sidecars.sh` builds it twice: statically linked for the CPU, and against sherpa-onnx's shared library for the GPU.

- `Cargo.toml`: the crate; its default `static` feature is the CPU build, and `shared` links the shared library for the GPU.
- [`src`](src): the code.
