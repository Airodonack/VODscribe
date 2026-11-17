//! The network as a test has it (`Net`): where a name resolves, and the
//! answer to each request. Three seams in the library's code ask it first:
//!
//! - `net::public_url`'s lookup of a link's host (`resolve`), so a link with
//!   a site's name needs no real lookup;
//! - `net::get`, `net::get_from` and `net::get_bytes` (thumbnails, emotes,
//!   Kick's playlists, yt-dlp's checksums, the GPU add-on's downloads),
//!   answered in place of a request (`answer`), its `Range` honoured and its
//!   body held partway where a test says (`Net::holds_partway`);
//! - the HTTP clients (`net::agent`, and the file host's in `annotations`),
//!   built on `Connector` and `Resolver`: a request to a link that's the
//!   fake's goes over a connection to the fake, which reads it as a site
//!   would and answers in HTTP, so the client does with the answer what it
//!   does with a site's (its status, headers, redirects and limits). Kick's
//!   API, Twitch's GQL, the emote services and the file host are reached so.
//!
//! A host a test names (`public`, `local`) is the fake's while the test
//! holds its `Net`: a request to it that nothing answers gets 404, as a site
//! answers for a page it doesn't have, and never reaches the network. Any
//! other link no test answers goes to the network as before.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, MutexGuard};

use serde_json::Value;
use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs};
use ureq::unversioned::transport::{Buffers, ConnectionDetails, DefaultConnector, LazyBuffers, NextTimeout, Transport};

use super::tools::Gate;

/// Where a host a test says is public resolves: an address on the public
/// internet, which nothing is ever sent to, since its requests are the fake's.
pub const PUBLIC: IpAddr = IpAddr::V4(Ipv4Addr::new(93, 184, 215, 14));
/// Where a host a test says is local resolves: an address on the local network.
pub const LOCAL: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20));

/// The hosts of the sites and services the library reaches (`Net::sites`):
/// the sites' pages, their APIs and image servers, the emote services and
/// the file host.
pub const SITES: &[&str] = &[
    "twitch.tv",
    "www.twitch.tv",
    "m.twitch.tv",
    "clips.twitch.tv",
    "gql.twitch.tv",
    "static-cdn.jtvnw.net",
    "youtube.com",
    "www.youtube.com",
    "m.youtube.com",
    "music.youtube.com",
    "youtu.be",
    "i.ytimg.com",
    "yt3.ggpht.com",
    "kick.com",
    "www.kick.com",
    "web.kick.com",
    "files.kick.com",
    "images.kick.com",
    "stream.kick.com",
    "7tv.io",
    "cdn.7tv.app",
    "api.betterttv.net",
    "cdn.betterttv.net",
    "api.frankerfacez.com",
    "cdn.frankerfacez.com",
    "dl.vodscribe.com",
    "img.vodscribe.com",
];

static RULES: Mutex<Vec<Rule>> = Mutex::new(Vec::new());

fn rules() -> MutexGuard<'static, Vec<Rule>> {
    RULES.lock().unwrap_or_else(|e| e.into_inner())
}

struct Rule {
    what: What,
    /// The `Net` it's part of, so dropping that takes back only its own.
    owner: usize,
}

enum What {
    /// A host that resolves to this address, and is the fake's.
    Host(String, IpAddr),
    /// Requests to links starting with the prefix wait at the gate.
    Hold(String, Gate),
    /// Requests to links starting with the prefix get this answer.
    Answer(String, Answer),
    /// GETs of links starting with the prefix (`net::get_from`) send the
    /// file up to this byte, then wait at the gate for the rest.
    Partway(String, u64, Gate),
    /// Requests to links starting with the prefix are kept, as asked.
    Record(String, Arc<Mutex<Vec<Request>>>),
}

#[derive(Clone)]
enum Answer {
    /// The file, or the part of it a `Range` asks for (`Reply::ranged`).
    Body(Arc<Vec<u8>>),
    /// The whole file, whatever is asked, as a server without ranges sends it.
    Whole(Arc<Vec<u8>>),
    Status(u16),
    /// Answers what it knows; None for a request that isn't its own.
    Replies(Arc<Replier>),
}

type Replier = dyn Fn(&Request) -> Option<Reply> + Send + Sync;

/// A request as the site gets it.
#[derive(Clone, Debug)]
pub struct Request {
    /// `GET`, `POST`, `HEAD`…
    pub method: String,
    /// The whole link: `https://host/path?query`.
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    /// Header `name`'s value, its name in any case.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    /// The body read as JSON (a GraphQL request's query and variables).
    pub fn json(&self) -> Option<Value> {
        serde_json::from_slice(&self.body).ok()
    }
}

/// A site's answer to a request.
#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Reply {
    /// 200, with `body`.
    pub fn body(body: impl Into<Vec<u8>>) -> Self {
        Reply { status: 200, headers: Vec::new(), body: body.into() }
    }

    /// 200, with `json` as the body.
    pub fn json(json: &Value) -> Self {
        Self::body(json.to_string()).with_header("Content-Type", "application/json")
    }

    /// `status` (404, 429, 500…), with no body.
    pub fn status(status: u16) -> Self {
        Reply { status, headers: Vec::new(), body: Vec::new() }
    }

    /// `file` as a server that honours ranges answers `request`: the part
    /// its `Range: bytes=<from>-[<to>]` asks for (206, with its
    /// `Content-Range`), a range past its end refused (416), or all of it
    /// (200) for a request that asks for none.
    pub fn ranged(file: &[u8], request: &Request) -> Self {
        let Some((from, to)) = request.header("Range").and_then(|r| parse_range(r, file.len())) else {
            return Self::body(file.to_vec());
        };
        if from >= file.len() || to < from {
            return Self::status(416).with_header("Content-Range", &format!("bytes */{}", file.len()));
        }
        let to = to.min(file.len() - 1);
        Reply { status: 206, headers: Vec::new(), body: file[from..=to].to_vec() }
            .with_header("Content-Range", &format!("bytes {from}-{to}/{}", file.len()))
    }

    /// The same, with header `name` set to `value` (an `ETag`, say).
    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

/// The network a test has, until it's dropped. Tests run at once in one
/// process and all see it: give a test's links a host of its own, or, on a
/// site's own host (Twitch's GQL, say), answer only the requests for the
/// test's own ids (`replies`).
pub struct Net {
    owner: usize,
}

impl Net {
    pub fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);
        Net { owner: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed) }
    }

    fn add(&self, what: What) -> &Self {
        let mut rules = rules();
        rules.push(Rule { what, owner: self.owner });
        // The longest prefix first, so a link answered alone beats its host.
        rules.sort_by_key(|r| std::cmp::Reverse(r.prefix().map_or(0, str::len)));
        self
    }

    /// `host` resolves to an address on the public internet (`PUBLIC`),
    /// and its requests are the fake's: those nothing answers get 404.
    pub fn public(&self, host: &str) -> &Self {
        self.add(What::Host(host.to_ascii_lowercase(), PUBLIC))
    }

    /// Every host in `SITES` is `public`: a test of the app reaches no
    /// real site, and those nothing answers get 404.
    pub fn sites(&self) -> &Self {
        for host in SITES {
            self.public(host);
        }
        self
    }

    /// `host` resolves to an address on the local network (`LOCAL`), as a
    /// router's name does; its requests are the fake's, as `public`'s are.
    pub fn local(&self, host: &str) -> &Self {
        self.add(What::Host(host.to_ascii_lowercase(), LOCAL))
    }

    /// Links starting with `prefix` answer `body`, whatever is asked.
    pub fn serves(&self, prefix: &str, body: impl Into<Vec<u8>>) -> &Self {
        self.add(What::Answer(prefix.into(), Answer::Body(Arc::new(body.into()))))
    }

    /// Links starting with `prefix` answer `body` whole, whatever range is
    /// asked: a server that doesn't honour ranges.
    pub fn serves_whole(&self, prefix: &str, body: impl Into<Vec<u8>>) -> &Self {
        self.add(What::Answer(prefix.into(), Answer::Whole(Arc::new(body.into()))))
    }

    /// GETs of links starting with `prefix` send their file up to byte `at`
    /// (of the file, wherever the answer starts), then wait at `gate` for
    /// the rest, as a download that's partway when the test acts. Let go
    /// when this is dropped (`let_go`).
    pub fn holds_partway(&self, prefix: &str, at: u64, gate: &Gate) -> &Self {
        self.add(What::Partway(prefix.into(), at, gate.clone()))
    }

    /// The requests to links starting with `prefix` from now, kept as each
    /// came (`Recorded::requests`).
    pub fn records(&self, prefix: &str) -> Recorded {
        let recorded = Recorded::default();
        self.add(What::Record(prefix.into(), recorded.0.clone()));
        recorded
    }

    /// Links starting with `prefix` answer HTTP `status` (404, 403, 500…)
    /// with no body.
    pub fn answers(&self, prefix: &str, status: u16) -> &Self {
        self.add(What::Answer(prefix.into(), Answer::Status(status)))
    }

    /// Requests to links starting with `prefix` are answered by `reply`,
    /// which reads each as the site does: its answer, or None for one that
    /// isn't this test's, which the next answer for the link is asked.
    pub fn replies(&self, prefix: &str, reply: impl Fn(&Request) -> Option<Reply> + Send + Sync + 'static) -> &Self {
        self.add(What::Answer(prefix.into(), Answer::Replies(Arc::new(reply))))
    }

    /// Requests to links starting with `prefix` wait at `gate`, as a slow
    /// site keeps them, then are answered: links that are the fake's (a
    /// host named, or a link answered). Let go when this is dropped.
    pub fn holds(&self, prefix: &str, gate: &Gate) -> &Self {
        self.add(What::Hold(prefix.into(), gate.clone()))
    }

    /// Lets go of the requests its holds keep, and holds none from now
    /// (`holds`), as its drop does: for the end of a test whose app waits
    /// for what it asked.
    pub fn let_go(&self) {
        rules().retain(|r| match &r.what {
            What::Hold(_, gate) | What::Partway(_, _, gate) if r.owner == self.owner => {
                gate.open();
                false
            }
            _ => true,
        });
    }
}

impl Default for Net {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Net {
    fn drop(&mut self) {
        self.let_go();
        rules().retain(|r| r.owner != self.owner);
    }
}

impl Rule {
    fn prefix(&self) -> Option<&str> {
        match &self.what {
            What::Host(..) => None,
            What::Hold(prefix, _) | What::Answer(prefix, _) | What::Partway(prefix, ..) | What::Record(prefix, _) => {
                Some(prefix)
            }
        }
    }
}

/// `url`'s host, lowercase, without its port or user.
fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = match host.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or_default(),
        None => host.split(':').next().unwrap_or_default(),
    };
    host.to_ascii_lowercase()
}

/// Where `host` resolves, if a test says (`net::public_url`'s lookup).
pub(crate) fn resolve(host: &str) -> Option<std::io::Result<Vec<IpAddr>>> {
    let host = host.to_ascii_lowercase();
    rules().iter().find_map(|r| match &r.what {
        What::Host(h, ip) if *h == host => Some(Ok(vec![*ip])),
        _ => None,
    })
}

/// Whether a request to `url` is the fake's: its host is one a test names,
/// or a test answers the link.
fn faked(url: &str) -> bool {
    let host = host_of(url);
    rules().iter().any(|r| match &r.what {
        What::Host(h, _) => *h == host,
        What::Answer(prefix, _) => url.starts_with(prefix.as_str()),
        What::Hold(..) | What::Partway(..) | What::Record(..) => false,
    })
}

/// The fake's answer to `request`, once any hold on it lets go: the first
/// answer for its link that gives one, else 404 for a link that's the
/// fake's. None for one that isn't (`faked`).
fn respond(request: &Request) -> Option<Reply> {
    let url = request.url.as_str();
    for rule in rules().iter() {
        if let What::Record(prefix, kept) = &rule.what
            && url.starts_with(prefix.as_str())
        {
            kept.lock().unwrap().push(request.clone());
        }
    }
    let holds: Vec<Gate> = rules()
        .iter()
        .filter_map(|r| match &r.what {
            What::Hold(prefix, gate) if url.starts_with(prefix.as_str()) => Some(gate.clone()),
            _ => None,
        })
        .collect();
    for gate in holds {
        gate.wait();
    }
    // Taken out first: a reply may take its time, and other tests' requests
    // go on meanwhile.
    let answers: Vec<Answer> = rules()
        .iter()
        .filter_map(|r| match &r.what {
            What::Answer(prefix, answer) if url.starts_with(prefix.as_str()) => Some(answer.clone()),
            _ => None,
        })
        .collect();
    for answer in answers {
        let reply = match answer {
            Answer::Body(body) => Some(Reply::ranged(&body, request)),
            Answer::Whole(body) => Some(Reply::body(body.to_vec())),
            Answer::Status(code) => Some(Reply::status(code)),
            Answer::Replies(reply) => reply(request),
        };
        if reply.is_some() {
            return reply;
        }
    }
    faked(url).then(|| Reply::status(404))
}

/// The requests a test keeps (`Net::records`).
#[derive(Clone, Default)]
pub struct Recorded(Arc<Mutex<Vec<Request>>>);

impl Recorded {
    /// The requests kept so far, in the order they came.
    pub fn requests(&self) -> Vec<Request> {
        self.0.lock().unwrap().clone()
    }
}

/// The part of the file a `Range` header asks for, as first and last byte
/// (`bytes=<from>-` to its end, `bytes=<from>-<to>`); None for a range of
/// another kind.
fn parse_range(range: &str, len: usize) -> Option<(usize, usize)> {
    let (from, to) = range.trim().strip_prefix("bytes=")?.split_once('-')?;
    let from = from.trim().parse().ok()?;
    let to = match to.trim() {
        "" => len.saturating_sub(1),
        to => to.parse().ok()?,
    };
    Some((from, to))
}

/// A GET's answer from the fake (`net::get_from`).
pub(crate) struct Answered {
    pub(crate) status: u16,
    pub(crate) body: Vec<u8>,
    /// Where a 206's `Content-Range` says it starts in the file.
    pub(crate) start: Option<u64>,
    /// Where in `body` it waits, and the gate it waits at (`Net::holds_partway`).
    pub(crate) hold: Option<(usize, Gate)>,
}

/// The answer to a GET of `url` with `headers`, if it's the fake's
/// (`net::get_from`): its body (from where a `Range` asks), or a status that
/// isn't success as `ureq` reports it.
pub(crate) fn answer(url: &str, headers: &[(String, String)]) -> Option<Result<Answered, anyhow::Error>> {
    if !faked(url) {
        return None;
    }
    let request = Request { method: "GET".into(), url: url.into(), headers: headers.to_vec(), body: Vec::new() };
    let reply = respond(&request)?;
    if !(200..=299).contains(&reply.status) {
        return Some(Err(ureq::Error::StatusCode(reply.status).into()));
    }
    // Where this answer starts in the file: a 206's `Content-Range`.
    let start = reply
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("Content-Range"))
        .and_then(|(_, v)| crate::net::range_start(v))
        .filter(|_| reply.status == 206);
    let from = start.unwrap_or(0);
    let hold = rules().iter().find_map(|r| match &r.what {
        What::Partway(prefix, at, gate) if url.starts_with(prefix.as_str()) => Some((*at, gate.clone())),
        _ => None,
    });
    let len = reply.body.len() as u64;
    let hold = hold.filter(|&(at, _)| at >= from && at < from + len).map(|(at, gate)| ((at - from) as usize, gate));
    Some(Ok(Answered { status: reply.status, body: reply.body, start, hold }))
}

/// The HTTP clients' connections (`net::agent`): to the fake for a link
/// that's the fake's, else to the network as before (ureq's own).
#[derive(Debug, Default)]
pub struct Connector(DefaultConnector);

impl ureq::unversioned::transport::Connector for Connector {
    type Out = Box<dyn Transport>;

    fn connect(&self, details: &ConnectionDetails, chained: Option<()>) -> Result<Option<Self::Out>, ureq::Error> {
        let uri = details.uri;
        if !faked(&uri.to_string()) {
            return self.0.connect(details, chained);
        }
        let authority = uri.authority().map_or("", |a| a.as_str());
        let origin = format!("{}://{authority}", uri.scheme_str().unwrap_or("https"));
        let config = details.config;
        Ok(Some(Box::new(Connection {
            buffers: LazyBuffers::new(config.input_buffer_size(), config.output_buffer_size()),
            https: details.needs_tls(),
            origin,
            sent: Vec::new(),
            answer: None,
            given: 0,
        })))
    }
}

/// The HTTP clients' lookups (`net::agent`): none for a link that's the
/// fake's, else the system's as before (ureq's own).
#[derive(Debug, Default)]
pub struct Resolver(DefaultResolver);

impl ureq::unversioned::resolver::Resolver for Resolver {
    fn resolve(
        &self,
        uri: &ureq::http::Uri,
        config: &ureq::config::Config,
        timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        if !faked(&uri.to_string()) {
            return self.0.resolve(uri, config, timeout);
        }
        let mut addrs = self.empty();
        addrs.push(SocketAddr::new(PUBLIC, uri.port_u16().unwrap_or(443)));
        Ok(addrs)
    }
}

/// One connection to the fake: what the client sends is the request, read
/// whole once the client waits for the answer; then the answer, in
/// HTTP/1.1, and the connection closes.
#[derive(Debug)]
struct Connection {
    buffers: LazyBuffers,
    https: bool,
    /// The link's scheme and host, which the request line leaves out.
    origin: String,
    sent: Vec<u8>,
    answer: Option<Vec<u8>>,
    /// How much of the answer the client has.
    given: usize,
}

impl Transport for Connection {
    fn buffers(&mut self) -> &mut dyn Buffers {
        &mut self.buffers
    }

    fn transmit_output(&mut self, amount: usize, _timeout: NextTimeout) -> Result<(), ureq::Error> {
        self.sent.extend_from_slice(&self.buffers.output()[..amount]);
        Ok(())
    }

    fn await_input(&mut self, _timeout: NextTimeout) -> Result<bool, ureq::Error> {
        if self.answer.is_none() {
            let request = read_request(&self.origin, &self.sent)
                .ok_or_else(|| ureq::Error::Io(std::io::Error::other("the fake site couldn't read the request")))?;
            let reply = respond(&request).unwrap_or_else(|| Reply::status(404));
            self.answer = Some(written(&request, &reply));
        }
        let answer = self.answer.as_deref().unwrap_or_default();
        let input = self.buffers.input_append_buf();
        let n = input.len().min(answer.len() - self.given);
        if n == 0 {
            return Err(ureq::Error::Io(std::io::ErrorKind::UnexpectedEof.into()));
        }
        input[..n].copy_from_slice(&answer[self.given..self.given + n]);
        self.given += n;
        self.buffers.input_appended(n);
        Ok(true)
    }

    fn is_open(&mut self) -> bool {
        self.answer.as_ref().is_none_or(|a| self.given < a.len())
    }

    fn is_tls(&self) -> bool {
        // As a TLS connection is for an https link: the fake reads it plain.
        self.https
    }
}

/// The request in `sent` (its line, headers and body) to a link on `origin`.
fn read_request(origin: &str, sent: &[u8]) -> Option<Request> {
    let end = sent.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = std::str::from_utf8(&sent[..end]).ok()?;
    let mut lines = head.split("\r\n");
    let mut line = lines.next()?.split(' ');
    let (method, target) = (line.next()?.to_string(), line.next()?);
    let headers: Vec<(String, String)> =
        lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect();
    let rest = &sent[end + 4..];
    let chunked = headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("transfer-encoding") && v.contains("chunked"));
    let body = if chunked { unchunked(rest)? } else { rest.to_vec() };
    let url = if target.contains("://") { target.to_string() } else { format!("{origin}{target}") };
    Some(Request { method, url, headers, body })
}

/// A chunked body's bytes.
fn unchunked(mut rest: &[u8]) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    loop {
        let end = rest.windows(2).position(|w| w == b"\r\n")?;
        let size = std::str::from_utf8(&rest[..end]).ok()?.split(';').next()?.trim();
        let size = usize::from_str_radix(size, 16).ok()?;
        rest = &rest[end + 2..];
        if size == 0 {
            return Some(body);
        }
        body.extend_from_slice(rest.get(..size)?);
        rest = rest.get(size + 2..)?;
    }
}

/// `reply` to `request` as a site writes it: its status, headers and body
/// (none for a HEAD), the connection closed after.
fn written(request: &Request, reply: &Reply) -> Vec<u8> {
    let mut head = format!("HTTP/1.1 {} Fake\r\n", reply.status);
    for (k, v) in &reply.headers {
        head += &format!("{k}: {v}\r\n");
    }
    head += &format!("Content-Length: {}\r\nConnection: close\r\n\r\n", reply.body.len());
    let mut out = head.into_bytes();
    if request.method != "HEAD" {
        out.extend_from_slice(&reply.body);
    }
    out
}
