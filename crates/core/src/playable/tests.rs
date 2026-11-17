//! The specs of what this computer's webview plays: `playable.rs`'s answer
//! saved and read back, what plays of a file that arrived, and the format
//! choice handed to yt-dlp, as the string it's given.

use super::*;
use std::collections::HashSet;
use std::fs;

fn streams(video: Option<&str>, audio: Option<&str>) -> Streams {
    Streams { video: video.map(Into::into), audio: audio.map(Into::into) }
}

/// A selector's choices, each with its web limit taken off, in order.
fn choices(selector: &str) -> Vec<String> {
    selector.split('/').map(|c| c.replace(WEB, "")).collect()
}

fn h264() -> Playable {
    Playable { video: vec![(Video::H264, Audio::Aac)], audio: vec![Audio::Mp3] }
}

// ---------------------------------------------------------- saved answer

/// "Before the webview has been asked: what every webview plays" (core
/// playable.rs, `Playable::default`): H.264 with AAC, and AAC or MP3 alone.
#[test]
fn before_the_webview_is_asked_h264_with_aac_plays() {
    let dir = crate::TempDir::new("playable-default");
    let never = Playable::load(&dir.join("playable.json"));
    assert_eq!(never, Playable::default());
    assert_eq!(never.video, [(Video::H264, Audio::Aac)]);
    assert_eq!(never.audio, [Audio::Aac, Audio::Mp3]);
}

/// "the answer is saved here, so downloads, which may run with no window
/// open, pick a format that plays" (core playable.rs, module doc): what's
/// saved is read back, in its order.
#[test]
fn what_plays_is_saved_and_read_back() {
    let dir = crate::TempDir::new("playable-saved");
    let path = dir.join("playable.json");
    let p = Playable {
        video: vec![(Video::Av1, Audio::Opus), (Video::Vp9, Audio::Opus), (Video::Hevc, Audio::Aac)],
        audio: vec![Audio::Opus, Audio::Flac, Audio::Vorbis, Audio::Mp3],
    };
    p.save(&path).unwrap();
    assert_eq!(Playable::load(&path), p);
}

/// The page sends and reads the answer by the codec families' names
/// (apps/desktop `Playable` in api.ts: "av1", "vp9", "hevc", "h264"; "opus",
/// "aac", "mp3", "vorbis", "flac"), so the file says them as that does.
#[test]
fn what_plays_is_written_by_the_codec_families_names() {
    let dir = crate::TempDir::new("playable-names");
    let path = dir.join("playable.json");
    let p = Playable {
        video: vec![
            (Video::Av1, Audio::Opus),
            (Video::Vp9, Audio::Aac),
            (Video::Hevc, Audio::Mp3),
            (Video::H264, Audio::Vorbis),
        ],
        audio: vec![Audio::Opus, Audio::Aac, Audio::Mp3, Audio::Vorbis, Audio::Flac],
    };
    p.save(&path).unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        r#"{"video":[["av1","opus"],["vp9","aac"],["hevc","mp3"],["h264","vorbis"]],"audio":["opus","aac","mp3","vorbis","flac"]}"#
    );
}

/// A file that's missing, damaged or something else reads as never asked
/// (core playable.rs, `Playable::load`), so downloads still pick a format.
#[test]
fn an_answer_that_is_missing_or_unreadable_reads_as_never_asked() {
    let dir = crate::TempDir::new("playable-unreadable");
    let path = dir.join("playable.json");
    assert_eq!(Playable::load(&path), Playable::default(), "no file");
    for text in ["", "{", "[]", r#"{"video":[["av2","opus"]],"audio":[]}"#, r#"{"video":[]}"#] {
        fs::write(&path, text).unwrap();
        assert_eq!(Playable::load(&path), Playable::default(), "{text}");
    }
    // A folder where the file should be.
    let folder = dir.join("folder.json");
    fs::create_dir(&folder).unwrap();
    assert_eq!(Playable::load(&folder), Playable::default());
}

/// An answer with nothing in it is an answer: a webview that plays nothing
/// isn't read as never asked (core playable.rs, `Playable`).
#[test]
fn an_answer_of_nothing_is_kept_as_it_is() {
    let dir = crate::TempDir::new("playable-nothing");
    let path = dir.join("playable.json");
    let none = Playable { video: vec![], audio: vec![] };
    none.save(&path).unwrap();
    assert_eq!(Playable::load(&path), none);
}

/// A new answer replaces the old whole and leaves no half-written file
/// beside it (core playable.rs, `Playable::save`: written to a part file,
/// then renamed into place).
#[test]
fn a_new_answer_replaces_the_old_and_leaves_no_part_file() {
    let dir = crate::TempDir::new("playable-replaced");
    let path = dir.join("playable.json");
    Playable::default().save(&path).unwrap();
    let next = Playable { video: vec![(Video::Vp9, Audio::Opus)], audio: vec![Audio::Opus] };
    next.save(&path).unwrap();
    assert_eq!(Playable::load(&path), next);
    let names: Vec<_> = fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names, ["playable.json"]);
}

/// Saving where there's no folder says so, and doesn't pretend (core
/// playable.rs, `Playable::save`).
#[test]
fn saving_where_there_is_no_folder_fails() {
    let dir = crate::TempDir::new("playable-nofolder");
    assert!(Playable::default().save(&dir.join("missing/playable.json")).is_err());
}

// --------------------------------------------------------------- arrivals

/// "Whether a file of `streams` plays here: its video and audio as a pair
/// that does" (core playable.rs, `Playable::plays`).
#[test]
fn a_file_plays_when_its_video_and_audio_are_a_pair_that_plays() {
    let p = Playable { video: vec![(Video::H264, Audio::Aac), (Video::Vp9, Audio::Opus)], audio: vec![] };
    assert!(p.plays(&streams(Some("h264"), Some("aac"))));
    assert!(p.plays(&streams(Some("vp9"), Some("opus"))));
}

/// "Sites only claim their codecs (and some say "unknown"), so this is asked
/// of what arrived" (core playable.rs, `Playable::plays`): a video of a
/// family that plays, with a sound that isn't its pair, doesn't play.
#[test]
fn a_video_whose_sound_isnt_paired_with_it_doesnt_play() {
    let p = Playable { video: vec![(Video::H264, Audio::Aac), (Video::Vp9, Audio::Opus)], audio: vec![Audio::Mp3] };
    assert!(!p.plays(&streams(Some("h264"), Some("opus"))));
    assert!(!p.plays(&streams(Some("vp9"), Some("aac"))));
    assert!(!p.plays(&streams(Some("h264"), Some("mp3"))), "audio alone playing isn't a pair");
}

/// "Sites only claim their codecs" (core playable.rs, `Playable::plays`): a
/// family the list doesn't name doesn't play, nor one nobody names.
#[test]
fn a_codec_the_list_doesnt_name_doesnt_play() {
    let p = h264();
    assert!(!p.plays(&streams(Some("hevc"), Some("aac"))), "a family not in the list");
    assert!(!p.plays(&streams(Some("mpeg4"), Some("aac"))), "a codec with no family");
    assert!(!p.plays(&streams(Some("unknown"), Some("aac"))));
    assert!(!p.plays(&streams(Some("h264"), Some("pcm_s16le"))));
    assert!(!p.plays(&streams(Some("h264"), Some("unknown"))));
}

/// "its video and audio as a pair that does" (core playable.rs,
/// `Playable::plays`): each family of video and of sound, as ffmpeg names it,
/// is known: listed alone or with sound, it plays.
#[test]
fn every_codec_family_plays_when_it_is_listed() {
    let videos = [("av1", Video::Av1), ("vp9", Video::Vp9), ("hevc", Video::Hevc), ("h264", Video::H264)];
    let sounds = [
        ("opus", Audio::Opus),
        ("aac", Audio::Aac),
        ("mp3", Audio::Mp3),
        ("vorbis", Audio::Vorbis),
        ("flac", Audio::Flac),
    ];
    for (video_name, video) in videos {
        for (sound_name, sound) in sounds {
            let p = Playable { video: vec![(video, sound)], audio: vec![sound] };
            assert!(p.plays(&streams(Some(video_name), Some(sound_name))), "{video_name} with {sound_name}");
            assert!(p.plays(&streams(None, Some(sound_name))), "{sound_name} alone");
            let none = Playable { video: vec![], audio: vec![] };
            assert!(!none.plays(&streams(Some(video_name), Some(sound_name))), "{video_name} with {sound_name}");
        }
    }
}

/// "or, with no video, its audio alone" (core playable.rs, `Playable::plays`):
/// sound alone plays when it's in the list for sound alone, not for a pair's.
#[test]
fn sound_alone_plays_when_the_list_for_sound_has_it() {
    let p = h264();
    assert!(p.plays(&streams(None, Some("mp3"))));
    assert!(!p.plays(&streams(None, Some("aac"))), "AAC is a pair's here, not in the list for sound alone");
    assert!(!p.plays(&streams(None, Some("pcm_s16le"))));
}

/// A video with no sound doesn't play: nothing to transcribe or hear (core
/// playable.rs, `Playable::plays`), nor does a file with no streams.
#[test]
fn a_video_with_no_sound_and_a_file_with_no_streams_dont_play() {
    let p = h264();
    assert!(!p.plays(&streams(Some("h264"), None)));
    assert!(!p.plays(&streams(None, None)));
}

// ---------------------------------------------------------- the selector

/// "the first pair that plays here, as separate video and audio streams
/// (merged) or one stream with both, at most `height` tall" (core
/// playable.rs, `Playable::selector`): each pair in the order given, the
/// merged form before the one stream, each with the pair's codecs.
#[test]
fn the_pairs_that_play_are_asked_for_first_in_their_order_merged_then_whole() {
    let p = Playable {
        video: vec![(Video::Av1, Audio::Opus), (Video::H264, Audio::Aac), (Video::Hevc, Audio::Mp3)],
        audio: vec![],
    };
    let c = choices(&p.selector(720));
    assert_eq!(c[0], "bv*[height<=720][vcodec~='^(av01|av1)']+ba[acodec~='^opus']");
    assert_eq!(c[1], "b[height<=720][vcodec~='^(av01|av1)'][acodec~='^opus']");
    assert_eq!(c[2], "bv*[height<=720][vcodec~='^(avc1|avc3|h264)']+ba[acodec~='^(mp4a|aac)']");
    assert_eq!(c[3], "b[height<=720][vcodec~='^(avc1|avc3|h264)'][acodec~='^(mp4a|aac)']");
    assert_eq!(c[4], "bv*[height<=720][vcodec~='^(hvc1|hev1|hevc|h265)']+ba[acodec~='^mp3']");
    assert_eq!(c[5], "b[height<=720][vcodec~='^(hvc1|hev1|hevc|h265)'][acodec~='^mp3']");
}

/// Each family and codec is matched by its prefix, "as each site's player
/// does" (core playable.rs, module doc: "yt-dlp names codecs as each site's
/// player does, so they're matched by prefix").
#[test]
fn codecs_are_matched_by_the_names_sites_give_them() {
    let all = Playable {
        video: vec![
            (Video::Av1, Audio::Opus),
            (Video::Vp9, Audio::Aac),
            (Video::Hevc, Audio::Mp3),
            (Video::H264, Audio::Vorbis),
            (Video::H264, Audio::Flac),
        ],
        audio: vec![],
    };
    let c = choices(&all.selector(480));
    assert!(c[0].contains("^(av01|av1)") && c[0].contains("^opus"));
    assert!(c[2].contains("^(vp09|vp9)") && c[2].contains("^(mp4a|aac)"));
    assert!(c[4].contains("^(hvc1|hev1|hevc|h265)") && c[4].contains("^mp3"));
    assert!(c[6].contains("^(avc1|avc3|h264)") && c[6].contains("^vorbis"));
    assert!(c[8].contains("^(avc1|avc3|h264)") && c[8].contains("^flac"));
}

/// "at most `height` tall" (core playable.rs, `Playable::selector`): the
/// height asked is the limit of the pairs that play, and of the ones the site
/// doesn't name.
#[test]
fn the_height_asked_is_the_most_a_choice_may_be() {
    for height in [480, 1080] {
        let c = choices(&h264().selector(height));
        assert_eq!(c[0], format!("bv*[height<={height}][vcodec~='^(avc1|avc3|h264)']+ba[acodec~='^(mp4a|aac)']"));
        assert!(c[2].starts_with(&format!("b[height<={height}][ext=mp4]")), "{}", c[2]);
        assert!(c[6].starts_with(&format!("wv*[height>{height}]")), "{}", c[6]);
    }
}

/// "else the best video at most that tall whose codecs the site doesn't name
/// (Rumble's say "unknown"; almost always H.264), MP4s first and never Ogg"
/// (core playable.rs, `Playable::selector`).
#[test]
fn video_whose_codecs_the_site_doesnt_name_comes_next_mp4s_first_and_never_ogg() {
    let c = choices(&h264().selector(720));
    // What isn't of a family known not to play here, nor an older one.
    let skip = "[vcodec!~=?'^(av01|av1)'][vcodec!~=?'^(vp09|vp9)'][vcodec!~=?'^(hvc1|hev1|hevc|h265)'][vcodec!~=?'^(vp8|theora)']";
    assert_eq!(c[2], format!("b[height<=720][ext=mp4]{skip}"));
    assert_eq!(c[3], format!("bv*[height<=720][ext=mp4]{skip}+ba"));
    assert_eq!(c[4], format!("b[height<=720][ext!=ogv]{skip}"));
    assert_eq!(c[5], format!("bv*[height<=720][ext!=ogv]{skip}+ba"));
}

/// "not of a family known not to play here" (core playable.rs, `selector`):
/// a family that does play here isn't ruled out of the unnamed ones, and
/// every other family is, with the older ones nobody is asked about.
#[test]
fn a_family_that_plays_here_isnt_ruled_out_of_video_the_site_doesnt_name() {
    let vp9 = Playable { video: vec![(Video::Vp9, Audio::Opus)], audio: vec![] };
    let c = choices(&vp9.selector(720));
    let unnamed = c.iter().find(|c| c.contains("[ext=mp4]")).unwrap();
    let ruled_out = |name: &str| unnamed.contains(&format!("[vcodec!~=?'{name}']"));
    assert!(!ruled_out("^(vp09|vp9)"));
    assert!(ruled_out("^(av01|av1)") && ruled_out("^(hvc1|hev1|hevc|h265)") && ruled_out("^(avc1|avc3|h264)"));
    assert!(ruled_out("^(vp8|theora)"));
    let everything = Playable {
        video: vec![
            (Video::Av1, Audio::Opus),
            (Video::Vp9, Audio::Opus),
            (Video::Hevc, Audio::Aac),
            (Video::H264, Audio::Aac),
        ],
        audio: vec![],
    };
    let c = choices(&everything.selector(720));
    let unnamed = c.iter().find(|c| c.contains("[ext=mp4]")).unwrap();
    assert_eq!(unnamed.matches("[vcodec!~=?").count(), 1, "only the older ones: {unnamed}");
}

/// "else the smallest taller pair that plays here (a small channel's Twitch
/// VOD has only its source, often 1080p)" (core playable.rs, `selector`):
/// only after what's at most that tall, the smallest first ("w"), each pair
/// again, whole or merged.
#[test]
fn taller_video_comes_only_after_what_fits_and_the_smallest_first() {
    let p = Playable { video: vec![(Video::Av1, Audio::Opus), (Video::H264, Audio::Aac)], audio: vec![] };
    let c = choices(&p.selector(720));
    assert_eq!(c[8], "wv*[height>720][vcodec~='^(av01|av1)']+ba[acodec~='^opus']");
    assert_eq!(c[9], "w[height>720][vcodec~='^(av01|av1)'][acodec~='^opus']");
    assert_eq!(c[10], "wv*[height>720][vcodec~='^(avc1|avc3|h264)']+ba[acodec~='^(mp4a|aac)']");
    assert_eq!(c[11], "w[height>720][vcodec~='^(avc1|avc3|h264)'][acodec~='^(mp4a|aac)']");
}

/// "else audio alone that plays here (podcasts, music); else whatever the
/// site has, which is still transcribed even if it can't be played" (core
/// playable.rs, `selector`).
#[test]
fn audio_alone_that_plays_comes_next_and_whatever_the_site_has_last() {
    let p = Playable { video: vec![(Video::H264, Audio::Aac)], audio: vec![Audio::Opus, Audio::Mp3] };
    let c = choices(&p.selector(720));
    let n = c.len();
    assert_eq!(c[n - 4], "ba[vcodec=none][acodec~='^opus']");
    assert_eq!(c[n - 3], "ba[vcodec=none][acodec~='^mp3']");
    assert_eq!(&c[n - 2..], ["ba", "b"]);
}

/// A webview that plays nothing in particular is still given choices: the
/// unnamed video, then whatever the site has (core playable.rs, `selector`).
#[test]
fn with_nothing_known_to_play_only_unnamed_video_and_whatever_the_site_has_are_asked() {
    let c = choices(&Playable { video: vec![], audio: vec![] }.selector(720));
    assert_eq!(c.len(), 6);
    assert!(c[..4].iter().all(|c| c.contains("[ext=mp4]") || c.contains("[ext!=ogv]")));
    assert_eq!(&c[4..], ["ba", "b"]);
}

/// "Only streams yt-dlp downloads itself over the web (not rtsp:// or the
/// like, which it would hand to ffmpeg)" (core playable.rs, `selector`):
/// every stream of every choice, a merged choice's two included, is limited.
#[test]
fn every_stream_asked_for_is_one_yt_dlp_fetches_itself_over_the_web() {
    let p = Playable {
        video: vec![(Video::Av1, Audio::Opus), (Video::H264, Audio::Aac)],
        audio: vec![Audio::Opus, Audio::Aac],
    };
    let selector = p.selector(720);
    let limited = "[protocol~='^(https?|m3u8(_native)?|http_dash_segments)$']";
    assert_eq!(WEB, limited);
    for choice in selector.split('/') {
        for stream in choice.split('+') {
            assert!(stream.ends_with(limited), "{stream}");
        }
    }
    let distinct: HashSet<&str> = selector.split('/').collect();
    assert_eq!(distinct.len(), selector.split('/').count(), "no choice twice");
}

/// "`choice` limited to `WEB` streams: each format in it (`a+b` merges two)"
/// (core playable.rs, `with_web`).
#[test]
fn a_choice_is_limited_to_the_web_stream_by_stream() {
    assert_eq!(with_web("ba"), format!("ba{WEB}"));
    assert_eq!(
        with_web("bv*[height<=720]+ba[acodec~='^opus']"),
        format!("bv*[height<=720]{WEB}+ba[acodec~='^opus']{WEB}")
    );
}
