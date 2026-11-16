//! Asking the file host: the set fetched and kept, and which VODs are asked about.

use super::*;
use crate::annotations::tests::file;
use crate::testkit::tools::{At, hold_at};
use crate::testkit::{Gate, eventually};

struct Library {
    db: Mutex<Db>,
    dir: crate::TempDir,
}

impl Library {
    fn new() -> Library {
        Library {
            db: Mutex::new(Db::open(Path::new(":memory:")).unwrap()),
            dir: crate::TempDir::new("annotations-filter"),
        }
    }

    fn settings(&self) -> PathBuf {
        self.dir.join("settings.json")
    }

    fn cache(&self) -> PathBuf {
        self.dir.join("annotations-filter.bin")
    }

    fn ask(&self, on: bool) {
        let settings = Settings { ask_for_annotations: on, ..Settings::default() };
        settings.save(&self.settings()).unwrap();
    }

    /// Adds YouTube VOD `id`, imported in `imported` (an ETag) if given.
    fn add(&self, id: &str, imported: Option<&str>) -> i64 {
        let db = self.db.lock().unwrap();
        let (vod, _) = db.add_vod(&crate::edits::tests::info(id)).unwrap();
        if let Some(etag) = imported {
            db.import_annotations(&file("youtube", id), None).unwrap();
            db.set_annotations_imported(vod.id, Some(etag)).unwrap();
        }
        vod.id
    }

    fn check(&self, base: &str) -> (Result<Checked>, Vec<i64>) {
        let mut changed = Vec::new();
        let checked = check_at(base, &self.db, &self.settings(), &self.cache(), Duration::ZERO, |id| changed.push(id));
        (checked, changed)
    }
}

const MD5_A: &str = "0123456789abcdef0123456789abcdef";
const MD5_B: &str = "fedcba9876543210fedcba9876543210";

/// The published set: `there` has its file in version MD5_A, `newer` in MD5_B.
fn published() -> Vec<u8> {
    Filter::of(&[
        exists_key("youtube", "there"),
        version_key("youtube", "there", MD5_A),
        exists_key("youtube", "same"),
        version_key("youtube", "same", MD5_A),
        exists_key("youtube", "newer"),
        version_key("youtube", "newer", MD5_B),
    ])
    .to_bytes()
}

fn file_host() -> (String, Arc<Mutex<Vec<String>>>) {
    let set = published();
    host(Arc::new(move |req: &str| {
        let line = req.lines().next().unwrap_or("");
        if line.contains("/annotations/filter.bin") {
            if req.to_ascii_lowercase().contains("if-none-match: \"0123456789abcdef0123456789abcdef\"") {
                return ("HTTP/1.1 304 Not Modified\r\nETag: \"0123456789abcdef0123456789abcdef\"".into(), Vec::new());
            }
            return ("HTTP/1.1 200 OK\r\nETag: \"0123456789abcdef0123456789abcdef\"".into(), set.clone());
        }
        let etag = if line.contains("/newer.json") { MD5_B } else { MD5_A };
        if ["/there.json", "/same.json", "/newer.json"].iter().any(|f| line.contains(f)) {
            return (format!("HTTP/1.1 200 OK\r\nETag: W/\"{etag}\""), Vec::new());
        }
        ("HTTP/1.1 404 Not Found".into(), Vec::new())
    }))
}

/// CHANGELOG.md, 0.7.0: "Your library is checked for annotations privately. At every start VODscribe downloads one
/// small list of every VOD with annotations, again only when it has changed, and checks your library against it on
/// your computer, so the file host never learns which VODs you have and is asked about one only when the list has
/// it"; `annotations_filter.rs` `judge`. The decision table, with the requests counted: absent, there and never
/// imported, imported in the version published, imported in an older one.
#[test]
fn only_the_vods_the_set_has_are_asked_about_once() {
    let lib = Library::new();
    lib.ask(true);
    let absent = lib.add("absent", None);
    let there = lib.add("there", None);
    let same = lib.add("same", Some(&format!("W/\"{MD5_A}\"")));
    let newer = lib.add("newer", Some(&format!("\"{MD5_A}\"")));
    let (base, asked) = file_host();

    let (checked, changed) = lib.check(&base);
    let checked = checked.unwrap();
    assert_eq!(
        *asked.lock().unwrap(),
        [
            "GET /annotations/filter.bin HTTP/1.1",
            "HEAD /annotations/youtube/there.json HTTP/1.1",
            "HEAD /annotations/youtube/newer.json HTTP/1.1",
        ]
    );
    assert_eq!(checked, Checked { asked: true, confirmed: 2, found: vec![there], newer: vec![newer] });
    assert_eq!(changed, [there, newer]);
    let db = lib.db.lock().unwrap();
    assert_eq!(db.vod(absent).unwrap().unwrap().annotations, Availability::Unknown);
    assert_eq!(db.vod(there).unwrap().unwrap().annotations, Availability::Available);
    assert!(db.vod(newer).unwrap().unwrap().annotations_newer);
    assert!(db.annotations_offer(newer).unwrap().newer);
    assert!(!db.vod(same).unwrap().unwrap().annotations_newer);
    assert!(db.annotations_offer(same).unwrap().up_to_date);
    drop(db);

    // Checked again (the next start, or the library's "Check for a VODscription"):
    // the set is asked for only if it changed, and what's known isn't asked
    // again, though it's still counted.
    asked.lock().unwrap().clear();
    let (checked, _) = lib.check(&base);
    assert_eq!(checked.unwrap(), Checked { asked: true, confirmed: 0, found: vec![there], newer: vec![newer] });
    assert_eq!(
        *asked.lock().unwrap(),
        ["GET /annotations/filter.bin HTTP/1.1 since \"0123456789abcdef0123456789abcdef\""]
    );
}

/// `annotations_filter.rs` `Checked` ("found by this check, or known from an earlier answer (their gift is
/// showing)"), `Judged::Found` and `Judged::Newer` ("not asked again"). A second check (the library's "Check all
/// VODs for VODscriptions", pressed during or after the one at start) counts what the first found, from the answers
/// kept, without asking about it again: a VOD whose gift is showing is found, one marked as having newer
/// annotations is newer.
#[test]
fn a_second_check_counts_what_the_first_found_without_asking_again() {
    let lib = Library::new();
    lib.ask(true);
    let there = lib.add("there", None);
    let newer = lib.add("newer", Some(&format!("\"{MD5_A}\"")));
    let (base, asked) = file_host();
    let (checked, _) = lib.check(&base);
    assert_eq!(checked.unwrap(), Checked { asked: true, confirmed: 2, found: vec![there], newer: vec![newer] });

    asked.lock().unwrap().clear();
    let (checked, changed) = lib.check(&base);
    assert_eq!(checked.unwrap(), Checked { asked: true, confirmed: 0, found: vec![there], newer: vec![newer] });
    assert!(changed.is_empty());
    assert_eq!(
        *asked.lock().unwrap(),
        ["GET /annotations/filter.bin HTTP/1.1 since \"0123456789abcdef0123456789abcdef\""]
    );
}

/// `annotations_filter.rs` `judge` ("in it, imported in version V and last seen in V, or in none since (the file
/// host said none was published): asked only when the set hasn't V"). A VOD whose imported annotations were taken
/// down (the file host said 404 when its row was checked) and published again later in another version: the check
/// asks about it and finds it newer. Published again in the version imported, it isn't asked about.
#[test]
fn annotations_taken_down_and_published_again_are_found_newer() {
    for (published, asked_about) in [(MD5_B, true), (MD5_A, false)] {
        let lib = Library::new();
        lib.ask(true);
        let back = lib.add("back", Some(&format!("\"{MD5_A}\"")));
        lib.db.lock().unwrap().set_annotations_found(back, &Found::None).unwrap();
        let set = Filter::of(&[exists_key("youtube", "back"), version_key("youtube", "back", published)]).to_bytes();
        let (base, asked) = host(Arc::new(move |req: &str| {
            if req.lines().next().unwrap_or("").contains("filter.bin") {
                return (format!("HTTP/1.1 200 OK\r\nETag: \"{MD5_A}\""), set.clone());
            }
            (format!("HTTP/1.1 200 OK\r\nETag: W/\"{published}\""), Vec::new())
        }));
        let (checked, _) = lib.check(&base);
        let checked = checked.unwrap();
        let mut want = vec!["GET /annotations/filter.bin HTTP/1.1"];
        if asked_about {
            want.push("HEAD /annotations/youtube/back.json HTTP/1.1");
            assert_eq!(checked, Checked { asked: true, confirmed: 1, found: vec![], newer: vec![back] });
        } else {
            assert_eq!(checked, Checked { asked: true, ..Checked::default() });
        }
        assert_eq!(*asked.lock().unwrap(), want, "published in {published}");
    }
}

/// `annotations_filter.rs`'s module doc ("Only a VOD the set says is probably there is asked about, with one HEAD
/// request"); `Checked::confirmed` ("HEAD requests made, one per VOD the set had that wasn't known already"). A set
/// that says "probably" when the file host says no, or says the same version: the answer is kept, nothing is
/// offered.
#[test]
fn a_false_positive_costs_one_request_and_offers_nothing() {
    let lib = Library::new();
    lib.ask(true);
    let set = Filter::of(&[exists_key("youtube", "gone"), exists_key("youtube", "same")]).to_bytes();
    let (base, asked) = host(Arc::new(move |req: &str| {
        let line = req.lines().next().unwrap_or("");
        match line {
            l if l.contains("filter.bin") => ("HTTP/1.1 200 OK\r\nETag: \"s\"".into(), set.clone()),
            l if l.contains("/same.json") => (format!("HTTP/1.1 200 OK\r\nETag: \"{MD5_A}\""), Vec::new()),
            _ => ("HTTP/1.1 404 Not Found".into(), Vec::new()),
        }
    }));
    let gone = lib.add("gone", None);
    let same = lib.add("same", Some(&format!("\"{MD5_A}\"")));
    let (checked, _) = lib.check(&base);
    assert_eq!(checked.unwrap(), Checked { asked: true, confirmed: 2, found: vec![], newer: vec![] });
    assert_eq!(asked.lock().unwrap().len(), 3);
    let db = lib.db.lock().unwrap();
    assert_eq!(db.vod(gone).unwrap().unwrap().annotations, Availability::None);
    assert!(db.annotations_offer(same).unwrap().up_to_date);
}

/// CHANGELOG.md, 0.7.0: "With asking turned off in Settings › Privacy, nothing is downloaded or asked";
/// `annotations_filter.rs` `check_library` ("Nothing at all when the user turned asking off").
#[test]
fn with_asking_turned_off_nothing_is_fetched() {
    let lib = Library::new();
    lib.ask(false);
    lib.add("there", None);
    let (base, asked) = file_host();
    let (checked, changed) = lib.check(&base);
    assert_eq!(checked.unwrap(), Checked::default());
    assert!(asked.lock().unwrap().is_empty() && changed.is_empty());
}

/// CHANGELOG.md, 0.7.0: "Check all VODs for annotations checks everything. Right-click any VOD from a site in the
/// library and pick it when you hear annotations just went up"; `check_library` ("fetching it again if it
/// changed"). Every check asks for the set (startup's, and the library's "Check for annotations"): no day has to
/// pass between two.
#[test]
fn every_check_asks_for_the_set_and_a_new_one_is_used() {
    let lib = Library::new();
    lib.ask(true);
    let there = lib.add("there", None);
    let empty = Filter::sized(0).to_bytes();
    let full = published();
    let round = Arc::new(Mutex::new(0));
    let now = round.clone();
    let (base, asked) = host(Arc::new(move |req: &str| {
        let line = req.lines().next().unwrap_or("");
        if line.contains("filter.bin") {
            // First none of them; then the set with "there" in it, published since.
            let set = if *now.lock().unwrap() == 0 { &empty } else { &full };
            let etag = *now.lock().unwrap();
            // MD5-shaped, as the host's are: any other ETag isn't kept.
            return (format!("HTTP/1.1 200 OK\r\nETag: \"{etag:0>32}\""), set.clone());
        }
        (format!("HTTP/1.1 200 OK\r\nETag: \"{MD5_A}\""), Vec::new())
    }));
    let (checked, _) = lib.check(&base);
    assert_eq!(checked.unwrap().confirmed, 0);
    *round.lock().unwrap() = 1;
    let (checked, _) = lib.check(&base);
    assert_eq!(checked.unwrap().found, [there]);
    assert_eq!(
        *asked.lock().unwrap(),
        [
            "GET /annotations/filter.bin HTTP/1.1",
            "GET /annotations/filter.bin HTTP/1.1 since \"00000000000000000000000000000000\"",
            "HEAD /annotations/youtube/there.json HTTP/1.1",
        ]
    );
}

/// `annotations_filter.rs` `CHECKING` ("One check at a time: a second waits, then finds the first's answers
/// kept") and `check_at` ("Turned off while it waited for another check: nothing is asked, not even the set").
/// Asking turned off while a check waits for another (the library's "Check for a VODscription" pressed during the
/// one at start): it asks the file host nothing, not even for the set, and says asking is off.
#[test]
fn a_check_waiting_for_another_asks_nothing_once_asking_is_turned_off() {
    let lib = Library::new();
    lib.ask(true);
    lib.add("there", None);
    let set = published();
    // The first check's question about "there" is answered only when the test says.
    let (release, held) = std::sync::mpsc::channel::<()>();
    let held = Mutex::new(held);
    let (base, asked) = host(Arc::new(move |req: &str| {
        let line = req.lines().next().unwrap_or("");
        if line.contains("filter.bin") {
            if req.to_ascii_lowercase().contains("if-none-match: \"set1\"") {
                return ("HTTP/1.1 304 Not Modified\r\nETag: \"set1\"".into(), Vec::new());
            }
            return ("HTTP/1.1 200 OK\r\nETag: \"set1\"".into(), set.clone());
        }
        let _ = held.lock().unwrap().recv();
        (format!("HTTP/1.1 200 OK\r\nETag: \"{MD5_A}\""), Vec::new())
    }));
    let (first, second) = std::thread::scope(|s| {
        let first = s.spawn(|| lib.check(&base).0);
        eventually("the first check asks about \"there\"", || {
            asked.lock().unwrap().iter().any(|l| l.starts_with("HEAD"))
        });
        // The second has found asking on, and goes to wait for the first.
        let turn = Gate::new();
        hold_at(At::CheckTurn, &lib.settings(), &turn);
        let second = s.spawn(|| lib.check(&base).0);
        turn.reached();
        lib.ask(false);
        turn.open();
        release.send(()).unwrap();
        (first.join().unwrap(), second.join().unwrap())
    });
    assert_eq!(first.unwrap().confirmed, 1);
    assert_eq!(second.unwrap(), Checked::default(), "the second check asked, with asking off");
    assert_eq!(
        *asked.lock().unwrap(),
        ["GET /annotations/filter.bin HTTP/1.1", "HEAD /annotations/youtube/there.json HTTP/1.1"]
    );
}

/// `annotations_filter.rs` `check_library` ("A failure to reach the file host is an error"); `db.rs`
/// `Availability::Unknown` ("Never asked, or the asking failed (offline, an error)").
#[test]
fn offline_is_an_error_that_changes_nothing() {
    let lib = Library::new();
    lib.ask(true);
    let there = lib.add("there", None);
    let gone = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
    let (checked, changed) = lib.check(&format!("http://{gone}"));
    assert!(checked.is_err());
    assert!(changed.is_empty());
    assert_eq!(lib.db.lock().unwrap().vod(there).unwrap().unwrap().annotations, Availability::Unknown);
}

/// `annotations_filter.rs` `Fetched::Missing` ("None published (404, 403, 410)"): nothing asked, and a set kept
/// from before is dropped.
#[test]
fn no_set_published_asks_nothing_more() {
    let lib = Library::new();
    lib.ask(true);
    lib.add("there", None);
    keep(&lib.cache(), &published(), Some("\"old\"")).unwrap();
    let (base, asked) = host(Arc::new(|_: &str| ("HTTP/1.1 404 Not Found".into(), Vec::new())));
    let (checked, _) = lib.check(&base);
    assert_eq!(checked.unwrap(), Checked { asked: true, ..Checked::default() });
    assert_eq!(asked.lock().unwrap().len(), 1);
    assert!(!lib.cache().exists());
}

/// `annotations_filter.rs`'s module doc ("A set whose bits would say yes to more than `MAX_FALSE_POSITIVES` of the
/// keys not in it ... is refused: broken or made that way, it would have the app ask about most of the library").
#[test]
fn a_set_that_says_yes_to_everything_is_refused() {
    let lib = Library::new();
    lib.ask(true);
    for id in ["a", "b", "c"] {
        lib.add(id, None);
    }
    let mut full = Filter::sized(0);
    full.data.fill(0xff);
    let set = full.to_bytes();
    let (base, asked) = host(Arc::new(move |req: &str| {
        if req.starts_with("GET") {
            return ("HTTP/1.1 200 OK\r\nETag: \"all\"".into(), set.clone());
        }
        ("HTTP/1.1 404 Not Found".into(), Vec::new())
    }));
    let (checked, _) = lib.check(&base);
    assert_eq!(*asked.lock().unwrap(), ["GET /annotations/filter.bin HTTP/1.1"]);
    assert!(checked.unwrap_err().to_string().contains("says yes to"));
    assert!(!lib.cache().exists(), "a refused set isn't kept");
    // Half its bits set with one hash: one VOD in two would be asked about.
    let mut half = Filter::sized(1000);
    half.hashes = 1;
    half.data.iter_mut().step_by(2).for_each(|b| *b = 0xff);
    assert!(Filter::parse(&half.to_bytes()).is_err());
    // A set sized for its keys, full to its 1%, is read.
    let keys: Vec<String> = (0..10_000).map(|i| format!("twitch:v{i}")).collect();
    assert!(Filter::parse(&Filter::of(&keys).to_bytes()).is_ok());
}

/// `annotations_filter.rs` `check_at` ("Turned off while the check went on (the pause included): nothing more is
/// asked"). Asking turned off during the pause between two VODs' requests: the second isn't asked about. (The
/// check is held at the pause's end, and the switch turned off then, so the order is certain.)
#[test]
fn asking_turned_off_during_the_pause_asks_nothing_more() {
    let lib = Library::new();
    lib.ask(true);
    lib.add("there", None);
    lib.add("newer", Some(&format!("\"{MD5_A}\"")));
    let (base, asked) = file_host();
    let pause = Gate::new();
    hold_at(At::CheckPause, &lib.settings(), &pause);
    let checked = std::thread::scope(|s| {
        let checking = s.spawn(|| check_at(&base, &lib.db, &lib.settings(), &lib.cache(), Duration::ZERO, |_| {}));
        pause.reached();
        lib.ask(false);
        pause.open();
        checking.join().unwrap()
    });
    assert_eq!(checked.unwrap().confirmed, 1);
    assert_eq!(
        *asked.lock().unwrap(),
        ["GET /annotations/filter.bin HTTP/1.1", "HEAD /annotations/youtube/there.json HTTP/1.1"]
    );
}

/// `annotations_filter.rs` `check_at` ("Taken out of the library meanwhile: not asked about"). A VOD taken out of
/// the library while a check goes on (it waits between two requests) isn't asked about after.
#[test]
fn a_vod_removed_during_a_check_isnt_asked_about() {
    let lib = Library::new();
    lib.ask(true);
    let first = lib.add("there", None);
    let second = lib.add("same", None);
    let (base, asked) = file_host();
    let mut changed = Vec::new();
    let checked = check_at(&base, &lib.db, &lib.settings(), &lib.cache(), Duration::ZERO, |id| {
        changed.push(id);
        if id == first {
            lib.db.lock().unwrap().remove_entry(second).unwrap();
        }
    });
    assert_eq!(checked.unwrap().found, [first]);
    assert_eq!(
        *asked.lock().unwrap(),
        ["GET /annotations/filter.bin HTTP/1.1", "HEAD /annotations/youtube/there.json HTTP/1.1"]
    );
    assert_eq!(changed, [first]);
}

/// `annotations_filter.rs` `same_version` ("the file host gives `W/"<md5>"` when it compresses its answer and
/// `"<md5>"` when it doesn't, for the same file"). A VOD imported in `"<md5>"` and last seen as `W/"<MD5>"` is
/// imported in the version it was seen in.
#[test]
fn a_version_seen_weak_or_in_capitals_is_the_one_imported() {
    let vod = Candidate {
        vod_id: 1,
        platform: "youtube".into(),
        platform_id: "x1".into(),
        available: Availability::Available,
        seen: Some(format!("W/\"{}\"", MD5_A.to_ascii_uppercase())),
        imported: Some(format!("\"{MD5_A}\"")),
    };
    // The set has the version imported: nothing to ask, nothing newer.
    let same = Filter::of(&[exists_key("youtube", "x1"), version_key("youtube", "x1", MD5_A)]);
    assert_eq!(judge(&same, &vod), Judged::Skip);
    // It has another: asked, as one seen in the version imported is.
    let other = Filter::of(&[exists_key("youtube", "x1"), version_key("youtube", "x1", MD5_B)]);
    assert_eq!(judge(&other, &vod), Judged::Ask);
}

/// `annotations.rs` `can_be_published` ("Twitch's, YouTube's and Kick's, the only ones the file host holds. The
/// file host is never asked of others"): the set's VODs are the library's, of the sites that publish.
#[test]
fn a_vod_of_another_site_is_never_asked_about() {
    let filter = Filter::of(&["dailymotion:x1", "youtube:x1"]);
    let vod = |platform: &str| Candidate {
        vod_id: 1,
        platform: platform.into(),
        platform_id: "x1".into(),
        available: Availability::Unknown,
        seen: None,
        imported: None,
    };
    assert_eq!(judge(&filter, &vod("dailymotion")), Judged::Skip);
    assert_eq!(judge(&filter, &vod("youtube")), Judged::Ask);
}

/// `annotations_filter.rs` `check_library` ("A failure to reach the file host is an error; what was answered before
/// it is kept"): the first VOD's answer stays on it, and the page heard of it, when asking about the second fails.
#[test]
fn a_failure_part_way_keeps_what_was_answered_before_it() {
    let lib = Library::new();
    lib.ask(true);
    let there = lib.add("there", None);
    let newer = lib.add("newer", Some(&format!("\"{MD5_A}\"")));
    let set = published();
    let (base, asked) = host(Arc::new(move |req: &str| {
        let line = req.lines().next().unwrap_or("");
        match line {
            l if l.contains("filter.bin") => (format!("HTTP/1.1 200 OK\r\nETag: \"{MD5_A}\""), set.clone()),
            l if l.contains("/there.json") => (format!("HTTP/1.1 200 OK\r\nETag: \"{MD5_A}\""), Vec::new()),
            _ => ("HTTP/1.1 503 Service Unavailable".into(), Vec::new()),
        }
    }));
    let (checked, changed) = lib.check(&base);
    assert!(checked.is_err(), "the second VOD's question failed");
    assert_eq!(asked.lock().unwrap().len(), 3, "the set, then both VODs");
    assert_eq!(changed, [there]);
    let db = lib.db.lock().unwrap();
    assert_eq!(db.vod(there).unwrap().unwrap().annotations, Availability::Available, "the first answer kept");
    let second = db.vod(newer).unwrap().unwrap();
    assert!(!second.annotations_newer, "nothing known of the second");
}

/// `annotations_filter.rs` `PAUSE` ("The pause between two VODs' HEAD requests in one check, so a library with many
/// VODs newly published never asks in a burst") and `check_library`, which keeps it: the check through the app's
/// own way in, on the kit's network, waits `PAUSE` between two VODs' requests; and held at each pause's end, it
/// has made one request more each time, none before the first VOD's.
#[test]
fn a_check_pauses_between_two_vods_requests_and_never_before_the_first() {
    let lib = Library::new();
    let paths = crate::Paths::new(lib.dir.join("data"));
    paths.ensure().unwrap();
    Settings { ask_for_annotations: true, ..Settings::default() }.save(&paths.settings()).unwrap();
    lib.add("pausea00001", None);
    lib.add("pauseb00001", None);
    let net = crate::testkit::Net::new();
    net.public("dl.vodscribe.com");
    let set = Filter::of(&[exists_key("youtube", "pausea00001"), exists_key("youtube", "pauseb00001")]).to_bytes();
    net.serves("https://dl.vodscribe.com/annotations/filter.bin", set);
    let heads = Arc::new(Mutex::new(Vec::new()));
    let at = heads.clone();
    for id in ["pausea00001", "pauseb00001"] {
        let at = at.clone();
        net.replies(&format!("https://dl.vodscribe.com/annotations/youtube/{id}.json"), move |asked| {
            at.lock().unwrap().push(std::time::Instant::now());
            (asked.method == "HEAD").then(|| crate::testkit::Reply::status(404))
        });
    }
    let checked = check_library(&lib.db, &paths, |_| {}).unwrap();
    assert_eq!(checked.confirmed, 2);
    let heads = heads.lock().unwrap();
    assert_eq!(heads.len(), 2);
    assert!(heads[1] - heads[0] >= PAUSE, "asked {:?} apart", heads[1] - heads[0]);
    drop(heads);

    // Three VODs asked about: held at each pause's end, one request more each time.
    let lib = Library::new();
    lib.ask(true);
    for id in ["there", "same", "newer"] {
        lib.add(id, None);
    }
    let (base, asked) = file_host();
    let heads = || asked.lock().unwrap().iter().filter(|l| l.starts_with("HEAD")).count();
    let first = Gate::new();
    hold_at(At::CheckPause, &lib.settings(), &first);
    let checked = std::thread::scope(|s| {
        let checking = s.spawn(|| check_at(&base, &lib.db, &lib.settings(), &lib.cache(), Duration::ZERO, |_| {}));
        first.reached();
        assert_eq!(heads(), 1, "the first VOD's request, then the pause");
        let second = Gate::new();
        hold_at(At::CheckPause, &lib.settings(), &second);
        first.open();
        second.reached();
        assert_eq!(heads(), 2, "the second VOD's request, then the pause");
        second.open();
        checking.join().unwrap()
    });
    assert_eq!(checked.unwrap().confirmed, 3);
    assert_eq!(heads(), 3);
}

/// `annotations_filter.rs` `load_kept` ("None if there's none that reads (then the next fetch asks for it
/// whole)"): a kept set that no longer reads isn't named to the file host, and the set it sends is used.
#[test]
fn a_kept_set_that_no_longer_reads_is_asked_for_whole() {
    let lib = Library::new();
    lib.ask(true);
    let there = lib.add("there", None);
    keep(&lib.cache(), b"not a set", Some(&format!("\"{MD5_A}\""))).unwrap();
    let (base, asked) = file_host();
    let (checked, _) = lib.check(&base);
    assert_eq!(checked.unwrap().found, [there]);
    assert_eq!(
        *asked.lock().unwrap(),
        ["GET /annotations/filter.bin HTTP/1.1", "HEAD /annotations/youtube/there.json HTTP/1.1"]
    );
    assert_eq!(std::fs::read(lib.cache()).unwrap(), published(), "the set sent is kept in its place");
}

/// `annotations_filter.rs` `annotations_candidates` and `check_at` ("Taken out of the library meanwhile: not asked
/// about"): a VOD taken out of the library before a check is no VOD of the library's, and isn't asked about.
#[test]
fn a_vod_taken_out_of_the_library_is_not_asked_about() {
    let lib = Library::new();
    lib.ask(true);
    let there = lib.add("there", None);
    lib.db.lock().unwrap().remove_entry(there).unwrap();
    let (base, asked) = file_host();
    let (checked, changed) = lib.check(&base);
    assert_eq!(checked.unwrap(), Checked { asked: true, ..Checked::default() });
    assert_eq!(*asked.lock().unwrap(), ["GET /annotations/filter.bin HTTP/1.1"]);
    assert!(changed.is_empty());
}

/// An empty set of `bytes` bytes in all, its header included.
fn set_of_bytes(bytes: usize) -> Vec<u8> {
    let data = vec![0; bytes - HEADER_BYTES];
    Filter { bits: data.len() as u64 * 8, hashes: HASHES, items: 0, data }.to_bytes()
}

/// Checks the library against a set of `bytes` bytes, a set the app takes but for its size: whether it was read
/// (and kept), having asked for nothing more.
fn checked_with_a_set_of(bytes: usize) -> bool {
    let lib = Library::new();
    lib.ask(true);
    lib.add("there", None);
    let set = set_of_bytes(bytes);
    assert!(Filter::parse(&set).is_ok(), "a set the app takes but for its size");
    let (base, asked) = host(Arc::new(move |_: &str| (format!("HTTP/1.1 200 OK\r\nETag: \"{MD5_A}\""), set.clone())));
    let (checked, _) = lib.check(&base);
    assert_eq!(*asked.lock().unwrap(), ["GET /annotations/filter.bin HTTP/1.1"], "{bytes} bytes");
    assert_eq!(lib.cache().exists(), checked.is_ok(), "kept only when read: {bytes} bytes");
    checked.is_ok()
}

/// `annotations_filter.rs` `MAX_FILTER_BYTES` ("The largest set read: 16 MB holds well over ten million VODs"): a
/// set under that size is read; one larger is refused, nothing more is asked and nothing is kept.
#[test]
fn a_set_larger_than_the_app_reads_is_refused_and_nothing_is_asked() {
    assert!(checked_with_a_set_of(MAX_FILTER_BYTES as usize - 1));
    assert!(!checked_with_a_set_of(MAX_FILTER_BYTES as usize + 1));
}

/// The largest published file the app reads (`annotations.rs` `MAX_FILE_BYTES`, private there).
const MAX_FILE_BYTES: usize = 64 * 1024 * 1024;

/// YouTube VOD `id`'s published file, padded to `bytes` bytes, on the kit's network; whether `download` read it.
fn downloaded_of(net: &crate::testkit::Net, id: &str, bytes: usize) -> Result<()> {
    let mut json = serde_json::to_string(&crate::annotations::tests::file("youtube", id)).unwrap();
    json.push_str(&" ".repeat(bytes - json.len()));
    net.serves(&format!("https://dl.vodscribe.com/annotations/youtube/{id}.json"), json);
    let (read, _) = annotations::download("youtube", id)?;
    assert_eq!(read.source.id, id);
    Ok(())
}

/// `annotations.rs` `MAX_FILE_BYTES` ("The largest published file read: a long VOD's is a few megabytes") and
/// `download`: a published file under that size is read; one larger is refused, saying the VODscription couldn't
/// be got.
#[test]
fn a_published_file_larger_than_the_app_reads_is_refused() {
    let net = crate::testkit::Net::new();
    net.public("dl.vodscribe.com");
    downloaded_of(&net, "underlimit1", MAX_FILE_BYTES - 1).unwrap();
    let err = downloaded_of(&net, "pastlimit01", MAX_FILE_BYTES + 1).unwrap_err();
    assert!(format!("{err:#}").starts_with("Couldn't get the annotations for this VOD"), "{err:#}");
}

/// `MAX_FILTER_BYTES` ("The largest set read") and `annotations.rs` `MAX_FILE_BYTES` ("The largest published file
/// read"): a set, or a published file, of exactly that size is read.
#[test]
#[ignore = "disagreement: a set or a published file of exactly the largest size read is refused"]
fn a_set_or_a_published_file_of_exactly_the_largest_size_is_read() {
    assert!(checked_with_a_set_of(MAX_FILTER_BYTES as usize), "the set");
    let net = crate::testkit::Net::new();
    net.public("dl.vodscribe.com");
    downloaded_of(&net, "atlimit0001", MAX_FILE_BYTES).unwrap();
}
