# The test kit

Fakes for what's outside the app, and helpers to set up a library, so a unit or integration test does no slow I/O and decides the order things happen in. The app's own kit, `apps/desktop/src-tauri/src/testkit.rs`, runs the whole app on them.

- `tools.rs`: programs faked in this process, keyed by their path; `Gate`, a point a fake waits at; the points of the library's own code a test holds (`hold_at`); the runs the app tracks for its quit to stop, by the folder they work in (`stop_tracked_in`); and the PATH a test gives the app (`path_for`).
- `ffmpeg.rs`: a fake ffmpeg: details, cuts and renders with progress, stills, the player's and the storyboards' decoding, and its failures.
- `yt_dlp.rs`: a fake yt-dlp: a VOD's details, its downloads and its chat replay, as the site serves them, and its version.
- `nvidia_smi.rs`: a fake `nvidia-smi`: the graphics card the computer has, or none.
- `worker.rs`: a fake transcription worker: the words it's told are said.
- `media.rs`: the fake media files the fakes write and read, and the frames they decode.
- `net.rs`: the network: where a name resolves, and each request's answer.
- `sites.rs`: the sites' own APIs, answered as each site answers.
- `library.rs`: a library in a folder of its own, VODs and words put in it, and a connection of its own beside the app's; older libraries: one at any format, one taken back to a format, and those releases left; and one a newer version changed.
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

`TestApp::new()` starts the app as it starts for real, on Tauri's mock runtime, with a library of its own, and ffmpeg, yt-dlp and the network faked:

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
- `invoke("cancel_job", json!({ "kind": "clip", "id": id }))` calls a command as the page does, with its arguments in camelCase; `invoke_bytes` for one that answers in bytes (`player_still`). Call the command's Rust function through `app.run(...)` when the test needs its typed answer.
- `heard("clip")` gives the events the page heard, in order.
- `desktop()` tells what the file manager was asked to show or open.
- `quit()` quits as closing the window does (lib.rs `quit`, which stops what this test's app started and nothing of another test's), and `kill()` ends the app as if it were killed. Each gives back its `Folder`, and `TestApp::on(folder)` starts the app on it again, as after that quit: the start's own steps (lib.rs `open_at_start`: the library opened and settled), its tidying, then the queues taking up what was left, the storyboards' sweep among them.
- `TestApp::on(folder)` starts on any library: one that won't open starts on the recovery screen, with no state but `library_problem`'s, and one it can't read in full starts saying what didn't (`invoke("not_started")`).
- `TestApp::on_with(folder, |fakes| fakes.ffmpeg.holds_at(0.25, &gate))` tells the fakes what to do before the start does anything with them: a cut left unmade is held as it begins again.
- `invoke("quit_app")` (Ctrl+Q) ends the app as its event loop's end would, which Tauri's mock runtime can't; `exited()` then gives the code it ended with and its `Folder`.
- `drag_out(&[ids])` drags clips out of the window as the page does, and `drags().last()` is the drag. No system drag begins. `drag_ended()` waits for what the page hears at its end.
- `drop_files(&[("Day 3.mp4", None)])` drops files with no path on the window (a video in a zip folder, a file promise), as the system's drop target hands them over: each the window takes is received, and the test gives its bytes (`give(&Media::video(60.0).to_bytes())`), then ends it `whole()`, `fails("why")` as its app, or drops it (canceled). `drop_paths(&[path])` and `drop_links(&[url])` drop files with paths and a browser's links: the page hears them as paths (`heard("tauri://drag-drop")`). On Linux, `drop_gives_nothing("why")` is a drop whose app saved nothing.
- `lacks_yt_dlp()`: the computer has no yt-dlp, as before setup. A test's app finds none of the computer's programs on the PATH.
- `nvidia_smi().shows("NVIDIA GeForce RTX 4070", 12282, 11000)`: the computer has that graphics card, with that much memory free (MB); without it, none.
- `takes_no_hard_links()` puts the library on a disk without hard links, as exFAT is.
- `holds_copies(&gate)` holds a pasted clip's copy of its file at the gate once it's written, as a slow disk would.
- `folder.holds_worker_start(&gate)`, before `TestApp::on(folder)`: that start's transcription waits at the gate while the other queues take up what was left, as on a busy start.
- `holds_cut_ends(&gate)` holds a clip's cut at the gate once its encode is over, before the library records it: the moment a clip is made just as it's canceled.
- `holds_erasures(&gate)` holds each erasure of a removed VOD between its parts, the library let go of; `holds_erasure_parts(&gate)` holds it in a part, the library still held.
- `holds_sweeps(&gate)` holds the storyboards' sweep once it has read the library's VODs, before it goes through the storyboards, so a VOD can be added meanwhile.
- `holds_plans(&gate)` holds each plan of an edit the page asks for once it's worked out, before it's played.
- `holds_searches(&gate)` holds each search once it's asked, before it has its connection, so a search typed after it can go first.
- `holds_settings_reads(&gate)` holds a read of an unreadable settings file before it sets the file aside, and `holds_settings_saves(&gate)` each save before its turn.
- Each `holds_…` takes the place of the gate it held at before: `Gate::opened()` holds no more.
- `worker()` is the transcription worker the app runs, faked, with its model installed: `app.worker().hears(said(10.0, "The Eagle has landed"))`, and the queue transcribes what's asked. The words are at the VOD's seconds, a section downloaded for a stretch included. A start on its library again has the worker from the start, hearing nothing till told.
- `says_as(vod.id, 12.0, "Hello", "Ada")` is a line someone says, as a VODscription names them; `add_site_vod_of(…, "Ada Plays")` a VOD on that channel.
- `dragged_out(&clip)` drags a clip out and lets it go on an editor that takes the file, and gives the file it got. `erasures_done()` waits till no VOD is being erased.
- `threads_done()` waits for the threads the app's commands start and don't wait for (an added VOD's picture asked for, the file host asked about its VODscription). The app's end waits for them too, before its state goes; a new such thread is passed to `testkit::started`.
- `net` is the network the app reaches. Every site's and service's host is the fake's (`Net::sites`), so nothing reaches a real site, and a request nothing answers gets 404. Tell it what a site answers, as below.
- `fetches_chats()` runs the chat queue, as the app's start does: the chats not fetched yet first, then each one asked for. Its fetches reach `net` and the fake yt-dlp.
- `clock()` is the app's clock. It stands still till the test moves it with `clock().advance(Duration::from_millis(250))`, so a cut's progress, sent at most every quarter second, is sent when the test says.
- `with_screen()` gives the window a screen for the player to draw on, which the mock runtime's lacks, and `screen()` is it: `showing()` is the frame on it (None while it's black), `frame_shown()` waits for one and gives its number, `closed()` says whether it was taken away for good.
- `started_with("VODSCRIBE_DRAG_WAITS", "1")` has the app see that variable, as if it had been started with it. Each test's variables are its own, and the process's aren't seen.
- The app's queues end with the test, but a command that starts a thread of its own (looking up a VODscription after an add or a refresh, a site's player's picture let go, setup's and the GPU pack's installs) runs on, unless it's passed to `started`: wait for what it does before the test ends.

Times are written as a clock shows them (`"1:02:03.5"`), and `at` reads them. Run the tests with `cargo test` in `crates/core` and in `apps/desktop/src-tauri`; `cargo test clips::` runs one module's.

## The fakes

Each fake answers as its program does, failures included. A test changes how it behaves:

- `app.ffmpeg.fails_at(0.5, "message")`: each file it makes fails halfway, as ffmpeg does when a source breaks off.
- `app.ffmpeg.lacks("libx265")`: a build without that encoder, left out of its `-encoders` list too.
- `app.ffmpeg.holds_list(&gate)`: its list of encoders waits at the gate, as a slow first run does.
- `app.ffmpeg.holds_stills(&gate)`: each still it makes (a clip's picture) waits at the gate, its input read, as ffmpeg has the file open while it decodes.
- `app.ffmpeg.holds_details(&gate)`: each time it's asked a file's details (a probe, the player's open among them), it waits at the gate.
- `app.ffmpeg.updated()`: its file replaced, as an update does, so the app sees a new ffmpeg at the same path.
- `app.yt_dlp.refuses(url)`: the site refuses the download.
- `app.yt_dlp.holds(&gate)`: each download waits at the gate once it's begun, before it says how far it is; `holds_at(0.5, &gate)` once half of it is in. A download is written a quarter at a time to a part file beside its file till it's whole, as yt-dlp's is: one stopped or held partway leaves it, and the next carries on from it. A section's progress is gauged from its part file too, against its bitrate (`tbr` in the details).
- `app.yt_dlp.is_version("2025.11.12")`: what `--version` says.
- `app.yt_dlp.holds_details(&gate)`: each look-up of a link's details (an add, a refresh) waits at the gate before it answers.
- `app.yt_dlp.serves_chat(url, lines)`: a YouTube video's chat replay, each line a moment's messages (`yt_dlp::chat_line(12.0, "@ada", "hi")`), written a line at a time as yt-dlp writes it. A video it serves none of has none. `holds_chat(&gate)` holds each once its first line is written.
- `app.worker().holds(&gate)` waits after the first minute it transcribes, and `fails_at(200.0, "message")` fails there.
- `works()` puts ffmpeg back as it was, and the worker too, keeping what it hears.
- `app.ffmpeg.runs()` gives the arguments of each run so far, in order.

To play a race in a set order, hold a fake at a `Gate`, act, then let it go:

```rust
let halfway = Gate::new();
app.ffmpeg.holds_at(0.5, &halfway);
let clip = app.add_clip(vod.id, "0:10", "0:20").unwrap();
halfway.reached(); // ffmpeg is halfway through the clip
app.invoke("cancel_job", json!({ "kind": "clip", "id": clip.id })).unwrap();
halfway.open();
```

A file ffmpeg makes is there from its run's start, as a part file beside the clip, but it's media only once the run is over. Its picture keeps what ffmpeg was asked to make it with: `crf`, `profile`, `pix_fmt` and `tag`.

To see a cut's progress, hold ffmpeg before it says how far it is (`holds_at(0.25, …)`), move the clock on, then let it go. ffmpeg says a quarter, a half, three quarters and the end; the page hears the first that comes a quarter second or more after the last it heard:

```rust
let first = Gate::new();
app.ffmpeg.holds_at(0.25, &first);
let clip = app.add_clip(vod.id, "0:10", "0:20").unwrap();
first.reached();
app.clock().advance(Duration::from_millis(250));
first.open();
app.made(clip.id); // the page heard "clip-progress" once, at 0.25
```

A restart, and a drag let go on an app:

```rust
let app = TestApp::new();
let vod = app.add_video_file("Day 3.mp4", 60.0);
let clip = app.make_clip(vod.id, "0:10", "0:20");
let app = TestApp::on(app.quit()); // or app.kill()
app.drag_out(&[clip.id]).unwrap();
let drag = app.drags().last();
drag.let_go(LetGo::On(Receiver::taking_files("Kdenlive")));
assert_eq!(app.drag_ended()["delivered"], true);
assert_eq!(Media::read(&drag.given()[0]), Some(app.clip_media(&clip)));
```

The test says where a drag ends: `LetGo::CalledOff` (Escape), `Back` on the window, `Nowhere`, or `On` another app. That app takes the files themselves (`Receiver::taking_files`), copies written for it (`taking_copies`), or only files already made (`finished_only`). The first two wait for a file still being made, and the page hears that they wait. `drags().releases_early()` has the button come up before the next drag begins. On Linux an app named `resolve` is DaVinci Resolve. `names()`, `file_types()`, `image()` and `given()` tell what the drag offered and what the app got.

Never sleep to wait for something. Wait for what shows it happened: `made`, an event `heard`, or `eventually(what, condition)`.

The player plays fake media as the real one plays a file: the fake ffmpeg decodes it, and frame `n` of its picture says `n` (`media::frame_number` reads it from the frame's YUV, or from the RGB the page is given). Give a test's video a small picture, so it decodes in a moment:

```rust
let app = TestApp::new();
app.with_screen();
let mut source = Media::video(60.0);
let video = source.video.as_mut().unwrap();
(video.width, video.height) = (64, 36);
let vod = app.add_file("Day 3.mp4", &source);
app.invoke("player_open", json!({ "id": vod.id, "at": 10.0, "play": false })).unwrap();
assert_eq!(app.screen().frame_shown(), 300); // 0:10, at 30 a second
```

Decoded, a fake video's sound is silence, at the rate asked. Its storyboard has a keyframe each second, each picture naming its time (`media::storyboard_time`). The player's sound plays out in time on a stand-in, not the computer's speakers.

Fake media files hold what a file would (length, picture, sound) and what they were cut from, as JSON. Make a source with `Media::video(60.0)` or `Media::sound(60.0)` and `write`, and read a result with `Media::read`.

`Net::new()` is the network for as long as it's kept, and `app.net` is the app's:

- `public("peertube.example")`: the name resolves to a public address, so a link to it is followed, and `local("router.example")` to the local network, so it's refused. Either host is the fake's: a request nothing answers gets 404. `sites()` makes every site's host public.
- `serves(prefix, body)` and `answers(prefix, 404)` answer the links starting with `prefix`, whatever is asked.
- `replies(prefix, |request| …)` reads each request as the site does (`method`, `url`, `header(…)`, `json()`) and answers with a `Reply` (`Reply::json(&value)`, `Reply::status(429)`, `.with_header("ETag", …)`), or None for a request that isn't the test's, which the next answer is asked.
- `holds(prefix, &gate)` holds each request to those links at the gate before it's answered.
- `twitch_chat("2901646294", &[(10.0, "ada", "hello")])` is Twitch's API with that VOD's chat (sites.rs).

A request goes to the fake whether it's made through `net::get` or the HTTP clients (Kick's API, Twitch's GQL, the emote services, the file host), and the client does with the answer what it does with a site's: its status, headers, redirects and limits. Tests run at once and see each other's answers, so give a test's links a host of its own, or on a site's own host answer only the test's own ids:

```rust
let app = TestApp::new();
app.net.replies("https://gql.twitch.tv/gql", |asked| {
    let ours = asked.json()?["variables"]["slug"] == "MyClipSlug";
    ours.then(|| Reply::json(&json!({ "data": { "clip": null } })))
});
```

Core tests use the same fakes without the app: `Library::new()` for the library, `Ffmpeg::at(dir.join("bin/ffmpeg"))` and the like for a program, `TempDir::new(name)` for files, `tools::hold_at(At::SettingsSave, &path, &gate)` to hold the library's own code at a point for a file, and `tools::path_for(&bin, &[dir])` for the PATH the system's programs are found on (`NvidiaSmi::in_dir(&dir)` there).

## Older libraries

An upgrade's test starts from a library at an older format, the number of migrations it has had (`PRAGMA user_version`):

```rust
let path = dir.join("library.sqlite");
let conn = at_version(&path, 29); // the first 29 migrations, no rows
conn.execute_batch("INSERT INTO vods (id, platform, platform_id, url, title, status, added_at) VALUES (1, 'kick', 'a', 'u', 't', 'done', 0);").unwrap();
drop(conn);
let db = Db::open(&path).unwrap(); // upgraded from 29
```

- `at_version(path, n)` makes a library at format `n`, for rows written as that version wrote them.
- `back_to(path, n)` takes a library this version made, with rows put in through `Db`, back to format `n`: each table's rows in the columns format `n` has, with its triggers and search index. Close the library first; its connection is at format `n`, for changing rows as that version would.
- `newer(path, "ALTER TABLE renders DROP COLUMN encoding;")` takes a library this version made to the next format, as a newer version that changed it so would leave it: `Db::open` opens it as it is.
- `Released::new("0.5.1")` copies the library and settings file that release left (tests/e2e/fixtures) into a folder of the test's own, as `library` and `settings`. `Released::versions()` lists the releases there. A file's path in the library keeps its placeholder, `{FIXTURES}`.

`db::MIGRATIONS` is the list `Db::open` runs, so neither needs changing when a migration is added.

## What's real and what isn't

The library is SQLite with every migration, and files are files, in a temporary folder that goes with the test, or with the last app started on it. Programs, the network, the desktop and drags out of the window and onto it are faked. A restart starts the fakes anew: what a test told them before has to be told again, and the fake site serves each site's VOD the library has. A clip's progress goes by the test's clock, its download's part too; an edit's render's and the footage and video downloads' own go by the real one. A restart starts a new clock and no variables. These aren't faked, so a test can't reach them yet: the job queues' timing, and the updater. The yt-dlp update's version check answers, but the update before it checks the release's signature, which only the real release files pass. The chat queue runs only once a test asks (`fetches_chats`), and the start's other jobs that reach the network (the missing thumbnails, the streamer cards) don't run.

A core test's own network: a link a test answers or a host it names is the fake's; any other goes to the network as before, for the tests that ask a site for real (`--ignored`, named `…_live`). Those are run alone: a test beside them that names a site's host takes it from them.

Where the restart and the fake drag differ from the real ones:

- After `quit()` or `kill()`, until the app has ended, a job still does its file work. A failing cut deletes its part file, and one that finishes renames its file into place while the library still says it's being made. A killed process would leave the part file for the next start. Hold the cut at a `Gate` to keep that from happening.
- A quit or kill with a transcription or download running waits up to 5 seconds for each of their writes, which the stopped library refuses.
- An app a drag is let go on goes by the name the test gives it. On Linux the real drag names it from the desktop entries (`Files` for `nautilus`).
- `taking_copies` is the macOS and Windows way. On Linux only Dolphin copies, and its end names no app.
- A drag refused by `finished_only` ends `Elsewhere`. Dolphin's real refusal ends `Nowhere`.

The files a library couldn't delete are listed per library in the tests, and that list is read again from its library at each start. A retry of them (a cut's end, a quit) tries every running test's list.
