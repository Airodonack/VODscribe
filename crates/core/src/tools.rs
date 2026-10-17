//! External binaries: yt-dlp (app-managed so it can stay current as platforms
//! change) and ffmpeg (from PATH or the app's bin dir).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result, bail};

use crate::{Progress, net};

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
    crate::deps::managed_path(bin_dir, "yt-dlp")
}

/// A yt-dlp binary plus the JavaScript runtime it needs for YouTube, if one exists.
#[derive(Clone, Debug)]
pub struct YtDlp {
    pub path: PathBuf,
    pub js_runtime: Option<(&'static str, PathBuf)>,
}

impl YtDlp {
    pub fn command(&self) -> Command {
        let mut cmd = self.command_any_extractor();
        // Only sites yt-dlp knows by name. The generic extractor reads any web
        // page for a video in it, following whatever the page points to.
        cmd.args(["--use-extractors", "default,-generic"]);
        cmd
    }

    /// `command` with the generic extractor too, which reads a direct stream
    /// link (an HLS manifest). Only for a link the app got from a platform's
    /// own API (Kick's), never one pasted in.
    pub fn command_any_extractor(&self) -> Command {
        let mut cmd = command(&self.path);
        // A yt-dlp.conf (user, system, or one planted in the working dir) could
        // add --exec, browser cookies or output paths outside the work dir.
        cmd.arg("--ignore-config");
        // Nor plugins: Python code from folders on this computer, run by yt-dlp.
        cmd.arg("--no-plugin-dirs");
        // Python writes to a pipe in Windows' ANSI code page unless told otherwise,
        // which mangles paths with characters outside it (a user named Jürgen).
        cmd.env("PYTHONIOENCODING", "utf-8").env("PYTHONUTF8", "1");
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
        let local = crate::deps::managed_path(bin_dir, name);
        let path = if local.is_file() { local } else { which::which(name).ok()? };
        Some((name, path))
    })
}

pub fn find_ffmpeg(bin_dir: &Path) -> Option<PathBuf> {
    let local = crate::deps::managed_path(bin_dir, "ffmpeg");
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
        .context("The video downloader couldn't be checked, so it wasn't installed. Try again later.")?;
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
/// Returns the binary's path and whether it changed. Its steps are named like
/// the GPU add-on's; its size isn't known until it arrives.
pub fn install_yt_dlp(
    bin_dir: &Path,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(Progress),
) -> Result<(PathBuf, bool)> {
    fs::create_dir_all(bin_dir)?;
    let asset = yt_dlp_asset()?;
    on_progress(Progress::step("Checking yt-dlp's release signature"));
    let sums = net::get_bytes(&format!("{YT_DLP_RELEASE}/SHA2-256SUMS"), SUMS_MAX_BYTES, cancel)?;
    let sig = net::get_bytes(&format!("{YT_DLP_RELEASE}/SHA2-256SUMS.sig"), SUMS_MAX_BYTES, cancel)?;
    let sha256 = signed_sha256(&sums, &sig, asset)?;

    let dest = managed_yt_dlp(bin_dir);
    if file_sha256(&dest).is_some_and(|h| h == sha256) {
        return Ok((dest, false));
    }
    let new = dest.with_extension("new");
    net::download_verified(&format!("{YT_DLP_RELEASE}/{asset}"), &new, None, &sha256, cancel, |n| {
        on_progress(Progress::file("Downloading yt-dlp", n, None))
    })?;
    on_progress(Progress::step("yt-dlp matches its signed SHA‑256 · installing"));
    crate::deps::install_executable(&new, &dest)?;
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
pub fn update_yt_dlp(bin_dir: &Path, on_progress: impl FnMut(Progress)) -> Result<String> {
    let (path, changed) = install_yt_dlp(bin_dir, &AtomicBool::new(false), on_progress)?;
    let out = command(&path).arg("--version").output().context("run yt-dlp --version")?;
    let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok(if changed {
        format!("Updated yt-dlp to {version} · signature and SHA‑256 checked")
    } else {
        format!("yt-dlp {version} is up to date · signature and SHA‑256 checked")
    })
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
        // Bytes that aren't UTF-8 (a program writing in Windows' code page) are
        // replaced rather than ending the stream.
        let mut reader = std::io::BufReader::new(stream);
        let mut buf = Vec::new();
        while matches!(reader.read_until(b'\n', &mut buf), Ok(n) if n > 0) {
            let line = String::from_utf8_lossy(&buf).trim_end_matches(['\r', '\n']).to_string();
            buf.clear();
            if tx.send(line).is_err() {
                break;
            }
        }
    })
}

/// The last `n` non-blank lines of `text`, for error messages from a tool's log.
pub fn last_lines(text: &str, n: usize) -> String {
    let tail: Vec<&str> = text.lines().rev().filter(|l| !l.trim().is_empty()).take(n).collect();
    hide_home(&tail.into_iter().rev().collect::<Vec<_>>().join("\n"))
}

/// `text` (a tool's error lines, which name the files it was given) with
/// the home folder written `~`, as `shown` writes a path.
pub fn hide_home(text: &str) -> String {
    let Some(home) = std::env::home_dir().filter(|h| !h.as_os_str().is_empty()) else { return text.into() };
    let home = home.display().to_string();
    let home = home.trim_end_matches(['/', '\\']);
    if home.is_empty() {
        return text.into();
    }
    // Windows tools may write it either way round.
    text.replace(home, "~").replace(&home.replace('\\', "/"), "~")
}

/// A helper binary shipped next to the running executable, such as the worker.
pub fn sidecar(name: &str) -> Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let path = exe.parent().context("executable has no parent dir")?.join(format!("{name}{EXE}"));
    if !path.is_file() {
        bail!("{} is missing", shown(path));
    }
    Ok(path)
}

// What ffmpeg may use on a file it reads: every file is from somewhere else (a
// site, or whatever was added from the computer), and ffmpeg's rarely used
// demuxers and decoders are where its security bugs have been. The lists hold
// what the app's files use (local::MEDIA_TYPES, and what sites serve); a file
// needing anything else fails to open rather than reaching that code.

/// Containers (demuxer names).
pub const FFMPEG_FORMATS: &str = "mov,mp4,m4a,3gp,3g2,mj2,matroska,webm,avi,mpegts,flv,mp3,aac,wav,flac,ogg";
/// Audio decoders, for reading the sound (transcribing, and a file's details).
pub const FFMPEG_AUDIO: &str = "aac,aac_fixed,aac_latm,mp3,mp3float,mp2,mp2float,mp1,mp1float,opus,libopus,\
     vorbis,libvorbis,flac,alac,ac3,ac3_fixed,eac3,dca,truehd,mlp,pcm_s16le,pcm_s16be,pcm_s24le,pcm_s24be,\
     pcm_s32le,pcm_s32be,pcm_f32le,pcm_f32be,pcm_f64le,pcm_u8,pcm_alaw,pcm_mulaw";
/// Picture decoders (and the JPEG encoder), for a thumbnail from a file's video.
pub const FFMPEG_PICTURE: &str = "h264,hevc,vp8,libvpx,vp9,libvpx-vp9,av1,libdav1d,libaom-av1,mpeg4,msmpeg4v3,\
     msmpeg4v2,mpeg2video,mpeg1video,mjpeg,png,theora,vc1,wmv3,prores,dnxhd";

/// Limits the next input `-i` of an ffmpeg `cmd` to `FFMPEG_FORMATS`, the
/// `decoders` given, and files.
pub fn ffmpeg_allow<'a>(cmd: &'a mut Command, decoders: &str) -> &'a mut Command {
    cmd.args(["-format_whitelist", FFMPEG_FORMATS, "-codec_whitelist", decoders, "-protocol_whitelist", "file"])
}

/// Stops `child` and everything it started, and waits until they're gone, so
/// nothing writes after the caller cleans up. The managed yt-dlp unpacks
/// itself and runs as a child of a small launcher; killing only the launcher
/// left the downloader writing fragments for a second or more.
pub fn kill_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let group = child.id() as libc::pid_t;
        // SAFETY: plain syscalls on the process group `command` gave the child.
        unsafe { libc::killpg(group, libc::SIGKILL) };
        let _ = child.wait();
        // The group is gone once none of it is left to signal.
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while unsafe { libc::killpg(group, 0) } == 0 && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
    #[cfg(windows)]
    {
        // taskkill /T stops the process and its descendants, and returns once
        // they're stopped. Windows' own, not whatever is first on the PATH.
        let windows = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
        let _ = command(&Path::new(&windows).join(r"System32\taskkill.exe"))
            .args(["/T", "/F", "/PID", &child.id().to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        let _ = child.kill();
        let _ = child.wait();
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// A `Command` that doesn't flash a console window on Windows.
/// Runs in the program's own directory rather than wherever the app was launched
/// from, so nothing in an untrusted working dir (configs, DLLs) gets picked up.
/// On Unix it leads a process group of its own, so `kill_tree` stops everything
/// it starts.
pub fn command(program: &Path) -> Command {
    let mut cmd = Command::new(program);
    if let Some(dir) = program.parent().filter(|d| d.is_dir()) {
        cmd.current_dir(dir);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Runs ffmpeg's `cmd`, which writes to `part` and was given `-progress
/// pipe:1`, reporting how far it is through `length` seconds of output from 0
/// to 1; once done, `part` becomes `out` (so `out` is never half written).
/// `what` says what it was making, for its error.
pub fn encode(
    mut cmd: Command,
    part: &Path,
    out: &Path,
    length: f64,
    cancel: &AtomicBool,
    mut progress: impl FnMut(f64),
    what: &str,
) -> Result<()> {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;
    use std::sync::atomic::Ordering;
    let mut child =
        cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().context("start ffmpeg")?;
    let errors = drain(child.stderr.take().context("ffmpeg's errors")?);
    let mut canceled = false;
    for line in BufReader::new(child.stdout.take().context("ffmpeg's progress")?).lines() {
        if cancel.load(Ordering::Relaxed) {
            canceled = true;
            kill_tree(&mut child);
            break;
        }
        if let Some(us) = line?.strip_prefix("out_time_us=").and_then(|v| v.trim().parse::<f64>().ok()) {
            progress((us / 1e6 / length).clamp(0.0, 1.0));
        }
    }
    let status = child.wait()?;
    let errors = errors.join().unwrap_or_default();
    if canceled {
        let _ = fs::remove_file(part);
        return Err(crate::Canceled.into());
    }
    if !status.success() {
        let _ = fs::remove_file(part);
        bail!("ffmpeg couldn't make {what}: {}", last_lines(errors.trim(), 3));
    }
    fs::rename(part, out)?;
    progress(1.0);
    Ok(())
}

/// `path` for a message a person may paste into an issue: the home folder,
/// whose name is often theirs, written `~`.
pub fn shown(path: impl AsRef<Path>) -> String {
    let path = path.as_ref();
    let rest = std::env::home_dir().filter(|h| !h.as_os_str().is_empty()).and_then(|h| path.strip_prefix(h).ok());
    match rest {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => Path::new("~").join(rest).display().to_string(),
        None => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Like the managed yt-dlp: a launcher whose child does the work. Once
    /// stopped, nothing more is written.
    #[cfg(unix)]
    #[test]
    fn kill_tree_stops_the_child_too() {
        use std::os::unix::fs::PermissionsExt;
        let dir = crate::TempDir::new("kill-tree");
        let script = dir.join("launcher");
        // The child writes a new file every 50 ms; the launcher waits on it.
        std::fs::write(
            &script,
            "#!/bin/sh\n(i=0; while true; do i=$((i+1)); : > \"$1/f$i\"; sleep 0.05; done) &\nwait\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let out = dir.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let mut child = command(&script).arg(&out).spawn().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        kill_tree(&mut child);
        let written = std::fs::read_dir(&out).unwrap().count();
        assert!(written > 0, "the child was writing");
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert_eq!(std::fs::read_dir(&out).unwrap().count(), written, "nothing written once stopped");
    }

    #[test]
    fn lines_survive_bytes_that_arent_utf8() {
        // "C:\\Users\\Jürgen" as Windows-1252 writes it, then a normal line.
        let input: &[u8] = b"C:\\Users\\J\xfcrgen\\a.m4a\r\nnext\n";
        let (tx, rx) = std::sync::mpsc::channel();
        send_lines(input, tx).join().unwrap();
        let lines: Vec<String> = rx.iter().collect();
        assert_eq!(lines, ["C:\\Users\\J\u{fffd}rgen\\a.m4a", "next"]);
    }

    #[test]
    fn keeps_the_last_non_blank_lines() {
        assert_eq!(last_lines("a\nb\n\nc\n  \n", 2), "b\nc");
        assert_eq!(last_lines("", 3), "");
        let home = std::env::home_dir().unwrap().display().to_string();
        let error = format!("{home}/Videos/a.mp4.part: No space left on device");
        assert_eq!(last_lines(&error, 1), format!("~{}Videos/a.mp4.part: No space left on device", "/"));
    }

    /// Installs the latest release into $VODSCRIBE_TEST_BIN_DIR, then again (a no-op).
    #[test]
    #[ignore]
    fn installs_verified_yt_dlp() {
        let dir = PathBuf::from(std::env::var("VODSCRIBE_TEST_BIN_DIR").expect("set VODSCRIBE_TEST_BIN_DIR"));
        let (path, changed) = install_yt_dlp(&dir, &AtomicBool::new(false), |_| {}).unwrap();
        assert!(changed && path.is_file());
        assert!(!install_yt_dlp(&dir, &AtomicBool::new(false), |_| {}).unwrap().1);
        assert!(update_yt_dlp(&dir, |_| {}).unwrap().contains("is up to date"));
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

    #[test]
    fn paths_in_messages_hide_the_home_folder() {
        let home = std::env::home_dir().unwrap();
        assert_eq!(shown(&home), "~");
        assert_eq!(
            shown(home.join("Videos").join("a.mp4")),
            Path::new("~").join("Videos").join("a.mp4").display().to_string()
        );
        let elsewhere = if cfg!(windows) { Path::new(r"D:\clips\a.mp4") } else { Path::new("/mnt/clips/a.mp4") };
        assert_eq!(shown(elsewhere), elsewhere.display().to_string());
    }
}
