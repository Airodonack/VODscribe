//! Which VODs have published annotations, without telling the file host
//! which VODs the library has: one small file, `annotations/filter.bin`
//! beside `annotations/index.json`, holds a probabilistic set (a Bloom
//! filter) of every published VOD, and the app tests its own VODs against it
//! here. Only a VOD the set says is probably there is asked about, with one
//! HEAD request (`annotations::look_up`), as when it's added.
//!
//! # The set
//!
//! Each published VOD puts two keys in it, UTF-8, with nothing around them:
//!
//! - `<platform>:<platform id>`: annotations are published for it. The
//!   platform is `twitch`, `youtube` or `kick`; the id is the platform's own,
//!   exactly as the library stores it and as the file's `source.id` has it
//!   (Twitch's with its leading "v": `twitch:v2901646294`).
//! - `<platform>:<platform id>:<version>`: they're published in that version.
//!   The version is the file's ETag on the file host stripped to its
//!   MD5: the lowercase hex MD5 of the file's bytes, which is what the file
//!   host (Cloudflare R2) gives as its ETag, `"<md5>"`, or `W/"<md5>"` when it
//!   compresses the answer (`version_of_etag`). The app keeps the ETag of the
//!   file it imported, so the producer of the set and the app agree without a
//!   version of their own.
//!
//! Both kinds live in one set: one file to fetch, one ETag to ask about, and
//! one false-positive rate over all its keys (two sets of the same total
//! size would gain nothing).
//!
//! # The file
//!
//! All numbers little-endian. A header of `HEADER_BYTES`:
//!
//! | bytes | what |
//! |-------|------|
//! | 0-3   | `MAGIC`, "VSAF" |
//! | 4-7   | the format's version, u32, `FILTER_VERSION` (1) |
//! | 8-15  | m, the number of bits, u64, a multiple of 8, at least 8 |
//! | 16-19 | k, the number of hashes per key, u32, 1 to `MAX_HASHES` |
//! | 20-27 | n, the number of keys put in, u64 (for the record; not read) |
//!
//! then the m bits, m / 8 bytes, nothing after: bit j is bit `j % 8` (the
//! least significant first) of byte `j / 8`.
//!
//! A set whose bits would say yes to more than `MAX_FALSE_POSITIVES` of the
//! keys not in it (the share of bits set, to the power k) is refused: broken
//! or made that way, it would have the app ask about most of the library.
//!
//! A key's k bits: d = SHA-256 of the key's UTF-8 bytes; h1 = d's bytes 0-7
//! and h2 = its bytes 8-15, each as a little-endian u64, h2 with its lowest
//! bit set (so it's never 0); bit i (from 0 to k - 1) is
//! `(h1 + i * h2) mod 2^64 mod m`. A key is in the set when all k bits are.
//!
//! A producer sizes the set for a 1% false-positive rate (`sized`): with n
//! keys, m = 9.586 n bits (in whole numbers: n * 9586 / 1000 rounded up,
//! then up to a multiple of 8, and at least 64), and k = 7.
//!
//! `tests/data/annotations-filter-vectors.json` (in crates/core) holds keys,
//! their bits and whole files, for every implementation to check itself by.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use rusqlite::params;
use sha2::{Digest, Sha256};

use crate::annotations::{self, Found, can_be_published};
use crate::db::{Availability, Db};
use crate::settings::Settings;

/// Where the set is published, under `annotations::BASE_URL`.
pub const FILTER_PATH: &str = "annotations/filter.bin";
pub const MAGIC: [u8; 4] = *b"VSAF";
pub const FILTER_VERSION: u32 = 1;
pub const HEADER_BYTES: usize = 28;
/// Bits per key, in thousandths, and hashes per key, for a 1% false-positive rate.
pub const BITS_PER_KEY_THOUSANDTHS: u64 = 9586;
pub const HASHES: u32 = 7;
/// The fewest bits a producer makes (an empty set has these, all clear).
pub const MIN_BITS: u64 = 64;
pub const MAX_HASHES: u32 = 32;
/// The largest set read: 16 MB holds well over ten million VODs.
pub const MAX_FILTER_BYTES: u64 = 16 * 1024 * 1024;
/// The most a set read may be wrong about a key not in it: five times what a
/// producer sizes it for (`sized`).
pub const MAX_FALSE_POSITIVES: f64 = 0.05;
/// The pause between two VODs' HEAD requests in one check, so a library
/// with many VODs newly published never asks in a burst.
pub const PAUSE: Duration = Duration::from_millis(500);

/// The key saying annotations are published for VOD `id` of `platform`.
pub fn exists_key(platform: &str, id: &str) -> String {
    format!("{platform}:{id}")
}

/// The key saying they're published in `version` (`version_of_etag`).
pub fn version_key(platform: &str, id: &str, version: &str) -> String {
    format!("{platform}:{id}:{version}")
}

/// A file's version as the set names it, from its ETag: without `W/` and
/// the quotes, in lowercase (`"3CD2…"` and `W/"3cd2…"` are both `3cd2…`).
pub fn version_of_etag(etag: &str) -> String {
    let etag = etag.trim();
    etag.strip_prefix("W/").unwrap_or(etag).trim_matches('"').to_ascii_lowercase()
}

/// Whether a version is the shape of an MD5, 32 hex digits: what the file
/// host gives as an ETag for a plain upload, the same to everyone.
pub fn md5_shaped(version: &str) -> bool {
    version.len() == 32 && version.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The set (see the module's comment).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Filter {
    bits: u64,
    hashes: u32,
    items: u64,
    data: Vec<u8>,
}

impl Filter {
    /// An empty set sized for `keys` keys at a 1% false-positive rate.
    pub fn sized(keys: u64) -> Filter {
        let bits = (keys * BITS_PER_KEY_THOUSANDTHS).div_ceil(1000).max(MIN_BITS).next_multiple_of(8);
        Filter { bits, hashes: HASHES, items: 0, data: vec![0; (bits / 8) as usize] }
    }

    /// A set of `keys`, sized for them.
    pub fn of<S: AsRef<str>>(keys: &[S]) -> Filter {
        let mut filter = Filter::sized(keys.len() as u64);
        for key in keys {
            filter.insert(key.as_ref());
        }
        filter
    }

    pub fn insert(&mut self, key: &str) {
        for bit in positions(key, self.bits, self.hashes) {
            self.data[(bit / 8) as usize] |= 1 << (bit % 8);
        }
        self.items += 1;
    }

    /// Whether `key` is probably in the set; never false for one that is.
    pub fn contains(&self, key: &str) -> bool {
        positions(key, self.bits, self.hashes).all(|bit| self.data[(bit / 8) as usize] & (1 << (bit % 8)) != 0)
    }

    /// Reads a set's file, checking its header against its length.
    pub fn parse(bytes: &[u8]) -> Result<Filter> {
        let Some((header, data)) = bytes.split_at_checked(HEADER_BYTES) else {
            bail!("the annotations filter is shorter than its header");
        };
        let u32_at = |at: usize| u32::from_le_bytes(header[at..at + 4].try_into().unwrap());
        let u64_at = |at: usize| u64::from_le_bytes(header[at..at + 8].try_into().unwrap());
        if header[..4] != MAGIC {
            bail!("this isn't an annotations filter");
        }
        if u32_at(4) != FILTER_VERSION {
            bail!("the annotations filter's version {} isn't {FILTER_VERSION}", u32_at(4));
        }
        let (bits, hashes, items) = (u64_at(8), u32_at(16), u64_at(20));
        if bits == 0 || bits % 8 != 0 || bits / 8 != data.len() as u64 {
            bail!("the annotations filter's {bits} bits don't match its {} bytes", data.len());
        }
        if !(1..=MAX_HASHES).contains(&hashes) {
            bail!("the annotations filter's {hashes} hashes aren't 1 to {MAX_HASHES}");
        }
        let set: u64 = data.iter().map(|b| u64::from(b.count_ones())).sum();
        let wrong = (set as f64 / bits as f64).powi(hashes as i32);
        if wrong > MAX_FALSE_POSITIVES {
            bail!("the annotations filter says yes to about {:.0}% of the VODs not in it", wrong * 100.0);
        }
        Ok(Filter { bits, hashes, items, data: data.to_vec() })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_BYTES + self.data.len());
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&FILTER_VERSION.to_le_bytes());
        out.extend_from_slice(&self.bits.to_le_bytes());
        out.extend_from_slice(&self.hashes.to_le_bytes());
        out.extend_from_slice(&self.items.to_le_bytes());
        out.extend_from_slice(&self.data);
        out
    }
}

/// `key`'s bits in a set of `bits` bits with `hashes` hashes (see the module's comment).
pub fn positions(key: &str, bits: u64, hashes: u32) -> impl Iterator<Item = u64> {
    let digest = Sha256::digest(key.as_bytes());
    let h1 = u64::from_le_bytes(digest[..8].try_into().unwrap());
    let h2 = u64::from_le_bytes(digest[8..16].try_into().unwrap()) | 1;
    (0..u64::from(hashes)).map(move |i| h1.wrapping_add(i.wrapping_mul(h2)) % bits)
}

/// What the file host said of the set, asked whether it changed.
enum Fetched {
    /// The same as the one kept (304).
    Unchanged,
    New {
        bytes: Vec<u8>,
        etag: Option<String>,
    },
    /// None published (404, 403, 410).
    Missing,
}

fn fetch(base: &str, etag: Option<&str>) -> Result<Fetched> {
    let mut asked = annotations::agent().get(format!("{base}/{FILTER_PATH}"));
    if let Some(etag) = etag {
        asked = asked.header("If-None-Match", etag);
    }
    let mut response = match asked.call() {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(403 | 404 | 410)) => return Ok(Fetched::Missing),
        Err(e) => return Err(e).context("ask the file host for the annotations filter"),
    };
    match response.status().as_u16() {
        304 => Ok(Fetched::Unchanged),
        200 => {
            // Kept only when it's the MD5 the host gives everyone (`version_of_etag`):
            // any other ETag could be one made for this install, to know it again.
            let etag = response
                .headers()
                .get("etag")
                .and_then(|v| v.to_str().ok())
                .filter(|e| md5_shaped(&version_of_etag(e)))
                .map(str::to_string);
            let bytes = response
                .body_mut()
                .with_config()
                .limit(MAX_FILTER_BYTES)
                .read_to_vec()
                .context("get the annotations filter")?;
            Ok(Fetched::New { bytes, etag })
        }
        status => bail!("the file host answered {status} for the annotations filter"),
    }
}

fn etag_path(cache: &Path) -> PathBuf {
    cache.with_extension("etag")
}

/// The set kept from the last check, and its ETag; None if there's none
/// that reads (then the next fetch asks for it whole).
fn load_kept(cache: &Path) -> Option<(Filter, String)> {
    let filter = Filter::parse(&std::fs::read(cache).ok()?).ok()?;
    let etag = std::fs::read_to_string(etag_path(cache)).ok()?;
    let etag = etag.trim();
    (!etag.is_empty()).then(|| (filter, etag.to_string()))
}

fn keep(cache: &Path, bytes: &[u8], etag: Option<&str>) -> Result<()> {
    if let Some(dir) = cache.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let _ = std::fs::remove_file(etag_path(cache));
    let part = cache.with_extension("part");
    std::fs::write(&part, bytes)?;
    std::fs::rename(&part, cache)?;
    if let Some(etag) = etag {
        std::fs::write(etag_path(cache), etag)?;
    }
    Ok(())
}

/// What the library knows of a VOD's published annotations, to test it against the set.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub vod_id: i64,
    pub platform: String,
    pub platform_id: String,
    pub available: Availability,
    /// The published file's version as last seen, and the one last imported (ETags).
    pub seen: Option<String>,
    pub imported: Option<String>,
}

impl Db {
    /// The library's VODs, as `Candidate`s.
    pub fn annotations_candidates(&self) -> Result<Vec<Candidate>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, platform, platform_id, annotations, annotations_version, annotations_imported FROM vods
             WHERE EXISTS (SELECT 1 FROM clips WHERE clips.whole_of = vods.id) ORDER BY id",
        )?;
        let rows = stmt.query_map(params![], |r| {
            Ok(Candidate {
                vod_id: r.get(0)?,
                platform: r.get(1)?,
                platform_id: r.get(2)?,
                available: Availability::parse(r.get::<_, Option<String>>(3)?.as_deref()),
                seen: r.get(4)?,
                imported: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

/// Whether the file host is asked about `vod`, given the set:
///
/// - not in the set: certainly nothing published; not asked.
/// - in it, never imported from the file host: asked, unless it's known to
///   be published already (its gift is showing).
/// - in it, imported in version V: asked only when the set hasn't V (a newer
///   one, probably), and it isn't known to have a newer one already.
pub fn worth_asking(filter: &Filter, vod: &Candidate) -> bool {
    if !can_be_published(&vod.platform) || !filter.contains(&exists_key(&vod.platform, &vod.platform_id)) {
        return false;
    }
    match &vod.imported {
        None => vod.available != Availability::Available,
        Some(imported) => {
            vod.seen.as_ref() == Some(imported)
                && !filter.contains(&version_key(&vod.platform, &vod.platform_id, &version_of_etag(imported)))
        }
    }
}

/// What a check of the library found.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Checked {
    /// False when nothing was asked: the user turned asking off (Settings).
    pub asked: bool,
    /// HEAD requests made, one per VOD the set had.
    pub confirmed: usize,
    /// VODs with annotations newly found published, never imported.
    pub found: Vec<i64>,
    /// VODs whose imported annotations have a newer version published.
    pub newer: Vec<i64>,
}

/// One check at a time: a second waits, then finds the first's answers kept.
static CHECKING: Mutex<()> = Mutex::new(());

/// Checks every VOD in the library against the published set, fetching it
/// again if it changed, and asks the file host about each it has (see
/// `worth_asking`), keeping each answer on its VOD and calling `changed`
/// with the VOD's id. Nothing at all when the user turned asking off. A
/// failure to reach the file host is an error; what was answered before it is kept.
pub fn check_library(db: &Mutex<Db>, paths: &crate::Paths, changed: impl FnMut(i64)) -> Result<Checked> {
    check_at(&annotations::base_url(), db, &paths.settings(), &paths.annotations_filter(), PAUSE, changed)
}

fn may_ask(settings: &Path) -> bool {
    Settings::load(settings).ask_for_annotations
}

fn check_at(
    base: &str,
    db: &Mutex<Db>,
    settings: &Path,
    cache: &Path,
    pause: Duration,
    mut changed: impl FnMut(i64),
) -> Result<Checked> {
    let mut checked = Checked::default();
    if !may_ask(settings) {
        return Ok(checked);
    }
    let _one = CHECKING.lock().unwrap_or_else(|e| e.into_inner());
    checked.asked = true;
    let kept = load_kept(cache);
    let filter = match fetch(base, kept.as_ref().map(|(_, etag)| etag.as_str()))? {
        Fetched::Unchanged => match kept {
            Some((filter, _)) => filter,
            None => bail!("the file host said the annotations filter is unchanged, and none is kept"),
        },
        Fetched::New { bytes, etag } => {
            let filter = Filter::parse(&bytes)?;
            // Kept or not, this check goes on with it; unkept, the next fetches it whole.
            let _ = keep(cache, &bytes, etag.as_deref());
            filter
        }
        Fetched::Missing => {
            let _ = std::fs::remove_file(cache);
            let _ = std::fs::remove_file(etag_path(cache));
            return Ok(checked);
        }
    };
    let candidates = db.lock().unwrap().annotations_candidates()?;
    for vod in candidates.iter().filter(|vod| worth_asking(&filter, vod)) {
        if checked.confirmed > 0 {
            std::thread::sleep(pause);
        }
        // Turned off while the check went on (the pause included): nothing more is asked.
        if !may_ask(settings) {
            break;
        }
        // Taken out of the library meanwhile: not asked about.
        if !db.lock().unwrap().vod(vod.vod_id)?.is_some_and(|v| v.listed) {
            continue;
        }
        checked.confirmed += 1;
        let found = annotations::look_up_at(base, &vod.platform, &vod.platform_id)?;
        db.lock().unwrap().set_annotations_found(vod.vod_id, &found)?;
        changed(vod.vod_id);
        if let Found::Available { version } = found {
            match &vod.imported {
                None => checked.found.push(vod.vod_id),
                Some(imported)
                    if version.as_deref().is_some_and(|v| version_of_etag(v) != version_of_etag(imported)) =>
                {
                    checked.newer.push(vod.vod_id)
                }
                Some(_) => {}
            }
        }
    }
    Ok(checked)
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::annotations::tests::file;

    fn vectors() -> serde_json::Value {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/annotations-filter-vectors.json");
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    /// The keys of the vectors' large set, by its rule.
    fn ruled(rule: &serde_json::Value, prefix: &str, count: &str) -> Vec<String> {
        let (prefix, n) = (rule[prefix].as_str().unwrap(), rule[count].as_u64().unwrap());
        (0..n).map(|i| format!("{prefix}{i}")).collect()
    }

    #[test]
    fn the_vectors_bits_and_files_are_these() {
        let v = vectors();
        for case in v["keys"].as_array().unwrap() {
            let key = case["key"].as_str().unwrap();
            let (bits, hashes) = (case["bits"].as_u64().unwrap(), case["hashes"].as_u64().unwrap() as u32);
            let got: Vec<u64> = positions(key, bits, hashes).collect();
            let want: Vec<u64> = case["positions"].as_array().unwrap().iter().map(|p| p.as_u64().unwrap()).collect();
            assert_eq!(got, want, "{key}");
        }
        for case in v["filters"].as_array().unwrap() {
            let name = case["name"].as_str().unwrap();
            let bytes = unhex(case["file"].as_str().unwrap());
            let filter = Filter::parse(&bytes).unwrap();
            let present: Vec<&str> = case["present"].as_array().unwrap().iter().map(|k| k.as_str().unwrap()).collect();
            for key in &present {
                assert!(filter.contains(key), "{name}: {key}");
            }
            for key in case["absent"].as_array().unwrap() {
                assert!(!filter.contains(key.as_str().unwrap()), "{name}: {key}");
            }
            // Made here from the same keys, it's the same file, byte for byte.
            assert_eq!(Filter::of(&present).to_bytes(), bytes, "{name}");
        }
    }

    #[test]
    fn the_large_set_is_the_same_everywhere_and_wrong_about_one_in_a_hundred() {
        let rule = &vectors()["large"];
        let present = ruled(rule, "present_prefix", "present_count");
        let filter = Filter::of(&present);
        let digest = crate::net::hex(&Sha256::digest(filter.to_bytes()));
        assert_eq!(digest, rule["file_sha256"].as_str().unwrap());
        assert!(present.iter().all(|k| filter.contains(k)));
        let absent = ruled(rule, "absent_prefix", "absent_count");
        let wrong = absent.iter().filter(|k| filter.contains(k)).count();
        let rate = wrong as f64 / absent.len() as f64;
        assert!((0.005..=0.015).contains(&rate), "false positives: {wrong} of {}", absent.len());
        assert_eq!(wrong as u64, rule["absent_false_positives"].as_u64().unwrap());
    }

    #[test]
    fn a_file_that_isnt_a_filter_is_refused() {
        let good = Filter::of(&["twitch:v1"]).to_bytes();
        assert_eq!(Filter::parse(&good).unwrap().to_bytes(), good);
        let mut short = good.clone();
        short.pop();
        let mut magic = good.clone();
        magic[0] = b'X';
        let mut version = good.clone();
        version[4] = 2;
        let mut hashes = good.clone();
        hashes[16..20].copy_from_slice(&0u32.to_le_bytes());
        let mut odd = good.clone();
        odd[8..16].copy_from_slice(&63u64.to_le_bytes());
        for (bad, why) in [
            (&good[..10], "shorter than its header"),
            (&short[..], "don't match"),
            (&magic[..], "isn't an annotations filter"),
            (&version[..], "version 2"),
            (&hashes[..], "0 hashes"),
            (&odd[..], "63 bits"),
        ] {
            let err = Filter::parse(bad).unwrap_err().to_string();
            assert!(err.contains(why), "{why}: {err}");
        }
    }

    #[test]
    fn an_etag_is_the_files_md5() {
        assert_eq!(version_of_etag("\"3CD25F19\""), "3cd25f19");
        assert_eq!(version_of_etag(" W/\"3cd25f19\" "), "3cd25f19");
        assert_eq!(version_key("twitch", "v1", &version_of_etag("W/\"ab\"")), "twitch:v1:ab");
    }

    #[test]
    fn an_etag_that_isnt_an_md5_isnt_kept() {
        // Any other ETag could be one the host made for this install, to know
        // it again at the next start: the set is read, nothing is asked about
        // it next time (no If-None-Match), and nothing names the install.
        assert!(md5_shaped("0123456789abcdef0123456789abcdef"));
        assert!(!md5_shaped("set1"));
        assert!(!md5_shaped("0123456789abcdef0123456789abcde"));
        let set = Filter::of(&["twitch:v1"]).to_bytes();
        let (base, asked) =
            host(Arc::new(move |_req: &str| ("HTTP/1.1 200 OK\r\nETag: \"set1\"".to_string(), set.clone())));
        match fetch(&base, None).unwrap() {
            Fetched::New { etag, .. } => assert_eq!(etag, None),
            _ => panic!("the set wasn't read as new"),
        }
        assert_eq!(asked.lock().unwrap().len(), 1);
    }

    /// A file host on this computer: `answer(request)` (its whole head) gives
    /// the status line and headers, and the body. Returns its address and the
    /// requests' first lines, with their If-None-Match if any.
    type Answer = dyn Fn(&str) -> (String, Vec<u8>) + Send + Sync;
    fn host(answer: Arc<Answer>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let asked = Arc::new(Mutex::new(Vec::new()));
        let seen = asked.clone();
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(mut conn) = conn else { return };
                let mut req = [0u8; 4096];
                let n = conn.read(&mut req).unwrap_or(0);
                let req = String::from_utf8_lossy(&req[..n]).to_string();
                let line = req.lines().next().unwrap_or("").to_string();
                let since = req.lines().find_map(|l| {
                    l.split_once(':').filter(|(k, _)| k.eq_ignore_ascii_case("if-none-match")).map(|(_, v)| v.trim())
                });
                seen.lock().unwrap().push(match since {
                    Some(etag) => format!("{line} since {etag}"),
                    None => line.clone(),
                });
                let (head, body) = answer(&req);
                let body = if line.starts_with("HEAD") { Vec::new() } else { body };
                let _ = write!(conn, "{head}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = conn.write_all(&body);
            }
        });
        (format!("http://{addr}"), asked)
    }

    struct Library {
        db: Mutex<Db>,
        dir: crate::TempDir,
    }

    impl Library {
        fn new() -> Library {
            Library {
                db: Mutex::new(Db::open(Path::new(":memory:")).unwrap()),
                dir: crate::TempDir::new("annotations-filter"),
            }
        }

        fn settings(&self) -> PathBuf {
            self.dir.join("settings.json")
        }

        fn cache(&self) -> PathBuf {
            self.dir.join("annotations-filter.bin")
        }

        fn ask(&self, on: bool) {
            let settings = Settings { ask_for_annotations: on, ..Settings::default() };
            settings.save(&self.settings()).unwrap();
        }

        /// Adds YouTube VOD `id`, imported in `imported` (an ETag) if given.
        fn add(&self, id: &str, imported: Option<&str>) -> i64 {
            let db = self.db.lock().unwrap();
            let (vod, _) = db.add_vod(&crate::edits::tests::info(id)).unwrap();
            if let Some(etag) = imported {
                db.import_annotations(&file("youtube", id), None).unwrap();
                db.set_annotations_imported(vod.id, Some(etag)).unwrap();
            }
            vod.id
        }

        fn check(&self, base: &str) -> (Result<Checked>, Vec<i64>) {
            let mut changed = Vec::new();
            let checked =
                check_at(base, &self.db, &self.settings(), &self.cache(), Duration::ZERO, |id| changed.push(id));
            (checked, changed)
        }
    }

    const MD5_A: &str = "0123456789abcdef0123456789abcdef";
    const MD5_B: &str = "fedcba9876543210fedcba9876543210";

    /// The published set: `there` has its file in version MD5_A, `newer` in MD5_B.
    fn published() -> Vec<u8> {
        Filter::of(&[
            exists_key("youtube", "there"),
            version_key("youtube", "there", MD5_A),
            exists_key("youtube", "same"),
            version_key("youtube", "same", MD5_A),
            exists_key("youtube", "newer"),
            version_key("youtube", "newer", MD5_B),
        ])
        .to_bytes()
    }

    fn file_host() -> (String, Arc<Mutex<Vec<String>>>) {
        let set = published();
        host(Arc::new(move |req: &str| {
            let line = req.lines().next().unwrap_or("");
            if line.contains("/annotations/filter.bin") {
                if req.to_ascii_lowercase().contains("if-none-match: \"0123456789abcdef0123456789abcdef\"") {
                    return (
                        "HTTP/1.1 304 Not Modified\r\nETag: \"0123456789abcdef0123456789abcdef\"".into(),
                        Vec::new(),
                    );
                }
                return ("HTTP/1.1 200 OK\r\nETag: \"0123456789abcdef0123456789abcdef\"".into(), set.clone());
            }
            let etag = if line.contains("/newer.json") { MD5_B } else { MD5_A };
            if ["/there.json", "/same.json", "/newer.json"].iter().any(|f| line.contains(f)) {
                return (format!("HTTP/1.1 200 OK\r\nETag: W/\"{etag}\""), Vec::new());
            }
            ("HTTP/1.1 404 Not Found".into(), Vec::new())
        }))
    }

    /// The decision table, with the requests counted: absent, there and never
    /// imported, imported in the version published, imported in an older one.
    #[test]
    fn only_the_vods_the_set_has_are_asked_about_once() {
        let lib = Library::new();
        lib.ask(true);
        let absent = lib.add("absent", None);
        let there = lib.add("there", None);
        let same = lib.add("same", Some(&format!("W/\"{MD5_A}\"")));
        let newer = lib.add("newer", Some(&format!("\"{MD5_A}\"")));
        let (base, asked) = file_host();

        let (checked, changed) = lib.check(&base);
        let checked = checked.unwrap();
        assert_eq!(
            *asked.lock().unwrap(),
            [
                "GET /annotations/filter.bin HTTP/1.1",
                "HEAD /annotations/youtube/there.json HTTP/1.1",
                "HEAD /annotations/youtube/newer.json HTTP/1.1",
            ]
        );
        assert_eq!(checked, Checked { asked: true, confirmed: 2, found: vec![there], newer: vec![newer] });
        assert_eq!(changed, [there, newer]);
        let db = lib.db.lock().unwrap();
        assert_eq!(db.vod(absent).unwrap().unwrap().annotations, Availability::Unknown);
        assert_eq!(db.vod(there).unwrap().unwrap().annotations, Availability::Available);
        assert!(db.vod(newer).unwrap().unwrap().annotations_newer);
        assert!(db.annotations_offer(newer).unwrap().newer);
        assert!(!db.vod(same).unwrap().unwrap().annotations_newer);
        assert!(db.annotations_offer(same).unwrap().up_to_date);
        drop(db);

        // Checked again (the next start, or the library's "Check for annotations"):
        // the set is asked for only if it changed, and what's known isn't asked again.
        asked.lock().unwrap().clear();
        let (checked, _) = lib.check(&base);
        assert_eq!(checked.unwrap(), Checked { asked: true, ..Checked::default() });
        assert_eq!(
            *asked.lock().unwrap(),
            ["GET /annotations/filter.bin HTTP/1.1 since \"0123456789abcdef0123456789abcdef\""]
        );
    }

    /// A set that says "probably" when the file host says no, or says the same
    /// version: the answer is kept, nothing is offered.
    #[test]
    fn a_false_positive_costs_one_request_and_offers_nothing() {
        let lib = Library::new();
        lib.ask(true);
        let set = Filter::of(&[exists_key("youtube", "gone"), exists_key("youtube", "same")]).to_bytes();
        let (base, asked) = host(Arc::new(move |req: &str| {
            let line = req.lines().next().unwrap_or("");
            match line {
                l if l.contains("filter.bin") => ("HTTP/1.1 200 OK\r\nETag: \"s\"".into(), set.clone()),
                l if l.contains("/same.json") => (format!("HTTP/1.1 200 OK\r\nETag: \"{MD5_A}\""), Vec::new()),
                _ => ("HTTP/1.1 404 Not Found".into(), Vec::new()),
            }
        }));
        let gone = lib.add("gone", None);
        let same = lib.add("same", Some(&format!("\"{MD5_A}\"")));
        let (checked, _) = lib.check(&base);
        assert_eq!(checked.unwrap(), Checked { asked: true, confirmed: 2, found: vec![], newer: vec![] });
        assert_eq!(asked.lock().unwrap().len(), 3);
        let db = lib.db.lock().unwrap();
        assert_eq!(db.vod(gone).unwrap().unwrap().annotations, Availability::None);
        assert!(db.annotations_offer(same).unwrap().up_to_date);
    }

    #[test]
    fn with_asking_turned_off_nothing_is_fetched() {
        let lib = Library::new();
        lib.ask(false);
        lib.add("there", None);
        let (base, asked) = file_host();
        let (checked, changed) = lib.check(&base);
        assert_eq!(checked.unwrap(), Checked::default());
        assert!(asked.lock().unwrap().is_empty() && changed.is_empty());
    }

    /// Every check asks for the set (startup's, and the library's "Check for
    /// annotations"): no day has to pass between two.
    #[test]
    fn every_check_asks_for_the_set_and_a_new_one_is_used() {
        let lib = Library::new();
        lib.ask(true);
        let there = lib.add("there", None);
        let empty = Filter::sized(0).to_bytes();
        let full = published();
        let round = Arc::new(Mutex::new(0));
        let now = round.clone();
        let (base, asked) = host(Arc::new(move |req: &str| {
            let line = req.lines().next().unwrap_or("");
            if line.contains("filter.bin") {
                // First none of them; then the set with "there" in it, published since.
                let set = if *now.lock().unwrap() == 0 { &empty } else { &full };
                let etag = *now.lock().unwrap();
                // MD5-shaped, as the host's are: any other ETag isn't kept.
                return (format!("HTTP/1.1 200 OK\r\nETag: \"{etag:0>32}\""), set.clone());
            }
            (format!("HTTP/1.1 200 OK\r\nETag: \"{MD5_A}\""), Vec::new())
        }));
        let (checked, _) = lib.check(&base);
        assert_eq!(checked.unwrap().confirmed, 0);
        *round.lock().unwrap() = 1;
        let (checked, _) = lib.check(&base);
        assert_eq!(checked.unwrap().found, [there]);
        assert_eq!(
            *asked.lock().unwrap(),
            [
                "GET /annotations/filter.bin HTTP/1.1",
                "GET /annotations/filter.bin HTTP/1.1 since \"00000000000000000000000000000000\"",
                "HEAD /annotations/youtube/there.json HTTP/1.1",
            ]
        );
    }

    #[test]
    fn offline_is_an_error_that_changes_nothing() {
        let lib = Library::new();
        lib.ask(true);
        let there = lib.add("there", None);
        let gone = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
        let (checked, changed) = lib.check(&format!("http://{gone}"));
        assert!(checked.is_err());
        assert!(changed.is_empty());
        assert_eq!(lib.db.lock().unwrap().vod(there).unwrap().unwrap().annotations, Availability::Unknown);
    }

    /// No set published (yet): nothing asked, and a set kept from before is dropped.
    #[test]
    fn no_set_published_asks_nothing_more() {
        let lib = Library::new();
        lib.ask(true);
        lib.add("there", None);
        keep(&lib.cache(), &published(), Some("\"old\"")).unwrap();
        let (base, asked) = host(Arc::new(|_: &str| ("HTTP/1.1 404 Not Found".into(), Vec::new())));
        let (checked, _) = lib.check(&base);
        assert_eq!(checked.unwrap(), Checked { asked: true, ..Checked::default() });
        assert_eq!(asked.lock().unwrap().len(), 1);
        assert!(!lib.cache().exists());
    }

    /// A set that says yes to (nearly) every key: whether broken or made to,
    /// it would have the app ask about every VOD in the library, which is
    /// what the set is there to keep from the file host. It's refused.
    #[test]
    fn a_set_that_says_yes_to_everything_is_refused() {
        let lib = Library::new();
        lib.ask(true);
        for id in ["a", "b", "c"] {
            lib.add(id, None);
        }
        let mut full = Filter::sized(0);
        full.data.fill(0xff);
        let set = full.to_bytes();
        let (base, asked) = host(Arc::new(move |req: &str| {
            if req.starts_with("GET") {
                return ("HTTP/1.1 200 OK\r\nETag: \"all\"".into(), set.clone());
            }
            ("HTTP/1.1 404 Not Found".into(), Vec::new())
        }));
        let (checked, _) = lib.check(&base);
        assert_eq!(*asked.lock().unwrap(), ["GET /annotations/filter.bin HTTP/1.1"]);
        assert!(checked.unwrap_err().to_string().contains("says yes to"));
        assert!(!lib.cache().exists(), "a refused set isn't kept");
        // Half its bits set with one hash: one VOD in two would be asked about.
        let mut half = Filter::sized(1000);
        half.hashes = 1;
        half.data.iter_mut().step_by(2).for_each(|b| *b = 0xff);
        assert!(Filter::parse(&half.to_bytes()).is_err());
        // A set sized for its keys, full to its 1%, is read.
        let keys: Vec<String> = (0..10_000).map(|i| format!("twitch:v{i}")).collect();
        assert!(Filter::parse(&Filter::of(&keys).to_bytes()).is_ok());
    }

    /// Asking turned off during the pause between two VODs' requests: the
    /// second isn't asked about. (The switch is turned off a while after the
    /// first request, well inside the pause, so the order is certain.)
    #[test]
    fn asking_turned_off_during_the_pause_asks_nothing_more() {
        let lib = Library::new();
        lib.ask(true);
        lib.add("there", None);
        lib.add("newer", Some(&format!("\"{MD5_A}\"")));
        let (base, asked) = file_host();
        let settings = lib.settings();
        let checked = check_at(&base, &lib.db, &lib.settings(), &lib.cache(), Duration::from_millis(600), |_| {
            let settings = settings.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(200));
                Settings { ask_for_annotations: false, ..Settings::default() }.save(&settings).unwrap();
            });
        });
        assert_eq!(checked.unwrap().confirmed, 1);
        assert_eq!(
            *asked.lock().unwrap(),
            ["GET /annotations/filter.bin HTTP/1.1", "HEAD /annotations/youtube/there.json HTTP/1.1"]
        );
    }

    /// A VOD taken out of the library while a check goes on (it waits
    /// between two requests) isn't asked about after.
    #[test]
    fn a_vod_removed_during_a_check_isnt_asked_about() {
        let lib = Library::new();
        lib.ask(true);
        let first = lib.add("there", None);
        let second = lib.add("same", None);
        let (base, asked) = file_host();
        let mut changed = Vec::new();
        let checked = check_at(&base, &lib.db, &lib.settings(), &lib.cache(), Duration::ZERO, |id| {
            changed.push(id);
            if id == first {
                lib.db.lock().unwrap().remove_entry(second).unwrap();
            }
        });
        assert_eq!(checked.unwrap().found, [first]);
        assert_eq!(
            *asked.lock().unwrap(),
            ["GET /annotations/filter.bin HTTP/1.1", "HEAD /annotations/youtube/there.json HTTP/1.1"]
        );
        assert_eq!(changed, [first]);
    }

    /// The set's VODs are the library's listed ones, of the sites that publish.
    #[test]
    fn a_vod_of_another_site_is_never_asked_about() {
        let filter = Filter::of(&["dailymotion:x1", "youtube:x1"]);
        let vod = |platform: &str| Candidate {
            vod_id: 1,
            platform: platform.into(),
            platform_id: "x1".into(),
            available: Availability::Unknown,
            seen: None,
            imported: None,
        };
        assert!(!worth_asking(&filter, &vod("dailymotion")));
        assert!(worth_asking(&filter, &vod("youtube")));
    }
}
