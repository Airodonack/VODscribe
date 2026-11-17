//! The embed takeover's specs (embed.rs): what a site's player appends to
//! its media buffers, passed on by the page as it passes it (embed.ts),
//! kept as runs in the app's folder and played by the app's player, a
//! session at a time. The sites' players themselves are never in a test
//! here: what they do is the end-to-end tests' (site-*.mjs).
//!
//! Where the app's player is to show a picture, the site's player's init
//! segment is a fake video file (`Media`), which the fake ffmpeg decodes
//! frame by frame (frame n says n), and the media after it is nothing the
//! fake reads; where the test reads what was kept, the appends are MP4
//! boxes and WebM elements as the sites send them.

use super::*;

use serde_json::{Value, json};
use vodscribe_core::testkit::TempDir;

use crate::testkit::TestApp;

/// A buffer of the site's player: its number and its type, as the site
/// gave it.
type Feed = (u32, &'static str);

const PICTURE: Feed = (1, r#"video/mp4; codecs="avc1.640032""#);

/// What the app's player is at (player.rs `Seen`): whether one is open,
/// for which VOD, and for which session.
fn seen(app: &TestApp) -> Value {
    serde_json::to_value(app.state().player.seen()).unwrap()
}

/// An MP4 box: its size, its type, `body`.
fn mp4_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = ((8 + body.len()) as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    out
}

/// An fMP4 init segment: ftyp and moov.
fn mp4_init() -> Vec<u8> {
    [mp4_box(b"ftyp", b"isom"), mp4_box(b"moov", b"tracks")].concat()
}

/// An fMP4 segment: a moof and its mdat, `n` in it.
fn mp4_segment(n: u8) -> Vec<u8> {
    [mp4_box(b"moof", &[n]), mp4_box(b"mdat", &[n, n])].concat()
}

/// A site's player is taken over only in a build with the embed-takeover
/// feature, never a release; elsewhere the page leaves the sites' players
/// alone and no session opens (embed.rs `embed_takeover`: "Whether a site's
/// VOD's picture can be taken into the app's player (built with the
/// embed-takeover feature); the page leaves their players alone if not";
/// Cargo.toml: "Off in releases: YouTube's and Twitch's developer terms
/// forbid playing their video through another player").
#[test]
fn a_sites_player_is_taken_over_only_in_a_build_with_the_takeover() {
    let app = TestApp::new();
    let takeover = cfg!(feature = "embed-takeover");
    assert_eq!(app.invoke("embed_takeover", json!({})), Ok(json!(takeover)));
    if !takeover {
        let vod = app.add_site_vod("youtube", "noa29qsTc4g", "Day 3", 60.0);
        let refused = Err(json!("a site's player isn't taken over in this build"));
        assert_eq!(app.invoke("embed_open", json!({ "vodId": vod.id, "at": 0.0 })), refused);
        assert_eq!(app.invoke("embed_append", json!({})), refused);
        assert_eq!(seen(&app)["open"], false, "a player opened");
    }
}

/// Past the most a session keeps, the runs farthest from what's being
/// appended go, never the one appended to, till what's kept is well within
/// it (embed.rs `MOST`: "past it, the runs farthest from what's being
/// appended go (the site's player fetches them again if they're played)";
/// `Session::shed`). Four gigabytes can't be appended in a test, so the
/// session is told its runs hold all but a little of it.
#[test]
fn past_the_most_kept_the_farthest_runs_go() {
    let dir = TempDir::new("embed-shed");
    let mut s =
        Session { token: 1, duration: 10_000.0, dir: dir.to_path_buf(), buffers: HashMap::new(), files: 0, bytes: 0 };
    let video = PICTURE.1;
    // Runs at 0 s, 2000 s, 9000 s and 5000 s, then the one appended to, at 3000 s.
    s.append(1, video, true, true, &[(0.0, 2.0)], &[mp4_init(), mp4_segment(1)].concat()).unwrap();
    for (n, at) in [(2, 2000.0), (3, 9000.0), (4, 5000.0), (5, 3000.0)] {
        s.append(1, video, false, true, &[(at, at + 2.0)], &mp4_segment(n)).unwrap();
    }
    // What's kept is counted as its files hold.
    let size = |path: &Path| std::fs::metadata(path).unwrap().len();
    let runs = &s.buffers[&1].runs;
    assert!(runs.iter().all(|r| r.bytes == size(&r.path)));
    assert_eq!(s.bytes, runs.iter().map(|r| size(&r.path)).sum::<u64>());
    // As if each held an eighth of the most, and the session all of it
    // but the next append: that one reaches the most, and nothing goes.
    let paths: Vec<PathBuf> = runs.iter().map(|r| r.path.clone()).collect();
    for r in &mut s.buffers.get_mut(&1).unwrap().runs {
        r.bytes = MOST / 8;
    }
    let segment = mp4_segment(6);
    s.bytes = MOST - segment.len() as u64;
    s.append(1, video, false, true, &[(3002.0, 3004.0)], &segment).unwrap();
    assert_eq!(s.buffers[&1].runs.len(), 5, "runs went at the most, not past it");
    // The next goes past it: the two farthest from 3004 s go, at 9000 s and 0 s.
    s.append(1, video, false, true, &[(3004.0, 3006.0)], &segment).unwrap();
    let starts: Vec<f64> = s.buffers[&1].runs.iter().map(|r| r.start).collect();
    assert_eq!(starts, [2000.0, 5000.0, 3000.0]);
    assert!(!paths[0].exists() && !paths[2].exists(), "their files were kept");
    assert!(paths[1].exists() && paths[3].exists() && paths[4].exists());
    assert_eq!(s.bytes, MOST + segment.len() as u64 - MOST / 4);
    // The one appended to stays the one appended to.
    let current = s.buffers[&1].current.unwrap();
    assert_eq!(s.buffers[&1].runs[current].start, 3000.0);
}

/// The rest need a build with the takeover; CI runs them in one.
#[cfg(feature = "embed-takeover")]
mod takeover {
    use super::*;

    use vodscribe_core::db::Vod;
    use vodscribe_core::testkit::{Media, eventually};

    use crate::testkit::frame_number;

    const SOUND: Feed = (2, r#"audio/mp4; codecs="mp4a.40.2""#);
    const WEBM: Feed = (3, r#"video/webm; codecs="vp9""#);

    /// What the site's player appended to one of its buffers, as the page
    /// passes it on (embed.ts: its bytes, and what they are in headers).
    struct Append {
        feed: Feed,
        init: bool,
        ranges: Vec<(f64, f64)>,
        complete: bool,
        bytes: Vec<u8>,
    }

    impl Append {
        /// An init segment alone.
        fn init(feed: Feed, bytes: &[u8]) -> Self {
            Append { feed, init: true, ranges: Vec::new(), complete: true, bytes: bytes.to_vec() }
        }

        /// Media holding VOD times `ranges`, ending on a whole box or block.
        fn media(feed: Feed, ranges: &[(f64, f64)], bytes: &[u8]) -> Self {
            Append { feed, init: false, ranges: ranges.to_vec(), complete: true, bytes: bytes.to_vec() }
        }

        /// With an init segment before the media (`x-init`).
        fn after_init(self) -> Self {
            Append { init: true, ..self }
        }

        /// Ending partway through a box: the rest of it comes with the next.
        fn partway(self) -> Self {
            Append { complete: false, ..self }
        }
    }

    /// `append` passed on for session `token`, as the page does: its answer.
    fn append(app: &TestApp, token: u64, append: Append) -> Result<Value, Value> {
        let (buffer, kind) = append.feed;
        let mut headers = tauri::http::HeaderMap::new();
        for (name, value) in [
            ("x-session", token.to_string()),
            ("x-buffer", buffer.to_string()),
            ("x-type", kind.to_string()),
            ("x-init", if append.init { "1" } else { "0" }.to_string()),
            ("x-ranges", serde_json::to_string(&append.ranges).unwrap()),
            ("x-complete", if append.complete { "1" } else { "0" }.to_string()),
        ] {
            headers.insert(name, value.parse().unwrap());
        }
        let request = tauri::webview::InvokeRequest {
            cmd: "embed_append".into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            // The page's own origin, as the kit's `invoke` sends.
            url: if cfg!(windows) { "http://tauri.localhost" } else { "tauri://localhost" }.parse().unwrap(),
            body: tauri::ipc::InvokeBody::Raw(append.bytes),
            headers,
            invoke_key: tauri::test::INVOKE_KEY.into(),
        };
        let window = app.handle().get_webview_window("main").unwrap();
        tauri::test::get_ipc_response(&window, request).map(|body| body.deserialize::<Value>().unwrap())
    }

    /// A site's VOD of `duration` seconds.
    fn site_vod(app: &TestApp, duration: f64) -> Vod {
        app.add_site_vod("youtube", "noa29qsTc4g", "Day 3", duration)
    }

    /// Another site's VOD of a minute.
    fn other_site_vod(app: &TestApp) -> Vod {
        app.add_site_vod("twitch", "2901646294", "Day 4", 60.0)
    }

    /// The page's open of `vod`'s embed at `at`: its session's token.
    fn open(app: &TestApp, vod: &Vod, at: f64) -> u64 {
        app.invoke("embed_open", json!({ "vodId": vod.id, "at": at })).unwrap().as_u64().unwrap()
    }

    /// The page's seek to `time`, paused.
    fn seek(app: &TestApp, time: f64) -> Value {
        app.invoke("player_seek", json!({ "time": time, "play": false })).unwrap()
    }

    /// Whether the player, played from `time`, waits there for footage to come.
    fn waits_at(app: &TestApp, time: f64) -> bool {
        let status = app.invoke("player_seek", json!({ "time": time, "play": true })).unwrap();
        app.invoke("player_pause", json!({})).unwrap();
        status["waiting"].as_bool().unwrap()
    }

    /// Waits till the screen shows frame `n` of a fake video `width` wide.
    fn shows(app: &TestApp, n: u64, width: u32) {
        eventually(&format!("frame {n}, {width} wide, on the screen"), || {
            app.screen().showing().is_some_and(|f| f.width == width && frame_number(&f) == n)
        });
    }

    /// A fake video's picture, `width` wide (16:9, 30 fps, a minute), as an
    /// init segment.
    fn picture(width: u32) -> Vec<u8> {
        let mut media = Media::video(60.0);
        let video = media.video.as_mut().unwrap();
        (video.width, video.height) = (width, width * 9 / 16);
        media.to_bytes()
    }

    /// A fake video's sound alone (a minute), as an init segment.
    fn sound() -> Vec<u8> {
        Media::sound(60.0).to_bytes()
    }

    /// The site's player feeding `feed` from `init` for VOD times `from..to`,
    /// ending whole: the init segment (going on with the run it began, if it's
    /// the same), then the media.
    fn feeds(app: &TestApp, token: u64, feed: Feed, init: &[u8], (from, to): (f64, f64)) {
        append(app, token, Append::init(feed, init)).unwrap();
        append(app, token, Append::media(feed, &[(from, to)], b" ")).unwrap();
    }

    /// The files kept of what sites' players appended (`Paths::temp_embed`),
    /// each its extension and bytes, in order of their bytes.
    fn kept(app: &TestApp) -> Vec<(String, Vec<u8>)> {
        let Ok(sessions) = std::fs::read_dir(app.paths().temp_embed()) else { return Vec::new() };
        let mut files: Vec<(String, Vec<u8>)> = sessions
            .flatten()
            .flat_map(|session| std::fs::read_dir(session.path()).into_iter().flatten().flatten())
            .map(|file| {
                let ext = file.path().extension().unwrap_or_default().to_string_lossy().into_owned();
                (ext, std::fs::read(file.path()).unwrap_or_default())
            })
            .collect();
        files.sort_by(|a, b| a.1.cmp(&b.1));
        files
    }

    /// `files` (each an extension and its bytes) in `kept`'s order.
    fn files(files: &[(&str, Vec<u8>)]) -> Vec<(String, Vec<u8>)> {
        let mut files: Vec<_> = files.iter().map(|(ext, bytes)| (ext.to_string(), bytes.clone())).collect();
        files.sort_by(|a, b| a.1.cmp(&b.1));
        files
    }

    /// A WebM init segment: its EBML header.
    const WEBM_INIT: &[u8] = &[0x1a, 0x45, 0xdf, 0xa3, 0x80];

    /// A WebM cluster, `n` in it.
    fn webm_cluster(n: u8) -> Vec<u8> {
        vec![0x1f, 0x43, 0xb6, 0x75, 0x81, n]
    }

    /// An embed opens the app's player on its VOD, paused where the page
    /// asked, for its session alone, waiting where nothing has been
    /// appended (embed.rs `embed_open`: "Starts keeping what VOD `vod_id`'s
    /// embed appends, and opens the player on it, waiting for it, at `at`:
    /// the session's token"; module: "where nothing has been appended, the
    /// player waits").
    #[test]
    fn an_embed_opens_the_player_on_its_vod_paused_where_asked_waiting_for_its_appends() {
        let app = TestApp::new();
        app.with_screen();
        let vod = site_vod(&app, 60.0);
        let token = open(&app, &vod, 12.0);
        let seen = seen(&app);
        assert_eq!(seen["doc"], json!({ "kind": "vod", "id": vod.id }));
        assert_eq!(seen["embed"], token, "the session's own player");
        let status = app.invoke("player_pause", json!({})).unwrap();
        assert_eq!((status["time"].as_f64(), status["playing"].as_bool()), (Some(12.0), Some(false)));
        assert!(waits_at(&app, 12.0), "nothing appended, yet it played");
        assert!(waits_at(&app, 50.0));
    }

    /// One session at a time: an embed opened ends the one before, which
    /// had a token of its own, and what that one kept goes (embed.rs
    /// `embed_open`: "One at a time: an earlier one's files go"; module:
    /// "a session: what it kept goes when it closes").
    #[test]
    fn an_embed_opened_ends_the_one_before_and_what_it_kept_goes() {
        let app = TestApp::new();
        let first = open(&app, &site_vod(&app, 60.0), 0.0);
        feeds(&app, first, PICTURE, &picture(64), (0.0, 10.0));
        assert_eq!(kept(&app).len(), 1, "what the first one appended");
        let second = open(&app, &other_site_vod(&app), 0.0);
        assert_ne!(first, second, "each session has a token of its own");
        assert_eq!(seen(&app)["embed"], second);
        eventually("the first session's files gone", || kept(&app).is_empty());
    }

    /// What's appended for a session not open, a VOD's closed meanwhile
    /// among them, is dropped: it never reaches the next VOD's (embed.rs
    /// `embed_append`: "One for a session not open is dropped"; module:
    /// "each session has a token the page sends with each append and the
    /// close (a closing VOD's last appends never reach the next one's)").
    #[test]
    fn appends_for_a_session_not_open_are_dropped() {
        let app = TestApp::new();
        let first = open(&app, &site_vod(&app, 60.0), 0.0);
        let second = open(&app, &other_site_vod(&app), 0.0);
        eventually("the first session's files gone", || kept(&app).is_empty());
        // The first VOD's last appends, after the next one opened.
        assert_eq!(append(&app, first, Append::init(PICTURE, &picture(64))), Ok(Value::Null));
        assert_eq!(append(&app, first, Append::media(PICTURE, &[(0.0, 10.0)], b" ")), Ok(Value::Null));
        assert_eq!(append(&app, second + 1, Append::init(PICTURE, &picture(64))), Ok(Value::Null));
        assert!(kept(&app).is_empty(), "kept: {:?}", kept(&app).len());
        assert_eq!(seen(&app)["embed"], second);
        assert!(waits_at(&app, 5.0), "the next VOD's player plays the first one's appends");
    }

    /// Closing a session closes its player, and what it kept goes; a late
    /// close of an earlier VOD's session leaves the next one's be (embed.rs
    /// `embed_close`: "Ends session `token`, if it's the one open: its
    /// player closes, and what it kept goes"; player.rs `close_embed`: "a
    /// late close of an earlier VOD's leaves the next one's be"). The
    /// player's side of it is the player commands' (player/tests.rs
    /// `only_an_embed_session_changes_or_closes_its_player`).
    #[test]
    fn closing_an_embed_closes_its_player_and_what_it_kept_goes() {
        let app = TestApp::new();
        let first = open(&app, &site_vod(&app, 60.0), 0.0);
        let second = open(&app, &other_site_vod(&app), 0.0);
        feeds(&app, second, PICTURE, &picture(64), (0.0, 10.0));
        eventually("the first session's files gone", || kept(&app).len() == 1);
        // The first VOD's close, late.
        app.invoke("embed_close", json!({ "token": first })).unwrap();
        assert_eq!(seen(&app)["embed"], second, "the next VOD's player closed");
        assert_eq!(kept(&app).len(), 1, "the next VOD's appends went");
        append(&app, second, Append::media(PICTURE, &[(30.0, 32.0)], b" ")).unwrap();
        assert_eq!(kept(&app).len(), 2, "the next VOD's session ended");
        app.invoke("embed_close", json!({ "token": second })).unwrap();
        assert_eq!(seen(&app)["open"], false, "its player is still open");
        eventually("what it kept gone", || kept(&app).is_empty());
    }

    /// Appends that follow on, each starting within a quarter second of
    /// where the last ended, are one run, played from the VOD's time where
    /// it began; what the last one names plays once the next comes, or at
    /// once if it ends on a whole box (embed.rs module: "Each buffer's
    /// appends are kept as runs: media that follows on"; `FOLLOWS`: "An
    /// append starting this near where its run ends follows on"; `Run`'s
    /// `safe`: "as of the append before the last (the last one's media may
    /// still be coming in the next)"; `embed_append`: `x-complete`, "whether
    /// it ends on a whole box or block").
    #[test]
    fn appends_that_follow_on_play_as_one_run_from_where_it_began() {
        let app = TestApp::new();
        app.with_screen();
        let token = open(&app, &site_vod(&app, 60.0), 0.0);
        append(&app, token, Append::init(PICTURE, &picture(64))).unwrap();
        append(&app, token, Append::media(PICTURE, &[(0.0, 2.0)], b" ").partway()).unwrap();
        assert!(waits_at(&app, 1.0), "the first append's media may not all be in");
        append(&app, token, Append::media(PICTURE, &[(2.0, 4.0)], b" ").partway()).unwrap();
        seek(&app, 1.0);
        shows(&app, 30, 64);
        assert!(waits_at(&app, 3.0), "the last append's media may not all be in");
        // Ending on a whole box: all of it plays at once.
        append(&app, token, Append::media(PICTURE, &[(4.0, 6.0)], b" ")).unwrap();
        seek(&app, 5.0);
        shows(&app, 150, 64);
        // A fifth of a second on from its end: still the same run.
        append(&app, token, Append::media(PICTURE, &[(6.2, 8.0)], b" ")).unwrap();
        seek(&app, 7.0);
        shows(&app, 210, 64);
        assert_eq!(kept(&app).len(), 1, "one run, one file");
    }

    /// An append further on, or back, than a quarter second from where its
    /// run ends starts a run of its own, played from there; between, the
    /// player waits (embed.rs `FOLLOWS`; `Run`'s `start`: "VOD time where
    /// the file's own time is 0"; module: "where nothing has been appended,
    /// the player waits").
    #[test]
    fn an_append_further_on_or_back_starts_a_run_of_its_own_there() {
        let app = TestApp::new();
        app.with_screen();
        let token = open(&app, &site_vod(&app, 60.0), 0.0);
        feeds(&app, token, PICTURE, &picture(64), (0.0, 4.0));
        append(&app, token, Append::media(PICTURE, &[(30.0, 32.0)], b" ")).unwrap();
        seek(&app, 31.0);
        shows(&app, 30, 64);
        assert!(waits_at(&app, 10.0), "nothing appended at 0:10");
        assert!(waits_at(&app, 20.0), "nothing appended at 0:20");
        // Three tenths of a second on from its end: a run of its own.
        append(&app, token, Append::media(PICTURE, &[(32.3, 34.0)], b" ")).unwrap();
        seek(&app, 33.5);
        shows(&app, 36, 64);
        assert!(waits_at(&app, 32.1), "nothing appended at 0:32.1");
        // Back to 0:10.
        append(&app, token, Append::media(PICTURE, &[(10.0, 12.0)], b" ")).unwrap();
        seek(&app, 11.0);
        shows(&app, 30, 64);
        seek(&app, 2.0);
        shows(&app, 60, 64);
        assert_eq!(kept(&app).len(), 4, "a file each run");
    }

    /// The same init segment again (Twitch sends it before each segment)
    /// goes on with the run; a new one begins a new run, played as it says
    /// (embed.rs `Session::append`: "The same init again (Twitch sends it
    /// before each segment) goes on with the run; a new one begins a new
    /// run").
    #[test]
    fn the_same_init_segment_goes_on_with_its_run_and_a_new_one_begins_another() {
        let app = TestApp::new();
        app.with_screen();
        let token = open(&app, &site_vod(&app, 60.0), 0.0);
        feeds(&app, token, PICTURE, &picture(64), (0.0, 4.0));
        feeds(&app, token, PICTURE, &picture(64), (4.0, 6.0));
        seek(&app, 5.0);
        shows(&app, 150, 64);
        assert_eq!(kept(&app).len(), 1, "the same init began a run");
        // A new init (a picture of another size), right where the run ends.
        feeds(&app, token, PICTURE, &picture(32), (6.0, 8.0));
        seek(&app, 7.0);
        shows(&app, 30, 32);
        seek(&app, 5.0);
        shows(&app, 150, 64);
        assert_eq!(kept(&app).len(), 2);
    }

    /// An init segment and the media after it, appended together, are told
    /// apart: fMP4's up to its first moof, WebM's up to its first cluster;
    /// each run's file is its buffer's init segment, then its media (embed.rs
    /// `split_init`: "`bytes` split where the media after an init segment
    /// begins: fMP4's first moof, WebM's first cluster"; module: "media
    /// that follows on, after the buffer's init segment, one file each").
    #[test]
    fn an_init_segment_is_kept_apart_from_the_media_after_it() {
        let app = TestApp::new();
        let token = open(&app, &site_vod(&app, 60.0), 0.0);
        let (init, webm) = (mp4_init(), WEBM_INIT);
        append(
            &app,
            token,
            Append::media(PICTURE, &[(0.0, 2.0)], &[init.clone(), mp4_segment(1)].concat()).after_init(),
        )
        .unwrap();
        // Twitch: the same init with each segment.
        append(
            &app,
            token,
            Append::media(PICTURE, &[(2.0, 4.0)], &[init.clone(), mp4_segment(2)].concat()).after_init(),
        )
        .unwrap();
        append(&app, token, Append::media(PICTURE, &[(30.0, 32.0)], &mp4_segment(3))).unwrap();
        append(&app, token, Append::media(WEBM, &[(0.0, 2.0)], &[webm, &webm_cluster(1)].concat()).after_init())
            .unwrap();
        append(&app, token, Append::media(WEBM, &[(30.0, 32.0)], &webm_cluster(2))).unwrap();
        // A box of the 64-bit size form (its size 1, the size after its type).
        let large = [&mp4_box(b"ftyp", b"isom")[..], &[0, 0, 0, 1], b"moov", &22u64.to_be_bytes(), b"tracks"].concat();
        let feed = (4, PICTURE.1);
        append(&app, token, Append::media(feed, &[(0.0, 2.0)], &[large.clone(), mp4_segment(4)].concat()).after_init())
            .unwrap();
        append(&app, token, Append::media(feed, &[(30.0, 32.0)], &mp4_segment(5))).unwrap();
        assert_eq!(
            kept(&app),
            files(&[
                ("mp4", [init.clone(), mp4_segment(1), mp4_segment(2)].concat()),
                ("mp4", [init, mp4_segment(3)].concat()),
                ("webm", [webm, &webm_cluster(1)].concat()),
                ("webm", [webm, &webm_cluster(2)].concat()),
                ("mp4", [large.clone(), mp4_segment(4)].concat()),
                ("mp4", [large, mp4_segment(5)].concat()),
            ])
        );
    }

    /// Media appended to a buffer before its init segment is kept nowhere,
    /// and so is the rest of something not kept (embed.rs module: "Each
    /// buffer's appends are kept as runs: media that follows on, after the
    /// buffer's init segment"; `Session::append`: "The rest of something
    /// not kept (no run to add it to)").
    #[test]
    fn media_before_its_buffers_init_segment_is_not_kept() {
        let app = TestApp::new();
        let token = open(&app, &site_vod(&app, 60.0), 0.0);
        assert_eq!(append(&app, token, Append::media(PICTURE, &[(0.0, 2.0)], &mp4_segment(1))), Ok(Value::Null));
        append(&app, token, Append::init(PICTURE, &mp4_init())).unwrap();
        // The rest of a box, naming no times, with no run to go on with.
        append(&app, token, Append::media(PICTURE, &[], &mp4_segment(2))).unwrap();
        assert!(kept(&app).is_empty(), "kept {} files", kept(&app).len());
        append(&app, token, Append::media(PICTURE, &[(2.0, 4.0)], &mp4_segment(3))).unwrap();
        // Naming no times, it goes on with the run it follows.
        append(&app, token, Append::media(PICTURE, &[], &mp4_segment(4))).unwrap();
        assert_eq!(kept(&app), files(&[("mp4", [mp4_init(), mp4_segment(3), mp4_segment(4)].concat())]));
    }

    /// An init segment whose box sizes don't fit what's there (one that
    /// would wrap the offset, one past the end, one under a box's own head)
    /// is taken whole as the init, and the app goes on (embed.rs
    /// `split_init`: "a size that doesn't fit what's here (or is no size)
    /// ends it: all of it counts as the init").
    #[test]
    fn a_box_size_that_doesnt_fit_ends_the_init_rather_than_the_app() {
        let app = TestApp::new();
        let token = open(&app, &site_vod(&app, 60.0), 0.0);
        // A 64-bit size near the largest there is.
        let wraps = [&[0, 0, 0, 1][..], b"ftyp", &u64::MAX.wrapping_sub(7).to_be_bytes()].concat();
        let long = [&100u32.to_be_bytes()[..], b"moov"].concat();
        let short = [&3u32.to_be_bytes()[..], b"moov"].concat();
        // A 64-bit size whose eight bytes aren't all here.
        let cut = [&[0, 0, 0, 1][..], b"ftyp", &[0, 0, 0]].concat();
        for (buffer, init) in [(1, &wraps), (4, &long), (5, &short), (6, &cut)] {
            let feed = (buffer, PICTURE.1);
            assert_eq!(append(&app, token, Append::init(feed, init)), Ok(Value::Null));
            append(&app, token, Append::media(feed, &[(0.0, 2.0)], &mp4_segment(1))).unwrap();
        }
        assert_eq!(
            kept(&app),
            files(&[
                ("mp4", [wraps, mp4_segment(1)].concat()),
                ("mp4", [long, mp4_segment(1)].concat()),
                ("mp4", [short, mp4_segment(1)].concat()),
                ("mp4", [cut, mp4_segment(1)].concat()),
            ])
        );
    }

    /// A buffer whose type changes starts over as the new type: its next
    /// init segment read as that type's, a run of its own in a file of that
    /// type (embed.rs `Session::append`: "A buffer's type can change
    /// (SourceBuffer.changeType): what follows is read as the new one's").
    #[test]
    fn a_buffer_that_changes_type_starts_over_as_the_new_one() {
        let app = TestApp::new();
        let token = open(&app, &site_vod(&app, 60.0), 0.0);
        let media = |kind, ranges: &[(f64, f64)], bytes: &[u8]| Append::media((1, kind), ranges, bytes);
        append(&app, token, media(PICTURE.1, &[(0.0, 2.0)], &[mp4_init(), mp4_segment(1)].concat()).after_init())
            .unwrap();
        // Now WebM, from where the MP4 ended: its init up to the first cluster.
        let webm = [WEBM_INIT, &webm_cluster(1)].concat();
        append(&app, token, media(WEBM.1, &[(2.0, 4.0)], &webm).after_init()).unwrap();
        append(&app, token, media(WEBM.1, &[(30.0, 32.0)], &webm_cluster(2))).unwrap();
        assert_eq!(
            kept(&app),
            files(&[
                ("mp4", [mp4_init(), mp4_segment(1)].concat()),
                ("webm", webm),
                ("webm", [WEBM_INIT, &webm_cluster(2)].concat()),
            ])
        );
    }

    /// The player plays where both a picture run and a sound run reach for
    /// certain, and waits elsewhere, to the VOD's end (embed.rs `layout`:
    /// "where a picture run and a sound run (if the embed has a sound
    /// buffer) both reach for certain, they play; elsewhere the player
    /// waits"; module: "The runs are laid out as the player's timeline,
    /// picture with sound").
    #[test]
    fn the_player_plays_where_picture_and_sound_both_are_and_waits_elsewhere() {
        let app = TestApp::new();
        app.with_screen();
        let token = open(&app, &site_vod(&app, 20.0), 0.0);
        feeds(&app, token, PICTURE, &picture(64), (0.0, 10.0));
        feeds(&app, token, SOUND, &sound(), (0.0, 8.0));
        seek(&app, 5.0);
        shows(&app, 150, 64);
        assert!(!waits_at(&app, 5.0));
        assert!(waits_at(&app, 9.0), "the picture played without its sound");
        assert!(waits_at(&app, 15.0), "nothing appended at 0:15");
        assert_eq!(seek(&app, 100.0)["time"].as_f64(), Some(20.0), "the VOD's length");
        // Both to the end: it plays to the end, waiting nowhere.
        feeds(&app, token, PICTURE, &picture(64), (10.0, 20.0));
        feeds(&app, token, SOUND, &sound(), (8.0, 20.0));
        assert!(!waits_at(&app, 15.0));
        assert!(!waits_at(&app, 20.0), "it waits at its end");
    }

    /// A buffer is sound alone by its type, whatever the type's own name
    /// says (Twitch names its sound buffer video/mp4), and is otherwise a
    /// picture; a picture without its sound doesn't play (embed.rs
    /// `is_sound`: "A buffer's type that is sound alone"; `layout`).
    #[test]
    fn a_buffer_is_sound_alone_by_its_codecs() {
        let app = TestApp::new();
        let vod = site_vod(&app, 20.0);
        for (kind, is_sound) in [
            (r#"audio/mp4; codecs="mp4a.40.2""#, true),
            (r#"audio/webm; codecs="opus""#, true),
            (r#"video/mp4;codecs="mp4a.40.2""#, true),
            (r#"video/mp4; codecs="avc1.640032""#, false),
            (r#"video/webm; codecs="vp09.00.40.08""#, false),
            (r#"video/mp4; codecs="avc1.64001f, mp4a.40.2""#, false),
        ] {
            let token = open(&app, &vod, 0.0);
            feeds(&app, token, PICTURE, &picture(64), (0.0, 10.0));
            let init = if is_sound { sound() } else { picture(64) };
            feeds(&app, token, (2, kind), &init, (0.0, 8.0));
            assert_eq!(waits_at(&app, 9.0), is_sound, "{kind} read as sound: {}", !is_sound);
        }
    }

    /// The times an append names are kept within the VOD: from 0, to a
    /// minute past its length at most (embed.rs `Session::within`: "`ranges`
    /// as times of the VOD: in order, from 0, and not far past its end";
    /// `PAST_END`: "How far past the VOD's length an append's times may
    /// reach (a site's stream can run a little longer than it says)").
    #[test]
    fn the_times_an_append_names_stay_within_the_vod() {
        let app = TestApp::new();
        app.with_screen();
        let token = open(&app, &site_vod(&app, 100.0), 0.0);
        append(&app, token, Append::init(PICTURE, &picture(64))).unwrap();
        append(&app, token, Append::media(PICTURE, &[(-5.0, 2.0)], b" ")).unwrap();
        // Its run begins at 0:00, not before.
        seek(&app, 1.0);
        shows(&app, 30, 64);
        append(&app, token, Append::media(PICTURE, &[(150.0, 1e300)], b" ")).unwrap();
        let end = seek(&app, 1000.0);
        assert_eq!(end["time"].as_f64(), Some(160.0), "the VOD reaches past a minute after its end");
        // Times of no length, or backwards, are no times: a buffer's first media naming only those is kept nowhere.
        let other = (4, PICTURE.1);
        append(&app, token, Append::init(other, &picture(64))).unwrap();
        append(&app, token, Append::media(other, &[(5.0, 5.0), (7.0, 6.0)], b" ")).unwrap();
        assert_eq!(kept(&app).len(), 2, "kept a run of no length");
    }

    /// An append is its bytes; one sent another way is refused (embed.rs
    /// `embed_append`: "the body is its bytes").
    #[test]
    fn an_append_not_sent_as_bytes_is_refused() {
        let app = TestApp::new();
        let token = open(&app, &site_vod(&app, 60.0), 0.0);
        let sent = app.invoke("embed_append", json!({ "session": token }));
        assert_eq!(sent, Err(json!("An append comes as bytes.")));
    }
}
