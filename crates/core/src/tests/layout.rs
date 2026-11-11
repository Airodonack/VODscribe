//! The library's layout: where `Paths` puts each file, and what clearing the cache keeps.

use super::*;
use crate::testkit::Library;
use rusqlite::{Connection, OpenFlags};

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
    let handed =
        ["work", "media", "received", "clips", "clip_thumbs", "footage", "thumbs", "peaks", "storyboards", "emotes"];
    let by_hand = |line: &str| {
        folders.iter().any(|f| line.contains(&format!(".{f}.join(")))
            || handed.iter().any(|f| {
                line.contains(&format!("with_file_name(\"{f}\")"))
                    || ["", "_dir"].iter().any(|end| {
                        let join = format!("{f}{end}.join(");
                        line.match_indices(&join)
                            .any(|(at, _)| !line[..at].ends_with(|c: char| c.is_alphanumeric() || c == '_' || c == '.'))
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
            // their own (`clips/tests.rs`, or `clips/tests/*.rs` for one
            // in parts) or a module's `mod tests`.
            let tests = path.file_name().is_some_and(|n| n == "tests.rs")
                || path.parent().is_some_and(|dir| dir.ends_with("tests"));
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

/// A library split as it is on Windows, where `root` roams and `local`
/// doesn't, in folders that need not exist.
fn split() -> Paths {
    Paths::split("/lib/roaming", "/lib/local")
}

/// Column `column` of `table`'s row `id`, read as 0.7.1 reads it: through
/// a read-only connection of its own, not through `Db`.
fn old_column(paths: &Paths, table: &str, column: &str, id: i64) -> Option<String> {
    let conn = Connection::open_with_flags(paths.db(), OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let sql = format!("SELECT {column} FROM {table} WHERE {} = ?1", if table == "renders" { "clip_id" } else { "id" });
    conn.query_row(&sql, [id], |r| r.get(0)).unwrap()
}

/// The library and settings are in `root`; what downloads again is in
/// `local`, which is `root` again unless the two are given apart
/// (`Paths::root`, `Paths::local`: "the same directory except on Windows,
/// where `root` roams and `local` doesn't").
#[test]
fn the_library_and_settings_are_in_root_and_what_downloads_again_in_local() {
    let apart = split();
    for (what, at) in [
        ("the library", apart.db()),
        ("the settings", apart.settings()),
        ("the choices", apart.choices()),
        ("what plays", apart.playable()),
        ("the layers", apart.layers()),
    ] {
        assert!(at.starts_with(&apart.root), "{what} is in root: {at:?}");
    }
    for (what, at) in [
        ("tools", apart.bin()),
        ("models", apart.models()),
        ("the GPU pack", apart.gpu_pack()),
        ("downloads under way", apart.work()),
        ("kept videos", apart.media()),
        ("pictures", apart.thumbs()),
        ("loudness", apart.peaks()),
        ("storyboards", apart.storyboards()),
        ("emote images", apart.emotes()),
        ("clips", apart.clips()),
        ("footage", apart.source_footage()),
    ] {
        assert!(at.starts_with(&apart.local), "{what} is in local: {at:?}");
    }
    assert_eq!(apart.old_clips, None);
    let with = apart.clone().with_old_clips("/videos/VODscribe");
    assert_eq!(with.old_clips.as_deref(), Some(Path::new("/videos/VODscribe")));
    assert_eq!((&with.root, &with.local), (&apart.root, &apart.local));
    let one = Paths::new("/lib/one");
    assert_eq!((&one.root, &one.local), (&PathBuf::from("/lib/one"), &PathBuf::from("/lib/one")));
    assert_eq!(one.db(), Path::new("/lib/one/library.sqlite"));
    assert_eq!(one.work(), Path::new("/lib/one/work"));
}

/// Each kind of file is where its kind and its row's id build it: nothing
/// stores a path, so a library copied to another folder finds them all
/// (db.rs's "each is built from its kind and its row's id (`Paths`)";
/// plans/0.7.2.md item 14, owner, 2026-11-05: "paths are not even stored
/// but constructed in code"). A place that changes is a migration, so the
/// places are pinned here.
#[test]
fn each_kind_of_file_has_its_place_built_from_its_rows_id() {
    let p = split();
    let local = |at: PathBuf| at.strip_prefix(&p.local).unwrap().to_path_buf();
    let table = [
        ("kept video", local(p.source_video(12, "mp4")), "media/12.mp4"),
        ("kept video's parts", local(p.source_video_stem(12)), "media/12"),
        ("whole audio", local(p.source_audio(12, "m4a")), "work/12.m4a"),
        ("a section", local(p.source_section(12, 3, "m4a")), "work/12.s3.m4a"),
        ("a received file", local(p.source_received(12, "mkv")), "media/received/12.mkv"),
        ("files being received", local(p.received_incoming()), "media/received/incoming"),
        ("a clip's file, make 0", local(p.clip_make(7, 0, "mp4")), "clips/7.mp4"),
        ("a clip's file, make 2", local(p.clip_make(7, 2, "mp4")), "clips/7-2.mp4"),
        ("a clip's names", local(p.clip_names(7)), "clips/7"),
        ("a clip's picture", local(p.clip_thumb(7)), "clip-thumbs/7.png"),
        ("footage", local(p.source_footage()), "footage"),
        ("a VOD's picture", local(p.source_thumb("12.jpg")), "thumbs/12.jpg"),
        ("a VOD's info, for its audio", local(p.source_info(12)), "work/12.info.json"),
        ("a VOD's info, for its video", local(p.source_video_info(12)), "work/video-12.info.json"),
        ("a VOD's info, for its footage", local(p.source_footage_info(12)), "work/footage-12.info.json"),
        ("the leftovers list", local(p.leftovers()), "leftovers.json"),
        ("a footage section's download", local(p.source_footage_download("12-1000")), "work/footage-12-1000"),
        ("emote images matched to words", local(p.cache_word_emotes()), "emotes/words"),
        ("a link's look-up", local(p.temp_probe(5)), "work/probe-5.json"),
        ("what a site's player is fed from", local(p.temp_embed()), "embed"),
        ("edits' old preview segments", local(p.temp_preview()), "preview"),
        ("the published filter", local(p.annotations_filter()), "annotations-filter.bin"),
        ("the model", local(p.model_dir()), "models/parakeet-tdt-0.6b-v3-int8wo"),
        ("the GPU model", local(p.gpu_model_dir()), "models/parakeet-tdt-0.6b-v3-fp16"),
    ];
    for (kind, built, expected) in table {
        assert_eq!(built, Path::new(expected), "{kind}");
    }
    assert_eq!(p.db().strip_prefix(&p.root).unwrap(), Path::new("library.sqlite"));
    assert_eq!(p.settings().strip_prefix(&p.root).unwrap(), Path::new("settings.json"));
}

/// Every make of a clip writes a file of its own, so none rewrites the one
/// another app may have been given; make 0 is the one file a clip had
/// before (lib.rs, `clip_make`: "Make 0 is the one file a clip had
/// before"; plans/clips-pilot.md "Decided" 4b).
#[test]
fn every_make_of_a_clip_writes_a_file_of_its_own_and_make_0_is_the_one_from_before() {
    let p = split();
    assert_eq!(p.clip_make(7, 0, "mp4"), p.clip_file(7, "mp4"));
    let makes: Vec<PathBuf> = (0..4).map(|make| p.clip_make(7, make, "mp4")).collect();
    for (i, a) in makes.iter().enumerate() {
        for b in &makes[i + 1..] {
            assert_ne!(a, b, "each make has a file of its own");
        }
    }
    assert_ne!(p.clip_make(7, 1, "mp4"), p.clip_make(71, 0, "mp4"), "a make of clip 7 isn't clip 71's file");
    assert_ne!(p.clip_make(7, 1, "mp4"), p.clip_make(7, 1, "mkv"), "the extension is part of the name");
}

/// A clip's names are handed out beside its own file, and go with it
/// (lib.rs, `clip_names`: "beside its own, and gone with it"): in the
/// clips folder, named for its id, as every file of the clip is.
#[test]
fn a_clips_names_are_beside_its_own_file_and_go_with_it() {
    let p = split();
    let names = p.clip_names(7);
    assert_eq!(names.parent(), p.clip_file(7, "mp4").parent());
    assert_eq!(names.file_name(), p.clip_file(7, "mp4").file_stem());
    assert_eq!(names.parent(), p.clip_make(7, 3, "mp4").parent());
    assert_ne!(names, p.clip_names(71), "a clip's names aren't another's");
}

/// A stem's own dots, as in `12.s3`, are kept before the extension
/// (lib.rs, `with_ext`: "a stem's own dots, as in `12.s3`, kept").
#[test]
fn a_stems_own_dots_are_kept() {
    let p = split();
    assert_eq!(p.source_section(12, 3, "m4a").file_name().unwrap(), "12.s3.m4a");
    assert_eq!(p.source_section(12, 3, "tar.gz").file_name().unwrap(), "12.s3.tar.gz");
    assert_eq!(p.source_section_stem(12, 3).file_name().unwrap(), "12.s3");
}

/// A file's storyboard has the same name in every version and on every
/// system, so one made by a run is found by the next (lib.rs, `fnv1a`:
/// "the same for the same bytes in every version and on every system, as
/// file names kept from one run to the next must be"). A fixed path gives
/// a fixed name: the vector is FNV-1a (64 bit) of the path's bytes.
#[test]
fn a_files_storyboard_name_is_the_same_in_every_version_and_on_every_system() {
    let p = split();
    let name = |vod: i64, file: &str| p.source_storyboard(vod, Path::new(file)).file_name().unwrap().to_owned();
    assert_eq!(name(3, "/x/media/1.mp4"), "3-2344e9f05baf2984.sb");
    assert_eq!(name(3, "/x/media/1.mp4"), name(3, "/x/media/1.mp4"));
    assert_ne!(name(3, "/x/media/1.mp4"), name(3, "/x/media/2.mp4"));
    assert_ne!(name(3, "/x/media/1.mp4"), name(4, "/x/media/1.mp4"));
    assert_eq!(p.source_storyboard(3, Path::new("/x/media/1.mp4")).parent().unwrap(), p.storyboards());
}

/// Each writer that records a file puts its full path beside the
/// extension, in the column 0.7.1 reads (db.rs's migration 39: "The old
/// columns still get the full path written beside the extension, for 0.7.1
/// opened on this library"; library.rs: "the app's writers keep both columns
/// the same way"; plans/0.7.2.md line 51: "the agent's alternative keeps
/// old columns filled with the new full path").
#[test]
fn the_old_columns_get_the_full_path_beside_the_extension_for_071() {
    let lib = Library::new();
    let (db, p) = (&lib.db, &lib.paths);
    let vod = lib.add_site_vod("youtube", "abc", "A stream", 600.0).id;
    let full = |at: PathBuf| at.to_string_lossy().into_owned();

    let video = full(p.source_video(vod, "mp4"));
    db.set_media_path(vod, Some(&video)).unwrap();
    assert_eq!(old_column(p, "vods", "media_path", vod), Some(video), "a kept video");
    assert_eq!(old_column(p, "vods", "video_ext", vod).as_deref(), Some("mp4"));

    let audio = full(p.source_audio(vod, "m4a"));
    db.set_audio_path(vod, Some(&audio)).unwrap();
    assert_eq!(old_column(p, "vods", "audio_path", vod), Some(audio), "whole audio");
    assert_eq!(old_column(p, "vods", "audio_ext", vod).as_deref(), Some("m4a"));

    let received = lib.add_site_vod("youtube", "def", "Dropped", 60.0).id;
    let kept = full(p.source_received(received, "mkv"));
    db.set_received(received, &kept).unwrap();
    assert_eq!(old_column(p, "vods", "source_path", received), Some(kept), "a received file");
    assert_eq!(old_column(p, "vods", "received_ext", received).as_deref(), Some("mkv"));

    let section = db.request(vod, 10.0, Some(20.0)).unwrap().id;
    let at = full(p.source_section(vod, section, "m4a"));
    db.set_stretch_audio(section, Some(&at)).unwrap();
    assert_eq!(old_column(p, "stretches", "audio", section), Some(at), "a section");
    assert_eq!(old_column(p, "stretches", "audio_ext", section).as_deref(), Some("m4a"));

    let clip = db.add_clip(vod, 10.0, 20.0, "Clip", true).unwrap();
    let file = full(p.clip_make(clip.id, 0, "mp4"));
    assert!(db.set_clip_file(clip.id, 10.0, 20.0, crate::db::ClipStatus::Ready, Some(&file), None).unwrap());
    assert_eq!(old_column(p, "renders", "file_path", clip.id), Some(file), "a clip's file");
    assert_eq!(old_column(p, "renders", "file_ext", clip.id).as_deref(), Some("mp4"));
    let again = full(p.clip_make(clip.id, 1, "mp4"));
    assert!(db.set_clip_file(clip.id, 10.0, 20.0, crate::db::ClipStatus::Ready, Some(&again), None).unwrap());
    assert_eq!(old_column(p, "renders", "file_path", clip.id), Some(again), "a clip's second make");

    let edit = db.add_edit("Edit").unwrap().id;
    let rendered = full(p.clip_make(edit, 0, "mp4"));
    db.set_edit_render(edit, crate::db::ClipStatus::Ready, Some(&rendered), None, None).unwrap();
    assert_eq!(old_column(p, "renders", "file_path", edit), Some(rendered), "an edit's file");
    assert_eq!(old_column(p, "renders", "file_ext", edit).as_deref(), Some("mp4"));
}

/// Only a VOD's file on this computer, outside the library, keeps its path
/// (db.rs's migration 39: "Only a VOD's file on this computer, outside the
/// library, keeps its path (`source_path`)"; library.rs: "A VOD's file on
/// this computer (`source_path`) outside the library stays where it is, its
/// path kept: the user's own").
#[test]
fn a_vods_own_file_outside_the_library_keeps_its_path() {
    let lib = Library::new();
    let vod = lib.add_file("Day 3.mp4", &crate::testkit::Media::video(60.0));
    let path = vod.source_path.clone().unwrap();
    assert!(!Path::new(&path).starts_with(&lib.paths.root), "{path} is outside the library");
    assert_eq!(old_column(&lib.paths, "vods", "source_path", vod.id), Some(path.clone()));
    assert_eq!(old_column(&lib.paths, "vods", "received_ext", vod.id), None, "it isn't the library's copy");
    assert_eq!(lib.db.playable_path(vod.id).unwrap(), Some(path));
}

/// Files being received wait in `incoming` under part names, so a name one
/// comes with ("1.mp4") is never the one a VOD's file is kept under
/// (lib.rs, `received_incoming`: "so a name one comes with (\"1.mp4\") is
/// never the one a VOD's file is kept under"; `received`: "as
/// `<name>.part`, till each is added").
#[test]
fn files_being_received_wait_in_incoming_under_part_names() {
    let p = split();
    let waiting = p.received_incoming();
    assert_eq!(waiting.parent().unwrap(), p.received(), "incoming is a folder of its own in received");
    assert_ne!(waiting.join("1.mp4"), p.source_received(1, "mp4"), "the name it comes with isn't its place");
    assert_ne!(waiting.join("1.mp4.part"), p.source_received(1, "mp4"));
    assert_ne!(waiting.join("1.mp4").parent(), p.source_received(1, "mp4").parent());
    assert!(p.source_received(1, "mp4").starts_with(p.received()));
}

/// The kept videos are counted apart from the cache: `media_size` is what
/// the folder of kept videos holds, nothing more (lib.rs, `media_size`:
/// "Bytes used by the kept videos (`media`)").
#[test]
fn the_kept_videos_are_counted_apart_from_the_cache() {
    let dir = TempDir::new("media-size");
    let paths = Paths::split(dir.join("roaming"), dir.join("local"));
    paths.ensure().unwrap();
    std::fs::create_dir_all(paths.received()).unwrap();
    std::fs::write(paths.source_video(1, "mp4"), b"12345").unwrap();
    std::fs::write(paths.source_received(2, "mkv"), b"123").unwrap();
    std::fs::write(paths.work().join("1.m4a"), b"1234567").unwrap();
    assert_eq!(paths.media_size(), 8);
    assert_eq!(paths.cache_size(), 7);
}

/// Clearing downloads leaves a VOD's pictures: "they belong with the
/// library, so Clear downloads (`cache_dirs`) leaves them" (lib.rs,
/// `thumbs`).
#[test]
fn clearing_downloads_keeps_the_vods_pictures() {
    let dir = TempDir::new("keeps-pictures");
    let paths = Paths::split(dir.join("roaming"), dir.join("local"));
    paths.ensure().unwrap();
    let picture = paths.source_thumb("12.jpg");
    std::fs::write(&picture, b"picture").unwrap();
    assert!(!paths.cache_dirs().iter().any(|d| picture.starts_with(d)));
    paths.clear_cache().unwrap();
    assert_eq!(std::fs::read(&picture).unwrap(), b"picture");
}

/// Loudness and storyboards aren't cache: loudness is "read from audio that
/// may be gone since" and a storyboard is "made again if gone, but slowly,
/// so not a cache either" (lib.rs, `peaks` and `storyboards`).
#[test]
fn loudness_and_storyboards_are_not_cache() {
    let dir = TempDir::new("not-cache");
    let paths = Paths::split(dir.join("roaming"), dir.join("local"));
    paths.ensure().unwrap();
    for folder in [paths.peaks(), paths.storyboards()] {
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("12"), b"123").unwrap();
    }
    assert_eq!(paths.cache_size(), 0, "they aren't counted as cache");
    paths.clear_cache().unwrap();
    assert_eq!(std::fs::read(paths.peaks().join("12")).unwrap(), b"123");
    assert_eq!(std::fs::read(paths.storyboards().join("12")).unwrap(), b"123");
}

/// After a clear, the folders the app always needs are there, empty:
/// "The ones the app always needs are recreated empty" (lib.rs,
/// `clear_cache`).
#[test]
fn the_folders_the_app_always_needs_are_there_again_after_a_clear() {
    let dir = TempDir::new("recreated");
    let paths = Paths::split(dir.join("roaming"), dir.join("local"));
    paths.ensure().unwrap();
    for folder in [paths.bin(), paths.models(), paths.work()] {
        std::fs::write(folder.join("file"), b"x").unwrap();
    }
    paths.clear_cache().unwrap();
    for folder in [paths.root.clone(), paths.local.clone(), paths.bin(), paths.models(), paths.work(), paths.thumbs()] {
        assert!(folder.is_dir(), "{folder:?} is there");
    }
    for folder in [paths.bin(), paths.models(), paths.work()] {
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0, "{folder:?} is empty");
    }

    // A clear where nothing was downloaded yet (a folder not there) is no failure.
    let fresh = Paths::split(dir.join("fresh-roaming"), dir.join("fresh-local"));
    fresh.clear_cache().unwrap();
    assert!(fresh.bin().is_dir() && fresh.models().is_dir() && fresh.work().is_dir());
}

/// A library set aside is laid out as a library, in a folder of its name
/// beside each of the two folders (lib.rs, `aside`: "a folder of that name
/// in each of its folders, laid out as a library").
#[test]
fn a_library_set_aside_is_laid_out_as_a_library_in_a_folder_of_its_name() {
    let p = split();
    let aside = p.aside("library-broken-2026-11-11");
    assert_eq!(aside.root, Path::new("/lib/roaming/library-broken-2026-11-11"));
    assert_eq!(aside.local, Path::new("/lib/local/library-broken-2026-11-11"));
    assert_eq!(aside.db(), Path::new("/lib/roaming/library-broken-2026-11-11/library.sqlite"));
    assert_eq!(aside.clip_file(7, "mp4"), Path::new("/lib/local/library-broken-2026-11-11/clips/7.mp4"));
    for (mine, theirs) in p.library_files().iter().zip(aside.library_files()) {
        let name = mine.strip_prefix(&p.local).unwrap();
        assert_eq!(theirs, aside.local.join(name), "{name:?} is laid out as in a library");
    }
}

/// A new library, whose ids start again at 1, starts without the old one's
/// folders (lib.rs, `library_files`: "a new library, whose ids start again
/// at 1, starts without them"; CHANGELOG.md, 0.7.3 Fixes: "Starting over
/// keeps your videos.").
#[test]
fn a_new_library_starts_without_the_old_ones_folders() {
    let dir = TempDir::new("starts-over");
    let paths = Paths::split(dir.join("roaming"), dir.join("local"));
    paths.ensure().unwrap();
    std::fs::write(paths.db(), b"not a database").unwrap();
    let kept = paths.library_files();
    for folder in &kept {
        if *folder == paths.leftovers() {
            std::fs::write(folder, b"[]").unwrap();
        } else {
            std::fs::create_dir_all(folder).unwrap();
            std::fs::write(folder.join("1.file"), b"old").unwrap();
        }
    }
    std::fs::create_dir_all(paths.emotes()).unwrap();
    std::fs::write(paths.emotes().join("1.png"), b"emote").unwrap();

    let aside = crate::db::set_aside(&paths).unwrap();
    let old = paths.aside(&aside.file_name().unwrap().to_string_lossy());
    for folder in kept.iter().filter(|f| **f != paths.work()) {
        assert!(!folder.exists(), "{folder:?} isn't in the new library");
    }
    assert!(paths.emotes().join("1.png").exists(), "emote images, which no row names, stay");
    assert!(!paths.work().join("1.file").exists(), "its downloads under way go too");
    assert_eq!(std::fs::read(old.media().join("1.file")).unwrap(), b"old");
    assert_eq!(std::fs::read(old.db()).unwrap(), b"not a database");
}

/// The library is private to this user: other accounts can't list or read
/// it (lib.rs, `ensure`: "The library is private to this user", 0700).
#[cfg(unix)]
#[test]
fn the_library_is_private_to_this_user() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new("private");
    let paths = Paths::split(dir.join("roaming"), dir.join("local"));
    paths.ensure().unwrap();
    for folder in [&paths.root, &paths.local] {
        assert_eq!(std::fs::metadata(folder).unwrap().permissions().mode() & 0o777, 0o700, "{folder:?}");
    }
}

/// A YouTube chat downloads under a name a transcription's end doesn't
/// clear: it isn't `<id>.…`, and the chat may still be downloading when
/// the transcription ends (lib.rs, `temp_chat`: "Not named `<id>.…`:
/// `pipeline::remove_work_files` clears those when a transcription ends,
/// which may be while the chat is still downloading").
#[test]
fn a_youtube_chat_downloads_under_a_name_a_transcriptions_end_does_not_clear() {
    let dir = TempDir::new("chat-name");
    let paths = Paths::new(dir.join("data"));
    paths.ensure().unwrap();
    let chat = paths.temp_chat(5).with_extension("live_chat.json.part");
    assert_eq!(chat.parent().unwrap(), paths.work());
    let ends = [paths.source_audio(5, "m4a"), paths.source_info(5), paths.source_section(5, 2, "m4a")];
    for file in ends.iter().chain([&chat]) {
        std::fs::write(file, b"x").unwrap();
    }
    crate::pipeline::remove_work_files(&paths.work(), 5);
    for file in &ends {
        assert!(!file.exists(), "{file:?} goes at the transcription's end");
    }
    assert!(chat.exists(), "the chat is still downloading");
}
