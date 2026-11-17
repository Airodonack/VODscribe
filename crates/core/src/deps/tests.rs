use super::*;

/// The program is the archive's entry ending in its path, and no other
/// (deps.rs `is_member`: "`member` exactly, or a path ending in it";
/// `Dep::member`).
#[test]
fn finds_the_program_in_its_archive() {
    assert!(is_member("ffmpeg-n9.0-linux64-gpl-9.0/bin/ffmpeg", "/bin/ffmpeg"));
    assert!(!is_member("ffmpeg-n9.0-linux64-gpl-9.0/bin/ffprobe", "/bin/ffmpeg"));
    assert!(!is_member("ffmpeg-n9.0/doc/bin/ffmpeg.html", "/bin/ffmpeg"));
    assert!(is_member("ffmpeg", "ffmpeg"));
    assert!(!is_member("sub/ffmpeg", "ffmpeg"));
    assert!(is_member("x\\bin\\ffmpeg.exe", "/bin/ffmpeg.exe"));
}

/// Setup can download ffmpeg and Deno on every platform VODscribe is
/// built for (setup.rs `SetupStatus::can_install_tools`; deps.rs `ffmpeg`,
/// `deno`).
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

// Downloading and unpacking a program, from a host of the tests' own.

use std::io::Write;

use crate::testkit::Net;

/// A program `name` the tests' own host serves as `archive` (bytes), with
/// its program at `member`; pinned to `pinned`'s size and SHA-256.
fn dep(name: &'static str, archive: Archive, member: &'static str, pinned: &[u8], net: &Net, bytes: &[u8]) -> Dep {
    use sha2::{Digest, Sha256};
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let url: &'static str = format!("https://programs.example/{n}/{name}.download").leak();
    net.public("programs.example");
    net.serves(url, bytes.to_vec());
    Dep {
        name,
        label: "FFmpeg 9.9",
        url,
        size: pinned.len() as u64,
        sha256: net::hex(&Sha256::digest(pinned)).leak(),
        archive,
        member,
    }
}

/// A zip of `entries` (path, bytes), as a release's archive.
fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (path, bytes) in entries {
        zip.start_file(*path, stored).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// A .tar.xz of `entries`, as BtbN's Linux builds are.
fn tar_xz_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut tar = tar::Builder::new(Vec::new());
    for (path, bytes) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, path, *bytes).unwrap();
    }
    let tar = tar.into_inner().unwrap();
    let mut xz = Vec::new();
    lzma_rs::xz_compress(&mut io::Cursor::new(tar), &mut xz).unwrap();
    xz
}

/// A release's archive: its program, the one beside it and its docs.
const RELEASE: &[(&str, &[u8])] = &[
    ("ffmpeg-9.9/bin/ffmpeg", b"the program"),
    ("ffmpeg-9.9/bin/ffprobe", b"another"),
    ("ffmpeg-9.9/doc/ffmpeg.html", b"<p>docs</p>"),
];

/// What's in `bin`, by name.
fn files_in(bin: &Path) -> Vec<String> {
    let mut names: Vec<String> =
        fs::read_dir(bin).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

/// A download that matches its pin leaves only its program in the app's
/// folder, ready to run: neither the archive nor the rest of it (deps.rs:
/// "only the program itself is unpacked from its archive"; `install`:
/// "Nothing is left behind").
#[test]
fn a_download_matching_its_pin_leaves_only_its_program() {
    let net = Net::new();
    for (archive, bytes) in [(Archive::Zip, zip_of(RELEASE)), (Archive::TarXz, tar_xz_of(RELEASE))] {
        let dep = dep("ffmpeg", archive, "/bin/ffmpeg", &bytes, &net, &bytes);
        let dir = crate::TempDir::new("deps-install");
        let bin = dir.join("bin");
        let path = install(&dep, &bin, &AtomicBool::new(false), |_| {}).unwrap();
        assert_eq!(path, managed_path(&bin, "ffmpeg"));
        assert_eq!(fs::read(&path).unwrap(), b"the program", "{archive:?}");
        assert_eq!(files_in(&bin), [format!("ffmpeg{EXE}")], "{archive:?}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o755, "it can run");
        }
    }
}

/// A download that isn't the file it's pinned to is refused: nothing of
/// it is kept, and the copy there before stays (deps.rs: "Each download
/// is pinned by size and SHA-256, so a replaced file is refused"; net.rs
/// `download_verified`).
#[test]
fn a_download_unlike_its_pin_is_refused_and_the_copy_before_stays() {
    let net = Net::new();
    let pinned = zip_of(RELEASE);
    let replaced = zip_of(&[("ffmpeg-9.9/bin/ffmpeg", b"something else")]);
    let dep = dep("ffmpeg", Archive::Zip, "/bin/ffmpeg", &pinned, &net, &replaced);
    let dir = crate::TempDir::new("deps-replaced");
    let bin = dir.join("bin");
    fs::create_dir_all(&bin).unwrap();
    fs::write(managed_path(&bin, "ffmpeg"), b"the copy before").unwrap();
    let error = format!("{:#}", install(&dep, &bin, &AtomicBool::new(false), |_| {}).unwrap_err());
    assert!(error.contains("A download was damaged or changed on the way, so VODscribe threw it away. Try again."));
    assert_eq!(fs::read(managed_path(&bin, "ffmpeg")).unwrap(), b"the copy before");
    assert_eq!(files_in(&bin), [format!("ffmpeg{EXE}")]);
}

/// A newer download replaces the copy before (deps.rs `install_executable`:
/// "replacing any older copy in one step").
#[test]
fn a_new_download_replaces_the_copy_before() {
    let net = Net::new();
    let bytes = zip_of(RELEASE);
    let dep = dep("ffmpeg", Archive::Zip, "/bin/ffmpeg", &bytes, &net, &bytes);
    let dir = crate::TempDir::new("deps-replace");
    let bin = dir.join("bin");
    fs::create_dir_all(&bin).unwrap();
    fs::write(managed_path(&bin, "ffmpeg"), b"the copy before").unwrap();
    install(&dep, &bin, &AtomicBool::new(false), |_| {}).unwrap();
    assert_eq!(fs::read(managed_path(&bin, "ffmpeg")).unwrap(), b"the program");
    assert_eq!(files_in(&bin), [format!("ffmpeg{EXE}")]);
}

/// An archive without its program where it should be says so, leaving
/// nothing (deps.rs `unpack`: "wasn't where expected in its download").
#[test]
fn an_archive_without_its_program_says_so() {
    let net = Net::new();
    for (archive, bytes) in [
        (Archive::Zip, zip_of(&[("ffmpeg-9.9/doc/bin/ffmpeg.html", b"<p>docs</p>")])),
        (Archive::TarXz, tar_xz_of(&[("ffmpeg-9.9/bin/ffprobe", b"another")])),
    ] {
        let dep = dep("ffmpeg", archive, "/bin/ffmpeg", &bytes, &net, &bytes);
        let dir = crate::TempDir::new("deps-missing");
        let bin = dir.join("bin");
        let error = install(&dep, &bin, &AtomicBool::new(false), |_| {}).unwrap_err().to_string();
        assert_eq!(error, "ffmpeg wasn't where expected in its download");
        assert_eq!(files_in(&bin), Vec::<String>::new(), "{archive:?}");
    }
}

/// Its progress counts the download's bytes against its exact size, then
/// says it matches as it unpacks (CHANGELOG.md, 0.1.2: "Download sizes are
/// exact."; deps.rs `install`; lib.rs `Progress::step`: "\"Downloading
/// <name>\", then \"<name> matches its SHA-256\"").
#[test]
fn its_progress_counts_its_exact_size_then_says_it_matches() {
    let net = Net::new();
    let bytes = zip_of(RELEASE);
    let dep = dep("ffmpeg", Archive::Zip, "/bin/ffmpeg", &bytes, &net, &bytes);
    let dir = crate::TempDir::new("deps-progress");
    let mut seen = Vec::new();
    install(&dep, &dir.join("bin"), &AtomicBool::new(false), |p| {
        seen.push((p.step.to_string(), p.done, p.total, p.file_size, p.part, p.parts))
    })
    .unwrap();
    let size = bytes.len() as u64;
    let (last, downloading) = seen.split_last().unwrap();
    assert_eq!(*last, ("FFmpeg 9.9 matches its SHA‑256 · unpacking".to_string(), size, Some(size), Some(size), 1, 1));
    assert!(!downloading.is_empty());
    for (step, done, total, file_size, part, parts) in downloading {
        assert_eq!(
            (step.as_str(), *total, *file_size, *part, *parts),
            ("Downloading FFmpeg 9.9", Some(size), Some(size), 1, 1)
        );
        assert!(*done <= size);
    }
    assert_eq!(downloading.last().unwrap().1, size, "all of it came");
}

/// A canceled install leaves nothing, whether canceled as it downloads or
/// as it unpacks, and ends as canceled, not failed (CHANGELOG.md, 0.1.3:
/// "Setup and the GPU add-on downloads have a Cancel button."; deps.rs
/// `install`: "A cancel during the unpack surfaces as a short read; report
/// it as the cancel it was.").
#[test]
fn a_canceled_install_leaves_nothing() {
    let net = Net::new();
    let bytes = tar_xz_of(RELEASE);
    let dep = dep("ffmpeg", Archive::TarXz, "/bin/ffmpeg", &bytes, &net, &bytes);
    for when in ["Downloading", "unpacking"] {
        let dir = crate::TempDir::new("deps-cancel");
        let bin = dir.join("bin");
        let cancel = AtomicBool::new(false);
        let result = install(&dep, &bin, &cancel, |p| {
            if p.step.contains(when) {
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        });
        let error = result.expect_err(when);
        assert!(crate::is_canceled(&error), "{when}: {error:#}");
        assert_eq!(files_in(&bin), Vec::<String>::new(), "{when}");
    }
}
