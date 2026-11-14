//! The specs of `mkv::written`: how far a Matroska file, whole or still being
//! written, can be read. The files are built
//! here byte by byte, as ffmpeg lays them out; one test checks a file ffmpeg
//! itself writes, where it's installed.

use super::*;

/// An element: its ID's bytes, its size (as short a number as holds it), its body.
fn el(id: &[u8], body: &[u8]) -> Vec<u8> {
    let mut out = id.to_vec();
    match body.len() {
        n if n < 0x7F => out.push(0x80 | n as u8),
        n if n < 0x3FFF => out.extend([0x40 | (n >> 8) as u8, n as u8]),
        n => {
            out.push(0x01);
            out.extend(&(n as u64).to_be_bytes()[1..]);
        }
    }
    out.extend_from_slice(body);
    out
}

/// A frame of `track` at `offset` (in the file's ticks) from its cluster's time.
fn frame(track: u8, offset: i16) -> Vec<u8> {
    let mut body = vec![0x80 | track];
    body.extend(offset.to_be_bytes());
    body.extend([0x80, 0xAA]);
    body
}

/// A cluster at `at` ticks with these frames, as SimpleBlocks.
fn cluster(at: u32, frames: &[(u8, i16)]) -> Vec<u8> {
    let mut body = el(&[0xE7], &at.to_be_bytes());
    for &(track, offset) in frames {
        body.extend(el(&[0xA3], &frame(track, offset)));
    }
    el(&[0x1F, 0x43, 0xB6, 0x75], &body)
}

/// The segment's info, its ticks `scale` nanoseconds each.
fn info(scale: u32) -> Vec<u8> {
    el(&[0x15, 0x49, 0xA9, 0x66], &el(&[0x2A, 0xD7, 0xB1], &scale.to_be_bytes()))
}

/// The EBML header a Matroska file starts with.
fn header() -> Vec<u8> {
    el(&[0x1A, 0x45, 0xDF, 0xA3], &el(&[0x42, 0x82], b"matroska"))
}

/// The EBML header, then a segment of `children` whose size isn't written
/// yet, as while ffmpeg is still writing it.
fn being_written(children: &[Vec<u8>]) -> Vec<u8> {
    let mut file = header();
    file.extend([0x18, 0x53, 0x80, 0x67, 0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
    file.extend(children.concat());
    file
}

/// The same, finished: the segment's size written.
fn finished(children: &[Vec<u8>]) -> Vec<u8> {
    let mut file = header();
    file.extend(el(&[0x18, 0x53, 0x80, 0x67], &children.concat()));
    file
}

/// What `written` reads of a file holding `bytes`.
fn read(bytes: &[u8]) -> Option<f64> {
    let dir = crate::TempDir::new("mkv");
    let path = dir.join("section.mkv.part");
    std::fs::write(&path, bytes).unwrap();
    written(&path)
}

/// A whole file reads to the last frame in it (mkv.rs `written`: "How far
/// into Matroska file `path` (in its seconds) each of its tracks is whole …
/// to the last frame of each in the clusters written whole").
#[test]
fn a_whole_file_reads_to_its_last_frame() {
    let file = finished(&[info(1_000_000), cluster(0, &[(1, 0), (1, 500)]), cluster(1000, &[(1, 0), (1, 750)])]);
    assert_eq!(read(&file), Some(1.75));
}

/// A file cut off partway through a cluster reads to the last frame of the
/// cluster before (mkv.rs's module doc: "ffmpeg writes a cluster at a time, each with
/// its size, so what's there of one can be read, up to its last whole
/// cluster, while the rest comes in").
#[test]
fn a_file_still_being_written_reads_to_its_last_whole_cluster() {
    let whole = [info(1_000_000), cluster(0, &[(1, 0), (1, 500)]), cluster(1000, &[(1, 0), (1, 500)])];
    let mut file = being_written(&whole);
    let last = cluster(2000, &[(1, 0), (1, 500)]);
    file.extend(&last[..last.len() - 3]);
    assert_eq!(read(&file), Some(1.5));
    file.extend(&last[last.len() - 3..]);
    assert_eq!(read(&file), Some(2.5), "once the cluster is in, it counts");
}

/// With a picture and a sound track, the file is whole as far as the one
/// that's furthest behind (mkv.rs `written`: "each of its tracks is whole,
/// the least of them").
#[test]
fn a_file_is_whole_only_as_far_as_its_track_furthest_behind() {
    let file = being_written(&[
        info(1_000_000),
        cluster(0, &[(1, 0), (2, 0), (1, 1000)]),
        cluster(2000, &[(1, 0), (1, 900), (2, -500)]),
    ]);
    assert_eq!(read(&file), Some(1.5), "track 2's last frame, at 2000 - 500");
}

/// Times are the file's own seconds, by the timestamp scale its info gives
/// (mkv.rs `written`: "in its seconds").
#[test]
fn times_are_in_seconds_by_the_files_own_timestamp_scale() {
    // Ticks of a tenth of a millisecond: 20,000 of them are 2 s.
    let file = being_written(&[info(100_000), cluster(0, &[(1, 0)]), cluster(20_000, &[(1, 5_000)])]);
    assert_eq!(read(&file), Some(2.5));
}

/// A file whose info gives no scale counts in milliseconds, Matroska's
/// default (mkv.rs `written`: "in its seconds"; RFC 9559, TimestampScale's
/// default 1000000).
#[test]
fn a_file_with_no_timestamp_scale_counts_in_milliseconds() {
    let file = being_written(&[cluster(0, &[(1, 0)]), cluster(3000, &[(1, 250)])]);
    assert_eq!(read(&file), Some(3.25));
}

/// A frame in a BlockGroup is a frame as a SimpleBlock is (mkv.rs `written`:
/// "to the last frame of each in the clusters written whole").
#[test]
fn frames_in_block_groups_count_as_frames() {
    let mut body = el(&[0xE7], &1000u32.to_be_bytes());
    body.extend(el(&[0xA3], &frame(1, 0)));
    body.extend(el(&[0xA0], &[el(&[0xA1], &frame(1, 800)), el(&[0x9B], &[0x10])].concat()));
    let grouped = el(&[0x1F, 0x43, 0xB6, 0x75], &body);
    let file = being_written(&[info(1_000_000), cluster(0, &[(1, 0)]), grouped.clone()]);
    assert_eq!(read(&file), Some(1.8));
    let file = being_written(&[info(1_000_000), grouped, cluster(2000, &[(1, 0)])]);
    assert_eq!(read(&file), Some(2.0), "and the clusters after it count too");
}

/// A finished file, its segment's size written, reads as far as that
/// segment goes, and nothing after it counts (mkv.rs `written`'s comment: "Its size is
/// written once it's done, if ever.").
#[test]
fn a_finished_file_reads_its_segment_and_nothing_after_it() {
    let mut file = finished(&[info(1_000_000), cluster(0, &[(1, 0)]), cluster(1000, &[(1, 500)])]);
    assert_eq!(read(&file), Some(1.5));
    file.extend(cluster(5000, &[(1, 0)]));
    assert_eq!(read(&file), Some(1.5), "a cluster past the segment's end isn't the segment's");
}

/// Sizes and numbers written at their longest, 8 bytes, as ffmpeg leaves
/// a size it fills in once it's known, read as the shortest do, an empty
/// element included (mkv.rs `written`: "How far into Matroska file `path`";
/// RFC 9559: an element's size is a variable-size integer of 1 to 8 bytes,
/// an unsigned integer up to 8 bytes long).
#[test]
fn sizes_and_numbers_written_at_their_longest_read_alike() {
    // An element whose size takes 8 bytes.
    let long = |id: &[u8], body: &[u8]| {
        let mut out = id.to_vec();
        out.push(0x01);
        out.extend(&(body.len() as u64).to_be_bytes()[1..]);
        out.extend_from_slice(body);
        out
    };
    let mut body = long(&[0xE7], &1000u64.to_be_bytes());
    body.extend(long(&[0xA3], &frame(1, 250)));
    let late = long(&[0x1F, 0x43, 0xB6, 0x75], &body);
    let void = long(&[0xEC], &[]);
    let file = finished(&[void, info(1_000_000), cluster(0, &[(1, 0)]), late]);
    assert_eq!(read(&file), Some(1.25));
}

/// A file that isn't Matroska, or isn't there, reads as nothing (mkv.rs
/// `written`: "None if it isn't Matroska").
#[test]
fn what_isnt_matroska_reads_as_nothing() {
    assert_eq!(read(b"not matroska at all"), None);
    assert_eq!(read(b""), None);
    // An EBML file, but not a Matroska segment after its header.
    let mut other = header();
    other.extend(el(&[0x1F, 0x43, 0xB6, 0x75], &cluster(0, &[(1, 0)])));
    assert_eq!(read(&other), None);
    let dir = crate::TempDir::new("mkv-none");
    assert_eq!(written(&dir.join("missing.mkv")), None);
}

/// A file with no whole cluster yet reads as nothing, not as 0 (mkv.rs
/// `written`: "None if it … has no whole cluster yet").
#[test]
fn a_file_with_no_whole_cluster_yet_reads_as_nothing() {
    assert_eq!(read(&being_written(&[info(1_000_000)])), None);
    let first = cluster(0, &[(1, 0), (1, 500)]);
    let mut file = being_written(&[info(1_000_000)]);
    file.extend(&first[..first.len() - 1]);
    assert_eq!(read(&file), None);
}

/// Clusters before one with a malformed child (here a child of unknown
/// size, which only some masters may have) still count (mkv.rs `written`'s comment:
/// "Something malformed in it: what came before still counts.").
#[test]
fn a_malformed_cluster_keeps_what_came_before() {
    // A third cluster whose child says its size is unknown (all ones).
    let mut bad = el(&[0xE7], &2000u16.to_be_bytes());
    bad.extend([0xA3, 0xFF, 0x81, 0, 0, 0x80]);
    let bad = el(&[0x1F, 0x43, 0xB6, 0x75], &bad);
    let file = being_written(&[info(1_000_000), cluster(0, &[(1, 0)]), cluster(1000, &[(1, 0)]), bad]);
    assert_eq!(read(&file), Some(1.0));
}

/// A file ffmpeg writes as footage sections are written reads to its end
/// when whole and to a last whole cluster when cut short, as the built files
/// above do (mkv.rs's module doc). Skipped where ffmpeg isn't installed.
#[test]
fn a_file_ffmpeg_writes_reads_as_far_as_is_whole() {
    let Ok(ffmpeg) = which::which("ffmpeg") else {
        eprintln!("skipped: no ffmpeg on PATH");
        return;
    };
    let dir = crate::TempDir::new("mkv");
    let whole = dir.join("whole.mkv");
    let ok = std::process::Command::new(&ffmpeg)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=160x90:rate=30:duration=20"])
        .args(["-f", "lavfi", "-i", "sine=duration=20", "-c:v", "libx264", "-g", "30", "-c:a", "aac"])
        .args(["-cluster_time_limit", "1000", "-f", "matroska"])
        .arg(&whole)
        .status();
    assert!(ok.unwrap().success());
    let all = written(&whole).unwrap();
    assert!((19.8..=20.0).contains(&all), "{all}");

    let bytes = std::fs::read(&whole).unwrap();
    let half = dir.join("half.mkv");
    std::fs::write(&half, &bytes[..bytes.len() / 2]).unwrap();
    let some = written(&half).unwrap();
    assert!((6.0..14.0).contains(&some), "{some}");

    std::fs::write(&half, &bytes[..200]).unwrap();
    assert_eq!(written(&half), None, "no cluster yet");
}
