//! How far a Matroska file still being written holds whole (`written`).
//! Footage sections download as Matroska (`fetch::Wanted::Section`), which
//! ffmpeg writes a cluster at a time, each with its size, so what's there of
//! one can be read, up to its last whole cluster, while the rest comes in.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

const EBML: u32 = 0x1A45_DFA3;
const SEGMENT: u32 = 0x1853_8067;
const INFO: u32 = 0x1549_A966;
const TIMESTAMP_SCALE: u32 = 0x2A_D7B1;
const CLUSTER: u32 = 0x1F43_B675;
const CLUSTER_TIMESTAMP: u32 = 0xE7;
const SIMPLE_BLOCK: u32 = 0xA3;
const BLOCK_GROUP: u32 = 0xA0;
const BLOCK: u32 = 0xA1;

/// How far into Matroska file `path` (in its seconds) each of its tracks is
/// whole, the least of them: to the last frame of each in the clusters written
/// whole. None if it isn't Matroska, or has no whole cluster yet.
pub fn written(path: &Path) -> Option<f64> {
    let file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let mut r = Reader { inner: BufReader::with_capacity(64 << 10, file), pos: 0, len };
    let (id, size) = r.header()?;
    if id != EBML {
        return None;
    }
    r.skip(size?)?;
    let (id, size) = r.header()?;
    if id != SEGMENT {
        return None;
    }
    // Its size is written once it's done, if ever.
    let end = size.map_or(len, |s| (r.pos + s).min(len));
    let mut scale = 1_000_000u64;
    let mut last: HashMap<u64, i64> = HashMap::new();
    while r.pos < end {
        let Some((id, Some(size))) = r.header() else { break };
        let child_end = r.pos + size;
        if child_end > len {
            break;
        }
        // Something malformed in it: what came before still counts.
        let read = match id {
            INFO => r.info(child_end, &mut scale),
            CLUSTER => r.cluster(child_end, &mut last),
            _ => Some(()),
        };
        if read.and_then(|()| r.to(child_end)).is_none() {
            break;
        }
    }
    let least = last.values().min()?;
    Some(*least as f64 * scale as f64 / 1e9)
}

struct Reader {
    inner: BufReader<File>,
    pos: u64,
    len: u64,
}

impl Reader {
    fn byte(&mut self) -> Option<u8> {
        let mut b = [0];
        self.inner.read_exact(&mut b).ok()?;
        self.pos += 1;
        Some(b[0])
    }

    /// A variable-length number: its length is told by its first byte's
    /// leading zeros. An element's ID keeps that marker; a size doesn't.
    fn varint(&mut self, keep_marker: bool) -> Option<(u64, u32)> {
        let first = self.byte()?;
        let length = first.leading_zeros() + 1;
        if length > 8 {
            return None;
        }
        let mut value = if keep_marker { first as u64 } else { (first as u64) & (0xFF >> length) };
        for _ in 1..length {
            value = (value << 8) | self.byte()? as u64;
        }
        Some((value, length))
    }

    /// An element's ID and size (None where it's unknown: all ones).
    fn header(&mut self) -> Option<(u32, Option<u64>)> {
        if self.pos >= self.len {
            return None;
        }
        let (id, _) = self.varint(true)?;
        let (size, length) = self.varint(false)?;
        let unknown = size == (1u64 << (7 * length)) - 1;
        Some((id as u32, (!unknown).then_some(size)))
    }

    fn uint(&mut self, size: u64) -> Option<u64> {
        if size > 8 {
            return None;
        }
        let mut value = 0;
        for _ in 0..size {
            value = (value << 8) | self.byte()? as u64;
        }
        Some(value)
    }

    /// A block's track and time (its cluster's, `at`, and its own after it):
    /// the latest yet of its track goes in `last`.
    fn block(&mut self, at: i64, last: &mut HashMap<u64, i64>) -> Option<()> {
        let (track, _) = self.varint(false)?;
        let time = at + i16::from_be_bytes([self.byte()?, self.byte()?]) as i64;
        let latest = last.entry(track).or_insert(time);
        *latest = (*latest).max(time);
        Some(())
    }

    /// The segment's info, to `end`: its timestamp scale goes in `scale`.
    fn info(&mut self, end: u64, scale: &mut u64) -> Option<()> {
        while self.pos < end {
            let (id, size) = self.header()?;
            let size = size?;
            if id == TIMESTAMP_SCALE {
                *scale = self.uint(size)?;
            } else {
                self.skip(size)?;
            }
        }
        Some(())
    }

    /// A whole cluster, to `end`: each track's latest frame in it goes in `last`.
    fn cluster(&mut self, end: u64, last: &mut HashMap<u64, i64>) -> Option<()> {
        let mut at = 0i64;
        while self.pos < end {
            let (id, size) = self.header()?;
            let next = self.pos + size?;
            match id {
                CLUSTER_TIMESTAMP => at = self.uint(next - self.pos)? as i64,
                SIMPLE_BLOCK => self.block(at, last)?,
                BLOCK_GROUP => {
                    while self.pos < next {
                        let (id, size) = self.header()?;
                        let inner = self.pos + size?;
                        if id == BLOCK {
                            self.block(at, last)?;
                        }
                        self.to(inner)?;
                    }
                }
                _ => {}
            }
            self.to(next)?;
        }
        Some(())
    }

    fn skip(&mut self, size: u64) -> Option<()> {
        self.to(self.pos + size)
    }

    fn to(&mut self, pos: u64) -> Option<()> {
        if pos > self.len {
            return None;
        }
        if pos != self.pos {
            let by = i64::try_from(pos as i128 - self.pos as i128).ok()?;
            self.inner.seek_relative(by).ok()?;
            self.pos = pos;
        }
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file cut short reads to its last whole cluster; a whole one, to its
    /// end. Skipped where ffmpeg isn't installed.
    #[test]
    fn reads_as_far_as_is_whole() {
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
        std::fs::write(&half, b"not matroska at all").unwrap();
        assert_eq!(written(&half), None);
    }

    /// Clusters before one with a malformed child (here a child of unknown
    /// size, which only some masters may have) still count.
    #[test]
    fn a_malformed_cluster_keeps_what_came_before() {
        // An element: its ID's bytes, its size as one byte (with the marker), its body.
        fn el(id: &[u8], body: &[u8]) -> Vec<u8> {
            let mut out = id.to_vec();
            out.push(0x80 | body.len() as u8);
            out.extend_from_slice(body);
            out
        }
        // A cluster at `ms` with one frame of track 1 at its start.
        let cluster = |ms: u16| {
            let mut body = el(&[0xE7], &ms.to_be_bytes());
            body.extend(el(&[0xA3], &[0x81, 0, 0, 0x80, 0xAA]));
            el(&[0x1F, 0x43, 0xB6, 0x75], &body)
        };
        let mut segment = el(&[0x15, 0x49, 0xA9, 0x66], &el(&[0x2A, 0xD7, 0xB1], &[0x0F, 0x42, 0x40]));
        segment.extend(cluster(0));
        segment.extend(cluster(1000));
        // A third cluster whose child says its size is unknown (all ones).
        let mut bad = el(&[0xE7], &2000u16.to_be_bytes());
        bad.extend([0xA3, 0xFF, 0x81, 0, 0, 0x80]);
        segment.extend(el(&[0x1F, 0x43, 0xB6, 0x75], &bad));
        let mut file = el(&[0x1A, 0x45, 0xDF, 0xA3], &[]);
        // The segment's size unknown, as while it's written.
        file.extend([0x18, 0x53, 0x80, 0x67, 0xFF]);
        file.extend(segment);

        let dir = crate::TempDir::new("mkv-bad");
        let path = dir.join("bad.mkv");
        std::fs::write(&path, &file).unwrap();
        assert_eq!(written(&path), Some(1.0));
    }
}
