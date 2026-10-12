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

/// Downloads the latest yt-dlp release into `bin_dir`.
pub fn install_yt_dlp(
    bin_dir: &Path,
    cancel: &AtomicBool,
    on_progress: impl FnMut(u64, Option<u64>),
) -> Result<PathBuf> {
    fs::create_dir_all(bin_dir)?;
    let url = format!(
        "https://github.com/yt-dlp/yt-dlp/releases/latest/download/{}",
        yt_dlp_asset()?
    );
    let dest = managed_yt_dlp(bin_dir);
    net::download(&url, &dest, cancel, on_progress)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dest, fs::Permissions::from_mode(0o755))?;
    }
    Ok(dest)
}

/// Runs the managed yt-dlp's self-updater and returns its output.
pub fn update_yt_dlp(bin_dir: &Path) -> Result<String> {
    let path = managed_yt_dlp(bin_dir);
    let out = command(&path).arg("-U").output().context("run yt-dlp -U")?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() {
        bail!("{text}\n{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(text)
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
pub fn command(program: &Path) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}
