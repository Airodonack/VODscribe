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
        match id {
            INFO => {
                while r.pos < child_end {
                    let (id, size) = r.header()?;
                    let size = size?;
                    if id == TIMESTAMP_SCALE {
                        scale = r.uint(size)?;
                    } else {
                        r.skip(size)?;
                    }
                }
            }
            CLUSTER => {
                let mut at = 0i64;
                while r.pos < child_end {
                    let (id, size) = r.header()?;
                    let size = size?;
                    let next = r.pos + size;
                    match id {
                        CLUSTER_TIMESTAMP => at = r.uint(size)? as i64,
                        SIMPLE_BLOCK => r.block(at, &mut last)?,
                        BLOCK_GROUP => {
                            while r.pos < next {
                                let (id, size) = r.header()?;
                                let size = size?;
                                let inner = r.pos + size;
                                if id == BLOCK {
                                    r.block(at, &mut last)?;
                                }
                                r.to(inner)?;
                            }
                        }
                        _ => {}
                    }
                    r.to(next)?;
                }
            }
            _ => {}
        }
        r.to(child_end)?;
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
}
