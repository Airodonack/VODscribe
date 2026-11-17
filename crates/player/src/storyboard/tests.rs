//! The specs of storyboard.rs: the small keyframe pictures kept of a file
//! for the timeline's thumbnail and the library's picture. The parts that
//! read and write a storyboard's file run on files of their own; a file's
//! storyboard is made by the real ffmpeg, as the player's tests do, and
//! skipped where there is none.

use super::*;
use vodscribe_core::local::Colour;

/// A fresh folder for a test, removed when it's dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("vodscribe-sb-{name}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fingerprint() -> Fingerprint {
    Fingerprint { path: "/videos/a.mp4".into(), size: 1234, modified: 5678 }
}

fn layout(slots: u32) -> Layout {
    Layout { width: 160, height: 90, interval: 2.0, slots, duration: slots as f64 * 2.0 }
}

/// A JPEG's shape, enough for `split_jpegs`: start, a segment, the
/// compressed data (with a stuffed 0xFF and a restart), end.
fn jpeg(fill: u8) -> Vec<u8> {
    vec![
        0xFF, 0xD8, 0xFF, 0xDB, 0x00, 0x04, fill, fill, 0xFF, 0xDA, 0x00, 0x02, fill, 0xFF, 0x00, 0xFF, 0xD3, fill,
        0xFF, 0xD9,
    ]
}

/// A storyboard's file with the frames `frames` (slot, pts) in it.
fn written(dir: &Path, slots: u32, frames: &[(u32, f64)]) -> PathBuf {
    let path = dir.join("a.sb");
    let mut writer = Writer::open(&path, &fingerprint(), layout(slots)).unwrap();
    for &(slot, pts) in frames {
        writer.add(slot, Some((pts, &jpeg(pts as u8)))).unwrap();
    }
    path
}

/// The slots are filled halves first, then quarters and eighths, each slot
/// once, so a frame near anywhere exists soon (storyboard.rs, module doc:
/// "The slots are filled halves first (½, then ¼ and ¾, then the
/// eighths…), so a frame near anywhere exists within a second of
/// starting").
#[test]
fn slots_are_filled_halves_first_each_once() {
    assert_eq!(order(16)[..7], [8, 4, 12, 2, 6, 10, 14]);
    for n in [1, 2, 3, 5, 16, 300, 3600] {
        let mut all = order(n);
        assert_eq!(all.len(), n as usize, "each slot once");
        all.sort();
        assert_eq!(all, (0..n).collect::<Vec<_>>());
    }
    assert_eq!(order(1), [0]);
}

/// A frame is taken every two seconds up to two hours, then the slots
/// spread so a file's storyboard stays about as large (storyboard.rs
/// `SHORTEST_INTERVAL`, `MOST_SLOTS`: "past two hours they spread out, so a
/// file's storyboard stays near 15 MB however long it is").
#[test]
fn slots_are_two_seconds_apart_up_to_two_hours_then_spread() {
    let picture = Picture { width: 1920, height: 1080, fps: 30.0, colour: Colour::UNTAGGED };
    let hour = Layout::of(picture, 3600.0);
    assert_eq!((hour.width, hour.height, hour.interval, hour.slots), (160, 90, 2.0, 1800));
    let two_hours = Layout::of(picture, 7200.0);
    assert_eq!((two_hours.interval, two_hours.slots), (2.0, 3600));
    let five = Layout::of(picture, 5.0 * 3600.0);
    assert_eq!((five.interval, five.slots), (5.0, 3600));
    assert_eq!(Layout::of(picture, 0.5).slots, 1, "a short file has a slot");
}

/// A frame keeps its video's shape as stored, so a phone's upright video is
/// narrow (storyboard.rs `Layout::of`: "as stored: the player doesn't
/// turn rotated phone videos either").
#[test]
fn a_frame_keeps_the_shape_of_its_video_as_stored() {
    let phone = Picture { width: 1080, height: 1920, fps: 30.0, colour: Colour::UNTAGGED };
    assert_eq!(Layout::of(phone, 60.0).width, 50);
}

/// A file's library picture is a frame an eighth of the way in, not a tenth
/// (0.6.0: "A file's library picture comes later. It's a frame an eighth of
/// the way in, not a tenth").
#[test]
fn the_library_picture_is_the_frame_an_eighth_of_the_way_in() {
    let picture = Picture { width: 1920, height: 1080, fps: 30.0, colour: Colour::UNTAGGED };
    let hour = Layout::of(picture, 3600.0);
    // Slot 225 of 1800, at 450 s: an eighth.
    assert_eq!(hour.time(hour.picture_slot()), 450.0);
    // Made fourth, within the first batch.
    assert!(order(hour.slots).iter().position(|&s| s == hour.picture_slot()).unwrap() < BATCH);
    assert_eq!(Layout::of(picture, 0.5).picture_slot(), 0, "a file of one slot");
}

/// What was written is what's read back: the file's fingerprint and layout,
/// each frame by its pts, which slots are made (storyboard.rs, module doc:
/// "The file: a header …, then records `[slot u32][pts f64][len u32][jpeg]`").
#[test]
fn a_storyboard_reads_back_what_was_written() {
    let dir = TempDir::new("records");
    let path = written(&dir.0, 4, &[(2, 4.0), (1, 2.0)]);
    let board = Storyboard::open(&path).unwrap();
    assert!(board.of(&fingerprint()) && board.layout == layout(4));
    assert_eq!(board.made(), 2);
    assert!(!board.complete());
    assert_eq!(board.slot(2), Some(4.0));
    assert_eq!(board.slot(0), None, "not made");
    assert_eq!(board.frame(2.0).unwrap(), jpeg(2));
    assert_eq!(board.frame(4.0).unwrap(), jpeg(4));
    assert!(board.frame(3.0).is_err(), "no frame stored there");
    assert_eq!(std::fs::metadata(&path).unwrap().len(), board.end);
}

/// Keyframes further apart than the slots: the slot after shows the same
/// keyframe, stored once (storyboard.rs, module doc: "A slot whose keyframe
/// is stored already (keyframes further apart than the interval) is a
/// record with no bytes and that keyframe's pts").
#[test]
fn a_keyframe_further_apart_than_the_slots_is_stored_once() {
    let dir = TempDir::new("once");
    let path = dir.0.join("a.sb");
    let mut writer = Writer::open(&path, &fingerprint(), layout(4)).unwrap();
    writer.add(2, Some((4.0, &jpeg(1)))).unwrap();
    writer.add(3, Some((4.0, &jpeg(1)))).unwrap();
    let board = Storyboard::open(&path).unwrap();
    assert_eq!(board.frames.len(), 1, "the same keyframe isn't stored twice");
    assert_eq!(board.slot(3), Some(4.0));
    assert_eq!(board.frame(4.0).unwrap(), jpeg(1));
}

/// The second slot of a keyframe stored already is a record with no picture
/// (storyboard.rs, module doc: "a record with no bytes and that keyframe's
/// pts"), so the file doesn't grow by the picture again.
#[test]
fn the_second_slot_of_a_stored_keyframe_adds_a_record_with_no_picture() {
    let dir = TempDir::new("once-bytes");
    let path = dir.0.join("a.sb");
    let mut writer = Writer::open(&path, &fingerprint(), layout(4)).unwrap();
    writer.add(2, Some((4.0, &jpeg(1)))).unwrap();
    let before = writer.board.end;
    writer.add(3, Some((4.0, &jpeg(1)))).unwrap();
    assert_eq!(writer.board.end - before, RECORD);
    assert_eq!(std::fs::metadata(&path).unwrap().len(), writer.board.end);
}

/// A slot with no frame at all (past the last keyframe) is made, with none
/// (storyboard.rs, module doc: "one with no frame at all (past the last),
/// no bytes and a NaN pts"), so the storyboard can be complete.
#[test]
fn a_slot_with_no_frame_is_made_with_none() {
    let dir = TempDir::new("none");
    let path = written(&dir.0, 2, &[(1, 2.0)]);
    let mut writer = Writer::open(&path, &fingerprint(), layout(2)).unwrap();
    writer.add(0, None).unwrap();
    let board = Storyboard::open(&path).unwrap();
    assert!(board.complete());
    assert_eq!(board.slot(0), None, "no frame to show");
    // Made, before the first keyframe: the first.
    assert_eq!(board.shown_at(0.5, 0.0), Some(2.0));
}

/// The nearest frame stored is found from either side, and the
/// first or last at the ends (storyboard.rs `nearest`: "The pts of the frame
/// stored nearest `time`").
#[test]
fn the_nearest_frame_is_found_from_either_side() {
    let dir = TempDir::new("nearest");
    let board = Storyboard::open(&written(&dir.0, 4, &[(2, 4.0), (1, 2.0)])).unwrap();
    assert_eq!(board.nearest(0.0), Some(2.0));
    assert_eq!(board.nearest(2.9), Some(2.0));
    assert_eq!(board.nearest(3.1), Some(4.0));
    assert_eq!(board.nearest(99.0), Some(4.0));
    let empty = Storyboard::open(&written(&TempDir::new("nearest-none").0, 4, &[])).unwrap();
    assert_eq!(empty.nearest(1.0), None);
}

/// Once the slots either side of a time are made, the picture shown is the
/// keyframe at or before it, however far back; before then, the nearest
/// within the interval (or the distance asked) stands in (0.6.0: "The
/// pictures are made in the background. … they show the one before the
/// pointer"; storyboard.rs `shown_at`).
#[test]
fn the_picture_shown_is_the_keyframe_at_or_before_the_time() {
    let dir = TempDir::new("shown");
    let board = Storyboard::open(&written(&dir.0, 4, &[(2, 4.0), (1, 2.0), (3, 4.0)])).unwrap();
    // Slots 1 to 3 made: from 2 s on, the keyframe at or before the time.
    assert_eq!(board.shown_at(3.9, 0.0), Some(2.0));
    assert_eq!(board.shown_at(7.9, 0.0), Some(4.0));
    assert_eq!(board.shown_at(99.0, 0.0), Some(4.0));
    // Slot 0 isn't made yet: only a frame within the interval (or `near`).
    assert_eq!(board.shown_at(1.0, 0.0), Some(2.0));
    assert_eq!(board.shown_at(-1.0, 0.0), None);
    assert_eq!(board.shown_at(-1.0, 5.0), Some(2.0));
}

/// Before the slot a time is in, or the one after, is made, a frame that's
/// there stands in only if it's near, and nothing is shown past the end until
/// the last slot is made (storyboard.rs `shown_at`: "That keyframe is known
/// once the slots either side of `time` are made").
#[test]
fn the_keyframe_before_a_time_is_shown_only_once_the_slots_either_side_are_made() {
    let dir = TempDir::new("either-side");
    // A screen recording's one keyframe at 0 stands for slots 0 and 2.
    let path = dir.0.join("a.sb");
    let mut writer = Writer::open(&path, &fingerprint(), layout(4)).unwrap();
    writer.add(0, Some((0.0, &jpeg(1)))).unwrap();
    writer.add(2, Some((0.0, &jpeg(1)))).unwrap();
    let board = Storyboard::open(&path).unwrap();
    // Slot 2 is made but slot 3 isn't: the keyframe at 0 is 5 s away.
    assert_eq!(board.shown_at(5.0, 0.0), None);
    assert_eq!(board.shown_at(5.0, 5.0), Some(0.0), "unless the timeline's few pixels reach it");
    // Past the end, with the last slot not made: nothing.
    assert_eq!(board.shown_at(99.0, 0.0), None);
    // A time past a storyboard of four is the last slot's.
    writer.add(3, Some((0.0, &jpeg(1)))).unwrap();
    assert_eq!(Storyboard::open(&path).unwrap().shown_at(99.0, 0.0), Some(0.0));
    // Slots 4 and 5 made and the time between them: the keyframe at or before it, not the nearer one after.
    let gap = TempDir::new("either-side-gap");
    let mut writer = Writer::open(&gap.0.join("a.sb"), &fingerprint(), layout(10)).unwrap();
    writer.add(4, Some((8.0, &jpeg(8)))).unwrap();
    writer.add(5, Some((10.0, &jpeg(10)))).unwrap();
    let board = Storyboard::open(&gap.0.join("a.sb")).unwrap();
    assert_eq!(board.shown_at(9.9, 0.0), Some(8.0));
}

/// In a file whose keyframes are far apart (a screen recording), anywhere
/// between two shows the keyframe before it, however far, not the nearer one
/// after (0.6.0: "between two keyframes (far apart in a screen recording)
/// they show the one before the pointer, the frame the picture there is made
/// from").
#[test]
fn between_far_apart_keyframes_the_one_before_the_pointer_is_shown() {
    // A keyframe every 5 s, slots every 2: slots 0-2 take the one at 0,
    // 3-4 the one at 5, 5-7 at 10, 8-9 at 15.
    let dir = TempDir::new("gop");
    let path = dir.0.join("a.sb");
    let mut writer = Writer::open(&path, &fingerprint(), layout(10)).unwrap();
    for slot in order(10) {
        let key = (slot as f64 * 2.0 / 5.0).floor() * 5.0;
        writer.add(slot, Some((key, &jpeg(key as u8)))).unwrap();
    }
    let board = Storyboard::open(&path).unwrap();
    assert_eq!(board.frames.len(), 4);
    for (time, key) in
        [(0.0, 0.0), (2.6, 0.0), (4.99, 0.0), (5.0, 5.0), (7.6, 5.0), (12.6, 10.0), (17.6, 15.0), (19.9, 15.0)]
    {
        assert_eq!(board.shown_at(time, 0.0), Some(key), "at {time} s");
    }
    // Not the nearest, which is the next keyframe, from after the pointer.
    assert_eq!(board.nearest(4.0), Some(5.0));
}

/// A crash mid-record loses only that record: read, the torn one isn't
/// there; carried on, it's cut and what was made stays (storyboard.rs,
/// module doc: "a storyboard made partway (the app quit, the job stopped)
/// carries on from where it was, and a crash loses at most the last, torn
/// record").
#[test]
fn a_torn_record_is_cut_and_what_was_made_carries_on() {
    let dir = TempDir::new("torn");
    let path = dir.0.join("a.sb");
    let mut writer = Writer::open(&path, &fingerprint(), layout(4)).unwrap();
    writer.add(2, Some((4.0, &jpeg(1)))).unwrap();
    let whole = writer.board.end;
    drop(writer);
    // Half a record written.
    let mut record = 1u32.to_le_bytes().to_vec();
    record.extend_from_slice(&2.0f64.to_le_bytes());
    record.extend_from_slice(&100u32.to_le_bytes());
    record.extend_from_slice(&[0xFF; 10]);
    std::fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(&record).unwrap();
    assert_eq!(Storyboard::open(&path).unwrap().made(), 1);
    let mut writer = Writer::open(&path, &fingerprint(), layout(4)).unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().len(), whole);
    assert_eq!(writer.board.slot(2), Some(4.0), "what was made stays");
    writer.add(1, Some((2.0, &jpeg(2)))).unwrap();
    let board = Storyboard::open(&path).unwrap();
    assert_eq!((board.made(), board.frame(2.0).unwrap()), (2, jpeg(2)));
}

/// A record whose length runs only a little past the end of the file is torn
/// too, not read as whole (storyboard.rs, module doc: "a crash loses at most
/// the last, torn record").
#[test]
fn a_record_cut_a_few_bytes_short_is_torn() {
    let dir = TempDir::new("torn-short");
    let path = written(&dir.0, 4, &[(2, 4.0)]);
    let mut record = 1u32.to_le_bytes().to_vec();
    record.extend_from_slice(&2.0f64.to_le_bytes());
    record.extend_from_slice(&20u32.to_le_bytes());
    record.extend_from_slice(&[0xFF; 10]);
    std::fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(&record).unwrap();
    let board = Storyboard::open(&path).unwrap();
    assert_eq!(board.made(), 1, "the record 10 bytes short was read");
    assert_eq!(board.slot(1), None);
}

/// A record naming a slot the storyboard doesn't have is torn too (storyboard.rs
/// `read_records`).
#[test]
fn a_record_for_a_slot_the_storyboard_lacks_is_not_read() {
    let dir = TempDir::new("torn-slot");
    let path = written(&dir.0, 4, &[(2, 4.0)]);
    let mut record = 4u32.to_le_bytes().to_vec();
    record.extend_from_slice(&2.0f64.to_le_bytes());
    record.extend_from_slice(&0u32.to_le_bytes());
    std::fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(&record).unwrap();
    assert_eq!(Storyboard::open(&path).unwrap().made(), 1);
}

/// Two records of the one keyframe are read as one frame (storyboard.rs
/// `Storyboard::note`: a keyframe stored once; the writer stores it once, and
/// the reader holds to that of a file with it twice).
#[test]
fn a_keyframe_written_twice_is_read_as_one_frame() {
    let dir = TempDir::new("twice");
    let path = written(&dir.0, 4, &[]);
    let mut records = Vec::new();
    for slot in [0u32, 1] {
        records.extend_from_slice(&slot.to_le_bytes());
        records.extend_from_slice(&2.0f64.to_le_bytes());
        records.extend_from_slice(&(jpeg(7).len() as u32).to_le_bytes());
        records.extend_from_slice(&jpeg(7));
    }
    std::fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(&records).unwrap();
    let board = Storyboard::open(&path).unwrap();
    assert_eq!(board.frames.len(), 1);
    assert_eq!((board.slot(0), board.slot(1)), (Some(2.0), Some(2.0)));
}

/// A reader holding a storyboard being made reads on from where it was
/// (storyboard.rs `refresh`: "Reads what's been appended since it was
/// read").
#[test]
fn a_reader_of_a_storyboard_being_made_reads_on_from_where_it_was() {
    let dir = TempDir::new("refresh");
    let path = written(&dir.0, 4, &[(2, 4.0)]);
    let mut reader = Storyboard::open(&path).unwrap();
    let mut writer = Writer::open(&path, &fingerprint(), layout(4)).unwrap();
    writer.add(0, Some((0.0, &jpeg(3)))).unwrap();
    assert_eq!(reader.made(), 1);
    reader.refresh().unwrap();
    assert_eq!(reader.made(), 2);
    assert_eq!(reader.frame(0.0).unwrap(), jpeg(3));
}

/// The file as it was is what a storyboard is of: a different one (its
/// size or time changed) is made again from the start (storyboard.rs
/// `Fingerprint`: "a different one is made again").
#[test]
fn a_storyboard_of_a_file_as_it_was_is_made_again() {
    let dir = TempDir::new("changed");
    let path = written(&dir.0, 4, &[(2, 4.0), (1, 2.0)]);
    assert!(Storyboard::open(&path).unwrap().of(&fingerprint()));
    let changed = Fingerprint { size: 99, ..fingerprint() };
    assert!(!Storyboard::open(&path).unwrap().of(&changed));
    assert_eq!(Writer::open(&path, &changed, layout(4)).unwrap().board.made(), 0, "starts over");
}

/// A header's layout is read back only if it makes a storyboard: from one slot
/// to as many as the most (`MOST_SLOTS`), with a time between them (storyboard.rs
/// `Storyboard::open`: "not a storyboard").
#[test]
fn a_header_is_a_storyboard_only_if_its_layout_is_one() {
    let dir = TempDir::new("layouts");
    let path = dir.0.join("a.sb");
    let opens = |layout: Layout| Writer::open(&path, &fingerprint(), layout).is_ok();
    assert!(opens(Layout { slots: 1, ..layout(1) }), "one slot");
    assert!(opens(Layout { slots: MOST_SLOTS, ..layout(1) }), "the most slots");
    assert!(!opens(Layout { slots: 0, ..layout(1) }), "no slots");
    assert!(!opens(Layout { slots: MOST_SLOTS + 1, ..layout(1) }), "one slot too many");
    assert!(!opens(Layout { interval: f64::NAN, ..layout(4) }), "no time between slots");
    assert!(!opens(Layout { interval: 0.0, ..layout(4) }), "no time between slots");
    assert!(!opens(Layout { interval: -2.0, ..layout(4) }), "a time backwards");
    assert!(opens(Layout { interval: 0.5, ..layout(4) }), "a short time");
}

/// A path longer than any file's is no header's (storyboard.rs
/// `Storyboard::open`: "not a storyboard" for a path of over 64 KiB), but one
/// of any length up to it is read back.
#[test]
fn a_header_with_a_path_of_over_64_kib_is_not_a_storyboard() {
    let dir = TempDir::new("long-path");
    let path = dir.0.join("a.sb");
    let reads = |bytes: usize| {
        let printed = Fingerprint { path: "x".repeat(bytes), ..fingerprint() };
        Writer::open(&path, &printed, layout(4)).is_ok()
    };
    assert!(reads(2000), "a long path");
    assert!(reads(64 * 1024), "the longest");
    assert!(!reads(64 * 1024 + 1), "a byte more");
}

/// A file that isn't a storyboard (or of another version) isn't read as one
/// (storyboard.rs `Storyboard::open`: "not a storyboard of this version").
#[test]
fn a_file_that_is_not_a_storyboard_is_refused() {
    let dir = TempDir::new("refused");
    let path = dir.0.join("a.sb");
    std::fs::write(&path, b"VSSB").unwrap();
    assert!(Storyboard::open(&path).is_err(), "cut short");
    std::fs::write(&path, b"not a storyboard at all, only some bytes").unwrap();
    assert!(Storyboard::open(&path).is_err());
    assert!(Storyboard::open(&dir.0.join("gone.sb")).is_err());
}

/// ffmpeg gives a batch's pictures one after another; each is split off by
/// its markers (storyboard.rs `split_jpegs`: "each from its start-of-image
/// marker to its end-of-image one"), even where the compressed data holds
/// what looks like an end marker.
#[test]
fn a_batchs_pictures_are_split_by_their_markers() {
    let mut bytes = jpeg(1);
    bytes.extend(jpeg(0xD9));
    bytes.extend(jpeg(3));
    let parts = split_jpegs(&bytes);
    assert_eq!(parts, [jpeg(1).as_slice(), jpeg(0xD9).as_slice(), jpeg(3).as_slice()]);
    // Cut short: the whole ones only.
    assert_eq!(split_jpegs(&bytes[..bytes.len() - 3]).len(), 2);
}

/// What isn't a picture's start ends the reading, and bytes between a
/// picture's markers that aren't a marker are passed (storyboard.rs
/// `split_jpegs`: "The next marker: 0xFF (any fill), then its code").
#[test]
fn a_batchs_pictures_are_split_around_fill_and_what_follows_them() {
    let image = |inside: &[u8]| [&[0xFF, 0xD8][..], inside, &[0xFF, 0xD9]].concat();
    // Fill before a marker, and bytes that aren't a marker.
    assert_eq!(split_jpegs(&image(&[0xFF, 0xFF, 0xFF])), [image(&[0xFF, 0xFF, 0xFF]).as_slice()]);
    assert_eq!(split_jpegs(&image(&[0x12, 0x34])), [image(&[0x12, 0x34]).as_slice()]);
    // Markers with no length (TEM, a restart) outside the picture's data.
    assert_eq!(split_jpegs(&image(&[0xFF, 0x01, 0xFF, 0xD0])), [image(&[0xFF, 0x01, 0xFF, 0xD0]).as_slice()]);
    // What follows a whole picture: a byte alone, or something that isn't a start.
    let one = jpeg(1);
    assert_eq!(split_jpegs(&[one.as_slice(), &[0x00]].concat()), [one.as_slice()]);
    assert_eq!(split_jpegs(&[one.as_slice(), &[0xFF]].concat()), [one.as_slice()]);
    assert_eq!(split_jpegs(&[one.as_slice(), &[0x00, 0xD8, 0xFF, 0xD9]].concat()), [one.as_slice()]);
    // Cut short in the data of a scan, ending in a marker's first byte.
    assert!(split_jpegs(&[0xFF, 0xD8, 0xFF, 0xDA, 0x00, 0x02, 0x12, 0xFF]).is_empty());
    assert!(split_jpegs(&[0xFF, 0xD8, 0xFF, 0xDA, 0x00, 0x02, 0x12]).is_empty());
}

/// Each picture's time in the file is read from what ffmpeg says of it
/// (storyboard.rs `grab`: "each input's first frame passed through
/// `showinfo` (which says its pts)"), the input's seek added; in MPEG-TS,
/// the last of an input's keyframes.
#[test]
fn each_pictures_time_is_read_from_what_ffmpeg_says() {
    let said = "\
[Parsed_showinfo_1 @ 0x7f] config in time_base: 1/15360, frame_rate: 30/1
[Parsed_showinfo_5 @ 0x7f] n:   0 pts: 153600 pts_time:-1      duration:    512
[Parsed_showinfo_5 @ 0x7f] color_range:unknown
[Parsed_showinfo_1 @ 0x7f] n:   0 pts:      0 pts_time:0       duration:    512";
    assert_eq!(showinfo(said, &[0.0, 19.0, 30.0], 4), [Some(0.0), Some(18.0), None]);
    // Other filters' lines, a line with no frame number, and an input that
    // isn't there are none of its business.
    let noise = "\
[Parsed_showinfo_2 @ 0x7f] n:   0 pts:      0 pts_time:99      duration:    512
[Parsed_showinfo_1 @ 0x7f] pts_time:77
[Parsed_showinfo_5 @ 0x7f] n:   0 pts:      0 pts_time:5       duration:    512
[Parsed_showinfo_1 @ 0x7f] n:   0 pts:      0 pts_time:0       duration:    512";
    assert_eq!(showinfo(noise, &[0.0], 4), [Some(0.0)]);
    let said = "\
[Parsed_showinfo_1 @ 0x7f] n:   0 pts:      0 pts_time:0       duration:    512
[Parsed_showinfo_7 @ 0x7f] n:   0 pts:  45000 pts_time:0.5     duration:    512
[Parsed_showinfo_1 @ 0x7f] n:   1 pts:  90000 pts_time:1       duration:    512
[Parsed_showinfo_7 @ 0x7f] n:   1 pts: 135000 pts_time:1.5     duration:    512";
    assert_eq!(showinfo(said, &[0.0, 3.0], 6), [Some(1.0), Some(4.5)]);
}

/// A 40 s file with a keyframe each second in `container` (`extension`), if
/// there's an ffmpeg to make one.
fn sample_in(dir: &Path, extension: &str) -> Option<(PathBuf, PathBuf)> {
    let ffmpeg = tools::find_ffmpeg(Path::new("/nonexistent"))?;
    let path = dir.join(format!("sample.{extension}"));
    let made = std::process::Command::new(&ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "testsrc=size=320x180:rate=30:duration=40"])
        .args(["-c:v", "libx264", "-preset", "ultrafast", "-g", "30", "-pix_fmt", "yuv420p", "-y"])
        .arg(&path)
        .status()
        .ok()?;
    made.success().then_some((ffmpeg, path))
}

fn sample(dir: &Path) -> Option<(PathBuf, PathBuf)> {
    sample_in(dir, "mp4")
}

/// A storyboard stopped after a batch carries on where it was, making the
/// rest and no slot twice (storyboard.rs `make`: "stopped by `stop` or
/// `between`, `Canceled`"; module doc: "carries on from where it was").
#[test]
fn a_stopped_storyboard_carries_on_where_it_was() {
    let dir = TempDir::new("make");
    let Some((ffmpeg, source)) = sample(&dir.0) else {
        eprintln!("no ffmpeg: skipped");
        return;
    };
    let file = dir.0.join("sample.sb");
    let stop = AtomicBool::new(false);
    let mut seen = Vec::new();
    let first = make(&ffmpeg, &source, &file, &stop, |board| {
        seen.push(board.made());
        false
    });
    assert!(first.as_ref().is_err_and(vodscribe_core::is_canceled), "{:?}", first.map(|b| b.map(|b| b.made())));
    assert_eq!(seen, [BATCH]);
    let board = Storyboard::open(&file).unwrap();
    assert_eq!((board.layout.width, board.layout.height, board.layout.slots), (160, 90, 20));
    // Halves first: the middle (20 s) and the picture's slot (5 s) are in.
    assert_eq!(board.slot(10), Some(20.0));
    assert_eq!(board.slot(board.layout.picture_slot()), Some(4.0));
    let mut batches = 0;
    let board = make(&ffmpeg, &source, &file, &stop, |_| {
        batches += 1;
        true
    })
    .unwrap()
    .unwrap();
    assert_eq!(batches, (20 - BATCH).div_ceil(BATCH), "only the rest was made");
    assert!(board.complete());
    assert_eq!(Storyboard::open(&file).unwrap().made(), 20, "no slot twice");
}

/// Each slot's picture is the keyframe at or before its time, a whole
/// second in a file with one each second, and a JPEG (storyboard.rs, module
/// doc: "each slot (every `Layout::interval` seconds) takes the keyframe at
/// or before it").
#[test]
fn each_slot_takes_the_keyframe_at_or_before_it() {
    let dir = TempDir::new("keyframes");
    let Some((ffmpeg, source)) = sample(&dir.0) else {
        eprintln!("no ffmpeg: skipped");
        return;
    };
    let board = make(&ffmpeg, &source, &dir.0.join("sample.sb"), &AtomicBool::new(false), |_| true).unwrap().unwrap();
    for slot in 0..20 {
        let pts = board.slot(slot).unwrap();
        assert!(pts <= board.layout.time(slot) + 1e-6 && pts.fract() == 0.0, "slot {slot}: {pts}");
        assert_eq!(pts, board.layout.time(slot), "a keyframe each second: the slot's own second");
    }
    let jpeg = board.frame(board.nearest(7.4).unwrap()).unwrap();
    assert_eq!(&jpeg[..2], [0xFF, 0xD8]);
}

/// In an MPEG-TS file, where a seek lands on the keyframe after the time,
/// each slot still shows the keyframe at or before it (0.6.0 Fixes:
/// "Seeking in .ts files shows the right frame. In an MPEG-TS video file,
/// moving the playhead or stepping a frame showed one from up to a few
/// seconds later, the next keyframe").
#[test]
fn in_an_mpeg_ts_file_each_slot_takes_the_keyframe_at_or_before_it_not_the_next() {
    let dir = TempDir::new("ts");
    let Some((ffmpeg, source)) = sample_in(&dir.0, "ts") else {
        eprintln!("no ffmpeg: skipped");
        return;
    };
    let board = make(&ffmpeg, &source, &dir.0.join("sample.sb"), &AtomicBool::new(false), |_| true).unwrap().unwrap();
    // The file's own times begin a little after 0 (MPEG-TS), so each slot's
    // frame is the one at or before its time and within a second of it.
    for slot in 1..board.layout.slots {
        let (time, pts) = (board.layout.time(slot), board.slot(slot).unwrap());
        assert!(pts <= time + 1e-3, "slot {slot} at {time} s shows the keyframe after it, at {pts} s");
        assert!(time - pts < 1.0 + 1e-3, "slot {slot} at {time} s shows a keyframe far before, at {pts} s");
    }
}

/// A file with no picture (sound alone) has no storyboard (storyboard.rs
/// `make`: "None if the source has no picture or no known length").
#[test]
fn a_file_with_no_picture_has_no_storyboard() {
    let dir = TempDir::new("sound");
    let Some(ffmpeg) = tools::find_ffmpeg(Path::new("/nonexistent")) else {
        eprintln!("no ffmpeg: skipped");
        return;
    };
    let source = dir.0.join("tone.m4a");
    let made = std::process::Command::new(&ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "sine=frequency=440:duration=3"])
        .args(["-c:a", "aac", "-y"])
        .arg(&source)
        .status()
        .unwrap();
    assert!(made.success());
    let file = dir.0.join("tone.sb");
    assert!(make(&ffmpeg, &source, &file, &AtomicBool::new(false), |_| true).unwrap().is_none());
}

/// A file of no length (a single frame a thousandth of a second long) has no
/// storyboard, as it has no slots (storyboard.rs `make`: "None if the source
/// has no picture or no known length").
#[test]
fn a_file_of_no_length_has_no_storyboard() {
    let dir = TempDir::new("no-length");
    let Some(ffmpeg) = tools::find_ffmpeg(Path::new("/nonexistent")) else {
        eprintln!("no ffmpeg: skipped");
        return;
    };
    let source = dir.0.join("flash.mkv");
    let made = std::process::Command::new(&ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "color=black:s=64x64:r=1000"])
        .args(["-frames:v", "1", "-c:v", "mjpeg", "-y"])
        .arg(&source)
        .status()
        .unwrap();
    assert!(made.success());
    assert!(make(&ffmpeg, &source, &dir.0.join("flash.sb"), &AtomicBool::new(false), |_| true).unwrap().is_none());
}

/// A file that goes while its storyboard is made ends the making, saying so,
/// rather than leaving slots with no frame (storyboard.rs `make`: "unless the
/// file changed under it").
#[test]
fn a_file_changed_while_its_storyboard_is_made_ends_the_making() {
    let dir = TempDir::new("changes");
    let Some((ffmpeg, source)) = sample(&dir.0) else {
        eprintln!("no ffmpeg: skipped");
        return;
    };
    let file = dir.0.join("sample.sb");
    let stop = AtomicBool::new(false);
    let ended = make(&ffmpeg, &source, &file, &stop, |_| {
        std::fs::remove_file(&source).unwrap();
        true
    });
    let error = format!("{:#}", ended.expect_err("the making ended in an error"));
    assert!(error.contains("the file changed while its storyboard was made"), "{error}");
    assert!(error.contains("No such file or directory"), "what ffmpeg said: {error}");
    let board = Storyboard::open(&file).unwrap();
    assert_eq!(board.made(), BATCH, "no slot made with none after it");
}

/// A storyboard made already is found done without a probe or a frame read
/// (storyboard.rs `make`: "Done already: no probe needed").
#[test]
fn a_made_storyboard_is_found_done_without_reading_the_file_again() {
    let dir = TempDir::new("done");
    let Some((ffmpeg, source)) = sample(&dir.0) else {
        eprintln!("no ffmpeg: skipped");
        return;
    };
    let file = dir.0.join("sample.sb");
    let stop = AtomicBool::new(false);
    make(&ffmpeg, &source, &file, &stop, |_| true).unwrap().unwrap();
    // With no ffmpeg at all, it still answers: nothing was read.
    let nothing = dir.0.join("no-ffmpeg");
    assert!(make(&nothing, &source, &file, &stop, |_| panic!("nothing to make")).unwrap().unwrap().complete());
}
