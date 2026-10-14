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

impl Playable {
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
    /// `height` tall; else audio alone that plays here (podcasts, music);
    /// else whatever the site has, which is still transcribed even if it
    /// can't be played.
    pub fn selector(&self, height: u32) -> String {
        let mut choices = Vec::new();
        for (v, a) in &self.video {
            let (v, a) = (v.pattern(), a.pattern());
            choices.push(format!("bv*[height<={height}][vcodec~='{v}']+ba[acodec~='{a}']"));
            choices.push(format!("b[height<={height}][vcodec~='{v}'][acodec~='{a}']"));
        }
        for a in &self.audio {
            choices.push(format!("ba[vcodec=none][acodec~='{}']", a.pattern()));
        }
        choices.push(format!("b[height<={height}]"));
        choices.push("ba".into());
        choices.push("b".into());
        choices.join("/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_what_plays_first_then_falls_back() {
        let p =
            Playable { video: vec![(Video::Av1, Audio::Opus), (Video::H264, Audio::Aac)], audio: vec![Audio::Opus] };
        let s = p.selector(720);
        let choices: Vec<&str> = s.split('/').collect();
        assert_eq!(choices[0], "bv*[height<=720][vcodec~='^(av01|av1)']+ba[acodec~='^opus']");
        assert_eq!(choices[2], "bv*[height<=720][vcodec~='^(avc1|avc3|h264)']+ba[acodec~='^(mp4a|aac)']");
        assert_eq!(choices[4], "ba[vcodec=none][acodec~='^opus']");
        assert_eq!(&choices[5..], ["b[height<=720]", "ba", "b"]);
    }

    #[test]
    fn reads_and_saves_what_plays() {
        let dir = crate::TempDir::new("playable");
        let path = dir.join("playable.json");
        assert_eq!(Playable::load(&path), Playable::default(), "never asked: H.264 and AAC");
        let p = Playable { video: vec![(Video::Vp9, Audio::Opus)], audio: vec![Audio::Opus, Audio::Flac] };
        p.save(&path).unwrap();
        assert_eq!(Playable::load(&path), p);
        let json = fs::read_to_string(&path).unwrap();
        assert_eq!(json, r#"{"video":[["vp9","opus"]],"audio":["opus","flac"]}"#);
    }
}
