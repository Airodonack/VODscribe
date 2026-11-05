# Licenses

What `scripts/third-party-notices.py` makes `apps/desktop/src-tauri/THIRD-PARTY-NOTICES.txt` from.

- `about.toml`: the licences the Rust dependencies may use; any other fails the notices, and with them CI.
- `about.hbs`: cargo-about's template for the Rust dependencies' part of the notices.
- `onnxruntime-LICENSE`: ONNX Runtime's licence, for the copy in sherpa-onnx's libraries.
