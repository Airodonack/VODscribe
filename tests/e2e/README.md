# End-to-end tests

These tests drive a built VODscribe through its page, as a person would: a library made, a file added and transcribed, clips cut, edits rendered, settings changed, the app restarted. Each scenario in `scenarios/` starts the app on an empty library and says in plain words at its top what it checks. Helpers are in `lib/`: `app.mjs` drives the page, `os.mjs` takes screenshots and real clicks from the operating system, `drivers.mjs` starts the driver.

## The three runners

- **`run.mjs`** drives one app binary on this computer, through WebDriver on Linux (tauri-driver with WebKitWebDriver) or, on Windows and macOS, through the app's own test hook in a build made with the `e2e` feature (`--driver bridge`). The other two runners end up calling it.
- **`distros.mjs`** (Linux only) runs the suite inside a podman container per distribution, its "cell": our .deb or .rpm installed with that distribution's package manager, then `run.mjs` under a headless display. `--only debian-13,fedora-44` picks distributions (the list is `DISTROS` in distros.mjs). `--env weston` or `--env gnome` runs under a headless Wayland compositor instead of Xvfb with openbox; `--scheme dark` in the dark colour scheme; `--gpu nvidia` with the host's NVIDIA GPU, `--gpu dri` with the host's render nodes (`/dev/dri/renderD*`) and the distribution's Mesa (Intel and AMD).
- **`runners.mjs`** runs the suite on another computer over SSH (a "runner"): it copies the harness and the app there, runs `run.mjs`, and brings the results back. Runners are listed in `runners.json`, which isn't in the repository: copy `runners.example.json` and fill in each one's name, SSH host, operating system (`windows` or `macos`), a folder to work in, and for Windows the folders to put first on `PATH`. Someone must be signed in on the runner, and its app data is emptied before each scenario, so use a computer kept for testing.

`contact-sheet.mjs` makes a page of the checkpoints a run took (`ctx.checkpoint`), one column per run, for looking at what the tests can't judge. `verify.mjs` has a model judge them (below, "Judging the pictures").

## What they need

- **Node 24** (the version in `.nvmrc`), through mise or any other way. The harness imports a TypeScript file from the app, which Node 24 runs as it is.
- **tauri-driver**, from cargo: `cargo install tauri-driver --locked`. It must be on `PATH`, or named by `TAURI_DRIVER`. `distros.mjs` mounts this binary into each container.
- **podman**, for `distros.mjs`. The containers pull public images (Debian, Ubuntu, Fedora, Arch and others) and install packages, so the first run of each needs the network.
- For `run.mjs` directly on Linux: WebKitWebDriver (`webkit2gtk-driver` on Debian and Ubuntu, renamed `webkitgtk-webdriver` in Ubuntu 26.04; `webkitgtk6.0` on Fedora), and for a headless run Xvfb, openbox, xdotool and ffmpeg.
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

### Several at once, and shards

`--parallel N` (Linux) runs N apps at once, each in a worker of its own: `run.mjs` again, with its own data folders (`XDG_DATA_HOME`, `XDG_CONFIG_HOME` and `XDG_CACHE_HOME` with `-1`, `-2`, … added, and a private `XDG_RUNTIME_DIR`), its own driver port (`E2E_DRIVER_PORT` + 1, + 2, …), its own D-Bus session and its own X display (Xvfb, with openbox if it's installed: the OS layer's screenshots and clicks are of the whole screen, so two windows on one display would see and click each other). Each worker starts with the tools and model of the configured data folder, hard-linked (so a run of setup there first saves each worker the download). Its output comes marked `[w1]`, `[w2]`, …, and its results are merged into `E2E_OUT` at the end in scenario order (`results.json`, `checkpoints/`, `perf.json`, the failures' pages and screenshots), as one run would have left them. It needs `XDG_DATA_HOME` set, and can't be used with `--profile`.

With the environment above (each worker makes its own bus and display, so the run needs neither around it):

```sh
node tests/e2e/run.mjs /usr/bin/vodscribe --parallel 3
```

Scenarios are dealt longest first, each to the worker with the least to do so far, by the seconds each took when it last passed: `durations.json`, refreshed from runs' results with `node tests/e2e/durations.mjs <out dir>...` (a scenario that's new to it goes last). `--shard i/N` runs the i-th of N such deals, the same on every computer for the same file, for CI's machines to split the suite between them; it combines with `--parallel`. `distros.mjs --parallel N` runs a cell's suite that way; `runners.mjs` passes `run.mjs`'s options on as they are (`--shard` works there; `--parallel` is Linux's only).

### In CI

`e2e.yml` runs the suite as a matrix of shards, `run.mjs --shard i/N`: 2 on Ubuntu, 2 on Windows and 3 on macOS (its runners are the slowest: the whole suite took 26 minutes there, 10 to 13 elsewhere). `durations.json` makes the shards about equal, so refresh it (above) when scenarios are added or change a lot. Each shard uploads its `tests/e2e/out` as the artifact `e2e-<target>-<i>`, and a last job runs

```sh
node tests/e2e/merge-results.mjs --expect <shards> <folder of the artifacts' folders>
```

which reads every `results.json` and prints one summary in `run.mjs`'s words (`e2e: P of T passed, F flaky, in K shards`, a line per shard, and a line for each failure or flaky pass, with its shard). It exits 1 on a failed scenario, or when fewer shards reported than `--expect` (a shard that crashed leaves no `results.json`).

Which shards run when (`build.yml`):

- **Fast lane**: every push to `main` and every pull request: the Ubuntu shards only, against the Linux build.
- **Full lane**: every night (09:00 UTC), on a manual run and on a `v*` tag: every system's shards. By hand: `gh workflow run build.yml` (add `-f lane=fast` for the fast lane). To test an earlier build's installers without building again: `gh workflow run e2e.yml -f build_run=<run id>` (`-f lane=fast` for the Ubuntu shards only). The full lane is also where `verify.mjs` belongs: `e2e.yml` has the step, commented out until `OPENROUTER_API_KEY` is a repository secret.

### The flake ledger

`flaky.json` names the scenarios known to fail now and then for reasons outside what they check, each with the reason and the date it was seen: `{ "<scenario>": "<what happened, where, when>" }`. A listed scenario that fails runs once more; if it passes then, it's reported `flaky`, on a line of its own and counted apart from the passes (`e2e: 31 of 33 passed, 1 flaky`), never as passed, and its entry in `results.json` has `attempts: 2`, `flaky: true` and the first attempt's error (`firstError`). The first attempt's page and screenshot are kept as `<scenario>.first.txt` and `.png`, its checkpoints labelled "(first attempt)". A scenario goes in the ledger with its reason once the failure has been looked into and found to be the computer's or the runner's, not the app's; it comes out when the cause is fixed or a few rounds pass without it. Every summary says how many the ledger holds.

The `setup` scenario always runs first when it's chosen, and downloads the tools and the speech model the others keep. Without them, whichever scenario runs first goes through setup and downloads them; with it, they're copied in and the run starts at once. Quit any VODscribe you have open first: the run refuses to start beside one.

## Environment

- `E2E_OUT`: where `run.mjs` writes its results (default `tests/e2e/out`).
- `E2E_RESET_DATA`: `1` lets a scenario empty a data folder that already has a library. Without it, the run refuses rather than delete one; point `XDG_DATA_HOME` at a folder of its own.
- `E2E_NETWORK`: `1` runs the scenarios that reach Twitch, YouTube or Kick; without it they're skipped.
- `E2E_SITE`: which site's VOD the site scenarios play (`twitch`, the default, or `youtube`).
- `E2E_SHOTS`: set, `site-embed` keeps a screenshot of the whole screen at each step.
- `E2E_TRACE`: `1` logs each call into the page and how long it took.
- `E2E_DRIVER_PORT`: tauri-driver's port (default 4444), for two runs side by side on one computer; with `--parallel`, the workers take the ports after it. The native driver under it (WebKitWebDriver) gets a free port of its own each run: tauri-driver's default, 4445 for every run, had a second run drive the first one's app.
- `E2E_WORKER`: set by `--parallel` in each worker (its number); not for setting by hand.
- `E2E_SCHEME`: the colour scheme the run is in (`light` or `dark`), which the site scenarios check; `distros.mjs` sets it from `--scheme`.
- `E2E_DISPLAY`: the Wayland compositor the run is under (`weston` or `gnome`), so screenshots know how to be taken; `distros.mjs` sets it from `--env`.
- `E2E_SEED`: for `distros.mjs`, a data folder whose `bin/` and `models/` each cell starts with (default none, so setup downloads them).
- `E2E_WORK`: for `distros.mjs`, where each cell's data folder goes (default `tests/e2e/out/distros`).
- `E2E_PODMAN`: the podman command for `distros.mjs` (default `podman`; a wrapper for another store, for example).
- `TAURI_DRIVER`: the tauri-driver binary, if it isn't on `PATH`.
- `NATIVE_DRIVER`: the platform's WebDriver, passed to tauri-driver, if it isn't where tauri-driver looks.
- `XDG_DATA_HOME`: on Linux, where the app keeps its library, and so what the tests empty.
- `VODSCRIBE_NO_UPDATE_CHECK`: read by the app, not the harness: set, it doesn't look for updates.
- `PROFILE_CG`: with `--profile`, how perf walks the stack: `fp` (frame pointers, the default) or `dwarf` (deeper through the system's libraries, about 60 times the disk).
- `PROFILE_HZ`: with `--profile`, samples a second (default 999).
- `PERF`: the perf command, if it isn't `perf` on `PATH`.
- `E2E_COMMIT`: what `perf.json` names the build by (default `git describe` of the checkout).

## Results

`run.mjs` writes `results.json` (each scenario, passed, failed, skipped or flaky, and why) to `E2E_OUT`, and ends with a summary: how many passed, a line for each scenario, the flake ledger's size, and the wall time on how many workers. For a failure it adds the page's text and errors (`<scenario>.txt`) and a screenshot (`<scenario>.png`); checkpoints go in `checkpoints/`. `distros.mjs` keeps each cell's results in `tests/e2e/out/distros/<cell>/`, named for the distribution and what differs from the default (`debian-13-weston-dark`), with the whole log in `run.log`, and prints one line per cell at the end. `runners.mjs` brings a runner's back to `tests/e2e/out/<runner>/`. Two runs of the same cell from the same checkout wipe each other's folders.

Every run also writes `perf.json` to `E2E_OUT`: the steps each scenario timed (below), the app's memory at each scenario's end, and what the run was on (CPU, kernel, WebKit, display). Each result in `results.json` has its `timings` (ms) and, on Linux, `memory`.

## Judging the pictures

The tests are layered: unit tests of the app's pure parts, then these scenarios, which check what the page and the screen say, then on top `verify.mjs`, which has a model look at the checkpoints the way a person would. It's the most flexible layer and the only one that costs money per run: it judges what no check can (a picture cut off, text over text, a dark theme half applied), never what a check already asserts.

A scenario says what each checkpoint should show beside its `run`:

```js
expects: {
  "search-or": 'Under the search box, which reads "zebra OR landed", the query is shown as two word chips, ...',
},
```

The rule for an expectation: write what a person should see; one or two sentences; nothing a check already asserts. Write it so a wrong picture fails it, not so anything passes: name what's there and where, not "the page looks right".

```sh
OPENROUTER_API_KEY=... E2E_VERIFY_MODEL='vendor/family*' node tests/e2e/verify.mjs tests/e2e/out
```

Each checkpoint with an expectation goes to OpenRouter's chat completions with its picture (scaled as the contact sheet does), the expectation and a fixed rubric; the answer is pass, fail or unsure, with a reason and, as concerns, anything that looks broken even if not asked. Checkpoints without an expectation are listed, not judged, and an expectation whose checkpoint a scenario that ran didn't take is named (a label renamed). `--model` or `E2E_VERIFY_MODEL` is a model id, or a prefix ending in `*` for the cheapest model with that prefix that takes pictures, looked up at run time (variants such as `:batch` left out); the line it prints says which, and its price. `--only a,b` keeps those scenarios, `--concurrency N` (default 4) caps the requests at once (each retried on 429 and 5xx, 60 s at most), `--out` names the verdict sheet (default `verify.html` in the first out dir), with `verify.json` beside it: each checkpoint's scenario, label, expectation, verdict, reason, concerns, model and tokens. `--dry-run` prints the pairs and the prompt and calls nothing. The last line is `verify: P pass, F fail, U unsure, N without expectation; ~T tokens`; the exit status is 1 on any fail, not on unsure.

It runs in the full lane only (nightly, by hand, on tags), never per push.

## Timings and profiling

A performance pass starts from measurements, not from reading the code: the scenarios time what a person waits on, and a profiled run says where the time went. Releases are compared by both.

### Timings

The scenarios time these steps (`ctx.time` and `ctx.timing` in a scenario; the `App` helpers time their own):

| Step                                                            | Where                                                                                                                                                       |
| --------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| start to first screen                                           | every scenario: from asking for the app to its first screen (a start App.ready had to reload the page for is named "(reloaded)", apart)                     |
| open to first frame                                             | `player-picture`: opening the VOD and Play, to the app's own player showing a frame (by its `player-time` status)                                           |
| seek to frame shown                                             | `player-picture`: a seek to the frame there showing                                                                                                         |
| search, many lines / one line a VOD / none                      | `search-large`: on 60 VODs of two hours' talk (`fixtures/large-library.mjs`), from the words typed to the results drawn                                     |
| clip made (2 s) / whole video's clip made (5 s) / edit rendered | every scenario that cuts the 1 to 3 s clip (`App.clipOneToThree`: `clip`, `edits`, `drag-out`, …) / `export-whole` / `edits`: from the key to the file made |
| transcribed (the file's name)                                   | every scenario that adds and transcribes a file (`App.addAndTranscribe`): the worker's time on it, 5 s of speech for `eagle-has-landed.mp3`                 |
| memory at the end                                               | every scenario (Linux): the app's processes' proportional set size (Pss: shared pages counted once), in `lib/memory.mjs`                                    |

`--repeat N` runs the chosen scenarios N times in turn, and `perf.json` keeps each step's median, least and most. Two runs compare with `node tests/e2e/perf-compare.mjs <before perf.json> <after perf.json>`: a step whose times moved clear of the other run's spread, by more than 10% (`--threshold`), is marked, and one that got worse makes it exit 1. Time the release build, unprofiled: perf slows what it watches, and `perf.json` says when a run was profiled.

### Profiling (Linux)

`run.mjs --profile <dir>` samples each scenario's app with perf, and every process it starts (the web view's, the worker, ffmpeg), for the whole scenario. The driver starts `lib/profile.sh` in the app's place, which starts perf in a session of its own attached to itself and then becomes the app, so WebKitWebDriver's kill at the end of a session reaches the app and perf finishes its file. Into `<dir>/<scenario>/`: perf's recordings (`<scenario>-<n>.data`, one for each start of the app; `perf report -i` reads them), `folded.txt` (every stack, `process;thread;outermost;…;innermost count`, for a flame graph) and `top.txt`; the page's own measures go in `page.json` (in `E2E_OUT/perf/` without `--profile`).

What it needs:

- **The profiling build**, for our own code's names: `scripts/build-profiling.sh` (after `scripts/prepare-sidecars.sh release`) builds the `profiling` cargo profile, release code with its symbols and line tables and frame pointers, and the page unminified; the binary is `target/profiling/vodscribe` (under `CARGO_TARGET_DIR`), with the worker beside it, and is run directly: `node tests/e2e/run.mjs <target>/profiling/vodscribe --only player-picture --profile out/profile`. A packaged build has no symbols of ours.
- **perf and symbols**, where the run is: `tests/e2e/profile-setup.sh` (Debian, with sudo) installs perf, binutils (whose `c++filt` names Rust's symbols) and libc's debug symbols, and fetches WebKitGTK's and JavaScriptCore's debug symbols for exactly the version installed from snapshot.debian.org (about 200 MB), without which the engine's functions show as addresses. A profile is read where it was recorded (`perf script` finds the symbols there). `kernel.perf_event_paranoid` must be 1 or less.
- In a distribution cell: `distros.mjs --profile` installs perf in the cell and lets it sample (`--security-opt seccomp=unconfined`: podman's default filter refuses perf), into the cell's `perf/`. Not Ubuntu's cells, whose perf must match the host's kernel.

### Reading top.txt

`top.txt` puts each sample down to an owner by its innermost frame: **ours** (our crates, `vodscribe_*`), **crates** (the Rust libraries the app links in), **engine** (WebKit, its JavaScript, and everything else in the web view's processes), **graphics** (wgpu, Mesa, software rendering), **media** (ffmpeg), **transcription** (the worker, its speech model's runtime included), **toolkit** (GTK, GLib, fonts), **system** (the kernel, libc). Its sections: the share by process and thread; by owner, and how much had our code anywhere on the stack; our functions by their own time, with who called them; everyone's functions; then, for each timed step, the samples taken during it, by owner and function; and the page's slow input events (16 ms or more, by the Event Timing API) and gaps between animation frames over 50 ms (WebKit has no long tasks API).

A **finding** is one of: time in our code that needn't be spent; engine time our page causes (style, layout or script while nothing changed on screen); work done every frame that could be done when something changes; the same work done twice; work on the main thread that could be elsewhere; or a timing worse than the last release's beyond its spread. It names the scenario, the step, the stack and its share. It is **confirmed** by the change and a before-and-after on the same scenario, machine and build (`--repeat 3`, `perf-compare.mjs`), with a profile showing the samples gone; until then it's a note.

### Every release

On the computer and box the last release was measured on (a Debian box whose WebKit matches the developer's desktop):

1. `scripts/build-profiling.sh`, and the release build.
2. Profiled once, the profiling build: `--only player-picture,player-seek,transcribe-search-play,search,search-large,clip,edits,export-whole --profile <dir>`.
3. Timed three times, the release build: the same scenarios, `--repeat 3`.
4. `perf-compare.mjs` against the last release's `perf.json`, and the results kept with the release's.

The rest (video-files, recovery, upgrade, the site scenarios, a DWARF profile) on demand. Profiled at 999 samples a second with frame pointers, a scenario has run no slower so far, and its recordings take about a megabyte; with DWARF, about 40 MB for ten seconds of it.

### What it doesn't measure

- **The GPU.** Headless, wgpu draws with lavapipe and WebKit composites with llvmpipe, on the CPU: graphics time in these profiles is CPU time a GPU would take. Profile with `distros.mjs --gpu` before calling graphics time a finding.
- **The Windows and macOS runners.** Timings and memory need nothing of Linux but `memory.mjs`; profiling there would wrap the bridge driver's start in the platform's profiler (`xctrace record --launch` on macOS, `wpr` on Windows).
- **The sites' players.** Their frames are theirs; only the app around them is measured.
- **Our page's JavaScript by function name.** perf sees JavaScriptCore's compiled code as `[JIT code]`. JavaScriptCore can describe it for perf (`JSC_useJITDump=1`), but WebKitGTK 2.54 (Debian) then aborts as the app starts; not looked into yet.

## Known limits

- `distros.mjs` is Linux only: it needs podman and Linux containers.
- ubuntu-22.04 needs a .deb built on Ubuntu 22.04, such as CI's artifact. One built on a newer system links a newer glibc, and the cell stops at "missing libraries".
- `--gpu nvidia` needs nvidia-container-toolkit, with a CDI spec generated for podman (`nvidia-ctk cdi generate`).
- `--gpu dri` needs a podman whose runtime is crun, to keep the user's video and render groups in the container. On a GPU Mesa has no driver for, it draws with Mesa's software fallback.
- Under `--env weston` there's no seat, so the window never has the keyboard and the operating system's clicks aren't available; the scenarios and steps that need them are skipped there.
- The drag-out scenario's Alt-drag step is skipped under window managers that move windows on Alt-drag.
