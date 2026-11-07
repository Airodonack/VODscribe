//! Everything VODscribe does that isn't UI: fetching VOD audio with yt-dlp,
//! decoding it with ffmpeg, transcribing it with Parakeet (via sherpa-onnx)
//! and indexing the transcript in SQLite for phrase search.

pub mod annotations;
pub mod annotations_filter;
#[cfg(feature = "asr")]
pub mod asr;
pub mod audio;
pub mod chat;
pub mod choices;
pub mod clipboard;
pub mod clips;
pub mod db;
pub mod deps;
pub mod edits;
pub mod emotes;
pub mod fetch;
pub mod footage;
pub mod gpu;
pub mod kick;
pub mod layers;
pub mod library;
pub mod local;
pub mod mkv;
pub mod model;
pub mod net;
pub mod peaks;
pub mod pipeline;
pub mod playable;
pub mod query;
pub mod render;
pub mod search;
pub mod searcher;
pub mod settings;
pub mod sources;
pub mod stretches;
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;
pub mod thumbs;
pub mod time;
pub mod tools;
pub mod transcript;
pub mod twitch;
pub mod word_emotes;
pub mod youtube;

use std::path::{Path, PathBuf};

/// Where a download is, as the app shows it.
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Progress<'a> {
    /// "Downloading <name>", then "<name> matches its SHA-256".
    pub step: &'a str,
    /// Bytes so far across the whole install, for a bar that only moves
    /// forward, and the install's size if known.
    pub done: u64,
    pub total: Option<u64>,
    /// Bytes of the file under way, and its size if known.
    pub file_done: u64,
    pub file_size: Option<u64>,
    /// Which of the install's files this is, from 1, out of how many. Shown as
    /// "(2 of 5)", so a run of small downloads doesn't look like one repeating.
    pub part: usize,
    pub parts: usize,
}

impl<'a> Progress<'a> {
    /// A step of a single-file install with no bytes to count yet, such as
    /// checking a signature.
    pub fn step(step: &'a str) -> Self {
        Progress { step, done: 0, total: None, file_done: 0, file_size: None, part: 1, parts: 1 }
    }

    /// A file of `size` bytes (if known) with `n` of them done, as the whole of
    /// a single-file install. Place it in a bigger one with `within`.
    pub fn file(step: &'a str, n: u64, size: Option<u64>) -> Self {
        Progress { step, done: n, total: size, file_done: n, file_size: size, part: 1, parts: 1 }
    }

    /// This file as part `part` of `parts` in an install of `total` bytes,
    /// after `before` bytes of the files ahead of it.
    pub fn within(self, before: u64, total: Option<u64>, part: usize, parts: usize) -> Self {
        Progress { done: before + self.file_done, total, part, parts, ..self }
    }
}

/// The error a job ends with when it was told to stop (pause, cancel, delete).
/// Check for it with `is_canceled` rather than by its message.
#[derive(Debug)]
pub struct Canceled;

impl std::fmt::Display for Canceled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("canceled")
    }
}

impl std::error::Error for Canceled {}

/// Whether `e` is, or was caused by, `Canceled`, including inside an I/O error.
pub fn is_canceled(e: &anyhow::Error) -> bool {
    e.chain().any(|c| {
        c.is::<Canceled>()
            || c.downcast_ref::<std::io::Error>()
                .and_then(|io| io.get_ref())
                .is_some_and(|inner| inner.is::<Canceled>())
    })
}

/// Layout of the app's data. The library and settings live in `root`; what can be
/// downloaded again (tools, models, the GPU pack, audio) lives in `local`, which is
/// the same directory except on Windows, where `root` roams and `local` doesn't.
#[derive(Clone, Debug)]
pub struct Paths {
    pub root: PathBuf,
    pub local: PathBuf,
    /// Where 0.7.1 made clips and edits (VODscribe in the Videos folder),
    /// if known: the one folder outside the library `library::settle`
    /// takes files from, and where a clip's file it couldn't link in is
    /// read from (`db::clip_file_of`).
    pub old_clips: Option<PathBuf>,
}

/// Bytes in `path`: a file's, or everything under a folder (not following links).
fn size_on_disk(path: &std::path::Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return 0 };
    if !meta.is_dir() {
        return meta.len();
    }
    std::fs::read_dir(path).map_or(0, |entries| entries.flatten().map(|e| size_on_disk(&e.path())).sum())
}

impl Paths {
    /// Everything in one directory.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self { local: root.clone(), root, old_clips: None }
    }

    pub fn split(root: impl Into<PathBuf>, local: impl Into<PathBuf>) -> Self {
        Self { root: root.into(), local: local.into(), old_clips: None }
    }

    /// These paths, with 0.7.1's clip folder at `dir` (`old_clips`).
    pub fn with_old_clips(self, dir: impl Into<PathBuf>) -> Self {
        Self { old_clips: Some(dir.into()), ..self }
    }

    pub fn db(&self) -> PathBuf {
        self.root.join("library.sqlite")
    }

    pub fn bin(&self) -> PathBuf {
        self.local.join("bin")
    }

    pub fn models(&self) -> PathBuf {
        self.local.join("models")
    }

    pub fn settings(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    /// The page's small choices (see `choices`).
    pub fn choices(&self) -> PathBuf {
        self.root.join("choices.json")
    }

    /// What this computer's webview plays (`playable::Playable`).
    pub fn playable(&self) -> PathBuf {
        self.root.join("playable.json")
    }

    /// Timeline layers (see `layers`).
    pub fn layers(&self) -> PathBuf {
        self.root.join("layers.json")
    }

    pub fn gpu_pack(&self) -> PathBuf {
        self.local.join("gpu")
    }

    /// Local copies of VOD thumbnails. They could be fetched again, but they belong
    /// with the library, so Clear downloads (`cache_dirs`) leaves them.
    pub fn thumbs(&self) -> PathBuf {
        self.local.join("thumbs")
    }

    /// Each VOD's loudness over time (`peaks`). Read from audio that may be
    /// gone since, so not a cache.
    pub fn peaks(&self) -> PathBuf {
        self.local.join("peaks")
    }

    /// Small frames of each file here, for the timeline's thumbnail and a
    /// file's picture (crates/player `storyboard`). Made again if gone, but
    /// slowly, so not a cache either.
    pub fn storyboards(&self) -> PathBuf {
        self.local.join("storyboards")
    }

    /// Chat emote images (see `emotes`).
    pub fn emotes(&self) -> PathBuf {
        self.local.join("emotes")
    }

    /// Scratch space for in-flight downloads.
    pub fn work(&self) -> PathBuf {
        self.local.join("work")
    }

    /// The videos kept from sites other than YouTube, Twitch and Kick, to play
    /// here (`pipeline::keeps_video`), as `<vod id>.<ext>`. Not a cache: a VOD's
    /// stays until it's deleted, or removed from the VOD. Beside the work dir,
    /// never in it, so clearing a VOD's work files after transcribing can't
    /// take its video.
    pub fn media(&self) -> PathBuf {
        self.local.join("media")
    }

    // The library's own files, one function per kind, each built from its
    // row's id (and an extension, where what's made decides it). Nothing
    // in the library stores where a file is: a copy of the library opened
    // from another folder, or on another computer, finds them all. Named
    // for the folders the library will have (`source_`: a VOD's files;
    // `clip_`: a clip's or an edit's; `temp_`: emptied at every start), so
    // a new layout changes only these bodies.

    /// Where VOD `vod_id`'s kept video downloads to, before its extension
    /// (`pipeline::keeps_video`); its part files start the same. The name a
    /// user sees is given only where it's handed out. Videos kept from 0.4.0
    /// to 0.7.1 were named `<title> [<id>]` (`pipeline::owner` still knows
    /// them, so their leftovers go).
    pub fn source_video_stem(&self, vod_id: i64) -> PathBuf {
        self.media().join(vod_id.to_string())
    }

    /// VOD `vod_id`'s kept video, once it's here (`Db::set_video`).
    pub fn source_video(&self, vod_id: i64, ext: &str) -> PathBuf {
        with_ext(self.source_video_stem(vod_id), ext)
    }

    /// Where VOD `vod_id`'s whole audio downloads to, before its extension.
    pub fn source_audio_stem(&self, vod_id: i64) -> PathBuf {
        self.work().join(vod_id.to_string())
    }

    /// VOD `vod_id`'s whole audio, downloaded to transcribe it.
    pub fn source_audio(&self, vod_id: i64, ext: &str) -> PathBuf {
        with_ext(self.source_audio_stem(vod_id), ext)
    }

    /// Where request `stretch_id`'s section of VOD `vod_id`'s audio downloads
    /// to, before its extension.
    pub fn source_section_stem(&self, vod_id: i64, stretch_id: i64) -> PathBuf {
        self.work().join(format!("{vod_id}.s{stretch_id}"))
    }

    /// Request `stretch_id`'s section of VOD `vod_id`'s audio.
    pub fn source_section(&self, vod_id: i64, stretch_id: i64, ext: &str) -> PathBuf {
        with_ext(self.source_section_stem(vod_id, stretch_id), ext)
    }

    /// Where files dropped from another app are kept (`source_received`);
    /// they arrive in a folder of its own, "incoming", as `<name>.part`,
    /// till each is added (desktop receive.rs).
    pub fn received(&self) -> PathBuf {
        self.media().join("received")
    }

    /// Where files being received are written, and wait to be added: a
    /// folder of their own, so a name one comes with ("1.mp4") is never the
    /// one a VOD's file is kept under (`source_received`).
    pub fn received_incoming(&self) -> PathBuf {
        self.received().join("incoming")
    }

    /// VOD `vod_id`'s file, received from another app's drop: the app's own
    /// copy, which it plays as a file on this computer.
    pub fn source_received(&self, vod_id: i64, ext: &str) -> PathBuf {
        with_ext(self.received().join(vod_id.to_string()), ext)
    }

    /// Stretches of VODs downloaded for clips and edits (`footage`).
    pub fn source_footage(&self) -> PathBuf {
        self.local.join("footage")
    }

    /// A VOD's picture, `name` as `Vod::thumb` has it.
    pub fn source_thumb(&self, name: &str) -> PathBuf {
        self.thumbs().join(name)
    }

    /// The storyboard of `file`, one of VOD `vod_id`'s files, named for its
    /// path. (Its header holds the path too, so one of a file that moved is
    /// made again: a cache, slow to make, but never wrong.)
    pub fn source_storyboard(&self, vod_id: i64, file: &Path) -> PathBuf {
        self.storyboards().join(format!("{vod_id}-{:016x}.sb", fnv1a(file.to_string_lossy().as_bytes())))
    }

    /// Clips' and edits' files, and the names they're handed out by.
    pub fn clips(&self) -> PathBuf {
        self.local.join("clips")
    }

    /// Clip `clip_id`'s file, cut or rendered (`Db::set_clip_file`). Edits are
    /// clips (the listed ones), so an edit's render is here too.
    pub fn clip_file(&self, clip_id: i64, ext: &str) -> PathBuf {
        with_ext(self.clips().join(clip_id.to_string()), ext)
    }

    /// Where clip `clip_id`'s file is handed out under the names it's known
    /// by (dragged to another app, copied, shown in its folder): beside its
    /// own, and gone with it.
    pub fn clip_names(&self, clip_id: i64) -> PathBuf {
        self.clips().join(clip_id.to_string())
    }

    /// Clips' first frames (`clip_thumb`).
    pub fn clip_thumbs(&self) -> PathBuf {
        self.local.join("clip-thumbs")
    }

    /// Clip `clip_id`'s first frame, shown while it's dragged out.
    pub fn clip_thumb(&self, clip_id: i64) -> PathBuf {
        self.clip_thumbs().join(format!("{clip_id}.png"))
    }

    /// What VOD `vod_id`'s site said of it, for its audio (`pipeline::fresh_info`).
    pub fn source_info(&self, vod_id: i64) -> PathBuf {
        self.work().join(format!("{vod_id}.info.json"))
    }

    /// What VOD `vod_id`'s site said of it, for its video kept whole.
    pub fn source_video_info(&self, vod_id: i64) -> PathBuf {
        self.work().join(format!("video-{vod_id}.info.json"))
    }

    /// What VOD `vod_id`'s site said of it, for its footage's sections.
    pub fn source_footage_info(&self, vod_id: i64) -> PathBuf {
        self.work().join(format!("footage-{vod_id}.info.json"))
    }

    /// Where footage section `name` (`footage::section_stem`) downloads to,
    /// before its extension, till it goes to `source_footage`.
    pub fn source_footage_download(&self, name: &str) -> PathBuf {
        self.work().join(format!("footage-{name}"))
    }

    /// Emote images matched to words in chat (`word_emotes`).
    pub fn cache_word_emotes(&self) -> PathBuf {
        self.emotes().join("words")
    }

    /// Where VOD `vod_id`'s YouTube chat downloads to, before its extension
    /// (`chat::fetch`). Not named `<id>.…`: `pipeline::remove_work_files`
    /// clears those when a transcription ends, which may be while the chat
    /// is still downloading.
    pub fn temp_chat(&self, vod_id: i64) -> PathBuf {
        self.work().join(format!("chat-{vod_id}"))
    }

    /// What a site said of a link looked up (`n` makes it unique), before
    /// it's a VOD of the library's.
    pub fn temp_probe(&self, n: u128) -> PathBuf {
        self.work().join(format!("probe-{n}.json"))
    }

    /// What a site's player is fed from here (the embed takeover).
    pub fn temp_embed(&self) -> PathBuf {
        self.local.join("embed")
    }

    /// Edits' preview segments from before edits played in the app's own
    /// player: nothing reads them now.
    pub fn temp_preview(&self) -> PathBuf {
        self.local.join("preview")
    }

    /// The last filter of published annotations fetched (`annotations_filter`),
    /// with its ETag beside it in a file of the same name ending `.etag`, so the
    /// next check asks only whether it changed.
    pub fn annotations_filter(&self) -> PathBuf {
        self.local.join("annotations-filter.bin")
    }

    pub fn model_dir(&self) -> PathBuf {
        model::model_dir(&self.models(), &model::CPU_MODEL)
    }

    pub fn gpu_model_dir(&self) -> PathBuf {
        model::model_dir(&self.models(), &model::GPU_MODEL)
    }

    /// Everything that can be downloaded again: models, tools, the GPU pack,
    /// emote images and in-flight downloads. The library and settings are not cache.
    pub fn cache_dirs(&self) -> [PathBuf; 6] {
        let gpu = self.gpu_pack();
        [self.models(), self.bin(), gpu::staging_dir(&gpu), gpu, self.emotes(), self.work()]
    }

    /// Bytes used by `cache_dirs`.
    pub fn cache_size(&self) -> u64 {
        self.cache_dirs().iter().map(|d| size_on_disk(d)).sum()
    }

    /// Bytes used by the kept videos (`media`).
    pub fn media_size(&self) -> u64 {
        size_on_disk(&self.media())
    }

    /// Deletes `cache_dirs`. The ones the app always needs are recreated empty.
    pub fn clear_cache(&self) -> std::io::Result<()> {
        for dir in self.cache_dirs() {
            match std::fs::remove_dir_all(&dir) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
        }
        self.ensure()
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        for dir in [self.root.clone(), self.local.clone(), self.bin(), self.models(), self.work(), self.thumbs()] {
            std::fs::create_dir_all(dir)?;
        }
        // The library is private to this user: other accounts can't list or read it.
        #[cfg(unix)]
        for dir in [&self.root, &self.local] {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }
}

/// `stem` with `.ext` after it (a stem's own dots, as in `12.s3`, kept).
fn with_ext(stem: PathBuf, ext: &str) -> PathBuf {
    let mut name = stem.into_os_string();
    name.push(".");
    name.push(ext);
    name.into()
}

/// FNV-1a: the same for the same bytes in every version and on every
/// system, as file names kept from one run to the next must be.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3))
}

/// The tests' temporary folders (`testkit::TempDir`), as the tests here
/// name them.
#[cfg(test)]
pub(crate) use testkit::TempDir;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clearing_the_cache_keeps_the_library_and_settings() {
        let dir = TempDir::new("cache");
        let paths = Paths::new(dir.join("data"));
        paths.ensure().unwrap();
        for dir in paths.cache_dirs() {
            std::fs::create_dir_all(dir.join("nested")).unwrap();
            std::fs::write(dir.join("nested/file"), b"12345").unwrap();
        }
        std::fs::write(paths.db(), b"library").unwrap();
        std::fs::write(paths.settings(), b"{}").unwrap();
        assert_eq!(paths.cache_size(), 5 * paths.cache_dirs().len() as u64);

        paths.clear_cache().unwrap();
        assert_eq!(paths.cache_size(), 0);
        let empty = |d: &PathBuf| std::fs::read_dir(d).map_or(true, |mut e| e.next().is_none());
        assert!(paths.cache_dirs().iter().all(empty), "cache dirs are gone or empty");
        assert_eq!(std::fs::read(paths.db()).unwrap(), b"library");
        assert_eq!(std::fs::read(paths.settings()).unwrap(), b"{}");
    }

    /// Every file of the library's is where `Paths` builds it: no code outside
    /// this module joins a folder of the library's to a name by hand, from
    /// `Paths` or from a folder handed in by its name (`work_dir`, `media`),
    /// or finds one beside another (`with_file_name("media")`).
    #[test]
    fn only_paths_joins_the_librarys_folders() {
        let folders = [
            "root",
            "local",
            "work()",
            "media()",
            "thumbs()",
            "peaks()",
            "storyboards()",
            "emotes()",
            "received()",
            "clips()",
            "clip_thumbs()",
            "source_footage()",
            "bin()",
            "models()",
            "gpu_pack()",
        ];
        // Folders holding the library's own files, as a variable or
        // parameter names them when it's handed one.
        let handed = [
            "work",
            "media",
            "received",
            "clips",
            "clip_thumbs",
            "footage",
            "thumbs",
            "peaks",
            "storyboards",
            "emotes",
        ];
        let by_hand = |line: &str| {
            folders.iter().any(|f| line.contains(&format!(".{f}.join(")))
                || handed.iter().any(|f| {
                    line.contains(&format!("with_file_name(\"{f}\")"))
                        || ["", "_dir"].iter().any(|end| {
                            let join = format!("{f}{end}.join(");
                            line.match_indices(&join).any(|(at, _)| {
                                !line[..at].ends_with(|c: char| c.is_alphanumeric() || c == '_' || c == '.')
                            })
                        })
                })
        };
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut found = Vec::new();
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(repo.join("crates"))
            .unwrap()
            .flatten()
            .map(|e| e.path().join("src"))
            .chain([repo.join("apps/desktop/src-tauri/src")])
            .collect();
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                    continue;
                }
                // Tests make folders of their own as they like, in files of
                // their own (`clips/tests.rs`) or a module's `mod tests`.
                let tests = path.file_name().is_some_and(|n| n == "tests.rs");
                if path.extension().is_none_or(|e| e != "rs") || path.ends_with("crates/core/src/lib.rs") || tests {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap();
                let code = text.split("\nmod tests {").next().unwrap_or_default();
                for (n, line) in code.lines().enumerate() {
                    if by_hand(line) {
                        found.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
                    }
                }
            }
        }
        assert!(found.is_empty(), "joined by hand, not built by Paths:\n{}", found.join("\n"));
    }

    #[test]
    fn a_file_placed_within_an_install() {
        let p = Progress::file("Downloading b", 30, Some(100)).within(250, Some(400), 2, 3);
        assert_eq!((p.done, p.total, p.file_done, p.file_size, p.part, p.parts), (280, Some(400), 30, Some(100), 2, 3));
        let alone = Progress::file("Downloading yt-dlp", 7, None);
        assert_eq!((alone.done, alone.total, alone.part, alone.parts), (7, None, 1, 1));
    }
}
