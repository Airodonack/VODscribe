# Third-party crates

Patched copies of crates, used in place of the published versions through
`[patch.crates-io]` in the root `Cargo.toml`. Each keeps its own license.

- `sherpa-onnx-sys` 1.13.8 (Apache-2.0, from k2-fsa/sherpa-onnx). `build.rs`
  links only the static libraries present in `SHERPA_ONNX_LIB_DIR`, so the
  worker can link sherpa-onnx's "no-tts" builds, which leave out the
  text-to-speech libraries and with them espeak-ng (GPL-3.0). The rest is
  unchanged; when bumping sherpa-onnx, copy the new version and reapply that.
