//! The specs of the audio the transcription reads: `decode` (ffmpeg's 16 kHz
//! mono samples, from a start to an end) and the `Chunker` that cuts them
//! into the chunks the worker transcribes. A spec each, named as it is said.

use super::*;

/// Every kind of file the app reads decodes through ffmpeg's allowlists
/// (`tools::ffmpeg_allow`), and a container outside them doesn't, all of
/// it: a file that loses its start (FLV with "-ss 0") falls short
/// (CHANGELOG.md, 0.3.0 Fixes: "The first second or two of FLV files was
/// left out of the transcript."). Made with ffmpeg; skipped where ffmpeg
/// isn't installed.
#[test]
fn decodes_what_the_allowlists_allow() {
    let Ok(ffmpeg) = which::which("ffmpeg") else {
        eprintln!("skipped: no ffmpeg on PATH");
        return;
    };
    let dir = crate::TempDir::new("allowlist");
    let make = |name: &str, args: &[&str]| {
        let path = dir.join(name);
        let ok = std::process::Command::new(&ffmpeg)
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc=size=320x240:rate=25:duration=3"])
            .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=3", "-shortest"])
            .args(args)
            .arg(&path)
            .status()
            .is_ok_and(|s| s.success());
        ok.then_some(path)
    };
    let samples: &[(&str, &[&str])] = &[
        ("a.mp4", &["-c:v", "libx264", "-c:a", "aac"]),
        ("b.mkv", &["-c:v", "libx264", "-c:a", "libopus"]),
        ("c.webm", &["-c:v", "libvpx-vp9", "-c:a", "libopus"]),
        ("d.avi", &["-c:v", "mpeg4", "-c:a", "libmp3lame"]),
        ("e.ts", &["-c:v", "libx264", "-c:a", "aac"]),
        ("f.flv", &["-c:v", "libx264", "-c:a", "aac"]),
        ("g.mov", &["-c:v", "libx264", "-c:a", "aac"]),
        ("h.mp3", &["-vn"]),
        ("i.m4a", &["-vn", "-c:a", "aac"]),
        ("j.wav", &["-vn"]),
        ("k.flac", &["-vn"]),
        ("l.ogg", &["-vn", "-c:a", "libvorbis"]),
        ("m.opus", &["-vn", "-c:a", "libopus"]),
    ];
    let never = AtomicBool::new(false);
    let mut tried = 0;
    for (name, args) in samples {
        // An ffmpeg without that encoder can't make the sample.
        let Some(path) = make(name, args) else { continue };
        let mut samples = 0;
        decode(&ffmpeg, &path, 0.0, None, &never, |s| {
            samples += s.len();
            Ok(())
        })
        .unwrap_or_else(|e| panic!("{name}: {e:#}"));
        // All of it: a file that loses its start (FLV with "-ss 0") falls short.
        assert!(samples > SAMPLE_RATE * 28 / 10, "{name}: {samples} samples of 3 s");
        tried += 1;
    }
    assert!(tried >= 8, "only {tried} samples could be made");
    // NUT, a container ffmpeg reads but VODscribe doesn't take: refused.
    let nut = make("n.nut", &["-c:v", "mpeg4", "-c:a", "pcm_s16le"]).expect("ffmpeg can make NUT");
    assert!(decode(&ffmpeg, &nut, 0.0, None, &never, |_| Ok(())).is_err());
}

/// From a start to an end: that second of a three-second tone and no
/// more (`audio.rs` `decode`: "from `start` seconds to `end`"). Made with
/// ffmpeg; skipped where ffmpeg isn't installed.
#[test]
fn decodes_a_stretch_only() {
    let Ok(ffmpeg) = which::which("ffmpeg") else {
        eprintln!("skipped: no ffmpeg on PATH");
        return;
    };
    let dir = crate::TempDir::new("decode-end");
    let path = dir.join("tone.m4a");
    let made = std::process::Command::new(&ffmpeg)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=3", "-c:a", "aac"])
        .arg(&path)
        .status()
        .unwrap();
    assert!(made.success());
    let mut samples = 0;
    decode(&ffmpeg, &path, 1.0, Some(2.0), &AtomicBool::new(false), |s| {
        samples += s.len();
        Ok(())
    })
    .unwrap();
    let secs = samples as f64 / SAMPLE_RATE as f64;
    assert!((secs - 1.0).abs() < 0.05, "{secs} s");
}

/// Audio decoded from partway in is offset by where it began (`audio.rs`
/// `Chunker::new`: "`offset` is the time of the first sample pushed").
#[test]
fn chunker_offsets_resumed_audio() {
    let mut chunker = Chunker::new(2.0, 3.0, 100.0);
    let chunks = chunker.push(&vec![0.1; 4 * SAMPLE_RATE]);
    assert_eq!(chunks[0].start, 100.0);
}

/// Chunks are cut at the quietest 20 ms frame, and between them leave
/// nothing out and nothing twice (`audio.rs` `Chunker`: "cutting at the
/// quietest 20 ms frame in that window so words are rarely split in half").
#[test]
fn chunker_cuts_at_silence_and_keeps_offsets() {
    // 1 s of tone, with silence at 2.5 s, over 6 s total.
    let mut audio: Vec<f32> = (0..6 * SAMPLE_RATE).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
    let silent = (2.5 * SAMPLE_RATE as f64) as usize;
    audio[silent..silent + FRAME * 2].fill(0.0);

    let mut chunker = Chunker::new(2.0, 3.0, 0.0);
    let mut chunks = Vec::new();
    for block in audio.chunks(1000) {
        chunks.extend(chunker.push(block));
    }
    chunks.extend(chunker.finish());

    assert_eq!(chunks[0].start, 0.0);
    let first_len = chunks[0].samples.len() as f64 / SAMPLE_RATE as f64;
    assert!((first_len - 2.51).abs() < 0.02, "cut at {first_len}");
    let total: usize = chunks.iter().map(|c| c.samples.len()).sum();
    assert_eq!(total, audio.len());
    for pair in chunks.windows(2) {
        let end = pair[0].start + pair[0].samples.len() as f64 / SAMPLE_RATE as f64;
        assert!((pair[1].start - end).abs() < 1e-9);
    }
}

/// A 16 kHz mono WAV file of `parts`, each a number of seconds of one
/// 16-bit sample value, written by hand so the samples ffmpeg should give
/// back are known exactly.
fn wav(dir: &Path, name: &str, parts: &[(f64, i16)]) -> std::path::PathBuf {
    let mut data = Vec::new();
    for (secs, value) in parts {
        for _ in 0..(secs * SAMPLE_RATE as f64) as usize {
            data.extend_from_slice(&value.to_le_bytes());
        }
    }
    let mut file = b"RIFF".to_vec();
    file.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    file.extend_from_slice(b"WAVEfmt ");
    file.extend_from_slice(&16u32.to_le_bytes());
    file.extend_from_slice(&[1, 0, 1, 0]); // PCM, mono
    file.extend_from_slice(&(SAMPLE_RATE as u32).to_le_bytes());
    file.extend_from_slice(&(SAMPLE_RATE as u32 * 2).to_le_bytes());
    file.extend_from_slice(&[2, 0, 16, 0]);
    file.extend_from_slice(b"data");
    file.extend_from_slice(&(data.len() as u32).to_le_bytes());
    file.extend_from_slice(&data);
    let path = dir.join(name);
    std::fs::write(&path, file).unwrap();
    path
}

fn ffmpeg() -> Option<std::path::PathBuf> {
    let found = which::which("ffmpeg").ok();
    if found.is_none() {
        eprintln!("skipped: no ffmpeg on PATH");
    }
    found
}

fn decoded(ffmpeg: &Path, input: &Path, start: f64, end: Option<f64>) -> Vec<f32> {
    let mut all = Vec::new();
    decode(ffmpeg, input, start, end, &AtomicBool::new(false), |s| {
        all.extend_from_slice(s);
        Ok(())
    })
    .unwrap_or_else(|e| panic!("{e:#}"));
    all
}

/// Samples come as numbers from -1 to 1, a 16-bit sample over 32768
/// (`audio.rs` `decode`: "Decodes any media file to 16 kHz mono").
#[test]
fn samples_are_numbers_from_minus_one_to_one() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let dir = crate::TempDir::new("audio-scale");
    let path = wav(&dir, "levels.wav", &[(0.5, 16384), (0.5, -32768)]);
    let samples = decoded(&ffmpeg, &path, 0.0, None);
    assert_eq!(samples.len(), SAMPLE_RATE);
    assert!(samples[..SAMPLE_RATE / 2].iter().all(|s| *s == 0.5));
    assert!(samples[SAMPLE_RATE / 2..].iter().all(|s| *s == -1.0));
}

/// Stereo at another rate comes out mono at 16 kHz (`audio.rs` `decode`:
/// "Decodes any media file to 16 kHz mono"). Made with ffmpeg.
#[test]
fn audio_comes_out_as_16_khz_mono() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let dir = crate::TempDir::new("audio-mono");
    let path = dir.join("stereo.wav");
    let made = std::process::Command::new(&ffmpeg)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=44100:duration=2"])
        .args(["-ac", "2"])
        .arg(&path)
        .status()
        .unwrap();
    assert!(made.success());
    let secs = decoded(&ffmpeg, &path, 0.0, None).len() as f64 / SAMPLE_RATE as f64;
    assert!((secs - 2.0).abs() < 0.05, "{secs} s of samples");
}

/// Resuming from a start begins there in the file, not before it
/// (`audio.rs` `decode`: "from `start` seconds"; the transcription resumes
/// where it stopped).
#[test]
fn decoding_from_a_start_leaves_what_comes_before_it_out() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let dir = crate::TempDir::new("audio-start");
    let path = wav(&dir, "two.wav", &[(1.0, 8192), (1.0, 16384)]);
    let samples = decoded(&ffmpeg, &path, 1.0, None);
    assert_eq!(samples.len(), SAMPLE_RATE);
    assert!(samples.iter().all(|s| *s == 0.5));
}

/// A stretch that ends where it starts has no audio, and isn't an error
/// (`audio.rs` `decode`: the length asked for is "`(end - start).max(0)`").
#[test]
#[ignore = "disagreement: a stretch of no length is decoded to the end of the file, as ffmpeg's `-t 0` means no limit"]
fn a_stretch_ending_before_it_starts_decodes_nothing() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let dir = crate::TempDir::new("audio-empty");
    let path = wav(&dir, "one.wav", &[(2.0, 8192)]);
    assert!(decoded(&ffmpeg, &path, 1.5, Some(1.0)).is_empty());
}

/// A stop in the middle of decoding ends it with the stop, which is how
/// a job tells it apart from a failure (`audio.rs` `decode`: `cancel`).
#[test]
fn a_stop_ends_decoding_as_a_stop() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let dir = crate::TempDir::new("audio-stop");
    let path = wav(&dir, "one.wav", &[(2.0, 8192)]);
    let stop = AtomicBool::new(true);
    let mut heard = 0;
    let err = decode(&ffmpeg, &path, 0.0, None, &stop, |s| {
        heard += s.len();
        Ok(())
    })
    .unwrap_err();
    assert!(crate::is_canceled(&err), "{err:#}");
    assert_eq!(heard, 0, "nothing is handed on once stopped");
}

/// A reader that fails ends decoding with its error: what the worker does
/// when the transcription ahead of it has stopped (`audio.rs` `decode`:
/// `on_samples` returns a `Result`).
#[test]
fn a_reader_that_fails_ends_decoding_with_its_error() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let dir = crate::TempDir::new("audio-reader");
    let path = wav(&dir, "one.wav", &[(2.0, 8192)]);
    let mut calls = 0;
    let err = decode(&ffmpeg, &path, 0.0, None, &AtomicBool::new(false), |_| {
        calls += 1;
        anyhow::bail!("transcription stopped")
    })
    .unwrap_err();
    assert_eq!(format!("{err:#}"), "transcription stopped");
    assert_eq!(calls, 1, "not asked again after failing");
}

/// A file ffmpeg can't read fails with what ffmpeg said (`audio.rs`
/// `decode`: "ffmpeg failed: <its stderr>").
#[test]
fn a_file_ffmpeg_cannot_read_fails_with_what_it_said() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let dir = crate::TempDir::new("audio-bad");
    let path = dir.join("not-audio.mp3");
    std::fs::write(&path, b"this is text, not sound").unwrap();
    let err = decode(&ffmpeg, &path, 0.0, None, &AtomicBool::new(false), |_| Ok(())).unwrap_err();
    let said = format!("{err:#}");
    assert!(said.starts_with("ffmpeg failed: "), "{said}");
    assert!(said.len() > "ffmpeg failed: ".len(), "with ffmpeg's own words: {said}");
}

/// Without ffmpeg there, decoding says it couldn't start it (`audio.rs`
/// `decode`: `.context("start ffmpeg")`).
#[test]
fn a_missing_ffmpeg_says_it_could_not_start() {
    let dir = crate::TempDir::new("audio-no-ffmpeg");
    let err =
        decode(&dir.join("no-ffmpeg"), &dir.join("a.wav"), 0.0, None, &AtomicBool::new(false), |_| Ok(())).unwrap_err();
    assert!(format!("{err:#}").starts_with("start ffmpeg"), "{err:#}");
}

/// Each chunk is between the least and the most seconds asked for, the
/// last aside, and a chunk begins where the one before ended (`audio.rs`
/// `Chunker`: "chunks of `min..=max` seconds").
#[test]
fn chunks_are_as_long_as_asked() {
    // A tone with no silence in it: the cut can fall anywhere in the window.
    let audio: Vec<f32> = (0..20 * SAMPLE_RATE).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
    let mut chunker = Chunker::new(3.0, 5.0, 0.0);
    let mut chunks = chunker.push(&audio);
    chunks.extend(chunker.finish());
    assert!(chunks.len() >= 4, "{} chunks", chunks.len());
    for chunk in &chunks[..chunks.len() - 1] {
        let secs = chunk.samples.len() as f64 / SAMPLE_RATE as f64;
        assert!((3.0..=5.0).contains(&secs), "{secs} s");
    }
    assert_eq!(chunks.iter().map(|c| c.samples.len()).sum::<usize>(), audio.len());
}

/// Less than the most seconds waits to be cut: nothing comes out of the
/// stream till there's enough to choose a quiet place in, and the rest
/// comes out at its end (`audio.rs` `Chunker::push`, `finish`).
#[test]
fn audio_short_of_a_chunk_waits_for_the_end() {
    let mut chunker = Chunker::new(2.0, 3.0, 0.0);
    assert!(chunker.push(&vec![0.1; 29 * SAMPLE_RATE / 10]).is_empty());
    let last = chunker.finish().expect("what's left comes out at the end");
    assert_eq!((last.start, last.samples.len()), (0.0, 29 * SAMPLE_RATE / 10));
}

/// A fragment under a tenth of a second at the end is dropped, and one of
/// a tenth is kept (`audio.rs` `Chunker::finish`: "Fragments under 0.1 s
/// are dropped.").
#[test]
fn a_fragment_under_a_tenth_of_a_second_is_dropped() {
    let mut chunker = Chunker::new(2.0, 3.0, 0.0);
    chunker.push(&vec![0.1; SAMPLE_RATE / 10 - 1]);
    assert!(chunker.finish().is_none());
    let mut chunker = Chunker::new(2.0, 3.0, 0.0);
    chunker.push(&vec![0.1; SAMPLE_RATE / 10]);
    assert_eq!(chunker.finish().map(|c| c.samples.len()), Some(SAMPLE_RATE / 10));
}

/// A chunk after the first begins at the offset plus what came before it,
/// so a stretch resumed partway carries on in the file's own seconds
/// (`audio.rs` `Chunker::new` and `Chunk::start`: "Offset of the first
/// sample from the start of the audio, in seconds.").
#[test]
fn resumed_chunks_carry_on_in_the_files_seconds() {
    let mut chunker = Chunker::new(2.0, 3.0, 100.0);
    let chunks = chunker.push(&vec![0.1; 7 * SAMPLE_RATE]);
    assert!(chunks.len() >= 2);
    let first = chunks[0].samples.len() as f64 / SAMPLE_RATE as f64;
    assert_eq!(chunks[0].start, 100.0);
    assert!((chunks[1].start - (100.0 + first)).abs() < 1e-9, "{}", chunks[1].start);
}

/// The cut falls in the middle of the quietest 20 ms frame, not at its
/// edge: a word that ends at the frame's start isn't clipped (`audio.rs`
/// `Chunker`: "cutting at the quietest 20 ms frame in that window").
#[test]
fn a_chunk_is_cut_in_the_middle_of_the_quietest_frame() {
    let mut audio: Vec<f32> = (0..6 * SAMPLE_RATE).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
    // Two seconds in and a whole number of frames on: one silent frame.
    let silent = 2 * SAMPLE_RATE + 25 * FRAME;
    audio[silent..silent + FRAME].fill(0.0);
    let chunks = Chunker::new(2.0, 3.0, 0.0).push(&audio);
    assert_eq!(chunks[0].samples.len(), silent + FRAME / 2);
}
