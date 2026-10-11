//! Optional NVIDIA GPU transcription. The "GPU pack" is sherpa-onnx's CUDA
//! build plus NVIDIA's redistributable CUDA 12 and cuDNN 9 runtime libraries
//! (from their official PyPI wheels), downloaded into the app's data dir. The
//! GPU worker binary ships with the app and loads its libraries from there.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::Value;

use crate::{net, tools};

/// Free VRAM needed before a job is sent to the GPU.
// TODO: measure with the int8 Parakeet model once the GPU is free; this is a
// conservative estimate.
pub const MIN_FREE_MB: u64 = 3000;

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

struct Platform {
    sherpa_url: &'static str,
    /// The wheel platform tag to pick from PyPI.
    wheel_tag: &'static str,
    /// Library file extensions to keep.
    lib_exts: &'static [&'static str],
}

fn platform() -> Option<Platform> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some(Platform {
            sherpa_url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.8/sherpa-onnx-v1.13.8-cuda-12.x-cudnn-9.x-onnxruntime1.28.2-linux-x64-gpu.tar.bz2",
            wheel_tag: "manylinux",
            lib_exts: &[".so"],
        }),
        ("windows", "x86_64") => Some(Platform {
            sherpa_url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.8/sherpa-onnx-v1.13.8-cuda-12.x-cudnn-9.x-onnxruntime1.28.2-win-x64-cuda.tar.bz2",
            wheel_tag: "win_amd64",
            lib_exts: &[".dll"],
        }),
        _ => None,
    }
}

/// NVIDIA runtime wheels, pinned to a set known to work together.
const WHEELS: [(&str, &str); 4] = [
    ("nvidia-cuda-runtime-cu12", "12.8.90"),
    ("nvidia-cublas-cu12", "12.8.4.1"),
    ("nvidia-curand-cu12", "10.3.9.90"),
    ("nvidia-cudnn-cu12", "9.19.0.56"),
];

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
    let staging = pack_dir.with_extension("partial");
    let _ = fs::remove_dir_all(&staging);
    let libs = lib_dir(&staging);
    fs::create_dir_all(&libs)?;

    let result = (|| -> Result<()> {
        // sherpa-onnx + ONNX Runtime with the CUDA provider: stream the archive, keep the libraries.
        let (body, len) = net::get(platform.sherpa_url)?;
        let total = len.unwrap_or(0);
        let reader = net::ProgressReader::new(body, cancel, |n| on_progress("ONNX Runtime (CUDA)", n, total));
        let mut archive = tar::Archive::new(bzip2::read::BzDecoder::new(reader));
        for entry in archive.entries()? {
            let mut entry = entry?;
            let path = entry.path()?.into_owned();
            if let Some(name) = keep_lib(&path, platform.lib_exts) {
                entry.unpack(libs.join(name))?;
            }
        }

        for (name, version) in WHEELS {
            let (url, size) = wheel_url(name, version, platform.wheel_tag)?;
            let wheel = staging.join(format!("{name}.whl"));
            net::download(&url, &wheel, cancel, |n, _| on_progress(name, n, size))?;
            extract_wheel_libs(&wheel, &libs, platform.lib_exts)?;
            fs::remove_file(&wheel)?;
        }
        fs::write(marker(&staging), PACK_VERSION)?;
        Ok(())
    })();
    if let Err(e) = result {
        let _ = fs::remove_dir_all(&staging);
        if cancel.load(Ordering::Relaxed) {
            bail!("canceled");
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

fn wheel_url(name: &str, version: &str, tag: &str) -> Result<(String, u64)> {
    let (mut body, _) = net::get(&format!("https://pypi.org/pypi/{name}/{version}/json"))?;
    let mut text = String::new();
    body.read_to_string(&mut text)?;
    let json: Value = serde_json::from_str(&text)?;
    let files = json["urls"].as_array().context("unexpected PyPI response")?;
    let file = files
        .iter()
        .find(|f| {
            let fname = f["filename"].as_str().unwrap_or_default();
            fname.ends_with(".whl") && fname.contains(tag) && (tag != "manylinux" || fname.contains("x86_64"))
        })
        .with_context(|| format!("no {tag} wheel for {name} {version}"))?;
    Ok((file["url"].as_str().context("wheel has no URL")?.to_string(), file["size"].as_u64().unwrap_or(0)))
}

fn extract_wheel_libs(wheel: &Path, dest: &Path, exts: &[&str]) -> Result<()> {
    let mut zip = zip::ZipArchive::new(File::open(wheel)?)?;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i)?;
        let Some(path) = file.enclosed_name() else { continue };
        if !path.starts_with("nvidia") {
            continue;
        }
        if let Some(name) = keep_lib(&path, exts) {
            let mut out = File::create(dest.join(name))?;
            io::copy(&mut file, &mut out)?;
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
    fn keeps_only_runtime_libraries() {
        let so = [".so"];
        assert_eq!(keep_lib(Path::new("x/lib/libcudnn.so.9"), &so), Some("libcudnn.so.9"));
        assert_eq!(keep_lib(Path::new("x/lib/libonnxruntime.so"), &so), Some("libonnxruntime.so"));
        assert_eq!(keep_lib(Path::new("x/lib/libonnxruntime_providers_tensorrt.so"), &so), None);
        assert_eq!(keep_lib(Path::new("x/include/cudnn.h"), &so), None);
        assert_eq!(keep_lib(Path::new("x/bin/cudnn64_9.dll"), &[".dll"]), Some("cudnn64_9.dll"));
    }
}
