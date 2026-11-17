//! The specs of `setup.rs`: the files in `tests/` that hold them, and what
//! they share: the downloads' hosts faked, what the page hears of an
//! install, and the speech models as an install leaves them.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use serde_json::{Value, json};
use tauri::Listener;
use vodscribe_core::model::ModelSpec;
use vodscribe_core::testkit::eventually;

use super::*;
use crate::testkit::TestApp;

mod first_run;
mod gpu_add_on;
mod settings;
mod storage;

/// The test's app, with the hosts setup and the GPU add-on download from
/// the fake's too (GitHub for yt-dlp, ffmpeg, Deno and ONNX Runtime, PyPI
/// for NVIDIA's libraries, and the macOS ffmpeg's host; the speech models'
/// host is one of the sites' already): nothing reaches the real ones, and
/// a download nothing answers is refused (404).
fn app() -> TestApp {
    let app = TestApp::new();
    app.net.public("github.com").public("files.pythonhosted.org").public("ffmpeg.martin-riedl.de");
    app
}

/// A turn at the downloads' links. Their links are the same for every
/// test (a release's, a model's), and what a test serves or holds there
/// every test running sees, so the tests that serve or hold them take
/// turns.
fn turn() -> MutexGuard<'static, ()> {
    static TURN: Mutex<()> = Mutex::new(());
    TURN.lock().unwrap_or_else(|e| e.into_inner())
}

/// The payloads of an event the page hears, kept from when this is made:
/// an install's events, which the kit's `heard` doesn't keep.
struct Heard(Arc<Mutex<Vec<Value>>>);

impl Heard {
    fn of(app: &TestApp, event: &'static str) -> Self {
        let heard = Heard(Arc::default());
        let kept = heard.0.clone();
        app.handle().listen_any(event, move |e| {
            kept.lock().unwrap().push(serde_json::from_str(e.payload()).unwrap_or(Value::Null));
        });
        heard
    }

    fn all(&self) -> Vec<Value> {
        self.0.lock().unwrap().clone()
    }

    fn last(&self) -> Option<Value> {
        self.all().last().cloned()
    }

    /// The first heard, once there is one: an install's end.
    fn first(&self) -> Value {
        eventually("it was heard", || !self.all().is_empty());
        self.all()[0].clone()
    }
}

/// Command `cmd`'s answer, with no arguments.
fn ask(app: &TestApp, cmd: &str) -> Result<Value, Value> {
    app.invoke(cmd, json!({}))
}

/// Model `spec`'s first `n` files, as an earlier try that was stopped
/// left them: whole, waiting in its folder for the rest. Each is the size
/// it's pinned to (`model::is_installed` goes by size; their bytes are
/// never read), and takes no room on the disk.
fn finished_before(models: &Path, spec: &ModelSpec, n: usize) -> PathBuf {
    let staging = models.join(format!(".{}.partial", spec.id));
    std::fs::create_dir_all(&staging).unwrap();
    for file in &spec.files[..n] {
        std::fs::File::create(staging.join(file.name)).unwrap().set_len(file.size).unwrap();
    }
    staging
}

/// Model `spec` as its install leaves it (`model::is_installed`).
fn installed(models: &Path, spec: &ModelSpec) {
    let dir = vodscribe_core::model::model_dir(models, spec);
    std::fs::create_dir_all(&dir).unwrap();
    for file in spec.files {
        std::fs::File::create(dir.join(file.name)).unwrap().set_len(file.size).unwrap();
    }
}

/// The link model `spec`'s file `name` downloads from.
fn model_link(spec: &ModelSpec, name: &str) -> String {
    format!("https://dl.vodscribe.com/models/{}/{name}", spec.id)
}

#[test]
fn a_claim_holds_the_flag_and_says_it_isnt_an_install() {
    let install = Install::new();
    let claim = Claim::take(&install).expect("free");
    assert!(install.running() && install.held.load(Ordering::Relaxed) == CLAIMED);
    assert!(Claim::take(&install).is_none(), "one holder at a time");
    drop(claim);
    assert!(!install.running());
    // Held by an install (as spawn sets it), it isn't claimed.
    install.held.store(INSTALLING, Ordering::Relaxed);
    assert!(Claim::take(&install).is_none() && install.held.load(Ordering::Relaxed) == INSTALLING);
}
