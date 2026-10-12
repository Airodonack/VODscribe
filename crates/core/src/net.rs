use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

/// A download that receives nothing for this long has stalled.
const STALL_TIMEOUT: Duration = Duration::from_secs(60);
/// How often a waiting download checks for cancellation.
const CANCEL_POLL: Duration = Duration::from_millis(250);

/// The shared HTTP client. Connecting and receiving headers time out; the
/// body has a generous overall limit, since stalls are caught by `get`.
pub fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(30)))
            .timeout_recv_response(Some(Duration::from_secs(60)))
            .timeout_recv_body(Some(Duration::from_secs(6 * 60 * 60)))
            .build()
            .into()
    })
}

/// The error a canceled read returns. Not `ErrorKind::Interrupted`, which
/// `io::copy` and other readers retry rather than stop on.
fn canceled() -> io::Error {
    io::Error::other("canceled")
}

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
            return Err(canceled());
        }
        let n = self.inner.read(buf)?;
        self.read += n as u64;
        (self.on_progress)(self.read);
        Ok(n)
    }
}

/// Opens a streaming GET request. Returns the body reader and its length, if
/// known. Reads fail when `cancel` is set (within `CANCEL_POLL`) or when no
/// data arrives for `STALL_TIMEOUT`, rather than blocking on a dead connection.
pub fn get<'a>(url: &str, cancel: &'a AtomicBool) -> Result<(impl Read + 'a, Option<u64>)> {
    let resp = agent().get(url).call().with_context(|| format!("GET {url}"))?;
    let len = resp.body().content_length();
    let mut body = resp.into_body().into_reader();

    // ureq's reads block until data arrives, so read on a separate thread. If
    // the download is abandoned, the thread ends at its next read or when the
    // agent's overall body timeout expires.
    let (tx, rx) = mpsc::sync_channel::<io::Result<Vec<u8>>>(8);
    std::thread::spawn(move || {
        loop {
            let mut buf = vec![0u8; 1 << 16];
            let msg = match body.read(&mut buf) {
                Ok(n) => {
                    buf.truncate(n);
                    Ok(buf)
                }
                Err(e) => Err(e),
            };
            let done = !matches!(&msg, Ok(b) if !b.is_empty());
            if tx.send(msg).is_err() || done {
                return;
            }
        }
    });
    Ok((GuardedReader { rx, pending: Vec::new(), offset: 0, eof: false, cancel }, len))
}

struct GuardedReader<'a> {
    rx: Receiver<io::Result<Vec<u8>>>,
    pending: Vec<u8>,
    offset: usize,
    /// The reader thread exits after end of body; later reads must keep
    /// reporting end of body, not a closed channel.
    eof: bool,
    cancel: &'a AtomicBool,
}

impl Read for GuardedReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.offset == self.pending.len() {
            if self.eof {
                return Ok(0);
            }
            let waiting_since = Instant::now();
            loop {
                if self.cancel.load(Ordering::Relaxed) {
                    return Err(canceled());
                }
                match self.rx.recv_timeout(CANCEL_POLL) {
                    Ok(Ok(chunk)) if chunk.is_empty() => {
                        self.eof = true;
                        return Ok(0);
                    }
                    Ok(Ok(chunk)) => {
                        self.pending = chunk;
                        self.offset = 0;
                        break;
                    }
                    Ok(Err(e)) => return Err(e),
                    Err(RecvTimeoutError::Timeout) if waiting_since.elapsed() < STALL_TIMEOUT => {}
                    Err(RecvTimeoutError::Timeout) => {
                        return Err(io::Error::new(io::ErrorKind::TimedOut, "the download stalled"));
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "the connection closed"));
                    }
                }
            }
        }
        let n = buf.len().min(self.pending.len() - self.offset);
        buf[..n].copy_from_slice(&self.pending[self.offset..self.offset + n]);
        self.offset += n;
        Ok(n)
    }
}

/// Downloads `url` to `dest` via a `.part` file, so an interrupted download never
/// leaves a truncated file at `dest`. `on_progress` receives (bytes, total).
pub fn download(
    url: &str,
    dest: &Path,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(u64, Option<u64>),
) -> Result<()> {
    let (body, total) = get(url, cancel)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use std::net::TcpListener;
    use std::sync::Arc;

    /// Serves one response: `body`, then stays connected without sending more
    /// if `stall` is set (claiming more bytes than it sends).
    fn serve(body: &'static [u8], stall: bool) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            let mut req = [0u8; 1024];
            let _ = conn.read(&mut req);
            let len = if stall { body.len() + 1000 } else { body.len() };
            write!(conn, "HTTP/1.1 200 OK\r\nContent-Length: {len}\r\n\r\n").unwrap();
            conn.write_all(body).unwrap();
            conn.flush().unwrap();
            if stall {
                std::thread::sleep(Duration::from_secs(30));
            }
        });
        format!("http://{addr}/file")
    }

    #[test]
    fn reads_a_whole_body() {
        let never = AtomicBool::new(false);
        let (mut body, len) = get(&serve(b"hello world", false), &never).unwrap();
        let mut out = Vec::new();
        body.read_to_end(&mut out).unwrap();
        assert_eq!((out.as_slice(), len), (&b"hello world"[..], Some(11)));
        assert_eq!(body.read(&mut [0u8; 8]).unwrap(), 0, "end of body repeats");
    }

    #[test]
    fn cancel_ends_a_stalled_download_promptly() {
        let cancel = Arc::new(AtomicBool::new(false));
        let (body, _) = get(&serve(b"partial", true), &cancel).unwrap();
        let flag = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            flag.store(true, Ordering::Relaxed);
        });
        let started = Instant::now();
        // io::copy retries ErrorKind::Interrupted, so this also checks that
        // cancellation isn't reported that way.
        let mut reader = ProgressReader::new(body, &cancel, |_| {});
        let err = io::copy(&mut reader, &mut io::sink()).unwrap_err();
        assert_eq!(err.to_string(), "canceled");
        assert!(started.elapsed() < Duration::from_secs(2), "took {:?}", started.elapsed());
    }
}
