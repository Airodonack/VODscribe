# vodscribe

Paste a Kick, Twitch or YouTube VOD link and get a transcript you can search by phrase. Clicking a match plays the VOD from that moment. Everything runs locally: [yt-dlp](https://github.com/yt-dlp/yt-dlp) fetches the audio, ffmpeg decodes it, and NVIDIA's [Parakeet TDT 0.6B v3](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3) (via [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx)) transcribes it.

## Layout

- `crates/core`: the pipeline, with no UI. It covers fetch (`fetch.rs`), decode and chunk (`audio.rs`), speech recognition (`asr.rs`), words and segments (`transcript.rs`), and the SQLite FTS5 index (`db.rs`). It also includes `vodscribe-cli` for testing without the app.
- `src-tauri`: the desktop shell. It provides the commands the UI calls and a background worker that processes the queue one VOD at a time.
- `src`: the SvelteKit (Svelte 5) UI, built statically into the app.

On first run the app downloads the speech model (about 470 MB) and yt-dlp into its data directory. ffmpeg must be installed separately. YouTube needs a JavaScript runtime for yt-dlp; Deno, Node or Bun on `PATH` is detected automatically.

## Development

Prerequisites: Rust, Node and pnpm, plus [Tauri's system dependencies](https://tauri.app/start/prerequisites/) and ffmpeg.

```sh
pnpm install
pnpm tauri dev
```

Core tests and the CLI:

```sh
cargo test -p vodscribe-core
VODSCRIBE_DATA=./vodscribe-data cargo run --release -p vodscribe-core --bin vodscribe-cli -- setup
VODSCRIBE_DATA=./vodscribe-data cargo run --release -p vodscribe-core --bin vodscribe-cli -- add <url>
VODSCRIBE_DATA=./vodscribe-data cargo run --release -p vodscribe-core --bin vodscribe-cli -- search "some phrase"
```
