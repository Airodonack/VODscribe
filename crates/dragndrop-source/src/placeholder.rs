//! The empty file a promised file stands in for at the receiver's place
//! while it's made, and the copy that takes its place.
//!
//! The wait can be minutes, and meanwhile the place is the person's: they
//! may save another file of that name there, or something may put a link
//! there. So the placeholder is known by its file (device and inode, kept
//! open so neither is reused), not by its name: only that file is ever
//! removed, and the made file goes to a new name only, never onto or
//! through whatever is there.

use std::fs::{File, OpenOptions};
use std::io::{self, ErrorKind};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// The empty file at a promised file's place.
pub(crate) struct Placeholder {
    to: PathBuf,
    /// Held open: while it is, its inode isn't given to another file.
    file: File,
}

impl Placeholder {
    /// Makes it, as a new file: one already there is refused (AlreadyExists).
    pub(crate) fn create(to: &Path) -> io::Result<Self> {
        let file = OpenOptions::new().write(true).create_new(true).open(to)?;
        Ok(Self { to: to.to_path_buf(), file })
    }

    /// Takes it away, if it's still what's there: the place left empty for
    /// the made file, or for nothing when it won't come. Anything else there
    /// (a file saved at that name since, a link) is left as it is and
    /// refused with AlreadyExists; nothing there at all is fine.
    pub(crate) fn remove(&self) -> io::Result<()> {
        let there = match std::fs::symlink_metadata(&self.to) {
            Ok(there) => there,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        let ours = self.file.metadata()?;
        if !there.file_type().is_file() || (there.dev(), there.ino()) != (ours.dev(), ours.ino()) {
            return Err(io::Error::new(ErrorKind::AlreadyExists, "a file of that name is already there"));
        }
        std::fs::remove_file(&self.to)
    }
}

/// Copies `from` to `to` as a new file, never onto or through one there
/// (AlreadyExists): on APFS a clone, at once; elsewhere written out, and
/// taken away again if that fails partway.
pub(crate) fn copy_new(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    match clone(from, to) {
        Ok(()) => return Ok(()),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => return Err(e),
        // Not APFS, or another volume: written out instead.
        Err(_) => {}
    }
    let mut source = File::open(from)?;
    let copy = Placeholder::create(to)?;
    if let Err(e) = io::copy(&mut source, &mut &copy.file) {
        let _ = copy.remove();
        return Err(e);
    }
    Ok(())
}

/// APFS's clone of `from` at `to`, which must be a new name: clonefile(2)
/// refuses one taken (EEXIST), a link included, and never follows it.
#[cfg(target_os = "macos")]
fn clone(from: &Path, to: &Path) -> io::Result<()> {
    use std::ffi::{CString, c_char, c_int};
    use std::os::unix::ffi::OsStrExt;
    unsafe extern "C" {
        fn clonefile(src: *const c_char, dst: *const c_char, flags: u32) -> c_int;
    }
    /// CLONE_NOFOLLOW: a link at `from` is cloned as the link.
    const NO_FOLLOW: u32 = 1;
    let from = CString::new(from.as_os_str().as_bytes())?;
    let to = CString::new(to.as_os_str().as_bytes())?;
    // SAFETY: two NUL-terminated paths that outlive the call.
    if unsafe { clonefile(from.as_ptr(), to.as_ptr(), NO_FOLLOW) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests;
