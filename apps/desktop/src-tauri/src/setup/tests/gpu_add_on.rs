//! The GPU add-on: what it says, its install counted as one download, its
//! cancel, and its removal (`gpu_status`, `install_gpu_pack`,
//! `cancel_gpu_install`, `remove_gpu_pack`). Whether the computer has a
//! card is the kit's own spec (testkit/tests.rs
//! `an_app_finds_only_the_programs_its_test_gives_it`).

use vodscribe_core::model::GPU_MODEL;
use vodscribe_core::testkit::Gate;

use super::*;

/// The GPU pack as this version's install leaves it (`gpu::is_installed`):
/// its marker says the pack's version.
fn installed_pack(app: &TestApp) {
    let pack = app.paths().gpu_pack();
    std::fs::create_dir_all(gpu::lib_dir(&pack)).unwrap();
    std::fs::write(pack.join("installed"), "2").unwrap();
    assert!(gpu::is_installed(&pack), "the pack is this version's");
}

/// What `gpu_status` says.
fn status(app: &TestApp) -> Value {
    ask(app, "gpu_status").unwrap()
}

/// The GPU add-on installed with its pack there and its model's last file
/// held as it's asked for, the others finished by an earlier try: the
/// gate it's held at, once it's there, and what the page hears of it.
fn held_model(app: &TestApp) -> (Gate, Heard, Heard) {
    installed_pack(app);
    finished_before(&app.paths().models(), &GPU_MODEL, 4);
    let asked = Gate::new();
    app.net.holds(&model_link(&GPU_MODEL, "tokens.txt"), &asked);
    let (progress, done) = (Heard::of(app, "gpu-progress"), Heard::of(app, "gpu-done"));
    ask(app, "install_gpu_pack").unwrap();
    asked.reached();
    (asked, progress, done)
}

/// The GPU add-on installed with nothing of it there and its pack's first
/// download held as it's asked for: the gate, once it's there, and its end.
fn held_pack(app: &TestApp) -> (Gate, Heard) {
    let asked = Gate::new();
    app.net.holds("https://github.com/k2-fsa/sherpa-onnx/", &asked);
    let done = Heard::of(app, "gpu-done");
    ask(app, "install_gpu_pack").unwrap();
    asked.reached();
    (asked, done)
}

/// Before it's installed, the add-on says it isn't, and what it would
/// download: the pack and the GPU speech model (setup.rs `GpuStatus`).
#[test]
fn the_add_on_says_what_it_would_download() {
    let app = app();
    let expected = json!({
        "supported": gpu::supported(),
        "installed": false,
        "installing": false,
        "progress": null,
        "gpu": null,
        "min_free_mb": gpu::MIN_FREE_MB,
        "download_bytes": gpu::download_size() + GPU_MODEL.total_bytes(),
    });
    let mut status = status(&app);
    // Whether this build has the GPU worker beside it is the build's.
    status.as_object_mut().unwrap().remove("worker");
    assert_eq!(status, expected);
}

/// It downloads only what's missing: the pack alone after an update
/// changes it, the model alone, or nothing (setup.rs `GpuStatus`: "Only
/// what's missing, such as just the pack after an update changes it";
/// CHANGELOG.md, 0.1.3: "It downloads again once after updating.").
#[test]
fn the_add_on_downloads_only_whats_missing() {
    let app = app();
    installed_pack(&app);
    assert_eq!(status(&app)["download_bytes"], json!(GPU_MODEL.total_bytes()));
    installed(&app.paths().models(), &GPU_MODEL);
    assert_eq!((&status(&app)["installed"], &status(&app)["download_bytes"]), (&json!(true), &json!(0)));
    std::fs::write(app.paths().gpu_pack().join("installed"), "1").unwrap();
    assert_eq!(
        (&status(&app)["installed"], &status(&app)["download_bytes"]),
        (&json!(false), &json!(gpu::download_size()))
    );
}

/// An install with the pack there and the model's files finished by an
/// earlier try puts them in place, saying every file matches, and its bar
/// goes once it's installed (CHANGELOG.md, 0.4.1 Fixes: "The GPU add-on's
/// progress bar could stay up after it had installed."; setup.rs
/// `install_gpu_pack`: "completion as `gpu-done` (an error message or
/// null)").
#[test]
fn an_installed_add_on_shows_no_progress() {
    let app = app();
    installed_pack(&app);
    finished_before(&app.paths().models(), &GPU_MODEL, GPU_MODEL.files.len());
    let (progress, done) = (Heard::of(&app, "gpu-progress"), Heard::of(&app, "gpu-done"));
    ask(&app, "install_gpu_pack").unwrap();
    assert_eq!(done.first(), Value::Null);
    let last = progress.last().unwrap();
    let total = GPU_MODEL.total_bytes();
    assert_eq!(last["step"], json!("Every file matches its SHA‑256"));
    assert_eq!(
        (&last["done"], &last["total"], &last["part"], &last["parts"]),
        (&json!(total), &json!(total), &json!(5), &json!(5))
    );
    let status = status(&app);
    assert_eq!(
        (&status["installed"], &status["installing"], &status["progress"]),
        (&json!(true), &json!(false), &json!(null))
    );
}

/// Settings opened again partway through the install shows where it is,
/// not an empty bar until the next file (CHANGELOG.md, 0.2.0 Fixes: "The
/// GPU add-on's progress shows when Settings is reopened mid-download.";
/// setup.rs `Reporter`).
#[test]
fn settings_opened_mid_install_shows_its_progress() {
    let _turn = turn();
    let app = app();
    let (asked, progress, done) = held_model(&app);
    let status = status(&app);
    assert_eq!(status["installing"], json!(true));
    assert_eq!(status["progress"], progress.last().unwrap());
    assert_eq!(
        (&status["progress"]["step"], &status["progress"]["part"]),
        (&json!("joiner.fp16.onnx matches its SHA‑256"), &json!(4))
    );
    let finished: u64 = GPU_MODEL.files[..4].iter().map(|f| f.size).sum();
    let counted = (&status["progress"]["done"], &status["progress"]["total"]);
    assert_eq!(counted, (&json!(finished), &json!(GPU_MODEL.total_bytes())));
    asked.open();
    done.first();
}

/// The pack and the model download as one, the bar counting the bytes and
/// files of both (setup.rs `install_gpu_pack`: "One count of bytes and of
/// files across the pack and the model, so the bar only moves forward and
/// the files read \"(1 of 10)\" to \"(10 of 10)\".").
#[test]
fn the_pack_and_model_download_as_one() {
    let _turn = turn();
    let app = app();
    if !gpu::supported() {
        return;
    }
    app.net.serves("https://github.com/k2-fsa/sherpa-onnx/", b"x".to_vec());
    let (progress, done) = (Heard::of(&app, "gpu-progress"), Heard::of(&app, "gpu-done"));
    ask(&app, "install_gpu_pack").unwrap();
    let error = done.first();
    assert!(error.as_str().is_some_and(|e| e.contains("A download was damaged or changed on the way")), "{error}");
    let first = &progress.all()[0];
    let total = gpu::download_size() + GPU_MODEL.total_bytes();
    assert_eq!(
        (&first["done"], &first["total"], &first["part"], &first["parts"]),
        (&json!(1), &json!(total), &json!(1), &json!(10))
    );
}

/// A canceled install ends without an error, and its pack starts over:
/// nothing of it is kept (CHANGELOG.md, 0.1.3: "the GPU add-on's own pack
/// starts over."; api.ts `cancelGpuInstall`: "`gpuDone` then fires with
/// null").
#[test]
fn a_canceled_install_starts_its_pack_over() {
    let _turn = turn();
    let app = app();
    if !gpu::supported() {
        return;
    }
    let (asked, done) = held_pack(&app);
    ask(&app, "cancel_gpu_install").unwrap();
    asked.open();
    assert_eq!(done.first(), Value::Null);
    let pack = app.paths().gpu_pack();
    assert!(!pack.exists() && !gpu::staging_dir(&pack).exists(), "some of the pack was kept");
    assert_eq!(status(&app)["installing"], json!(false));
}

/// A canceled install keeps the model's files it finished (CHANGELOG.md,
/// 0.1.3: "Tools and model files that finished stay").
#[test]
fn a_canceled_install_keeps_the_model_files_it_finished() {
    let _turn = turn();
    let app = app();
    let (asked, _, done) = held_model(&app);
    ask(&app, "cancel_gpu_install").unwrap();
    asked.open();
    assert_eq!(done.first(), Value::Null);
    let staging = app.paths().models().join(format!(".{}.partial", GPU_MODEL.id));
    for file in &GPU_MODEL.files[..4] {
        assert_eq!(std::fs::metadata(staging.join(file.name)).unwrap().len(), file.size, "{}", file.name);
    }
}

/// Installed again while it installs, the add-on ends once, with the
/// install under way (setup.rs `Install::spawn`: "With one running already
/// its done event comes as for this one").
#[test]
fn an_install_asked_again_while_it_runs_ends_with_it() {
    let _turn = turn();
    let app = app();
    let (asked, _, done) = held_model(&app);
    ask(&app, "install_gpu_pack").unwrap();
    asked.open();
    done.first();
    eventually("it ended", || status(&app)["installing"] == json!(false));
    assert_eq!(done.all().len(), 1, "{:?}", done.all());
}

/// Cancel with no install running doesn't stop the next one (setup.rs
/// `Install::cancel`: "With none running this sets nothing, so a click
/// can't carry over into the next download.").
#[test]
fn a_cancel_with_no_install_running_doesnt_stop_the_next() {
    let _turn = turn();
    let app = app();
    ask(&app, "cancel_gpu_install").unwrap();
    let (asked, _, done) = held_model(&app);
    asked.open();
    let ended = done.first();
    assert!(ended.as_str().is_some_and(|e| e.contains("download tokens.txt")), "it was canceled: {ended}");
}

/// The add-on isn't removed while it downloads, and says so; the download
/// goes on (setup.rs `remove_gpu_pack`: "Held till it's gone, so an install
/// can't start on it meanwhile").
#[test]
fn the_add_on_isnt_removed_while_it_downloads() {
    let _turn = turn();
    let app = app();
    let (asked, _, done) = held_model(&app);
    assert_eq!(ask(&app, "remove_gpu_pack").unwrap_err(), json!("The GPU add-on is still downloading."));
    assert!(gpu::is_installed(&app.paths().gpu_pack()));
    assert_eq!(status(&app)["installing"], json!(true));
    asked.open();
    done.first();
}

/// The add-on isn't removed while a transcription runs, which may have its
/// libraries loaded, and says so (setup.rs `remove_gpu_pack`: "A
/// transcription may have the pack's libraries loaded"; "Wait for the
/// current transcription to finish, or pause it, first.").
#[test]
fn the_add_on_isnt_removed_while_a_transcription_runs() {
    let app = app();
    installed_pack(&app);
    installed(&app.paths().models(), &GPU_MODEL);
    let held = Gate::new();
    app.worker().holds(&held);
    let vod = app.add_video_file("Day 3.mp4", 600.0);
    app.invoke("transcribe_stretch", json!({ "vodId": vod.id, "start": 0.0, "end": null })).unwrap();
    held.reached();
    let refused = ask(&app, "remove_gpu_pack").unwrap_err();
    assert_eq!(refused, json!("Wait for the current transcription to finish, or pause it, first."));
    assert_eq!(status(&app)["installed"], json!(true));
    held.open();
}

/// Removing the add-on takes its pack and its model, and it would download
/// both again; removing it when it isn't there is no error (setup.rs
/// `remove_gpu_pack`; gpu.rs `uninstall`).
#[test]
fn removing_the_add_on_takes_its_pack_and_model() {
    let app = app();
    installed_pack(&app);
    installed(&app.paths().models(), &GPU_MODEL);
    ask(&app, "remove_gpu_pack").unwrap();
    assert!(!app.paths().gpu_pack().exists() && !app.paths().gpu_model_dir().exists());
    let status = status(&app);
    assert_eq!(status["installed"], json!(false));
    assert_eq!(status["download_bytes"], json!(gpu::download_size() + GPU_MODEL.total_bytes()));
    ask(&app, "remove_gpu_pack").unwrap();
}
