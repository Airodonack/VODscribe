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
    use std::sync::mpsc;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dragndrop-target-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn names_are_made_safe_everywhere() {
        assert_eq!(safe_file_name("a b.mp4"), "a b.mp4");
        assert_eq!(safe_file_name("a:b?.mkv"), "a_b_.mkv");
        assert_eq!(safe_file_name("..\\..\\x.mp4"), ".._.._x.mp4");
        assert_eq!(safe_file_name("tab\there"), "tab_here");
        assert_eq!(safe_file_name("ends. . "), "ends");
        assert_eq!(safe_file_name(".."), "file");
        assert_eq!(safe_file_name(""), "file");
        assert_eq!(safe_file_name("con"), "_con");
        assert_eq!(safe_file_name("Com1.txt"), "_Com1.txt");
        assert_eq!(safe_file_name("lpt9 .mp4"), "_lpt9 .mp4");
        assert_eq!(safe_file_name("console.mp4"), "console.mp4");
        assert_eq!(safe_file_name("com10.mp4"), "com10.mp4");
        assert_eq!(safe_file_name("été.mp4"), "été.mp4");
    }

    #[test]
    fn a_name_taken_finished_or_being_written_gets_a_number() {
        let dir = scratch("names");
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(free_path(&dir, "a.mp4"), dir.join("a.mp4"));
        std::fs::write(dir.join("a.mp4"), b"").unwrap();
        std::fs::write(dir.join("a (2).mp4.part"), b"").unwrap();
        assert_eq!(free_path(&dir, "a.mp4"), dir.join("a (3).mp4"));
        std::fs::write(dir.join("noext"), b"").unwrap();
        assert_eq!(free_path(&dir, "noext"), dir.join("noext (2)"));
        std::fs::write(dir.join(".hidden"), b"").unwrap();
        assert_eq!(free_path(&dir, ".hidden"), dir.join(".hidden (2)"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_whole_item_is_renamed_from_its_part_and_a_failed_one_leaves_nothing() {
        let dir = scratch("sink");
        let (tell, heard) = mpsc::channel();
        let told = tell.clone();
        let mut whole = Box::new(FolderSink::new(dir.clone(), "c:d.mp4", Box::new(move |r| told.send(r).unwrap())));
        let mut failed = Box::new(FolderSink::new(dir.clone(), "c:d.mp4", Box::new(move |r| tell.send(r).unwrap())));
        assert!(dir.join("c_d.mp4.part").exists() && dir.join("c_d (2).mp4.part").exists());
        whole.write(b"abc").unwrap();
        failed.write(b"x").unwrap();
        whole.finish(Ok(3));
        assert_eq!(heard.recv().unwrap().unwrap(), dir.join("c_d.mp4"));
        failed.finish(Err(Error::Canceled));
        assert!(matches!(heard.recv().unwrap(), Err(Error::Canceled)));
        let mut left: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        left.sort();
        assert_eq!(left, ["c_d.mp4"]);
        assert_eq!(std::fs::read(dir.join("c_d.mp4")).unwrap(), b"abc");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_file_adopted_whole_lands_under_the_items_name() {
        let dir = scratch("adopt");
        let (tell, heard) = mpsc::channel();
        let mut sink = Box::new(FolderSink::new(dir.clone(), "a.mp4", Box::new(move |r| tell.send(r).unwrap())));
        let from = dir.join("received elsewhere.mp4");
        std::fs::write(&from, b"whole").unwrap();
        assert_eq!(sink.adopt(&from).unwrap(), 5);
        assert!(!from.exists());
        sink.finish(Ok(5));
        assert_eq!(heard.recv().unwrap().unwrap(), dir.join("a.mp4"));
        let left: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(left, ["a.mp4"]);
        assert_eq!(std::fs::read(dir.join("a.mp4")).unwrap(), b"whole");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_folder_that_cant_be_made_says_why() {
        let file = scratch("not-a-dir");
        std::fs::write(&file, b"").unwrap();
        let (tell, heard) = mpsc::channel();
        let mut sink = Box::new(FolderSink::new(file.join("under"), "a.mp4", Box::new(move |r| tell.send(r).unwrap())));
        assert!(sink.write(b"x").is_err());
        sink.finish(Err(Error::Source("gone".into())));
        assert!(matches!(heard.recv().unwrap(), Err(Error::Source(_))));
        std::fs::remove_file(&file).unwrap();
    }
}
