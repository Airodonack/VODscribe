//! The GPU add-on's download as a job (CHANGELOG.md, 0.7.4: "The GPU add-on
//! carries on after a restart. It shows in the jobs list with Cancel."): listed with the jobs, with Cancel, beside
//! the clips; carried on after a quit from the files it had unpacked and
//! the bytes it had of the one downloading (setup.rs `start_addon`,
//! `settle_addon`, `resume_addon`; core gpu.rs `install`). The pack is the
//! kit's (`TestApp::addon`), served by the app's fake network.

use std::sync::Mutex;

use serde_json::{Value, json};
use vodscribe_core::asked::Queue;
use vodscribe_core::db::Db;
use vodscribe_core::gpu;
use vodscribe_core::testkit::{Gate, Recorded, eventually};

use crate::testkit::{Folder, TestApp};

fn library_of(folder: &Folder) -> Db {
    Db::open_in(&folder.paths()).unwrap()
}

/// The add-on's line in the list of jobs, if it has one.
fn addon_job(app: &TestApp) -> Option<Value> {
    let jobs = app.invoke("list_jobs", json!({})).unwrap();
    jobs.as_array().unwrap().iter().find(|j| j["kind"] == "addon").cloned()
}

fn installed(app: &TestApp) -> bool {
    app.invoke("gpu_status", json!({})).unwrap()["installed"] == true
}

/// Bytes the add-on has across its files, as Settings shows it (`gpu_status`).
fn downloaded(app: &TestApp) -> Option<u64> {
    app.invoke("gpu_status", json!({})).unwrap()["progress"]["done"].as_u64()
}

/// The links asked for and the range each asked, from `recorded`.
fn asked(recorded: &Recorded) -> Vec<(String, Option<String>)> {
    recorded.requests().into_iter().map(|r| (r.url.clone(), r.header("Range").map(String::from))).collect()
}

/// The host the kit's pack is served from.
fn host(addon: &vodscribe_core::testkit::Addon) -> String {
    addon.files()[0].0.rsplit_once('/').unwrap().0.to_string()
}

/// While it downloads, the add-on is a job in the list, "GPU add-on",
/// running, with how far it is; Settings shows the same progress
/// (`gpu_status`). Cancel there stops it, and what it had goes: asked
/// again, it starts over (setup.rs `settle_addon`: "Canceled, what it had
/// downloaded goes, so asking again starts over.").
#[test]
fn the_addon_downloading_is_a_job_and_cancel_throws_away_what_it_had() {
    let app = TestApp::new();
    let addon = app.addon();
    addon.served_by(&app.net);
    let gate = Gate::new();
    let third = addon.files()[2].0.clone();
    app.net.holds_partway(&third, 1_000, &gate);
    app.invoke("install_gpu_pack", json!({})).unwrap();
    gate.reached();
    eventually("its progress heard", || addon_job(&app).is_some_and(|j| j["progress"].is_number()));
    let job = addon_job(&app).unwrap();
    assert_eq!(
        (job["id"].clone(), job["label"].clone(), job["running"].clone()),
        (json!(0), json!("GPU add-on"), json!(true))
    );
    let along = job["progress"].as_f64().unwrap();
    assert!(along > 0.0 && along < 1.0, "{along}");
    let status = app.invoke("gpu_status", json!({})).unwrap();
    assert_eq!((status["installing"].clone(), status["progress"]["done"].is_number()), (json!(true), true));
    app.invoke("cancel_job", json!({ "kind": "addon", "id": 0 })).unwrap();
    eventually("its line gone", || addon_job(&app).is_none());
    let staging = gpu::staging_dir(&app.paths().gpu_pack());
    assert!(!staging.exists(), "what it had downloaded was kept");
    assert!(!app.db().is_asked(Queue::Addon, 0).unwrap(), "still asked for");
    let recorded = app.net.records(&host(&addon));
    app.net.let_go();
    app.invoke("install_gpu_pack", json!({})).unwrap();
    // Both installs' ends heard, the canceled one's and this one's, and
    // each null, so one that failed says why (setup.rs `Install::spawn`:
    // "emits `done_event` with an error message, or null when it finished
    // or was canceled").
    eventually("both installs' ends told", || app.heard("gpu-done").len() == 2);
    assert_eq!(app.heard("gpu-done"), [Value::Null, Value::Null], "an install failed");
    assert!(installed(&app));
    assert!(asked(&recorded).iter().all(|(_, range)| range.is_none()), "carried on from a canceled download");
}

/// A quit partway through the add-on's download keeps what it had, and the
/// next start carries it on: the files it unpacked aren't downloaded again,
/// and the one it was downloading is asked for from the byte it had
/// (setup.rs `resume_addon`: "the GPU add-on's install a quit stopped
/// carries on"; core gpu.rs `install`: "the parts it unpacked are kept, and
/// the file it was downloading carries on from the bytes it had").
#[test]
fn a_quit_partway_through_the_addon_carries_on_from_the_bytes_it_had() {
    let app = TestApp::new();
    let addon = app.addon();
    addon.served_by(&app.net);
    let gate = Gate::new();
    let third = addon.files()[2].0.clone();
    app.net.holds_partway(&third, 1_000, &gate);
    app.invoke("install_gpu_pack", json!({})).unwrap();
    gate.reached();
    // What came before the gate written: the first two files and a thousand bytes.
    let before: u64 = addon.files()[..2].iter().map(|(_, b)| b.len() as u64).sum::<u64>() + 1_000;
    eventually("what came before the gate written", || downloaded(&app) == Some(before));
    let folder = app.quit();
    assert!(library_of(&folder).is_asked(Queue::Addon, 0).unwrap(), "the quit forgot it was asked");
    let recorded = Mutex::new(None);
    let app = TestApp::on_with(folder, |fakes| {
        *recorded.lock().unwrap() = Some(fakes.net.records(&host(&addon)));
        addon.served_by(fakes.net);
    });
    eventually("installed", || installed(&app));
    let recorded = recorded.lock().unwrap().take().unwrap();
    let mut want = vec![(third, Some("bytes=1000-".to_string()))];
    want.extend(addon.files()[3..].iter().map(|(url, _)| (url.clone(), None)));
    assert_eq!(asked(&recorded), want);
    eventually("its line gone", || addon_job(&app).is_none());
    assert!(!app.db().is_asked(Queue::Addon, 0).unwrap(), "asked for still, installed");
}

/// An install that failed isn't started again at the next start, as a
/// video's download isn't; the files that matched stay, and asking again
/// carries on from them (setup.rs `settle_addon`: "a failed one isn't
/// started again at the next start, as a video's download isn't; the files
/// that matched stay, for asking again to carry on from").
#[test]
fn a_failed_addon_install_is_not_started_again_and_asking_again_carries_on() {
    let app = TestApp::new();
    let addon = app.addon();
    // The first answer for a link is the one given: the refusal, then the rest served.
    app.net.answers(&addon.files()[1].0, 503);
    addon.served_by(&app.net);
    app.invoke("install_gpu_pack", json!({})).unwrap();
    eventually("its failure told", || app.heard("gpu-done").iter().any(Value::is_string));
    let app = TestApp::on(app.quit());
    assert!(!app.state().gpu.running(), "a failed install started again, unasked");
    assert!(addon_job(&app).is_none());
    let recorded = app.net.records(&host(&addon));
    addon.served_by(&app.net);
    app.invoke("install_gpu_pack", json!({})).unwrap();
    eventually("installed", || installed(&app));
    let urls: Vec<String> = asked(&recorded).into_iter().map(|(url, _)| url).collect();
    let rest: Vec<String> = addon.files()[1..].iter().map(|(url, _)| url.clone()).collect();
    assert_eq!(urls, rest, "the file that matched downloaded again");
}

/// Removing the add-on deletes what an install of it had too (setup.rs
/// `remove_gpu_pack`; core model.rs `uninstall`: "Removes `spec`'s files,
/// installed or on their way"), and a start after that has nothing to
/// carry on.
#[test]
fn removing_the_addon_deletes_what_its_install_had() {
    let app = TestApp::new();
    let addon = app.addon();
    addon.served_by(&app.net);
    let gate = Gate::new();
    app.net.holds_partway(&addon.files()[1].0, 500, &gate);
    app.invoke("install_gpu_pack", json!({})).unwrap();
    gate.reached();
    let folder = app.quit();
    // Started again with nothing served: it fails, and keeps what it had.
    let app = TestApp::on_with(folder, |fakes| {
        fakes.net.answers(&host(&addon), 404);
    });
    eventually("its end told", || !app.heard("gpu-done").is_empty());
    let staging = gpu::staging_dir(&app.paths().gpu_pack());
    assert!(staging.exists());
    app.invoke("remove_gpu_pack", json!({})).unwrap();
    assert!(!staging.exists(), "what its install had was kept");
}
