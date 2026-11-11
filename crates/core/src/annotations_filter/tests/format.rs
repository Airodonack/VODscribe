//! Reading a filter file and a version: the set's header and bits, its keys, the producer's vectors, an ETag.

use super::*;

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

/// crates/core/tests/data/annotations-filter-vectors.json; annotations_filter.rs:1 ("`tests/data/annotations-filter-vectors.json`
/// holds keys, their bits and whole files, for every implementation to check itself by"). Each key's bits are
/// the vector's, each whole file reads and says yes to what it holds, and made here from the same keys it's
/// the same file byte for byte.
#[test]
fn the_vectors_are_the_producers_and_the_apps() {
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

/// annotations_filter.rs:27 (the file's table: `MAGIC` "VSAF", the version 1, m, k, n, then the m bits, bit j
/// being bit `j % 8` of byte `j / 8`, the least significant first): a file written out by that table is
/// read, and the set made here from its key is that file. n is "for the record; not read".
#[test]
fn the_filter_file_is_its_header_then_its_bits() {
    let key = "twitch:v2901646294";
    let (bits, hashes) = (64u64, 7u32);
    let mut written = Vec::new();
    written.extend_from_slice(b"VSAF");
    written.extend_from_slice(&1u32.to_le_bytes());
    written.extend_from_slice(&bits.to_le_bytes());
    written.extend_from_slice(&hashes.to_le_bytes());
    written.extend_from_slice(&1u64.to_le_bytes());
    let mut data = vec![0u8; (bits / 8) as usize];
    for j in positions(key, bits, hashes) {
        data[(j / 8) as usize] |= 1 << (j % 8);
    }
    written.extend_from_slice(&data);
    assert_eq!(written.len(), 28 + 8, "a header of 28 bytes, then m / 8");
    let filter = Filter::parse(&written).unwrap();
    assert!(filter.contains(key) && !filter.contains("twitch:v1"));
    assert_eq!(Filter::of(&[key]).to_bytes(), written, "a set of one key is 64 bits, the least there are");
    // n isn't read: a set that says another number holds the same keys.
    let mut other_n = written.clone();
    other_n[20..28].copy_from_slice(&999u64.to_le_bytes());
    assert!(Filter::parse(&other_n).unwrap().contains(key));
    // Sized for n keys: n * 9.586 bits up to a multiple of 8, at least 64, 7 hashes.
    for (keys, bits) in [(0u64, 64u64), (1, 64), (7, 72), (10, 96), (1000, 9592)] {
        let bytes = Filter::sized(keys).to_bytes();
        assert_eq!(u64::from_le_bytes(bytes[8..16].try_into().unwrap()), bits, "{keys} keys");
        assert_eq!(u32::from_le_bytes(bytes[16..20].try_into().unwrap()), 7);
        assert_eq!(bytes.len() as u64, 28 + bits / 8);
    }
}

/// annotations_filter.rs:10 ("Each published VOD puts two keys in it, UTF-8, with nothing around them":
/// `<platform>:<platform id>` and `<platform>:<platform id>:<version>`).
#[test]
fn each_published_vod_puts_its_two_keys_in_the_set() {
    let md5 = "0123456789abcdef0123456789abcdef";
    assert_eq!(exists_key("twitch", "v2901646294"), "twitch:v2901646294");
    assert_eq!(version_key("twitch", "v2901646294", md5), format!("twitch:v2901646294:{md5}"));
    let set = Filter::of(&[exists_key("youtube", "abcdefghijk"), version_key("youtube", "abcdefghijk", md5)]);
    assert!(set.contains("youtube:abcdefghijk"));
    assert!(set.contains(&format!("youtube:abcdefghijk:{md5}")));
    assert!(!set.contains("youtube:abcdefghijk:fedcba9876543210fedcba9876543210"), "another version");
    assert!(!set.contains("twitch:abcdefghijk"), "another site");
}

/// The vectors' large set (`large`): built here from its rule it's the file the producer made, and says
/// yes to one key in a hundred that isn't in it (annotations_filter.rs:67: "a 1% false-positive rate").
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

/// annotations_filter.rs (`MAGIC`, `FILTER_VERSION`, `MAX_HASHES`, `MAX_FALSE_POSITIVES`; the file's table):
/// a file that isn't a set the app can use is refused, saying which part is wrong.
#[test]
fn a_file_that_is_not_a_filter_is_refused() {
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
    let mut many = good.clone();
    many[16..20].copy_from_slice(&(MAX_HASHES + 1).to_le_bytes());
    let mut none = good.clone();
    none[8..16].copy_from_slice(&0u64.to_le_bytes());
    // No bits at all, with no bytes for them to match.
    let empty = good[..HEADER_BYTES].to_vec();
    let mut empty = empty;
    empty[8..16].copy_from_slice(&0u64.to_le_bytes());
    // 65 bits in 8 bytes: as many bytes as bits / 8, but not a multiple of 8.
    let mut ragged = good.clone();
    ragged[8..16].copy_from_slice(&65u64.to_le_bytes());
    let mut full = good.clone();
    full[HEADER_BYTES..].fill(0xff);
    // As many hashes as are allowed is read.
    let mut most = good.clone();
    most[16..20].copy_from_slice(&MAX_HASHES.to_le_bytes());
    Filter::parse(&most).unwrap();
    // A set that is wrong about 5% of the keys not in it (4 of 80 bits set, one hash) is read; 5 of 80 is not.
    let sparse = |set: usize| {
        let mut file = good[..HEADER_BYTES].to_vec();
        file[8..16].copy_from_slice(&80u64.to_le_bytes());
        file[16..20].copy_from_slice(&1u32.to_le_bytes());
        let mut data = vec![0u8; 10];
        data[..set].fill(1);
        file.extend(data);
        file
    };
    Filter::parse(&sparse(4)).unwrap();
    assert!(Filter::parse(&sparse(5)).unwrap_err().to_string().contains("says yes to about 6%"));
    for (bad, why) in [
        (&good[..10], "shorter than its header"),
        (&short[..], "don't match"),
        (&magic[..], "isn't an annotations filter"),
        (&version[..], "version 2"),
        (&hashes[..], "0 hashes"),
        (&odd[..], "63 bits"),
        (&many[..], "33 hashes"),
        (&none[..], "0 bits"),
        (&empty[..], "0 bits"),
        (&ragged[..], "65 bits"),
        (&full[..], "says yes to about 100%"),
    ] {
        let err = Filter::parse(bad).unwrap_err().to_string();
        assert!(err.contains(why), "{why}: {err}");
    }
}

/// annotations_filter.rs:16 ("The version is the file's ETag on the file host stripped to its MD5"): quotes,
/// `W/` and capitals go (`version_of_etag`).
#[test]
fn an_etag_is_the_files_md5() {
    assert_eq!(version_of_etag("\"3CD25F19\""), "3cd25f19");
    assert_eq!(version_of_etag(" W/\"3cd25f19\" "), "3cd25f19");
    assert_eq!(version_key("twitch", "v1", &version_of_etag("W/\"ab\"")), "twitch:v1:ab");
}

/// annotations_filter.rs:16 and `md5_shaped`: only an MD5 is a version; any other ETag could be one the
/// host made for this install, so the set is read and nothing is kept of it.
#[test]
fn an_etag_that_is_not_an_md5_is_not_kept() {
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
