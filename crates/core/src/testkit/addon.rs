//! The GPU add-on as a test has it (`Addon`): its pack (gpu.rs) made small,
//! a sherpa-onnx archive and four wheels, each with the libraries
//! `gpu::install` keeps, pinned by their size and SHA-256 as the real ones
//! are, and served from a host of the test's own. It's the pack for one
//! folder, the one the test installs it at (`gpu::platform`'s seam,
//! `platform`); every other folder has the platform's real one.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use sha2::{Digest, Sha256};

use super::Net;
use crate::gpu::{Pinned, Platform};

/// The libraries in the archive (sherpa-onnx's own among them, which the
/// pack leaves out) and in each wheel, as the real ones name them.
const ARCHIVE: &[&str] = &["libonnxruntime.so", "libonnxruntime_providers_cuda.so", "libsherpa-onnx-c-api.so"];
const WHEELS: &[(&str, &str)] = &[
    ("CUDA runtime", "nvidia/cuda_runtime/lib/libcudart.so.12"),
    ("cuBLAS", "nvidia/cublas/lib/libcublas.so.12"),
    ("cuRAND", "nvidia/curand/lib/libcurand.so.10"),
    ("cuDNN", "nvidia/cudnn/lib/libcudnn.so.9"),
];

/// Bytes of each library: enough that a download held partway is partway.
const LIBRARY: usize = 48 * 1024;

/// The fake pack for the folder it's installed at, while it's held.
pub struct Addon {
    pack_dir: PathBuf,
    /// Each file it downloads, in the order `gpu::install` takes them: its
    /// link and its bytes.
    files: Vec<(String, Vec<u8>)>,
}

/// A file of a pack: what progress calls it, its link and its bytes.
type File = (String, String, Vec<u8>);

/// The packs tests give, each with the folder it's for.
static PACKS: Mutex<Vec<(PathBuf, Vec<File>)>> = Mutex::new(Vec::new());

fn packs() -> MutexGuard<'static, Vec<(PathBuf, Vec<File>)>> {
    PACKS.lock().unwrap_or_else(|e| e.into_inner())
}

impl Addon {
    /// The pack to install at `pack_dir` (`Paths::gpu_pack`), its files on
    /// a host named for that folder, so tests at once don't share one.
    pub fn at(pack_dir: &Path) -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let host = format!("https://addon-{}-{n}.test", std::process::id());
        let mut labelled = vec![("ONNX Runtime with CUDA".to_string(), format!("{host}/sherpa.tar.bz2"), archive())];
        for (i, (label, path)) in WHEELS.iter().enumerate() {
            labelled.push((label.to_string(), format!("{host}/wheel-{i}.whl"), wheel(path, i as u8)));
        }
        let files = labelled.iter().map(|(_, url, bytes)| (url.clone(), bytes.clone())).collect();
        packs().push((pack_dir.to_path_buf(), labelled));
        Addon { pack_dir: pack_dir.to_path_buf(), files }
    }

    /// Each file the pack downloads, in the order it downloads them: its
    /// link and its bytes.
    pub fn files(&self) -> &[(String, Vec<u8>)] {
        &self.files
    }

    /// `net` serves the pack's files, each as a server that honours ranges.
    pub fn served_by(&self, net: &Net) {
        for (url, bytes) in &self.files {
            net.serves(url, bytes.clone());
        }
    }

    /// The names of the libraries the pack installs.
    pub fn libraries() -> Vec<String> {
        let ours = ARCHIVE.iter().filter(|n| n.contains("onnxruntime")).map(|n| n.to_string());
        ours.chain(WHEELS.iter().map(|(_, p)| p.rsplit('/').next().unwrap().to_string())).collect()
    }
}

impl Drop for Addon {
    fn drop(&mut self) {
        packs().retain(|(dir, _)| *dir != self.pack_dir);
    }
}

/// The pack a test gives for `pack_dir`, if one does (`gpu::platform`).
pub(crate) fn platform(pack_dir: &Path) -> Option<Platform> {
    let packs = packs();
    let (_, files) = packs.iter().find(|(dir, _)| dir == pack_dir)?;
    let pinned = |(label, url, bytes): &File| {
        let sha256 = crate::net::hex(&Sha256::digest(bytes));
        (label.clone(), Pinned { url: url.clone(), size: bytes.len() as u64, sha256 })
    };
    let mut parts = files.iter().map(pinned);
    Some(Platform { sherpa: parts.next()?, wheels: parts.collect(), lib_exts: &[".so"] })
}

/// `LIBRARY` bytes that don't compress, different for each `seed`.
fn noise(seed: u8) -> Vec<u8> {
    let mut x = 0x2545_f491_4f6c_dd1du64 ^ u64::from(seed);
    (0..LIBRARY)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}

/// The sherpa-onnx archive: its libraries under a release's folder, as
/// bzip2'd tar.
fn archive() -> Vec<u8> {
    let mut tar = tar::Builder::new(bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast()));
    for (i, name) in ARCHIVE.iter().enumerate() {
        let bytes = noise(100 + i as u8);
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, format!("sherpa-onnx/lib/{name}"), bytes.as_slice()).unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap()
}

/// A wheel with its one library at `path`, as NVIDIA's are laid out.
fn wheel(path: &str, seed: u8) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file(path, stored).unwrap();
    zip.write_all(&noise(seed)).unwrap();
    zip.finish().unwrap().into_inner()
}
