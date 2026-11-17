use super::*;

/// Whether the launcher's child writes into `dir` within a few seconds.
/// Not a fixed wait: macOS checks a program the first time it runs, and
/// on the Mac runner a script just written took about 750 ms to start.
#[cfg(unix)]
fn writes(dir: &Path) -> bool {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::fs::read_dir(dir).unwrap().count() == 0 {
        if std::time::Instant::now() > until {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    true
}

/// What quitting stops: a tracked launcher and its child, and nothing
/// that isn't tracked (CHANGELOG.md, 0.4.0 Fixes: "Quitting left downloads
/// and transcription running."; tools.rs `stop_tracked`).
#[cfg(unix)]
#[test]
fn stop_tracked_stops_what_quitting_would_leave_running() {
    use std::os::unix::fs::PermissionsExt;
    let dir = crate::TempDir::new("stop-tracked");
    let script = dir.join("launcher");
    std::fs::write(&script, "#!/bin/sh\n(i=0; while true; do i=$((i+1)); : > \"$1/f$i\"; sleep 0.05; done) &\nwait\n")
        .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let (tracked_out, other_out) = (dir.join("tracked"), dir.join("other"));
    std::fs::create_dir_all(&tracked_out).unwrap();
    std::fs::create_dir_all(&other_out).unwrap();
    let mut child = command(&script).arg(&tracked_out).spawn().unwrap();
    let guard = track(&child);
    let mut other = command(&script).arg(&other_out).spawn().unwrap();
    assert!(writes(&tracked_out) && writes(&other_out), "they were writing");
    let id = child.id();
    stop_in(&TRACKED, |tracked| tracked == id, false);
    let _ = child.wait();
    let written = std::fs::read_dir(&tracked_out).unwrap().count();
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert_eq!(std::fs::read_dir(&tracked_out).unwrap().count(), written, "it and its child stopped");
    let before = std::fs::read_dir(&other_out).unwrap().count();
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(std::fs::read_dir(&other_out).unwrap().count() > before, "what isn't tracked runs on");
    kill_tree(&mut other);
    drop(guard);
}

/// A process started as the app quits: spawned before `stop_tracked`
/// ran and tracked after it (or started by a job after it). It stops
/// too, rather than running on after the app.
#[cfg(unix)]
#[test]
fn what_is_tracked_once_quitting_began_stops_too() {
    static REGISTRY: std::sync::Mutex<Registry> = std::sync::Mutex::new(Registry::new());
    let mut child = command(Path::new("/bin/sh")).args(["-c", "sleep 30"]).spawn().unwrap();
    stop_in(&REGISTRY, |_| true, true);
    let guard = track_in(&REGISTRY, &child);
    let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let stopped = loop {
        match child.try_wait().unwrap() {
            Some(_) => break true,
            None if std::time::Instant::now() > until => break false,
            None => std::thread::sleep(std::time::Duration::from_millis(20)),
        }
    };
    if !stopped {
        kill_tree(&mut child);
    }
    drop(guard);
    assert!(stopped, "it ran on after quitting began");
}

/// A process its owner doesn't wait for as the app quits (the player's
/// decoder, held by a player that's never dropped): once stopped it's a
/// zombie till then, which mustn't hold the quit up.
#[cfg(unix)]
#[test]
fn quitting_doesnt_wait_for_a_stopped_process_no_one_waits_for() {
    static REGISTRY: std::sync::Mutex<Registry> = std::sync::Mutex::new(Registry::new());
    let mut child = command(Path::new("/bin/sh")).args(["-c", "sleep 30"]).spawn().unwrap();
    let guard = track_in(&REGISTRY, &child);
    let began = std::time::Instant::now();
    stop_in(&REGISTRY, |_| true, true);
    let took = began.elapsed();
    let _ = child.wait();
    drop(guard);
    assert!(took < std::time::Duration::from_millis(500), "the quit waited {took:?} for it");
}

/// Like the managed yt-dlp: a launcher whose child does the work. Once
/// stopped, nothing more is written (tools.rs `kill_tree`: "Stops `child`
/// and everything it started").
#[cfg(unix)]
#[test]
fn kill_tree_stops_the_child_too() {
    use std::os::unix::fs::PermissionsExt;
    let dir = crate::TempDir::new("kill-tree");
    let script = dir.join("launcher");
    // The child writes a new file every 50 ms; the launcher waits on it.
    std::fs::write(&script, "#!/bin/sh\n(i=0; while true; do i=$((i+1)); : > \"$1/f$i\"; sleep 0.05; done) &\nwait\n")
        .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let out = dir.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let mut child = command(&script).arg(&out).spawn().unwrap();
    assert!(writes(&out), "the child was writing");
    kill_tree(&mut child);
    let written = std::fs::read_dir(&out).unwrap().count();
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert_eq!(std::fs::read_dir(&out).unwrap().count(), written, "nothing written once stopped");
}

/// tools.rs `send_lines`: "Bytes that aren't UTF-8 (a program writing in
/// Windows' code page) are replaced rather than ending the stream."
#[test]
fn lines_survive_bytes_that_arent_utf8() {
    // "C:\\Users\\Jürgen" as Windows-1252 writes it, then a normal line.
    let input: &[u8] = b"C:\\Users\\J\xfcrgen\\a.m4a\r\nnext\n";
    let (tx, rx) = std::sync::mpsc::channel();
    send_lines(input, tx).join().unwrap();
    let lines: Vec<String> = rx.iter().collect();
    assert_eq!(lines, ["C:\\Users\\J\u{fffd}rgen\\a.m4a", "next"]);
}

/// A tool's error lines, as a message gives them: the last that aren't
/// blank, the home folder written `~` (tools.rs `last_lines`; CHANGELOG.md,
/// 0.4.0 Fixes: "Error messages showed your home folder's full path.").
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

/// The release published 2026.08.19: its checksum list and signature. A
/// list changed after it was signed is refused, as is one with no entry
/// for this computer's yt-dlp (tools.rs `install_yt_dlp`: "verified
/// against the release's checksum list, which must carry the yt-dlp
/// signing key's signature").
#[test]
fn verifies_signed_checksums() {
    let sums = include_bytes!("../../tests/data/yt-dlp-SHA2-256SUMS");
    let sig = include_bytes!("../../tests/data/yt-dlp-SHA2-256SUMS.sig");
    let hash = signed_sha256(sums, sig, "yt-dlp_linux").unwrap();
    assert_eq!(hash, "58162f9bfdc27458ea47bfcb311cf47028f17d8154a8bf7d689861d46399230a");
    let mut tampered = sums.to_vec();
    tampered[0] ^= 1;
    assert!(signed_sha256(&tampered, sig, "yt-dlp_linux").is_err());
    assert!(signed_sha256(sums, sig, "yt-dlp_nonexistent").is_err());
}

/// CHANGELOG.md, 0.4.0 Fixes: "Error messages showed your home folder's
/// full path." (tools.rs `shown`).
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

/// tools.rs `encoder_names`: "after its line of dashes, an encoder a line,
/// its flags and then its name. None without that line."
#[test]
fn reads_ffmpegs_list_of_encoders() {
    let listed = "Encoders:\n V..... = Video\n A..... = Audio\n ------\n \
        V....D libx264              libx264 H.264 / AVC (codec h264)\n \
        VFS... prores_ks            Apple ProRes (iCodec Pro) (codec prores)\n \
        A....D aac                  AAC (Advanced Audio Coding)\n";
    let names: Vec<_> = encoder_names(listed).unwrap().into_iter().collect();
    assert_eq!(names, ["aac", "libx264", "prores_ks"]);
    assert_eq!(encoder_names("Unrecognized option 'encoders'.\n"), None, "not a list");
}

/// An ffmpeg that doesn't list its encoders in time is stopped and taken
/// as unable to say, holding up no other ffmpeg meanwhile; what it said
/// (nothing) isn't kept, so it's asked again.
#[cfg(unix)]
#[test]
fn an_ffmpeg_slow_to_list_its_encoders_holds_up_no_other_and_is_asked_again() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};
    let dir = std::env::temp_dir().join(format!("vodscribe-encoders-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let script = |name: &str, body: &str| {
        let path = dir.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    };
    let list = "echo ' ------'; echo ' V....D libx264  libx264 H.264'";
    let (begun, quick) = (dir.join("begun"), dir.join("quick"));
    let slow = script(
        "ffmpeg-slow",
        &format!("if [ -f '{}' ]; then {list}; exit 0; fi\ntouch '{}'\nsleep 30", quick.display(), begun.display()),
    );
    let other = script("ffmpeg-other", list);
    let asked = std::thread::spawn({
        let slow = slow.clone();
        move || (encoders_within(&slow, Duration::from_secs(2)), Instant::now())
    });
    let waited = Instant::now();
    while !begun.exists() {
        assert!(waited.elapsed() < Duration::from_secs(5), "the slow one never began");
        std::thread::sleep(Duration::from_millis(10));
    }
    let other_asked = Instant::now();
    assert!(encoders_within(&other, Duration::from_secs(2)).is_some_and(|l| l.contains("libx264")));
    assert!(other_asked.elapsed() < Duration::from_secs(1), "waited {:?} on the slow one", other_asked.elapsed());
    let (answer, ended) = asked.join().unwrap();
    assert_eq!(answer, None, "stopped, it can't say");
    assert!(ended - waited < Duration::from_secs(5), "not stopped at its time: {:?}", ended - waited);
    fs::write(&quick, "").unwrap();
    assert!(encoders_within(&slow, Duration::from_secs(2)).is_some(), "asked again, the same file");
    let _ = fs::remove_dir_all(&dir);
}

/// An ffmpeg that fails as it lists its encoders can't say, whatever it
/// wrote first, and is asked again.
#[cfg(unix)]
#[test]
fn an_ffmpeg_that_fails_listing_its_encoders_cant_say_and_is_asked_again() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;
    let dir = std::env::temp_dir().join(format!("vodscribe-encoders-failed-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let (ran, path) = (dir.join("ran"), dir.join("ffmpeg"));
    let body = format!("echo x >> '{}'\necho ' ------'; echo ' V....D libx264  libx264 H.264'\nexit 1", ran.display());
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(encoders_within(&path, Duration::from_secs(5)), None);
    assert_eq!(encoders_within(&path, Duration::from_secs(5)), None);
    assert_eq!(fs::read_to_string(&ran).unwrap().lines().count(), 2, "asked again");
    let _ = fs::remove_dir_all(&dir);
}

// Finding the programs, and installing yt-dlp.

use crate::testkit::Net;
use crate::testkit::tools::path_for;

/// A program `name` in `dir` that a PATH lookup takes: an executable file.
fn program(dir: &Path, name: &str) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let path = dir.join(format!("{name}{EXE}"));
    fs::write(&path, b"").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    path
}

/// A computer in `dir`: the app's own programs in `bin`, and the system's
/// on a PATH of `system` alone.
fn computer(dir: &Path) -> (PathBuf, PathBuf) {
    let (bin, system) = (dir.join("bin"), dir.join("system"));
    fs::create_dir_all(&bin).unwrap();
    path_for(&bin, std::slice::from_ref(&system));
    (bin, system)
}

/// The yt-dlp VODscribe downloaded is used over one the system has, and
/// the system's when it has none of its own (tools.rs `find_yt_dlp`:
/// "Prefers the managed yt-dlp; a system install is often too stale for
/// YouTube.").
#[test]
fn its_own_yt_dlp_is_used_before_the_systems() {
    let dir = crate::TempDir::new("find-yt-dlp");
    let (bin, system) = computer(&dir);
    assert!(find_yt_dlp(&bin).is_none(), "neither");
    let systems = program(&system, "yt-dlp");
    assert_eq!(find_yt_dlp(&bin).unwrap().path, systems);
    let own = program(&bin, "yt-dlp");
    assert_eq!(own, managed_yt_dlp(&bin));
    assert_eq!(find_yt_dlp(&bin).unwrap().path, own);
}

/// VODscribe's own ffmpeg, else the system's, and which it is (tools.rs
/// `find_ffmpeg`: "VODscribe's own ffmpeg in `bin_dir`, else the system's
/// on PATH"; `own_ffmpeg`).
#[test]
fn its_own_ffmpeg_is_used_before_the_systems_and_said_to_be_its_own() {
    let dir = crate::TempDir::new("find-ffmpeg");
    let (bin, system) = computer(&dir);
    assert_eq!(find_ffmpeg(&bin), None);
    let systems = program(&system, "ffmpeg");
    assert_eq!(find_ffmpeg(&bin), Some(systems.clone()));
    assert!(!own_ffmpeg(&bin, &systems));
    let own = program(&bin, "ffmpeg");
    assert_eq!(find_ffmpeg(&bin), Some(own.clone()));
    assert!(own_ffmpeg(&bin, &own));
}

/// yt-dlp is given a JavaScript runtime for YouTube: Deno, else Node, else
/// Bun, VODscribe's own before the system's (tools.rs `find_js_runtime`:
/// "yt-dlp solves YouTube's JavaScript challenges with an external
/// runtime"; `YtDlp`, "plus the JavaScript runtime it needs for YouTube").
#[test]
fn yt_dlp_gets_deno_then_node_then_bun_its_own_first() {
    let dir = crate::TempDir::new("find-js");
    let (bin, system) = computer(&dir);
    program(&bin, "yt-dlp");
    assert_eq!(find_js_runtime(&bin), None);
    let bun = program(&system, "bun");
    assert_eq!(find_js_runtime(&bin), Some(("bun", bun)));
    let node = program(&system, "node");
    assert_eq!(find_js_runtime(&bin), Some(("node", node)));
    let deno = program(&system, "deno");
    assert_eq!(find_js_runtime(&bin), Some(("deno", deno)));
    let own = program(&bin, "deno");
    assert_eq!(find_yt_dlp(&bin).unwrap().js_runtime, Some(("deno", own)));
}

/// yt-dlp reads no configuration or plugins from this computer and only
/// sites it knows by name, and is told the JavaScript runtime it has
/// (tools.rs `YtDlp::command`: "Only sites yt-dlp knows by name";
/// `command_any_extractor`: "A yt-dlp.conf … could add --exec, browser
/// cookies or output paths outside the work dir", "Nor plugins"). The
/// generic extractor is only for a link the app got from a site's own API.
#[test]
fn yt_dlp_reads_no_config_or_plugins_and_only_sites_it_knows() {
    let yt_dlp = YtDlp { path: PathBuf::from("/opt/yt-dlp"), js_runtime: Some(("deno", PathBuf::from("/opt/deno"))) };
    let args = |cmd: Command| cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>();
    let pasted = args(yt_dlp.command());
    let has = |args: &[String], pair: &[&str]| args.windows(pair.len()).any(|w| w == pair);
    for given in [&pasted, &args(yt_dlp.command_any_extractor())] {
        assert!(given.contains(&"--ignore-config".to_string()) && given.contains(&"--no-plugin-dirs".to_string()));
        assert!(has(given, &["--js-runtimes", &format!("deno:{}", Path::new("/opt/deno").display())]));
    }
    assert!(has(&pasted, &["--use-extractors", "default,-generic"]), "{pasted:?}");
    assert!(!args(yt_dlp.command_any_extractor()).contains(&"--use-extractors".to_string()));
}

/// A helper program missing beside the app says so (tools.rs `sidecar`).
#[test]
fn a_missing_helper_program_says_it_is_missing() {
    let error = sidecar("vodscribe-no-such-helper").unwrap_err().to_string();
    assert!(error.ends_with("is missing"), "{error}");
    assert!(error.contains("vodscribe-no-such-helper"), "{error}");
}

/// ffmpeg reads files, and the app's own media server over HTTP, and
/// nothing else (tools.rs `ffmpeg_allow_for`: "for an `input` that may be
/// the app's own media server rather than a file (`served`): then HTTP
/// over TCP instead of files"; `served`).
#[test]
fn ffmpeg_reads_files_and_only_the_apps_own_server_over_http() {
    let allowed = |input: &str| {
        let mut cmd = Command::new("ffmpeg");
        ffmpeg_allow_for(&mut cmd, FFMPEG_AUDIO, Path::new(input));
        let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        let at = args.iter().position(|a| a == "-protocol_whitelist").unwrap();
        args[at + 1].clone()
    };
    assert_eq!(allowed("/home/ada/Videos/Day 3.mp4"), "file");
    assert_eq!(allowed("http://127.0.0.1:41000/media/3"), "http,tcp");
    assert_eq!(allowed("http://example.com/media/3"), "file", "another server is no file it may read");
}

/// The release yt-dlp's own site serves: its checksum list, signature and
/// this computer's build. The tests that serve it take turns, since its
/// links are the same for each; the fake's for as long as the `Net` is kept.
struct Release {
    net: Net,
    _turn: std::sync::MutexGuard<'static, ()>,
}

impl Release {
    fn new() -> Self {
        static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
        let net = Net::new();
        net.public("github.com");
        Release { net, _turn: turn }
    }

    /// The 2026.08.19 release's list and signature, `sums` changed by
    /// `change` first; and `build` as this computer's yt-dlp.
    fn serves(&self, change: impl FnOnce(&mut Vec<u8>), build: &[u8]) {
        let mut sums = include_bytes!("../../tests/data/yt-dlp-SHA2-256SUMS").to_vec();
        change(&mut sums);
        self.net.serves(&format!("{YT_DLP_RELEASE}/SHA2-256SUMS"), sums);
        self.net.serves(
            &format!("{YT_DLP_RELEASE}/SHA2-256SUMS.sig"),
            &include_bytes!("../../tests/data/yt-dlp-SHA2-256SUMS.sig")[..],
        );
        self.net.serves(&format!("{YT_DLP_RELEASE}/{}", yt_dlp_asset().unwrap()), build);
    }
}

/// What's in `bin`, by name.
fn files_in(bin: &Path) -> Vec<String> {
    let mut names: Vec<String> =
        fs::read_dir(bin).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

/// A checksum list changed after it was signed installs nothing: the
/// yt-dlp there before stays, and it says why (tools.rs `install_yt_dlp`;
/// `signed_sha256`: "The video downloader couldn't be checked, so it
/// wasn't installed. Try again later.").
#[test]
fn a_checksum_list_not_as_signed_installs_nothing() {
    let release = Release::new();
    release.serves(|sums| sums[0] ^= 1, b"a yt-dlp");
    let dir = crate::TempDir::new("yt-dlp-unsigned");
    let bin = dir.join("bin");
    fs::create_dir_all(&bin).unwrap();
    fs::write(managed_yt_dlp(&bin), b"the yt-dlp before").unwrap();
    let error = format!("{:#}", install_yt_dlp(&bin, &AtomicBool::new(false), |_| {}).unwrap_err());
    assert!(
        error.contains("The video downloader couldn't be checked, so it wasn't installed. Try again later."),
        "{error}"
    );
    assert_eq!(fs::read(managed_yt_dlp(&bin)).unwrap(), b"the yt-dlp before");
    assert_eq!(files_in(&bin), [format!("yt-dlp{EXE}")]);
}

/// A yt-dlp whose bytes aren't those its signed list gives is thrown away,
/// and the one before stays (tools.rs `install_yt_dlp`: "verified against
/// the release's checksum list"; net.rs `download_verified`: "Nothing
/// appears at `dest` unless it matches").
#[test]
fn a_yt_dlp_unlike_its_signed_checksum_is_thrown_away() {
    let release = Release::new();
    release.serves(|_| {}, b"not the yt-dlp that was signed");
    let dir = crate::TempDir::new("yt-dlp-changed");
    let bin = dir.join("bin");
    fs::create_dir_all(&bin).unwrap();
    fs::write(managed_yt_dlp(&bin), b"the yt-dlp before").unwrap();
    let error = format!("{:#}", install_yt_dlp(&bin, &AtomicBool::new(false), |_| {}).unwrap_err());
    assert!(error.contains("A download was damaged or changed on the way, so VODscribe threw it away."), "{error}");
    assert_eq!(fs::read(managed_yt_dlp(&bin)).unwrap(), b"the yt-dlp before");
    assert_eq!(files_in(&bin), [format!("yt-dlp{EXE}")], "nothing half downloaded is left");
}
