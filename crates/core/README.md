# vodscribe-core

Everything VODscribe does that isn't UI, with no Tauri: the library, fetching VODs and chat, transcription, search, clips and edits.

- `Cargo.toml`: the crate and its `vodscribe-cli` binary; the `static` and `shared` features link sherpa-onnx for the worker, and the app leaves them off.
- [`src`](src): the code.
- [`tests`](tests): test cases shared with the website, and recorded data.
