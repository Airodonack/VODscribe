# Harness helpers

What the scenarios and the runners share.

- `app.mjs`: what a scenario does with the app: run code in its page, call its commands, click, wait, answer its dialogs.
- `drivers.mjs`: the two ways into the page, WebDriver and the app's own test hook, behind one interface.
- `os.mjs`: the OS's side: whole-screen screenshots, colours read from them, and real clicks and keys.
- `data.mjs`: empties the app's data folders before each scenario, keeping the tools and model.
- `fixtures.mjs`: test files made at test time with ffmpeg, and a synthetic chat.
- `siteVod.mjs`: VODs "from a site" added without the network, through a stand-in yt-dlp.
- `fileHost.mjs`: a stand-in for the host of VODscriptions and the cast's pictures.
- `fileManager.mjs`: the file manager a dragged clip is dropped on, Nautilus or Dolphin on Linux and Finder on the Mac, and the drag scenarios' shared steps, such as choosing the clip format and quality.
- `explorer.mjs`: File Explorer and drop targets on Windows, for drags in and out.
- `archives.mjs`: apps that drag out files with no address yet on Linux: an archive manager, or stand-ins for one and for a sandboxed app.
- `inOut.mjs`: I right after a timeline click, shared by two scenarios.
- `screen.mjs`: a smaller screen than the computer's, for layout checks.
- `preflight.mjs`: what a run needs of the computer, checked before any scenario.
- `runnerScreen.mjs`: whether a runner's screen is clear before the suite.
- `procs.mjs`: processes the app started that are still running.
- `dump.mjs`: what a failure leaves to look at, as text.
- `memory.mjs`: the app's memory at a scenario's end, on Linux.
- `durations.mjs`: scenarios' last times, and dealing them out to workers and shards.
- `budgets.mjs`: the performance budgets, and judging timings against them.
- `profile.mjs`, `profile.sh`: running the app under perf, and reading the recordings.
- `*.test.mjs`: unit tests of the module of the same name, run with `node --test`.
