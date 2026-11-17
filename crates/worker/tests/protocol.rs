//! The specs of the worker process, run as the app runs it: its arguments,
//! the JSON lines it prints, and how it stops. The ones that need the speech
//! model download the CPU model into a folder of their own under this
//! build's `tmp` (never the app's models folder), once for the whole run;
//! they skip, saying so, where there's no network or no ffmpeg. A spec each,
//! named as it is said.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;

use serde_json::{Value, json};
use vodscribe_core::asr::{Parallelism, Provider, Transcriber};
use vodscribe_core::audio::{self, Chunk, Chunker};
use vodscribe_core::model::{self, CPU_MODEL};
use vodscribe_core::testkit::TempDir;

const WORKER: &str = env!("CARGO_BIN_EXE_vodscribe-worker");

/// Apollo 11's landing, five seconds: "Houston, Tranquility Base here. The
/// Eagle has landed." (NASA, public domain; tests/e2e/fixtures).
fn eagle() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/e2e/fixtures/eagle-has-landed.mp3")
}
const EAGLE_SECONDS: f64 = 5.364;

fn ffmpeg() -> Option<PathBuf> {
    let found = which_ffmpeg();
    if found.is_none() {
        eprintln!("skipped: no ffmpeg on PATH");
    }
    found
}

fn which_ffmpeg() -> Option<PathBuf> {
    std::env::var_os("PATH")?.to_str()?.split(':').map(|d| Path::new(d).join("ffmpeg")).find(|p| p.is_file())
}

/// The CPU model, downloaded once into this build's own folder (the files
/// that finished stay for the next run); None, saying so, where it can't be.
fn model_dir() -> Option<&'static Path> {
    static MODEL: OnceLock<Option<PathBuf>> = OnceLock::new();
    MODEL
        .get_or_init(|| {
            let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("models");
            std::fs::create_dir_all(&root).ok()?;
            // The server drops a long download now and then; the files that
            // finished stay, so each try goes on from there.
            let mut last = None;
            for _ in 0..6 {
                match model::install(&root, &CPU_MODEL, &AtomicBool::new(false), |_| {}) {
                    Ok(dir) => return Some(dir),
                    Err(e) => last = Some(e),
                }
            }
            eprintln!("skipped: the speech model can't be downloaded: {:#}", last.unwrap());
            None
        })
        .as_deref()
}

fn worker() -> Command {
    let mut cmd = Command::new(WORKER);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd
}

/// The worker run with `args` to its end.
fn run(args: &[&str]) -> Output {
    worker().args(args).output().unwrap()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The lines the worker printed, each read as JSON.
fn lines(out: &Output) -> Vec<Value> {
    String::from_utf8(out.stdout.clone())
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("not JSON ({e}): {l}")))
        .collect()
}

/// A run to its end on the model, over `audio`, with `more` arguments.
fn transcribe(model: &Path, ffmpeg: &Path, audio: &Path, more: &[&str]) -> Output {
    let mut cmd = worker();
    cmd.args(["--model"]).arg(model).args(["--provider", "cpu", "--ffmpeg"]).arg(ffmpeg).args(more).arg(audio);
    cmd.output().unwrap()
}

/// The words of the `end`/`words` lines, in order, as (text, start, end).
fn words(lines: &[Value]) -> Vec<(String, f64, f64)> {
    lines
        .iter()
        .flat_map(|l| l["words"].as_array().cloned().unwrap_or_default())
        .map(|w| (w["text"].as_str().unwrap().to_string(), w["start"].as_f64().unwrap(), w["end"].as_f64().unwrap()))
        .collect()
}

fn said(words: &[(String, f64, f64)]) -> String {
    words.iter().map(|w| w.0.as_str()).collect::<Vec<_>>().join(" ").to_lowercase()
}

/// A worker started wrong says what's wrong on stderr as `error: ...`, exits
/// with 1 and prints no line (`main.rs`: "Exits non-zero on error.").
fn fails_saying(args: &[&str], said: &str) {
    let out = run(args);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(stderr(&out).starts_with(&format!("error: {said}")), "{}", stderr(&out));
    assert!(out.stdout.is_empty(), "nothing is printed");
}

#[test]
fn a_worker_given_no_model_says_it_is_required() {
    fails_saying(&["--ffmpeg", "ffmpeg", "audio.mp3"], "--model is required");
}

#[test]
fn a_worker_given_no_ffmpeg_says_it_is_required() {
    fails_saying(&["--model", "m", "audio.mp3"], "--ffmpeg is required");
}

#[test]
fn a_worker_given_no_audio_says_it_is_required() {
    fails_saying(&["--model", "m", "--ffmpeg", "ffmpeg"], "an audio file is required");
}

#[test]
fn a_worker_given_an_unknown_provider_names_it() {
    fails_saying(
        &["--model", "m", "--ffmpeg", "ffmpeg", "--provider", "tpu", "audio.mp3"],
        "unknown provider Some(\"tpu\")",
    );
}

/// How an old worker answers an argument it doesn't know (a stale sidecar
/// given `--exit-on-stdin-close`): it names it, and stops.
#[test]
fn a_worker_given_an_argument_it_does_not_know_names_it() {
    fails_saying(
        &["--model", "m", "--ffmpeg", "ffmpeg", "--no-such-flag", "audio.mp3"],
        "unexpected argument --no-such-flag",
    );
}

#[test]
fn a_worker_given_a_start_that_is_no_number_says_so() {
    fails_saying(&["--model", "m", "--ffmpeg", "ffmpeg", "--start", "soon", "audio.mp3"], "--start");
}

/// The worker has no `--help`, so this is how a copy is told from a stale
/// one: it takes `--exit-on-stdin-close` (`main.rs`), going on to the
/// arguments it's missing rather than refusing the flag.
#[test]
fn a_worker_knows_to_exit_when_stdin_closes() {
    fails_saying(&["--exit-on-stdin-close", "--ffmpeg", "ffmpeg", "audio.mp3"], "--model is required");
}

/// A folder with no model in it can't be loaded: the worker says so, naming
/// the folder, and has printed no ready line (`asr.rs` `Transcriber::load`:
/// "failed to load model from <dir>"; `main.rs`: `{"ready":true}` "once the
/// model has loaded").
#[test]
fn a_model_folder_with_no_model_fails_to_load_before_ready() {
    let dir = TempDir::new("worker-no-model");
    let out = run(&["--model", dir.to_str().unwrap(), "--ffmpeg", "ffmpeg", "audio.mp3"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains(&format!("failed to load model from {}", dir.display())), "{}", stderr(&out));
    assert!(out.stdout.is_empty(), "no ready line");
}

/// The worker says it's ready once the model has loaded, then gives each
/// batch of audio as `{"end":<secs>,"words":[...]}`, its ends rising to the
/// audio's own, and what's said is what was (`main.rs`: "Prints JSON lines
/// to stdout: `{"ready":true}` once the model has loaded, then
/// `{"end":<secs>,"words":[...]}` per batch of audio.").
#[test]
fn a_worker_says_ready_then_the_words_batch_by_batch() {
    let (Some(ffmpeg), Some(model)) = (ffmpeg(), model_dir()) else { return };
    let out = transcribe(model, &ffmpeg, &eagle(), &[]);
    assert!(out.status.success(), "{}", stderr(&out));
    let lines = lines(&out);
    assert_eq!(lines[0], json!({ "ready": true }));
    let batches = &lines[1..];
    assert!(!batches.is_empty());
    for line in batches {
        let mut keys: Vec<&String> = line.as_object().unwrap().keys().collect();
        keys.sort();
        assert_eq!(keys, ["end", "words"], "{line}");
    }
    let ends: Vec<f64> = batches.iter().map(|l| l["end"].as_f64().unwrap()).collect();
    assert!(ends.windows(2).all(|w| w[0] < w[1]), "{ends:?}");
    assert!((ends.last().unwrap() - EAGLE_SECONDS).abs() < 0.1, "{ends:?}");
    let words = words(batches);
    assert!(said(&words).contains("has landed"), "{}", said(&words));
    for (text, start, end) in &words {
        assert!(0.0 <= *start && start <= end && *end <= EAGLE_SECONDS + 0.5, "{text}: {start}..{end}");
    }
    assert!(words.windows(2).all(|w| w[0].1 <= w[1].1), "in order of time");
}

/// From a start, the words are in the file's own seconds and none comes
/// from before it, so a transcript resumed carries on where it stopped
/// (`main.rs`: `--start <secs>`, "Resume point, in seconds").
#[test]
fn a_worker_started_partway_says_words_in_the_files_seconds() {
    let (Some(ffmpeg), Some(model)) = (ffmpeg(), model_dir()) else { return };
    let whole = words(&lines(&transcribe(model, &ffmpeg, &eagle(), &[])));
    let landed =
        whole.iter().find(|w| w.0.to_lowercase().starts_with("landed")).expect("the whole says landed").clone();
    let from = (landed.1 - 2.0).max(0.5);
    let out = transcribe(model, &ffmpeg, &eagle(), &["--start", &format!("{from}")]);
    assert!(out.status.success(), "{}", stderr(&out));
    let part = words(&lines(&out));
    assert!(part.iter().all(|w| w.1 >= from - 0.1), "{part:?} from {from}");
    let again = part.iter().find(|w| w.0.to_lowercase().starts_with("landed")).unwrap_or_else(|| panic!("{part:?}"));
    assert!((again.1 - landed.1).abs() < 0.5, "landed at {} then, {} now", landed.1, again.1);
}

/// To an end, the worker stops there: its last batch ends there and nothing
/// said after it is given (`main.rs`: `--end <secs>`, "Where to stop").
#[test]
fn a_worker_given_an_end_stops_there() {
    let (Some(ffmpeg), Some(model)) = (ffmpeg(), model_dir()) else { return };
    let out = transcribe(model, &ffmpeg, &eagle(), &["--end", "3"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let lines = lines(&out);
    let last = lines.last().unwrap()["end"].as_f64().unwrap();
    assert!((last - 3.0).abs() < 0.1, "ends at {last}");
    assert!(words(&lines).iter().all(|w| w.1 < 3.0), "{:?}", words(&lines));
}

/// An audio file that isn't there fails the run with ffmpeg's words, after
/// ready (`main.rs`: "Exits non-zero on error.").
#[test]
fn a_worker_given_audio_that_is_not_there_fails() {
    let (Some(ffmpeg), Some(model)) = (ffmpeg(), model_dir()) else { return };
    let dir = TempDir::new("worker-no-audio");
    let out = transcribe(model, &ffmpeg, &dir.join("gone.mp3"), &[]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(lines(&out), [json!({ "ready": true })]);
    assert!(stderr(&out).starts_with("error: ffmpeg failed"), "{}", stderr(&out));
}

/// A hour of sound, so a worker given it is still at work when the test
/// closes its stdin.
fn long_audio(dir: &Path, ffmpeg: &Path) -> PathBuf {
    let path = dir.join("long.wav");
    let made = Command::new(ffmpeg)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", "anoisesrc=d=3600:sample_rate=16000:amplitude=0.1"])
        .args(["-ac", "1"])
        .arg(&path)
        .status()
        .unwrap();
    assert!(made.success());
    path
}

fn alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
        && std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| !s.contains(") Z "))
}

fn stopped(child: &mut Child) -> bool {
    child.try_wait().unwrap().is_some()
}

/// With `--exit-on-stdin-close` the worker stops once its stdin closes, as
/// it does when the app is gone, and the ffmpeg it started goes too
/// (`main.rs`: "when it closes, the app is gone ... and the worker stops at
/// once"; the `run` comment: "and its ffmpeg after it").
#[cfg(unix)]
#[test]
fn a_worker_stops_with_its_ffmpeg_when_its_stdin_closes() {
    use std::os::unix::fs::PermissionsExt;
    let (Some(ffmpeg), Some(model)) = (ffmpeg(), model_dir()) else { return };
    let dir = TempDir::new("worker-stdin");
    let audio = long_audio(&dir, &ffmpeg);
    // The ffmpeg it starts says which process it is, then is ffmpeg.
    let pidfile = dir.join("ffmpeg.pid");
    let wrapper = dir.join("ffmpeg-wrapper");
    std::fs::write(
        &wrapper,
        format!("#!/bin/sh\necho $$ > '{}'\nexec '{}' \"$@\"\n", pidfile.display(), ffmpeg.display()),
    )
    .unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();

    let mut child = worker()
        .args(["--exit-on-stdin-close", "--provider", "cpu", "--model"])
        .arg(model)
        .arg("--ffmpeg")
        .arg(&wrapper)
        .arg(&audio)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    // Ready, then a first batch: it's at work, its ffmpeg running.
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    for _ in 0..2 {
        line.clear();
        out.read_line(&mut line).unwrap();
        assert!(!line.is_empty(), "the worker ended early");
    }
    let ffmpeg_pid: u32 = std::fs::read_to_string(&pidfile).unwrap().trim().parse().unwrap();
    assert!(alive(ffmpeg_pid));
    drop(child.stdin.take());
    vodscribe_core::testkit::eventually("the worker stopped", || stopped(&mut child));
    assert_eq!(child.wait().unwrap().code(), Some(1));
    vodscribe_core::testkit::eventually("its ffmpeg stopped", || !alive(ffmpeg_pid));
    let mut rest = String::new();
    let _ = out.read_to_string(&mut rest);
}

/// By default the app's cores but two are used, and a decode is given what
/// that leaves (`asr.rs` `Parallelism::auto`: "Uses all cores but two,
/// leaving room for the UI and the download."), a chunk at a time on the CPU
/// (`batch`: "so the CPU uses 1").
#[test]
fn the_cpu_uses_all_cores_but_two_a_chunk_at_a_time() {
    let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
    let budget = cores.saturating_sub(2).max(1);
    let p = Parallelism::auto();
    assert_eq!(p.batch, 1);
    assert!(p.workers >= 1 && p.threads >= 1);
    assert!(p.workers * p.threads <= budget.max(1), "{p:?} of {budget}");
    if budget < 8 {
        assert_eq!((p.workers, p.threads), (1, budget), "few cores: one decode with all of them");
    } else {
        assert_eq!(p.threads, 4, "many cores: decodes of four threads");
        assert_eq!(p.workers, (budget / 4).min(6));
    }
}

/// Eagle's chunks, as the worker cuts them (short ones, so a few).
fn eagle_chunks(ffmpeg: &Path) -> Vec<Chunk> {
    let mut chunker = Chunker::new(1.0, 1.5, 0.0);
    let mut chunks = Vec::new();
    audio::decode(ffmpeg, &eagle(), 0.0, None, &AtomicBool::new(false), |s| {
        chunks.extend(chunker.push(s));
        Ok(())
    })
    .unwrap();
    chunks.extend(chunker.finish());
    chunks
}

fn transcribed(transcriber: &Transcriber, chunks: Vec<Chunk>) -> Vec<(f64, String)> {
    let (tx, rx) = std::sync::mpsc::channel();
    for chunk in chunks {
        tx.send(chunk).unwrap();
    }
    drop(tx);
    let mut out = Vec::new();
    transcriber
        .transcribe_stream(rx, |end, words| {
            out.push((end, words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ")));
            Ok(())
        })
        .unwrap();
    out
}

/// Chunks decoded at once by several workers still come out in order, each
/// with its end and the words it alone has, as one at a time gives them
/// (`asr.rs` `Transcriber::transcribe_stream`: "handing each one's end (in
/// seconds) and words to `out`, in order").
#[test]
fn chunks_decoded_at_once_come_out_in_order() {
    let (Some(ffmpeg), Some(model)) = (ffmpeg(), model_dir()) else { return };
    let chunks = || eagle_chunks(&ffmpeg);
    assert!(chunks().len() >= 4);
    let one = Transcriber::load(model, Provider::Cpu, Parallelism { workers: 1, threads: 2, batch: 1 }).unwrap();
    let many = Transcriber::load(model, Provider::Cpu, Parallelism { workers: 3, threads: 1, batch: 1 }).unwrap();
    let (alone, together) = (transcribed(&one, chunks()), transcribed(&many, chunks()));
    assert!(alone.windows(2).all(|w| w[0].0 < w[1].0), "{alone:?}");
    assert_eq!(together, alone);
}

/// A reader that fails stops the transcription with its error, and isn't
/// handed more (`asr.rs` `Transcriber::transcribe_stream`: "Stops early if
/// `out` fails, with its error.").
#[test]
fn a_failing_reader_stops_the_transcription_with_its_error() {
    let (Some(ffmpeg), Some(model)) = (ffmpeg(), model_dir()) else { return };
    for workers in [1, 3] {
        let transcriber =
            Transcriber::load(model, Provider::Cpu, Parallelism { workers, threads: 1, batch: 1 }).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        for chunk in eagle_chunks(&ffmpeg) {
            tx.send(chunk).unwrap();
        }
        drop(tx);
        let mut calls = 0;
        let err = transcriber
            .transcribe_stream(rx, |_, _| {
                calls += 1;
                anyhow::bail!("the app is gone")
            })
            .unwrap_err();
        assert_eq!((format!("{err:#}"), calls), ("the app is gone".to_string(), 1), "{workers} workers");
    }
}

/// The chunks kept ready ahead are twice what's decoded at a time (`asr.rs`
/// `Transcriber::queue_size`: "How many decoded chunks to keep ready").
#[test]
fn twice_the_chunks_in_hand_are_kept_ready() {
    let Some(model) = model_dir() else { return };
    let transcriber =
        Transcriber::load(model, Provider::Cpu, Parallelism { workers: 3, threads: 1, batch: 2 }).unwrap();
    assert_eq!(transcriber.queue_size(), 12);
}

/// The kit's fake worker gives the lines the real one does: an `end` and its
/// `words` each (text, start, end), the words in the file's seconds from the
/// start asked for to the end, and a failure on stderr with exit code 1 for
/// audio that isn't there. (The real one prints `{"ready":true}` first,
/// which `pipeline`'s reader ignores, and its batches follow the audio's
/// chunks where the fake's are a minute.)
#[test]
fn the_fake_worker_gives_the_lines_the_real_worker_gives() {
    use vodscribe_core::testkit::media::Media;
    use vodscribe_core::testkit::worker::Worker;
    use vodscribe_core::tools;
    use vodscribe_core::transcript::Word;
    let (Some(ffmpeg), Some(model)) = (ffmpeg(), model_dir()) else { return };

    let real = lines(&transcribe(model, &ffmpeg, &eagle(), &[]));
    let real_words = words(&real);
    let dir = TempDir::new("worker-fake");
    let fake = Worker::at(dir.join("vodscribe-worker"));
    fake.hears(
        real_words.iter().map(|(text, start, end)| Word { text: text.clone(), start: *start, end: *end }).collect(),
    );
    let audio = dir.join("audio.m4a");
    Media::sound(EAGLE_SECONDS).write(&audio);
    let mut cmd = tools::command(fake.path());
    cmd.arg(&audio);
    let out = tools::output(&mut cmd).unwrap();
    assert!(out.status.success());
    let fake_lines: Vec<Value> =
        String::from_utf8(out.stdout).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();

    // The keys of each batch and of each word in it, whichever worker gave them.
    let shape = |lines: &[Value]| -> std::collections::BTreeSet<Vec<String>> {
        let keys = |v: &Value| {
            let mut keys: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
            keys.sort();
            keys
        };
        let batches = lines.iter().filter(|l| l.get("end").is_some());
        batches.flat_map(|l| [keys(l)].into_iter().chain(l["words"].as_array().unwrap().iter().map(keys))).collect()
    };
    assert_eq!(shape(&real), shape(&fake_lines));
    assert_eq!(words(&fake_lines), real_words, "the words the fake was told are the words said");
    let fake_end = fake_lines.last().unwrap()["end"].as_f64().unwrap();
    assert!((fake_end - EAGLE_SECONDS).abs() < 0.1, "ends at the audio's end, as the real one: {fake_end}");

    // Audio that isn't there: exit 1 with something on stderr, in both.
    let gone = dir.join("gone.m4a");
    let mut cmd = tools::command(fake.path());
    cmd.arg(&gone);
    let out = tools::output(&mut cmd).unwrap();
    let real_out = transcribe(model, &ffmpeg, &gone, &[]);
    assert_eq!(out.status.code(), real_out.status.code());
    assert!(!out.stderr.is_empty() && !real_out.stderr.is_empty());
}
