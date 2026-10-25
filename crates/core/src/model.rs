//! Speech models: Parakeet TDT 0.6B v3, exported and quantized by
//! tools/model-export and served from VODscribe's model bucket. Every file is
//! pinned by size and SHA-256.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result};

use crate::{Progress, net};

/// Where model files are served from, as `<base>/<model id>/<file>`: the
/// `vodscribe` R2 bucket, on its custom domain.
const MODELS_BASE_URL: &str = "https://dl.vodscribe.com/models";

/// `VODSCRIBE_MODELS_URL` overrides the server in debug builds, e.g. to test
/// against a local one.
fn base_url() -> String {
    let dev = cfg!(debug_assertions).then(|| std::env::var("VODSCRIBE_MODELS_URL").ok()).flatten();
    dev.unwrap_or_else(|| MODELS_BASE_URL.to_string())
}

pub struct ModelFile {
    pub name: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

pub struct ModelSpec {
    pub id: &'static str,
    /// The exact model, for progress.
    pub name: &'static str,
    pub files: &'static [ModelFile],
}

impl ModelSpec {
    pub fn total_bytes(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
}

/// For the CPU: weight-only int8 with int8 math, within ~1% of fp32.
pub const CPU_MODEL: ModelSpec = ModelSpec {
    id: "parakeet-tdt-0.6b-v3-int8wo",
    name: "Parakeet TDT 0.6B v3 (int8)",
    files: &[
        ModelFile {
            name: "decoder.int8.onnx",
            size: 47233763,
            sha256: "1d9e0a3abd09af7e4ab9584d2b93b0e0430c0dadc0343502ac2e39b3d77ede3a",
        },
        ModelFile {
            name: "encoder.int8.onnx",
            size: 41894229,
            sha256: "c5a5164abc5ff7acb6fce27d307cee6592edd0f3ba3b93c1bf6be531f3d2d348",
        },
        ModelFile {
            name: "encoder.int8.weights",
            size: 854036480,
            sha256: "1ba8d31d5afa25fb26e84945f6758101d3b7b9f1cc20b3ee1878e674e031020f",
        },
        ModelFile {
            name: "joiner.int8.onnx",
            size: 6549064,
            sha256: "48e276e539ef1e08e9c56a0ca13494a6b77206102b2a326cd145c2960daac4b1",
        },
        ModelFile {
            name: "tokens.txt",
            size: 93939,
            sha256: "d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d",
        },
    ],
};

/// For NVIDIA GPUs: mixed-precision fp16, half the VRAM of fp32 at the same speed.
pub const GPU_MODEL: ModelSpec = ModelSpec {
    id: "parakeet-tdt-0.6b-v3-fp16",
    name: "Parakeet TDT 0.6B v3 (fp16)",
    files: &[
        ModelFile {
            name: "decoder.fp16.onnx",
            size: 23621639,
            sha256: "379bc61b3adb0e7bd1f0e73d5e663d554ec382a4e863ced2c2cc2dac71541cbd",
        },
        ModelFile {
            name: "encoder.fp16.onnx",
            size: 21835082,
            sha256: "7d92e4ed3b41543db59e795c32b24bce8dfc2d19ae19f332924624e98841b6d7",
        },
        ModelFile {
            name: "encoder.fp16.weights",
            size: 1218199040,
            sha256: "4c926a41b3e318fc0c554d69e31d6bb77593c189980981ab15da342c86cb75cc",
        },
        ModelFile {
            name: "joiner.fp16.onnx",
            size: 12645840,
            sha256: "0785eecb0e428ae84b660da6c161696d0e5881008067f99edca0a9c7f5f7ed3a",
        },
        ModelFile {
            name: "tokens.txt",
            size: 93939,
            sha256: "d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d",
        },
    ],
};

/// The model a transcript imported with annotations is recorded as made by
/// (`annotations.rs`): not one of ours, and not one to transcribe again with.
pub const IMPORTED_MODEL: &str = "imported";

/// Whether transcripts from model `id` are as good as this version produces.
/// An imported one counts: a newer model of ours isn't a reason to replace it.
pub fn is_current(id: &str) -> bool {
    id == CPU_MODEL.id || id == GPU_MODEL.id || id == IMPORTED_MODEL
}

pub fn model_dir(models_root: &Path, spec: &ModelSpec) -> PathBuf {
    models_root.join(spec.id)
}

/// Present with the expected sizes. Hashes are checked when downloading.
pub fn is_installed(dir: &Path, spec: &ModelSpec) -> bool {
    spec.files.iter().all(|f| fs::metadata(dir.join(f.name)).is_ok_and(|m| m.len() == f.size))
}

/// Downloads the model's files, verifying each. Files finished by an earlier,
/// interrupted attempt are kept.
pub fn install(
    models_root: &Path,
    spec: &ModelSpec,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(Progress),
) -> Result<PathBuf> {
    let dest = model_dir(models_root, spec);
    if is_installed(&dest, spec) {
        return Ok(dest);
    }
    let staging = models_root.join(format!(".{}.partial", spec.id));
    fs::create_dir_all(&staging)?;

    let total = spec.total_bytes();
    let base = base_url();
    let mut done = 0;
    let parts = spec.files.len();
    for (i, file) in spec.files.iter().enumerate() {
        let part = i + 1;
        let path = staging.join(file.name);
        let size = file.size;
        if !fs::metadata(&path).is_ok_and(|m| m.len() == size) {
            let url = format!("{}/{}/{}", base.trim_end_matches('/'), spec.id, file.name);
            let step = format!("Downloading {} · {}", spec.name, file.name);
            net::download_verified(&url, &path, Some(size), file.sha256, cancel, |n| {
                on_progress(Progress::file(&step, n, Some(size)).within(done, Some(total), part, parts))
            })
            .with_context(|| format!("download {}", file.name))?;
        }
        let step = format!("{} matches its SHA‑256", file.name);
        on_progress(Progress::file(&step, size, Some(size)).within(done, Some(total), part, parts));
        done += size;
    }

    let _ = fs::remove_dir_all(&dest);
    fs::rename(&staging, &dest)?;
    Ok(dest)
}

/// Deletes model dirs that aren't in `keep`, such as the sherpa-onnx int8
/// release earlier versions used, which drops stretches of speech.
pub fn remove_others(models_root: &Path, keep: &[&ModelSpec]) {
    let Ok(entries) = fs::read_dir(models_root) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with('.') && keep.iter().all(|s| s.id != name) && entry.path().is_dir() {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}
