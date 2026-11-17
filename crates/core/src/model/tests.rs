//! The specs of the speech models' files: which are installed, how one is
//! downloaded and checked, how a stopped one carries on, and which are
//! removed. A spec each, named as it
//! is said. Models here are small made-up ones, served by the fake network
//! at the address the real ones are fetched from.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use sha2::{Digest, Sha256};

use super::*;
use crate::testkit::Gate;
use crate::testkit::net::{Net, Reply};
use crate::{TempDir, is_canceled};

/// A model of these files, as the real ones are specced: each pinned by its
/// size and SHA-256. Its id is the test's own, so the fake network's answers
/// for it are no other test's.
fn model(id: &str, files: &[(&'static str, &[u8])]) -> &'static ModelSpec {
    let files: Vec<ModelFile> = files
        .iter()
        .map(|(name, bytes)| ModelFile {
            name,
            size: bytes.len() as u64,
            sha256: Box::leak(
                Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect::<String>().into_boxed_str(),
            ),
        })
        .collect();
    Box::leak(Box::new(ModelSpec {
        id: Box::leak(id.to_string().into_boxed_str()),
        name: "A test model",
        files: Box::leak(files.into_boxed_slice()),
    }))
}

fn url(spec: &ModelSpec, file: &str) -> String {
    format!("{MODELS_BASE_URL}/{}/{file}", spec.id)
}

/// The network serving `files` of `spec`.
fn serving(net: &Net, spec: &ModelSpec, files: &[(&str, &[u8])]) {
    for (name, bytes) in files {
        net.serves(&url(spec, name), *bytes);
    }
}

fn never() -> AtomicBool {
    AtomicBool::new(false)
}

const FILES: &[(&str, &[u8])] =
    &[("encoder.int8.onnx", b"the encoder"), ("decoder.int8.onnx", b"the decoder"), ("tokens.txt", b"a b c")];

/// Installing downloads every file of the model from the model bucket, as
/// `<bucket>/<model id>/<file>`, into the model's folder, and says each one
/// it checked (`model.rs` `install`: "Downloads the model's files, verifying
/// each."; Progress: "<name> matches its SHA-256").
#[test]
fn installing_downloads_each_file_into_the_models_folder() {
    let (root, net) = (TempDir::new("model-install"), Net::new());
    let spec = model("installs-each-file", FILES);
    serving(&net, spec, FILES);
    let mut steps = Vec::new();
    let dir =
        install(&root, spec, &never(), |p| steps.push((p.step.to_string(), p.done, p.total, p.part, p.parts))).unwrap();
    assert_eq!(dir, root.join(spec.id));
    for (name, bytes) in FILES {
        assert_eq!(fs::read(dir.join(name)).unwrap(), *bytes, "{name}");
    }
    assert!(is_installed(&dir, spec));
    assert!(!root.join(format!(".{}.partial", spec.id)).exists(), "the staging folder is gone");
    for (i, (name, _)) in FILES.iter().enumerate() {
        let said = format!("{name} matches its SHA\u{2011}256");
        assert!(
            steps.iter().any(|(s, _, _, part, parts)| *s == said && *part == i + 1 && *parts == FILES.len()),
            "{said}: {steps:?}"
        );
    }
    let total = spec.total_bytes();
    let (_, done, of, ..) = steps.last().unwrap();
    assert_eq!((*done, *of), (total, Some(total)), "the bar ends full");
}

/// A model already there is left as it is: nothing is downloaded (`model.rs`
/// `install`). With no answer for its files, a download would fail.
#[test]
fn an_installed_model_is_not_downloaded_again() {
    let (root, _net) = (TempDir::new("model-again"), Net::new());
    let spec = model("installed-already", FILES);
    let dir = root.join(spec.id);
    fs::create_dir_all(&dir).unwrap();
    for (name, bytes) in FILES {
        fs::write(dir.join(name), bytes).unwrap();
    }
    let mut steps = 0;
    assert_eq!(install(&root, spec, &never(), |_| steps += 1).unwrap(), dir);
    assert_eq!(steps, 0);
}

/// A model is installed when all its files are there at their sizes, and a
/// missing one or one of another size says it isn't (`model.rs`
/// `is_installed`: "Present with the expected sizes.").
#[test]
fn a_model_is_installed_when_every_file_is_there_at_its_size() {
    let root = TempDir::new("model-installed");
    let spec = model("installed-by-size", FILES);
    let dir = root.join("m");
    fs::create_dir_all(&dir).unwrap();
    assert!(!is_installed(&dir, spec), "nothing there");
    for (name, bytes) in FILES {
        fs::write(dir.join(name), bytes).unwrap();
    }
    assert!(is_installed(&dir, spec));
    fs::write(dir.join("tokens.txt"), b"a b").unwrap();
    assert!(!is_installed(&dir, spec), "a file of another size");
    fs::write(dir.join("tokens.txt"), b"a b c").unwrap();
    fs::remove_file(dir.join("encoder.int8.onnx")).unwrap();
    assert!(!is_installed(&dir, spec), "a file missing");
}

/// Only sizes are looked at once it's there: "Hashes are checked when
/// downloading." (`model.rs` `is_installed`).
#[test]
fn an_installed_model_s_files_are_not_hashed_again() {
    let root = TempDir::new("model-sizes-only");
    let spec = model("sizes-only", FILES);
    for (name, bytes) in FILES {
        fs::write(root.join(name), vec![b'x'; bytes.len()]).unwrap();
    }
    assert!(is_installed(&root, spec));
}

/// A file that arrives changed is refused and thrown away, and no model is
/// left installed (`net.rs` `download_resumable`: "Its size and SHA-256 are
/// checked over the whole file before it's renamed to `dest`, so nothing at
/// `dest` was ever unchecked"; the message: "A download was damaged or
/// changed on the way").
#[test]
fn a_file_that_does_not_match_its_hash_is_thrown_away() {
    let (root, net) = (TempDir::new("model-damaged"), Net::new());
    let spec = model("damaged-file", FILES);
    serving(&net, spec, &FILES[..2]);
    // Same length as it should be, other bytes: only its hash can tell.
    net.serves(&url(spec, "tokens.txt"), b"a b d".as_slice());
    let err = install(&root, spec, &never(), |_| {}).unwrap_err();
    assert!(format!("{err:#}").contains("damaged or changed"), "{err:#}");
    assert!(!root.join(spec.id).exists(), "no model installed");
    let staging = root.join(format!(".{}.partial", spec.id));
    assert!(!staging.join("tokens.txt").exists(), "the damaged file isn't kept");
    assert!(!net::part_of(&staging.join("tokens.txt")).exists(), "nor what it had of it");
}

/// A canceled install ends as a cancel, keeps the files that finished, and
/// the next one downloads only the rest (CHANGELOG.md, 0.1.3: "Setup and
/// the GPU add-on downloads have a Cancel button. Tools and model files that
/// finished stay"; `model.rs` `install`: "Files finished by an earlier,
/// interrupted attempt are kept.").
#[test]
fn a_canceled_install_keeps_the_files_that_finished() {
    let (root, net) = (TempDir::new("model-cancel"), Net::new());
    let spec = model("canceled-install", FILES);
    let cancel = Arc::new(AtomicBool::new(false));
    let asked: Arc<[AtomicUsize; 3]> = Arc::new(Default::default());
    for (i, (name, bytes)) in FILES.iter().enumerate() {
        let (cancel, asked) = (cancel.clone(), asked.clone());
        net.replies(&url(spec, name), move |_| {
            asked[i].fetch_add(1, Ordering::SeqCst);
            // The second file is asked for, and the user cancels as it comes.
            if i == 1 && asked[1].load(Ordering::SeqCst) == 1 {
                cancel.store(true, Ordering::SeqCst);
            }
            Some(Reply::body(*bytes))
        });
    }
    let err = install(&root, spec, &cancel, |_| {}).unwrap_err();
    assert!(is_canceled(&err), "{err:#}");
    assert!(!root.join(spec.id).exists(), "not installed");
    let staging = root.join(format!(".{}.partial", spec.id));
    assert_eq!(fs::read(staging.join("encoder.int8.onnx")).unwrap(), b"the encoder", "the first file stays");

    cancel.store(false, Ordering::SeqCst);
    install(&root, spec, &cancel, |_| {}).unwrap();
    assert!(is_installed(&root.join(spec.id), spec));
    let asked: Vec<usize> = asked.iter().map(|n| n.load(Ordering::SeqCst)).collect();
    assert_eq!(asked, [1, 2, 1], "the finished file isn't downloaded again");
}

/// Files an interrupted attempt left, at their sizes, are used as they are,
/// and the rest are downloaded (`model.rs` `install`: "Files finished by an
/// earlier, interrupted attempt are kept.").
#[test]
fn files_an_earlier_attempt_finished_are_not_downloaded_again() {
    let (root, net) = (TempDir::new("model-resume"), Net::new());
    let spec = model("resumed-install", FILES);
    let staging = root.join(format!(".{}.partial", spec.id));
    fs::create_dir_all(&staging).unwrap();
    fs::write(staging.join("encoder.int8.onnx"), b"the encoder").unwrap();
    // Nothing answers for it: asked for, it would fail.
    serving(&net, spec, &FILES[1..]);
    let dir = install(&root, spec, &never(), |_| {}).unwrap();
    assert_eq!(fs::read(dir.join("encoder.int8.onnx")).unwrap(), b"the encoder");
    assert!(is_installed(&dir, spec));
}

/// Stopped partway through a file, the next install keeps the files it
/// finished and asks for the rest of that one from the byte it had
/// (`model.rs` `install`: "the file it was downloading carries on from the
/// bytes it had (`net::download_resumable`)").
#[test]
fn a_model_install_stopped_partway_carries_on_from_the_bytes_it_had() {
    let root = TempDir::new("model-carries-on");
    let base = "https://model-carries-on.example/models";
    let (a, b): (Vec<u8>, Vec<u8>) = (vec![1u8; 3_000], (0..9_000u32).map(|n| n as u8).collect());
    let spec = model("carries-on", &[("a.onnx", &a), ("b.weights", &b)]);
    let net = Net::new();
    let asked = net.records(base);
    net.serves(&format!("{base}/{}/a.onnx", spec.id), a.clone());
    net.serves(&format!("{base}/{}/b.weights", spec.id), b.clone());
    let gate = Gate::new();
    net.holds_partway(&format!("{base}/{}/b.weights", spec.id), 5_000, &gate);
    let cancel = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicU64::new(0));
    let (stop, dir, seen) = (cancel.clone(), root.to_path_buf(), done.clone());
    let first =
        std::thread::spawn(move || install_from(base, &dir, spec, &stop, |p| seen.store(p.done, Ordering::Relaxed)));
    gate.reached();
    // What came before the gate written: the first file and 5000 bytes of the second.
    crate::testkit::eventually("8000 bytes in", || done.load(Ordering::Relaxed) >= 8_000);
    cancel.store(true, Ordering::Relaxed);
    assert!(is_canceled(&first.join().unwrap().unwrap_err()));
    net.let_go();
    let dest = install_from(base, &root, spec, &never(), |_| {}).unwrap();
    assert!(is_installed(&dest, spec));
    assert_eq!(fs::read(dest.join("b.weights")).unwrap(), b);
    let ranges: Vec<(String, Option<String>)> = asked
        .requests()
        .into_iter()
        .map(|r| (r.url.rsplit('/').next().unwrap().to_string(), r.header("Range").map(String::from)))
        .collect();
    assert_eq!(
        ranges,
        [("a.onnx".into(), None), ("b.weights".into(), None), ("b.weights".into(), Some("bytes=5000-".into()))]
    );
}

/// What an install had goes with Cancel's discard, and removing the model
/// takes the installed files too (`model.rs` `discard`: "Deletes what an
/// install of `spec` had downloaded so far (Cancel's)."; `uninstall`:
/// "Removes `spec`'s files, installed or on their way").
#[test]
fn discarding_or_removing_a_model_deletes_its_files() {
    let root = TempDir::new("model-discard");
    let spec = model("discarded", FILES);
    fs::create_dir_all(staging_dir(&root, spec)).unwrap();
    discard(&root, spec).unwrap();
    assert!(!staging_dir(&root, spec).exists());
    fs::create_dir_all(model_dir(&root, spec)).unwrap();
    fs::create_dir_all(staging_dir(&root, spec)).unwrap();
    uninstall(&root, spec).unwrap();
    assert!(!staging_dir(&root, spec).exists() && !model_dir(&root, spec).exists());
}

/// Removing the others deletes the model folders not kept, such as the
/// sherpa-onnx release earlier versions used, and leaves the kept, a folder
/// an install is staging in, and anything that isn't a folder (`model.rs`
/// `remove_others`: "Deletes model dirs that aren't in `keep`").
#[test]
fn removing_the_others_keeps_the_models_asked_for() {
    let root = TempDir::new("model-others");
    let (cpu, gpu) = (model("others-cpu", FILES), model("others-gpu", FILES));
    for dir in [
        "others-cpu",
        "others-gpu",
        "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8",
        "something-else",
        ".others-cpu.partial",
    ] {
        fs::create_dir_all(root.join(dir)).unwrap();
        fs::write(root.join(dir).join("file"), b"x").unwrap();
    }
    fs::write(root.join("notes.txt"), b"x").unwrap();
    remove_others(&root, &[cpu, gpu]);
    let mut left: Vec<String> =
        fs::read_dir(&*root).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    left.sort();
    assert_eq!(left, [".others-cpu.partial", "notes.txt", "others-cpu", "others-gpu"]);
    assert!(root.join("others-cpu/file").exists(), "a kept model keeps its files");
}

/// With no models folder yet there is nothing to remove, and it's no
/// error (`model.rs` `remove_others`).
#[test]
fn removing_the_others_where_there_is_no_folder_does_nothing() {
    let root = TempDir::new("model-no-folder");
    remove_others(&root.join("not-there"), &[&CPU_MODEL]);
    assert!(!root.join("not-there").exists());
}

/// A transcript is current when one of ours made it, the CPU's or the
/// GPU's, or it was imported, and not when an older model did (`model.rs`
/// `is_current`: "Whether transcripts from model `id` are as good as this
/// version produces. An imported one counts").
#[test]
fn a_transcript_is_current_when_our_models_or_an_import_made_it() {
    assert!(is_current(CPU_MODEL.id));
    assert!(is_current(GPU_MODEL.id));
    assert!(is_current(IMPORTED_MODEL));
    assert!(!is_current("sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8"));
    assert!(!is_current(""));
}

/// The CPU's model is int8 and the GPU's fp16, each with the three parts
/// and the tokens the worker loads, in the one precision its folder holds
/// (`asr.rs` `Transcriber::load`: "A model dir holds one precision"), and
/// its size is its files' (`model.rs` `ModelSpec::total_bytes`).
#[test]
fn each_model_holds_the_parts_the_worker_loads_in_one_precision() {
    for (spec, precision) in [(&CPU_MODEL, "int8"), (&GPU_MODEL, "fp16")] {
        let names: Vec<&str> = spec.files.iter().map(|f| f.name).collect();
        for part in ["encoder", "decoder", "joiner"] {
            assert!(names.contains(&format!("{part}.{precision}.onnx").as_str()), "{}: {part}", spec.id);
        }
        assert!(names.contains(&"tokens.txt"), "{}", spec.id);
        assert!(spec.files.iter().all(|f| f.size > 0 && f.sha256.len() == 64), "{}", spec.id);
        assert_eq!(spec.total_bytes(), spec.files.iter().map(|f| f.size).sum::<u64>());
    }
    assert_ne!(CPU_MODEL.id, GPU_MODEL.id);
    assert_eq!(model_dir(Path::new("/models"), &CPU_MODEL), Path::new("/models").join(CPU_MODEL.id));
}
