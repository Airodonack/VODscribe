# Crates

The Rust crates the desktop app is built on. None of them depends on Tauri.

- [`dragndrop-source`](dragndrop-source): dragging files out of a window before they're made; to be published on its own.
- [`dragndrop-target`](dragndrop-target): what a drag dropped on a window offers, virtual files included; to be published on its own.
- [`core`](core): the library, fetching, transcription, search, clips and edits.
- [`player`](player): the app's own video player.
- [`worker`](worker): the transcription process the app starts for each job.
