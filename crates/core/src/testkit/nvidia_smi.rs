//! A fake `nvidia-smi`, the NVIDIA driver's tool, which the app asks for the
//! graphics card and its free memory (`gpu::detect`). It answers the query
//! the app makes with the card the test says the computer has, as one CSV
//! line; with none, as the tool does on a computer whose driver finds none
//! ("No devices were found", exit 6). It's found on the PATH a test gives
//! the app (`tools::path_for`), so it's an executable file there.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::tools::{self, Fake, Run};

/// The fake `nvidia-smi` in a folder, faked until it's dropped.
pub struct NvidiaSmi {
    fake: Fake,
    card: Arc<Mutex<Option<Card>>>,
}

/// A graphics card, as `nvidia-smi` names it and counts its memory.
#[derive(Clone)]
struct Card {
    name: String,
    total_mb: u64,
    free_mb: u64,
}

impl NvidiaSmi {
    /// Fakes `nvidia-smi` in `dir`, on a computer with no card till told.
    pub fn in_dir(dir: &Path) -> Self {
        let card: Arc<Mutex<Option<Card>>> = Arc::default();
        let has = card.clone();
        let path: PathBuf = dir.join(format!("nvidia-smi{}", std::env::consts::EXE_SUFFIX));
        let fake = tools::fake(&path, move |run| answer(run, has.lock().unwrap().clone()));
        // Found on a PATH only as a program it may run.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        NvidiaSmi { fake, card }
    }

    pub fn path(&self) -> &Path {
        self.fake.path()
    }

    /// The computer has card `name`, with `total_mb` of memory, `free_mb`
    /// of it free.
    pub fn shows(&self, name: &str, total_mb: u64, free_mb: u64) {
        *self.card.lock().unwrap() = Some(Card { name: name.into(), total_mb, free_mb });
    }
}

fn answer(run: &mut Run, card: Option<Card>) -> i32 {
    if !run.args().iter().any(|a| a.starts_with("--query-gpu=")) {
        run.err("fake nvidia-smi: nothing it knows was asked\n");
        return 2;
    }
    match card {
        Some(card) => {
            run.out(&format!("{}, {}, {}\n", card.name, card.total_mb, card.free_mb));
            0
        }
        None => {
            run.out("No devices were found\n");
            6
        }
    }
}
