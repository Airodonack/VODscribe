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

/// The decision table, with the requests counted: absent, there and never
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

/// A second check (the library's "Check all VODs for VODscriptions",
/// pressed during or after the one at start) counts what the first
/// found, from the answers kept, without asking about it again: a VOD
/// whose gift is showing is found, one marked as having newer
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

/// A VOD whose imported annotations were taken down (the file host said
/// 404 when its row was checked) and published again later in another
/// version: the check asks about it and finds it newer. Published again
/// in the version imported, it isn't asked about.
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

/// A set that says "probably" when the file host says no, or says the same
/// version: the answer is kept, nothing is offered.
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

/// Every check asks for the set (startup's, and the library's "Check for
/// annotations"): no day has to pass between two.
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

/// Asking turned off while a check waits for another (the library's
/// "Check for a VODscription" pressed during the one at start): it asks the
/// file host nothing, not even for the set, and says asking is off.
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

/// No set published (yet): nothing asked, and a set kept from before is dropped.
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

/// A set that says yes to (nearly) every key: whether broken or made to,
/// it would have the app ask about every VOD in the library, which is
/// what the set is there to keep from the file host. It's refused.
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

/// Asking turned off during the pause between two VODs' requests: the
/// second isn't asked about. (The check is held at the pause's end, and
/// the switch turned off then, so the order is certain.)
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

/// A VOD taken out of the library while a check goes on (it waits
/// between two requests) isn't asked about after.
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

/// A VOD imported in `"<md5>"` and last seen as `W/"<MD5>"` (the host
/// compressed its answer) is imported in the version it was seen in.
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

/// The set's VODs are the library's listed ones, of the sites that publish.
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
