<p align="center">
  <img src="apps/desktop/src-tauri/icons/app-icon.svg" width="96" height="96" alt="VODscribe logo">
</p>

<h1 align="center">VODscribe</h1>

Paste a Kick, Twitch or YouTube VOD link (or one from any other site yt-dlp knows) and get a transcript you can search by phrase, with the stream's chat beside it. Clicking a match plays the VOD from that moment. Transcription runs on your computer: [yt-dlp](https://github.com/yt-dlp/yt-dlp) fetches the audio, ffmpeg decodes it, and NVIDIA's [Parakeet TDT 0.6B v3](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3) (via [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx)) transcribes it.

## Using it

On first run the app downloads what it needs into its data directory: yt-dlp, ffmpeg (FFmpeg 9.0 from BtbN's builds on Linux and Windows, Martin Riedl's on macOS), the speech model (about 950 MB, from `dl.vodscribe.com`) and Deno, which yt-dlp needs for some YouTube VODs. Each is checked before use: yt-dlp against its signed checksums, the rest against SHA-256 sums pinned in the app (`crates/core/src/deps.rs`, `model.rs`). Copies of ffmpeg, Deno, Node or Bun already on `PATH` are used instead of downloading.

- **Library.** Paste a link in the sidebar, or drop links and video or audio files anywhere on the window ("add video files…" opens a file picker). Files are transcribed where they are; the app never copies, moves or deletes them. Dropping links works on Linux; on Windows and macOS the webview only passes file drops to the app, so links are pasted. VODs download and transcribe in the background, one of each at a time, working up from the bottom of the list, and can be paused, resumed, cancelled or removed from their row. The transcript is searchable while it is still being produced. Drag a row to reorder the library (or `Alt+↑`/`↓` on a focused row); the lowest waiting VOD is processed next, and searches across the library list matches in the same order. New VODs go on top, so they wait their turn unless dragged down.
- **Other sites.** Links from any site yt-dlp has an extractor for (not its generic page reader) work too. Without an embeddable player, their video, or audio, is kept in the data folder to play in the app: at most the height chosen in Settings (720p unless changed), in codecs the app checked this computer's webview plays at launch, and checked again once downloaded. The crossed-out camera in its row deletes the video and keeps the transcript. Sites that need a login aren't supported. Links must lead to the public internet, not this computer or the local network.
- **Search.** Searches are exact phrases, so fewer words match more, through what was said, what chat wrote, or both. The From button by the search field picks those, and whether to search the selected VOD or the whole library (choosing All VODs locks it there as you switch VODs).
- **Chat.** For Kick, Twitch and YouTube livestreams, the chat replay is fetched in the background and shown beside the transcript, each line with the messages sent while it was said; the column can be hidden. A fetch that's interrupted carries on where it stopped. Subs, gifts and tips (bits, KICKs, Super Chats) are recognized and badged. Emotes show as images: the platforms' own, and for Kick and Twitch, if turned on in Settings → Appearance, the channel's 7TV, BetterTTV and FrankerFaceZ emotes, the providers in the order you drag them to.
- **Timeline.** A strip at the right edge shows how busy chat was over the whole VOD; click or drag on it to go there. The Timeline button adds layers: saved chat searches (laughter, hype, "clip it" to start, or your own words and emotes, for every channel or one), up to three drawn at once with their messages highlighted, and the events (subs, gifts, bits, KICKs, Super Chats and memberships), drawn as bars, stronger where more money was spent.
- **Appearance.** Settings chooses the theme (light, dark, or as the system is) and, off until turned on, the 7TV, BetterTTV and FrankerFaceZ emotes in chat.
- **Player.** Selecting a VOD shows its thumbnail; clicking it, a match or a transcript line loads the player and jumps there. Files, videos kept from other sites and edits play in VODscribe's own player (frame-exact, with captions over the picture); Kick, Twitch and YouTube VODs in their sites' players. The player can be collapsed to leave more room for the transcript, and clicking the timestamp in its header copies a link to that moment on the VOD's site. Keyboard: `Space` or `K` plays and pauses, `←`/`→` scrub 5 s (1 s with `Shift`), `J`/`L` 10 s, `,`/`.` step a frame, `Shift+C` turns captions on or off and `Shift+M` mutes; the keyboard button in its header lists the rest.
- **Cuts, chapters and highlights.** `B` cuts the timeline at the playhead, splitting the VOD into stretches; `N` starts a chapter and `M` marks a highlight, each with a title (`Escape` right away takes one made by mistake; `Enter` renames the selected one). Chapters head the transcript and copy as YouTube chapters. Marks are dragged, or moved a frame at a time, and saved per VOD.
- **Clips.** Clip (`C`) makes the lit-up stretch a clip (the selected lines, else the stretch at the playhead, or with no cuts the whole video): a file in the Videos folder under VODscribe, as MP4 or MOV (Settings), shown as a pill in the clip bar. Drag a pill, the Clip button, or a stretch of the timeline with `Alt` (`Option` on a Mac) into a video editor or a folder; a clip dragged out stays where it went. `Enter` or `F2` renames a clip; one whose file is deleted outside the app leaves the clip bar.
- **Edits.** Join stretches of any VODs into one video: an edit sits in the library's list, takes clips and stretches dropped on it, and opens like a VOD, with its pieces as bands to reorder or resize (`Ctrl+←`/`→`, `⌘` on a Mac, moves the one at the playhead), `B` to split one, `Delete` or Remove to take it out, its transcript under it (only what plays: click a line to play from it, drag across lines or `Shift`+click to select them, `Delete` to take them out), and Render (`E`, export) to make its file.
- **Undo, back and forward.** `Ctrl+Z` and `Ctrl+Shift+Z` (`⌘` on a Mac) undo and redo changes to marks, the clip bar and edits; `Alt+←`/`Alt+→` (`⌘[`/`⌘]`) step between the VODs and edits opened, each as it was left.
- **GPU.** NVIDIA GPU users can download an optional GPU add-on (about 3.4 GB) from the settings. Transcription falls back to the CPU when the GPU is busy or has too little free memory.
- **Older models.** When a release ships a new speech model, transcripts made with the previous one are marked. They still work; re-transcribing replaces them.
- **Storage.** "Repair" in the settings deletes everything that can be downloaded again (models, tools, the GPU add-on, audio) and sets VODscribe up again. The library, transcripts and videos kept from other sites are kept; Settings shows the space those videos take. Removing a VOD scrubs its transcript from the database file. Settings → Help → Reset settings puts every setting back as it started, keeping the library.

### Privacy

Audio, transcripts and searches stay on your computer; there's no account or telemetry. The app contacts other servers only for these:

- **App updates:** at launch and once a day, the app fetches `dl.vodscribe.com/releases/latest.json`, which names the newest version. Updates install only when the user agrees, and only if signed with VODscribe's update key. What changed in each version is on vodscribe.com/releases, made from `CHANGELOG.md`; the version number in the app's sidebar and the update notice link to it.
- **Setup and tool updates:** yt-dlp from its GitHub releases (checked against the yt-dlp signing key); ffmpeg and Deno from their builders' GitHub releases or site, the speech model from `dl.vodscribe.com`, and the optional GPU add-on from GitHub and PyPI (each file pinned by SHA-256).
- **Adding a VOD:** yt-dlp fetches its details and audio from the platform (for other sites, the video, kept to play here), and the app saves a copy of its thumbnail. Playing a Twitch clip asks Twitch for a signed link to its video. Local files are read in place, and nothing about them is sent anywhere.
- **Chat:** for Kick and Twitch VODs, the chat replay comes from those sites' own web APIs (`web.kick.com`, `gql.twitch.tv`); for YouTube, through yt-dlp. Emote images come from the platforms' image servers (`files.kick.com`, `static-cdn.jtvnw.net`, `yt3.ggpht.com`), each once, then kept in the data folder.
- **Emotes chat types as words** (only once turned on in Settings → Appearance): for Kick and Twitch chats, the channel's 7TV, BetterTTV and FrankerFaceZ emotes and their global ones (`7tv.io`, `api.betterttv.net`, `api.frankerfacez.com`; images from `cdn.7tv.app`, `cdn.betterttv.net`, `cdn.frankerfacez.com`), looked up by the channel's id (from Twitch's API or `kick.com`) at most once a day per channel. 7TV is the only one of the three on Kick.
- **Playing a VOD:** only after you click play or a timestamp, the platform's player loads (YouTube's privacy-enhanced `youtube-nocookie.com` embed, Twitch's player, or Kick's stream). YouTube and Twitch run in sandboxed frames with no access to the app.

The library (`library.sqlite`, with transcripts and chat), `settings.json` and `layers.json` (timeline layers) live in `~/.local/share/app.vodscribe` on Linux, `~/Library/Application Support/app.vodscribe` on macOS and `%APPDATA%\app.vodscribe` on Windows. Downloads (`models`, `bin`, `gpu`, `emotes`, `work`) go in the same folder, except on Windows, where they go in `%LOCALAPPDATA%\app.vodscribe` so they don't roam.

## Layout

- `crates/core`: the pipeline, with no UI. It covers fetching (`fetch.rs`, plus `kick.rs` for Kick's current VOD API, and `local.rs` for files on this computer), the queue's download and transcription stages (`pipeline.rs`), speech recognition (`asr.rs`), words and segments (`transcript.rs`), the SQLite FTS5 index (`db.rs`), model downloads (`model.rs`) and the optional GPU pack (`gpu.rs`). It also includes `vodscribe-cli` for testing without the app.
- `crates/worker`: `vodscribe-worker`, which transcribes one audio file in its own process and streams words back. It is built twice: statically linked for the CPU, and against sherpa-onnx's shared no-tts library for the GPU, which ships with the app (`apps/desktop/src-tauri/lib`, a bundle resource) while the GPU add-on supplies only ONNX Runtime with CUDA and NVIDIA's libraries. sherpa-onnx's CUDA release includes its full build with the GPL-3.0 espeak-ng, which is why that library isn't taken from it.
- `apps/desktop`: the desktop app. `src` is the SvelteKit (Svelte 5) UI, built statically into the app; `src-tauri` is the Tauri shell, which provides the commands the UI calls and runs the queue. Icons are inline SVGs in `src/lib/Icon.svelte`; the logo is `src/lib/Logo.svelte`.
- `tools/model-export`: exports and quantizes the speech models we ship, and publishes them. See its README.

## Development

Prerequisites: Rust (the version in `rust-toolchain.toml`, which rustup picks up), Node (`.nvmrc`) and pnpm, plus [Tauri's system dependencies](https://tauri.app/start/prerequisites/) and ffmpeg. `pnpm dev` runs a bash script, so on Windows it needs Git Bash on `PATH`; release builds on Linux also need `patchelf`.

The JavaScript side is a pnpm workspace (`apps/*`) and the Rust side a Cargo workspace (`crates/*` and `apps/desktop/src-tauri`), both rooted here. From the repo root:

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

Transcription runs in a separate `vodscribe-worker` process, bundled as a Tauri sidecar along with a CUDA build of it on Linux and Windows. `scripts/prepare-sidecars.sh` builds both into `apps/desktop/src-tauri/binaries/`. `pnpm dev` runs it automatically; run it yourself (`bash scripts/prepare-sidecars.sh [debug|release]`) before building the app with plain `cargo`.

Core tests and the CLI:

```sh
cargo test -p vodscribe-core
VODSCRIBE_DATA=./vodscribe-data cargo run --release -p vodscribe-core --bin vodscribe-cli -- setup
VODSCRIBE_DATA=./vodscribe-data cargo run --release -p vodscribe-core --bin vodscribe-cli -- add <url>
VODSCRIBE_DATA=./vodscribe-data cargo run --release -p vodscribe-core --bin vodscribe-cli -- add-file <path>
VODSCRIBE_DATA=./vodscribe-data cargo run --release -p vodscribe-core --bin vodscribe-cli -- search "some phrase"
```

The app icon is generated from `apps/desktop/src-tauri/icons/app-icon.svg`; after changing it, run `pnpm tauri icon src-tauri/icons/app-icon.svg -o src-tauri/icons` from `apps/desktop` and copy the 32 px PNG to `static/favicon.png`, and `icons/icon.ico` to `apps/site/static/favicon.ico` for the website. macOS icons follow Apple's grid (a smaller rounded square with a shadow), so `icon.icns` comes from `app-icon-macos.svg` instead: generate into a temporary directory with `pnpm tauri icon src-tauri/icons/app-icon-macos.svg -o /tmp/mac-icons` and copy just its `icon.icns` over. Keep the two SVGs' glyphs in sync.

The app's content security policy is in `tauri.conf.json` (`app.security.csp`). It only applies to built apps, not `pnpm dev`, so a new external script, frame or image host needs adding there or it will work in development and break in release.

`scripts/check-pinned-downloads.py` sends a HEAD request for every pinned download (tools, the GPU add-on, the models, the sidecar prebuilts) and fails if one is gone or a different size; the Pinned downloads workflow runs it weekly, since a vanished build would otherwise break setup for new users silently.

`scripts/reset-setup.sh` (Linux and macOS) removes the speech model and yt-dlp so the first-run setup can be tested again; the library, settings and GPU add-on stay.

## Website

`apps/site` is vodscribe.com: a landing page whose live demo is the app itself. It imports the desktop app's components as the workspace package `desktop` (`desktop/lib/…`, exported from `apps/desktop/package.json`), and `vite.config.ts` swaps the Tauri modules they call for stand-ins (`apps/site/src/lib/tauri/`) that answer from transcripts in `static/demo/`, with the same phrase search as the app. With `__DEMO__` defined, controls that only work in the app gray out and explain themselves on hover (`apps/desktop/src/lib/demo.ts`); mark new ones with `demoOff`.

The demo's VODs are transcribed with the CLI into a fresh library, then exported:

```sh
export VODSCRIBE_DATA=/tmp/demo
vodscribe-cli add <url>          # for each VOD; `setup` first, or link bin/ and models/ from an existing library
python3 apps/site/scripts/export-demo.py /tmp/demo/library.sqlite
```

`pnpm --filter site build` prerenders every page into `apps/site/build`, with a content security policy that SvelteKit writes into each page and response headers in `static/_headers`. The download buttons link to `dl.vodscribe.com/releases/latest/` (`src/lib/downloads.ts`), so each release has to be uploaded there under those stable names; the files are served with a `Content-Disposition` that names the versioned file, so what's saved says which version it is.

It deploys from this repo through Cloudflare's Git integration: build command `pnpm install --frozen-lockfile && pnpm --filter site build`, with `apps/site/build` as the output directory on Pages. On Workers, `apps/site/wrangler.jsonc` sets the same directory and serves `404.html` for missing pages. The production site builds from the `site` branch, which each release moves to its tag: the demo is the app's own code, so built from `main` it would show what hasn't been released. Changes to the site alone between releases go on `site` too (or are merged into it); every other branch gets a preview.

## Building

`pnpm tauri build` in `apps/desktop` (after `prepare-sidecars.sh release`) produces the installers for the current platform. The GitHub Actions workflow in `.github/workflows/build.yml` builds Linux, Windows and macOS (Intel and Apple Silicon) on a manual run or a `v*` tag, and uploads the bundles as artifacts. macOS bundles are ad-hoc signed; nothing is signed with a paid certificate yet.

## License

VODscribe is source-available under the [Functional Source License, Version 1.1, ALv2 Future License](LICENSE.md): use, read, change and redistribute it for anything except a competing product, and each version becomes Apache 2.0 two years after its release. The third-party software it's built on keeps its own licenses (`THIRD-PARTY-NOTICES.txt`, and the website's licenses page). [CONTRIBUTING.md](CONTRIBUTING.md) covers changes and [SECURITY.md](SECURITY.md) vulnerability reports.

## Releasing

Between releases, `version` in the root `Cargo.toml` is the next one in development, `0.4.1-dev`: development builds don't pass for the last release, and semver orders it after 0.4.0 and before 0.4.1, so the updater neither offers 0.4.0 to them nor holds back 0.4.1. (An MSI's version is numbers only, so builds of a development version give theirs as `0.4.1`.)

1. Write the release's entry in `CHANGELOG.md` as you go (the heading says "(unreleased)" until then; CI checks that the version in `Cargo.toml` has one).
2. Set `version` to the release's (drop `-dev`), date the changelog heading, and commit ("Version 0.4.1", with the entry in its message).
3. `python3 scripts/ready-to-release.py` checks it's ready, then tag it (`git tag v0.4.1`) and push the tag. The entry becomes the release notes in `latest.json`; the tag build fails if it's missing or still marked unreleased.
4. Once it's out, move `site` to the tag (`git push origin v0.4.1^{commit}:refs/heads/site`), and set `main`'s version to the next in development (`0.4.2-dev`) in a commit of its own, which also opens its changelog entry (`## 0.4.2 (unreleased)`: CI's check takes a development version's entry to be that one). `ready-to-release.py` prints both.

The tag's build signs the update bundles (`TAURI_SIGNING_PRIVATE_KEY`), then `scripts/release.py` names every file (`VODscribe-<version>-windows-x64-setup.exe` and so on, plus the stable names for the website) and writes `latest.json`, and the publish job uploads them to R2: `releases/<version>/` for the updater and winget, `releases/latest/` for the website's buttons, and `releases/latest.json` last. The app checks that manifest at launch and daily, and installs an update only when the user says so, and only if it's signed with the key whose public half is in `tauri.conf.json`. Keep the private key (`~/.tauri/vodscribe-updater.key`) backed up: without it, installed copies can't be updated.

The winget job then submits the new version to `microsoft/winget-pkgs` as `Airodonack.VODscribe`, once `WINGET_TOKEN` (a classic token of the Airodonack GitHub account, with `public_repo`) is set. Setting the repository variable `WINGET_SUBMIT` to `false` skips it, for while a version waits in winget's review: a newer PR would supersede that one and start the review over. The workflow's header lists every secret and variable.

Speech models are served from Cloudflare R2 at `https://dl.vodscribe.com/models/<model id>/`, pinned by size and SHA-256 in `crates/core/src/model.rs`. `VODSCRIBE_MODELS_URL` overrides the base URL in debug builds, for testing.
