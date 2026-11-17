//! Optional NVIDIA GPU transcription. The "GPU pack" is ONNX Runtime with its
//! CUDA provider (from sherpa-onnx's CUDA release) plus NVIDIA's redistributable
//! CUDA 12 and cuDNN 9 runtime libraries (from their official PyPI wheels),
//! downloaded into the app's data dir. The GPU worker binary and the sherpa-onnx
//! library it links ship with the app; only the libraries below come from the
//! pack. sherpa-onnx's own library in that release is its full build, with the
//! GPL-3.0 espeak-ng compiled in, so it's left out and the app's no-tts build
//! is used instead.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::{Progress, net, tools};

/// Free VRAM needed before a job is sent to the GPU (the smallest batch).
pub const MIN_FREE_MB: u64 = 3500;

/// Chunks per GPU decode for the free VRAM, or None if the GPU can't take the
/// job. Peak use of the fp16 model was 2.4 / 4.5 / 5.5 GB at batch 1 / 4 / 8
/// (RTX 3080); each tier leaves about 1 GB for other programs' growth.
pub fn batch_for_free_vram(free_mb: u64) -> Option<usize> {
    match free_mb {
        6500.. => Some(8),
        5500.. => Some(4),
        MIN_FREE_MB.. => Some(1),
        _ => None,
    }
}

/// Bump when the pack's contents change, so existing installs are refreshed.
const PACK_VERSION: &str = "2";

#[derive(Serialize, Clone, Debug)]
pub struct GpuInfo {
    pub name: String,
    pub total_mb: u64,
    pub free_mb: u64,
}

/// Queries the first NVIDIA GPU. `nvidia-smi` ships with the driver on Linux
/// and Windows, and is found on PATH (`tools::on_path`, for the app whose
/// own programs are in `bin_dir`).
pub fn detect(bin_dir: &Path) -> Option<GpuInfo> {
    let smi = tools::on_path(bin_dir, "nvidia-smi")?;
    let mut cmd = tools::command(&smi);
    cmd.args(["--query-gpu=name,memory.total,memory.free", "--format=csv,noheader,nounits"]).stdin(Stdio::null());
    let out = tools::output(&mut cmd).ok()?;
    if !out.status.success() {
        return None;
    }
    parse_smi(&String::from_utf8_lossy(&out.stdout))
}

fn parse_smi(text: &str) -> Option<GpuInfo> {
    let line = text.lines().next()?;
    let mut fields = line.split(',').map(str::trim);
    Some(GpuInfo {
        name: fields.next()?.to_string(),
        total_mb: fields.next()?.parse().ok()?,
        free_mb: fields.next()?.parse().ok()?,
    })
}

/// Whether a GPU pack exists for this platform (to install at `pack_dir`).
pub fn supported(pack_dir: &Path) -> bool {
    platform(pack_dir).is_some()
}

/// A download pinned by size and SHA-256, so a replaced release asset or
/// wheel is refused rather than loaded into the GPU worker.
#[derive(Clone)]
pub(crate) struct Pinned {
    pub(crate) url: String,
    pub(crate) size: u64,
    pub(crate) sha256: String,
}

impl Pinned {
    fn of(url: &str, size: u64, sha256: &str) -> Self {
        Pinned { url: url.into(), size, sha256: sha256.into() }
    }
}

pub(crate) struct Platform {
    /// sherpa-onnx + ONNX Runtime built with the CUDA provider, and how it's named for progress.
    pub(crate) sherpa: (String, Pinned),
    /// NVIDIA's CUDA 12 / cuDNN 9 runtime wheels from PyPI, labelled for progress.
    pub(crate) wheels: Vec<(String, Pinned)>,
    /// Library file extensions to keep.
    pub(crate) lib_exts: &'static [&'static str],
}

/// The pack for this platform, to install at `pack_dir`. In a test build,
/// the test's own where it gives one for that folder (`testkit::addon`).
fn platform(pack_dir: &Path) -> Option<Platform> {
    #[cfg(any(test, feature = "testkit"))]
    if let Some(fake) = crate::testkit::addon::platform(pack_dir) {
        return Some(fake);
    }
    // The real pack is the same for every folder.
    let _ = pack_dir;
    let wheels = |wheels: [(&str, Pinned); 4]| wheels.into_iter().map(|(l, p)| (l.to_string(), p)).collect();
    let sherpa = |pinned| ("ONNX Runtime 1.28 with CUDA (sherpa-onnx 1.13.8)".to_string(), pinned);
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some(Platform {
            sherpa: sherpa(Pinned::of(
                "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.8/sherpa-onnx-v1.13.8-cuda-12.x-cudnn-9.x-onnxruntime1.28.2-linux-x64-gpu.tar.bz2",
                393010348,
                "2132af1ae8c84c84f86ac091e4311839287851d2bf5e660332ebce3898a1e0b2",
            )),
            wheels: wheels([
                (
                    "CUDA runtime 12.8",
                    Pinned::of(
                        "https://files.pythonhosted.org/packages/0d/9b/a997b638fcd068ad6e4d53b8551a7d30fe8b404d6f1804abf1df69838932/nvidia_cuda_runtime_cu12-12.8.90-py3-none-manylinux2014_x86_64.manylinux_2_17_x86_64.whl",
                        954765,
                        "adade8dcbd0edf427b7204d480d6066d33902cab2a4707dcfc48a2d0fd44ab90",
                    ),
                ),
                (
                    "cuBLAS 12.8",
                    Pinned::of(
                        "https://files.pythonhosted.org/packages/dc/61/e24b560ab2e2eaeb3c839129175fb330dfcfc29e5203196e5541a4c44682/nvidia_cublas_cu12-12.8.4.1-py3-none-manylinux_2_27_x86_64.whl",
                        594346921,
                        "8ac4e771d5a348c551b2a426eda6193c19aa630236b418086020df5ba9667142",
                    ),
                ),
                (
                    "cuRAND 10.3",
                    Pinned::of(
                        "https://files.pythonhosted.org/packages/fb/aa/6584b56dc84ebe9cf93226a5cde4d99080c8e90ab40f0c27bda7a0f29aa1/nvidia_curand_cu12-10.3.9.90-py3-none-manylinux_2_27_x86_64.whl",
                        63619976,
                        "b32331d4f4df5d6eefa0554c565b626c7216f87a06a4f56fab27c3b68a830ec9",
                    ),
                ),
                (
                    "cuDNN 9.19",
                    Pinned::of(
                        "https://files.pythonhosted.org/packages/c5/41/65225d42fba06fb3dd3972485ea258e7dd07a40d6e01c95da6766ad87354/nvidia_cudnn_cu12-9.19.0.56-py3-none-manylinux_2_27_x86_64.whl",
                        657906812,
                        "ac6ad90a075bb33a94f2b4cf4622eac13dd4dc65cf6dd9c7572a318516a36625",
                    ),
                ),
            ]),
            lib_exts: &[".so"],
        }),
        ("windows", "x86_64") => Some(Platform {
            sherpa: sherpa(Pinned::of(
                "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.8/sherpa-onnx-v1.13.8-cuda-12.x-cudnn-9.x-onnxruntime1.28.2-win-x64-cuda.tar.bz2",
                595017373,
                "066c5b54dbafaa1388001a9c9837ac1374dbba6d6678f193ca06aa0d8e94d8c3",
            )),
            wheels: wheels([
                (
                    "CUDA runtime 12.8",
                    Pinned::of(
                        "https://files.pythonhosted.org/packages/30/a5/a515b7600ad361ea14bfa13fb4d6687abf500adc270f19e89849c0590492/nvidia_cuda_runtime_cu12-12.8.90-py3-none-win_amd64.whl",
                        944318,
                        "c0c6027f01505bfed6c3b21ec546f69c687689aad5f1a377554bc6ca4aa993a8",
                    ),
                ),
                (
                    "cuBLAS 12.8",
                    Pinned::of(
                        "https://files.pythonhosted.org/packages/70/61/7d7b3c70186fb651d0fbd35b01dbfc8e755f69fd58f817f3d0f642df20c3/nvidia_cublas_cu12-12.8.4.1-py3-none-win_amd64.whl",
                        567544208,
                        "47e9b82132fa8d2b4944e708049229601448aaad7e6f296f630f2d1a32de35af",
                    ),
                ),
                (
                    "cuRAND 10.3",
                    Pinned::of(
                        "https://files.pythonhosted.org/packages/b9/75/70c05b2f3ed5be3bb30b7102b6eb78e100da4bbf6944fd6725c012831cab/nvidia_curand_cu12-10.3.9.90-py3-none-win_amd64.whl",
                        62765309,
                        "f149a8ca457277da854f89cf282d6ef43176861926c7ac85b2a0fbd237c587ec",
                    ),
                ),
                (
                    "cuDNN 9.19",
                    Pinned::of(
                        "https://files.pythonhosted.org/packages/a7/a5/48f07449fc9c6cc146dcafe6149fa5d69630137d2ec5b7d9e09f255fadd7/nvidia_cudnn_cu12-9.19.0.56-py3-none-win_amd64.whl",
                        644003014,
                        "cec70596b9ce878fab83810c3f5a2e606d35f510e5fee579759e4cbc68a23750",
                    ),
                ),
            ]),
            lib_exts: &[".dll"],
        }),
        _ => None,
    }
}

/// Where `install` assembles the pack before moving it into place.
pub fn staging_dir(pack_dir: &Path) -> PathBuf {
    pack_dir.with_extension("partial")
}

pub fn lib_dir(pack_dir: &Path) -> PathBuf {
    pack_dir.join("lib")
}

fn marker(pack_dir: &Path) -> PathBuf {
    pack_dir.join("installed")
}

pub fn is_installed(pack_dir: &Path) -> bool {
    fs::read_to_string(marker(pack_dir)).is_ok_and(|v| v.trim() == PACK_VERSION)
}

/// Bytes `install` downloads on this platform (0 where there's no pack).
pub fn download_size(pack_dir: &Path) -> u64 {
    platform(pack_dir).map_or(0, |p| p.parts().map(|(_, w)| w.size).sum::<u64>())
}

/// How many files the GPU pack downloads.
pub fn download_parts(pack_dir: &Path) -> usize {
    platform(pack_dir).map_or(0, |p| p.parts().count())
}

impl Platform {
    /// Its downloads in the order `install` takes them: the sherpa-onnx
    /// archive, then the wheels.
    fn parts(&self) -> impl Iterator<Item = &(String, Pinned)> {
        std::iter::once(&self.sherpa).chain(&self.wheels)
    }
}

/// The list in the staging folder of the parts unpacked so far, by number
/// from 1, after the first line, `PACK_VERSION`, which says which pack
/// they're of.
fn done_list(staging: &Path) -> PathBuf {
    staging.join("unpacked")
}

/// The parts of this pack version unpacked into `staging` so far, or None
/// if it holds no such list (an older version's staging, or another pack's).
fn unpacked_parts(staging: &Path) -> Option<Vec<usize>> {
    let list = fs::read_to_string(done_list(staging)).ok()?;
    let mut lines = list.lines();
    (lines.next()? == PACK_VERSION).then(|| lines.filter_map(|n| n.parse().ok()).collect())
}

/// Records `parts` as unpacked into `staging`, the whole list written
/// beside, then put in place, so a quit leaves the old list or the new one.
fn record_unpacked(staging: &Path, parts: &[usize]) -> io::Result<()> {
    let mut list = format!("{PACK_VERSION}\n");
    for part in parts {
        list += &format!("{part}\n");
    }
    // The libraries it lists on the disk first: after a power cut, a part
    // listed is never one whose files were still only in memory.
    sync_folder(&lib_dir(staging))?;
    let next = staging.join("unpacked.next");
    let mut file = File::create(&next)?;
    io::Write::write_all(&mut file, list.as_bytes())?;
    file.sync_all()?;
    fs::rename(next, done_list(staging))?;
    sync_dir(staging)
}

/// Writes each file in `dir` and the folder itself to the disk.
fn sync_folder(dir: &Path) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            File::open(entry.path())?.sync_all()?;
        }
    }
    sync_dir(dir)
}

/// Writes folder `dir`'s entries to the disk (where a folder can be
/// opened: not on Windows, whose rename is written through).
fn sync_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    File::open(dir)?.sync_all()?;
    let _ = dir;
    Ok(())
}

/// Downloads and unpacks the GPU pack, reporting each download in turn.
/// It carries on from what an earlier install left in its staging folder
/// (a quit, a failure): the parts it unpacked are kept, and the file it was
/// downloading carries on from the bytes it had (`net::download_resumable`).
/// Staging from another pack version, or from a version that didn't list
/// what it unpacked, is deleted first. Each download matches its pinned
/// size and SHA-256 over the whole file before anything is unpacked from
/// it, and is deleted only once the list says it's unpacked, so a quit
/// while unpacking unpacks it again from the checked file (one a quit
/// left after its part was listed is deleted at the next install). The
/// libraries are on the disk before the list that names their part. Stopped or
/// failed, what it has stays for the next install; `discard` deletes it.
pub fn install(pack_dir: &Path, cancel: &AtomicBool, mut on_progress: impl FnMut(Progress)) -> Result<()> {
    let platform = platform(pack_dir).context("GPU transcription isn't available on this platform")?;
    let staging = staging_dir(pack_dir);
    let libs = lib_dir(&staging);
    let mut unpacked = match unpacked_parts(&staging) {
        Some(parts) => parts,
        None => {
            let _ = fs::remove_dir_all(&staging);
            fs::create_dir_all(&libs)?;
            record_unpacked(&staging, &[])?;
            Vec::new()
        }
    };
    fs::create_dir_all(&libs)?;

    let result = (|| -> Result<()> {
        // Progress reads "Downloading <name>" while bytes arrive (the SHA-256 is
        // computed as they do), then "<name> matches its SHA-256" while unpacking.
        // Progress counts across every file, so the bar never moves backwards.
        let (total, parts) = (download_size(pack_dir), download_parts(pack_dir));
        let mut offset = 0;
        // `before`: the bytes of the files ahead of this one.
        let mut report = |step: &str, before: u64, n: u64, size: u64, part: usize| {
            on_progress(Progress::file(step, n, Some(size)).within(before, Some(total), part, parts))
        };
        for (i, (label, Pinned { url, size, sha256 })) in platform.parts().enumerate() {
            let (part, size) = (i + 1, *size);
            let checked = format!("{label} matches its SHA‑256");
            let archive = staging.join(format!("part-{part}.download"));
            if unpacked.contains(&part) {
                // Its download, if a quit came between listing it and
                // deleting that: never moved into the pack.
                let _ = fs::remove_file(net::part_of(&archive));
                let _ = fs::remove_file(&archive);
                report(&checked, offset, size, size, part);
                offset += size;
                continue;
            }
            let downloading = format!("Downloading {label}");
            net::download_resumable(url, &archive, size, sha256, cancel, |n| {
                report(&downloading, offset, n, size, part)
            })?;
            report(&format!("{checked} · unpacking"), offset, size, size, part);
            offset += size;
            if part == 1 {
                extract_tar_libs(&archive, &libs, platform.lib_exts)?;
            } else {
                extract_wheel_libs(&archive, &libs, platform.lib_exts)?;
            }
            unpacked.push(part);
            record_unpacked(&staging, &unpacked)?;
            fs::remove_file(&archive)?;
        }
        fs::write(marker(&staging), PACK_VERSION)?;
        Ok(())
    })();
    if let Err(e) = result {
        if cancel.load(Ordering::Relaxed) {
            return Err(crate::Canceled.into());
        }
        return Err(e);
    }
    let _ = fs::remove_dir_all(pack_dir);
    fs::rename(&staging, pack_dir)?;
    // Once in place: staging is never without its list.
    let _ = fs::remove_file(done_list(pack_dir));
    Ok(())
}

/// Deletes what an install of the pack at `pack_dir` had downloaded and
/// unpacked so far (its staging folder): Cancel's, and removing the add-on's.
pub fn discard(pack_dir: &Path) -> io::Result<()> {
    crate::remove_folder(&staging_dir(pack_dir))
}

/// Removes the pack, and what an install of it had so far (`discard`).
pub fn uninstall(pack_dir: &Path) -> io::Result<()> {
    discard(pack_dir)?;
    crate::remove_folder(pack_dir)
}

/// If `path` is a shared library worth keeping, returns its file name.
fn keep_lib<'a>(path: &'a Path, exts: &[&str]) -> Option<&'a str> {
    let name = path.file_name()?.to_str()?;
    // Matches "libfoo.so", "libfoo.so.12" and "foo.dll".
    let is_lib = exts.iter().any(|ext| name.ends_with(ext) || name.contains(&format!("{ext}.")));
    (is_lib && !name.contains("tensorrt")).then_some(name)
}

/// Creates `path` afresh, so an earlier entry can't redirect the write elsewhere.
fn create_fresh(path: &Path) -> io::Result<File> {
    let _ = fs::remove_file(path);
    fs::OpenOptions::new().write(true).create_new(true).open(path)
}

/// Whether a library from the sherpa-onnx archive is ONNX Runtime (the runtime
/// itself and its CUDA provider), the only part of that archive the pack uses.
fn is_onnxruntime(name: &str) -> bool {
    name.contains("onnxruntime")
}

/// Unpacks the ONNX Runtime libraries from the sherpa-onnx archive into `dest`:
/// regular files, plus symlinks between libraries in the same directory
/// (libfoo.so -> libfoo.so.1). Hard links, devices and links pointing elsewhere
/// are skipped, as is sherpa-onnx's own library (the app ships its no-tts build).
fn extract_tar_libs(archive: &Path, dest: &Path, exts: &[&str]) -> Result<()> {
    let mut tar = tar::Archive::new(bzip2::read::BzDecoder::new(File::open(archive)?));
    for entry in tar.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let Some(name) = keep_lib(&path, exts).filter(|n| is_onnxruntime(n)) else { continue };
        let kind = entry.header().entry_type();
        if kind.is_file() {
            io::copy(&mut entry, &mut create_fresh(&dest.join(name))?)?;
        } else if kind.is_symlink() {
            let Some(target) = entry.link_name()?.map(|t| t.into_owned()) else { continue };
            let bare = target.components().count() == 1 && target.file_name().is_some();
            if bare {
                let link = dest.join(name);
                let _ = fs::remove_file(&link);
                #[cfg(unix)]
                std::os::unix::fs::symlink(&target, &link)?;
            }
        }
    }
    Ok(())
}

fn extract_wheel_libs(wheel: &Path, dest: &Path, exts: &[&str]) -> Result<()> {
    let mut zip = zip::ZipArchive::new(File::open(wheel)?)?;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i)?;
        if !file.is_file() || file.is_symlink() {
            continue;
        }
        let Some(path) = file.enclosed_name() else { continue };
        if !path.starts_with("nvidia") {
            continue;
        }
        if let Some(name) = keep_lib(&path, exts) {
            io::copy(&mut file, &mut create_fresh(&dest.join(name))?)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod install;

#[cfg(test)]
mod tests;
