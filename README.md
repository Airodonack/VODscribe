<p align="center">
  <img src="apps/desktop/src-tauri/icons/app-icon.svg" width="96" height="96" alt="VODscribe logo">
</p>

<h1 align="center">VODscribe</h1>

Paste a Kick, Twitch or YouTube VOD link and get a transcript you can search by phrase. Clicking a match plays the VOD from that moment. Transcription runs on your computer: [yt-dlp](https://github.com/yt-dlp/yt-dlp) fetches the audio, ffmpeg decodes it, and NVIDIA's [Parakeet TDT 0.6B v3](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3) (via [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx)) transcribes it.

## Using it

On first run the app downloads yt-dlp and the speech model (about 900 MB, from `dl.vodscribe.com`) into its data directory. ffmpeg must be installed separately. YouTube needs a JavaScript runtime for yt-dlp; Deno, Node or Bun on `PATH` is detected automatically.

- **Library.** Paste a link in the sidebar, or drop links and video or audio files anywhere on the window ("add video files…" opens a file picker). Files are transcribed where they are; the app never copies, moves or deletes them. Dropping links doesn't work on Windows yet, where pasting does. VODs download and transcribe in the background, one of each at a time, working up from the bottom of the list, and can be paused, resumed, cancelled or removed from their row. The transcript is searchable while it is still being produced. Drag a row to reorder the library (or `Alt+↑`/`↓` on a focused row); the lowest waiting VOD is processed next, and searches across the library list matches in the same order. New VODs go on top, so they wait their turn unless dragged down.
- **Search.** Searches are exact phrases, so fewer words match more. With a VOD selected, the chip in the search field toggles between searching just that VOD and the whole library.
- **Player.** Selecting a VOD shows its thumbnail; clicking it, a match or a transcript line loads the player and jumps there. The player can be collapsed to leave more room for the transcript, and clicking the timestamp in its header copies a link to that moment on the VOD's site. Keyboard: `Space` or `K` plays and pauses, `←`/`→` scrub 5 s (1 s with `Shift`), `J`/`L` 10 s.
- **GPU.** NVIDIA GPU users can download an optional GPU pack (about 3 GB) from the settings. Transcription falls back to the CPU when the GPU is busy or has too little free memory.
- **Older models.** When a release ships a new speech model, transcripts made with the previous one are marked. They still work; re-transcribing replaces them.
- **Storage.** "Free up space" in the settings deletes everything that can be downloaded again (models, tools, the GPU pack, audio). The library and transcripts are kept. Removing a VOD scrubs its transcript from the database file.

### Privacy

Audio, transcripts and searches stay on your computer; there's no account, telemetry or update check. The app contacts other servers only for these:

- **Setup and updates:** yt-dlp from its GitHub releases (checked against the yt-dlp signing key), the speech model from `dl.vodscribe.com`, and the optional GPU pack from GitHub and PyPI (each file pinned by SHA-256).
- **Adding a VOD:** yt-dlp fetches its details and audio from the platform, and the app saves a copy of its thumbnail. Local files are read in place, and nothing about them is sent anywhere.
- **Playing a VOD:** only after you click play or a timestamp, the platform's player loads (YouTube's privacy-enhanced `youtube-nocookie.com` embed, Twitch's player, or Kick's stream). YouTube and Twitch run in sandboxed frames with no access to the app.

The library (`library.sqlite`) and `settings.json` live in `~/.local/share/app.vodscribe` on Linux, `~/Library/Application Support/app.vodscribe` on macOS and `%APPDATA%\app.vodscribe` on Windows. Downloads (`models`, `bin`, `gpu`, `work`) go in the same folder, except on Windows, where they go in `%LOCALAPPDATA%\app.vodscribe` so they don't roam.

## Layout

- `crates/core`: the pipeline, with no UI. It covers fetching (`fetch.rs`, plus `kick.rs` for Kick's current VOD API, and `local.rs` for files on this computer), the queue's download and transcription stages (`pipeline.rs`), speech recognition (`asr.rs`), words and segments (`transcript.rs`), the SQLite FTS5 index (`db.rs`), model downloads (`model.rs`) and the optional GPU pack (`gpu.rs`). It also includes `vodscribe-cli` for testing without the app.
- `crates/worker`: `vodscribe-worker`, which transcribes one audio file in its own process and streams words back. It is built twice: statically linked for the CPU, and against sherpa-onnx's CUDA build for the GPU pack.
- `apps/desktop`: the desktop app. `src` is the SvelteKit (Svelte 5) UI, built statically into the app; `src-tauri` is the Tauri shell, which provides the commands the UI calls and runs the queue. Icons are inline SVGs in `src/lib/Icon.svelte`; the logo is `src/lib/Logo.svelte`.
- `tools/model-export`: exports and quantizes the speech models we ship, and publishes them. See its README.

## Development

Prerequisites: Rust (the version in `rust-toolchain.toml`, which rustup picks up), Node (`.nvmrc`) and pnpm, plus [Tauri's system dependencies](https://tauri.app/start/prerequisites/) and ffmpeg. `pnpm dev` runs a bash script, so on Windows it needs Git Bash on `PATH`; release builds on Linux also need `patchelf`.

The JavaScript side is a pnpm workspace (`apps/*`) and the Rust side a Cargo workspace (`crates/*` and `apps/desktop/src-tauri`), both rooted here. From the repo root:

```sh
pnpm install
pnpm dev          # the desktop app, via `tauri dev`
pnpm check        # type-check every app
pnpm --filter desktop format   # Prettier (lint checks without writing)
```

Transcription runs in a separate `vodscribe-worker` process, bundled as a Tauri sidecar along with a CUDA build of it on Linux and Windows. `scripts/prepare-sidecars.sh` builds both into `apps/desktop/src-tauri/binaries/`. `pnpm dev` runs it automatically; run it yourself (`bash scripts/prepare-sidecars.sh [debug|release]`) before building the app with plain `cargo`.

Core tests and the CLI:

```sh
cargo test -p vodscribe-core
VODSCRIBE_DATA=./vodscribe-data cargo run --release -p vodscribe-core --bin vodscribe-cli -- setup
VODSCRIBE_DATA=./vodscribe-data cargo run --release -p vodscribe-core --bin vodscribe-cli -- add <url>
VODSCRIBE_DATA=./vodscribe-data cargo run --release -p vodscribe-core --bin vodscribe-cli -- add-file <path>
VODSCRIBE_DATA=./vodscribe-data cargo run --release -p vodscribe-core --bin vodscribe-cli -- search "some phrase"
```

The app icon is generated from `apps/desktop/src-tauri/icons/app-icon.svg`; after changing it, run `pnpm tauri icon src-tauri/icons/app-icon.svg -o src-tauri/icons` from `apps/desktop` and copy the 32 px PNG to `static/favicon.png`. macOS icons follow Apple's grid (a smaller rounded square with a shadow), so `icon.icns` comes from `app-icon-macos.svg` instead: generate into a temporary directory with `pnpm tauri icon src-tauri/icons/app-icon-macos.svg -o /tmp/mac-icons` and copy just its `icon.icns` over. Keep the two SVGs' glyphs in sync.

The app's content security policy is in `tauri.conf.json` (`app.security.csp`). It only applies to built apps, not `pnpm dev`, so a new external script, frame or image host needs adding there or it will work in development and break in release.

`scripts/reset-setup.sh` (Linux and macOS) removes the speech model and yt-dlp so the first-run setup can be tested again; the library, settings and GPU pack stay.

## Building

`pnpm tauri build` in `apps/desktop` (after `prepare-sidecars.sh release`) produces the installers for the current platform. The GitHub Actions workflow in `.github/workflows/build.yml` builds Linux, Windows and macOS (Intel and Apple Silicon) on a manual run or a `v*` tag, and uploads the unsigned bundles as artifacts.

Speech models are served from Cloudflare R2 at `https://dl.vodscribe.com/models/<model id>/`, pinned by size and SHA-256 in `crates/core/src/model.rs`. `VODSCRIBE_MODELS_URL` overrides the base URL in debug builds, for testing.
