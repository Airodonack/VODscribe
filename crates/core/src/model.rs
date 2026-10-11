use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, bail};

use crate::net;

pub struct ModelSpec {
    pub id: &'static str,
    pub url: &'static str,
    /// Size of the compressed archive, used when the server omits Content-Length.
    pub archive_bytes: u64,
}

/// Parakeet TDT 0.6B v3: English plus 24 other European languages.
pub const PARAKEET_V3: ModelSpec = ModelSpec {
    id: "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8",
    url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2",
    archive_bytes: 487_170_055,
};

const FILES: [&str; 4] = ["encoder.int8.onnx", "decoder.int8.onnx", "joiner.int8.onnx", "tokens.txt"];

pub fn model_dir(models_root: &Path, spec: &ModelSpec) -> PathBuf {
    models_root.join(spec.id)
}

pub fn is_installed(dir: &Path) -> bool {
    FILES.iter().all(|f| dir.join(f).is_file())
}

/// Streams the archive through bzip2 and tar, so it is never stored compressed.
/// `on_progress` receives (archive bytes downloaded, archive total).
pub fn install(
    models_root: &Path,
    spec: &ModelSpec,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<PathBuf> {
    let dest = model_dir(models_root, spec);
    if is_installed(&dest) {
        return Ok(dest);
    }
    let staging = models_root.join(format!(".{}.partial", spec.id));
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging)?;

    let result = (|| -> Result<()> {
        let (body, len) = net::get(spec.url)?;
        let total = len.unwrap_or(spec.archive_bytes);
        let reader = net::ProgressReader::new(body, cancel, |n| on_progress(n, total));
        let mut archive = tar::Archive::new(bzip2::read::BzDecoder::new(reader));
        archive.unpack(&staging).context("extract model archive")?;
        Ok(())
    })();
    if let Err(e) = result {
        let _ = fs::remove_dir_all(&staging);
        if cancel.load(Ordering::Relaxed) {
            bail!("canceled");
        }
        return Err(e);
    }

    // The archive contains a single top-level directory named after the model.
    let extracted = staging.join(spec.id);
    if !is_installed(&extracted) {
        let _ = fs::remove_dir_all(&staging);
        bail!("model archive is missing expected files");
    }
    let _ = fs::remove_dir_all(&dest);
    fs::rename(&extracted, &dest)?;
    let _ = fs::remove_dir_all(&staging);
    Ok(dest)
}
