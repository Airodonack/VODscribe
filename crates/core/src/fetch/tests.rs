//! The specs of `fetch.rs`'s reading of a site's details (`parse_info`,
//! `channel_page`: a parser, so the function is the spec) and of a refused
//! download (`is_refused`, and `download_or_reprobe` on the fake yt-dlp).
//! The tests after them are the rest of `fetch.rs`'s, as they were.

use super::*;
use crate::testkit::{Media, TempDir, yt_dlp};
use serde_json::json;

/// What `parse_info` reads of yt-dlp's details `json`.
fn info(json: Value) -> VodInfo {
    parse_info(&json).unwrap()
}

/// A VOD's upload date is the day it was published, written YYYY-MM-DD
/// (fetch.rs `VodInfo::published`: "YYYY-MM-DD").
#[test]
fn an_upload_date_is_the_day_it_was_published() {
    let vod =
        info(json!({ "id": "abc", "extractor_key": "TwitchVod", "webpage_url": "https://www.twitch.tv/videos/abc",
        "upload_date": "20260102" }));
    assert_eq!(vod.published.as_deref(), Some("2026-01-02"));
}

/// An upload date of eight characters that aren't eight digits is no date,
/// and no crash (CHANGELOG.md, 0.3.0 Fixes: "A rare crash on an unusual
/// upload date."; fetch.rs `parse_info`: "Digits only: slicing at byte positions would
/// panic inside a multi-byte character").
#[test]
fn an_unusual_upload_date_is_no_date_and_no_crash() {
    for date in ["202é102", "2026-1-2", "2026012", "202601022"] {
        let vod = info(json!({ "id": "abc", "webpage_url": "https://www.twitch.tv/videos/abc", "upload_date": date }));
        assert_eq!(vod.published, None, "{date}");
    }
}

/// A VOD's site is youtube, twitch or kick, or yt-dlp's extractor's name
/// for any other (fetch.rs `VodInfo::platform`: ""youtube", "twitch", "kick", or yt-dlp's
/// extractor name for other sites").
#[test]
fn a_vods_site_is_youtube_twitch_kick_or_yt_dlps_extractor() {
    let site = |extractor: &str| {
        info(json!({ "id": "a", "extractor_key": extractor, "webpage_url": "https://example.com/a" })).platform
    };
    assert_eq!(site("Youtube"), "youtube");
    assert_eq!(site("TwitchVod"), "twitch");
    assert_eq!(site("TwitchClips"), "twitch");
    assert_eq!(site("Kick"), "kick");
    assert_eq!(site("KickVOD"), "kick");
    // How its name is written (in lower case, today) no source says.
    assert!(site("Dailymotion").eq_ignore_ascii_case("Dailymotion"));
}

/// A Twitch clip goes by its slug, not the number of its own yt-dlp gives
/// it, which Twitch's VOD look-ups would take for a VOD's (fetch.rs `parse_info`: "it
/// goes by its slug instead"; CHANGELOG.md, 0.3.0: "Twitch clips play, and
/// show the chat from the broadcast they were cut from").
#[test]
fn a_twitch_clip_goes_by_its_slug() {
    let clip = info(json!({
        "id": "1431111279", "display_id": "CoweringRudeDiamondMikeHogu-somPr5Um-WHGc4ga",
        "extractor_key": "TwitchClips", "webpage_url": "https://www.twitch.tv/crelly/clip/CoweringRudeDiamondMikeHogu-somPr5Um-WHGc4ga"
    }));
    assert_eq!(
        (clip.platform.as_str(), clip.platform_id.as_str()),
        ("twitch", "CoweringRudeDiamondMikeHogu-somPr5Um-WHGc4ga")
    );
    assert!(crate::twitch::is_clip(&clip.platform_id));
    // A VOD keeps its own id.
    let vod = info(json!({ "id": "v2901646294", "display_id": "x", "extractor_key": "TwitchVod",
        "webpage_url": "https://www.twitch.tv/videos/2901646294" }));
    assert_eq!(vod.platform_id, "v2901646294");
}

/// Every Twitch channel keeps its login beside its name, which can differ,
/// so its streamer's picture is found by it (CHANGELOG.md, 0.7.2 Fixes:
/// "Every Twitch channel shows its picture."; fetch.rs `VodInfo::channel_login`: "The channel's
/// login (Twitch's, Kick's slug) or handle, as the site gives it, beside
/// `channel`, its display name, which can differ").
#[test]
fn every_twitch_channel_keeps_its_login_beside_its_name() {
    let vod = info(json!({ "id": "v1", "extractor_key": "TwitchVod", "webpage_url": "https://www.twitch.tv/videos/1",
        "uploader": "마를로", "uploader_id": "marlowe" }));
    assert_eq!((vod.channel.as_deref(), vod.channel_login.as_deref()), (Some("마를로"), Some("marlowe")));
    let kick =
        info(json!({ "id": "k1", "extractor_key": "KickVOD", "webpage_url": "https://kick.com/ada_plays/videos/k1",
        "channel": "Ada Plays", "uploader": "Ada Plays", "uploader_id": "ada_plays" }));
    assert_eq!((kick.channel.as_deref(), kick.channel_login.as_deref()), (Some("Ada Plays"), Some("ada_plays")));
    let none = info(json!({ "id": "v2", "extractor_key": "TwitchVod", "webpage_url": "https://www.twitch.tv/videos/2",
        "uploader": "someone" }));
    assert_eq!((none.channel.as_deref(), none.channel_login), (Some("someone"), None));
}

/// A Twitch clip's uploader is whoever clipped it, so neither their login
/// nor their page is taken for the channel's (fetch.rs `parse_info`: "The uploader's
/// login and page are the channel's only when the uploader is the channel:
/// a Twitch clip's uploader is whoever clipped it").
#[test]
fn a_twitch_clips_clipper_is_not_taken_for_its_channel() {
    let clip = info(json!({ "id": "1", "display_id": "Slug", "extractor_key": "TwitchClips",
        "webpage_url": "https://www.twitch.tv/ea/clip/Slug", "channel": "EA", "uploader": "stereotype_",
        "uploader_id": "43566419", "uploader_url": "https://www.twitch.tv/stereotype_" }));
    assert_eq!(clip.channel.as_deref(), Some("EA"));
    assert_eq!((clip.channel_login, clip.channel_url), (None, None));
}

/// A channel's page is kept as the site gives it, from the VOD's own site
/// only, YouTube's handle's page before its channel id's (fetch.rs `VodInfo::channel_url`: "The
/// channel's page, as the site gives it (yt-dlp's `uploader_url` or
/// `channel_url`): https on the VOD's own site only"; fetch.rs `parse_info`: "The
/// uploader's page first: YouTube's goes by the handle (/@name), its
/// channel_url by the channel's id"; CHANGELOG.md, 0.7.2: "The channel over
/// the player links to its page on every site.").
#[test]
fn a_channels_page_is_kept_from_its_own_site_only() {
    let page = |json: Value| info(json).channel_url;
    let yt = json!({ "id": "noa29qsTc4g", "extractor_key": "Youtube",
        "webpage_url": "https://www.youtube.com/watch?v=noa29qsTc4g", "channel": "Quill", "uploader": "Quill",
        "uploader_id": "@quill", "uploader_url": "https://www.youtube.com/@quill",
        "channel_id": "UCabcdefghijklmnopqrstuv", "channel_url": "https://www.youtube.com/channel/UCabcdefghijklmnopqrstuv" });
    assert_eq!(page(yt).as_deref(), Some("https://www.youtube.com/@quill"));
    let id_only = json!({ "id": "v", "extractor_key": "Youtube", "webpage_url": "https://www.youtube.com/watch?v=v",
        "channel_url": "https://www.youtube.com/channel/UCabcdefghijklmnopqrstuv" });
    assert_eq!(page(id_only).as_deref(), Some("https://www.youtube.com/channel/UCabcdefghijklmnopqrstuv"));
    // Another site, on its own host.
    let other = json!({ "id": "x9", "extractor_key": "Dailymotion", "webpage_url": "https://www.dailymotion.com/video/x9",
        "uploader": "Quill", "uploader_url": "https://www.dailymotion.com/quill" });
    assert_eq!(page(other).as_deref(), Some("https://www.dailymotion.com/quill"));
    // Elsewhere, not https, or none: no page.
    let elsewhere = json!({ "id": "x9", "extractor_key": "Dailymotion", "webpage_url": "https://www.dailymotion.com/video/x9",
        "uploader_url": "https://dailymotion.com.example/quill", "channel_url": "http://www.dailymotion.com/quill" });
    assert_eq!(page(elsewhere), None);
    let none = json!({ "id": "v2", "extractor_key": "TwitchVod", "webpage_url": "https://www.twitch.tv/videos/2" });
    assert_eq!(page(none), None);
}

/// A channel's page is an https link with a plain host on the VOD's own
/// site, with or without "www." or "m.", and nothing else is (fetch.rs `channel_page`:
/// "an https link with a plain host (no user, no port) on the VOD's own
/// site (its host, or one under it, either without "www." or "m."). None
/// for anything else").
#[test]
fn a_channels_page_must_be_https_on_the_vods_site() {
    let vod = "https://www.youtube.com/watch?v=noa29qsTc4g";
    for ok in [
        "https://www.youtube.com/@quill",
        "https://youtube.com/@quill",
        "https://m.youtube.com/channel/UC1",
        "https://music.youtube.com/@q",
    ] {
        assert_eq!(channel_page(ok, vod).as_deref(), Some(ok), "{ok}");
    }
    let phone = "https://m.youtube.com/watch?v=noa29qsTc4g";
    assert_eq!(
        channel_page("https://www.youtube.com/@quill", phone).as_deref(),
        Some("https://www.youtube.com/@quill")
    );
    for refused in [
        "http://www.youtube.com/@quill",
        "https://www.youtube.com.example/@quill",
        "https://evil.example/www.youtube.com",
        "https://notyoutube.com/@quill",
        "https://user@www.youtube.com/@quill",
        "https://www.youtube.com:8443/@quill",
        "https://www..youtube.com/@quill",
        "https://www.youtube.com/@quill name",
        "https://www.youtube.com\\@evil.example/",
        "javascript:alert(1)",
        "",
    ] {
        assert_eq!(channel_page(refused, vod), None, "{refused}");
    }
    // A VOD with no plain https host of its own has no page to match.
    assert_eq!(channel_page("https://www.youtube.com/@quill", "file:///a.mp4"), None);
    assert_eq!(channel_page("https://localhost/x", "https://localhost/v"), None, "a bare name isn't a site");
}

/// When a VOD started is the site's word for a Twitch VOD (its
/// `timestamp`) and a YouTube livestream (its `release_timestamp`) only, in
/// UTC to the second; not a clip's, an uploaded video's or another site's
/// (fetch.rs `started_by_site`: "Not a clip's (when it was clipped), an uploaded video's
/// or another site's: those say when it was put up, not recorded";
/// fetch.rs `VodInfo::started_at`: "as an RFC 3339 time in UTC to the second"; CHANGELOG.md,
/// 0.7.0: "Names link to a stream's same moment.").
#[test]
fn a_vods_start_is_the_sites_word_for_twitch_vods_and_youtube_livestreams_only() {
    let started = |json: Value| info(json).started_at;
    let vod = json!({ "id": "v1", "extractor_key": "TwitchVod", "webpage_url": "https://www.twitch.tv/videos/1",
        "timestamp": 1790631005 });
    assert_eq!(started(vod).as_deref(), Some("2026-09-28T21:30:05Z"));
    let clip = json!({ "id": "1", "display_id": "Slug", "extractor_key": "TwitchClips",
        "webpage_url": "https://www.twitch.tv/x/clip/Slug", "timestamp": 1790631005 });
    assert_eq!(started(clip), None, "when it was clipped");
    let youtube = |live: &str| {
        json!({ "id": "abc", "extractor_key": "Youtube", "webpage_url": "https://www.youtube.com/watch?v=abc",
            "timestamp": 1790640000, "release_timestamp": 1790631005, "live_status": live })
    };
    assert_eq!(started(youtube("was_live")).as_deref(), Some("2026-09-28T21:30:05Z"));
    assert_eq!(started(youtube("not_live")), None, "when it was uploaded");
    for (extractor, url) in
        [("Kick", "https://kick.com/ada/videos/x9"), ("Dailymotion", "https://www.dailymotion.com/video/x9")]
    {
        let other = json!({ "id": "x9", "extractor_key": extractor, "webpage_url": url,
            "timestamp": 1790631005, "release_timestamp": 1790631005, "live_status": "was_live" });
        assert_eq!(started(other), None, "{extractor}: when it was put up");
    }
}

/// A download refused with a 403 reads as refused, through whatever
/// context it's given (fetch.rs `is_refused`: "Whether `e` is the media being
/// refused (`REFUSED`), which a new probe may fix").
#[test]
fn a_403_reads_as_refused_through_context() {
    let e = anyhow::anyhow!("{}", clean_error("ERROR: [youtube] abc: HTTP Error 403: Forbidden")).context("download");
    assert!(is_refused(&e));
    assert!(!is_refused(&anyhow::anyhow!("{}", clean_error("ERROR: [youtube] abc: HTTP Error 404: Not Found"))));
}

/// The test video's link on a fake yt-dlp, its details looked up, and
/// where its download goes.
struct Site {
    fake: crate::testkit::YtDlp,
    yt_dlp: YtDlp,
    url: &'static str,
    info: PathBuf,
    out: PathBuf,
    _dir: TempDir,
}

impl Site {
    fn new(name: &str) -> Self {
        let dir = TempDir::new(name);
        let fake = crate::testkit::YtDlp::at(dir.join("bin/yt-dlp"));
        let url = "https://www.youtube.com/watch?v=noa29qsTc4g";
        fake.serves(yt_dlp::details("youtube", "noa29qsTc4g", url, "The test video", 70.0));
        let yt_dlp = YtDlp { path: fake.path().into(), js_runtime: None };
        let (info, out) = (dir.join("info.json"), dir.join("audio"));
        Site { fake, yt_dlp, url, info, out, _dir: dir }
    }

    fn download(&self) -> Result<PathBuf> {
        download_or_reprobe(
            &self.yt_dlp,
            self.url,
            &self.info,
            &self.out,
            Wanted::Audio,
            &AtomicBool::new(false),
            |_| {},
        )
    }
}

/// A download the site refused is tried again from a fresh look-up before
/// it's reported: the details looked up before are of a session the site
/// has soured on (here, a link of their own it refuses), and the new
/// look-up's download comes in (CHANGELOG.md, 0.1.3: "A YouTube download
/// that the site refused is tried again from a fresh lookup before it's
/// reported"; fetch.rs `download_or_reprobe`: "probing `url` again (into
/// `info_path`) and trying once more if the site refuses").
#[test]
fn a_download_the_site_refused_is_tried_again_from_a_fresh_lookup() {
    let site = Site::new("fetch-reprobe");
    let soured = format!("{}&session=soured", site.url);
    let looked_up = yt_dlp::details("youtube", "noa29qsTc4g", &soured, "The test video", 70.0);
    fs::write(&site.info, looked_up.to_string()).unwrap();
    site.fake.refuses(&soured);
    let file = site.download().unwrap();
    assert_eq!(Media::read(&file).unwrap().cut_from, Some((site.url.to_string(), 0.0)));
}

/// A download the site refuses again is reported as refused, saying an
/// update of the video downloader often fixes it (fetch.rs `REFUSED`:
/// "yt-dlp got an HTTP 403 for the media", and "Updating the video
/// downloader in Settings often fixes this.").
#[test]
fn a_download_refused_again_is_reported_as_refused() {
    let site = Site::new("fetch-refused");
    probe(&site.yt_dlp, site.url, &site.info, &AtomicBool::new(false)).unwrap();
    site.fake.refuses(site.url);
    let e = site.download().unwrap_err();
    assert!(is_refused(&e));
    assert_eq!(e.to_string(), REFUSED);
}

/// A download failing for anything but a refusal is reported as it is,
/// with no new look-up (fetch.rs `download_or_reprobe`: "trying once more if the site
/// refuses"). Here the details looked up before can't be read.
#[test]
fn a_download_failing_otherwise_is_reported_without_a_new_lookup() {
    let site = Site::new("fetch-not-refused");
    fs::write(&site.info, "not yt-dlp's details").unwrap();
    let e = site.download().unwrap_err();
    assert!(!is_refused(&e), "{e:#}");
    assert_eq!(fs::read_to_string(&site.info).unwrap(), "not yt-dlp's details", "looked up again");
}

/// A link the video downloader doesn't know is said so in plain words, and
/// nothing is kept of it (e2e add-link-error.mjs: "A link VODscribe can't
/// use: an error under the box, and nothing added."; fetch.rs `clean_error`: "a
/// plain sentence for the common failures").
#[test]
fn a_link_the_video_downloader_does_not_know_says_so() {
    let site = Site::new("fetch-unknown-link");
    let e = probe(&site.yt_dlp, "https://example.com/not-a-vod", &site.info, &AtomicBool::new(false)).unwrap_err();
    assert_eq!(
        e.to_string(),
        "VODscribe can't use that link: it isn't a site the video downloader knows. Paste a link to a video page."
    );
    assert!(!site.info.exists());
}

#[test]
fn info_parsing() {
    assert!(parse_info(&json!({"_type": "playlist"})).is_err());
}

#[test]
fn finds_the_finished_download_without_its_printed_path() {
    let dir = crate::TempDir::new("finished");
    let stem = dir.join("12");
    for name in ["12.info.json", "12.m4a.part", "123.m4a", "12.f251.webm"] {
        std::fs::write(dir.join(name), b"x").unwrap();
    }
    assert_eq!(finished_file(&stem), None, "nothing finished yet");
    std::fs::write(dir.join("12.m4a"), b"x").unwrap();
    assert_eq!(finished_file(&stem), Some(dir.join("12.m4a")));
    std::fs::write(dir.join("12.webm"), b"x").unwrap();
    assert_eq!(finished_file(&stem), None, "two candidates: don't guess");
}

#[test]
fn errors_read_as_plain_sentences() {
    assert_eq!(
        clean_error("[info] x\nERROR: [youtube] abc123: Video unavailable\nmore"),
        "That video isn't available. It may have been deleted or made private."
    );
    assert_eq!(clean_error("ERROR: [youtube] abc: Private video. Sign in"), "That video is private.");
    let hint = " Use --cookies-from-browser or --cookies for the authentication.";
    assert_eq!(
        clean_error(&format!("ERROR: [youtube] abc: Private video. Sign in if you've been granted access.{hint}")),
        "That video is private."
    );
    assert!(
        clean_error(&format!("ERROR: [youtube] abc: Sign in to confirm your age.{hint}")).contains("age-restricted")
    );
    assert_eq!(
        clean_error("ERROR: [twitch:vod] v12: Something odd happened"),
        "Couldn't get this video: Something odd happened"
    );
    assert_eq!(clean_error("one\ntwo\nthree\nfour\n"), "Couldn't get this video: two\nthree\nfour");
    // A video id containing "404" isn't a 404, and the format error still reads right.
    assert_eq!(
        clean_error("ERROR: [twitch:vod] v2404123: Requested format is not available"),
        "That video has no audio VODscribe can download."
    );
    assert!(clean_error("ERROR: [youtube] x: HTTP Error 503: Service Unavailable").contains("having problems"));
    assert!(
        clean_error("ERROR: [youtube] x: Unable to download webpage: HTTP Error 429: Too Many Requests")
            .contains("limiting")
    );
    assert!(clean_error("ERROR: [youtube] x: HTTP Error 403: Forbidden").contains("refused"));
    assert!(clean_error("ERROR: No suitable extractor found for URL https://example.com/").contains("isn't a site"));
    assert!(
        clean_error("ERROR: [vimeo] 76979871: The web client only works when logged-in. Use --cookies, …")
            .contains("signed in")
    );
    assert!(clean_error("yt-dlp: error: no such option: --no-plugin-dirs").contains("out of date"));
    assert!(
        clean_error("ERROR: [youtube] x: Sign in to confirm you're not a bot. Use --cookies-from-browser")
            .contains("not a bot")
    );
    // Only a leading tag is stripped.
    assert_eq!(
        clean_error("ERROR: unable to open for writing: [Errno 28] No space left on device"),
        "Couldn't get this video: unable to open for writing: [Errno 28] No space left on device"
    );
}

#[test]
fn progress_parsing() {
    assert_eq!(parse_progress(" 50 100 NA NA NA"), Some(0.5));
    assert_eq!(parse_progress(" 50 NA 200 NA NA"), Some(0.25));
    assert_eq!(parse_progress(" 50 NA NA 3 12"), Some(0.25));
    assert_eq!(parse_progress(" NA NA NA NA NA"), None);
}

/// A link's lookup stops when canceled: a stand-in yt-dlp that never answers.
#[cfg(unix)]
#[test]
fn a_lookup_stops_when_canceled() {
    use std::os::unix::fs::PermissionsExt;
    let dir = crate::TempDir::new("probe-cancel");
    let path = dir.join("yt-dlp");
    fs::write(&path, "#!/bin/sh\nsleep 30\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    let yt_dlp = YtDlp { path, js_runtime: None };
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    let stopper = {
        let cancel = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            cancel.store(true, Ordering::Relaxed);
            std::time::Instant::now()
        })
    };
    let result = probe(&yt_dlp, "https://example.com/v/1", &dir.join("info.json"), &cancel);
    let stopped = stopper.join().unwrap();
    assert!(result.is_err_and(|e| crate::is_canceled(&e)));
    assert!(stopped.elapsed() < std::time::Duration::from_secs(1), "{:?} after the cancel", stopped.elapsed());
}

#[test]
fn kick_streams_only_from_kick() {
    assert!(is_kick_host("https://web.kick.com/api/v1/stream/manifest.m3u8?init=x"));
    assert!(is_kick_host("https://kick.com/x"));
    for bad in [
        "https://evil.com?x.kick.com/m.m3u8",
        "https://evil.com#.kick.com/",
        "https://evilkick.com/",
        "http://web.kick.com/",
        "https://kick.com@evil.com/",
        "https://web.kick.com.evil.com/",
    ] {
        assert!(!is_kick_host(bad), "{bad}");
    }
}

#[test]
#[ignore = "network"]
fn keeps_videos_live() {
    let bin = std::env::var("VODSCRIBE_BIN").expect("VODSCRIBE_BIN: the folder with yt-dlp");
    let yt_dlp = crate::tools::find_yt_dlp(Path::new(&bin)).expect("yt-dlp");
    let ffmpeg = crate::tools::find_ffmpeg(Path::new(&bin)).expect("ffmpeg");
    let dir = crate::TempDir::new("keep-video");
    let selector = crate::playable::Playable::default().selector(480);
    for (name, url) in [
        ("dailymotion", "https://www.dailymotion.com/video/x7tgad0"),
        ("soundcloud", "https://soundcloud.com/forss/flickermood"),
        ("rumble", "https://rumble.com/v76ea7s-president-trump-announces-major-combat-operations-against-iran.html"),
    ] {
        let info = dir.join(format!("{name}.info.json"));
        probe(&yt_dlp, url, &info, &AtomicBool::new(false)).unwrap();
        let wanted = Wanted::Video { selector: &selector, ffmpeg: &ffmpeg };
        let file = download(&yt_dlp, &info, &dir.join(name), wanted, &AtomicBool::new(false), |_| {}).unwrap();
        let streams = crate::local::codecs(&ffmpeg, &file).unwrap();
        assert!(crate::playable::Playable::default().plays(&streams), "{name}: {streams:?}");
        let out = std::process::Command::new(&ffmpeg).arg("-hide_banner").arg("-i").arg(&file).output().unwrap();
        let streams: Vec<String> = String::from_utf8_lossy(&out.stderr)
            .lines()
            .filter(|l| l.contains("Stream #"))
            .map(|l| l.trim().to_string())
            .collect();
        eprintln!("{name}: {} ({} bytes)\n  {}", file.display(), file.metadata().unwrap().len(), streams.join("\n  "));
    }
}

/// A Kick VOD, which yt-dlp can't read itself, through the app's own Kick
/// lookup: `VODSCRIBE_BIN=<data dir>/bin cargo test -p vodscribe-core -- --ignored kick_vod_live`.
#[test]
#[ignore = "network"]
fn kick_vod_live() {
    let bin = std::env::var("VODSCRIBE_BIN").expect("VODSCRIBE_BIN: the folder with yt-dlp");
    let yt_dlp = crate::tools::find_yt_dlp(Path::new(&bin)).expect("yt-dlp");
    let dir = crate::TempDir::new("kick-live");
    let url = std::env::var("KICK_URL")
        .unwrap_or_else(|_| "https://kick.com/geega/videos/01a0db44-6218-70fb-977c-6bbe91a018d1".into());
    let info = probe(&yt_dlp, &url, &dir.join("info.json"), &AtomicBool::new(false)).unwrap();
    eprintln!("{} · {:?} · {:?}", info.title, info.channel, info.duration);
    assert_eq!(info.platform, "kick");
}
