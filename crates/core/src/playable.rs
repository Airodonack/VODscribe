//! What this computer's webview can play, for the video VODscribe keeps from
//! sites other than YouTube, Twitch and Kick (which play in their own
//! embeds). The interface asks the webview at each start (playable.ts) and
//! the answer is saved here, so downloads, which may run with no window open,
//! pick a format that plays. yt-dlp names codecs as each site's player does,
//! so they're matched by prefix.

use std::fs;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// A video codec family, as yt-dlp's `vcodec` starts.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Video {
    Av1,
    Vp9,
    Hevc,
    H264,
}

/// An audio codec family, as yt-dlp's `acodec` starts.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Audio {
    Opus,
    Aac,
    Mp3,
    Vorbis,
    Flac,
}

impl Video {
    fn pattern(self) -> &'static str {
        match self {
            Video::Av1 => "^(av01|av1)",
            Video::Vp9 => "^(vp09|vp9)",
            Video::Hevc => "^(hvc1|hev1|hevc|h265)",
            Video::H264 => "^(avc1|avc3|h264)",
        }
    }
}

impl Audio {
    fn pattern(self) -> &'static str {
        match self {
            Audio::Opus => "^opus",
            Audio::Aac => "^(mp4a|aac)",
            Audio::Mp3 => "^mp3",
            Audio::Vorbis => "^vorbis",
            Audio::Flac => "^flac",
        }
    }
}

/// What plays here, best first: video and audio pairs (in an MP4), then
/// audio alone. The interface orders them: what the hardware decodes first,
/// smaller files first among those.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Playable {
    pub video: Vec<(Video, Audio)>,
    pub audio: Vec<Audio>,
}

impl Default for Playable {
    /// Before the webview has been asked: what every webview plays.
    fn default() -> Self {
        Self { video: vec![(Video::H264, Audio::Aac)], audio: vec![Audio::Aac, Audio::Mp3] }
    }
}

/// A file's video and audio codecs, by ffmpeg's names (`local::codecs`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Streams {
    pub video: Option<String>,
    pub audio: Option<String>,
}

impl Video {
    /// The family of ffmpeg's codec `name`.
    fn of(name: &str) -> Option<Self> {
        Some(match name {
            "av1" => Video::Av1,
            "vp9" => Video::Vp9,
            "hevc" => Video::Hevc,
            "h264" => Video::H264,
            _ => return None,
        })
    }
}

impl Audio {
    fn of(name: &str) -> Option<Self> {
        Some(match name {
            "opus" => Audio::Opus,
            "aac" => Audio::Aac,
            "mp3" => Audio::Mp3,
            "vorbis" => Audio::Vorbis,
            "flac" => Audio::Flac,
            _ => return None,
        })
    }
}

impl Playable {
    /// Whether a file of `streams` plays here: its video and audio as a pair
    /// that does, or, with no video, its audio alone. Sites only claim their
    /// codecs (and some say "unknown"), so this is asked of what arrived.
    pub fn plays(&self, streams: &Streams) -> bool {
        let audio = streams.audio.as_deref().and_then(Audio::of);
        match (&streams.video, audio) {
            (Some(v), Some(a)) => Video::of(v).is_some_and(|v| self.video.contains(&(v, a))),
            (None, Some(a)) => self.audio.contains(&a),
            _ => false,
        }
    }

    pub fn load(path: &Path) -> Self {
        fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, serde_json::to_vec(self)?)?;
        fs::rename(tmp, path)?;
        Ok(())
    }

    /// yt-dlp's format selection: the first pair that plays here, as separate
    /// video and audio streams (merged) or one stream with both, at most
    /// `height` tall; else the best video at most that tall whose codecs the
    /// site doesn't name (Rumble's say "unknown"; almost always H.264), MP4s
    /// first and never Ogg; else the smallest taller pair that plays here (a
    /// small channel's Twitch VOD has only its source, often 1080p); else
    /// audio alone that plays here (podcasts, music); else whatever the site
    /// has, which is still transcribed even if it can't be played.
    pub fn selector(&self, height: u32) -> String {
        let mut choices = Vec::new();
        for (v, a) in &self.video {
            let (v, a) = (v.pattern(), a.pattern());
            choices.push(format!("bv*[height<={height}][vcodec~='{v}']+ba[acodec~='{a}']"));
            choices.push(format!("b[height<={height}][vcodec~='{v}'][acodec~='{a}']"));
        }
        // Video the site doesn't name ("!~=?" keeps a format with no codec
        // given): not of a family known not to play here, nor older ones no
        // webview here is asked about. Sites that name no codecs (archive.org)
        // still give the file type: an MP4 is almost always H.264, and an Ogg
        // video (Theora) plays in none of them.
        let unplayable: String = [Video::Av1, Video::Vp9, Video::Hevc, Video::H264]
            .into_iter()
            .filter(|v| !self.video.iter().any(|(pv, _)| pv == v))
            .map(Video::pattern)
            .chain(["^(vp8|theora)"])
            .map(|p| format!("[vcodec!~=?'{p}']"))
            .collect();
        for ext in ["[ext=mp4]", "[ext!=ogv]"] {
            choices.push(format!("b[height<={height}]{ext}{unplayable}"));
            choices.push(format!("bv*[height<={height}]{ext}{unplayable}+ba"));
        }
        for (v, a) in &self.video {
            let (v, a) = (v.pattern(), a.pattern());
            choices.push(format!("wv*[height>{height}][vcodec~='{v}']+ba[acodec~='{a}']"));
            choices.push(format!("w[height>{height}][vcodec~='{v}'][acodec~='{a}']"));
        }
        for a in &self.audio {
            choices.push(format!("ba[vcodec=none][acodec~='{}']", a.pattern()));
        }
        choices.push("ba".into());
        choices.push("b".into());
        // Only streams yt-dlp downloads itself over the web (not rtsp:// or
        // the like, which it would hand to ffmpeg).
        choices.iter().map(|c| with_web(c)).collect::<Vec<_>>().join("/")
    }
}

/// What web downloads yt-dlp makes itself: plain files, HLS and DASH.
/// (`fetch::download` has yt-dlp fetch HLS itself, rather than through ffmpeg.)
pub(crate) const WEB: &str = "[protocol~='^(https?|m3u8(_native)?|http_dash_segments)$']";

/// `choice` limited to `WEB` streams: each format in it (`a+b` merges two).
pub(crate) fn with_web(choice: &str) -> String {
    choice.split('+').map(|f| format!("{f}{WEB}")).collect::<Vec<_>>().join("+")
}

#[cfg(test)]
mod tests;
