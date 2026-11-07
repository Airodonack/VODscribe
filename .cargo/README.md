# Cargo settings

- `config.toml`: links the static C runtime on Windows, as sherpa-onnx's prebuilt libraries need.

CI sets `RUSTFLAGS`, which replaces this file's flags, so `build.yml` repeats the flag.
