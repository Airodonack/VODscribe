//! The local servers' shared specs (loopback.rs): the port held on both
//! loopback addresses, the requests answered and refused, and the bounds
//! on what another program on the computer can make the app do.

use super::*;

fn request(method: &str, host: Option<&str>) -> Request {
    let headers = host.map(|h| ("Host".to_string(), h.to_string())).into_iter().collect();
    Request { method: method.into(), url: "/".into(), headers, body: Vec::new() }
}

/// A server on 127.0.0.1 answering every request with `answer`,
/// `max_active` at once, taking bodies of `max_body` bytes at most; its
/// address.
fn server(
    name: &'static str,
    max_active: usize,
    max_body: u64,
    answer: impl Fn(&Request) -> Response + Send + Sync + 'static,
) -> std::net::SocketAddr {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || serve(listener, name, max_active, max_body, Arc::new(answer)));
    addr
}

/// `request` sent on a new connection to `addr`.
fn ask(addr: std::net::SocketAddr, request: &str) -> TcpStream {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    stream
}

/// What `stream` is answered within two seconds (well inside the read
/// timeout a server waiting on the request would take), to the
/// connection's end.
fn answer_to(mut stream: &TcpStream) -> String {
    stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let mut out = Vec::new();
    let _ = stream.read_to_end(&mut out);
    String::from_utf8_lossy(&out).into_owned()
}

/// Threads of this process named `name`, where that can be counted.
fn threads_named(name: &str) -> Option<usize> {
    let tasks = std::fs::read_dir("/proc/self/task").ok()?;
    let named = |t: &std::fs::DirEntry| std::fs::read_to_string(t.path().join("comm")).is_ok_and(|c| c.trim() == name);
    Some(tasks.flatten().filter(named).count())
}

/// The port is held on 127.0.0.1 and, where there's IPv6, on ::1 (loopback.rs
/// `bind_both`: "Webviews resolve `localhost` to either (often ::1 first),
/// so the app holds both: otherwise another account on the computer could
/// listen on the other one and get the app's commands").
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

/// Only a GET or HEAD naming exactly the server's own host is answered
/// (loopback.rs `check`: "it must be a GET or HEAD (else 405), naming
/// exactly this server's `host` (else 403), so a page that reaches the
/// server through DNS rebinding, with its own name in the Host header, gets
/// nothing").
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

/// Each answer says its body's length and closes the connection after it;
/// a HEAD's has no body (loopback.rs `serve`: "one request per connection
/// … Always sends the body's length (players need it to seek and to show a
/// timeline) and closes the connection after").
#[test]
fn each_answer_says_its_length_and_closes_the_connection() {
    let addr = server("lb-answer", 4, 0, |_| Response::bytes(200, b"x".to_vec()));
    let text = answer_to(&ask(addr, &format!("GET /a.js HTTP/1.1\r\nHost: {addr}\r\n\r\n")));
    assert!(text.starts_with("HTTP/1.1 200 OK\r\n"), "{text}");
    assert!(text.contains("Content-Length: 1\r\n") && text.contains("Connection: close\r\n"), "{text}");
    // The connection ended once the answer was sent: all of it was read.
    assert!(text.ends_with("\r\n\r\nx"), "{text}");
    let head = answer_to(&ask(addr, &format!("HEAD /a.js HTTP/1.1\r\nHost: {addr}\r\n\r\n")));
    assert!(head.starts_with("HTTP/1.1 200 OK") && head.contains("Content-Length: 1\r\n"), "{head}");
    assert!(head.ends_with("\r\n\r\n"), "a HEAD's answer has no body: {head}");
}

/// Past its limit of connections at once, each one more is refused with
/// 503 at once, without a thread or a wait for its request (loopback.rs
/// `serve`: "past `max_active` connections at once, with 503"; "Refused
/// here, without a thread or a wait for its request: another program on
/// the computer opening connections by the thousand would otherwise get a
/// thread in the app for each"). The connections are taken in the order
/// they were made, so the two held come first.
#[test]
fn past_its_limit_refuses_at_once_without_a_thread_each() {
    let addr = server("lb-busy", 2, 0, |_| Response::empty(200));
    // Another program opening connections and sending nothing.
    let held: Vec<_> = (0..2).map(|_| TcpStream::connect(addr).unwrap()).collect();
    let more: Vec<_> = (0..50).map(|_| TcpStream::connect(addr).unwrap()).collect();
    for stream in &more {
        let text = answer_to(stream);
        assert!(text.starts_with("HTTP/1.1 503"), "{text:?}");
    }
    if let Some(n) = threads_named("lb-busy") {
        assert!(n <= 2, "{n} threads for 2 connections served and 50 refused");
    }
    drop(held);
}

/// A body sent to a server that takes none is refused before it's read
/// (loopback.rs `serve`: "A request's body may be at most `max_body` bytes
/// (else 413)"): a web page or program sending a large body isn't waited
/// for and kept in memory.
#[test]
fn a_body_where_none_is_taken_is_refused_unread() {
    let addr = server("lb-body", 4, 0, |_| Response::empty(200));
    let mut stream = TcpStream::connect(addr).unwrap();
    let host = format!("Host: {addr}");
    stream.write_all(format!("POST / HTTP/1.1\r\n{host}\r\nContent-Length: 16000000\r\n\r\n").as_bytes()).unwrap();
    let text = answer_to(&stream);
    assert!(text.starts_with("HTTP/1.1 413"), "{text:?}");
}

/// A body up to the server's limit is read and given to its answer; one
/// byte more is refused (loopback.rs `serve`: "A request's body may be at
/// most `max_body` bytes (else 413)").
#[test]
fn a_body_up_to_the_limit_is_read_and_one_past_it_refused() {
    let addr = server("lb-limit", 4, 16, |request| Response::bytes(200, request.body.clone()));
    let post = |body: &str| {
        let request = format!("POST / HTTP/1.1\r\nHost: {addr}\r\nContent-Length: {}\r\n\r\n{body}", body.len());
        answer_to(&ask(addr, &request))
    };
    let whole = post("sixteen bytes ok");
    assert!(whole.starts_with("HTTP/1.1 200") && whole.ends_with("\r\n\r\nsixteen bytes ok"), "{whole:?}");
    let over = post("seventeen bytes!!");
    assert!(over.starts_with("HTTP/1.1 413"), "{over:?}");
}

/// A request's head is read whole up to 64 KiB, its last header too
/// (loopback.rs `MAX_HEAD`: "The most a request's head (its line and
/// headers) may take").
#[test]
fn a_request_head_up_to_its_most_is_read_whole() {
    let addr = server("lb-head", 4, 0, |request| Response::bytes(200, request.header("Host").unwrap_or("").into()));
    let padding = "a".repeat(60 * 1024);
    let text = answer_to(&ask(addr, &format!("GET / HTTP/1.1\r\nX-Padding: {padding}\r\nHost: localhost\r\n\r\n")));
    assert!(text.ends_with("\r\n\r\nlocalhost"), "{}", &text[..text.len().min(200)]);
}
