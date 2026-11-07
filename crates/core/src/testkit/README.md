# The test kit

Fakes for what's outside the app, and helpers to set up a library, so a unit or integration test does no slow I/O and decides the order things happen in. The app's own kit, `apps/desktop/src-tauri/src/testkit.rs`, runs the whole app on them.

- `tools.rs`: programs faked in this process, keyed by their path, and `Gate`, a point a fake waits at.
- `ffmpeg.rs`: a fake ffmpeg: details, cuts and renders with progress, stills, and its failures.
- `yt_dlp.rs`: a fake yt-dlp: a VOD's details and its downloads, as the site serves them.
- `worker.rs`: a fake transcription worker: the words it's told are said.
- `media.rs`: the fake media files the fakes write and read.
- `net.rs`: recorded answers for links.
- `library.rs`: a library in a folder of its own, and VODs and words put in it.
- `tests.rs`: the kit's own tests, each fake beside the real program where it's installed.

## Writing a test

Test a spec: something the app promises its user. Find it in the changelog, an e2e scenario or a doc comment, and name the test after it as a sentence:

```rust
/// A canceled clip says so on the clip bar and has no file
/// (CHANGELOG.md, 0.7.1: "A canceled clip or edit says so.").
#[test]
fn a_clip_canceled_while_it_is_made_says_so() {
```

The doc comment cites where the spec is said. When the code does something else, the test still states the spec and fails; report it. Behaviour that no spec states isn't tested: ask the owner about it.

Tests go in a file of their own beside the module, never in a `mod tests { … }` inside it. For `clips.rs`, the module ends with

```rust
#[cfg(test)]
mod tests;
```

and the tests are in `clips/tests.rs`, starting with `use super::*;`. A new folder such as `clips/` gets a README.md with a line for its file. Group them by feature, not by function. Test at a seam that doesn't move: a core module's public functions, a Tauri command and the state it leaves. Test a single function only when it is the spec, as a parser is.

## The app in a test

`TestApp::new()` starts the app as it starts for real, on Tauri's mock runtime, with a library of its own and ffmpeg and yt-dlp faked:

```rust
let app = TestApp::new();
let vod = app.add_video_file("Day 3.mp4", 60.0); // a file on this computer
app.says(vod.id, 12.0, "The Eagle has landed"); // its transcript
let clip = app.make_clip(vod.id, "0:10", "0:20"); // Clip, then its file made
let media = app.clip_media(&clip); // what the clip's file holds
assert_eq!(media.cut_from.unwrap().0, vod.source_path.unwrap());
```

- `add_video_file`, `add_file` and `add_site_vod` add VODs; the fake site serves a site's VOD.
- `make_clip` makes a clip and waits for its file; `add_clip` only asks for it, and `made` waits.
- `clips`, `clip` and `db` tell what the library has.
- `invoke("cancel_job", json!({ "kind": "clip", "id": id }))` calls a command as the page does, with its arguments in camelCase. Call the command's Rust function through `app.run(...)` when the test needs its typed answer.
- `heard("clip")` gives the events the page heard, in order.
- `desktop()` tells what the file manager was asked to show or open, and `saves_to` answers the save dialog.
- The app's queues end with the test, but a command that starts a thread of its own (looking up a VODscription after an add or a refresh, a site's player's picture let go, setup's and the GPU pack's installs) runs on: wait for what it does before the test ends.

Times are written as a clock shows them (`"1:02:03.5"`), and `at` reads them. Run the tests with `cargo test` in `crates/core` and in `apps/desktop/src-tauri`; `cargo test clips::` runs one module's.

## The fakes

Each fake answers as its program does, failures included. A test changes how it behaves:

- `app.ffmpeg.fails_at(0.5, "message")`: each file it makes fails halfway, as ffmpeg does when a source breaks off.
- `app.ffmpeg.lacks("libx265")`: a build without that encoder.
- `app.yt_dlp.refuses(url)`: the site refuses the download.
- `works()` puts ffmpeg back as it was.

To play a race in a set order, hold a fake at a `Gate`, act, then let it go:

```rust
let halfway = Gate::new();
app.ffmpeg.holds_at(0.5, &halfway);
let clip = app.add_clip(vod.id, "0:10", "0:20").unwrap();
halfway.reached(); // ffmpeg is halfway through the clip
app.invoke("cancel_job", json!({ "kind": "clip", "id": clip.id })).unwrap();
halfway.open();
```

Never sleep to wait for something. Wait for what shows it happened: `made`, an event `heard`, or `eventually(what, condition)`.

Fake media files hold what a file would (length, picture, sound) and what they were cut from, as JSON. Make a source with `Media::video(60.0)` or `Media::sound(60.0)` and `write`, and read a result with `Media::read`.

`Net::new()` answers links for as long as it's kept: `serves(prefix, body)`, or `answers(prefix, 404)`. Give a test's links a host of its own, since tests run at once.

Core tests use the same fakes without the app: `Library::new()` for the library, `Ffmpeg::at(dir.join("bin/ffmpeg"))` and the like for a program, `TempDir::new(name)` for files.

## What's real and what isn't

The library is SQLite with every migration, and files are files, in a temporary folder that goes with the test. Programs, the network and the desktop are faked. These aren't, so a test can't reach them yet: the clock and the job queues' timing, requests made outside `net::get` (Kick's API, chat replays, the emote services, the VODscription host), drags, and the updater. These still start their program directly, so with a fake in the bin folder they run its empty file and fail: YouTube chat through yt-dlp (`chat.rs`), the yt-dlp update's version check (`tools::update_yt_dlp`), the GPU check (`gpu.rs`), and the player's decoders and storyboards (`crates/player`).
