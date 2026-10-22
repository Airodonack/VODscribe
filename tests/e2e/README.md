# End-to-end tests

These tests drive a built VODscribe through its page, as a person would: a library made, a file added and transcribed, clips cut, edits rendered, settings changed, the app restarted. Each scenario in `scenarios/` starts the app on an empty library and says in plain words at its top what it checks. Helpers are in `lib/`: `app.mjs` drives the page, `os.mjs` takes screenshots and real clicks from the operating system, `drivers.mjs` starts the driver.

## The three runners

- **`run.mjs`** drives one app binary on this computer, through WebDriver on Linux (tauri-driver with WebKitWebDriver) or, on Windows and macOS, through the app's own test hook in a build made with the `e2e` feature (`--driver bridge`). The other two runners end up calling it.
- **`distros.mjs`** (Linux only) runs the suite inside a podman container per distribution, its "cell": our .deb or .rpm installed with that distribution's package manager, then `run.mjs` under a headless display. `--only debian-13,fedora-44` picks distributions (the list is `DISTROS` at the top of the file). `--env weston` or `--env gnome` runs under a headless Wayland compositor instead of Xvfb with openbox; `--scheme dark` in the dark colour scheme; `--gpu nvidia` with the host's NVIDIA GPU, `--gpu dri` with the host's `/dev/dri` and the distribution's Mesa (Intel and AMD).
- **`runners.mjs`** runs the suite on another computer over SSH (a "runner"): it copies the harness and the app there, runs `run.mjs`, and brings the results back. Runners are listed in `runners.json`, which isn't in the repository: copy `runners.example.json` and fill in each one's name, SSH host, operating system (`windows` or `macos`), a folder to work in, and for Windows the folders to put first on `PATH`. Someone must be signed in on the runner, and its app data is emptied before each scenario, so use a computer kept for testing.

`contact-sheet.mjs` makes a page of the checkpoints a run took (`ctx.checkpoint`), one column per run, for looking at what the tests can't judge.

## What they need

- **Node 24** (the version in `.nvmrc`), through mise or any other way. The harness imports a TypeScript file from the app, which Node 24 runs as it is.
- **tauri-driver**, from cargo: `cargo install tauri-driver --locked`. It must be on `PATH`, or named by `TAURI_DRIVER`. `distros.mjs` mounts this binary into each container.
- **podman**, for `distros.mjs`. The containers pull public images (Debian, Ubuntu, Fedora, Arch and others) and install packages, so the first run of each needs the network.
- For `run.mjs` directly on Linux: WebKitWebDriver (`webkit2gtk-driver` on Debian and Ubuntu, `webkitgtk6.0` on Fedora), and for a headless run Xvfb, openbox, xdotool and ffmpeg.
- For `runners.mjs`: SSH access to each runner by key (it runs in batch mode), and Node on the runner.

### Building the packages

`distros.mjs` takes a folder holding the .deb and the .rpm; it finds them anywhere under it, so the layout is usually `deb/` and `rpm/` side by side, as Tauri writes them. A fresh checkout can't build the app yet: three folders under `apps/desktop/src-tauri` are ignored by Git and made by the build steps.

- `binaries/` holds the transcription workers, which Tauri bundles as sidecars, and `lib/` the sherpa-onnx library the GPU worker links. `scripts/prepare-sidecars.sh release` makes both: it downloads sherpa-onnx's prebuilt libraries, checks them against the hashes pinned in `scripts/sherpa-onnx-prebuilt.sha256`, and builds the workers. It needs `patchelf` for a release build.
- `gen/` holds the capability schemas, which the Rust build writes the first time it runs.

From the repository root, with Rust, pnpm and Tauri's system dependencies installed (CI installs `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libasound2-dev patchelf` on Ubuntu; see `.github/workflows/build.yml`):

```sh
pnpm install
scripts/prepare-sidecars.sh release
pnpm --filter desktop tauri build --bundles deb,rpm
```

The packages land in `target/release/bundle/` (under `CARGO_TARGET_DIR` if that is set), with `deb/` and `rpm/` inside, which `distros.mjs` takes as its folder. For Windows and macOS runners, build the app with the `e2e` feature as `build.yml` does: `--features e2e --no-bundle` on Windows (the installer and this `vodscribe.exe` both go to `runners.mjs`, with `--test-app` for the second), `--features e2e --bundles app` on macOS.

## Running

One scenario, on one distribution:

```sh
node tests/e2e/distros.mjs target/release/bundle --only debian-13 -- --only edits
```

Everything after `--` goes to `run.mjs`. The same scenario under Wayland, then in the dark scheme with the GPU:

```sh
node tests/e2e/distros.mjs target/release/bundle --only debian-13 --env weston -- --only edits
node tests/e2e/distros.mjs target/release/bundle --only fedora-44 --scheme dark --gpu dri -- --only site-plain
```

Directly against an installed app, headless, on a library of its own:

```sh
export XDG_DATA_HOME=$HOME/.cache/vodscribe-e2e-data E2E_RESET_DATA=1 VODSCRIBE_NO_UPDATE_CHECK=1
unset WAYLAND_DISPLAY; export GDK_BACKEND=x11
export XDG_RUNTIME_DIR=$(mktemp -d); chmod 700 "$XDG_RUNTIME_DIR"
dbus-run-session -- xvfb-run -a -s '-screen 0 1400x900x24' sh -c 'openbox & sleep 1; node tests/e2e/run.mjs /usr/bin/vodscribe --only edits'
```

The private runtime dir matters: with the shared one, the test bus's gnome-keyring finds your session's keyring there and quits without claiming its name, and GTK's portal start then waits out D-Bus's 25 s activation timeout before the window appears.

On another computer: `node tests/e2e/runners.mjs <runner> <app> [--test-app <exe>] --only edits`.

The `setup` scenario always runs first when it's chosen, and downloads the tools and the speech model the others keep. Without them, whichever scenario runs first goes through setup and downloads them; with it, they're copied in and the run starts at once. Quit any VODscribe you have open first: the run refuses to start beside one.

## Environment

- `E2E_OUT`: where `run.mjs` writes its results (default `tests/e2e/out`).
- `E2E_RESET_DATA`: `1` lets a scenario empty a data folder that already has a library. Without it, the run refuses rather than delete one; point `XDG_DATA_HOME` at a folder of its own.
- `E2E_NETWORK`: `1` runs the scenarios that reach Twitch, YouTube or Kick; without it they're skipped.
- `E2E_SITE`: which site's VOD the site scenarios play (`twitch`, the default, or `youtube`).
- `E2E_SHOTS`: set, `site-embed` keeps a screenshot of the whole screen at each step.
- `E2E_TRACE`: `1` logs each call into the page and how long it took.
- `E2E_DRIVER_PORT`: tauri-driver's port (default 4444), for two runs side by side on one computer.
- `E2E_SCHEME`: the colour scheme the run is in (`light` or `dark`), which the site scenarios check; `distros.mjs` sets it from `--scheme`.
- `E2E_DISPLAY`: the Wayland compositor the run is under (`weston` or `gnome`), so screenshots know how to be taken; `distros.mjs` sets it from `--env`.
- `E2E_SEED`: for `distros.mjs`, a data folder whose `bin/` and `models/` each cell starts with (default none, so setup downloads them).
- `E2E_WORK`: for `distros.mjs`, where each cell's data folder goes (default `tests/e2e/out/distros`).
- `E2E_PODMAN`: the podman command for `distros.mjs` (default `podman`; a wrapper for another store, for example).
- `TAURI_DRIVER`: the tauri-driver binary, if it isn't on `PATH`.
- `NATIVE_DRIVER`: the platform's WebDriver, passed to tauri-driver, if it isn't where tauri-driver looks.
- `XDG_DATA_HOME`: on Linux, where the app keeps its library, and so what the tests empty.
- `VODSCRIBE_NO_UPDATE_CHECK`: read by the app, not the harness: set, it doesn't look for updates.

## Results

`run.mjs` writes `results.json` (each scenario, passed, failed or skipped, and why) to `E2E_OUT`. For a failure it adds the page's text and errors (`<scenario>.txt`) and a screenshot (`<scenario>.png`); checkpoints go in `checkpoints/`. `distros.mjs` keeps each cell's results in `tests/e2e/out/distros/<cell>/`, named for the distribution and what differs from the default (`debian-13-weston-dark`), with the whole log in `run.log`, and prints one line per cell at the end. `runners.mjs` brings a runner's back to `tests/e2e/out/<runner>/`. Two runs of the same cell from the same checkout wipe each other's folders.

## Known limits

- `distros.mjs` is Linux only: it needs podman and Linux containers.
- ubuntu-22.04 needs a .deb built on Ubuntu 22.04, such as CI's artifact. One built on a newer system links a newer glibc, and the cell stops at "missing libraries".
- `--gpu nvidia` needs nvidia-container-toolkit, with a CDI spec generated for podman (`nvidia-ctk cdi generate`).
- `--gpu dri` needs a podman whose runtime is crun, to keep the user's video and render groups in the container. On a GPU Mesa has no driver for, it draws with Mesa's software fallback.
- Under `--env weston` there's no seat, so the window never has the keyboard and the operating system's clicks aren't available; the scenarios and steps that need them are skipped there.
- The drag-out scenario's Alt-drag step is skipped under window managers that move windows on Alt-drag.
