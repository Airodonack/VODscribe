//! The specs of `download_resumable`: a download carries on from what it
//! has, and nothing reaches its file unchecked.

use super::*;
use crate::testkit::{Gate, Net};

fn never() -> AtomicBool {
    AtomicBool::new(false)
}

fn file() -> Vec<u8> {
    (0..10_000u32).map(|n| (n * 7 % 251) as u8).collect()
}

fn sha(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// Stopped partway (a quit, the network gone), the next try asks for the
/// rest from the byte it had (`Range: bytes=<n>-`), and the file it ends
/// with is the whole one, checked whole (`download_resumable`: "the bytes
/// there are hashed, the rest asked for from where they end").
#[test]
fn a_download_stopped_partway_carries_on_from_the_bytes_it_had() {
    let dir = crate::TempDir::new("resume-partway");
    let (net, whole, gate) = (Net::new(), file(), Gate::new());
    let url = "https://resume-partway.example/file";
    let asked = net.records(url);
    net.serves(url, whole.clone()).holds_partway(url, 4_000, &gate);
    let dest = dir.join("file");
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    let stop = cancel.clone();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(0u64));
    let mark = seen.clone();
    let first = std::thread::spawn({
        let dest = dest.clone();
        move || download_resumable(url, &dest, 10_000, &sha(&file()), &stop, |n| *mark.lock().unwrap() = n)
    });
    gate.reached();
    crate::testkit::eventually("its first 4000 bytes in", || *seen.lock().unwrap() == 4_000);
    cancel.store(true, Ordering::Relaxed);
    assert!(crate::is_canceled(&first.join().unwrap().unwrap_err()));
    assert!(!dest.exists(), "unchecked, at its place");
    assert_eq!(fs::metadata(part_of(&dest)).unwrap().len(), 4_000, "what it had, kept");
    net.let_go();
    let mut last = 0;
    download_resumable(url, &dest, 10_000, &sha(&whole), &never(), |n| last = n).unwrap();
    assert_eq!((fs::read(&dest).unwrap(), last), (whole, 10_000));
    let ranges: Vec<Option<String>> = asked.requests().iter().map(|r| r.header("Range").map(String::from)).collect();
    assert_eq!(ranges, [None, Some("bytes=4000-".into())]);
    assert!(!part_of(&dest).exists());
}

/// A part file that's been damaged (junk where its bytes were) doesn't
/// match once whole: it's deleted and downloaded once more from nothing,
/// and only that one reaches its place (`download_resumable`: "On a
/// mismatch … the file is deleted and downloaded once more from nothing").
#[test]
fn a_damaged_part_is_downloaded_again_from_nothing() {
    let dir = crate::TempDir::new("resume-damaged");
    let (net, file) = (Net::new(), file());
    let url = "https://resume-damaged.example/file";
    let asked = net.records(url);
    net.serves(url, file.clone());
    let dest = dir.join("file");
    fs::write(part_of(&dest), vec![0xee; 3_000]).unwrap();
    download_resumable(url, &dest, 10_000, &sha(&file), &never(), |_| {}).unwrap();
    assert_eq!(fs::read(&dest).unwrap(), file);
    let ranges: Vec<Option<String>> = asked.requests().iter().map(|r| r.header("Range").map(String::from)).collect();
    assert_eq!(ranges, [Some("bytes=3000-".into()), None]);
}

/// A file that doesn't match even from nothing fails as a download always
/// has, leaving nothing behind (`download_verified`'s rule).
#[test]
fn a_file_that_never_matches_fails_and_leaves_nothing() {
    let dir = crate::TempDir::new("resume-never");
    let net = Net::new();
    let url = "https://resume-never.example/file";
    net.serves(url, file());
    let dest = dir.join("file");
    fs::write(part_of(&dest), &file()[..2_000]).unwrap();
    let err = download_resumable(url, &dest, 10_000, &sha(b"something else"), &never(), |_| {}).unwrap_err();
    assert!(err.to_string().contains("damaged or changed"), "{err:#}");
    assert!(!dest.exists() && !part_of(&dest).exists(), "a mismatch leaves nothing behind");
}

/// A server that answers a range with the whole file (200) has the file
/// written from its start (`download_resumable`: "A server that sends the
/// whole file again has it written from the start").
#[test]
fn a_server_without_ranges_has_the_file_start_over() {
    let dir = crate::TempDir::new("resume-whole");
    let (net, file) = (Net::new(), file());
    let url = "https://resume-whole.example/file";
    net.serves_whole(url, file.clone());
    let dest = dir.join("file");
    fs::write(part_of(&dest), &file[..6_000]).unwrap();
    download_resumable(url, &dest, 10_000, &sha(&file), &never(), |_| {}).unwrap();
    assert_eq!(fs::read(&dest).unwrap(), file);
}

/// A file already at its place is checked again whole, as a part would be,
/// and asks for nothing when it matches.
#[test]
fn a_file_at_its_place_is_checked_again_and_kept() {
    let dir = crate::TempDir::new("resume-there");
    let (net, file) = (Net::new(), file());
    let url = "https://resume-there.example/file";
    let asked = net.records(url);
    net.serves(url, file.clone());
    let dest = dir.join("file");
    fs::write(&dest, &file).unwrap();
    download_resumable(url, &dest, 10_000, &sha(&file), &never(), |_| {}).unwrap();
    assert_eq!(fs::read(&dest).unwrap(), file);
    assert!(asked.requests().is_empty(), "downloaded again");
}

/// A 206 that doesn't start where it was asked to (its `Content-Range`
/// from 0) isn't appended to what the part has: the whole file is asked
/// for again, and progress never passes the file's size
/// (`download_resumable`: "A 206 whose `Content-Range` doesn't start there
/// isn't used").
#[test]
fn a_206_that_isnt_the_rest_is_not_appended() {
    let dir = crate::TempDir::new("resume-wrong-206");
    let (net, whole) = (Net::new(), file());
    let url = "https://resume-wrong-206.example/file";
    let asked = net.records(url);
    let w = whole.clone();
    net.replies(url, move |r| {
        Some(match r.header("Range") {
            Some(_) => crate::testkit::Reply { status: 206, headers: vec![], body: w.clone() }
                .with_header("Content-Range", "bytes 0-9999/10000"),
            None => crate::testkit::Reply::body(w.clone()),
        })
    });
    let dest = dir.join("file");
    fs::write(part_of(&dest), &whole[..3_000]).unwrap();
    let mut most = 0;
    download_resumable(url, &dest, 10_000, &sha(&whole), &never(), |n| most = most.max(n)).unwrap();
    assert_eq!(fs::read(&dest).unwrap(), whole);
    assert!(most <= 10_000, "progress went to {most} of 10000");
    let ranges: Vec<Option<String>> = asked.requests().iter().map(|r| r.header("Range").map(String::from)).collect();
    assert_eq!(ranges, [Some("bytes=3000-".into()), None]);
}

/// A 206 with more than the rest (junk past the file's end) is read only
/// to the file's size: the file matches, nothing is asked again, and
/// progress stops at its size (`download_resumable`: "Reads stop at the
/// file's size").
#[test]
fn reads_stop_at_the_files_size() {
    let dir = crate::TempDir::new("resume-long-206");
    let (net, whole) = (Net::new(), file());
    let url = "https://resume-long-206.example/file";
    let asked = net.records(url);
    let w = whole.clone();
    net.replies(url, move |r| {
        let mut body = w[3_000..].to_vec();
        body.extend(vec![0xaa; 200_000]);
        let _ = r;
        Some(
            crate::testkit::Reply { status: 206, headers: vec![], body }
                .with_header("Content-Range", "bytes 3000-9999/10000"),
        )
    });
    let dest = dir.join("file");
    fs::write(part_of(&dest), &whole[..3_000]).unwrap();
    let mut most = 0;
    download_resumable(url, &dest, 10_000, &sha(&whole), &never(), |n| most = most.max(n)).unwrap();
    assert_eq!((fs::read(&dest).unwrap(), most), (whole, 10_000));
    assert_eq!(asked.requests().len(), 1, "asked again");
}

/// A body that ends cleanly before the file does keeps what came: it isn't
/// damage, and the next try carries on from there (`download_resumable`:
/// "A body that ends before the file does keeps what came").
#[test]
fn a_body_that_ends_early_keeps_what_came() {
    let dir = crate::TempDir::new("resume-short");
    let (net, whole) = (Net::new(), file());
    let url = "https://resume-short.example/file";
    let asked = net.records(url);
    let short = std::sync::Arc::new(AtomicBool::new(true));
    let (w, cut) = (whole.clone(), short.clone());
    net.replies(url, move |r| {
        let end = if cut.load(Ordering::Relaxed) { 6_000 } else { 10_000 };
        Some(crate::testkit::Reply::ranged(&w[..end], r))
    });
    let dest = dir.join("file");
    fs::write(part_of(&dest), &whole[..3_000]).unwrap();
    let err = download_resumable(url, &dest, 10_000, &sha(&whole), &never(), |_| {}).unwrap_err();
    assert!(err.to_string().contains("ended before"), "{err:#}");
    assert_eq!(fs::read(part_of(&dest)).unwrap(), whole[..6_000], "what came went");
    short.store(false, Ordering::Relaxed);
    download_resumable(url, &dest, 10_000, &sha(&whole), &never(), |_| {}).unwrap();
    assert_eq!(fs::read(&dest).unwrap(), whole);
    let ranges: Vec<Option<String>> = asked.requests().iter().map(|r| r.header("Range").map(String::from)).collect();
    assert_eq!(ranges, [Some("bytes=3000-".into()), Some("bytes=6000-".into())]);
}

/// A link where the part goes (to a file of the user's) is never written
/// through: it's taken away, and the download writes a file of its own
/// (`open_part`).
#[cfg(unix)]
#[test]
fn a_link_where_the_part_goes_is_not_written_through() {
    let dir = crate::TempDir::new("resume-link");
    let (net, whole) = (Net::new(), file());
    let url = "https://resume-link.example/file";
    net.serves(url, whole.clone());
    let theirs = dir.join("theirs");
    fs::write(&theirs, b"precious").unwrap();
    let dest = dir.join("file");
    std::os::unix::fs::symlink(&theirs, part_of(&dest)).unwrap();
    download_resumable(url, &dest, 10_000, &sha(&whole), &never(), |_| {}).unwrap();
    assert_eq!(fs::read(&theirs).unwrap(), b"precious", "written through the link");
    assert!(!fs::symlink_metadata(&dest).unwrap().file_type().is_symlink());
    assert_eq!(fs::read(&dest).unwrap(), whole);
}
