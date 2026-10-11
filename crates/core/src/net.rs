use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, bail};

/// Wraps a reader, reporting bytes read and aborting when `cancel` is set.
pub struct ProgressReader<'a, R, F> {
    inner: R,
    read: u64,
    on_progress: F,
    cancel: &'a AtomicBool,
}

impl<'a, R: Read, F: FnMut(u64)> ProgressReader<'a, R, F> {
    pub fn new(inner: R, cancel: &'a AtomicBool, on_progress: F) -> Self {
        Self { inner, read: 0, on_progress, cancel }
    }
}

impl<R: Read, F: FnMut(u64)> Read for ProgressReader<'_, R, F> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "canceled"));
        }
        let n = self.inner.read(buf)?;
        self.read += n as u64;
        (self.on_progress)(self.read);
        Ok(n)
    }
}

/// Opens a streaming GET request. Returns the body reader and its length, if known.
pub fn get(url: &str) -> Result<(impl Read + use<>, Option<u64>)> {
    let resp = ureq::get(url).call().with_context(|| format!("GET {url}"))?;
    let len = resp.body().content_length();
    Ok((resp.into_body().into_reader(), len))
}

/// Downloads `url` to `dest` via a `.part` file, so an interrupted download never
/// leaves a truncated file at `dest`. `on_progress` receives (bytes, total).
pub fn download(
    url: &str,
    dest: &Path,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(u64, Option<u64>),
) -> Result<()> {
    let (body, total) = get(url)?;
    let part = dest.with_extension("part");
    let mut reader = ProgressReader::new(body, cancel, |n| on_progress(n, total));
    let mut out = File::create(&part).with_context(|| format!("create {}", part.display()))?;
    if let Err(e) = io::copy(&mut reader, &mut out) {
        drop(out);
        let _ = fs::remove_file(&part);
        if cancel.load(Ordering::Relaxed) {
            bail!("canceled");
        }
        return Err(e).with_context(|| format!("download {url}"));
    }
    out.flush()?;
    drop(out);
    fs::rename(&part, dest)?;
    Ok(())
}
