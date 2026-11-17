//! The specs of peaks.rs: a VOD's loudness over time, ten levels a second,
//! read from a file here, saved whole, put at its place by a section's
//! stretch, and tidied.

use super::*;
use crate::testkit::{Ffmpeg, Media};

/// Ten levels a second, each the loudness (RMS) of its tenth of a second on
/// a log scale from 60 dB below full scale (0) to full scale (255) (peaks.rs,
/// module doc: "one byte per tenth of a second"; `Levels::flush`: "its
/// loudness (RMS) on a log scale … as loudness is heard").
#[test]
fn loudness_is_a_level_for_each_tenth_of_a_second() {
    let mut levels = Levels::default();
    // Silence, then a full-scale square wave, then a 60 dB quieter one,
    // fed in uneven pieces, then half a window.
    let mut samples = vec![0.0; Levels::WINDOW];
    samples.extend((0..Levels::WINDOW).map(|i| if i % 2 == 0 { 1.0 } else { -1.0 }));
    samples.extend((0..Levels::WINDOW).map(|i| if i % 2 == 0 { 0.001 } else { -0.001 }));
    samples.extend((0..Levels::WINDOW / 2).map(|_| 0.1));
    for piece in samples.chunks(777) {
        levels.push(piece);
    }
    assert_eq!(levels.finish(), [0, 255, 0, 170], "-20 dB is two thirds of the way up");
}

/// A stretch transcribed from a section of its own adds that section's
/// levels at their place, zeros where nothing was read (peaks.rs, module
/// doc: "A stretch transcribed from a section of its own adds that
/// section's levels at their place (`merge`), with zeros where nothing was
/// read"; 0.7.4 Fixes: "A request paused or failed after its audio came
/// showed levels only up to where it stopped").
#[test]
fn a_sections_levels_go_at_their_place_among_the_saved_ones() {
    assert_eq!(merged(Vec::new(), 0.3, &[7, 8]), [0, 0, 0, 7, 8], "zeros before");
    assert_eq!(merged(vec![1, 2, 3, 4, 5, 6], 0.2, &[7, 8]), [1, 2, 7, 8, 5, 6], "over what was there");
    assert_eq!(merged(vec![1, 2, 3], 0.2, &[7, 8]), [1, 2, 7, 8], "on past its end");
    let dir = crate::TempDir::new("peaks-merge");
    merge(&dir, 3, 1.0, &[9]).unwrap();
    merge(&dir, 3, 0.0, &[4]).unwrap();
    assert_eq!(load(&dir, 3).unwrap(), [4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 9]);
}

/// Saved again, a VOD's levels are the new ones, whole, with nothing
/// written aside left (peaks.rs `save`: "Written aside and renamed, so a
/// reader never sees half a file, even with two writers at once").
#[test]
fn a_saved_loudness_replaces_the_one_before_whole() {
    let dir = crate::TempDir::new("peaks-save");
    save(&dir, 3, &[1, 2, 3]).unwrap();
    save(&dir, 4, &[4]).unwrap();
    save(&dir, 3, &[5, 6]).unwrap();
    assert_eq!(load(&dir, 3).unwrap(), [5, 6]);
    assert_eq!(load(&dir, 4).unwrap(), [4], "another VOD's is its own");
    let mut left: Vec<_> = fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
    left.sort();
    assert_eq!(left, ["3.peaks", "4.peaks"], "no file written aside is left");
}

/// Two writers at once each leave a whole file, one or the other's, never a
/// mix or half (peaks.rs `save`: "even with two writers at once").
#[test]
fn two_writers_at_once_leave_a_whole_file() {
    let dir = crate::TempDir::new("peaks-two");
    let (a, b) = (vec![1u8; 50_000], vec![2u8; 70_000]);
    std::thread::scope(|scope| {
        for _ in 0..20 {
            scope.spawn(|| save(&dir, 3, &a).unwrap());
            scope.spawn(|| save(&dir, 3, &b).unwrap());
        }
    });
    let saved = load(&dir, 3).unwrap();
    assert!(saved == a || saved == b, "{} bytes, a mix", saved.len());
}

/// Removing a VOD deletes its levels (peaks.rs `remove`).
#[test]
fn a_removed_vods_loudness_is_deleted() {
    let dir = crate::TempDir::new("peaks-remove");
    save(&dir, 3, &[1, 2, 3]).unwrap();
    save(&dir, 4, &[4]).unwrap();
    remove(&dir, 3);
    assert!(load(&dir, 3).is_none());
    assert_eq!(load(&dir, 4).unwrap(), [4]);
}

/// The start's tidying deletes the levels of VODs no longer in the library
/// (a VOD deleted while a download was writing one, say) and any left
/// half-written (peaks.rs `tidy`: "Removes the files in `dir` of VODs not
/// in `library` …, and any left half-written").
#[test]
fn tidying_deletes_the_loudness_of_vods_gone_and_what_was_left_half_written() {
    let dir = crate::TempDir::new("peaks-tidy");
    save(&dir, 3, &[1]).unwrap();
    save(&dir, 4, &[4]).unwrap();
    fs::write(dir.join("3.peaks.123.tmp"), b"half").unwrap();
    fs::write(dir.join("notes.txt"), b"x").unwrap();
    tidy(&dir, &[3]);
    let mut left: Vec<_> = fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
    left.sort();
    assert_eq!(left, ["3.peaks"]);
}

/// A file with sound is read ten levels a second, to the end (peaks.rs
/// `read`: "Reads `input`'s levels with ffmpeg"). The fake ffmpeg's sound is
/// silence, so each level is the quietest.
#[test]
fn a_file_with_sound_is_read_a_level_for_each_tenth_of_a_second() {
    let dir = crate::TempDir::new("peaks-read-fake");
    let ffmpeg = Ffmpeg::at(dir.join("bin/ffmpeg"));
    let file = dir.join("a.m4a");
    Media::sound(2.5).write(&file);
    let levels = read(ffmpeg.path(), &file, &AtomicBool::new(false)).unwrap();
    assert_eq!(levels, vec![0u8; 25]);
}

/// A video without sound has no levels, rather than failing (peaks.rs
/// `read`: "none for a video without sound (saved as such, so it isn't read
/// again each time)").
#[test]
fn a_video_without_sound_has_no_levels() {
    let dir = crate::TempDir::new("peaks-silent");
    let ffmpeg = Ffmpeg::at(dir.join("bin/ffmpeg"));
    let file = dir.join("silent.mp4");
    Media { audio: None, ..Media::video(3.0) }.write(&file);
    assert_eq!(read(ffmpeg.path(), &file, &AtomicBool::new(false)).unwrap(), Vec::<u8>::new());
}

/// A file that isn't there fails, as no levels would pass for silence
/// (peaks.rs `read`).
#[test]
fn a_file_that_is_not_there_fails() {
    let dir = crate::TempDir::new("peaks-gone");
    let ffmpeg = Ffmpeg::at(dir.join("bin/ffmpeg"));
    assert!(read(ffmpeg.path(), &dir.join("gone.mkv"), &AtomicBool::new(false)).is_err());
}

/// Loudness is read from a real file as it sounds: two seconds of tone loud,
/// then a second of silence quiet. Skipped where ffmpeg isn't installed
/// (peaks.rs, module doc: "Read once from the audio while it's on this
/// computer").
#[test]
fn a_real_file_is_loud_where_it_sounds_and_quiet_where_it_does_not() {
    let Ok(ffmpeg) = which::which("ffmpeg") else {
        eprintln!("skipped: no ffmpeg on PATH");
        return;
    };
    let dir = crate::TempDir::new("peaks-read");
    let file = dir.join("a.m4a");
    let ok = std::process::Command::new(&ffmpeg)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=2"])
        .args(["-af", "apad=pad_dur=1", "-c:a", "aac"])
        .arg(&file)
        .status()
        .unwrap();
    assert!(ok.success());
    let levels = read(&ffmpeg, &file, &AtomicBool::new(false)).unwrap();
    assert!((29..=31).contains(&levels.len()), "{}", levels.len());
    assert!(levels[5..15].iter().all(|&l| l > 150), "{levels:?}");
    assert!(levels[24..28].iter().all(|&l| l < 20), "{levels:?}");
}
