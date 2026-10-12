//! Optional NVIDIA GPU transcription. The "GPU pack" is sherpa-onnx's CUDA
//! build plus NVIDIA's redistributable CUDA 12 and cuDNN 9 runtime libraries
//! (from their official PyPI wheels), downloaded into the app's data dir. The
//! GPU worker binary ships with the app and loads its libraries from there.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::{net, tools};

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
const PACK_VERSION: &str = "1";

#[derive(Serialize, Clone, Debug)]
pub struct GpuInfo {
    pub name: String,
    pub total_mb: u64,
    pub free_mb: u64,
}

/// Queries the first NVIDIA GPU. `nvidia-smi` ships with the driver on Linux and Windows.
pub fn detect() -> Option<GpuInfo> {
    let smi = which::which("nvidia-smi").ok()?;
    let out = tools::command(&smi)
        .args(["--query-gpu=name,memory.total,memory.free", "--format=csv,noheader,nounits"])
        .stdin(Stdio::null())
        .output()
        .ok()?;
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

/// Whether a GPU pack exists for this platform.
pub fn supported() -> bool {
    platform().is_some()
}

/// A download pinned by size and SHA-256, so a replaced release asset or
/// wheel is refused rather than loaded into the GPU worker.
struct Pinned {
    url: &'static str,
    size: u64,
    sha256: &'static str,
}

struct Platform {
    /// sherpa-onnx + ONNX Runtime built with the CUDA provider.
    sherpa: Pinned,
    /// NVIDIA's CUDA 12 / cuDNN 9 runtime wheels from PyPI, labelled for progress.
    wheels: [(&'static str, Pinned); 4],
    /// Library file extensions to keep.
    lib_exts: &'static [&'static str],
}

fn platform() -> Option<Platform> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some(Platform {
            sherpa: Pinned {
                url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.8/sherpa-onnx-v1.13.8-cuda-12.x-cudnn-9.x-onnxruntime1.28.2-linux-x64-gpu.tar.bz2",
                size: 393010348,
                sha256: "2132af1ae8c84c84f86ac091e4311839287851d2bf5e660332ebce3898a1e0b2",
            },
            wheels: [
                ("CUDA runtime 12.8", Pinned {
                    url: "https://files.pythonhosted.org/packages/0d/9b/a997b638fcd068ad6e4d53b8551a7d30fe8b404d6f1804abf1df69838932/nvidia_cuda_runtime_cu12-12.8.90-py3-none-manylinux2014_x86_64.manylinux_2_17_x86_64.whl",
                    size: 954765,
                    sha256: "adade8dcbd0edf427b7204d480d6066d33902cab2a4707dcfc48a2d0fd44ab90",
                }),
                ("cuBLAS 12.8", Pinned {
                    url: "https://files.pythonhosted.org/packages/dc/61/e24b560ab2e2eaeb3c839129175fb330dfcfc29e5203196e5541a4c44682/nvidia_cublas_cu12-12.8.4.1-py3-none-manylinux_2_27_x86_64.whl",
                    size: 594346921,
                    sha256: "8ac4e771d5a348c551b2a426eda6193c19aa630236b418086020df5ba9667142",
                }),
                ("cuRAND 10.3", Pinned {
                    url: "https://files.pythonhosted.org/packages/fb/aa/6584b56dc84ebe9cf93226a5cde4d99080c8e90ab40f0c27bda7a0f29aa1/nvidia_curand_cu12-10.3.9.90-py3-none-manylinux_2_27_x86_64.whl",
                    size: 63619976,
                    sha256: "b32331d4f4df5d6eefa0554c565b626c7216f87a06a4f56fab27c3b68a830ec9",
                }),
                ("cuDNN 9.19", Pinned {
                    url: "https://files.pythonhosted.org/packages/c5/41/65225d42fba06fb3dd3972485ea258e7dd07a40d6e01c95da6766ad87354/nvidia_cudnn_cu12-9.19.0.56-py3-none-manylinux_2_27_x86_64.whl",
                    size: 657906812,
                    sha256: "ac6ad90a075bb33a94f2b4cf4622eac13dd4dc65cf6dd9c7572a318516a36625",
                }),
            ],
            lib_exts: &[".so"],
        }),
        ("windows", "x86_64") => Some(Platform {
            sherpa: Pinned {
                url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.8/sherpa-onnx-v1.13.8-cuda-12.x-cudnn-9.x-onnxruntime1.28.2-win-x64-cuda.tar.bz2",
                size: 595017373,
                sha256: "066c5b54dbafaa1388001a9c9837ac1374dbba6d6678f193ca06aa0d8e94d8c3",
            },
            wheels: [
                ("CUDA runtime 12.8", Pinned {
                    url: "https://files.pythonhosted.org/packages/30/a5/a515b7600ad361ea14bfa13fb4d6687abf500adc270f19e89849c0590492/nvidia_cuda_runtime_cu12-12.8.90-py3-none-win_amd64.whl",
                    size: 944318,
                    sha256: "c0c6027f01505bfed6c3b21ec546f69c687689aad5f1a377554bc6ca4aa993a8",
                }),
                ("cuBLAS 12.8", Pinned {
                    url: "https://files.pythonhosted.org/packages/70/61/7d7b3c70186fb651d0fbd35b01dbfc8e755f69fd58f817f3d0f642df20c3/nvidia_cublas_cu12-12.8.4.1-py3-none-win_amd64.whl",
                    size: 567544208,
                    sha256: "47e9b82132fa8d2b4944e708049229601448aaad7e6f296f630f2d1a32de35af",
                }),
                ("cuRAND 10.3", Pinned {
                    url: "https://files.pythonhosted.org/packages/b9/75/70c05b2f3ed5be3bb30b7102b6eb78e100da4bbf6944fd6725c012831cab/nvidia_curand_cu12-10.3.9.90-py3-none-win_amd64.whl",
                    size: 62765309,
                    sha256: "f149a8ca457277da854f89cf282d6ef43176861926c7ac85b2a0fbd237c587ec",
                }),
                ("cuDNN 9.19", Pinned {
                    url: "https://files.pythonhosted.org/packages/a7/a5/48f07449fc9c6cc146dcafe6149fa5d69630137d2ec5b7d9e09f255fadd7/nvidia_cudnn_cu12-9.19.0.56-py3-none-win_amd64.whl",
                    size: 644003014,
                    sha256: "cec70596b9ce878fab83810c3f5a2e606d35f510e5fee579759e4cbc68a23750",
                }),
            ],
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

/// Downloads and unpacks the GPU pack. `on_progress` receives
/// (step label, bytes done, bytes total) for each download in turn.
pub fn install(pack_dir: &Path, cancel: &AtomicBool, mut on_progress: impl FnMut(&str, u64, u64)) -> Result<()> {
    let platform = platform().context("GPU transcription isn't available on this platform")?;
    let staging = staging_dir(pack_dir);
    let _ = fs::remove_dir_all(&staging);
    let libs = lib_dir(&staging);
    fs::create_dir_all(&libs)?;

    let result = (|| -> Result<()> {
        // Each download is verified before anything is unpacked from it.
        let Pinned { url, size, sha256 } = platform.sherpa;
        let archive = staging.join("sherpa-onnx.tar.bz2");
        // Progress reads "Downloading <name>" while bytes arrive (the SHA-256 is
        // computed as they do), then "<name> matches its SHA-256" while unpacking.
        let label = "ONNX Runtime 1.28 with CUDA (sherpa-onnx 1.13.8)";
        let downloading = format!("Downloading {label}");
        net::download_verified(url, &archive, Some(size), sha256, cancel, |n| on_progress(&downloading, n, size))?;
        on_progress(&format!("{label} matches its SHA‑256 · unpacking"), size, size);
        extract_tar_libs(&archive, &libs, platform.lib_exts)?;
        fs::remove_file(&archive)?;

        for (label, Pinned { url, size, sha256 }) in platform.wheels {
            let wheel = staging.join("wheel.whl");
            let downloading = format!("Downloading {label}");
            net::download_verified(url, &wheel, Some(size), sha256, cancel, |n| on_progress(&downloading, n, size))?;
            on_progress(&format!("{label} matches its SHA‑256 · unpacking"), size, size);
            extract_wheel_libs(&wheel, &libs, platform.lib_exts)?;
            fs::remove_file(&wheel)?;
        }
        fs::write(marker(&staging), PACK_VERSION)?;
        Ok(())
    })();
    if let Err(e) = result {
        let _ = fs::remove_dir_all(&staging);
        if cancel.load(Ordering::Relaxed) {
            return Err(crate::Canceled.into());
        }
        return Err(e);
    }
    let _ = fs::remove_dir_all(pack_dir);
    fs::rename(&staging, pack_dir)?;
    Ok(())
}

pub fn uninstall(pack_dir: &Path) -> io::Result<()> {
    match fs::remove_dir_all(pack_dir) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        r => r,
    }
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

/// Unpacks the libraries from the sherpa-onnx archive into `dest`: regular files,
/// plus symlinks between libraries in the same directory (libfoo.so -> libfoo.so.1).
/// Hard links, devices and links pointing elsewhere are skipped.
fn extract_tar_libs(archive: &Path, dest: &Path, exts: &[&str]) -> Result<()> {
    let mut tar = tar::Archive::new(bzip2::read::BzDecoder::new(File::open(archive)?));
    for entry in tar.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let Some(name) = keep_lib(&path, exts) else { continue };
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
mod tests {
    use super::*;

    #[test]
    fn parses_nvidia_smi() {
        let info = parse_smi("NVIDIA GeForce RTX 3080, 10240, 4039\n").unwrap();
        assert_eq!(info.name, "NVIDIA GeForce RTX 3080");
        assert_eq!((info.total_mb, info.free_mb), (10240, 4039));
        assert!(parse_smi("").is_none());
    }

    #[test]
    fn batch_follows_free_vram() {
        assert_eq!(batch_for_free_vram(9000), Some(8));
        assert_eq!(batch_for_free_vram(6000), Some(4));
        assert_eq!(batch_for_free_vram(3500), Some(1));
        assert_eq!(batch_for_free_vram(3499), None);
    }

    /// Downloads the real pack (~1.7 GB) into $VODSCRIBE_TEST_PACK_DIR, checking the
    /// pinned hashes and the unpacked libraries: `cargo test -p vodscribe-core -- --ignored`.
    #[test]
    #[ignore]
    fn installs_the_pinned_pack() {
        let dir = PathBuf::from(std::env::var("VODSCRIBE_TEST_PACK_DIR").expect("set VODSCRIBE_TEST_PACK_DIR"));
        install(&dir, &AtomicBool::new(false), |_, _, _| {}).unwrap();
        assert!(is_installed(&dir));
        let names: Vec<String> =
            fs::read_dir(lib_dir(&dir)).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        for want in ["onnxruntime", "cudnn", "cublas", "cudart", "curand"] {
            assert!(names.iter().any(|n| n.contains(want)), "no {want} library in {names:?}");
        }
    }

    #[test]
    fn keeps_only_runtime_libraries() {
        let so = [".so"];
        assert_eq!(keep_lib(Path::new("x/lib/libcudnn.so.9"), &so), Some("libcudnn.so.9"));
        assert_eq!(keep_lib(Path::new("x/lib/libonnxruntime.so"), &so), Some("libonnxruntime.so"));
        assert_eq!(keep_lib(Path::new("x/lib/libonnxruntime_providers_tensorrt.so"), &so), None);
        assert_eq!(keep_lib(Path::new("x/include/cudnn.h"), &so), None);
        assert_eq!(keep_lib(Path::new("x/bin/cudnn64_9.dll"), &[".dll"]), Some("cudnn64_9.dll"));
    }
}
