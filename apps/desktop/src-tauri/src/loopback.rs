//! What the app's local HTTP servers share: the app server (app_server.rs,
//! the app's pages), the media server (media.rs, local video files) and the
//! tests' hook (e2e.rs). All answer only on this computer's loopback
//! addresses, through [`serve`].

use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::net::{Ipv4Addr, Ipv6Addr, Shutdown, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::{Result, bail};

/// One free port on both 127.0.0.1 and [::1]. Webviews resolve `localhost` to
/// either (often ::1 first), so the app holds both: otherwise another account
/// on the computer could listen on the other one and get the app's commands.
// Only release builds serve the app over HTTP (app_server.rs).
#[cfg_attr(dev, allow(dead_code))]
pub(crate) fn bind_both() -> Result<(Vec<TcpListener>, u16)> {
    for _ in 0..20 {
        let v4 = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = v4.local_addr()?.port();
        match TcpListener::bind((Ipv6Addr::LOCALHOST, port)) {
            Ok(v6) => return Ok((vec![v4, v6], port)),
            // Taken on ::1, perhaps on purpose: try another port.
            Err(e) if e.kind() == ErrorKind::AddrInUse => continue,
            // No IPv6 here, so `localhost` can only mean 127.0.0.1.
            Err(_) => return Ok((vec![v4], port)),
        }
    }
    bail!("no free loopback port")
}

/// Whether to answer `request`, or the status to refuse it with: it must be a
/// GET or HEAD (else 405), naming exactly this server's `host` (else 403), so
/// a page that reaches the server through DNS rebinding, with its own name in
/// the Host header, gets nothing.
pub(crate) fn check(request: &Request, host: &str) -> Result<(), u16> {
    if !matches!(request.method.as_str(), "GET" | "HEAD") {
        return Err(405);
    }
    if !request.headers("Host").any(|h| h == host) {
        return Err(403);
    }
    Ok(())
}

/// A request as read: its method, URL (path and query, as sent), headers and body.
pub(crate) struct Request {
    pub(crate) method: String,
    pub(crate) url: String,
    headers: Vec<(String, String)>,
    pub(crate) body: Vec<u8>,
}

impl Request {
    /// The values of the headers called `name` (in any case).
    pub(crate) fn headers<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> {
        self.headers.iter().filter(move |(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    /// The value of the first header called `name`.
    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

/// An answer: a status, headers, and a body of a known length.
pub(crate) struct Response {
    status: u16,
    headers: Vec<(&'static str, String)>,
    body: Box<dyn Read + Send>,
    len: u64,
    then: Option<Box<dyn FnOnce() + Send>>,
}

impl Response {
    /// `status` with no body.
    pub(crate) fn empty(status: u16) -> Self {
        Self::bytes(status, Vec::new())
    }

    pub(crate) fn bytes(status: u16, body: Vec<u8>) -> Self {
        let len = body.len() as u64;
        Self::reader(status, Box::new(std::io::Cursor::new(body)), len)
    }

    /// `len` bytes read from `body` (a file streams as it's sent).
    pub(crate) fn reader(status: u16, body: Box<dyn Read + Send>, len: u64) -> Self {
        Self { status, headers: Vec::new(), body, len, then: None }
    }

    pub(crate) fn header(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
    }

    /// Runs `then` once the answer is sent.
    // Only the tests' builds use it (their request log, e2e.rs's quit).
    #[cfg_attr(not(feature = "e2e"), allow(dead_code))]
    pub(crate) fn then(mut self, then: impl FnOnce() + Send + 'static) -> Self {
        self.then = Some(Box::new(then));
        self
    }
}

/// What a server answers a request with.
pub(crate) type Answer = dyn Fn(&Request) -> Response + Send + Sync;

/// Answers each connection to `listener` on a thread of its own, one request
/// per connection, with `answer`; past `max_active` connections at once, with
/// 503. Always sends the body's length (players need it to seek and to show a
/// timeline) and closes the connection after. A request's body may be at most
/// `max_body` bytes (else 413).
///
/// This was tiny_http, and the page then sometimes stayed empty for two
/// minutes at start (WebKitGTK and macOS's WebKit, mostly in the tests): its
/// thread pool can count a thread as free that it has already woken for an
/// earlier connection, and then leaves the new connection waiting until a
/// thread comes back, which a kept-alive connection only does when the web
/// view drops it, about two minutes later. The start's burst of script
/// preloads hit that, one preload never got its answer, and the page waited
/// on it. (tiny_http also drops a `Connection: close` set on a response.)
pub(crate) fn serve(listener: TcpListener, name: &str, max_active: usize, max_body: u64, answer: Arc<Answer>) {
    let active = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        // Refused here, without a thread or a wait for its request: another
        // program on the computer opening connections by the thousand would
        // otherwise get a thread in the app for each.
        if active.load(Ordering::SeqCst) >= max_active {
            refuse(stream);
            continue;
        }
        // Only this loop adds, so the count checked is the count added to.
        active.fetch_add(1, Ordering::SeqCst);
        let (answer, done) = (answer.clone(), active.clone());
        let spawned = std::thread::Builder::new().name(name.into()).spawn(move || {
            let _ = handle(stream, max_body, &*answer);
            done.fetch_sub(1, Ordering::SeqCst);
        });
        if spawned.is_err() {
            active.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

/// Answers `stream` 503 without reading its request.
fn refuse(mut stream: TcpStream) {
    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
    let _ = stream.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    let _ = stream.shutdown(Shutdown::Write);
}

/// The most a request's head (its line and headers) may take.
const MAX_HEAD: u64 = 64 * 1024;
/// The most a body may take where one is taken at all (the tests' hook).
#[cfg_attr(not(feature = "e2e"), allow(dead_code))]
pub(crate) const MAX_BODY: u64 = 16 * 1024 * 1024;

/// Reads one request from `stream`, answers it and closes the connection.
fn handle(mut stream: TcpStream, max_body: u64, answer: &Answer) -> std::io::Result<()> {
    // A connection that never sends its request doesn't hold a thread for long.
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut reader = BufReader::new(&stream);
    let mut head = (&mut reader).take(MAX_HEAD);
    let mut first = String::new();
    head.read_line(&mut first)?;
    let mut words = first.split_whitespace();
    let (method, url) = (words.next().unwrap_or("").to_string(), words.next().unwrap_or("/").to_string());
    let (mut line, mut headers) = (String::new(), Vec::new());
    loop {
        line.clear();
        if head.read_line(&mut line)? == 0 || line.trim_end().is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_string(), value.trim().to_string()));
        }
    }
    let mut request = Request { method, url, headers, body: Vec::new() };
    let len = request.header("Content-Length").and_then(|n| n.parse::<u64>().ok()).unwrap_or(0);
    if len > max_body {
        return write(&mut stream, &request, Response::empty(413));
    }
    (&mut reader).take(len).read_to_end(&mut request.body)?;
    drop(reader);
    let response = answer(&request);
    write(&mut stream, &request, response)
}

fn write(stream: &mut TcpStream, request: &Request, mut response: Response) -> std::io::Result<()> {
    let mut head = format!("HTTP/1.1 {} {}\r\n", response.status, reason(response.status));
    for (name, value) in &response.headers {
        head += &format!("{name}: {value}\r\n");
    }
    head += &format!("Content-Length: {}\r\nConnection: close\r\n\r\n", response.len);
    stream.write_all(head.as_bytes())?;
    if request.method != "HEAD" {
        std::io::copy(&mut (&mut response.body).take(response.len), stream)?;
    }
    stream.flush()?;
    if let Some(then) = response.then.take() {
        then();
    }
    Ok(())
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        206 => "Partial Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Content Too Large",
        416 => "Range Not Satisfiable",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(method: &str, host: Option<&str>) -> Request {
        let headers = host.map(|h| ("Host".to_string(), h.to_string())).into_iter().collect();
        Request { method: method.into(), url: "/".into(), headers, body: Vec::new() }
    }

    #[test]
    fn answers_only_its_own_host_and_reads() {
        let host = "localhost:41723";
        assert_eq!(check(&request("GET", Some(host)), host), Ok(()));
        assert_eq!(check(&request("HEAD", Some(host)), host), Ok(()));
        assert_eq!(check(&request("POST", Some(host)), host), Err(405));
        assert_eq!(check(&request("GET", Some("evil.example:41723")), host), Err(403), "DNS rebinding");
        assert_eq!(check(&request("GET", Some("127.0.0.1:41723")), host), Err(403), "another name for it");
        assert_eq!(check(&request("GET", Some("localhost:41724")), host), Err(403));
        assert_eq!(check(&request("GET", None), host), Err(403));
    }

    /// A server on 127.0.0.1 answering every request with 200, `max_active`
    /// at once; its address.
    fn server(name: &'static str, max_active: usize) -> std::net::SocketAddr {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let answer = |_: &Request| Response::empty(200);
        std::thread::spawn(move || serve(listener, name, max_active, 0, Arc::new(answer)));
        addr
    }

    /// What `stream` is answered within two seconds (well inside the read
    /// timeout a server waiting on the request would take).
    fn answer_to(mut stream: &TcpStream) -> String {
        stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut out = Vec::new();
        let _ = stream.read_to_end(&mut out);
        String::from_utf8_lossy(&out).into_owned()
    }

    /// Threads of this process named `name`, where that can be counted.
    fn threads_named(name: &str) -> Option<usize> {
        let tasks = std::fs::read_dir("/proc/self/task").ok()?;
        let named =
            |t: &std::fs::DirEntry| std::fs::read_to_string(t.path().join("comm")).is_ok_and(|c| c.trim() == name);
        Some(tasks.flatten().filter(named).count())
    }

    #[test]
    fn past_its_limit_refuses_at_once_without_a_thread_each() {
        // Another program on the computer opening connections and sending
        // nothing: each one past the limit is refused now, not held on a
        // thread of its own while its request is waited for.
        let addr = server("lb-busy", 2);
        let held: Vec<_> = (0..2).map(|_| TcpStream::connect(addr).unwrap()).collect();
        std::thread::sleep(Duration::from_millis(200));
        let more: Vec<_> = (0..50).map(|_| TcpStream::connect(addr).unwrap()).collect();
        std::thread::sleep(Duration::from_millis(200));
        if let Some(n) = threads_named("lb-busy") {
            assert!(n <= 2, "{n} threads for 2 connections served and 50 refused");
        }
        for stream in &more {
            let text = answer_to(stream);
            assert!(text.starts_with("HTTP/1.1 503"), "{text:?}");
        }
        drop(held);
    }

    #[test]
    fn a_body_where_none_is_taken_is_refused_unread() {
        // A web page or program sending a large body to a server that takes
        // none: refused before it's read, not waited for and kept in memory.
        let addr = server("lb-body", 4);
        let mut stream = TcpStream::connect(addr).unwrap();
        let host = format!("Host: {addr}");
        stream.write_all(format!("POST / HTTP/1.1\r\n{host}\r\nContent-Length: 16000000\r\n\r\n").as_bytes()).unwrap();
        let text = answer_to(&stream);
        assert!(text.starts_with("HTTP/1.1 413"), "{text:?}");
    }

    #[test]
    fn holds_one_port_on_both_loopbacks() {
        let (listeners, port) = bind_both().unwrap();
        let addrs: Vec<_> = listeners.iter().map(|l| l.local_addr().unwrap()).collect();
        assert!(addrs.iter().all(|a| a.port() == port && a.ip().is_loopback()));
        assert!(addrs.iter().any(|a| a.is_ipv4()));
        // Where there's IPv6, ::1 is held too, so nothing else can answer `localhost` there.
        if TcpListener::bind((Ipv6Addr::LOCALHOST, 0)).is_ok() {
            assert!(addrs.iter().any(|a| a.is_ipv6()), "{addrs:?}");
        }
    }
}
