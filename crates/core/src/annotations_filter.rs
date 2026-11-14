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

/// Whether two ETags name the same version (`version_of_etag`): the file
/// host gives `W/"<md5>"` when it compresses its answer and `"<md5>"` when
/// it doesn't, for the same file.
pub fn same_version(a: &str, b: &str) -> bool {
    version_of_etag(a) == version_of_etag(b)
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

/// What a check does about a VOD, given the set (`judge`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Judged {
    /// Nothing to ask and nothing to count: certainly nothing published, or
    /// the version imported is the one the set has.
    Skip,
    /// The file host is asked about it (one HEAD).
    Ask,
    /// Already known to be published and never imported (its gift is
    /// showing): counted as found, not asked again.
    Found,
    /// Already known to have a newer version than the one imported (it's
    /// marked): counted as newer, not asked again.
    Newer,
}

/// What a check does about `vod`, given the set:
///
/// - not in the set: certainly nothing published; skipped.
/// - in it, never imported from the file host: asked, unless it's known to
///   be published already (its gift is showing): then found, unasked.
/// - in it, imported in version V and last seen in another: newer, unasked
///   (it's marked already).
/// - in it, imported in version V and last seen in V, or in none since (the
///   file host said none was published): asked only when the set hasn't V
///   (a newer one, probably).
pub fn judge(filter: &Filter, vod: &Candidate) -> Judged {
    if !can_be_published(&vod.platform) || !filter.contains(&exists_key(&vod.platform, &vod.platform_id)) {
        return Judged::Skip;
    }
    match &vod.imported {
        None if vod.available == Availability::Available => Judged::Found,
        None => Judged::Ask,
        Some(imported) => match vod.seen.as_deref() {
            Some(seen) if !same_version(seen, imported) => Judged::Newer,
            // Seen in V, or in none since (the file host said none was published,
            // and the set has it again): asked when the set hasn't V.
            _ if !filter.contains(&version_key(&vod.platform, &vod.platform_id, &version_of_etag(imported))) => {
                Judged::Ask
            }
            _ => Judged::Skip,
        },
    }
}

/// What a check of the library found.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Checked {
    /// False when nothing was asked: the user turned asking off (Settings).
    pub asked: bool,
    /// HEAD requests made, one per VOD the set had that wasn't known already.
    pub confirmed: usize,
    /// VODs with annotations published and never imported: found by this
    /// check, or known from an earlier answer (their gift is showing).
    pub found: Vec<i64>,
    /// VODs whose imported annotations have a newer version published: found
    /// by this check, or known from an earlier answer (they're marked).
    pub newer: Vec<i64>,
}

/// One check at a time: a second waits, then finds the first's answers kept.
static CHECKING: Mutex<()> = Mutex::new(());

/// Checks every VOD in the library against the published set, fetching it
/// again if it changed, and asks the file host about each it has whose
/// answer isn't known already (see `judge`), keeping each answer on its VOD
/// and calling `changed` with the VOD's id; the ones known are counted from
/// what was kept. Nothing at all when the user turned asking off. A
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
    #[cfg(any(test, feature = "testkit"))]
    crate::testkit::tools::held_at(crate::testkit::tools::At::CheckTurn, settings);
    let _one = CHECKING.lock().unwrap_or_else(|e| e.into_inner());
    // Turned off while it waited for another check: nothing is asked, not even the set.
    if !may_ask(settings) {
        return Ok(checked);
    }
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
    for vod in &candidates {
        match judge(&filter, vod) {
            Judged::Skip => continue,
            Judged::Found => {
                checked.found.push(vod.vod_id);
                continue;
            }
            Judged::Newer => {
                checked.newer.push(vod.vod_id);
                continue;
            }
            Judged::Ask => {}
        }
        if checked.confirmed > 0 {
            std::thread::sleep(pause);
            #[cfg(any(test, feature = "testkit"))]
            crate::testkit::tools::held_at(crate::testkit::tools::At::CheckPause, settings);
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
                Some(imported) if version.as_deref().is_some_and(|v| !same_version(v, imported)) => {
                    checked.newer.push(vod.vod_id)
                }
                Some(_) => {}
            }
        }
    }
    Ok(checked)
}

#[cfg(test)]
mod tests;
