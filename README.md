# VODscribe

Paste a Kick, Twitch or YouTube VOD link and get a transcript you can search by phrase. Clicking a match plays the VOD from that moment. Everything runs locally: [yt-dlp](https://github.com/yt-dlp/yt-dlp) fetches the audio, ffmpeg decodes it, and NVIDIA's [Parakeet TDT 0.6B v3](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3) (via [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx)) transcribes it.

## Layout

- `crates/core`: the pipeline, with no UI. It covers fetching (`fetch.rs`, plus `kick.rs` for Kick's current VOD API), the queue's download and transcription stages (`pipeline.rs`), speech recognition (`asr.rs`), words and segments (`transcript.rs`), the SQLite FTS5 index (`db.rs`), model downloads (`model.rs`) and the optional GPU pack (`gpu.rs`). It also includes `vodscribe-cli` for testing without the app.
- `crates/worker`: `vodscribe-worker`, which transcribes one audio file in its own process and streams words back. It is built twice: statically linked for the CPU, and against sherpa-onnx's CUDA build for the GPU pack.
- `apps/desktop`: the desktop app. `src` is the SvelteKit (Svelte 5) UI, built statically into the app; `src-tauri` is the Tauri shell, which provides the commands the UI calls and runs the queue (downloading one VOD while transcribing another).
- `apps/site`: the website, once it exists.
- `tools/model-export`: exports and quantizes the speech models we ship, and publishes them. See its README.

On first run the app downloads yt-dlp and the speech model (about 900 MB, from `dl.vodscribe.com`) into its data directory. ffmpeg must be installed separately. YouTube needs a JavaScript runtime for yt-dlp; Deno, Node or Bun on `PATH` is detected automatically. NVIDIA GPU users can download an optional GPU pack (about 3 GB) from the settings.

## Development

Prerequisites: Rust, Node and pnpm, plus [Tauri's system dependencies](https://tauri.app/start/prerequisites/) and ffmpeg.

The JavaScript side is a pnpm workspace (`apps/*`) and the Rust side a Cargo workspace (`crates/*` and `apps/desktop/src-tauri`), both rooted here. From the repo root:

```sh
pnpm install
pnpm dev          # the desktop app, via `tauri dev`
pnpm check        # type-check every app
```

Transcription runs in a separate `vodscribe-worker` process, bundled as a Tauri sidecar along with a CUDA build of it on Linux and Windows. `scripts/prepare-sidecars.sh` builds both into `apps/desktop/src-tauri/binaries/`. `pnpm dev` runs it automatically; run it yourself (`bash scripts/prepare-sidecars.sh [debug|release]`) before building the app with plain `cargo`.

Core tests and the CLI:

```sh
cargo test -p vodscribe-core
VODSCRIBE_DATA=./vodscribe-data cargo run --release -p vodscribe-core --bin vodscribe-cli -- setup
VODSCRIBE_DATA=./vodscribe-data cargo run --release -p vodscribe-core --bin vodscribe-cli -- add <url>
VODSCRIBE_DATA=./vodscribe-data cargo run --release -p vodscribe-core --bin vodscribe-cli -- search "some phrase"
```
