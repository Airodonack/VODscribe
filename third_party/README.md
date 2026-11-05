# Third-party crates

Patched copies of crates, used in place of the published ones through `[patch.crates-io]` in the root `Cargo.toml`. Each keeps its own licence.

- `sherpa-onnx-sys`: version 1.13.8, Apache-2.0, from k2-fsa/sherpa-onnx.

Its `build.rs` links only the static libraries in `SHERPA_ONNX_LIB_DIR`, so the worker can link the "no-tts" builds without espeak-ng, which is GPL-3.0. When bumping sherpa-onnx, copy the new version and make that change again.
