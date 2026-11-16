//! The specs of what's downloaded to transcribe a stretch (`audio_for`,
//! through `fetch::download_or_reprobe` on the kit's fake yt-dlp), of a
//! download so far (`partial_download`) and of the work files cleared after
//! (`remove_work_files`, `remove_section`). The tests after them are the
//! rest of `pipeline.rs`'s, as they were.

use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::Instant;

use super::*;
use crate::fetch::VodInfo;
use crate::playable::Playable;
use crate::testkit::{Ffmpeg, Gate, Library, Media, TempDir, said, yt_dlp};

/// A library whose site's VODs are served by the fake yt-dlp, with the fake
/// ffmpeg reading what a download holds.
struct Site {
    lib: Library,
    fake: crate::testkit::YtDlp,
    yt_dlp: YtDlp,
    ffmpeg: Ffmpeg,
    _bin: TempDir,
}

impl Site {
    fn new() -> Self {
        let bin = TempDir::new("pipeline-bin");
        let fake = crate::testkit::YtDlp::at(bin.join("yt-dlp"));
        let ffmpeg = Ffmpeg::at(bin.join("ffmpeg"));
        let yt_dlp = YtDlp { path: fake.path().into(), js_runtime: None };
        Site { lib: Library::new(), fake, yt_dlp, ffmpeg, _bin: bin }
    }

    /// A VOD of `platform` with `id`, 600 s long, in the library and served
    /// by the site, its details with `more` (a bitrate, `tbr`).
    fn vod_with(&self, platform: &str, id: &str, more: serde_json::Value) -> Vod {
        let vod = self.lib.add_site_vod(platform, id, "A talk", 600.0);
        let mut details = yt_dlp::details(platform, id, &vod.info.url, "A talk", 600.0);
        details.as_object_mut().unwrap().extend(more.as_object().unwrap().clone());
        self.fake.serves(details);
        vod
    }

    fn vod(&self, platform: &str, id: &str) -> Vod {
        self.vod_with(platform, id, serde_json::json!({}))
    }

    /// `audio_for` the stretch `start`..`end` of `vod`, offered `video` to
    /// keep as the worker offers it, with each fraction it reported.
    fn find(
        &mut self,
        vod: &Vod,
        start: f64,
        end: Option<f64>,
        video: Option<&Playable>,
    ) -> (Result<Source>, Vec<f64>) {
        let stretch = self.lib.db.request(vod.id, start, end).unwrap();
        let tools = Tools { yt_dlp: Some(&self.yt_dlp), ffmpeg: self.ffmpeg.path() };
        let mut seen = Vec::new();
        let no = AtomicBool::new(false);
        let video = video.map(|p| (p, 1080));
        let found = audio_for(&mut self.lib.db, vod, &stretch, &tools, &self.lib.paths, video, &no, |s, f| {
            assert_eq!(s, Status::Downloading);
            seen.push(f);
        });
        (found, seen)
    }

    /// `find`, on a thread of the scope, stopped when `cancel` is set; its
    /// fractions sent to `seen`.
    fn find_on<'s>(
        &'s mut self,
        scope: &'s std::thread::Scope<'s, '_>,
        vod: &'s Vod,
        stretch: Stretch,
        cancel: &'s AtomicBool,
        seen: mpsc::Sender<f64>,
    ) -> std::thread::ScopedJoinHandle<'s, Result<Source>> {
        let Site { lib, yt_dlp, ffmpeg, .. } = self;
        let Library { db, paths, .. } = lib;
        scope.spawn(move || {
            let tools = Tools { yt_dlp: Some(yt_dlp), ffmpeg: ffmpeg.path() };
            audio_for(db, vod, &stretch, &tools, paths, None, cancel, |_, f| {
                let _ = seen.send(f);
            })
        })
    }

    fn work(&self) -> PathBuf {
        self.lib.paths.work()
    }

    /// The files in the work folder, by name, sorted.
    fn work_files(&self) -> Vec<String> {
        names(&self.work())
    }
}

/// The names of the files in `dir`, sorted; none if it isn't there.
fn names(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut names: Vec<String> = entries.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

/// A stretch is downloaded as a section of its own, for it alone, its 0 at
/// its start, in Matroska (pipeline.rs `audio_for`: "a section of the VOD
/// for the stretch alone"; `Source::Section`: "A section downloaded for
/// the stretch alone, its 0 at `start`"; fetch.rs `Wanted::Section`: "in
/// Matroska, which can be read while it's written").
#[test]
fn a_stretch_is_downloaded_as_a_section_of_its_own_its_0_at_its_start() {
    let mut site = Site::new();
    let vod = site.vod("twitch", "2901646294");
    let (found, _) = site.find(&vod, 100.0, Some(130.0), None);
    let Source::Section { path, start } = found.unwrap() else { panic!("not a section") };
    assert_eq!((start, path.parent()), (100.0, Some(site.work().as_path())));
    let media = Media::read(&path).unwrap();
    assert_eq!(media.cut_from, Some((vod.info.url.clone(), 100.0)));
    assert_eq!((media.duration, media.container.as_str()), (30.0, "matroska"));
    assert!(!site.lib.paths.source_audio(vod.id, "m4a").exists(), "not the whole audio");
}

/// A stretch whose edge cuts through a line downloads from that line's
/// start, as it's transcribed again whole (pipeline.rs `audio_for`: "Lines
/// it would cut through are transcribed again whole.").
#[test]
fn a_stretch_cutting_through_a_line_downloads_from_the_lines_start() {
    let mut site = Site::new();
    let vod = site.vod("twitch", "2901646294");
    site.lib.says(vod.id, said(98.0, "a line said over the edge")); // 98 to 101
    let (found, _) = site.find(&vod, 100.0, Some(130.0), None);
    let Source::Section { path, start } = found.unwrap() else { panic!("not a section") };
    assert_eq!(start, 98.0);
    assert_eq!(Media::read(&path).unwrap().cut_from, Some((vod.info.url.clone(), 98.0)));
}

/// A stretch from the playhead to the end is downloaded as a section to
/// the VOD's end (CHANGELOG.md, 0.6.0: "Hold Transcribe to start at the
/// playhead. Pick 5 minutes, 15 minutes, an hour, to the end, or the whole
/// VOD"; pipeline.rs `audio_for`: "a section of the VOD for the stretch
/// alone").
#[test]
fn a_stretch_to_the_end_is_downloaded_as_a_section_to_the_vods_end() {
    let mut site = Site::new();
    let vod = site.vod("youtube", "noa29qsTc4g");
    let (found, _) = site.find(&vod, 300.0, None, None);
    let Source::Section { path, start } = found.unwrap() else { panic!("not a section") };
    assert_eq!(start, 300.0);
    assert_eq!(Media::read(&path).unwrap().duration, 300.0);
}

/// A request for all of a VOD downloads its whole audio straight away
/// (pipeline.rs `audio_for`: "A request for all of it goes straight here.";
/// fetch.rs `Wanted::Audio`).
#[test]
fn a_request_for_all_of_a_vod_downloads_its_whole_audio() {
    let mut site = Site::new();
    let vod = site.vod("youtube", "noa29qsTc4g");
    let (found, _) = site.find(&vod, 0.0, None, None);
    let path = site.lib.paths.source_audio(vod.id, "m4a");
    assert_eq!(found.unwrap(), Source::Whole { path: path.clone(), downloaded: true });
    let mut whole = Media::sound(600.0);
    whole.cut_from = Some((vod.info.url.clone(), 0.0));
    assert_eq!(Media::read(&path), Some(whole));
}

/// The VOD's whole audio, already here for another request, serves a
/// stretch with nothing downloaded (here, with no video downloader at all)
/// (pipeline.rs `audio_for`: "the VOD's whole audio, already in the work
/// dir for another request").
#[test]
fn the_whole_audio_already_here_serves_the_vods_other_stretches() {
    let mut site = Site::new();
    let vod = site.vod("youtube", "noa29qsTc4g");
    let Source::Whole { path, .. } = site.find(&vod, 0.0, None, None).0.unwrap() else { panic!("not whole") };
    // As the worker records it once downloaded.
    site.lib.db.set_audio_path(vod.id, Some(&path.to_string_lossy())).unwrap();
    let stretch = site.lib.db.request(vod.id, 300.0, Some(310.0)).unwrap();
    let tools = Tools { yt_dlp: None, ffmpeg: site.ffmpeg.path() };
    let no = AtomicBool::new(false);
    let found = audio_for(&mut site.lib.db, &vod, &stretch, &tools, &site.lib.paths, None, &no, |_, _| {});
    assert_eq!(found.unwrap(), Source::Whole { path, downloaded: false });
}

/// A stretch of a site's VOD with no video downloader installed says so
/// (fetch.rs `NO_YT_DLP`: "The message when yt-dlp is needed and missing";
/// pipeline.rs `Tools::yt_dlp`: "Not needed for local files.").
#[test]
fn without_the_video_downloader_a_sites_stretch_says_it_is_not_installed() {
    let mut site = Site::new();
    let vod = site.vod("youtube", "noa29qsTc4g");
    let stretch = site.lib.db.request(vod.id, 100.0, Some(130.0)).unwrap();
    let tools = Tools { yt_dlp: None, ffmpeg: site.ffmpeg.path() };
    let no = AtomicBool::new(false);
    let found = audio_for(&mut site.lib.db, &vod, &stretch, &tools, &site.lib.paths, None, &no, |_, _| {});
    assert_eq!(found.unwrap_err().to_string(), fetch::NO_YT_DLP);
}

/// A stop while a stretch's section downloads stops it, and the whole
/// audio isn't downloaded instead (pipeline.rs `audio_for`: "when the
/// section fails for any reason but a stop").
#[test]
fn a_stop_while_a_section_downloads_downloads_nothing_else() {
    let mut site = Site::new();
    let vod = site.vod("twitch", "2901646294");
    let begun = Gate::new();
    site.fake.holds(&begun);
    let stretch = site.lib.db.request(vod.id, 100.0, Some(130.0)).unwrap();
    let cancel = AtomicBool::new(false);
    let (seen, _) = mpsc::channel();
    let work = site.work();
    let found = std::thread::scope(|s| {
        let finding = site.find_on(s, &vod, stretch, &cancel, seen);
        begun.reached();
        cancel.store(true, Ordering::Relaxed);
        finding.join().unwrap()
    });
    assert!(found.is_err_and(|e| crate::is_canceled(&e)));
    let whole = format!("{}.m4a", vod.id);
    assert!(!names(&work).iter().any(|n| n.starts_with(&whole)), "{:?}", names(&work));
}

/// A download's details looked up over half an hour ago are looked up
/// again before it starts: here the old ones can't be read, and the new
/// look-up's download comes in (pipeline.rs `INFO_MAX_AGE`: "Stream URLs in
/// a saved info JSON expire …, so a download that starts later than this
/// probes the VOD again").
#[test]
fn details_looked_up_over_half_an_hour_ago_are_looked_up_again() {
    let mut site = Site::new();
    let vod = site.vod("youtube", "noa29qsTc4g");
    let info = site.lib.paths.source_info(vod.id);
    fs::write(&info, "expired").unwrap();
    let then = SystemTime::now() - Duration::from_secs(31 * 60);
    fs::File::options().write(true).open(&info).unwrap().set_modified(then).unwrap();
    let Source::Whole { path, .. } = site.find(&vod, 0.0, None, None).0.unwrap() else { panic!("not whole") };
    assert_eq!(Media::read(&path).unwrap().cut_from, Some((vod.info.url.clone(), 0.0)));
}

/// A download's details looked up less than half an hour ago are used as
/// they are, not looked up again: here they name a link of the session
/// they came from (pipeline.rs `INFO_MAX_AGE`: "a download that starts
/// later than this probes the VOD again"; fetch.rs `probe`: "so the later
/// download reuses it instead of resolving the URL a second time").
#[test]
fn details_looked_up_minutes_ago_are_used_as_they_are() {
    let mut site = Site::new();
    let vod = site.vod("youtube", "noa29qsTc4g");
    let session = format!("{}&session=earlier", vod.info.url);
    let looked_up = yt_dlp::details("youtube", "noa29qsTc4g", &session, "A talk", 600.0);
    fs::write(site.lib.paths.source_info(vod.id), looked_up.to_string()).unwrap();
    let Source::Whole { path, .. } = site.find(&vod, 0.0, None, None).0.unwrap() else { panic!("not whole") };
    assert_eq!(Media::read(&path).unwrap().cut_from, Some((session, 0.0)));
}

/// A VOD's whole audio stopped partway carries on from what's in when it's
/// asked again, rather than starting over: its progress goes on from where
/// it stopped (pipeline.rs `download`: "Downloads a VOD's whole audio,
/// resuming a partial download"; fetch.rs `download`: "An interrupted
/// download resumes … when called again with the same `out_stem`").
#[test]
fn a_whole_audio_stopped_partway_carries_on_from_where_it_stopped() {
    let mut site = Site::new();
    let vod = site.vod("youtube", "noa29qsTc4g");
    let halfway = Gate::new();
    site.fake.holds_at(0.5, &halfway);
    let stretch = site.lib.db.request(vod.id, 0.0, None).unwrap();
    let cancel = AtomicBool::new(false);
    let (seen, _) = mpsc::channel();
    let stopped = std::thread::scope(|s| {
        let finding = site.find_on(s, &vod, stretch, &cancel, seen);
        halfway.reached();
        cancel.store(true, Ordering::Relaxed);
        finding.join().unwrap()
    });
    assert!(stopped.is_err_and(|e| crate::is_canceled(&e)));
    site.fake.holds_at(0.5, &Gate::opened());
    let (found, seen) = site.find(&vod, 0.0, None, None);
    assert_eq!(found.unwrap(), Source::Whole { path: site.lib.paths.source_audio(vod.id, "m4a"), downloaded: true });
    assert_eq!(seen, [0.0, 0.75, 1.0], "on from the half that was in");
}

/// A VOD's whole audio already downloaded in full, not yet recorded (as
/// after a quit just as it ended), is used as it is, not downloaded again
/// (fetch.rs `download`: "a finished one is reused, when called again with
/// the same `out_stem`").
#[test]
fn a_whole_audio_downloaded_in_full_is_not_downloaded_again() {
    let mut site = Site::new();
    let vod = site.vod("youtube", "noa29qsTc4g");
    let first = site.find(&vod, 0.0, None, None).0.unwrap();
    let (again, seen) = site.find(&vod, 0.0, None, None);
    assert_eq!(again.unwrap(), first);
    assert_eq!(seen, [0.0], "nothing downloaded");
}

/// A stretch being downloaded says so, from nothing in, and how far its
/// download is as it comes (CHANGELOG.md, 0.6.0: "a part waiting or being
/// transcribed says how far it is"; pipeline.rs `audio_for`'s `report`).
#[test]
fn a_stretch_being_downloaded_says_so_and_how_far_it_is() {
    let mut site = Site::new();
    let vod = site.vod("youtube", "noa29qsTc4g");
    let begun = Gate::new();
    site.fake.holds(&begun);
    let stretch = site.lib.db.request(vod.id, 0.0, None).unwrap();
    let id = stretch.id;
    let other = Db::open_in(&site.lib.paths).unwrap();
    let cancel = AtomicBool::new(false);
    let (tx, seen) = mpsc::channel();
    let found = std::thread::scope(|s| {
        let finding = site.find_on(s, &vod, stretch, &cancel, tx);
        begun.reached();
        let asked = other.stretch(id).unwrap().unwrap();
        // Let go before asserting, so a failure fails rather than waits.
        begun.open();
        (asked, finding.join().unwrap())
    });
    let (asked, found) = found;
    assert_eq!((asked.status, asked.progress), (Status::Downloading, 0.0));
    assert!(found.is_ok());
    assert_eq!(seen.try_iter().collect::<Vec<_>>(), [0.0, 0.25, 0.5, 0.75, 1.0]);
}

/// A section's progress is gauged from its part file's growth against the
/// size its bitrate makes of its length, as yt-dlp says nothing of a
/// section ffmpeg downloads till it's done (fetch.rs `download`: "its
/// progress is gauged from the part files' growth against the size the
/// stream's bitrate makes of its length").
#[test]
fn a_sections_progress_is_gauged_from_its_part_file() {
    let mut site = Site::new();
    let vod = site.vod_with("twitch", "2901646294", serde_json::json!({ "tbr": 128.0 }));
    let halfway = Gate::new();
    site.fake.holds_at(0.5, &halfway);
    let stretch = site.lib.db.request(vod.id, 100.0, Some(130.0)).unwrap();
    let cancel = AtomicBool::new(false);
    let (tx, seen) = mpsc::channel();
    let found = std::thread::scope(|s| {
        let finding = site.find_on(s, &vod, stretch, &cancel, tx);
        halfway.reached();
        // Half of it is in, and the fake hasn't said so yet. Let go before
        // asserting, so a failure fails rather than waits.
        let until = Instant::now() + Duration::from_secs(5);
        let gauged = std::iter::from_fn(|| seen.recv_timeout(until.saturating_duration_since(Instant::now())).ok())
            .any(|f| f == 0.5);
        halfway.open();
        (gauged, finding.join().unwrap())
    });
    let (gauged, found) = found;
    assert!(gauged, "half of the section gauged while it was held");
    assert!(matches!(found, Ok(Source::Section { .. })));
}

/// A download under way is its part file as it is, and once it's finished
/// there's none (pipeline.rs `partial_download`: "The file VOD `vod`'s
/// download is being written to, as it is … None when no download has
/// begun, or it has finished"; CHANGELOG.md, 0.7.1 Fixes: "Show in folder
/// works during a download.").
#[test]
fn a_download_under_way_is_its_part_file_and_none_once_finished() {
    let mut site = Site::new();
    let vod = site.vod("youtube", "noa29qsTc4g");
    let halfway = Gate::new();
    site.fake.holds_at(0.5, &halfway);
    let stretch = site.lib.db.request(vod.id, 0.0, None).unwrap();
    let cancel = AtomicBool::new(false);
    let (tx, _) = mpsc::channel();
    let paths = site.lib.paths.clone();
    let part = paths.work().join(format!("{}.m4a.part", vod.id));
    let found = std::thread::scope(|s| {
        let finding = site.find_on(s, &vod, stretch, &cancel, tx);
        halfway.reached();
        let partial = partial_download(&paths, &vod);
        halfway.open();
        assert_eq!(partial, Some(part.clone()));
        finding.join().unwrap()
    });
    assert!(found.is_ok());
    assert_eq!(partial_download(&paths, &vod), None);
}

/// All of a VOD from a site without its own player downloads its video,
/// kept to play here when it plays on this computer (pipeline.rs
/// `download`: "or, with `video`, its video to keep (sites without their
/// own player)"; CHANGELOG.md, 0.3.0: "Their video, or audio, is kept on
/// your computer to play in VODscribe, in a format your computer plays").
#[test]
fn all_of_a_vod_from_a_site_without_its_own_player_keeps_its_video() {
    let mut site = Site::new();
    let vod = site.vod("rumble", "v1");
    let (found, _) = site.find(&vod, 0.0, None, Some(&Playable::default()));
    let path = site.lib.paths.media().join(format!("{}.mp4", vod.id));
    assert_eq!(found.unwrap(), Source::Whole { path: path.clone(), downloaded: true });
    assert!(Media::read(&path).unwrap().video.is_some());
    assert_eq!(site.lib.db.media_path(vod.id).unwrap(), Some(path.to_string_lossy().into_owned()));
}

/// A video downloaded that doesn't play on this computer isn't kept: it's
/// transcribed from the work folder and goes with the work files (pipeline.rs
/// `download`: "Kept to play only if what arrived plays here … Otherwise
/// it's still transcribed, from the work dir, and goes with the rest of the
/// work files after").
#[test]
fn a_video_downloaded_that_does_not_play_here_is_not_kept() {
    let mut site = Site::new();
    let vod = site.vod("rumble", "v1");
    let plays_nothing = Playable { video: Vec::new(), audio: Vec::new() };
    let (found, _) = site.find(&vod, 0.0, None, Some(&plays_nothing));
    let path = site.lib.paths.source_audio(vod.id, "mp4");
    assert_eq!(found.unwrap(), Source::Whole { path: path.clone(), downloaded: true });
    assert_eq!(site.lib.db.media_path(vod.id).unwrap(), None);
    assert_eq!(names(&site.lib.paths.media()), Vec::<String>::new());
    remove_work_files(&site.work(), vod.id);
    assert!(!path.exists(), "gone with the work files");
}

/// A stretch of a VOD from a site without its own player downloads its
/// section's sound, never the video, even offered one to keep (worker.rs
/// `cancel`: "A stretch's own section never downloads the video: a
/// download there is "Keep video"'s, or Play's").
#[test]
fn a_stretch_of_a_site_without_its_own_player_never_downloads_the_video() {
    let mut site = Site::new();
    let vod = site.vod("rumble", "v1");
    let (found, _) = site.find(&vod, 100.0, Some(130.0), Some(&Playable::default()));
    assert!(matches!(found, Ok(Source::Section { start: 100.0, .. })));
    assert_eq!(site.lib.db.media_path(vod.id).unwrap(), None);
    assert_eq!(names(&site.lib.paths.media()), Vec::<String>::new());
}

/// All of a VOD from YouTube, Twitch or Kick, whose own players play it,
/// downloads its audio alone, never its video, even offered one to keep
/// (pipeline.rs `keeps_video`: "any site but those with embeds of their
/// own"; `download`: "its video to keep (sites without their own player)").
#[test]
fn all_of_a_vod_from_a_site_with_its_own_player_downloads_its_audio_alone() {
    let mut site = Site::new();
    for (platform, id) in [("youtube", "noa29qsTc4g"), ("twitch", "2901646294"), ("kick", "k1")] {
        let vod = site.vod(platform, id);
        let (found, _) = site.find(&vod, 0.0, None, Some(&Playable::default()));
        let path = site.lib.paths.source_audio(vod.id, "m4a");
        assert_eq!(found.unwrap(), Source::Whole { path, downloaded: true }, "{platform}");
        assert_eq!(site.lib.db.media_path(vod.id).unwrap(), None, "{platform}");
    }
    assert_eq!(names(&site.lib.paths.media()), Vec::<String>::new());
}

/// A VOD's work files all go once it's done with, its details, its whole
/// audio, its sections and what was left of a download stopped partway,
/// and no other VOD's (pipeline.rs `remove_work_files`: "Deletes everything
/// in `work_dir` belonging to a VOD: its info JSON, the audio, and any
/// partial download fragments"; CHANGELOG.md, 0.3.0 Fixes: "Stopping or
/// removing a download could leave partial files behind.").
#[test]
fn a_vods_work_files_all_go_and_no_other_vods() {
    let mut site = Site::new();
    let (vod, other) = (site.vod("youtube", "noa29qsTc4g"), site.vod("twitch", "2901646294"));
    site.find(&vod, 0.0, None, None).0.unwrap();
    site.find(&vod, 100.0, Some(130.0), None).0.unwrap();
    site.find(&other, 0.0, None, None).0.unwrap();
    let begun = Gate::new();
    site.fake.holds(&begun);
    let stretch = site.lib.db.request(vod.id, 200.0, Some(230.0)).unwrap();
    let cancel = AtomicBool::new(false);
    let (tx, _) = mpsc::channel();
    let stopped = std::thread::scope(|s| {
        let finding = site.find_on(s, &vod, stretch, &cancel, tx);
        begun.reached();
        cancel.store(true, Ordering::Relaxed);
        finding.join().unwrap()
    });
    assert!(stopped.is_err());
    let mine = site.work_files().into_iter().filter(|n| n.starts_with(&format!("{}.", vod.id))).count();
    assert_eq!(mine, 4, "details, whole audio, a section, a part: {:?}", site.work_files());
    remove_work_files(&site.work(), vod.id);
    let others = [format!("{}.info.json", other.id), format!("{}.m4a", other.id)];
    assert_eq!(site.work_files(), others);
}

/// A request's own section goes when it's done with, finished or part of
/// one, and nothing else of its VOD (pipeline.rs `remove_section`:
/// "Deletes request `stretch_id`'s section of audio, and any part of one";
/// worker.rs `cancel`: "Its section of audio goes").
#[test]
fn a_requests_section_goes_and_nothing_else_of_its_vod() {
    let mut site = Site::new();
    let vod = site.vod("youtube", "noa29qsTc4g");
    let Source::Section { path: done, .. } = site.find(&vod, 100.0, Some(130.0), None).0.unwrap() else {
        panic!("not a section")
    };
    // The request `find` made, asked for again: the same one.
    let finished = site.lib.db.request(vod.id, 100.0, Some(130.0)).unwrap();
    site.find(&vod, 0.0, None, None).0.unwrap();
    let begun = Gate::new();
    site.fake.holds(&begun);
    let stopped = site.lib.db.request(vod.id, 200.0, Some(230.0)).unwrap();
    let cancel = AtomicBool::new(false);
    let (tx, _) = mpsc::channel();
    std::thread::scope(|s| {
        let finding = site.find_on(s, &vod, stopped.clone(), &cancel, tx);
        begun.reached();
        cancel.store(true, Ordering::Relaxed);
        assert!(finding.join().unwrap().is_err());
    });
    let before = site.work_files();
    remove_section(&site.lib.paths, vod.id, finished.id);
    assert!(!done.exists());
    let less_one: Vec<_> =
        before.iter().filter(|n| **n != done.file_name().unwrap().to_string_lossy()).cloned().collect();
    assert_eq!(site.work_files(), less_one);
    remove_section(&site.lib.paths, vod.id, stopped.id);
    let left = [format!("{}.info.json", vod.id), format!("{}.m4a", vod.id)];
    assert_eq!(site.work_files(), left, "the details and the whole audio stay");
}

/// A YouTube chat still downloading in the work folder isn't cleared with
/// a VOD's work files when its transcription ends first (lib.rs
/// `Paths::temp_chat`: "Not named `<id>.…`: `pipeline::remove_work_files`
/// clears those when a transcription ends, which may be while the chat is
/// still downloading"; CHANGELOG.md, 0.3.0 Fixes: "A YouTube chat could fail
/// when its transcription finished first.").
#[test]
fn a_youtube_chat_still_downloading_is_not_cleared_with_the_work_files() {
    let mut site = Site::new();
    let vod = site.vod("youtube", "noa29qsTc4g");
    site.find(&vod, 0.0, None, None).0.unwrap();
    let chat = PathBuf::from(format!("{}.live_chat.json.part", site.lib.paths.temp_chat(vod.id).display()));
    fs::write(&chat, "{}\n").unwrap();
    remove_work_files(&site.work(), vod.id);
    assert!(chat.is_file());
}

/// A VOD's video is kept for any site but those whose own players play it,
/// and never for a file on this computer (pipeline.rs `keeps_video`: "any
/// site but those with embeds of their own (and files on this computer,
/// played in place)").
#[test]
fn videos_are_kept_for_sites_without_players_of_their_own() {
    for platform in SITE_PLAYERS {
        assert!(!keeps_video(platform), "{platform}");
    }
    assert!(!keeps_video(crate::local::PLATFORM), "files are played in place");
    assert!(keeps_video("rumble") && keeps_video("dailymotion"));
}

/// The page keeps its own copies of these (it can't ask for them before
/// drawing): they must say what the app does.
#[test]
fn the_page_agrees_on_site_players_and_the_default_height() {
    let page = include_str!("../../../../apps/desktop/src/lib/platform.ts");
    let players = SITE_PLAYERS.map(|p| format!("{p:?}")).join(", ");
    assert!(page.contains(&format!("[{players}].includes(vod.platform)")), "hasSitePlayer lists {players}");
    let height = crate::settings::DEFAULT_VIDEO_HEIGHT;
    assert!(page.contains(&format!("export const DEFAULT_HEIGHT = {height};")), "DEFAULT_HEIGHT is {height}");
}

/// A download's pieces go by their name, not another's that only starts
/// the same (pipeline.rs `remove_work_prefix`: "not another's whose name
/// only starts the same").
#[test]
fn work_files_go_by_their_prefix_only() {
    let dir = crate::TempDir::new("prefix");
    for name in ["clip-12.info.json", "clip-12-1000-2000.mp4.part", "clip-123.info.json", "clip-12x"] {
        fs::write(dir.join(name), b"").unwrap();
    }
    remove_work_prefix(&dir, "clip-12");
    let mut left: Vec<_> =
        fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    left.sort();
    assert_eq!(left, ["clip-123.info.json", "clip-12x"]);
}

/// A video without sound is done at once, with no lines: no worker runs
/// (the one given doesn't exist). Skipped where ffmpeg isn't installed.
#[test]
fn a_video_without_sound_is_done_without_transcribing() {
    let Ok(ffmpeg) = which::which("ffmpeg") else {
        eprintln!("skipped: no ffmpeg on PATH");
        return;
    };
    let dir = crate::TempDir::new("silent");
    let file = dir.join("silent.mp4");
    let made = std::process::Command::new(&ffmpeg)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc=d=1:s=160x90"])
        .arg(&file)
        .status()
        .unwrap();
    assert!(made.success());
    let mut db = Db::open(Path::new(":memory:")).unwrap();
    let (vod, _) = db.add_vod(&crate::local::probe(&ffmpeg, &file).unwrap()).unwrap();
    let stretch = db.request(vod.id, 0.0, None).unwrap();
    let none = WorkerSpec {
        program: dir.join("no-such-worker"),
        model_id: "none",
        model_dir: dir.join("none"),
        provider: "cpu",
        batch: 1,
        lib_dirs: Vec::new(),
    };
    let transcription = Transcription { worker: none, fallback: None };
    let mut seen = Vec::new();
    let (cancel, mut notice) = (AtomicBool::new(false), |_: String| {});
    let mut report = |s, _| seen.push(s);
    let audio = Audio { path: file, offset: 0.0 };
    transcribe(&mut db, &vod, &stretch, &ffmpeg, &audio, &transcription, &cancel, &mut report, &mut notice).unwrap();
    assert_eq!(seen, [Status::Done]);
    let vod = db.vod(vod.id).unwrap().unwrap();
    assert_eq!((vod.status, vod.transcribed), (Status::Done, crate::stretches::Transcribed::All));
    assert_eq!(db.transcribed_until(vod.id).unwrap(), 0.0, "no lines");
}

#[test]
fn fills_in_a_channel_from_the_info_kept() {
    let root = crate::TempDir::new("fill-channels");
    let paths = Paths::new(root.to_path_buf());
    fs::create_dir_all(paths.work()).unwrap();
    let db = Db::open(Path::new(":memory:")).unwrap();
    let add = |id: &str| {
        let info = VodInfo { platform: "youtube".into(), ..crate::edits::tests::info(id) };
        db.add_vod(&info).unwrap().0
    };
    let (kept, other, none) = (add("aaa"), add("bbb"), add("ccc"));
    let info = |id: &str| {
        serde_json::json!({ "id": id, "extractor_key": "Youtube", "webpage_url": format!("https://www.youtube.com/watch?v={id}"),
            "uploader_id": "@quill", "uploader_url": "https://www.youtube.com/@quill" })
        .to_string()
    };
    fs::write(paths.source_info(kept.id), info("aaa")).unwrap();
    // Another video's info under this VOD's name isn't its.
    fs::write(paths.source_info(other.id), info("zzz")).unwrap();
    fill_channels(&db, &paths, &db.vods().unwrap());
    let channel = |id: i64| {
        let info = db.vod(id).unwrap().unwrap().info;
        (info.channel_login, info.channel_url)
    };
    assert_eq!(channel(kept.id), (Some("@quill".into()), Some("https://www.youtube.com/@quill".into())));
    assert_eq!(channel(other.id), (None, None));
    assert_eq!(channel(none.id), (None, None), "nothing kept, nothing known");
}

/// Refresh details takes the channel's login and page from what the
/// site says now (a stand-in yt-dlp answering with `answer` beside it),
/// only for the VOD's own video, and only what's named; the VOD's own
/// info JSON, which the pipeline reads, stays as it was, and nothing of
/// the look-up is left in the work folder.
#[test]
fn refreshing_details_takes_the_channel_the_site_names() {
    let root = crate::TempDir::new("refresh-details");
    let paths = Paths::new(root.join("data"));
    fs::create_dir_all(paths.work()).unwrap();
    let answer = root.join("answer.json");
    let path = root.join("yt-dlp");
    fs::write(&path, format!("#!/bin/sh\ncat '{}' || exit 1\n", answer.display())).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    let yt_dlp = YtDlp { path, js_runtime: None };
    let db = std::sync::Mutex::new(Db::open(Path::new(":memory:")).unwrap());
    let info = VodInfo {
        platform: "youtube".into(),
        url: "https://www.youtube.com/watch?v=aaa".into(),
        title: "As added".into(),
        ..crate::edits::tests::info("aaa")
    };
    let vod = db.lock().unwrap().add_vod(&info).unwrap().0;
    // What the pipeline found when it was added, and reads for a download.
    fs::write(paths.source_info(vod.id), b"{\"kept\":true}").unwrap();
    let say = |id: &str, channel: serde_json::Value| {
        let mut json = serde_json::json!({ "id": id, "extractor_key": "Youtube", "title": "Renamed since",
            "webpage_url": format!("https://www.youtube.com/watch?v={id}"), "duration": 99.0 });
        json.as_object_mut().unwrap().extend(channel.as_object().unwrap().clone());
        fs::write(&answer, json.to_string()).unwrap();
    };
    let no = AtomicBool::new(false);
    let refresh = || refresh_details(&db, &yt_dlp, &paths, vod.id, &no);
    let channel = || {
        let info = db.lock().unwrap().vod(vod.id).unwrap().unwrap().info;
        (info.channel_login, info.channel_url)
    };

    // Nothing named, nothing guessed: what's kept stays as it is.
    say("aaa", serde_json::json!({ "channel": "Quill" }));
    refresh().unwrap();
    assert_eq!(channel(), (None, None));

    // Another video's info isn't this VOD's.
    say("zzz", serde_json::json!({ "uploader_id": "@other", "uploader_url": "https://www.youtube.com/@other" }));
    assert!(refresh().unwrap_err().to_string().contains("another video"));
    assert_eq!(channel(), (None, None));

    // Its own: the channel's login and page, and nothing else of it.
    say("aaa", serde_json::json!({ "uploader_id": "@quill", "uploader_url": "https://www.youtube.com/@quill" }));
    let refreshed = refresh().unwrap();
    assert_eq!(
        (refreshed.info.channel_login.as_deref(), refreshed.info.channel_url.as_deref()),
        (Some("@quill"), Some("https://www.youtube.com/@quill"))
    );
    assert_eq!((refreshed.info.title.as_str(), refreshed.info.duration), ("As added", info.duration));

    // A page off the VOD's site isn't one; the login named still is. And
    // what the site no longer names stays.
    say("aaa", serde_json::json!({ "uploader_id": "@quill2", "uploader_url": "https://evil.example/@quill" }));
    refresh().unwrap();
    assert_eq!(channel(), (Some("@quill2".into()), Some("https://www.youtube.com/@quill".into())));

    // The site can't be asked: an error, and nothing changes.
    fs::remove_file(&answer).unwrap();
    assert!(refresh().is_err());
    assert_eq!(channel(), (Some("@quill2".into()), Some("https://www.youtube.com/@quill".into())));

    assert_eq!(fs::read(paths.source_info(vod.id)).unwrap(), b"{\"kept\":true}");
    let left: Vec<_> = fs::read_dir(paths.work()).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(left, [paths.source_info(vod.id).file_name().unwrap()], "nothing of the look-ups left");

    // A file on this computer has no site to ask.
    let file = VodInfo { platform: crate::local::PLATFORM.into(), ..crate::edits::tests::info("f") };
    let file = db.lock().unwrap().add_vod(&file).unwrap().0;
    assert!(refresh_details(&db, &yt_dlp, &paths, file.id, &no).is_err());
}

/// Kept videos of VODs no longer in the library go at the start, and those
/// of VODs still there stay (pipeline.rs `tidy_media`: "Files of VODs still
/// there stay, as a paused download carries on from its parts"; `remove_media`).
#[test]
fn tidies_media_of_vods_gone() {
    let root = crate::TempDir::new("tidy-media");
    let paths = Paths::new(root.to_path_buf());
    let media = paths.media();
    fs::create_dir_all(&media).unwrap();
    for name in
        ["3.mp4", "A title [4].mp4.part-Frag7.part", "29.mp4.part-Frag65.part", "Gone [30].m4a", "notes.txt", "77"]
    {
        fs::write(media.join(name), b"x").unwrap();
    }
    // Folders stay, even named as a VOD's file would be.
    fs::create_dir_all(media.join("31.mp4")).unwrap();
    fs::create_dir_all(media.join("Folder [32]")).unwrap();
    tidy_media(&paths, &[3, 4]);
    let mut left: Vec<String> =
        fs::read_dir(&media).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    left.sort();
    assert_eq!(left, ["3.mp4", "31.mp4", "77", "A title [4].mp4.part-Frag7.part", "Folder [32]", "notes.txt"]);
    // And a VOD's own go, named either way, but not another's with its digits.
    fs::write(media.join("Version 3.1 [3].f137.mp4"), b"x").unwrap();
    fs::write(media.join("Other [34].mp4"), b"x").unwrap();
    remove_media(&paths, 3);
    let mut left: Vec<String> =
        fs::read_dir(&media).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    left.sort();
    assert_eq!(left, ["31.mp4", "77", "A title [4].mp4.part-Frag7.part", "Folder [32]", "Other [34].mp4", "notes.txt"]);
}

/// A VOD's download so far is the biggest of its part files, and only a
/// download begun and not finished is looked for (pipeline.rs
/// `partial_download`, `file_on_disk`, `mark_partial`; CHANGELOG.md, 0.7.1
/// Fixes: "Show in folder works during a download.").
#[test]
fn a_download_so_far_is_its_biggest_part_file() {
    let root = crate::TempDir::new("partial");
    let paths = Paths::new(root.to_path_buf());
    let (work, media) = (paths.work(), paths.media());
    fs::create_dir_all(&media).unwrap();
    let db = Db::open(&root.join("library.sqlite")).unwrap();
    let info = |id: &str| VodInfo {
        platform: "youtube".into(),
        platform_id: id.into(),
        url: format!("https://www.youtube.com/watch?v={id}"),
        title: "A talk".into(),
        channel: None,
        channel_login: None,
        channel_url: None,
        duration: Some(60.0),
        thumbnail: None,
        published: None,
        started_at: None,
    };
    let (vod, _) = db.add_vod(&info("one")).unwrap();
    let (other, _) = db.add_vod(&info("two")).unwrap();
    let stretch = db.request(vod.id, 0.0, None).unwrap();
    db.set_stretch_status(stretch.id, Status::Paused, 0.3, None).unwrap();
    let listed = |db: &Db, id: i64| {
        let mut vod = db.vod(id).unwrap().unwrap();
        mark_partial(&paths, &mut vod);
        vod
    };

    // Nothing begun: no file, nothing to show.
    assert_eq!(file_on_disk(&db, &paths, vod.id).unwrap(), None);
    assert!(!listed(&db, vod.id).partial);

    // The audio's parts in the work dir and a kept video's in media: the
    // biggest, as it is; another VOD's (its id a prefix of none) aren't.
    let id = vod.id;
    fs::create_dir_all(&work).unwrap();
    fs::write(work.join(format!("{id}.f140.m4a.part")), [0; 10]).unwrap();
    fs::write(media.join(format!("{id}.f137.mp4.part")), [0; 30]).unwrap();
    fs::write(work.join(format!("{id}.info.json")), [0; 99]).unwrap();
    fs::write(work.join(format!("{id}0.m4a.part")), [0; 99]).unwrap();
    fs::write(work.join(format!("{}.m4a.part", other.id)), [0; 99]).unwrap();
    let shown = media.join(format!("{id}.f137.mp4.part"));
    assert_eq!(file_on_disk(&db, &paths, id).unwrap(), Some(shown));
    assert!(listed(&db, id).partial);
    fs::write(work.join(format!("{id}.f251.webm.part")), [0; 50]).unwrap();
    assert_eq!(file_on_disk(&db, &paths, id).unwrap(), Some(work.join(format!("{id}.f251.webm.part"))));

    // Not asked for (the other VOD), its parts aren't looked for in the list.
    assert!(!listed(&db, other.id).partial);

    // Kept, it's the kept file, and the list doesn't call it partial.
    let kept = media.join(format!("{id}.mp4"));
    fs::write(&kept, b"x").unwrap();
    db.set_media_path(id, Some(&kept.to_string_lossy())).unwrap();
    assert_eq!(file_on_disk(&db, &paths, id).unwrap(), Some(kept));
    assert!(!listed(&db, id).partial);
}

/// A work or media file's name says whose it is (pipeline.rs `owner`: "`<id>.…`
/// … or `<title> [<id>]` followed by its extension or a download's part
/// suffixes").
#[test]
fn files_say_whose_they_are() {
    assert_eq!(owner("12.info.json"), Some(12));
    assert_eq!(owner("12.m4a.part"), Some(12));
    assert_eq!(owner("Version 1.2 [7].mp4"), Some(7), "not the 1 of 1.2");
    assert_eq!(owner("Day 3 [7].f399.mp4.part"), Some(7));
    assert_eq!(owner("Talk (3) [7]"), Some(7));
    assert_eq!(owner("Talk [000007].mp4"), Some(7), "padded");
    assert_eq!(owner("2024.10.15 Stream [000007].mp4"), Some(7), "not 2024");
    assert_eq!(owner("12. Intro [000007].f399.mp4.part"), Some(7), "not 12");
    assert_eq!(owner("clip-12-1000-5000.mp4"), None);
    assert_eq!(owner("video-12.info.json"), None);
    assert_eq!(owner("notes.txt"), None);
    assert_eq!(owner("A [b] [x].mp4"), None);
}

/// A file on this computer is transcribed where it is, and every path that
/// cleans up after a VOD leaves it alone (pipeline.rs `audio_for`: "a file
/// on this computer, or the VOD's kept video, read in place").
#[test]
fn local_files_survive_every_cleanup() {
    let root = crate::TempDir::new("local");
    let paths = crate::Paths::new(root.join("data"));
    paths.ensure().unwrap();
    let mine = root.join("mine");
    fs::create_dir_all(&mine).unwrap();
    let source = mine.join("1.mp4");
    fs::write(&source, b"the user's video").unwrap();

    let mut db = Db::open(&paths.db()).unwrap();
    let info = VodInfo {
        platform: crate::local::PLATFORM.into(),
        platform_id: source.to_string_lossy().into_owned(),
        url: source.to_string_lossy().into_owned(),
        title: "1".into(),
        channel: None,
        channel_login: None,
        channel_url: None,
        duration: Some(1.0),
        thumbnail: None,
        published: None,
        started_at: None,
    };
    let vod = db.add_vod(&info).unwrap().0;
    db.set_source_path(vod.id, &source.to_string_lossy()).unwrap();
    let vod = db.vod(vod.id).unwrap().unwrap();
    let stretch = db.request(vod.id, 0.0, None).unwrap();

    let yt_dlp = YtDlp { path: "/nonexistent/yt-dlp".into(), js_runtime: None };
    let tools = Tools { yt_dlp: Some(&yt_dlp), ffmpeg: Path::new("/nonexistent/ffmpeg") };
    let no = AtomicBool::new(false);
    let found = audio_for(&mut db, &vod, &stretch, &tools, &paths, None, &no, |_, _| {}).unwrap();
    assert_eq!(found, Source::Whole { path: source.clone(), downloaded: false }, "transcribed in place");
    db.set_audio_path(vod.id, Some(&source.to_string_lossy())).unwrap();

    remove_work_files(&paths.work(), vod.id);
    paths.clear_cache().unwrap();
    db.forget_downloads().unwrap();
    db.delete_vod(vod.id).unwrap();
    assert_eq!(fs::read(&source).unwrap(), b"the user's video");

    fs::remove_file(&source).unwrap();
    assert!(audio_for(&mut db, &vod, &stretch, &tools, &paths, None, &no, |_, _| {}).is_err());
}

/// A VOD whose video is kept is transcribed from that file, with nothing
/// downloaded (the tools here don't exist), until the file is gone
/// (pipeline.rs `audio_for`: "A VOD whose video is kept … transcribed from
/// that video, once it's here").
#[test]
fn a_kept_video_is_used_rather_than_downloaded_again() {
    let root = crate::TempDir::new("kept");
    let paths = crate::Paths::new(root.join("data"));
    paths.ensure().unwrap();
    let mut db = Db::open(&paths.db()).unwrap();
    let info = VodInfo {
        platform: "rumble".into(),
        platform_id: "v1".into(),
        url: "https://rumble.com/v1-x.html".into(),
        title: "x".into(),
        channel: None,
        channel_login: None,
        channel_url: None,
        duration: Some(1.0),
        thumbnail: None,
        published: None,
        started_at: None,
    };
    let vod = db.add_vod(&info).unwrap().0;
    let stretch = db.request(vod.id, 0.0, None).unwrap();
    let kept = paths.source_video(vod.id, "mp4");
    fs::create_dir_all(kept.parent().unwrap()).unwrap();
    fs::write(&kept, b"video").unwrap();
    db.set_media_path(vod.id, Some(&kept.to_string_lossy())).unwrap();
    let yt_dlp = YtDlp { path: "/nonexistent/yt-dlp".into(), js_runtime: None };
    let tools = Tools { yt_dlp: Some(&yt_dlp), ffmpeg: Path::new("/nonexistent/ffmpeg") };
    let no = AtomicBool::new(false);
    let found = audio_for(&mut db, &vod, &stretch, &tools, &paths, None, &no, |_, _| {}).unwrap();
    assert_eq!(found, Source::Whole { path: kept.clone(), downloaded: false });
    fs::remove_file(&kept).unwrap();
    assert!(audio_for(&mut db, &vod, &stretch, &tools, &paths, None, &no, |_, _| {}).is_err(), "downloads again");
}

/// A "yt-dlp" that fails every section asked of it, as a site ffmpeg can't
/// seek in does, and downloads anything else whole (a file saying so).
/// The kit's fake has no site that serves a VOD but not its sections.
fn failing_sections(dir: &Path) -> YtDlp {
    let path = dir.join("yt-dlp-no-sections");
    let script = "#!/bin/sh\nprev=\nfor a in \"$@\"; do\n  case \"$a\" in --download-sections) echo 'ERROR: no' >&2; exit 1;; esac\n  \
                  [ \"$prev\" = -o ] && out=$a\n  prev=$a\ndone\nfile=$(echo \"$out\" | sed 's/%(ext)s/m4a/')\n\
                  echo audio > \"$file\"\necho \"[vodscribe-file]$file\"\n";
    fs::write(&path, script).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    YtDlp { path, js_runtime: None }
}

/// A library with a VOD of `platform` 600 s long, its details looked up a moment
/// ago (so not looked up again), and a stretch of it asked for.
fn asked_for_a_stretch(platform: &str) -> (Library, Vod, Stretch) {
    let lib = Library::new();
    let vod = lib.add_site_vod(platform, "v1", "A talk", 600.0);
    fs::write(lib.paths.source_info(vod.id), b"{}").unwrap();
    let stretch = lib.db.request(vod.id, 100.0, Some(130.0)).unwrap();
    (lib, vod, stretch)
}

/// A stretch whose section fails downloads the VOD's whole audio instead
/// (pipeline.rs `audio_for`: "the VOD's whole audio, downloaded as for a
/// request for all of it, when the section fails for any reason but a
/// stop").
#[test]
fn a_stretch_whose_section_fails_downloads_the_whole_audio_instead() {
    let (mut lib, vod, stretch) = asked_for_a_stretch("twitch");
    let dir = TempDir::new("no-sections");
    let yt_dlp = failing_sections(&dir);
    let tools = Tools { yt_dlp: Some(&yt_dlp), ffmpeg: Path::new("/nonexistent/ffmpeg") };
    let no = AtomicBool::new(false);
    let found = audio_for(&mut lib.db, &vod, &stretch, &tools, &lib.paths, None, &no, |_, _| {}).unwrap();
    assert_eq!(found, Source::Whole { path: lib.paths.source_audio(vod.id, "m4a"), downloaded: true });
}

/// A stretch of a site without its own player whose section fails gets
/// the whole audio, never the video, even offered one to keep (pipeline.rs
/// `audio_for`: "the VOD's whole audio, downloaded as for a request for
/// all of it"; worker.rs `cancel`: "A stretch's own section never
/// downloads the video").
#[test]
fn a_stretch_whose_section_fails_never_downloads_the_video() {
    let (mut lib, vod, stretch) = asked_for_a_stretch("rumble");
    let dir = TempDir::new("no-sections-video");
    let yt_dlp = failing_sections(&dir);
    let tools = Tools { yt_dlp: Some(&yt_dlp), ffmpeg: Path::new("/nonexistent/ffmpeg") };
    let no = AtomicBool::new(false);
    let playable = Playable::default();
    let video = Some((&playable, 1080));
    let found = audio_for(&mut lib.db, &vod, &stretch, &tools, &lib.paths, video, &no, |_, _| {}).unwrap();
    assert_eq!(found, Source::Whole { path: lib.paths.source_audio(vod.id, "m4a"), downloaded: true });
    assert_eq!(lib.db.media_path(vod.id).unwrap(), None);
}

/// A stretch of the Twitch test VOD (67 s long) downloads as a section
/// of its own, sound alone, its 0 at the stretch's start. `VODSCRIBE_BIN`
/// is a folder with yt-dlp (and ffmpeg, or one on PATH).
#[test]
#[ignore = "network"]
fn a_twitch_stretch_downloads_as_a_section_live() {
    let bin = std::env::var("VODSCRIBE_BIN").expect("VODSCRIBE_BIN: the folder with yt-dlp");
    let yt_dlp = tools::find_yt_dlp(Path::new(&bin)).expect("yt-dlp");
    let ffmpeg = tools::find_ffmpeg(Path::new(&bin)).expect("ffmpeg");
    let root = crate::TempDir::new("section-live");
    let paths = crate::Paths::new(root.join("data"));
    paths.ensure().unwrap();
    let work = paths.work();
    fs::create_dir_all(&work).unwrap();
    let mut db = Db::open(&paths.db()).unwrap();
    let no = AtomicBool::new(false);
    let probe = root.join("probe.json");
    let info = fetch::probe(&yt_dlp, "https://www.twitch.tv/videos/2901646294", &probe, &no).unwrap();
    let vod = db.add_vod(&info).unwrap().0;
    fs::rename(&probe, paths.source_info(vod.id)).unwrap();
    let stretch = db.request(vod.id, 30.0, Some(50.0)).unwrap();
    let tools = Tools { yt_dlp: Some(&yt_dlp), ffmpeg: &ffmpeg };
    let started = Instant::now();
    let found = audio_for(&mut db, &vod, &stretch, &tools, &paths, None, &no, |_, _| {}).unwrap();
    let Source::Section { path, start } = found else { panic!("not a section: {found:?}") };
    eprintln!("{} in {:?}", path.display(), started.elapsed());
    assert_eq!(start, 30.0);
    let streams = crate::local::codecs(&ffmpeg, &path).unwrap();
    assert!(streams.audio.is_some() && streams.video.is_none(), "{streams:?}");
    let mut samples = 0;
    crate::audio::decode(&ffmpeg, &path, 0.0, None, &no, |s| {
        samples += s.len();
        Ok(())
    })
    .unwrap();
    let secs = samples as f64 / crate::audio::SAMPLE_RATE as f64;
    eprintln!("{secs:.2} s of audio");
    assert!((secs - 20.0).abs() < 1.5, "{secs} s");
}

#[test]
fn a_stop_kills_a_quiet_worker_promptly() {
    // A "worker" that prints nothing for 30 s.
    let dir = crate::TempDir::new("stop");
    let program = dir.join("worker.sh");
    fs::write(&program, "#!/bin/sh\nsleep 30\n").unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();

    let mut db = Db::open(Path::new(":memory:")).unwrap();
    let info = VodInfo {
        platform: "youtube".into(),
        platform_id: "x".into(),
        url: "u".into(),
        title: "t".into(),
        channel: None,
        channel_login: None,
        channel_url: None,
        duration: Some(60.0),
        thumbnail: None,
        published: None,
        started_at: None,
    };
    let (vod, _) = db.add_vod(&info).unwrap();
    let stretch = db.request(vod.id, 0.0, None).unwrap();
    let spec = WorkerSpec {
        program,
        model_id: "m",
        model_dir: dir.to_path_buf(),
        provider: "cpu",
        batch: 1,
        lib_dirs: Vec::new(),
    };

    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        flag.store(true, Ordering::Relaxed);
    });
    let started = Instant::now();
    let result = run_worker(
        &mut db,
        &vod,
        stretch.id,
        &spec,
        Path::new("ffmpeg"),
        &Audio { path: "audio".into(), offset: 0.0 },
        &cancel,
        &mut |_, _| {},
    );
    assert!(result.is_err());
    assert!(started.elapsed() < std::time::Duration::from_secs(2), "took {:?}", started.elapsed());
}

/// A "worker" that writes its arguments to `args` beside it, prints
/// `lines` (whatever it's asked), then waits `then` seconds.
fn fake_worker(dir: &Path, lines: &[String], then: u32) -> WorkerSpec {
    let program = dir.join(format!("worker-{}.sh", lines.len()));
    let mut script = format!("#!/bin/sh\necho \"$@\" > '{}'\n", dir.join("args").display());
    for line in lines {
        script += &format!("echo '{line}'\n");
    }
    script += &format!("sleep {then}\n");
    fs::write(&program, script).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    WorkerSpec {
        program,
        model_id: crate::model::CPU_MODEL.id,
        model_dir: dir.to_path_buf(),
        provider: "cpu",
        batch: 1,
        lib_dirs: Vec::new(),
    }
}

/// A worker's batch: the audio transcribed up to `end`, and `words` as (text, start).
fn batch(end: f64, words: &[(&str, f64)]) -> String {
    let words: Vec<Word> =
        words.iter().map(|&(text, start)| Word { text: text.into(), start, end: start + 0.3 }).collect();
    serde_json::json!({ "end": end, "words": words }).to_string()
}

fn texts(db: &Db, vod: i64) -> Vec<String> {
    db.segments(vod).unwrap().into_iter().map(|s| s.text).collect()
}

fn line(text: &str, start: f64, end: f64) -> crate::transcript::Segment {
    crate::transcript::Segment { start, end, text: text.into(), words: Vec::new() }
}

/// A stretch widens to the lines its edges cut through, and its lines
/// replace the old ones as they come: stopped part-way, the old lines
/// beyond where it got stay; ended, it's done as far as it got.
#[test]
fn a_stretch_snaps_to_lines_and_replaces_them_as_it_goes() {
    let dir = crate::TempDir::new("replace");
    let mut db = Db::open(Path::new(":memory:")).unwrap();
    let info = VodInfo {
        platform: "youtube".into(),
        platform_id: "x".into(),
        url: "u".into(),
        title: "t".into(),
        channel: None,
        channel_login: None,
        channel_url: None,
        duration: Some(60.0),
        thumbnail: None,
        published: None,
        started_at: None,
    };
    let (vod, _) = db.add_vod(&info).unwrap();
    let old = [line("old a", 0.0, 5.0), line("old b", 8.0, 12.0), line("old c", 20.0, 25.0), line("old d", 40.0, 45.0)];
    db.insert_segments(vod.id, &old).unwrap();
    let whole = db.request(vod.id, 0.0, None).unwrap().id;
    db.set_stretch_model(whole, Some(crate::model::CPU_MODEL.id)).unwrap();
    db.finish_stretch(whole).unwrap();

    // 10-22 cuts through "old b" and "old c": it's 8-25.
    let ask = db.request(vod.id, 10.0, Some(22.0)).unwrap().id;
    let snapped = db.snap_stretch(ask).unwrap();
    assert_eq!((snapped.start, snapped.end), (8.0, Some(25.0)));
    db.set_stretch_model(ask, Some(crate::model::CPU_MODEL.id)).unwrap();
    // One batch to 16 s, a line and two words held back; then it's stopped.
    let first = batch(16.0, &[("New", 9.0), ("first", 9.3), ("line.", 9.6), ("And", 14.0), ("more", 14.5)]);
    let spec = fake_worker(&dir, &[first], 30);
    // Stopped once that batch is in, not after a fixed wait: a script just
    // written may take most of a second to start on macOS.
    let cancel = AtomicBool::new(false);
    let audio = Audio { path: "audio".into(), offset: 0.0 };
    let stopped = run_worker(&mut db, &vod, ask, &spec, Path::new("ffmpeg"), &audio, &cancel, &mut |_, done| {
        if done > 0.0 {
            cancel.store(true, Ordering::Relaxed);
        }
    });
    assert!(stopped.is_err());
    let args = fs::read_to_string(dir.join("args")).unwrap();
    assert!(args.contains("--exit-on-stdin-close"), "{args}");
    assert_eq!(args.trim().split(' ').skip(9).collect::<Vec<_>>(), ["--start", "8.000", "--end", "25.000", "audio"]);
    // "old b" went with the first batch; "old c", beyond it, stays; the
    // words held back are written at the stop.
    assert_eq!(texts(&db, vod.id), ["old a", "New first line.", "And more", "old c", "old d"]);
    assert_eq!(db.stretch(ask).unwrap().unwrap().reached, Some(16.0));
    // Ended there: done to 16 s, one with the rest by the same model.
    db.end_request(ask).unwrap();
    let done: Vec<_> = db.stretches(vod.id).unwrap().into_iter().map(|s| (s.start, s.end, s.status)).collect();
    assert_eq!(done, [(0.0, None, Status::Done)]);

    // Again, to the end of it: what it wrote before is replaced too.
    let again = db.request(vod.id, 8.0, Some(25.0)).unwrap().id;
    db.snap_stretch(again).unwrap();
    let spec = fake_worker(&dir, &[batch(25.0, &[("Said", 10.0), ("over", 10.5), ("again.", 11.0)])], 0);
    let no = AtomicBool::new(false);
    let replaced = run_worker(&mut db, &vod, again, &spec, Path::new("ffmpeg"), &audio, &no, &mut |_, _| {}).unwrap();
    assert_eq!(replaced, 3);
    assert_eq!(texts(&db, vod.id), ["old a", "Said over again.", "old d"]);
}

/// Words held back for the next line, then a batch with none (silence):
/// the old lines there went with it, so a run that fails then (the GPU
/// running out, say, before the processor takes over) carries on from
/// the held words, not past them, or they'd never be written.
#[test]
fn a_failed_run_resumes_from_the_words_it_held_back() {
    let dir = crate::TempDir::new("held-back");
    let mut db = Db::open(Path::new(":memory:")).unwrap();
    let info = VodInfo {
        platform: "youtube".into(),
        platform_id: "x".into(),
        url: "u".into(),
        title: "t".into(),
        channel: None,
        channel_login: None,
        channel_url: None,
        duration: Some(60.0),
        thumbnail: None,
        published: None,
        started_at: None,
    };
    let (vod, _) = db.add_vod(&info).unwrap();
    db.insert_segments(vod.id, &[line("old a", 0.0, 5.0), line("old b", 15.0, 17.0)]).unwrap();
    let ask = db.request(vod.id, 0.0, None).unwrap().id;
    db.set_stretch_model(ask, Some(crate::model::CPU_MODEL.id)).unwrap();
    let program = dir.join("failing-worker.sh");
    let script = format!(
        "#!/bin/sh\necho '{}'\necho '{}'\necho 'out of memory' >&2\nexit 1\n",
        batch(20.0, &[("A", 2.0), ("line.", 2.5), ("Held", 15.0), ("back", 15.5)]),
        batch(30.0, &[])
    );
    fs::write(&program, script).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    let spec = WorkerSpec { program, ..fake_worker(&dir, &[], 0) };
    let audio = Audio { path: "audio".into(), offset: 0.0 };
    let no = AtomicBool::new(false);
    assert!(run_worker(&mut db, &vod, ask, &spec, Path::new("ffmpeg"), &audio, &no, &mut |_, _| {}).is_err());
    assert_eq!(texts(&db, vod.id), ["A line."], "\"old b\" went with the first batch");
    assert_eq!(db.stretch(ask).unwrap().unwrap().reached, Some(15.0), "from the words held back");
}

/// A section's words are in its own time, from 0: they're moved to the
/// VOD's, and the worker is asked for the stretch in the section's.
#[test]
fn a_sections_lines_are_in_the_vods_time() {
    let dir = crate::TempDir::new("section-offset");
    let mut db = Db::open(Path::new(":memory:")).unwrap();
    let info = VodInfo {
        platform: "twitch".into(),
        platform_id: "x".into(),
        url: "u".into(),
        title: "t".into(),
        channel: None,
        channel_login: None,
        channel_url: None,
        duration: Some(600.0),
        thumbnail: None,
        published: None,
        started_at: None,
    };
    let (vod, _) = db.add_vod(&info).unwrap();
    let ask = db.request(vod.id, 100.0, Some(130.0)).unwrap().id;
    let spec = fake_worker(&dir, &[batch(30.0, &[("Hello", 1.0), ("there", 2.0), ("friend.", 3.0)])], 0);
    let audio = Audio { path: "section.mka".into(), offset: 100.0 };
    let mut seen = Vec::new();
    let no = AtomicBool::new(false);
    run_worker(&mut db, &vod, ask, &spec, Path::new("ffmpeg"), &audio, &no, &mut |_, p| seen.push(p)).unwrap();
    let args = fs::read_to_string(dir.join("args")).unwrap();
    assert!(args.contains("--start 0.000 --end 30.000 section.mka"), "{args}");
    let segments = db.segments(vod.id).unwrap();
    assert_eq!((segments[0].start, segments[0].words[2].start), (101.0, 103.0));
    assert_eq!(seen, [0.0, 1.0], "progress within the stretch");
}
