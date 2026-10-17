//! Programs VODscribe downloads for the user during setup: ffmpeg (reads audio
//! and video) and Deno (a JavaScript runtime yt-dlp needs for some YouTube
//! VODs). Each download is pinned by size and SHA-256, so a replaced file is
//! refused, and only the program itself is unpacked from its archive.

use std::fs::{self, File};
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result, bail};

use crate::{Progress, net, tools};

const EXE: &str = std::env::consts::EXE_SUFFIX;

#[derive(Clone, Copy, Debug)]
pub enum Archive {
    Zip,
    TarXz,
}

/// A program to download: where from, how to check it, and where it is inside the archive.
#[derive(Debug)]
pub struct Dep {
    /// File name in the app's bin dir, without the platform's exe suffix.
    pub name: &'static str,
    /// The exact program and version, for progress.
    pub label: &'static str,
    pub url: &'static str,
    pub size: u64,
    pub sha256: &'static str,
    pub archive: Archive,
    /// The program's path inside the archive ends with this.
    pub member: &'static str,
}

/// FFmpeg 9.0 builds: BtbN's for Linux and Windows (a month-end build, which
/// BtbN keeps for about two years) and Martin Riedl's signed builds for macOS.
pub fn ffmpeg() -> Option<Dep> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Dep {
            name: "ffmpeg",
            label: "FFmpeg 9.0.1",
            url: "https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-08-31-13-27/ffmpeg-n9.0.1-11-ge47273f4d9-linux64-gpl-9.0.tar.xz",
            size: 126600656,
            sha256: "182c1b509720e939bb47bfb47dc29cc0c298640401128e3dce8627d10707eb5a",
            archive: Archive::TarXz,
            member: "/bin/ffmpeg",
        },
        ("linux", "aarch64") => Dep {
            name: "ffmpeg",
            label: "FFmpeg 9.0.1",
            url: "https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-08-31-13-27/ffmpeg-n9.0.1-11-ge47273f4d9-linuxarm64-gpl-9.0.tar.xz",
            size: 108761296,
            sha256: "e2dd447c8a47849c5812d87e54a47b20ae0f3603d38989440f4a5fe1af8755b1",
            archive: Archive::TarXz,
            member: "/bin/ffmpeg",
        },
        ("windows", "x86_64") => Dep {
            name: "ffmpeg",
            label: "FFmpeg 9.0.1",
            url: "https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-08-31-13-27/ffmpeg-n9.0.1-11-ge47273f4d9-win64-gpl-9.0.zip",
            size: 169202846,
            sha256: "ec9db2cda1f5894ab95446076ad8bf49379db4b53c02e778ad3b49adf91fec83",
            archive: Archive::Zip,
            member: "/bin/ffmpeg.exe",
        },
        ("macos", "aarch64") => Dep {
            name: "ffmpeg",
            label: "FFmpeg 9.0.2",
            url: "https://ffmpeg.martin-riedl.de/download/macos/arm64/1789931890_9.0.2/ffmpeg.zip",
            size: 28395699,
            sha256: "c8ed4c4e6978a03c485edbfe4e0a5dc2380f8a30bba5150531b31b094492d924",
            archive: Archive::Zip,
            member: "ffmpeg",
        },
        ("macos", "x86_64") => Dep {
            name: "ffmpeg",
            label: "FFmpeg 9.0.2",
            url: "https://ffmpeg.martin-riedl.de/download/macos/amd64/1789931006_9.0.2/ffmpeg.zip",
            size: 33816391,
            sha256: "7c6b4125b191cbf773832dc51f424cf2b6bb7da43007d1e066f95909e47cacd4",
            archive: Archive::Zip,
            member: "ffmpeg",
        },
        _ => return None,
    })
}

/// Deno 2.9.7, from Deno's own GitHub releases.
pub fn deno() -> Option<Dep> {
    let (url, size, sha256) = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => (
            "https://github.com/denoland/deno/releases/download/v2.9.7/deno-x86_64-unknown-linux-gnu.zip",
            41596794,
            "c6527f24f4b16031d3ae4fa9f658d5f11534c8d84ce7dc8502420280919c3490",
        ),
        ("linux", "aarch64") => (
            "https://github.com/denoland/deno/releases/download/v2.9.7/deno-aarch64-unknown-linux-gnu.zip",
            39821318,
            "c832298b1ad4422481334855f6003e0f54145762c5a134f20a489511d2f65bbf",
        ),
        ("windows", "x86_64") => (
            "https://github.com/denoland/deno/releases/download/v2.9.7/deno-x86_64-pc-windows-msvc.zip",
            42630221,
            "a0c3101b4158d1dfb7d6a78a7bf0f3de80c96bb423c152beec8beb22786f2238",
        ),
        ("macos", "aarch64") => (
            "https://github.com/denoland/deno/releases/download/v2.9.7/deno-aarch64-apple-darwin.zip",
            38469316,
            "5cd46d6268f6f78f5d88bdc7159d20bd44cdaa4b3303474839f87ec6fe7ae25c",
        ),
        ("macos", "x86_64") => (
            "https://github.com/denoland/deno/releases/download/v2.9.7/deno-x86_64-apple-darwin.zip",
            42295422,
            "95daaff11c116a52ad54785e7914c8e9c9cdcaba793c5ed929c74ca2d8e6259a",
        ),
        _ => return None,
    };
    Some(Dep {
        name: "deno",
        label: "Deno 2.9.7",
        url,
        size,
        sha256,
        archive: Archive::Zip,
        member: if cfg!(windows) { "deno.exe" } else { "deno" },
    })
}

/// Where the app keeps its own copy of program `name` (`ffmpeg`, `deno`).
pub fn managed_path(bin_dir: &Path, name: &str) -> PathBuf {
    bin_dir.join(format!("{name}{EXE}"))
}

/// Makes the downloaded program at `new` executable and moves it to `dest`,
/// replacing any older copy in one step.
pub fn install_executable(new: &Path, dest: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(new, fs::Permissions::from_mode(0o755))?;
    }
    fs::rename(new, dest).with_context(|| format!("install {}", tools::shown(dest)))
}

/// Downloads and checks `dep`, then unpacks just its program into `bin_dir`.
pub fn install(
    dep: &Dep,
    bin_dir: &Path,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(Progress),
) -> Result<PathBuf> {
    fs::create_dir_all(bin_dir)?;
    let archive = bin_dir.join(format!("{}.download", dep.name));
    let size = dep.size;
    let step = format!("Downloading {}", dep.label);
    let mut report = |step: &str, n| on_progress(Progress::file(step, n, Some(size)));
    net::download_verified(dep.url, &archive, Some(size), dep.sha256, cancel, |n| report(&step, n))?;
    report(&format!("{} matches its SHA‑256 · unpacking", dep.label), size);
    let dest = managed_path(bin_dir, dep.name);
    let new = dest.with_extension("new");
    let result = unpack(dep, &archive, &new, cancel).and_then(|()| install_executable(&new, &dest));
    // Nothing is left behind, whether it worked or not.
    let _ = fs::remove_file(&archive);
    let _ = fs::remove_file(&new);
    // A cancel during the unpack surfaces as a short read; report it as the cancel it was.
    if result.is_err() && cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(crate::Canceled.into());
    }
    result?;
    Ok(dest)
}

/// Writes the program from `archive` to `out`.
fn unpack(dep: &Dep, archive: &Path, out: &Path, cancel: &AtomicBool) -> Result<()> {
    let _ = fs::remove_file(out);
    let found = match dep.archive {
        Archive::Zip => {
            let mut zip = zip::ZipArchive::new(File::open(archive)?)?;
            let index = (0..zip.len()).find(|&i| {
                zip.by_index(i)
                    .is_ok_and(|f| f.is_file() && !f.is_symlink() && f.name().is_ok_and(|n| is_member(&n, dep.member)))
            });
            match index {
                Some(i) => {
                    io::copy(&mut zip.by_index(i)?, &mut File::create(out)?)?;
                    true
                }
                None => false,
            }
        }
        // The xz decoder writes rather than reads, so it runs on its own thread
        // into a pipe the tar reader reads from; nothing is unpacked to disk but
        // the program, and reading stops as soon as it's found.
        Archive::TarXz => std::thread::scope(|scope| -> Result<bool> {
            let (reader, mut writer) = io::pipe()?;
            let input = File::open(archive)?;
            scope.spawn(move || {
                let mut input = BufReader::new(net::ProgressReader::new(input, cancel, |_| {}));
                // Fails with a broken pipe once the reader has what it needs; that's fine.
                let _ = lzma_rs::xz_decompress(&mut input, &mut writer);
            });
            let mut tar = tar::Archive::new(reader);
            for entry in tar.entries()? {
                let mut entry = entry?;
                let path = entry.path()?.to_string_lossy().into_owned();
                if entry.header().entry_type().is_file() && is_member(&path, dep.member) {
                    io::copy(&mut entry, &mut File::create(out)?)?;
                    return Ok(true);
                }
            }
            Ok(false)
        })?,
    };
    if !found {
        bail!("{} wasn't where expected in its download", dep.name);
    }
    Ok(())
}

/// Whether archive entry `name` is the program: `member` exactly, or a path ending in it.
fn is_member(name: &str, member: &str) -> bool {
    let name = name.replace('\\', "/");
    name == member.trim_start_matches('/') || (member.starts_with('/') && name.ends_with(member))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_program_in_its_archive() {
        assert!(is_member("ffmpeg-n9.0-linux64-gpl-9.0/bin/ffmpeg", "/bin/ffmpeg"));
        assert!(!is_member("ffmpeg-n9.0-linux64-gpl-9.0/bin/ffprobe", "/bin/ffmpeg"));
        assert!(!is_member("ffmpeg-n9.0/doc/bin/ffmpeg.html", "/bin/ffmpeg"));
        assert!(is_member("ffmpeg", "ffmpeg"));
        assert!(!is_member("sub/ffmpeg", "ffmpeg"));
        assert!(is_member("x\\bin\\ffmpeg.exe", "/bin/ffmpeg.exe"));
    }

    #[test]
    fn every_platform_we_build_for_has_both() {
        // Whatever this machine is, it's one we ship for.
        assert!(ffmpeg().is_some() && deno().is_some());
    }

    /// Downloads and unpacks the real ffmpeg and Deno for this machine into
    /// $VODSCRIBE_TEST_BIN_DIR: `cargo test -p vodscribe-core -- --ignored`.
    #[test]
    #[ignore]
    fn installs_the_pinned_programs() {
        let dir = PathBuf::from(std::env::var("VODSCRIBE_TEST_BIN_DIR").expect("set VODSCRIBE_TEST_BIN_DIR"));
        let never = AtomicBool::new(false);
        for dep in [ffmpeg().unwrap(), deno().unwrap()] {
            let path = install(&dep, &dir, &never, |_| {}).unwrap();
            let out = std::process::Command::new(&path)
                .arg(if dep.name == "deno" { "--version" } else { "-version" })
                .output()
                .unwrap();
            assert!(out.status.success(), "{} doesn't run", dep.name);
            assert!(fs::read_dir(&dir).unwrap().count() <= 2, "only the programs are left behind");
        }
    }
}
