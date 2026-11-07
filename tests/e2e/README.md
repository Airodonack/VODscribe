# End-to-end tests

These tests drive a built VODscribe through its page, as a person would: a file added and transcribed, clips cut, edits rendered, settings changed, the app restarted. Each scenario starts the app on an empty library.

- `run.mjs`: runs the scenarios against one app on this computer, through WebDriver on Linux or the app's own test hook on Windows and macOS.
- `distros.mjs`: runs them on Linux distributions, each in a podman container with our package installed.
- `runners.mjs`: runs them on another computer over SSH, a "runner", and brings the results back.
- `runners.example.json`: the shape of `runners.json`, the list of runners, which stays out of the repository.
- `merge-results.mjs`: merges shards' results into one summary.
- `durations.mjs`, `durations.json`: each scenario's last passing time, which shards are dealt by.
- `flaky.json`: the flake ledger.
- `budgets.json`: the `perf-*` scenarios' budgets.
- `perf-compare.mjs`: compares two runs' timings, or judges one against the budgets.
- `contact-sheet.mjs`: a page of a run's checkpoints, one column per run.
- `verify.mjs`: has a model judge the checkpoints' pictures.
- `profile-setup.sh`: readies a Debian system for profiled runs.
- `*.test.mjs`: unit tests of the harness, run with `node --test`.
- `.gitignore`: results, `runners.json`, and SQLite's side files.
- [`scenarios`](scenarios): the scenarios.
- [`lib`](lib): helpers; `app.mjs` drives the page, `os.mjs` the OS's screenshots and clicks, `drivers.mjs` the driver.
- [`fixtures`](fixtures): recorded test files.
- [`gnome-shell`](gnome-shell): an extension for headless GNOME Shell runs.

## What they need

- **Node 24**, the version in `.nvmrc`, which runs the app's TypeScript file the harness imports.
- **tauri-driver**: `cargo install tauri-driver --locked`, on `PATH` or named by `TAURI_DRIVER`; `distros.mjs` mounts it into each container.
- **podman**, for `distros.mjs`, with the network for each distribution's first run.
- **For `run.mjs` on Linux**: WebKitWebDriver (`webkit2gtk-driver` on Debian and Ubuntu, `webkitgtk-webdriver` from Ubuntu 26.04, `webkitgtk6.0` on Fedora); Xvfb, openbox, xdotool and ffmpeg for a headless run; Nautilus, or Dolphin under KDE, for the scenarios that drop a clip on a file manager; `prlimit` and `file` to keep and name crash cores.
- **For `runners.mjs`**: SSH access to each runner by key, and Node on the runner.

### Building the packages

A fresh checkout needs three folders under `apps/desktop/src-tauri` that Git ignores. `binaries/` holds the transcription workers and `lib/` the sherpa-onnx library the GPU worker links; `scripts/prepare-sidecars.sh release` makes both, checking the libraries against `scripts/sherpa-onnx-prebuilt.sha256`, and needs `patchelf`. `gen/` is written by the first Rust build.

With Rust, pnpm and Tauri's system dependencies installed (CI's list for Ubuntu is in `.github/workflows/build.yml`):

```sh
pnpm install
scripts/prepare-sidecars.sh release
pnpm --filter desktop tauri build --bundles deb,rpm
```

The packages land in `deb/` and `rpm/` under `target/release/bundle/`, or under `CARGO_TARGET_DIR`; `distros.mjs` takes that folder. For runners, build with the `e2e` feature as `build.yml` does: on Windows `--features e2e --no-bundle`, whose `vodscribe.exe` goes to `runners.mjs` with `--test-app` beside the installer; on macOS `--features e2e --bundles app`.

## Running

One scenario on one distribution, then under Wayland, then dark with the host's GPU. Everything after `--` goes to `run.mjs`:

```sh
node tests/e2e/distros.mjs target/release/bundle --only debian-13 -- --only edits
node tests/e2e/distros.mjs target/release/bundle --only debian-13 --env weston -- --only edits
node tests/e2e/distros.mjs target/release/bundle --only fedora-44 --scheme dark --gpu dri -- --only site-plain
```

`DISTROS` in `distros.mjs` lists the distributions. `--env weston` or `--env gnome` runs under a headless Wayland compositor instead of Xvfb with openbox. `--gpu nvidia` passes the host's NVIDIA GPU, and `--gpu dri` its render nodes with the distribution's Mesa.

Directly against an installed app, headless, on a library of its own:

```sh
export XDG_DATA_HOME=$HOME/.cache/vodscribe-e2e-data E2E_RESET_DATA=1 VODSCRIBE_NO_UPDATE_CHECK=1
unset WAYLAND_DISPLAY; export GDK_BACKEND=x11
export XDG_RUNTIME_DIR=$(mktemp -d); chmod 700 "$XDG_RUNTIME_DIR"
dbus-run-session -- xvfb-run -a -s '-screen 0 1920x1080x24' sh -c 'openbox & sleep 1; node tests/e2e/run.mjs /usr/bin/vodscribe --only edits'
```

Keep the private `XDG_RUNTIME_DIR`: with the shared one, the test session's gnome-keyring quits, and the window waits 25 s for GTK's portal before it appears.

On a runner: `node tests/e2e/runners.mjs <runner> <app> [--test-app <exe>] --only edits`. Copy `runners.example.json` to `runners.json` and give each runner its name, SSH host, `windows` or `macos`, a folder to work in, and on Windows the folders to put first on `PATH`. Someone must be signed in on the runner, and its app data is emptied before each scenario, so use a computer kept for testing. The Windows machine takes one run at a time.

The `setup` scenario always runs first when chosen, and downloads the tools and speech model the others keep. Without it, the first scenario goes through setup. Quit any VODscribe you have open: the run refuses to start beside one.

The screen is 1920 by 1080, as in the cells, so the drag scenarios have room for a file manager beside the app's window; on a smaller screen, or with a Nautilus older than 43, they skip. On Linux, drop-virtual drags a video out of a zip in an archive manager (File Roller before 43, Engrampa or Ark), or out of a stand-in where there's none. On the Windows machine, drag-promise drops on File Explorer and on PowerShell forms beside the app, and drop-virtual drags a video out of a zip folder onto the window. On the Mac, drag-promise drops on Finder in icon view and on an app built with swiftc that takes finished files only, and drop-virtual drags a file promise from another such app onto the window.

The app's short mode, the player alone in a short window, is off in every scenario but those that ask for it with `app.shortMode(true)`.

### Crash cores

On Linux the harness lifts tauri-driver's core limit with `prlimit` and starts it in `E2E_OUT`, so a crash leaves `E2E_OUT/core.<pid>`. The scenario's result line names the file and its binary. To read one, with the binary that crashed:

```sh
gdb -batch -ex bt /usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/WebKitWebProcess tests/e2e/out/core.<pid>
```

A system that pipes cores to systemd-coredump or apport keeps them there; `coredumpctl` lists them.

### Several at once, and shards

`--parallel N` runs N apps at once on Linux, each in a worker with its own D-Bus session, X display, driver port after `E2E_DRIVER_PORT`, and data folders: the `XDG_DATA_HOME`, `XDG_CONFIG_HOME` and `XDG_CACHE_HOME` given, with `-1`, `-2`, … added. Each worker starts with the configured data folder's tools and model, hard-linked, so run setup there first. Output is marked `[w1]`, `[w2]`, …, and results merge into `E2E_OUT` in scenario order. It needs `XDG_DATA_HOME` set, and doesn't combine with `--profile`.

```sh
node tests/e2e/run.mjs /usr/bin/vodscribe --parallel 3
```

Scenarios are dealt longest first, each to the worker with the least so far, by `durations.json`. Refresh it with `node tests/e2e/durations.mjs <out dir>...` when scenarios are added or change a lot; a new one goes last. `--shard i/N` runs the i-th of N such deals, the same on every computer, and combines with `--parallel`. `distros.mjs --parallel N` runs a cell that way; `runners.mjs` passes `--shard` on.

### In CI

`e2e.yml` runs the suite in shards with `run.mjs --shard i/N`: 2 on Ubuntu, 2 on Windows and 3 on macOS, the slowest. Each shard uploads `tests/e2e/out` as `e2e-<target>-<i>`, and a last job runs:

```sh
node tests/e2e/merge-results.mjs --expect <shards> <folder of the artifacts' folders>
```

It prints one summary in `run.mjs`'s words, with a line per shard and per failure or flaky pass, and exits 1 on a failed scenario or when fewer shards reported than `--expect`. The fast lane runs the Ubuntu shards only; the full lane every system's. `gh workflow run e2e.yml -f build_run=<run id>` tests an earlier build's installers, with `-f lane=fast` for the Ubuntu shards only.

### The flake ledger

`flaky.json` names scenarios known to fail now and then for reasons outside what they check: `{ "<scenario>": "<what happened, where, when>" }`. A listed scenario that fails runs once more. If it passes then, it's reported `flaky`, counted apart from the passes, with `attempts: 2`, `flaky: true` and `firstError` in `results.json`, and its first page and screenshot kept as `<scenario>.first.txt` and `.png`. A scenario goes in once its failure is found to be the computer's or the runner's, and comes out when the cause is fixed or a few rounds pass without it.

## Environment

- `E2E_OUT`: where `run.mjs` writes its results; default `tests/e2e/out`.
- `E2E_RESET_DATA`: `1` lets a scenario empty a data folder that already holds a library; without it the run refuses.
- `E2E_NETWORK`: `1` runs the scenarios that reach Twitch, YouTube or Kick.
- `E2E_SITE`: the site the site scenarios play, `twitch` by default, or `youtube` or `kick`.
- `E2E_SHOTS`: set, `site-embed` keeps a whole-screen screenshot at each step.
- `E2E_TRACE`: `1` logs each call into the page and its time.
- `E2E_DRIVER_PORT`: tauri-driver's port, default 4444; give two runs side by side different ones.
- `E2E_WORKER`: set by `--parallel` in each worker.
- `E2E_SCREEN`: `hosted-mac` sizes the page as CI's hosted Mac's 1024 by 768 screen does, at most 613 px tall at open and 581 px after a resize.
- `E2E_SCHEME`: the run's colour scheme, `light` or `dark`; `distros.mjs` sets it from `--scheme`.
- `E2E_FILE_MANAGER`: `dolphin` or `nautilus` for the drag scenarios on Linux; unset, Dolphin in a KDE session, else Nautilus, and always Finder on the Mac.
- `E2E_DISPLAY`: the Wayland compositor, `weston` or `gnome`; `distros.mjs` sets it from `--env`.
- `E2E_SEED`: for `distros.mjs`, a data folder whose `bin/` and `models/` each cell starts with.
- `E2E_WORK`: for `distros.mjs`, where each cell's data folder goes; default `tests/e2e/out/distros`.
- `E2E_PODMAN`: the podman command for `distros.mjs`.
- `E2E_BUDGET_SCALE`: multiplies every budget, for a slower computer.
- `E2E_COMMIT`: what `perf.json` names the build by; default `git describe`.
- `TAURI_DRIVER`: the tauri-driver binary, if not on `PATH`.
- `NATIVE_DRIVER`: the platform's WebDriver, if not where tauri-driver looks.
- `XDG_DATA_HOME`: on Linux, where the app keeps its library, and so what the tests empty.
- `VODSCRIBE_NO_UPDATE_CHECK`: read by the app; set, it doesn't look for updates.
- `PROFILE_CG`: with `--profile`, `fp` for frame pointers, the default, or `dwarf`, deeper and about 60 times the disk.
- `PROFILE_HZ`: with `--profile`, samples a second; default 999.
- `PERF`: the perf command, if not `perf` on `PATH`.

## Results

`run.mjs` writes `results.json` to `E2E_OUT`, each scenario passed, failed, skipped or flaky and why, with its `timings` and, on Linux, `memory`. It ends with a summary: how many passed, a line per scenario, the ledger's size, and the wall time. A failure leaves `<scenario>.txt`, the page dump from `lib/dump.mjs`, and `<scenario>.png`; checkpoints go in `checkpoints/`. Every run also writes `perf.json`: each scenario's timed steps, the app's memory at its end, and what the run was on.

`distros.mjs` keeps each cell's results in `tests/e2e/out/distros/<cell>/`, named like `debian-13-weston-dark`, with the whole log in `run.log`. Two runs of the same cell from one checkout wipe each other. `runners.mjs` brings a runner's back to `tests/e2e/out/<runner>/`, also with the whole log in `run.log`.

## Judging the pictures

`verify.mjs` has a model look at the checkpoints as a person would, for what no check can judge: a picture cut off, text over text, a dark theme half applied. A scenario says what each should show beside its `run`:

```js
expects: {
  "search-or": 'Under the search box, which reads "zebra OR landed", the query is shown as two word chips, ...',
},
```

Write what a person should see, in one or two sentences, and nothing a check already asserts. Name what's there and where, so a wrong picture fails.

```sh
OPENROUTER_API_KEY=... E2E_VERIFY_MODEL='vendor/family*' node tests/e2e/verify.mjs tests/e2e/out
```

`--model` or `E2E_VERIFY_MODEL` is a model id, or a prefix ending in `*` for the cheapest model with that prefix that takes pictures. `--only a,b` keeps those scenarios, `--concurrency N` caps the requests at once, default 4, and `--out` names the verdict sheet, default `verify.html` with `verify.json` beside it. Requests are retried on 429 and 5xx for up to 60 s, and `--dry-run` prints the prompts and calls nothing. The last line is `verify: P pass, F fail, U unsure, N without expectation; ~T tokens`, and it exits 1 on any fail.

Without a key, a person or a model that reads image files judges them instead:

```sh
node tests/e2e/verify.mjs tests/e2e/out --manifest verify-manifest.json   # [--only a,b]
# read each picture the manifest lists, and write verdicts.jsonl, one line each:
# {"file": "05-legend-legend-vod.png", "verdict": "pass", "reason": "...", "concerns": []}
node tests/e2e/verify.mjs tests/e2e/out --verdicts verdicts.jsonl         # [--only a,b]
```

`file` is a picture's name or absolute path. A picture with no line counts as unsure; a line that isn't a verdict stops the run with exit 2. Verifying costs money per run, so it belongs in the full lane only; `e2e.yml`'s step is commented out until `OPENROUTER_API_KEY` is a repository secret.

## A chat without Twitch

`chat()` in `lib/fixtures.mjs` is a synthetic two-hour Twitch chat of about 3,000 messages, the same on every run. It has a streamer, `CHANNEL`, named chatters, `CHATTERS`, one with a space in the name, three bursts at 1200, 3600 and 5400 s, subs, gifts and tips as Twitch writes them, `@name` mentions, and the streamer saying "landed". It returns the facts to check against: `by(name)`, `withWord(word)`, `between(from, to)` and `events`.

The e2e build serves `<data>/e2e-chat.json` as Twitch's comment pages. `serveChat(fixture, failAfter)` writes it, and `fetchChat(app, ctx, id, fixture)` makes a VOD the chat's Twitch VOD, restarts the app and returns it once the chat is whole.

The seam carries only a message's time, author and text: no emote images, no colours, and no kind or amount but what the app reads from the text. A Twitch VOD has no player offline, so "the part in view" is where the minimap scrolled the transcript.

## Timings and profiling

A performance pass starts from measurements: the scenarios time what a person waits on, and a profiled run says where the time went.

### Timings

| Step                                                            | Where                                                                                              |
| --------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| start to first screen                                           | every scenario; a start that needed a page reload is named "(reloaded)"                            |
| open to first frame                                             | `player-picture`: opening the VOD and Play, to the app's own player showing a frame                |
| seek to frame shown                                             | `player-picture`: a seek, to the frame there showing                                               |
| play to the picture moving                                      | `player-open`: Play on a file opened paused, to the next frame                                     |
| search, many lines / one line a VOD / none                      | `search-large`: on 60 VODs of two hours' talk, from the words typed to the results drawn           |
| clip made (2 s) / whole video's clip made (5 s) / edit rendered | the scenarios that cut the 1 to 3 s clip / `export-whole` / `edits`: from the key to the file made |
| transcribed (the file's name)                                   | every scenario that adds and transcribes a file: the worker's time on it                           |
| memory at the end                                               | every scenario, on Linux: the app's processes' proportional set size                               |

A scenario times a step with `ctx.time` and `ctx.timing`. `--repeat N` runs the chosen scenarios N times, and `perf.json` keeps each step's median, least and most. `node tests/e2e/perf-compare.mjs <before perf.json> <after perf.json>` marks a step that moved clear of the other run's spread by more than 10% (`--threshold`), and exits 1 when one got worse. Time the release build, unprofiled.

### Budgets

The `perf-*` scenarios take each measure several times and fail when a median is over its budget in `budgets.json`, saying the numbers.

| Scenario       | Steps, each taken five times; the scroll three                                                                                   |
| -------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `perf-player`  | open to first frame, five copies of the colour video in turn; a timeline click to the bar's time changing and to the frame there |
| `perf-library` | on 60 VODs: searches typed, to their results drawn; one transcript of 1,800 lines scrolled, its 95th percentile frame gap        |
| `perf-page`    | Settings opened, to the frame it's drawn in; a VOD with twenty clips opened, to all twenty pills drawn                           |

`budgets.json` has an entry per platform, `linux`, `darwin` and `win32`, only Linux's so far, with the computer and build they were set on. A budget is 1.5 times the slowest of three runs' medians, rounded up to 10 ms, and at least that median plus 34 ms. A debug build with `--features e2e` is within them too. Run them serially: under `--parallel` the numbers are logged, not judged. `perf-compare.mjs <perf.json> --budgets tests/e2e/budgets.json` judges a whole run's file the same way, with `--scale` for a slower computer.

### Profiling

On Linux, `run.mjs --profile <dir>` samples each scenario's app with perf, and every process it starts, through `lib/profile.sh`. Into `<dir>/<scenario>/` go perf's recordings, `<scenario>-<n>.data`, one per start of the app; `folded.txt`, every stack for a flame graph; `top.txt`; and `page.json`, the page's own measures.

- **The profiling build** has our code's names: `scripts/build-profiling.sh`, after `prepare-sidecars.sh release`, makes `target/profiling/vodscribe` with the worker beside it, run directly: `node tests/e2e/run.mjs <target>/profiling/vodscribe --only player-picture --profile out/profile`.
- **perf and symbols**: `tests/e2e/profile-setup.sh`, on Debian with sudo, installs perf, binutils, libc's debug symbols, and the debug symbols of the installed WebKitGTK from snapshot.debian.org; perf needs `kernel.perf_event_paranoid` at 1 or less, and a profile is read where it was recorded.
- **In a cell**: `distros.mjs --profile` installs perf there and lets it sample, into the cell's `perf/`; not in Ubuntu's cells, whose perf must match the host's kernel.

`top.txt` puts each sample down to an owner by its innermost frame: **ours**, our crates; **crates**, the Rust libraries we link; **engine**, WebKit and its JavaScript; **graphics**, wgpu, Mesa and software rendering; **media**, ffmpeg; **transcription**, the worker; **toolkit**, GTK, GLib and fonts; **system**, the kernel and libc. It shows the share by process and thread, by owner, our functions by their own time with their callers, everyone's functions, each timed step's samples, and the page's slow input events and animation frame gaps over 50 ms.

A **finding** is time in our code that needn't be spent, engine time our page causes, work done every frame that could be done on change, the same work twice, main-thread work that could be elsewhere, or a timing worse than the last release's. It names the scenario, step, stack and share. It is **confirmed** by a before-and-after on the same scenario, machine and build, `--repeat 3` and `perf-compare.mjs`, with a profile showing the samples gone.

### Every release

On the computer and container the last release was measured on:

1. `scripts/build-profiling.sh`, and the release build.
2. The profiling build, profiled once: `--only player-picture,player-seek,transcribe-search-play,search,search-large,clip,edits,export-whole --profile <dir>`.
3. The release build, timed: the same scenarios, `--repeat 3`.
4. `perf-compare.mjs` against the last release's `perf.json`, the results kept with the release's.

A scenario profiled at 999 samples a second with frame pointers has run no slower so far, and takes about a megabyte; with DWARF, about 40 MB for ten seconds.

### What it doesn't measure

- **The GPU.** Headless, wgpu and WebKit draw on the CPU; profile with `distros.mjs --gpu` before calling graphics time a finding.
- **The Windows machine and the Mac.** Timings and memory work there; profiling would need the platform's profiler, `wpr` or `xctrace`.
- **The sites' players.** Only the app around them is measured.
- **Our page's JavaScript by name.** perf sees it as `[JIT code]`, and `JSC_useJITDump=1` makes WebKitGTK 2.54 abort at start.

## Known limits

- `distros.mjs` is Linux only.
- ubuntu-22.04 needs a .deb built on Ubuntu 22.04, such as CI's; a newer one stops at "missing libraries".
- `--gpu nvidia` needs nvidia-container-toolkit and a CDI spec, `nvidia-ctk cdi generate`.
- `--gpu dri` needs podman with crun; on a GPU without a Mesa driver it draws in software.
- Under `--env weston` the window never has the keyboard and the OS's clicks aren't available, so the scenarios that need them skip.
- drag-out's Alt-drag step skips under window managers that move windows on Alt-drag.
