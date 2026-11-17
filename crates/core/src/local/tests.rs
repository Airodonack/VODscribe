//! The specs of files on this computer: the public functions of `local.rs`
//! with the kit's fake ffmpeg, and the readers of ffmpeg's details (what a
//! real ffmpeg prints is a parser's spec, so those take its text). The tests
//! that run the real ffmpeg are the fake's check against it; they skip where
//! ffmpeg isn't installed.

use super::*;
use crate::testkit::media::Video;
use crate::testkit::{Ffmpeg, Media};
use std::fs;

/// A folder of its own with the fake ffmpeg in it.
struct Computer {
    dir: crate::TempDir,
    ffmpeg: Ffmpeg,
}

impl Computer {
    fn new(name: &str) -> Self {
        let dir = crate::TempDir::new(name);
        let ffmpeg = Ffmpeg::at(dir.join("bin/ffmpeg"));
        Computer { dir, ffmpeg }
    }

    fn ffmpeg(&self) -> &Path {
        self.ffmpeg.path()
    }

    /// A fake media file of `media` named `name` in the folder.
    fn file(&self, name: &str, media: &Media) -> PathBuf {
        let path = self.dir.join(name);
        media.write(&path);
        path
    }

    fn probe(&self, path: &Path) -> Result<VodInfo> {
        probe(self.ffmpeg(), path)
    }
}

fn refusal(result: Result<VodInfo>) -> String {
    result.expect_err("refused").to_string()
}

// ------------------------------------------------------------ media types

/// "The file types VODscribe adds and plays, by extension, with the media
/// type the player is told" (core local.rs, `MEDIA_TYPES`). The picker's and
/// the media server's list is this one, so it's pinned as it is.
#[test]
fn the_file_types_added_and_played_are_these_with_their_media_types() {
    let want = [
        ("mp4", "video/mp4"),
        ("m4v", "video/mp4"),
        ("mkv", "video/x-matroska"),
        ("webm", "video/webm"),
        ("mov", "video/quicktime"),
        ("avi", "video/x-msvideo"),
        ("ts", "video/mp2t"),
        ("flv", "video/x-flv"),
        ("mp3", "audio/mpeg"),
        ("m4a", "audio/mp4"),
        ("aac", "audio/mp4"),
        ("wav", "audio/wav"),
        ("flac", "audio/flac"),
        ("ogg", "audio/ogg"),
        ("opus", "audio/ogg"),
    ];
    assert_eq!(MEDIA_TYPES, want);
    for (ext, mime) in want {
        assert_eq!(media_type(Path::new(&format!("/v/a.{ext}"))), Some(mime), "{ext}");
        assert!(is_media(Path::new(&format!("/v/a.{ext}"))), "{ext}");
    }
}

/// "The media type of `path` by its extension" (core local.rs, `media_type`):
/// whatever its case, and with a Windows path's folders.
#[test]
fn an_extension_is_read_in_any_case_and_with_either_kind_of_path() {
    assert_eq!(media_type(Path::new("/v/a.MP4")), Some("video/mp4"));
    assert_eq!(media_type(Path::new("/v/a.Mkv")), Some("video/x-matroska"));
    assert_eq!(media_type(Path::new("C:\\v\\b.mkv")), Some("video/x-matroska"));
}

/// "Anything else is refused, so a program can't be added (and later
/// "opened", which would run it)" (core local.rs, `MEDIA_TYPES`).
#[test]
fn a_program_or_a_file_with_no_type_isnt_media() {
    for name in ["/v/clip.mp4.exe", "/v/run.sh", "/v/x", "/v/clip.mp4.", "/v/readme.txt", "/v/a.mp5"] {
        assert_eq!(media_type(Path::new(name)), None, "{name}");
        assert!(!is_media(Path::new(name)), "{name}");
    }
}

// ----------------------------------------------------------------- adding

/// "Network shares and device paths (\\server\share, \\?\UNC\, \\.\): opening
/// one can make Windows sign in to someone else's server" (core local.rs,
/// `is_remote`), so `probe` refuses them before it asks ffmpeg anything: no
/// ffmpeg is there to ask.
#[test]
fn a_file_on_another_computer_is_refused_before_anything_opens_it() {
    let nowhere = Path::new("/nonexistent/ffmpeg");
    for path in
        ["\\\\attacker\\share\\a.mp4", "//host/share/a.mp4", "\\\\?\\UNC\\host\\share\\a.mp4", "\\\\.\\pipe\\a.mp4"]
    {
        let message = refusal(probe(nowhere, Path::new(path)));
        assert!(message.contains("is on another computer. Copy it to this one first."), "{path}: {message}");
    }
    // A plain path, Windows' drive letters too, is on this computer.
    assert!(!is_remote(Path::new("/home/a.mp4")) && !is_remote(Path::new("C:\\a.mp4")));
}

/// "Anything else is refused, so a program can't be added" (core local.rs,
/// `MEDIA_TYPES`): a real video under a program's extension is refused, said
/// plainly.
#[test]
fn a_program_is_refused_though_it_holds_a_video() {
    let pc = Computer::new("local-program");
    let program = pc.file("clip.mp4.exe", &Media::video(2.0));
    let message = refusal(pc.probe(&program));
    assert!(message.contains("clip.mp4.exe isn't a video or audio file VODscribe can use."), "{message}");
}

/// "so a program can't be added" (core local.rs, `MEDIA_TYPES`): a link named
/// like a video that leads to a program is the program.
#[cfg(unix)]
#[test]
fn a_link_named_like_a_video_that_leads_to_a_program_is_refused() {
    let pc = Computer::new("local-link-to-program");
    let program = pc.file("tool.exe", &Media::video(2.0));
    let link = pc.dir.join("clip.mp4");
    std::os::unix::fs::symlink(&program, &link).unwrap();
    let message = refusal(pc.probe(&link));
    assert!(message.contains("tool.exe isn't a video or audio file VODscribe can use."), "{message}");
}

/// A file that isn't where it was said to be can't be opened (core local.rs,
/// `probe`).
#[test]
fn a_file_that_isnt_there_cant_be_opened() {
    let pc = Computer::new("local-missing");
    let message = refusal(pc.probe(&pc.dir.join("gone.mp4")));
    assert!(message.contains("gone.mp4 can't be opened."), "{message}");
}

/// Only files are added: a folder named like a video isn't one (core
/// local.rs, `probe`).
#[test]
fn a_folder_named_like_a_video_isnt_a_file() {
    let pc = Computer::new("local-folder");
    let folder = pc.dir.join("Season 1.mkv");
    fs::create_dir(&folder).unwrap();
    let message = refusal(pc.probe(&folder));
    assert!(message.contains("Season 1.mkv isn't a file."), "{message}");
}

/// A file ffmpeg can't read isn't added as silence (CHANGELOG.md, 0.4.0
/// Fixes: "A file ffmpeg can't read passed as silent instead of failing.").
#[test]
fn a_file_ffmpeg_cannot_read_is_refused_not_added_as_silence() {
    let pc = Computer::new("local-unreadable");
    let broken = pc.dir.join("broken.mp4");
    fs::write(&broken, b"not a video at all").unwrap();
    let message = refusal(pc.probe(&broken));
    assert!(message.contains("broken.mp4 has no picture and no sound."), "{message}");
}

/// "refusing anything with neither a picture nor sound" (core local.rs,
/// `probe`), a file ffmpeg reads and finds nothing in too.
#[test]
fn a_file_with_neither_picture_nor_sound_is_refused() {
    let pc = Computer::new("local-empty");
    let nothing = Media { video: None, audio: None, ..Media::video(5.0) };
    let message = refusal(pc.probe(&pc.file("nothing.mp4", &nothing)));
    assert!(message.contains("nothing.mp4 has no picture and no sound."), "{message}");
}

/// "a video without sound is kept to play and edit, with nothing to
/// transcribe" (core local.rs, `probe`); CHANGELOG.md, 0.4.0: "Videos without
/// sound can be added."
#[test]
fn a_video_without_sound_is_added() {
    let pc = Computer::new("local-mute");
    let mute = Media { audio: None, ..Media::video(12.5) };
    let info = pc.probe(&pc.file("mute.mp4", &mute)).unwrap();
    assert_eq!((info.platform.as_str(), info.title.as_str(), info.duration), (PLATFORM, "mute", Some(12.5)));
}

/// "Audio alone (a podcast, a song, an audio file) plays in a slim bar"
/// (CHANGELOG.md, 0.3.0): so it's added, with its length.
#[test]
fn an_audio_file_is_added() {
    let pc = Computer::new("local-audio");
    let info = pc.probe(&pc.file("song.m4a", &Media::sound(200.0))).unwrap();
    assert_eq!((info.platform.as_str(), info.title.as_str(), info.duration), (PLATFORM, "song", Some(200.0)));
}

/// "The file's absolute path is its id, so adding it twice finds the first
/// one" (core local.rs, `probe`): the same file by a path that climbs out and
/// back, or by a link, has the same id, and the id is the path itself.
#[test]
fn a_file_is_known_by_its_absolute_path_however_it_is_reached() {
    let pc = Computer::new("local-id");
    let path = pc.file("Day 3.mp4", &Media::video(60.0));
    let first = pc.probe(&path).unwrap();
    let canonical = dunce::canonicalize(&path).unwrap().to_string_lossy().into_owned();
    assert_eq!((first.platform_id.as_str(), first.url.as_str()), (canonical.as_str(), canonical.as_str()));

    fs::create_dir(pc.dir.join("sub")).unwrap();
    let roundabout = pc.dir.join("sub/../Day 3.mp4");
    assert_eq!(pc.probe(&roundabout).unwrap().platform_id, first.platform_id);
    #[cfg(unix)]
    {
        let link = pc.dir.join("alias.mp4");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert_eq!(pc.probe(&link).unwrap().platform_id, first.platform_id);
    }
}

/// A file's title is its name without the extension (core local.rs, `probe`:
/// `title`), the dots inside it kept.
#[test]
fn a_files_title_is_its_name_without_the_extension() {
    let pc = Computer::new("local-title");
    let info = pc.probe(&pc.file("Day 3.final cut.mp4", &Media::video(3.0))).unwrap();
    assert_eq!(info.title, "Day 3.final cut");
}

/// A file has no channel, thumbnail or date of its own: what a site gives a
/// VOD, it lacks (core local.rs, `probe`).
#[test]
fn a_file_has_no_channel_picture_or_date() {
    let pc = Computer::new("local-bare");
    let info = pc.probe(&pc.file("a.mp4", &Media::video(3.0))).unwrap();
    assert_eq!(
        (info.channel, info.channel_login, info.channel_url, info.thumbnail, info.published, info.started_at),
        (None, None, None, None, None, None)
    );
}

/// "the app reads them but never copies, moves or deletes them" (core
/// local.rs, module doc): every reader leaves the file and its folder as they
/// were.
#[test]
fn the_app_reads_a_file_but_never_changes_it() {
    let pc = Computer::new("local-untouched");
    let folder = pc.dir.join("videos");
    let path = pc.file("videos/a.mp4", &Media::video(30.0));
    let before = (fs::read(&path).unwrap(), fs::metadata(&path).unwrap().modified().unwrap());
    pc.probe(&path).unwrap();
    codecs(pc.ffmpeg(), &path).unwrap();
    frame_rate(pc.ffmpeg(), &path).unwrap();
    media(pc.ffmpeg(), &path).unwrap();
    frame(pc.ffmpeg(), &path, 1.0, 80).unwrap();
    let after = (fs::read(&path).unwrap(), fs::metadata(&path).unwrap().modified().unwrap());
    assert_eq!(before, after);
    let names: Vec<_> = fs::read_dir(&folder).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names, ["a.mp4"], "nothing copied beside it");
}

// ---------------------------------------------------------------- lengths

/// A file's length is "as ffmpeg reads it": its details' "Duration" in
/// hours, minutes and seconds, or none where ffmpeg says N/A (core local.rs,
/// `Media::duration`, `probe`).
#[test]
fn a_files_length_is_read_from_its_details() {
    let line = |d: &str| format!("  Duration: {d}, start: 0.000000, bitrate: 1 kb/s\n  Stream #0:0: Audio: aac\n");
    assert_eq!(parse_streams(&line("01:02:03.50")).duration, Some(3723.5));
    assert_eq!(parse_streams(&line("00:00:00.04")).duration, Some(0.04));
    assert_eq!(parse_streams(&line("N/A")).duration, None);
    assert_eq!(parse_streams("  Stream #0:0: Audio: aac\n").duration, None, "no line at all");
}

/// A file's length as `probe` and `media` give it (core local.rs, `probe`,
/// `Media::duration`).
#[test]
fn a_files_length_comes_with_what_it_holds() {
    let pc = Computer::new("local-length");
    let path = pc.file("a.mp4", &Media::video(3723.5));
    assert_eq!(pc.probe(&path).unwrap().duration, Some(3723.5));
    assert_eq!(media(pc.ffmpeg(), &path).unwrap().duration, Some(3723.5));
}

/// "(cover art, an "attached pic", isn't video)" (core local.rs, `probe`'s
/// readers, `Streams::picture`): a song with a cover has sound and no
/// picture, and a file of only a cover has neither, so it's refused.
#[test]
fn cover_art_is_not_a_picture() {
    let song = "  Stream #0:0: Audio: mp3, 44100 Hz, stereo, fltp, 320 kb/s
  Stream #0:1: Video: mjpeg (Baseline), yuvj420p(pc, bt470bg/unknown/unknown), 600x600 [SAR 1:1 DAR 1:1], 90k tbr, 90k tbn (attached pic)";
    let streams = parse_streams(song);
    assert!(streams.audio && !streams.picture);
    let cover = "  Stream #0:0: Video: mjpeg (Baseline), yuvj420p(pc), 600x600, 90k tbr, 90k tbn (attached pic)";
    let streams = parse_streams(cover);
    assert!(!streams.audio && !streams.picture);
}

// ----------------------------------------------------------------- codecs

/// "The codecs of `path`'s first video and audio streams, as ffmpeg names
/// them" (core local.rs, `codecs`), as the fake ffmpeg is asked.
#[test]
fn a_files_codecs_are_its_pictures_and_sounds_as_ffmpeg_names_them() {
    let pc = Computer::new("local-codecs");
    let film = pc.file("film.mp4", &Media::video(3.0));
    let streams = codecs(pc.ffmpeg(), &film).unwrap();
    assert_eq!((streams.video.as_deref(), streams.audio.as_deref()), (Some("h264"), Some("aac")));
    let mut vp9 = Media::video(3.0);
    vp9.video = Some(Video { codec: "vp9".into(), ..vp9.video.clone().unwrap() });
    vp9.audio = Some("opus".into());
    let streams = codecs(pc.ffmpeg(), &pc.file("film.webm", &vp9)).unwrap();
    assert_eq!((streams.video.as_deref(), streams.audio.as_deref()), (Some("vp9"), Some("opus")));
}

/// A file of sound alone, or of a picture alone, has no codec of the other
/// kind (core local.rs, `codecs`).
#[test]
fn a_file_of_sound_alone_has_no_video_codec_and_a_mute_one_no_audio_codec() {
    let pc = Computer::new("local-codecs-alone");
    let song = codecs(pc.ffmpeg(), &pc.file("song.m4a", &Media::sound(3.0))).unwrap();
    assert_eq!((song.video, song.audio.as_deref()), (None, Some("aac")));
    let mute = codecs(pc.ffmpeg(), &pc.file("mute.mp4", &Media { audio: None, ..Media::video(3.0) })).unwrap();
    assert_eq!((mute.video.as_deref(), mute.audio), (Some("h264"), None));
}

/// "(cover art, an "attached pic", isn't video)" (core local.rs, `codecs`).
#[test]
fn cover_art_has_no_video_codec() {
    let song = "  Stream #0:0: Audio: mp3, 44100 Hz, stereo, fltp, 128 kb/s
  Stream #0:1: Video: mjpeg (Baseline), yuvj420p, 600x600, 90k tbr, 90k tbn (attached pic)
";
    let streams = parse_codecs(song);
    assert_eq!((streams.video, streams.audio.as_deref()), (None, Some("mp3")));
}

/// "The codecs of `path`'s first video and audio streams" (core local.rs,
/// `codecs`): a second stream of a kind isn't asked, and a codec's name ends
/// at the space, comma or bracket after it.
#[test]
fn a_files_codec_is_that_of_its_first_stream_of_a_kind_named_to_its_end() {
    let details = "  Stream #0:0[0x1](und): Video: h264 (High) (avc1 / 0x31637661), yuv420p, 320x240, 25 fps
  Stream #0:1[0x2](und): Audio: aac (LC) (mp4a / 0x6134706D), 44100 Hz, stereo, fltp
  Stream #0:2: Video: hevc, yuv420p, 320x240, 25 fps
  Stream #0:3: Audio: opus, 48000 Hz, stereo, fltp
";
    let streams = parse_codecs(details);
    assert_eq!((streams.video.as_deref(), streams.audio.as_deref()), (Some("h264"), Some("aac")));
    let bare = parse_codecs("  Stream #0:0: Video: vp9, yuv420p(tv), 64x64\n  Stream #0:1: Audio: vorbis, 44100 Hz\n");
    assert_eq!((bare.video.as_deref(), bare.audio.as_deref()), (Some("vp9"), Some("vorbis")));
    let none = parse_codecs("Input #0, wav, from 'a.wav':\n  Duration: 00:00:01.00\n");
    assert_eq!((none.video, none.audio), (None, None));
}

// ------------------------------------------------------------- frame rate

/// "The frame rate of `path`'s first video stream, as ffmpeg reads it; None
/// for sound alone" (core local.rs, `frame_rate`).
#[test]
fn a_videos_frame_rate_is_read_and_sound_alone_has_none() {
    let pc = Computer::new("local-rate");
    let mut film = Media::video(3.0);
    film.video.as_mut().unwrap().fps = 25.0;
    assert_eq!(frame_rate(pc.ffmpeg(), &pc.file("film.mp4", &film)).unwrap(), Some(25.0));
    assert_eq!(frame_rate(pc.ffmpeg(), &pc.file("song.m4a", &Media::sound(3.0))).unwrap(), None);
}

/// "The exact rate behind ffmpeg's two decimals: 23.98, 29.97 and 59.94 are
/// 24000/1001, 30000/1001 and 60000/1001" (core local.rs, `exact_rate`);
/// CHANGELOG.md, 0.5.0 Fixes: "Videos at 23.976, 29.97 or 59.94 frames a
/// second showed one frame twice every few minutes, and stepping frame by
/// frame drifted."
#[test]
fn a_rate_ffmpeg_shows_as_two_decimals_is_read_as_the_exact_one() {
    let rate = |shown: &str| parse_frame_rate(&format!("  Stream #0:0: Video: h264, yuv420p, 1280x720, {shown} fps"));
    assert_eq!(rate("23.98"), Some(24000.0 / 1001.0));
    assert_eq!(rate("29.97"), Some(30000.0 / 1001.0));
    assert_eq!(rate("59.94"), Some(60000.0 / 1001.0));
    assert_eq!(rate("119.88"), Some(120000.0 / 1001.0));
    assert_eq!(rate("23.976"), Some(24000.0 / 1001.0), "ffmpeg's own three decimals");
    assert_eq!(rate("239.76"), Some(240000.0 / 1001.0));
    assert_eq!(rate("299.7"), Some(300000.0 / 1001.0), "a fast camera's");
}

/// A rate that isn't one of those is kept as it's shown (core local.rs,
/// `exact_rate`: only "23.98, 29.97 and 59.94" are the 1001 rates).
#[test]
fn a_rate_that_isnt_a_1001_one_is_kept_as_shown() {
    let rate = |shown: &str| parse_frame_rate(&format!("  Stream #0:0: Video: h264, yuv420p, 1280x720, {shown} fps"));
    for shown in [12.5, 24.0, 25.0, 30.0, 50.0, 60.0, 29.5, 15.0, 144.0] {
        assert_eq!(rate(&shown.to_string()), Some(shown), "{shown}");
    }
}

/// "Rates ffmpeg shows as "120k" are a timebase, not a picture rate: unknown"
/// (core local.rs, `parse_frame_rate`); neither is none or zero one.
#[test]
fn a_timebase_or_a_zero_is_not_a_frame_rate() {
    let line = |shown: &str| format!("  Stream #0:0: Video: h264, yuv420p, 1280x720, {shown} fps");
    assert_eq!(parse_frame_rate(&line("120k")), None);
    assert_eq!(parse_frame_rate(&line("0")), None);
    assert_eq!(parse_frame_rate("  Stream #0:0: Video: h264, yuv420p, 1280x720, 3000 kb/s\n"), None, "no rate shown");
}

/// "The frame rate of `path`'s first video stream" (core local.rs,
/// `frame_rate`): found whatever streams come before it, a sound's included.
#[test]
fn a_frame_rate_is_found_after_a_sound_that_comes_first() {
    let details = "  Stream #0:0: Audio: aac (LC), 48000 Hz, stereo, fltp, 128 kb/s
  Stream #0:1: Video: h264 (High), yuv420p(tv, bt709), 1280x720, 25 fps, 25 tbr, 1k tbn
";
    assert_eq!(parse_frame_rate(details), Some(25.0));
}

/// "(cover art, an "attached pic", isn't video)" (core local.rs,
/// `codecs`): its rate isn't a rate of the file's picture either.
#[test]
fn cover_art_has_no_frame_rate() {
    let song = "  Stream #0:0: Audio: mp3, 44100 Hz, stereo, fltp, 128 kb/s
  Stream #0:1: Video: mjpeg (Baseline), yuvj420p, 600x600, 25 fps, 90k tbr, 90k tbn (attached pic)
";
    assert_eq!(parse_frame_rate(song), None);
}

// ------------------------------------------------------------ what's in it

/// "What's in a media file, for putting it in a sequence" (core local.rs,
/// `Media`, `media`): the picture's size, rate and colour, the sound, the
/// length, as the fake ffmpeg is asked.
#[test]
fn a_file_holds_its_picture_its_sound_and_its_length() {
    let pc = Computer::new("local-holds");
    let mut film = Media::video(10.0);
    let video = film.video.as_mut().unwrap();
    (video.width, video.height, video.fps) = (1280, 720, 25.0);
    let held = media(pc.ffmpeg(), &pc.file("film.mp4", &film)).unwrap();
    let colour = Colour { matrix: Matrix::Bt709, full_range: false };
    assert_eq!(held.picture, Some(Picture { width: 1280, height: 720, fps: 25.0, colour }));
    assert!(held.audio);
    assert_eq!(held.duration, Some(10.0));
}

/// "if it has a picture" (core local.rs, `Media::picture`): sound alone has
/// none, and a picture alone has no sound.
#[test]
fn sound_alone_holds_no_picture_and_a_mute_video_no_sound() {
    let pc = Computer::new("local-holds-alone");
    let song = media(pc.ffmpeg(), &pc.file("song.m4a", &Media::sound(4.0))).unwrap();
    assert_eq!((song.picture, song.audio), (None, true));
    let mute = media(pc.ffmpeg(), &pc.file("mute.mp4", &Media { audio: None, ..Media::video(4.0) })).unwrap();
    assert!(mute.picture.is_some() && !mute.audio);
}

/// "Its first video stream's size and frame rate" (core local.rs,
/// `Media::picture`): the size is the stream's, not another number with an x
/// in it, and its rate the exact one.
#[test]
fn a_pictures_size_is_its_streams_and_its_rate_the_exact_one() {
    let details = "  Stream #0:0[0x1](und): Video: h264 (High) (avc1 / 0x31637661), yuv420p(tv, bt709, progressive), 1920x1080 [SAR 1:1 DAR 16:9], 5000 kb/s, 29.97 fps, 29.97 tbr, 30k tbn (default)
  Stream #0:1[0x2](und): Audio: aac (LC) (mp4a / 0x6134706D), 48000 Hz, stereo, fltp, 192 kb/s (default)";
    let held = parse_media(details);
    let colour = Colour { matrix: Matrix::Bt709, full_range: false };
    assert_eq!(held.picture, Some(Picture { width: 1920, height: 1080, fps: 30000.0 / 1001.0, colour }));
    assert!(held.audio);
}

/// "Its first video stream's size and frame rate" (core local.rs,
/// `Media::picture`): found whatever streams come before it, a sound's included.
#[test]
fn a_picture_is_found_after_a_sound_that_comes_first() {
    let details = "  Stream #0:0: Audio: aac (LC), 48000 Hz, stereo, fltp, 128 kb/s
  Stream #0:1: Video: h264 (High), yuv420p(tv, bt709), 1280x720, 25 fps, 25 tbr, 1k tbn
";
    let held = parse_media(details);
    let colour = Colour { matrix: Matrix::Bt709, full_range: false };
    assert_eq!(held.picture, Some(Picture { width: 1280, height: 720, fps: 25.0, colour }));
    assert!(held.audio);
}

/// "(cover art, an "attached pic", isn't video)" (core local.rs, `codecs`):
/// a song with a cover holds sound and no picture.
#[test]
fn a_song_with_a_cover_holds_sound_and_no_picture() {
    let song = "  Stream #0:0: Audio: mp3, 44100 Hz, stereo, fltp, 320 kb/s
  Stream #0:1: Video: mjpeg (Baseline), yuvj420p(pc, bt470bg/unknown/unknown), 600x600 [SAR 1:1 DAR 1:1], 90k tbr, 90k tbn (attached pic)";
    assert_eq!(parse_media(song), super::Media { picture: None, audio: true, duration: None });
}

// ----------------------------------------------------------------- colour

fn colour_of(format: &str) -> Colour {
    Colour::of(&format!("  Stream #0:0: Video: h264 (High) (avc1 / 0x31637661), {format}, 1920x1080, 30 fps"))
}

fn limited(matrix: Matrix) -> Colour {
    Colour { matrix, full_range: false }
}

fn full(matrix: Matrix) -> Colour {
    Colour { matrix, full_range: true }
}

/// "What a stream that declares nothing is read as: limited BT.601, as
/// ffmpeg's own conversion to RGB reads it" (core local.rs, `Colour::UNTAGGED`).
#[test]
fn a_stream_that_declares_nothing_reads_as_limited_bt601() {
    assert_eq!(Colour::UNTAGGED, limited(Matrix::Bt601));
    assert_eq!(colour_of("yuv420p"), Colour::UNTAGGED);
    assert_eq!(colour_of("yuv420p(progressive)"), Colour::UNTAGGED);
    assert_eq!(colour_of("yuv420p(tv, progressive)"), Colour::UNTAGGED);
    assert_eq!(Colour::of("  Stream #0:0: Video: h264"), Colour::UNTAGGED, "no pixel format at all");
}

/// "the matrix its stream declares" (core local.rs, `Colour`): BT.709, BT.2020
/// (either of its two forms), and BT.601 for the rest.
#[test]
fn the_matrix_a_stream_declares_is_the_one_used() {
    assert_eq!(colour_of("yuv420p(tv, bt709, progressive)"), limited(Matrix::Bt709));
    assert_eq!(colour_of("yuv420p10le(tv, bt2020nc/bt2020/smpte2084)"), limited(Matrix::Bt2020));
    assert_eq!(colour_of("yuv420p10le(tv, bt2020c/bt2020/smpte2084)"), limited(Matrix::Bt2020));
    assert_eq!(colour_of("yuv420p(tv, smpte170m/unknown/unknown, progressive)"), limited(Matrix::Bt601));
    assert_eq!(colour_of("yuv420p(tv, bt470bg/unknown/unknown)"), limited(Matrix::Bt601));
}

/// "the first of three slashed is the matrix" (core local.rs, `Colour::of`):
/// the primaries and transfer after it don't count.
#[test]
fn only_the_first_of_the_three_slashed_tags_is_the_matrix() {
    assert_eq!(colour_of("yuv420p(tv, unknown/bt709/bt709)"), Colour::UNTAGGED);
    assert_eq!(colour_of("yuv420p(tv, smpte170m/bt709/bt709)"), limited(Matrix::Bt601));
    assert_eq!(colour_of("yuv420p(tv, bt709/unknown/unknown)"), limited(Matrix::Bt709));
}

/// "whether its values take the full 0-255 (JPEG's, "pc") or the limited
/// 16-235 of video ("tv")" (core local.rs, `Colour`): a "pc" tag, or a JPEG's
/// own pixel format, is full range.
#[test]
fn a_pc_tag_or_a_jpeg_format_is_full_range_and_a_tv_tag_limited() {
    assert_eq!(colour_of("yuv420p(pc)"), full(Matrix::Bt601));
    assert_eq!(colour_of("yuvj420p(pc, bt709/unknown/unknown, progressive)"), full(Matrix::Bt709));
    assert_eq!(colour_of("yuvj420p(progressive)"), full(Matrix::Bt601), "a JPEG format is full range");
    assert_eq!(colour_of("yuvj444p"), full(Matrix::Bt601));
    assert_eq!(colour_of("yuv420p(tv, bt709)"), limited(Matrix::Bt709));
}

/// "An RGB stream (a PNG's "rgb24(pc, gbr/…)") is turned into YUV by ffmpeg
/// with BT.601" (core local.rs, `Colour::of`).
#[test]
fn an_rgb_stream_is_read_with_the_bt601_matrix() {
    assert_eq!(colour_of("rgb24(pc, gbr/unknown/unknown, progressive)").matrix, Matrix::Bt601);
    assert_eq!(colour_of("rgb24(pc, gbr/bt709/bt709)").matrix, Matrix::Bt601, "gbr isn't a matrix to use");
}

/// A stream line's fields are split at the commas not in brackets, so a
/// pixel format's own commas stay in it (core local.rs, `fields`).
#[test]
fn a_pixel_formats_own_commas_dont_split_it() {
    let line =
        "  Stream #0:0: Video: h264 (High), yuv420p(tv, bt709, progressive), 1920x1080 [SAR 1:1 DAR 16:9], 30 fps";
    let fields: Vec<&str> = fields(line).collect();
    assert_eq!(fields, ["h264 (High)", "yuv420p(tv, bt709, progressive)", "1920x1080 [SAR 1:1 DAR 16:9]", "30 fps"]);
}

// ----------------------------------------------------------------- frames

/// "The frame showing at `time` seconds into `path`, as a JPEG" (core
/// local.rs, `frame`), here from the fake ffmpeg.
#[test]
fn a_frame_comes_as_a_jpeg() {
    let pc = Computer::new("local-frame");
    let path = pc.file("film.mp4", &Media::video(60.0));
    let jpeg = frame(pc.ffmpeg(), &path, 12.0, 160).unwrap();
    assert!(jpeg.starts_with(&[0xff, 0xd8]), "a JPEG");
}

/// A frame can't be read of sound alone, or of a file ffmpeg can't read, and
/// the error says so (core local.rs, `frame`: "ffmpeg couldn't read a frame").
#[test]
fn sound_alone_and_a_broken_file_have_no_frame() {
    let pc = Computer::new("local-frame-none");
    let song = pc.file("song.m4a", &Media::sound(60.0));
    let message = frame(pc.ffmpeg(), &song, 1.0, 160).unwrap_err().to_string();
    assert!(message.starts_with("ffmpeg couldn't read a frame:"), "{message}");
    let broken = pc.dir.join("broken.mp4");
    fs::write(&broken, b"nothing").unwrap();
    let message = frame(pc.ffmpeg(), &broken, 1.0, 160).unwrap_err().to_string();
    assert!(message.starts_with("ffmpeg couldn't read a frame:"), "{message}");
}

/// The width of a JPEG, from its start-of-frame marker.
fn jpeg_width(jpeg: &[u8]) -> u16 {
    let at = jpeg.windows(2).position(|w| w[0] == 0xff && (0xc0..=0xc2).contains(&w[1])).expect("a frame header");
    u16::from_be_bytes([jpeg[at + 7], jpeg[at + 8]])
}

/// "as a JPEG `width` pixels wide" (core local.rs, `frame`), read by the real
/// ffmpeg: the fake's picture is the same whatever it's asked.
#[test]
fn a_frame_is_as_wide_as_asked() {
    let Ok(ffmpeg) = which::which("ffmpeg") else { return };
    let dir = crate::TempDir::new("local-frame-width");
    let video = dir.join("clip.mp4");
    let made = std::process::Command::new(&ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc=d=2:s=160x90"])
        .arg(&video)
        .status()
        .unwrap();
    assert!(made.success());
    assert_eq!(jpeg_width(&frame(&ffmpeg, &video, 1.0, 80).unwrap()), 80);
    assert_eq!(jpeg_width(&frame(&ffmpeg, &video, 0.0, 40).unwrap()), 40);
}

/// A time past a video's end has no frame, and the error says so (core
/// local.rs, `frame`: "ffmpeg couldn't read a frame"), where ffmpeg itself
/// exits well with nothing written. The real ffmpeg: the fake gives a frame
/// at any time.
#[test]
fn a_time_past_the_end_has_no_frame() {
    let Ok(ffmpeg) = which::which("ffmpeg") else { return };
    let dir = crate::TempDir::new("local-frame-past");
    let video = dir.join("clip.mp4");
    let made = std::process::Command::new(&ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc=d=2:s=160x90"])
        .arg(&video)
        .status()
        .unwrap();
    assert!(made.success());
    let message = frame(&ffmpeg, &video, 50.0, 80).unwrap_err().to_string();
    assert!(message.starts_with("ffmpeg couldn't read a frame:"), "{message}");
}

// ------------------------------------------------------- inside a folder

/// "Whether `path` is inside `dir`, such as a file in the app's own data
/// folder" (core local.rs, `is_inside`).
#[test]
fn a_file_in_a_folder_or_below_it_is_inside() {
    let dir = crate::TempDir::new("local-inside");
    let data = dir.join("data");
    fs::create_dir_all(data.join("a/b")).unwrap();
    for name in ["x.mp4", "a/x.mp4", "a/b/x.mp4"] {
        fs::write(data.join(name), b"x").unwrap();
        assert!(is_inside(&data.join(name), &data), "{name}");
    }
}

/// A file beside the folder isn't inside it, nor is one whose folder's name
/// only starts with the folder's, nor one reached by climbing out of it
/// (core local.rs, `is_inside`).
#[test]
fn a_file_beside_a_folder_or_reached_by_climbing_out_of_it_isnt_inside() {
    let dir = crate::TempDir::new("local-outside");
    let data = dir.join("data");
    fs::create_dir_all(&data).unwrap();
    fs::create_dir_all(dir.join("data2")).unwrap();
    fs::write(dir.join("x.mp4"), b"x").unwrap();
    fs::write(dir.join("data2/x.mp4"), b"x").unwrap();
    assert!(!is_inside(&dir.join("x.mp4"), &data));
    assert!(!is_inside(&dir.join("data2/x.mp4"), &data), "a longer name isn't inside");
    assert!(!is_inside(&data.join("../x.mp4"), &data), "climbs out");
    assert!(is_inside(&dir.join("data2/../data"), &data), "comes back in");
}

/// A path or a folder that isn't there isn't inside anything (core local.rs,
/// `is_inside`).
#[test]
fn a_file_or_a_folder_that_isnt_there_is_not_inside() {
    let dir = crate::TempDir::new("local-inside-gone");
    fs::write(dir.join("x.mp4"), b"x").unwrap();
    assert!(!is_inside(&dir.join("gone.mp4"), &dir));
    assert!(!is_inside(&dir.join("x.mp4"), &dir.join("gone")));
}

/// A link out of the folder leads where it leads (core local.rs, `is_inside`:
/// the app's own data folder holds what it holds, not what it points to).
#[cfg(unix)]
#[test]
fn a_link_out_of_a_folder_isnt_inside_it() {
    let dir = crate::TempDir::new("local-inside-link");
    let data = dir.join("data");
    fs::create_dir_all(&data).unwrap();
    fs::write(dir.join("outside.mp4"), b"x").unwrap();
    std::os::unix::fs::symlink(dir.join("outside.mp4"), data.join("alias.mp4")).unwrap();
    assert!(!is_inside(&data.join("alias.mp4"), &data));
}

// ----------------------------------------------------------------- source

/// "A local VOD's file, if it's still where it was added" (core local.rs,
/// `source`).
#[test]
fn a_file_still_where_it_was_added_is_its_source() {
    let dir = crate::TempDir::new("local-source");
    let path = dir.join("a.mp4");
    fs::write(&path, b"x").unwrap();
    assert_eq!(source(path.to_str().unwrap()).unwrap(), path);
}

/// A file that has gone, or been replaced by a folder, says where it was
/// (core local.rs, `source`: "The file is no longer at …").
#[test]
fn a_file_that_went_says_where_it_was() {
    let dir = crate::TempDir::new("local-source-gone");
    let gone = dir.join("gone.mp4");
    let message = source(gone.to_str().unwrap()).unwrap_err().to_string();
    assert_eq!(message, format!("The file is no longer at {}.", tools::shown(&gone)));
    let folder = dir.join("folder.mp4");
    fs::create_dir(&folder).unwrap();
    assert!(source(folder.to_str().unwrap()).unwrap_err().to_string().starts_with("The file is no longer at"));
}

// -------------------------------------------------------------- real files

/// The real ffmpeg's details read as the fake's do: a check of the fake
/// against it. Skipped where ffmpeg isn't installed.
#[test]
fn the_real_ffmpegs_details_read_as_the_fakes_do() {
    let Ok(ffmpeg) = which::which("ffmpeg") else { return };
    let dir = crate::TempDir::new("local-real");
    let make = |name: &str, args: &[&str]| {
        let out = dir.join(name);
        let ok = std::process::Command::new(&ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-y"])
            .args(args)
            .arg(&out)
            .status()
            .unwrap()
            .success();
        assert!(ok, "ffmpeg couldn't make {name}");
        out
    };
    let video =
        make("clip.mp4", &["-f", "lavfi", "-i", "testsrc=d=2:s=160x90", "-f", "lavfi", "-i", "sine=d=2", "-shortest"]);
    let silent = make("silent.mp4", &["-f", "lavfi", "-i", "testsrc=d=1:s=160x90"]);
    let song = make("song.wav", &["-f", "lavfi", "-i", "sine=d=1"]);
    let film = make("film.mkv", &["-f", "lavfi", "-i", "testsrc=d=1:s=160x90:r=24000/1001"]);

    let info = probe(&ffmpeg, &video).unwrap();
    assert_eq!((info.platform.as_str(), info.title.as_str()), (PLATFORM, "clip"));
    assert!((info.duration.unwrap() - 2.0).abs() < 0.2, "{:?}", info.duration);
    assert!(probe(&ffmpeg, &silent).is_ok(), "a video without sound");
    assert!(probe(&ffmpeg, &song).is_ok(), "sound alone");
    assert_eq!(frame_rate(&ffmpeg, &video).unwrap(), Some(25.0));
    assert_eq!(frame_rate(&ffmpeg, &film).unwrap(), Some(24000.0 / 1001.0));
    assert_eq!(frame_rate(&ffmpeg, &song).unwrap(), None);
    let held = media(&ffmpeg, &video).unwrap();
    assert_eq!(held.picture.map(|p| (p.width, p.height)), Some((160, 90)));
    assert!(held.audio);
    let streams = codecs(&ffmpeg, &video).unwrap();
    assert_eq!((streams.video.as_deref(), streams.audio.as_deref()), (Some("h264"), Some("aac")));

    let broken = dir.join("broken.mp4");
    fs::write(&broken, b"not a video at all").unwrap();
    assert!(refusal(probe(&ffmpeg, &broken)).contains("has no picture and no sound"));
}
