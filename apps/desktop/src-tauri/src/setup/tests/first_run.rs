//! Setup and the tools: what setup says the computer has, setup's run, its
//! cancel, and yt-dlp's update (`setup_status`, `run_setup`,
//! `cancel_setup`, `update_yt_dlp`, `self_updatable`).

use vodscribe_core::model::{CPU_MODEL, GPU_MODEL};
use vodscribe_core::testkit::Gate;

use super::*;

/// Where yt-dlp's latest release is downloaded from (tools.rs).
const RELEASE: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download";

/// yt-dlp's release as its site serves it: the 2026.08.19 release's
/// checksum list, changed by `change` first, and its signature; and
/// `build` as each computer's yt-dlp.
fn serves_release(app: &TestApp, change: impl FnOnce(&mut Vec<u8>), build: &[u8]) {
    let mut sums = include_bytes!("../../../../../../crates/core/tests/data/yt-dlp-SHA2-256SUMS").to_vec();
    change(&mut sums);
    let sig = include_bytes!("../../../../../../crates/core/tests/data/yt-dlp-SHA2-256SUMS.sig");
    app.net.serves(&format!("{RELEASE}/SHA2-256SUMS.sig"), &sig[..]);
    app.net.serves(&format!("{RELEASE}/SHA2-256SUMS"), sums);
    app.net.serves(&format!("{RELEASE}/yt-dlp"), build.to_vec());
}

/// A program `name` on the computer's PATH, in `dir`: an executable file.
fn on_the_path(dir: &Path, name: &str) {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    std::fs::write(&path, b"").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// Setup run with the speech model's last file held as it's asked for,
/// the others finished by an earlier try (`finished_before`): the gate
/// it's held at, once it's there, and what the page hears of it.
fn held_setup(app: &TestApp) -> (Gate, Heard, Heard) {
    finished_before(&app.paths().models(), &CPU_MODEL, 4);
    let asked = Gate::new();
    app.net.holds(&model_link(&CPU_MODEL, "tokens.txt"), &asked);
    let (progress, done) = (Heard::of(app, "setup-progress"), Heard::of(app, "setup-done"));
    ask(app, "run_setup").unwrap();
    asked.reached();
    (asked, progress, done)
}

/// Setup says what the computer has and what it would download: VODscribe's
/// own yt-dlp and ffmpeg, the speech model once it's there, and no
/// JavaScript runtime on a computer without one (setup.rs `SetupStatus`).
#[test]
fn setup_says_what_the_computer_has_and_lacks() {
    let app = app();
    let status = ask(&app, "setup_status").unwrap();
    let expected = json!({
        "yt_dlp": true,
        "yt_dlp_managed": true,
        "ffmpeg": true,
        "ffmpeg_managed": true,
        "model": false,
        "js_runtime": false,
        "js_runtime_managed": false,
        "can_install_tools": true,
        "model_bytes": CPU_MODEL.total_bytes(),
        "running": false,
    });
    assert_eq!(status, expected);
    app.lacks_yt_dlp();
    app.worker();
    let status = ask(&app, "setup_status").unwrap();
    assert_eq!(
        (&status["yt_dlp"], &status["yt_dlp_managed"], &status["model"]),
        (&json!(false), &json!(false), &json!(true))
    );
}

/// A yt-dlp or JavaScript runtime the computer has counts, but not as
/// VODscribe's own, which it downloaded and checked (setup.rs
/// `SetupStatus::yt_dlp_managed`: "yt-dlp is the app's own copy, verified
/// when downloaded, rather than one found on PATH"; tools.rs
/// `find_js_runtime`).
#[test]
fn the_computers_own_programs_count_but_not_as_vodscribes() {
    let app = app();
    app.lacks_yt_dlp();
    let system = app.paths().root.with_file_name("system");
    on_the_path(&system, "yt-dlp");
    on_the_path(&system, "node");
    vodscribe_core::testkit::tools::path_for(&app.paths().bin(), std::slice::from_ref(&system));
    let status = ask(&app, "setup_status").unwrap();
    assert_eq!((&status["yt_dlp"], &status["yt_dlp_managed"]), (&json!(true), &json!(false)));
    assert_eq!((&status["js_runtime"], &status["js_runtime_managed"]), (&json!(true), &json!(false)));
    on_the_path(&app.paths().bin(), "deno");
    assert_eq!(ask(&app, "setup_status").unwrap()["js_runtime_managed"], json!(true));
}

/// Setup with the tools and model there ends well even when Deno can't be
/// downloaded: it's optional, needed by some YouTube VODs only (setup.rs
/// `run_setup`: "If it fails, setup still succeeded; YouTube links that
/// need it will say so."; "completion as `setup-done` (an error message
/// or null)").
#[test]
fn setup_ends_well_without_deno() {
    let app = app();
    app.worker();
    let done = Heard::of(&app, "setup-done");
    ask(&app, "run_setup").unwrap();
    assert_eq!(done.first(), Value::Null);
    let status = ask(&app, "setup_status").unwrap();
    assert_eq!((&status["js_runtime"], &status["running"]), (&json!(false), &json!(false)));
}

/// Setup deletes the speech models earlier versions used, keeping this
/// version's and the GPU add-on's (model.rs `remove_others`: "Deletes model
/// dirs that aren't in `keep`, such as the sherpa-onnx int8 release
/// earlier versions used"; setup.rs `run_setup`).
#[test]
fn setup_deletes_the_speech_models_earlier_versions_used() {
    let app = app();
    app.worker();
    let models = app.paths().models();
    let earlier = models.join("sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8");
    std::fs::create_dir_all(&earlier).unwrap();
    std::fs::write(earlier.join("encoder.int8.onnx"), b"an earlier model").unwrap();
    installed(&models, &GPU_MODEL);
    let done = Heard::of(&app, "setup-done");
    ask(&app, "run_setup").unwrap();
    assert_eq!(done.first(), Value::Null);
    assert!(!earlier.exists(), "the earlier model is kept");
    assert!(vodscribe_core::model::is_installed(&app.paths().gpu_model_dir(), &GPU_MODEL));
    assert!(vodscribe_core::model::is_installed(&app.paths().model_dir(), &CPU_MODEL));
}

/// A setup that can't finish says why (setup.rs `run_setup`: "completion as
/// `setup-done` (an error message or null)"): the model's file it couldn't
/// download.
#[test]
fn a_setup_that_cant_finish_says_why() {
    let app = app();
    let done = Heard::of(&app, "setup-done");
    ask(&app, "run_setup").unwrap();
    let error = done.first();
    assert!(error.as_str().is_some_and(|e| e.contains("download decoder.int8.onnx")), "{error}");
    assert_eq!(ask(&app, "setup_status").unwrap()["model"], json!(false));
}

/// Setup's progress says which of the model's files is under way, counted
/// across them all, and the files an earlier try finished aren't
/// downloaded again (CHANGELOG.md, 0.1.2: "Downloads with several files
/// show which one is under way, like \"(3 of 5)\"."; model.rs `install`:
/// "Files finished by an earlier, interrupted attempt are kept.").
#[test]
fn setup_says_which_file_is_under_way_out_of_how_many() {
    let _turn = turn();
    let app = app();
    let (asked, progress, done) = held_setup(&app);
    let heard = progress.all();
    assert!(heard.iter().all(|p| p["item"] == "model"), "{heard:?}");
    let finished: u64 = CPU_MODEL.files[..4].iter().map(|f| f.size).sum();
    let last = progress.last().unwrap();
    let wanted = ("joiner.int8.onnx matches its SHA‑256", finished, CPU_MODEL.total_bytes(), 4, 5);
    let got = (
        last["step"].as_str().unwrap(),
        last["done"].as_u64().unwrap(),
        last["total"].as_u64().unwrap(),
        last["part"].as_u64().unwrap(),
        last["parts"].as_u64().unwrap(),
    );
    assert_eq!(got, wanted);
    assert!(
        heard.iter().all(|p| !p["step"].as_str().unwrap().starts_with("Downloading")),
        "a finished file again: {heard:?}"
    );
    asked.open();
    done.first();
}

/// While setup runs it says so, and setup asked again meanwhile ends with
/// it, once (setup.rs `Install::spawn`: "With one running already its done
/// event comes as for this one").
#[test]
fn setup_asked_again_while_it_runs_ends_with_it() {
    let _turn = turn();
    let app = app();
    let (asked, _, done) = held_setup(&app);
    assert_eq!(ask(&app, "setup_status").unwrap()["running"], json!(true));
    ask(&app, "run_setup").unwrap();
    asked.open();
    done.first();
    eventually("setup ended", || ask(&app, "setup_status").unwrap()["running"] == json!(false));
    assert_eq!(done.all().len(), 1, "{:?}", done.all());
}

/// yt-dlp isn't updated while setup is downloading, since setup installs
/// it too (setup.rs `update_yt_dlp`: "the two mustn't write the same file
/// at once").
#[test]
fn yt_dlp_isnt_updated_while_setup_downloads() {
    let _turn = turn();
    let app = app();
    let (asked, _, done) = held_setup(&app);
    let refused = ask(&app, "update_yt_dlp").unwrap_err();
    assert_eq!(refused, json!("Setup is still downloading. Try again when it's done."));
    asked.open();
    done.first();
}

/// A canceled setup ends without an error, and the model's files it had
/// finished stay (CHANGELOG.md, 0.1.3: "Setup and the GPU add-on downloads
/// have a Cancel button. Tools and model files that finished stay"; api.ts
/// `cancelSetup`: "`setupDone` then fires with null").
#[test]
fn a_canceled_setup_ends_without_an_error_keeping_the_files_it_finished() {
    let _turn = turn();
    let app = app();
    let (asked, _, done) = held_setup(&app);
    ask(&app, "cancel_setup").unwrap();
    asked.open();
    assert_eq!(done.first(), Value::Null);
    let staging = app.paths().models().join(format!(".{}.partial", CPU_MODEL.id));
    for file in &CPU_MODEL.files[..4] {
        assert_eq!(std::fs::metadata(staging.join(file.name)).unwrap().len(), file.size, "{}", file.name);
    }
    eventually("setup ended", || ask(&app, "setup_status").unwrap()["running"] == json!(false));
}

/// Cancel with no setup running doesn't stop the next one (setup.rs
/// `Install::cancel`: "With none running this sets nothing, so a click
/// can't carry over into the next download.").
#[test]
fn a_cancel_with_no_setup_running_doesnt_stop_the_next() {
    let _turn = turn();
    let app = app();
    ask(&app, "cancel_setup").unwrap();
    let (asked, _, done) = held_setup(&app);
    asked.open();
    let ended = done.first();
    assert!(ended.as_str().is_some_and(|e| e.contains("download tokens.txt")), "it was canceled: {ended}");
}

/// Setup on a computer without yt-dlp installs only one its release signed:
/// with a checksum list not as signed, it installs none and says why
/// (tools.rs `install_yt_dlp`; `signed_sha256`: "The video downloader
/// couldn't be checked, so it wasn't installed. Try again later.").
#[test]
fn setup_installs_no_yt_dlp_its_release_didnt_sign() {
    let _turn = turn();
    let app = app();
    app.lacks_yt_dlp();
    serves_release(&app, |sums| sums[0] ^= 1, b"a yt-dlp");
    let done = Heard::of(&app, "setup-done");
    ask(&app, "run_setup").unwrap();
    let error = done.first();
    let why = "The video downloader couldn't be checked, so it wasn't installed. Try again later.";
    assert!(error.as_str().is_some_and(|e| e.contains(why)), "{error}");
    assert_eq!(ask(&app, "setup_status").unwrap()["yt_dlp"], json!(false));
}

/// Setup on a computer without ffmpeg refuses a download that isn't the
/// ffmpeg it's pinned to, and says so (deps.rs: "Each download is pinned by
/// size and SHA-256, so a replaced file is refused").
#[test]
fn setup_refuses_an_ffmpeg_unlike_its_pin() {
    let _turn = turn();
    let app = app();
    std::fs::remove_file(app.ffmpeg.path()).unwrap();
    let ffmpeg = vodscribe_core::deps::ffmpeg().unwrap();
    app.net.serves(ffmpeg.url, b"not ffmpeg".to_vec());
    let done = Heard::of(&app, "setup-done");
    ask(&app, "run_setup").unwrap();
    let error = done.first();
    assert!(error.as_str().is_some_and(|e| e.contains("A download was damaged or changed on the way")), "{error}");
    let status = ask(&app, "setup_status").unwrap();
    assert_eq!((&status["ffmpeg"], &status["ffmpeg_managed"]), (&json!(false), &json!(false)));
}

/// An update checks yt-dlp's release signature first, saying so, and with
/// a checksum list not as signed keeps the yt-dlp it had (tools.rs
/// `install_yt_dlp`, `update_yt_dlp`: "Updates the managed yt-dlp the same
/// verified way it was installed"; setup.rs `update_yt_dlp`: "reporting its
/// steps as `yt-dlp-progress` events").
#[test]
fn an_update_checks_the_signature_first_and_keeps_the_yt_dlp_it_had() {
    let _turn = turn();
    let app = app();
    serves_release(&app, |sums| sums[0] ^= 1, b"a yt-dlp");
    std::fs::write(app.yt_dlp.path(), b"the yt-dlp before").unwrap();
    let refused = ask(&app, "update_yt_dlp").unwrap_err();
    let why = "The video downloader couldn't be checked, so it wasn't installed. Try again later.";
    assert!(refused.as_str().is_some_and(|e| e.contains(why)), "{refused}");
    assert_eq!(app.heard("yt-dlp-progress")[0]["step"], json!("Checking yt-dlp's release signature"));
    assert_eq!(std::fs::read(app.yt_dlp.path()).unwrap(), b"the yt-dlp before");
}

/// An update whose yt-dlp isn't the one its signed list gives is thrown
/// away, and the yt-dlp it had stays (net.rs `download_verified`: "Nothing
/// appears at `dest` unless it matches").
#[test]
fn an_update_unlike_its_signed_checksum_keeps_the_yt_dlp_it_had() {
    let _turn = turn();
    let app = app();
    serves_release(&app, |_| {}, b"not the yt-dlp that was signed");
    std::fs::write(app.yt_dlp.path(), b"the yt-dlp before").unwrap();
    let refused = ask(&app, "update_yt_dlp").unwrap_err();
    assert!(refused.as_str().is_some_and(|e| e.contains("A download was damaged or changed on the way")), "{refused}");
    assert_eq!(std::fs::read(app.yt_dlp.path()).unwrap(), b"the yt-dlp before");
    // Anything named after yt-dlp but yt-dlp itself (yt-dlp.exe on Windows) is a download left over.
    let program = app.yt_dlp.path().file_name().unwrap().to_owned();
    let left: Vec<_> = std::fs::read_dir(app.paths().bin()).unwrap().map(|e| e.unwrap().file_name()).collect();
    let leftover = |n: &std::ffi::OsString| *n != program && n.to_string_lossy().starts_with("yt-dlp.");
    assert!(!left.iter().any(leftover), "half a download left: {left:?}");
}

/// Setup doesn't start while yt-dlp is being updated, and says so
/// (setup.rs `run_setup`: "VODscribe is updating or repairing its tools.
/// Try again in a moment."; `update_yt_dlp`).
#[test]
fn setup_waits_for_a_yt_dlp_update() {
    let _turn = turn();
    let app = app();
    serves_release(&app, |_| {}, b"a yt-dlp");
    let asked = Gate::new();
    app.net.holds(&format!("{RELEASE}/SHA2-256SUMS"), &asked);
    std::thread::scope(|s| {
        // Asked as the page asks, from a thread of its own, as the app
        // runs each command.
        let handle = app.handle().clone();
        let updating = s
            .spawn(move || tauri::async_runtime::block_on(crate::setup::update_yt_dlp(handle.clone(), handle.state())));
        asked.reached();
        let refused = ask(&app, "run_setup").unwrap_err();
        assert_eq!(refused, json!("VODscribe is updating or repairing its tools. Try again in a moment."));
        assert_eq!(ask(&app, "setup_status").unwrap()["running"], json!(true));
        asked.open();
        updating.join().unwrap().unwrap_err();
    });
    assert_eq!(ask(&app, "setup_status").unwrap()["running"], json!(false));
}

/// A copy that wasn't installed from a package has no update to fetch on
/// Linux (CHANGELOG.md, 0.1.3: "A copy that wasn't installed from a package
/// (built from source, say) says so when asked to check for updates";
/// setup.rs `self_updatable`: "A binary that was never packaged (a
/// development build, or one built from source) has no stamp").
#[cfg(target_os = "linux")]
#[test]
fn a_copy_not_installed_from_a_package_cant_update_itself() {
    let app = app();
    assert_eq!(ask(&app, "self_updatable").unwrap(), json!(false));
}
