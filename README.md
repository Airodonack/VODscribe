<p align="center">
  <img src="apps/desktop/src-tauri/icons/app-icon.svg" width="96" height="96" alt="VODscribe logo">
</p>

<h1 align="center">VODscribe</h1>

VODscribe is a desktop app for Windows, macOS and Linux that plays Kick, Twitch and YouTube VODs (or links from any other site yt-dlp knows) and video files on your computer, transcribes them when you ask, and lets you search what was said and what chat wrote, then cut clips and edits from what you find. Transcription runs on your computer: [yt-dlp](https://github.com/yt-dlp/yt-dlp) fetches the audio, ffmpeg decodes it, and NVIDIA's [Parakeet TDT 0.6B v3](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3) (via [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx)) transcribes it.

To see it work, try the live demo at [vodscribe.com](https://vodscribe.com): it is the app's own interface, running in the browser on transcribed VODs. This README is for people who want to read the code, check what it does, build it or change it.

## Using it

Download it from [vodscribe.com](https://vodscribe.com), where the demo shows the library, search, the transcript and chat. What each version added, feature by feature, is on [vodscribe.com/releases](https://vodscribe.com/releases), made from [CHANGELOG.md](CHANGELOG.md). In the app, the keyboard button in the player's header lists the controls, and Settings › Controls changes them.

## How it's built

A Tauri 2 app: a Svelte 5 page in the system's web view, a Rust shell around it, and Rust crates that do the work. The JavaScript side is a pnpm workspace (`apps/*`) and the Rust side a Cargo workspace (`crates/*` and `apps/desktop/src-tauri`), both rooted here.

- [`crates/core`](crates/core/src): everything that isn't UI, and no Tauri. The library is one SQLite file (`db.rs`), with FTS5 indexes over the transcript and the chat; `stretches.rs` records what of each VOD is transcribed and what is asked to be. Search is `query.rs` (the search box's language, parsed purely), `search.rs` (the query run against each VOD's timed words, across line breaks, and against chat) and `searcher.rs` (a read connection of its own, so a slow search never holds up the views). Fetching is `fetch.rs` (yt-dlp), `kick.rs` (Kick's current VOD API, which yt-dlp doesn't support yet), `twitch.rs` (Twitch clips) and `local.rs` (files on this computer); `pipeline.rs` gets the audio for a stretch, `audio.rs` decodes it to 16 kHz mono and `asr.rs` runs the model. Also chat (`chat.rs`), clips and edits (`clips.rs`, `edits.rs`, `render.rs`, `footage.rs`), and the pinned downloads (`deps.rs`, `model.rs`, `gpu.rs`). `vodscribe-cli` runs the pipeline without the app.
- [`crates/player`](crates/player/src): the app's own player, for files, kept and downloaded videos and edits, since the web view's media stack differs on each platform. `decode.rs` reads frames and sound through the ffmpeg the app ships, a process per stream; `timeline.rs` says what plays when (a file, or an edit's pieces end to end); `clock.rs` keeps the picture to the sound, which `sound.rs` plays out through cpal; `compose.rs` draws with wgpu; `present_linux.rs`, `present_windows.rs` and `present_macos.rs` put a native surface over the page's player box. `storyboard.rs` makes the small keyframe pictures shown while pointing at the timeline. `vodscribe-play` plays a file in a bare window, without the app.
- [`crates/worker`](crates/worker/src/main.rs): `vodscribe-worker`, which transcribes one stretch of one audio file in its own process and prints the words as JSON lines, so a GPU crash or running out of memory fails only that job. It is built twice: statically linked for the CPU, and against sherpa-onnx's shared no-tts library for the GPU, which ships with the app (`apps/desktop/src-tauri/lib`, a bundle resource) while the GPU add-on supplies only ONNX Runtime with CUDA and NVIDIA's libraries. sherpa-onnx's CUDA release includes its full build with the GPL-3.0 espeak-ng, which is why that library isn't taken from it; [`third_party/sherpa-onnx-sys`](third_party/README.md) is the patched binding that links the no-tts builds.
- [`apps/desktop/src-tauri`](apps/desktop/src-tauri/src): the Tauri shell. The commands the page calls are in `vods.rs` (the library), `setup.rs` (setup, settings, the GPU add-on, storage), `clips.rs`, `edits.rs`, `videos.rs` and `thumbnails.rs`. `worker.rs` is the transcription queue, two stages on their own threads (one downloads while the other transcribes); `chats.rs` fetches chat replays apart from it; `jobs.rs` runs clips, renders, video downloads and storyboards one at a time. `player.rs` and `screen.rs` drive crates/player from the page; `history.rs` is undo and redo. `app_server.rs` serves the page from `http://localhost` in release builds (YouTube's and Twitch's players need an http(s) page), and `media.rs` serves local files to the page's `<video>`, both on loopback only (`loopback.rs`). `e2e.rs` (the `e2e` feature, for the tests) and `embed.rs` with `embed_shim.js` (the `embed-takeover` feature) are never in a release build.
- [`apps/desktop/src`](apps/desktop/src): the SvelteKit page, built statically into the app. `routes/+page.svelte` opens `lib/App.svelte`; `lib/api.ts` types the shell's commands and events. The keyboard controls are one table in `lib/keymap.ts`, with the user's own bindings over it in `lib/keys.svelte.ts`; `lib/embed.ts` drives YouTube's and Twitch's players in sandboxed frames over postMessage, and plays Kick's HLS stream in a plain `<video>`. The pure parts have `node --test` files beside them (`*.test.mjs`).
- [`apps/site`](apps/site): vodscribe.com, whose demo is the desktop app's page with stand-ins for the shell (see [Website](#website)).
- [`tests/e2e`](tests/e2e/README.md): the end-to-end harness, which drives built apps on Linux, Windows and macOS.
- [`scripts`](scripts): the sidecar build, release packaging and checks, the third-party notices, and the check of pinned downloads.
- [`tools/model-export`](tools/model-export/README.md): exports and quantizes the speech models we ship, and publishes them.

### From a link to a clip

1. A link pasted in the app goes to `vods.rs`, which has yt-dlp read the VOD's details (`fetch.rs`, or `kick.rs` for Kick) and adds it to the library, with a local copy of its thumbnail. For Kick, Twitch and YouTube livestreams, `chats.rs` fetches the chat replay in the background. A file is added in place: the app reads it but never copies, moves or deletes it.
2. Nothing is transcribed until asked. Transcribe adds a request to `stretches` for the whole VOD or a stretch of it, and the queue (`worker.rs`) takes requests in order. Its download stage gets the stretch's audio (`pipeline::audio_for`): the file itself, the VOD's whole audio if it's already here, or just that section through yt-dlp's `--download-sections`. Its transcription stage runs `vodscribe-worker` on it, on the GPU if the add-on is installed (falling back to the CPU when the GPU is busy or has too little free memory), and writes the words to the library as they come, so the transcript is searchable while it's made.
3. A search goes from the page to `search.rs` through `searcher.rs`; a click on a result plays the VOD there. YouTube and Twitch VODs play in their sites' players, Kick's from its stream, and everything on this computer in crates/player, frame-exact (in the web view's `<video>` where the player can't draw: sound only, or no screen to draw on).
4. A clip is a stretch made into a file: `footage.rs` finds where its picture and sound come from (the VOD's file, a clip's, or a section downloaded for it into `footage/`), and `clips.rs` in core cuts it with ffmpeg as H.264 and AAC, to the frame, into the Videos folder under VODscribe. An edit joins stretches of any VODs and is rendered from the same footage (`render.rs`).

### Where the library lives

Everything is in one data folder: `~/.local/share/app.vodscribe` on Linux, `~/Library/Application Support/app.vodscribe` on macOS and `%APPDATA%\app.vodscribe` on Windows (`Paths` in `crates/core/src/lib.rs`).

- `library.sqlite` holds the library: VODs, transcripts, chat, stretches, clips and edits. Before an update changes how it's stored, the app keeps a copy as it was, `library-v<N>.sqlite` beside it (the newest two, each removed after 30 days). Removing a VOD scrubs its transcript from the file.
- `settings.json`, `layers.json` (timeline layers) and `playable.json` (what this computer's web view plays).
- What can be downloaded or made again: `bin` (yt-dlp, ffmpeg, Deno), `models`, `gpu` (the add-on), `emotes`, `thumbs`, `peaks` (waveforms), `storyboards`, `footage` (sections downloaded for clips and edits) and `work` (downloads in progress); and `media`, the videos kept from sites without a player of their own, which stay until deleted. On Windows these go in `%LOCALAPPDATA%\app.vodscribe` instead, so they don't roam.

### Privacy

Audio, transcripts and searches stay on your computer; there's no account or telemetry. The app contacts other servers only for these (the website's [privacy page](https://vodscribe.com/privacy) says the same for people who use it):

- **App updates:** at launch and once a day, the app fetches `dl.vodscribe.com/releases/latest.json`, which names the newest version. Updates install only when the user agrees, and only if signed with VODscribe's update key.
- **Setup and tool updates:** yt-dlp from its GitHub releases (checked against the yt-dlp signing key); ffmpeg (FFmpeg 9.0, BtbN's builds on Linux and Windows, Martin Riedl's on macOS) and Deno, which yt-dlp needs for some YouTube VODs, from their builders' GitHub releases or site; the speech model (about 950 MB) from `dl.vodscribe.com`; and the optional GPU add-on from GitHub and PyPI. Everything but yt-dlp is pinned by SHA-256 (`crates/core/src/deps.rs`, `model.rs`, `gpu.rs`). Copies of ffmpeg, Deno, Node or Bun already on `PATH` are used instead of downloading.
- **Adding a VOD:** yt-dlp fetches its details from the platform, and the app saves a copy of its thumbnail. Its audio is fetched when it's transcribed; for sites without an embeddable player, its video too, kept to play here. When asked, the app downloads a VOD's video, or the section a clip or an edit needs, from the same place. Playing a Twitch clip asks Twitch for a signed link to its video. Links must lead to the public internet, not this computer or the local network. Local files are read in place, and nothing about them is sent anywhere.
- **Chat:** for Kick and Twitch VODs, the chat replay comes from those sites' own web APIs (`web.kick.com`, `gql.twitch.tv`); for YouTube, through yt-dlp. Emote images come from the platforms' image servers (`files.kick.com`, `static-cdn.jtvnw.net`, `yt3.ggpht.com`), each once, then kept in the data folder.
- **Emotes chat types as words** (only once turned on in Settings → Appearance): for Kick and Twitch chats, the channel's 7TV, BetterTTV and FrankerFaceZ emotes and their global ones (`7tv.io`, `api.betterttv.net`, `api.frankerfacez.com`; images from `cdn.7tv.app`, `cdn.betterttv.net`, `cdn.frankerfacez.com`), looked up by the channel's id (from Twitch's API or `kick.com`) at most once a day per channel. 7TV is the only one of the three on Kick.
- **Playing a VOD:** only after you click play or a timestamp, the platform's player loads (YouTube's privacy-enhanced `youtube-nocookie.com` embed, Twitch's player, or Kick's stream). YouTube and Twitch run in sandboxed frames with no access to the app.

## Verifying it

The tests are layered, each checking what the one below can't:

- **Unit tests** of the pure parts: the core (`cargo test -p vodscribe-core`; its search cases in `crates/core/tests/phrase-search.json` also test the website's port), the player (`cargo test -p vodscribe-player`, which needs ffmpeg and checks frames by their pixels), the shell (`cargo test -p vodscribe`, after `scripts/prepare-sidecars.sh`), and the page's `*.test.mjs` files (the `node --test` steps in `.github/workflows/check.yml`).
- **End-to-end scenarios** in `tests/e2e/scenarios`, which drive a built app through its page as a person would and check what the page and the screen show. `node tests/e2e/run.mjs <app> --only <scenario>` runs them against one build on this computer; `node tests/e2e/distros.mjs <folder with deb/ and rpm/> --only debian-13` installs the packages in a container per Linux distribution and runs them there. The scenarios that reach Twitch, YouTube or Kick run only with `E2E_NETWORK=1`, and the `perf-*` ones fail when a timing goes over its budget in `tests/e2e/budgets.json`.
- **The verifier** on top: `node tests/e2e/verify.mjs tests/e2e/out` has a model look at the checkpoints a run took, against what each scenario says a person should see there, for what no check can judge (a picture cut off, text over text). It needs an `OPENROUTER_API_KEY` and costs money per run, so it belongs in the full lane only.

[tests/e2e/README.md](tests/e2e/README.md) has what each needs (tauri-driver, podman, a headless display), the environment, shards, the flake ledger, timings and profiling.

### CI lanes

`check.yml` (formatting, type checks, the website build, the third-party notices, and the core's and the player's lints and tests) runs on every push and pull request. `build.yml` builds the installers and drives them end to end (`e2e.yml`), in one of two lanes:

| Event                                                         | Lane | Runs                                                                                                                                                                                                                           |
| ------------------------------------------------------------- | ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| push to `main`, pull request (not for site and notes changes) | fast | the Linux build; the Linux end-to-end tests in 2 shards. A newer push to the same ref cancels the run in progress.                                                                                                             |
| every night at 09:00 UTC, a manual run, a `v*` tag            | full | all four builds (Linux, Windows, macOS Apple silicon and Intel); the end-to-end tests of Linux and Windows in 2 shards each and macOS in 3. Never cancelled by a push. A tag then publishes the release and submits to winget. |

A last job merges the shards' results into one summary and fails on a failed scenario or a shard that never reported. To run the full lane by hand: `gh workflow run build.yml` (or `-f lane=fast` for the fast one), then `gh run watch`. To run only the end-to-end tests against an earlier build's installers, `gh workflow run e2e.yml -f build_run=<run id>`. The Pinned downloads workflow runs `scripts/check-pinned-downloads.py` weekly: it sends a HEAD request for every pinned download and fails if one is gone or a different size, since a vanished build would otherwise break setup for new users silently.

## Development

Prerequisites: Rust (the version in `rust-toolchain.toml`, which rustup picks up), Node (`.nvmrc`) and pnpm, plus [Tauri's system dependencies](https://tauri.app/start/prerequisites/) and ffmpeg. `pnpm dev` runs a bash script, so on Windows it needs Git Bash on `PATH`; release builds on Linux also need `patchelf`. From the repo root:

```sh
pnpm install
pnpm dev          # the desktop app, via `tauri dev`
pnpm site         # the website, apps/site, at localhost:5173
pnpm check        # type-check every app
pnpm format       # Prettier, every app (`pnpm lint` checks without writing)
cargo fmt --all   # rustfmt, with the settings in rustfmt.toml
```

Code is formatted with rustfmt and Prettier, and CI refuses code that isn't (`.github/workflows/check.yml` on every push, and again before release builds). Run `git config core.hooksPath .githooks` once per clone, so `.githooks/pre-commit` refuses an unformatted commit before CI does. Claude Code formats each file it edits by itself, through the hook in `.claude/settings.json`.

`apps/desktop/src-tauri/THIRD-PARTY-NOTICES.txt` holds the license of everything compiled into the app; the installers include it and the website serves it. After changing dependencies, regenerate it with `python3 scripts/third-party-notices.py` (it needs [cargo-about](https://github.com/EmbarkStudios/cargo-about) 0.9). CI fails when it's out of date, or when a Rust dependency's license isn't one of those accepted in `licenses/about.toml`.

The worker is bundled as a Tauri sidecar, along with a CUDA build of it on Linux and Windows. `scripts/prepare-sidecars.sh` builds both into `apps/desktop/src-tauri/binaries/`. `pnpm dev` runs it automatically; run it yourself (`bash scripts/prepare-sidecars.sh [debug|release]`) before building the app with plain `cargo`, and again after changing the worker, since the app runs the copy there.

The CLI runs the pipeline on a library of its own:

```sh
VODSCRIBE_DATA=./vodscribe-data cargo run --release -p vodscribe-core --bin vodscribe-cli -- setup
VODSCRIBE_DATA=./vodscribe-data cargo run --release -p vodscribe-core --bin vodscribe-cli -- add <url>
VODSCRIBE_DATA=./vodscribe-data cargo run --release -p vodscribe-core --bin vodscribe-cli -- add-file <path>
VODSCRIBE_DATA=./vodscribe-data cargo run --release -p vodscribe-core --bin vodscribe-cli -- search "some phrase"
```

The app icon is generated from `apps/desktop/src-tauri/icons/app-icon.svg`; after changing it, run `pnpm tauri icon src-tauri/icons/app-icon.svg -o src-tauri/icons` from `apps/desktop` and copy the 32 px PNG to `static/favicon.png`, and `icons/icon.ico` to `apps/site/static/favicon.ico` for the website. macOS icons follow Apple's grid (a smaller rounded square with a shadow), so `icon.icns` comes from `app-icon-macos.svg` instead: generate into a temporary directory with `pnpm tauri icon src-tauri/icons/app-icon-macos.svg -o /tmp/mac-icons` and copy just its `icon.icns` over. Keep the two SVGs' glyphs in sync.

The app's content security policy is in `tauri.conf.json` (`app.security.csp`). It only applies to built apps, not `pnpm dev`, so a new external script, frame or image host needs adding there or it will work in development and break in release.

`scripts/reset-setup.sh` (Linux and macOS) removes the speech model and yt-dlp so the first-run setup can be tested again; the library, settings and GPU add-on stay.

## Website

`apps/site` is vodscribe.com: a landing page whose live demo is the app itself. It imports the desktop app's components as the workspace package `desktop` (`desktop/lib/…`, exported from `apps/desktop/package.json`), and `vite.config.ts` swaps the Tauri modules they call for stand-ins (`apps/site/src/lib/tauri/`) that answer from transcripts in `static/demo/`, with a port of the app's phrase search (`search.ts`). With `__DEMO__` defined, controls that only work in the app gray out and explain themselves on hover (`apps/desktop/src/lib/demo.ts`); mark new ones with `demoOff`.

The demo's VODs are transcribed with the CLI into a fresh library, then exported (`scripts/export-demo-chat.py` exports a VOD's chat, with everyone in it given a made-up name):

```sh
export VODSCRIBE_DATA=/tmp/demo
vodscribe-cli add <url>          # for each VOD; `setup` first, or link bin/ and models/ from an existing library
python3 apps/site/scripts/export-demo.py /tmp/demo/library.sqlite
```

`pnpm --filter site build` prerenders every page into `apps/site/build`, with a content security policy that SvelteKit writes into each page and response headers in `static/_headers`. The download buttons link to `dl.vodscribe.com/releases/latest/` (`src/lib/downloads.ts`), so each release has to be uploaded there under those stable names; the files are served with a `Content-Disposition` that names the versioned file, so what's saved says which version it is.

It deploys from this repo through Cloudflare's Git integration: build command `pnpm install --frozen-lockfile && pnpm --filter site build`, with `apps/site/build` as the output directory on Pages. On Workers, `apps/site/wrangler.jsonc` sets the same directory and serves `404.html` for missing pages. The production site builds from the `site` branch, which each release moves to its tag: the demo is the app's own code, so built from `main` it would show what hasn't been released. Changes to the site alone between releases go on `site` too (or are merged into it); every other branch gets a preview.

## Building

`pnpm tauri build` in `apps/desktop` (after `prepare-sidecars.sh release`) produces the installers for the current platform. In CI the bundles are uploaded as artifacts (see [CI lanes](#ci-lanes)). macOS bundles are ad-hoc signed; nothing is signed with a paid certificate yet.

## License

VODscribe is source-available under the [Functional Source License, Version 1.1, ALv2 Future License](LICENSE.md): use, read, change and redistribute it for anything except a competing product, and each version becomes Apache 2.0 two years after its release. The third-party software it's built on keeps its own licenses (`THIRD-PARTY-NOTICES.txt`, and the website's licenses page). [CONTRIBUTING.md](CONTRIBUTING.md) covers changes and [SECURITY.md](SECURITY.md) vulnerability reports.

## Releasing

Between releases, `version` in the root `Cargo.toml` is the next one in development, such as `0.4.1-dev`: development builds don't pass for the last release, and semver orders it after 0.4.0 and before 0.4.1, so the updater neither offers 0.4.0 to them nor holds back 0.4.1. (An MSI's version is numbers only, so builds of a development version give theirs as `0.4.1`.)

1. Write the release's entry in `CHANGELOG.md` as you go (the heading says "(unreleased)" until then; CI checks that the version in `Cargo.toml` has one).
2. Set `version` to the release's (drop `-dev`), date the changelog heading, and commit ("Version 0.4.1", with the entry in its message).
3. `python3 scripts/ready-to-release.py` checks it's ready, then tag it (`git tag v0.4.1`) and push the tag. The entry becomes the release notes in `latest.json`; the tag build fails if it's missing or still marked unreleased.
4. Once it's out, move `site` to the tag (`git push origin v0.4.1^{commit}:refs/heads/site`), and set `main`'s version to the next in development (`0.4.2-dev`) in a commit of its own, which also opens its changelog entry (`## 0.4.2 (unreleased)`: CI's check takes a development version's entry to be that one). `ready-to-release.py` prints both.

The tag's build signs the update bundles (`TAURI_SIGNING_PRIVATE_KEY`), then `scripts/release.py` names every file (`VODscribe-<version>-windows-x64-setup.exe` and so on, plus the stable names for the website) and writes `latest.json`, and the publish job uploads them to R2: `releases/<version>/` for the updater and winget, `releases/latest/` for the website's buttons, and `releases/latest.json` last, then checks the release from the outside, as the app and the website see it (`scripts/verify-release.py`). The app checks that manifest at launch and daily, and installs an update only when the user says so, and only if it's signed with the key whose public half is in `tauri.conf.json`. Keep the private key (`~/.tauri/vodscribe-updater.key`) backed up: without it, installed copies can't be updated.

The winget job then submits the new version to `microsoft/winget-pkgs` as `Airodonack.VODscribe`, once `WINGET_TOKEN` (a classic token of the Airodonack GitHub account, with `public_repo`) is set. Setting the repository variable `WINGET_SUBMIT` to `false` skips it, for while a version waits in winget's review: a newer PR would supersede that one and start the review over. The workflow's header lists every secret and variable.

Speech models are served from Cloudflare R2 at `https://dl.vodscribe.com/models/<model id>/`, pinned by size and SHA-256 in `crates/core/src/model.rs`. `VODSCRIBE_MODELS_URL` overrides the base URL in debug builds, for testing.
