# The Tauri shell

The Rust around the page: the commands it calls, the queues that do the work, the player's window surface, and Tauri's settings for building and bundling.

- [`src`](src): the shell's code.
- `Cargo.toml`: the `vodscribe` crate, with two features never in a release: `e2e`, the tests' way in, and `embed-takeover`, a site's video drawn in the app's player.
- `build.rs`: declares the app's commands to Tauri, writing `permissions/app.toml`, and embeds the Windows app manifest.
- `tauri.conf.json`: the app's Tauri settings, with the content security policy and the updater's public key.
- `tauri.linux.conf.json`, `tauri.windows.conf.json`: each system's bundles, sidecars and resources.
- [`capabilities`](capabilities): what the main window may call.
- [`permissions`](permissions): the app's own commands as a permission set.
- [`icons`](icons): the app icon in every size.
- [`linux`](linux): the desktop entry for the .deb and .rpm.
- [`windows`](windows): the Windows installer's template.
- `Info.plist`: added to the macOS app's Info.plist; it words the local network prompt.
- `windows-app-manifest.xml`: the Windows app manifest.
- `THIRD-PARTY-NOTICES.txt`: the licence of everything compiled into the app, made by `scripts/third-party-notices.py`.
- `.gitignore`: the schemas Tauri writes into `gen/`.

The build also needs `binaries/`, the transcription workers, and `lib/`, the sherpa-onnx library the GPU worker links. `scripts/prepare-sidecars.sh` makes both; Git ignores them.

The content security policy applies only to built apps, not to `pnpm dev`. Add a new external script, frame or image host there, or it works in development and breaks in a release.
