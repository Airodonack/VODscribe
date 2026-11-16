//! A folder destination: the item written as "<name>.part" in the folder,
//! renamed once whole, removed when it isn't.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::{Error, Sink};

/// `name` as a file name that's valid on Windows, macOS and Linux: no folder
/// separators, nothing Windows refuses (`<>:"|?*`, control characters, a
/// device name such as "CON", a trailing dot or space), never "." or "..".
/// What's refused becomes "_"; an empty name becomes "file".
pub fn safe_file_name(name: &str) -> String {
    let mut safe: String =
        name.chars().map(|c| if c.is_control() || r#"<>:"/\|?*"#.contains(c) { '_' } else { c }).collect();
    safe.truncate(safe.trim_end_matches(['.', ' ']).len());
    if safe.is_empty() {
        return "file".into();
    }
    let stem = safe.split('.').next().unwrap_or_default().trim_end_matches(' ');
    let device = ["CON", "PRN", "AUX", "NUL"].iter().any(|d| stem.eq_ignore_ascii_case(d))
        || ["COM", "LPT"].iter().any(|d| {
            stem.len() == 4
                && stem.is_char_boundary(3)
                && stem[..3].eq_ignore_ascii_case(d)
                && matches!(stem.as_bytes()[3], b'1'..=b'9')
        });
    if device { format!("_{safe}") } else { safe }
}

/// `name` in `dir`, or the first of "<stem> (2).<ext>", "(3)"... that isn't
/// there, finished or being written.
pub(crate) fn free_path(dir: &Path, name: &str) -> PathBuf {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, Some(ext)),
        _ => (name, None),
    };
    let taken = |p: &Path| p.exists() || part_of(p).exists();
    let mut at = dir.join(name);
    let mut n = 2;
    while taken(&at) {
        at = dir.join(match ext {
            Some(ext) => format!("{stem} ({n}).{ext}"),
            None => format!("{stem} ({n})"),
        });
        n += 1;
    }
    at
}

/// Where file `path` is written until it's whole.
fn part_of(path: &Path) -> PathBuf {
    let mut part = path.as_os_str().to_owned();
    part.push(".part");
    part.into()
}

type Done = Box<dyn FnOnce(Result<PathBuf, Error>) + Send>;

/// An item being written into a folder.
pub(crate) struct FolderSink {
    /// Its name once whole, and its part file open, or why it couldn't be.
    file: Result<(PathBuf, BufWriter<File>), Option<io::Error>>,
    done: Done,
}

impl FolderSink {
    /// Item `name` into `dir`: its part file is made now, so another item of
    /// the same name gets another number.
    pub(crate) fn new(dir: PathBuf, name: &str, done: Done) -> Self {
        let file = std::fs::create_dir_all(&dir)
            .and_then(|()| {
                let to = free_path(&dir, &safe_file_name(name));
                let file = File::create_new(part_of(&to))?;
                Ok((to, BufWriter::with_capacity(1 << 20, file)))
            })
            .map_err(Some);
        FolderSink { file, done }
    }

    /// Takes file `from`, whole, as the item: moved over its part file
    /// (nothing copied, so only on the same volume). Its length, or why it
    /// couldn't be moved, leaving the part file as it was to be written.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn adopt(&mut self, from: &Path) -> io::Result<u64> {
        let Ok((to, _)) = &self.file else { return Err(io::ErrorKind::BrokenPipe.into()) };
        let len = std::fs::metadata(from)?.len();
        // (The part file still open here is then a name-less file, closed at the finish.)
        std::fs::rename(from, part_of(to))?;
        Ok(len)
    }
}

impl Sink for FolderSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        match &mut self.file {
            Ok((_, file)) => file.write_all(bytes),
            Err(e) => Err(e.take().unwrap_or_else(|| io::ErrorKind::BrokenPipe.into())),
        }
    }

    fn finish(self: Box<Self>, result: Result<u64, Error>) {
        let FolderSink { file, done } = *self;
        let (to, file) = match file {
            Ok(open) => open,
            Err(e) => {
                let why = match result {
                    Err(e) => e,
                    Ok(_) => Error::Write(e.unwrap_or_else(|| io::ErrorKind::BrokenPipe.into())),
                };
                return done(Err(why));
            }
        };
        let part = part_of(&to);
        let whole = result.and_then(|_| {
            let file = file.into_inner().map_err(|e| Error::Write(e.into_error()))?;
            file.sync_all().map_err(Error::Write)?;
            drop(file);
            std::fs::rename(&part, &to).map_err(Error::Write)
        });
        match whole {
            Ok(()) => done(Ok(to)),
            Err(why) => {
                // (Closed before it goes, by now: Windows keeps an open file.)
                let _ = std::fs::remove_file(&part);
                done(Err(why))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{fresh, names};
    use std::sync::mpsc;

    /// A sink for item `name` into `dir`, and where its end is heard.
    fn sink(dir: &Path, name: &str) -> (Box<FolderSink>, mpsc::Receiver<Result<PathBuf, Error>>) {
        let (tell, heard) = mpsc::channel();
        (Box::new(FolderSink::new(dir.to_path_buf(), name, Box::new(move |r| tell.send(r).unwrap()))), heard)
    }

    /// `safe_file_name`: "no folder separators ... What's refused becomes "_"".
    #[test]
    fn a_name_loses_its_folder_separators() {
        assert_eq!(safe_file_name("..\\..\\x.mp4"), ".._.._x.mp4");
        assert_eq!(safe_file_name("a/b.mp4"), "a_b.mp4");
    }

    /// `safe_file_name`: "nothing Windows refuses (`<>:"|?*`, control characters ...)".
    #[test]
    fn a_name_loses_what_windows_refuses() {
        assert_eq!(safe_file_name("a:b?.mkv"), "a_b_.mkv");
        assert_eq!(safe_file_name("<>\"|*"), "_____");
        assert_eq!(safe_file_name("tab\there"), "tab_here");
    }

    /// `safe_file_name`: "a trailing dot or space" is refused; "never "." or ".."; an empty name
    /// becomes "file"".
    #[test]
    fn a_name_has_no_trailing_dot_or_space_and_is_never_empty_or_dots() {
        assert_eq!(safe_file_name("ends. . "), "ends");
        for name in ["", ".", "..", " ", "..."] {
            assert_eq!(safe_file_name(name), "file", "{name:?}");
        }
    }

    /// `safe_file_name`: "a device name such as "CON"", with any extension or case.
    #[test]
    fn a_windows_device_name_is_given_a_leading_underscore() {
        for (name, safe) in [
            ("con", "_con"),
            ("NUL.mp4", "_NUL.mp4"),
            ("aux.tar.gz", "_aux.tar.gz"),
            ("Com1.txt", "_Com1.txt"),
            ("lpt9 .mp4", "_lpt9 .mp4"),
            ("PRN", "_PRN"),
        ] {
            assert_eq!(safe_file_name(name), safe);
        }
    }

    /// `safe_file_name`: only the device names are refused, not names that begin like one.
    #[test]
    fn a_name_that_only_begins_like_a_device_name_is_kept() {
        for name in ["console.mp4", "com10.mp4", "com0.mp4", "lpt.mp4", "nullable", "a b.mp4", "été.mp4"] {
            assert_eq!(safe_file_name(name), name);
        }
    }

    /// `Destination::folder`: "numbered if taken ("name (2).ext")".
    #[test]
    fn a_name_taken_in_the_folder_gets_the_next_number() {
        let dir = fresh("free-taken");
        assert_eq!(free_path(&dir, "a.mp4"), dir.join("a.mp4"));
        std::fs::write(dir.join("a.mp4"), b"").unwrap();
        assert_eq!(free_path(&dir, "a.mp4"), dir.join("a (2).mp4"));
        std::fs::write(dir.join("a (2).mp4"), b"").unwrap();
        assert_eq!(free_path(&dir, "a.mp4"), dir.join("a (3).mp4"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// `free_path`: "the first of "<stem> (2).<ext>", "(3)"... that isn't there, finished or being
    /// written."
    #[test]
    fn a_name_being_written_counts_as_taken() {
        let dir = fresh("free-part");
        std::fs::write(dir.join("a.mp4.part"), b"").unwrap();
        assert_eq!(free_path(&dir, "a.mp4"), dir.join("a (2).mp4"));
        std::fs::write(dir.join("a (2).mp4.part"), b"").unwrap();
        assert_eq!(free_path(&dir, "a.mp4"), dir.join("a (3).mp4"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// `free_path`: the number goes before the last extension, and a name with none or a leading dot
    /// only is numbered whole.
    #[test]
    fn the_number_goes_before_the_last_extension() {
        let dir = fresh("free-ext");
        for name in ["noext", ".hidden", "a.tar.gz"] {
            std::fs::write(dir.join(name), b"").unwrap();
        }
        assert_eq!(free_path(&dir, "noext"), dir.join("noext (2)"));
        assert_eq!(free_path(&dir, ".hidden"), dir.join(".hidden (2)"));
        assert_eq!(free_path(&dir, "a.tar.gz"), dir.join("a.tar (2).gz"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// `FolderSink::new`: "its part file is made now, so another item of the same name gets another
    /// number."
    #[test]
    fn two_items_of_one_name_are_written_under_two_names_at_once() {
        let dir = fresh("sink-two");
        let (_first, _) = sink(&dir, "c:d.mp4");
        let (_second, _) = sink(&dir, "c:d.mp4");
        assert_eq!(names(&dir), ["c_d (2).mp4.part", "c_d.mp4.part"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// `Destination::folder`: "It's written as "<name>.part" and renamed once whole".
    #[test]
    fn a_whole_item_is_renamed_from_its_part_file() {
        let dir = fresh("sink-whole");
        let (mut whole, heard) = sink(&dir, "c:d.mp4");
        whole.write(b"ab").unwrap();
        whole.write(b"c").unwrap();
        assert_eq!(names(&dir), ["c_d.mp4.part"]);
        whole.finish(Ok(3));
        assert_eq!(heard.recv().unwrap().unwrap(), dir.join("c_d.mp4"));
        assert_eq!(names(&dir), ["c_d.mp4"]);
        assert_eq!(std::fs::read(dir.join("c_d.mp4")).unwrap(), b"abc");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// `FolderSink`: "removed when it isn't", and `Destination::folder`: "nothing is left when it
    /// isn't."
    #[test]
    fn an_item_that_fails_or_is_canceled_leaves_nothing_and_says_why() {
        let dir = fresh("sink-failed");
        let (mut failed, heard) = sink(&dir, "a.mp4");
        failed.write(b"x").unwrap();
        failed.finish(Err(Error::Canceled));
        assert!(matches!(heard.recv().unwrap(), Err(Error::Canceled)));
        let (mut gone, heard) = sink(&dir, "b.mp4");
        gone.write(b"x").unwrap();
        gone.finish(Err(Error::Source("gone".into())));
        assert!(matches!(heard.recv().unwrap(), Err(Error::Source(_))));
        assert_eq!(names(&dir), Vec::<String>::new());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// `Destination::folder`: "Into folder `dir` (made if needed)".
    #[test]
    fn a_folder_that_isnt_there_is_made() {
        let dir = fresh("sink-made");
        let into = dir.join("a").join("b");
        let (mut sink, heard) = sink(&into, "a.mp4");
        sink.write(b"x").unwrap();
        sink.finish(Ok(1));
        assert_eq!(heard.recv().unwrap().unwrap(), into.join("a.mp4"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// `Destination::folder`: "`done` hears the file's path, or why it isn't there": a folder that
    /// can't be made refuses the bytes, and the item ends with the source's reason if it has one.
    #[test]
    fn a_folder_that_cant_be_made_refuses_the_bytes_and_ends_with_why() {
        let dir = fresh("sink-blocked");
        let file = dir.join("not-a-dir");
        std::fs::write(&file, b"").unwrap();
        let (mut sink, heard) = sink(&file.join("under"), "a.mp4");
        assert!(sink.write(b"x").is_err());
        sink.finish(Err(Error::Source("gone".into())));
        assert!(matches!(heard.recv().unwrap(), Err(Error::Source(_))));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// As above, an item its source says whole into a folder that can't be made still fails.
    #[test]
    fn a_whole_item_into_a_folder_that_cant_be_made_fails_with_a_write_error() {
        let dir = fresh("sink-blocked-whole");
        let file = dir.join("not-a-dir");
        std::fs::write(&file, b"").unwrap();
        let (sink, heard) = sink(&file.join("under"), "a.mp4");
        sink.finish(Ok(0));
        assert!(matches!(heard.recv().unwrap(), Err(Error::Write(_))));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// `FolderSink::adopt`: "Takes file `from`, whole, as the item: moved over its part file ... Its
    /// length".
    #[test]
    fn a_file_adopted_whole_lands_under_the_items_name() {
        let dir = fresh("adopt");
        let (mut sink, heard) = sink(&dir, "a.mp4");
        let from = dir.join("received elsewhere.mp4");
        std::fs::write(&from, b"whole").unwrap();
        assert_eq!(sink.adopt(&from).unwrap(), 5);
        assert!(!from.exists());
        sink.finish(Ok(5));
        assert_eq!(heard.recv().unwrap().unwrap(), dir.join("a.mp4"));
        assert_eq!(names(&dir), ["a.mp4"]);
        assert_eq!(std::fs::read(dir.join("a.mp4")).unwrap(), b"whole");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// `FolderSink::adopt`: "or why it couldn't be moved, leaving the part file as it was to be
    /// written."
    #[test]
    fn a_file_that_cant_be_adopted_leaves_the_item_to_be_written() {
        let dir = fresh("adopt-missing");
        let (mut sink, heard) = sink(&dir, "a.mp4");
        assert!(sink.adopt(&dir.join("not there.mp4")).is_err());
        sink.write(b"abc").unwrap();
        sink.finish(Ok(3));
        assert_eq!(heard.recv().unwrap().unwrap(), dir.join("a.mp4"));
        assert_eq!(std::fs::read(dir.join("a.mp4")).unwrap(), b"abc");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// `FolderSink::adopt` into a folder that couldn't be made has nowhere to put the file, and
    /// leaves it where it is.
    #[test]
    fn a_file_adopted_into_a_folder_that_cant_be_made_stays_where_it_is() {
        let dir = fresh("adopt-blocked");
        let blocked = dir.join("not-a-dir");
        std::fs::write(&blocked, b"").unwrap();
        let (mut sink, _) = sink(&blocked.join("under"), "a.mp4");
        let from = dir.join("b.mp4");
        std::fs::write(&from, b"whole").unwrap();
        assert!(sink.adopt(&from).is_err());
        assert!(from.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
