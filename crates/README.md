# Crates

The Rust crates the desktop app is built on. None of them depends on Tauri.

- [`core`](core): the library, fetching, transcription, search, clips and edits.
- [`player`](player): the app's own video player.
- [`worker`](worker): the transcription process the app starts for each job.
