# The desktop app

VODscribe itself: a SvelteKit page, built statically, in a Tauri 2 shell. The work is done by the crates in `crates/`.

- [`src`](src): the page.
- [`src-tauri`](src-tauri): the Rust shell, its Tauri settings, icons and installer pieces.
- `static/`: the favicon, served beside the page.
- `package.json`: the `desktop` package, which exports `lib/*` and `app.css` for the website.
- `vite.config.js`: Vite and SvelteKit's settings, with the static adapter and the `color-mix()` fallbacks.
- `colorMix.js`: writes a fallback for each `color-mix()` rule, for web views without it such as macOS 12's.
- `colorMix.test.mjs`: its tests.
- `tsconfig.json`, `.prettierrc`: TypeScript's and Prettier's settings.

## From a link to a clip

1. A pasted link goes to `vods.rs`, which reads the VOD's details with yt-dlp, adds it to the library, and for a livestream has `chats.rs` fetch its chat replay.
2. A file is added in place, and never copied, moved or deleted.
3. Transcribe asks for the whole VOD or a stretch, and the queue in `worker.rs` downloads its audio in one stage and runs `vodscribe-worker` on it in another, writing words to the library as they come.
4. A search goes to the core's `search.rs`, and a result plays in YouTube's or Twitch's own player, from Kick's stream, or in `crates/player` for everything on this computer.
5. A clip is cut to the frame with ffmpeg, from the VOD's file or a section downloaded for it, and an edit is rendered from the same footage.
