//! External binaries: yt-dlp (app-managed so it can stay current as platforms
//! change) and ffmpeg (from PATH or the app's bin dir).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result, bail};

use crate::net;

const EXE: &str = std::env::consts::EXE_SUFFIX;

/// How often code waiting on a child process checks whether to stop it.
pub const STOP_POLL: std::time::Duration = std::time::Duration::from_millis(100);

fn yt_dlp_asset() -> Result<&'static str> {
    Ok(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "yt-dlp_linux",
        ("linux", "aarch64") => "yt-dlp_linux_aarch64",
        ("macos", _) => "yt-dlp_macos",
        ("windows", "x86_64") => "yt-dlp.exe",
        ("windows", "aarch64") => "yt-dlp_arm64.exe",
        (os, arch) => bail!("no yt-dlp build for {os}/{arch}"),
    })
}

/// The yt-dlp binary the app manages inside `bin_dir`.
pub fn managed_yt_dlp(bin_dir: &Path) -> PathBuf {
    bin_dir.join(format!("yt-dlp{EXE}"))
}

/// A yt-dlp binary plus the JavaScript runtime it needs for YouTube, if one exists.
#[derive(Clone, Debug)]
pub struct YtDlp {
    pub path: PathBuf,
    pub js_runtime: Option<(&'static str, PathBuf)>,
}

impl YtDlp {
    pub fn command(&self) -> Command {
        let mut cmd = command(&self.path);
        // A yt-dlp.conf (user, system, or one planted in the working dir) could
        // add --exec, browser cookies or output paths outside the work dir.
        cmd.arg("--ignore-config");
        if let Some((name, path)) = &self.js_runtime {
            cmd.arg("--js-runtimes").arg(format!("{name}:{}", path.display()));
        }
        cmd
    }
}

/// Prefers the managed yt-dlp; a system install is often too stale for YouTube.
pub fn find_yt_dlp(bin_dir: &Path) -> Option<YtDlp> {
    let managed = managed_yt_dlp(bin_dir);
    let path = if managed.is_file() { managed } else { which::which("yt-dlp").ok()? };
    Some(YtDlp { path, js_runtime: find_js_runtime(bin_dir) })
}

/// yt-dlp solves YouTube's JavaScript challenges with an external runtime.
pub fn find_js_runtime(bin_dir: &Path) -> Option<(&'static str, PathBuf)> {
    ["deno", "node", "bun"].into_iter().find_map(|name| {
        let local = bin_dir.join(format!("{name}{EXE}"));
        let path = if local.is_file() { local } else { which::which(name).ok()? };
        Some((name, path))
    })
}

pub fn find_ffmpeg(bin_dir: &Path) -> Option<PathBuf> {
    let local = bin_dir.join(format!("ffmpeg{EXE}"));
    if local.is_file() {
        return Some(local);
    }
    which::which("ffmpeg").ok()
}

/// yt-dlp's release signing key (fingerprint AC0CBBE6848D6A873464AF4E57CF65933B5A7581),
/// from https://github.com/yt-dlp/yt-dlp/blob/master/public.key.
const YT_DLP_KEY: &str = include_str!("yt-dlp-signing-key.asc");
const YT_DLP_RELEASE: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download";

/// Checks the release's signed checksum list and returns `asset`'s SHA-256.
fn signed_sha256(sums: &[u8], sig: &[u8], asset: &str) -> Result<String> {
    use pgp::composed::{Deserializable, DetachedSignature, SignedPublicKey};
    let (key, _) = SignedPublicKey::from_armor_single(YT_DLP_KEY.as_bytes())?;
    let sig = DetachedSignature::from_bytes(sig).context("read yt-dlp's checksum signature")?;
    sig.verify(&key, sums)
        .context("yt-dlp's download couldn't be verified against its release signature, so it wasn't installed")?;
    std::str::from_utf8(sums)?
        .lines()
        .find_map(|line| {
            let (hash, name) = line.split_once(char::is_whitespace)?;
            (name.trim_start_matches([' ', '*']) == asset).then(|| hash.to_ascii_lowercase())
        })
        .with_context(|| format!("yt-dlp's checksum list has no entry for {asset}"))
}

/// Checksum lists and their signatures are a few KB.
const SUMS_MAX_BYTES: u64 = 1 << 20;

/// Downloads the latest yt-dlp release into `bin_dir`, verified against the
/// release's checksum list, which must carry the yt-dlp signing key's signature.
/// Returns the binary's path and whether it changed.
pub fn install_yt_dlp(
    bin_dir: &Path,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(u64, Option<u64>),
) -> Result<(PathBuf, bool)> {
    fs::create_dir_all(bin_dir)?;
    let asset = yt_dlp_asset()?;
    let sums = net::get_bytes(&format!("{YT_DLP_RELEASE}/SHA2-256SUMS"), SUMS_MAX_BYTES, cancel)?;
    let sig = net::get_bytes(&format!("{YT_DLP_RELEASE}/SHA2-256SUMS.sig"), SUMS_MAX_BYTES, cancel)?;
    let sha256 = signed_sha256(&sums, &sig, asset)?;

    let dest = managed_yt_dlp(bin_dir);
    if file_sha256(&dest).is_some_and(|h| h == sha256) {
        return Ok((dest, false));
    }
    let new = dest.with_extension("new");
    net::download_verified(&format!("{YT_DLP_RELEASE}/{asset}"), &new, None, &sha256, cancel, |n| on_progress(n, None))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&new, fs::Permissions::from_mode(0o755))?;
    }
    fs::rename(&new, &dest)?;
    Ok((dest, true))
}

fn file_sha256(path: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(fs::read(path).ok()?);
    Some(net::hex(&hasher.finalize()))
}

/// Updates the managed yt-dlp the same verified way it was installed (rather
/// than with its own `-U`) and describes the result.
pub fn update_yt_dlp(bin_dir: &Path) -> Result<String> {
    let (path, changed) = install_yt_dlp(bin_dir, &AtomicBool::new(false), |_, _| {})?;
    let out = command(&path).arg("--version").output().context("run yt-dlp --version")?;
    let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok(if changed { format!("Updated yt-dlp to {version}") } else { format!("yt-dlp is up to date ({version})") })
}

/// Reads a child's stream to the end on its own thread, so a chatty process
/// can't block on a full pipe; join the handle for the text.
pub fn drain(mut stream: impl std::io::Read + Send + 'static) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stream.read_to_string(&mut s);
        s
    })
}

/// Sends each line of a child's stream down `tx` from its own thread, so the
/// reader can wait with a timeout (and notice a stop) instead of blocking.
pub fn send_lines(
    stream: impl std::io::Read + Send + 'static,
    tx: std::sync::mpsc::Sender<String>,
) -> std::thread::JoinHandle<()> {
    use std::io::BufRead;
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(stream).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    })
}

/// The last `n` non-blank lines of `text`, for error messages from a tool's log.
pub fn last_lines(text: &str, n: usize) -> String {
    let tail: Vec<&str> = text.lines().rev().filter(|l| !l.trim().is_empty()).take(n).collect();
    tail.into_iter().rev().collect::<Vec<_>>().join("\n")
}

/// A helper binary shipped next to the running executable, such as the worker.
pub fn sidecar(name: &str) -> Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let path = exe.parent().context("executable has no parent dir")?.join(format!("{name}{EXE}"));
    if !path.is_file() {
        bail!("{} is missing", path.display());
    }
    Ok(path)
}

/// A `Command` that doesn't flash a console window on Windows.
/// Runs in the program's own directory rather than wherever the app was launched
/// from, so nothing in an untrusted working dir (configs, DLLs) gets picked up.
pub fn command(program: &Path) -> Command {
    let mut cmd = Command::new(program);
    if let Some(dir) = program.parent().filter(|d| d.is_dir()) {
        cmd.current_dir(dir);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_last_non_blank_lines() {
        assert_eq!(last_lines("a\nb\n\nc\n  \n", 2), "b\nc");
        assert_eq!(last_lines("", 3), "");
    }

    /// Installs the latest release into $VODSCRIBE_TEST_BIN_DIR, then again (a no-op).
    #[test]
    #[ignore]
    fn installs_verified_yt_dlp() {
        let dir = PathBuf::from(std::env::var("VODSCRIBE_TEST_BIN_DIR").expect("set VODSCRIBE_TEST_BIN_DIR"));
        let (path, changed) = install_yt_dlp(&dir, &AtomicBool::new(false), |_, _| {}).unwrap();
        assert!(changed && path.is_file());
        assert!(!install_yt_dlp(&dir, &AtomicBool::new(false), |_, _| {}).unwrap().1);
        assert!(update_yt_dlp(&dir).unwrap().starts_with("yt-dlp is up to date"));
    }

    /// The release published 2026.08.19: its checksum list and signature.
    #[test]
    fn verifies_signed_checksums() {
        let sums = include_bytes!("../tests/data/yt-dlp-SHA2-256SUMS");
        let sig = include_bytes!("../tests/data/yt-dlp-SHA2-256SUMS.sig");
        let hash = signed_sha256(sums, sig, "yt-dlp_linux").unwrap();
        assert_eq!(hash, "58162f9bfdc27458ea47bfcb311cf47028f17d8154a8bf7d689861d46399230a");
        let mut tampered = sums.to_vec();
        tampered[0] ^= 1;
        assert!(signed_sha256(&tampered, sig, "yt-dlp_linux").is_err());
        assert!(signed_sha256(sums, sig, "yt-dlp_nonexistent").is_err());
    }
}
