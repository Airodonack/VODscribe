use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

/// A download that receives nothing for this long has stalled.
const STALL_TIMEOUT: Duration = Duration::from_secs(60);
/// How often a waiting download checks for cancellation.
const CANCEL_POLL: Duration = Duration::from_millis(250);

/// Whether `e` is the server saying the resource doesn't exist or isn't
/// available (a 4xx status), as opposed to a failure worth retrying.
pub fn is_client_error(e: &anyhow::Error) -> bool {
    e.chain().any(|c| matches!(c.downcast_ref::<ureq::Error>(), Some(ureq::Error::StatusCode(400..=499))))
}

/// `url` without its query string, which can hold signed tokens, for error messages.
fn redact(url: &str) -> &str {
    url.split_once('?').map_or(url, |(base, _)| base)
}

/// The shared HTTP client. Connecting and receiving headers time out; the
/// body has a generous overall limit, since stalls are caught by `get`.
pub fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(30)))
            .timeout_recv_response(Some(Duration::from_secs(60)))
            .timeout_recv_body(Some(Duration::from_secs(6 * 60 * 60)))
            // Tests serve plain HTTP from localhost.
            .https_only(!cfg!(test))
            .build()
            .into()
    })
}

/// The error a canceled read returns. Not `ErrorKind::Interrupted`, which
/// `io::copy` and other readers retry rather than stop on.
fn canceled() -> io::Error {
    io::Error::other(crate::Canceled)
}

/// Wraps a reader, reporting bytes read and aborting when `cancel` is set.
pub(crate) struct ProgressReader<'a, R, F> {
    inner: R,
    read: u64,
    on_progress: F,
    cancel: &'a AtomicBool,
}

impl<'a, R: Read, F: FnMut(u64)> ProgressReader<'a, R, F> {
    pub(crate) fn new(inner: R, cancel: &'a AtomicBool, on_progress: F) -> Self {
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

/// Fails unless `url` is an http(s) link to a host on the public internet:
/// not this computer, the local network, or an address that resolves to them.
/// Links pasted in, and the image links sites give (thumbnails), are checked
/// before they're fetched, so a link can't make VODscribe (or yt-dlp, for it)
/// reach a router's page or the app's own media server. A redirect isn't
/// checked again.
pub fn check_public_url(url: &str) -> Result<()> {
    let (scheme, rest) = url.split_once("://").context("That isn't a link.")?;
    if !scheme.eq_ignore_ascii_case("https") && !scheme.eq_ignore_ascii_case("http") {
        bail!("VODscribe only follows web links (https://…).");
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = match host_port.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or_default(),
        None => host_port.split(':').next().unwrap_or_default(),
    }
    .trim_end_matches('.')
    .to_ascii_lowercase();
    let local = || anyhow::anyhow!("VODscribe doesn't follow links to this computer or the local network.");
    if host.is_empty() {
        bail!("That isn't a link.");
    }
    if host == "localhost"
        || [".localhost", ".local", ".internal", ".lan", ".home.arpa"].iter().any(|s| host.ends_with(s))
    {
        return Err(local());
    }
    let addresses: Vec<std::net::IpAddr> = match host.parse() {
        Ok(ip) => vec![ip],
        // A name: where it resolves now. (It could resolve elsewhere by the
        // time it's fetched; this keeps out the plain cases.)
        Err(_) => match std::net::ToSocketAddrs::to_socket_addrs(&(host.as_str(), 443)) {
            Ok(found) => found.map(|a| a.ip()).collect(),
            // Not resolving is the fetch's to report.
            Err(_) => return Ok(()),
        },
    };
    if addresses.iter().any(|ip| !is_public(*ip)) {
        return Err(local());
    }
    Ok(())
}

fn is_public(ip: std::net::IpAddr) -> bool {
    use std::net::IpAddr;
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
                || a == 0
                // Carrier-grade NAT, 100.64.0.0/10, and benchmarking, 198.18.0.0/15.
                || (a == 100 && (64..128).contains(&b))
                || (a == 198 && (b == 18 || b == 19)))
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            !(v6.is_loopback()
                || v6.is_unspecified()
                // Unique local (fc00::/7) and link-local (fe80::/10).
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80)
        }
    }
}

/// Opens a streaming GET request. Returns the body reader and its length, if
/// known. Reads fail when `cancel` is set (within `CANCEL_POLL`) or when no
/// data arrives for `STALL_TIMEOUT`, rather than blocking on a dead connection.
pub fn get<'a>(url: &str, cancel: &'a AtomicBool) -> Result<(impl Read + 'a, Option<u64>)> {
    let resp = agent().get(url).call().with_context(|| format!("GET {}", redact(url)))?;
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

/// The body at `url`, which must be at most `max` bytes (for small files such
/// as playlists and checksum lists, read into memory).
pub fn get_bytes(url: &str, max: u64, cancel: &AtomicBool) -> Result<Vec<u8>> {
    let (body, _) = get(url, cancel)?;
    let mut bytes = Vec::new();
    body.take(max + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        bail!("{} is larger than expected", redact(url));
    }
    Ok(bytes)
}

/// Lowercase hex, as SHA-256 sums are written.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Downloads `url` to `dest`, checking it against a pinned SHA-256 (and size,
/// when known). Nothing appears at `dest` unless it matches. `on_progress`
/// receives bytes so far.
pub fn download_verified(
    url: &str,
    dest: &Path,
    size: Option<u64>,
    sha256: &str,
    cancel: &AtomicBool,
    on_progress: impl FnMut(u64),
) -> Result<()> {
    let part = dest.with_extension("part");
    let result = (|| -> Result<()> {
        let (body, _) = get(url, cancel)?;
        let mut reader = ProgressReader::new(body, cancel, on_progress);
        let mut out = File::create(&part)?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 1 << 16];
        let mut len = 0u64;
        loop {
            let n = match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.into()),
            };
            hasher.update(&buf[..n]);
            out.write_all(&buf[..n])?;
            len += n as u64;
            if size.is_some_and(|size| len > size) {
                break;
            }
        }
        out.flush()?;
        let hash = hex(&hasher.finalize());
        if size.is_some_and(|size| len != size) || hash != sha256 {
            eprintln!("{}: got {len} bytes, sha256 {hash}", redact(url));
            bail!("A download was damaged or changed on the way, so VODscribe threw it away. Try again.");
        }
        Ok(())
    })();
    match result {
        Ok(()) => Ok(fs::rename(&part, dest)?),
        Err(e) => {
            let _ = fs::remove_file(&part);
            if cancel.load(Ordering::Relaxed) {
                return Err(crate::Canceled.into());
            }
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_public_links_are_followed() {
        for ok in ["https://www.twitch.tv/videos/1", "http://93.184.215.14/x", "https://[2606:4700::1111]/"] {
            assert!(check_public_url(ok).is_ok(), "{ok}");
        }
        for bad in [
            "https://localhost/x",
            "http://127.0.0.1:1234/media/1",
            "http://192.168.1.1/",
            "https://10.0.0.5/",
            "https://172.20.1.1/",
            "http://169.254.169.254/latest/meta-data",
            "http://[::1]:8080/",
            "http://[fd00::1]/",
            "http://[::ffff:192.168.0.1]/",
            "https://user@printer.local/",
            "http://100.100.1.1/",
            "http://0.0.0.0/",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "https:///nohost",
        ] {
            assert!(check_public_url(bad).is_err(), "{bad}");
        }
    }

    use super::*;
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
    fn verified_downloads_keep_only_matching_files() {
        let never = AtomicBool::new(false);
        let dir = crate::TempDir::new("verify");
        let dest = dir.join("file");
        let good = hex(&Sha256::digest(b"hello world"));
        let bad = hex(&Sha256::digest(b"something else"));
        assert!(download_verified(&serve(b"hello world", false), &dest, Some(11), &bad, &never, |_| {}).is_err());
        assert!(download_verified(&serve(b"hello world", false), &dest, Some(5), &good, &never, |_| {}).is_err());
        assert!(!dest.exists() && !dest.with_extension("part").exists(), "a mismatch leaves nothing behind");
        download_verified(&serve(b"hello world", false), &dest, Some(11), &good, &never, |_| {}).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"hello world");
    }

    #[test]
    fn small_bodies_have_a_limit() {
        let never = AtomicBool::new(false);
        assert_eq!(get_bytes(&serve(b"hello", false), 5, &never).unwrap(), b"hello");
        assert!(get_bytes(&serve(b"hello world", false), 5, &never).is_err());
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
        assert!(crate::is_canceled(&err.into()));
        assert!(started.elapsed() < Duration::from_secs(2), "took {:?}", started.elapsed());
    }
}
