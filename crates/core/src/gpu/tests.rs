use super::*;

/// nvidia-smi's line, read as the card's name and memory (gpu.rs `detect`,
/// `parse_smi`).
#[test]
fn parses_nvidia_smi() {
    let info = parse_smi("NVIDIA GeForce RTX 3080, 10240, 4039\n").unwrap();
    assert_eq!(info.name, "NVIDIA GeForce RTX 3080");
    assert_eq!((info.total_mb, info.free_mb), (10240, 4039));
    assert!(parse_smi("").is_none());
}

/// gpu.rs `batch_for_free_vram`: "Chunks per GPU decode for the free VRAM,
/// or None if the GPU can't take the job", from `MIN_FREE_MB`.
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
    let mut last = 0;
    install(&dir, &AtomicBool::new(false), |p| {
        let total = p.total.unwrap();
        assert!(p.done >= last && p.done <= total, "progress went from {last} to {} of {total}", p.done);
        assert!(p.file_done <= p.file_size.unwrap());
        last = p.done;
    })
    .unwrap();
    assert_eq!(last, download_size());
    assert!(is_installed(&dir));
    let names: Vec<String> =
        fs::read_dir(lib_dir(&dir)).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    for want in ["onnxruntime", "cudnn", "cublas", "cudart", "curand"] {
        assert!(names.iter().any(|n| n.contains(want)), "no {want} library in {names:?}");
    }
    assert!(!names.iter().any(|n| n.contains("sherpa")), "sherpa-onnx's own library in {names:?}");
}

/// Only ONNX Runtime is taken from the sherpa-onnx archive, not
/// sherpa-onnx's own library (gpu.rs: "it's left out and the app's no-tts
/// build is used instead"; `is_onnxruntime`).
#[test]
fn tells_onnxruntime_libraries_by_name() {
    assert!(is_onnxruntime("libonnxruntime.so"));
    assert!(is_onnxruntime("libonnxruntime_providers_cuda.so"));
    assert!(is_onnxruntime("onnxruntime_providers_shared.dll"));
    assert!(!is_onnxruntime("libsherpa-onnx-c-api.so"));
    assert!(!is_onnxruntime("sherpa-onnx-cxx-api.dll"));
}

/// The pack keeps shared libraries and nothing of TensorRT (gpu.rs
/// `keep_lib`; CHANGELOG.md, 0.1.3: "it now contains only ONNX Runtime and
/// NVIDIA's libraries").
#[test]
fn keeps_only_runtime_libraries() {
    let so = [".so"];
    assert_eq!(keep_lib(Path::new("x/lib/libcudnn.so.9"), &so), Some("libcudnn.so.9"));
    assert_eq!(keep_lib(Path::new("x/lib/libonnxruntime.so"), &so), Some("libonnxruntime.so"));
    assert_eq!(keep_lib(Path::new("x/lib/libonnxruntime_providers_tensorrt.so"), &so), None);
    assert_eq!(keep_lib(Path::new("x/include/cudnn.h"), &so), None);
    assert_eq!(keep_lib(Path::new("x/bin/cudnn64_9.dll"), &[".dll"]), Some("cudnn64_9.dll"));
}

// The card, and installing the pack from the tests' own hosts.

use crate::testkit::tools::path_for;
use crate::testkit::{Gate, Net, NvidiaSmi};

/// The graphics card is as the driver's nvidia-smi tells it, and with no
/// card, or no driver, there's none (gpu.rs `detect`: "Queries the first
/// NVIDIA GPU. `nvidia-smi` ships with the driver … and is found on PATH").
#[test]
fn the_card_is_the_one_the_driver_tells_of() {
    let dir = crate::TempDir::new("gpu-detect");
    let (bin, driver) = (dir.join("bin"), dir.join("driver"));
    path_for(&bin, std::slice::from_ref(&driver));
    assert!(detect(&bin).is_none(), "no driver");
    let smi = NvidiaSmi::in_dir(&driver);
    assert!(detect(&bin).is_none(), "a driver that finds no card");
    smi.shows("NVIDIA GeForce RTX 4070", 12282, 11000);
    let card = detect(&bin).unwrap();
    assert_eq!((card.name.as_str(), card.total_mb, card.free_mb), ("NVIDIA GeForce RTX 4070", 12282, 11000));
}

/// A pack another version installed isn't this version's, so it's
/// downloaded again (CHANGELOG.md, 0.1.3: "It downloads again once after
/// updating."; gpu.rs `PACK_VERSION`: "Bump when the pack's contents
/// change, so existing installs are refreshed").
#[test]
fn a_pack_of_another_version_is_not_installed() {
    let dir = crate::TempDir::new("gpu-version");
    let pack = dir.join("gpu");
    assert!(!is_installed(&pack));
    fs::create_dir_all(&pack).unwrap();
    fs::write(marker(&pack), "1").unwrap();
    assert!(!is_installed(&pack), "the pack before 0.1.3");
    fs::write(marker(&pack), PACK_VERSION).unwrap();
    assert!(is_installed(&pack));
}

/// A GPU pack exists for Linux and Windows on x86-64 (gpu.rs `supported`,
/// `platform`).
#[test]
fn a_pack_exists_for_linux_and_windows_on_x86_64() {
    let has = cfg!(all(any(target_os = "linux", target_os = "windows"), target_arch = "x86_64"));
    assert_eq!(supported(), has);
    assert_eq!(download_parts(), if has { 5 } else { 0 });
}

/// What the add-on says it downloads is the size of the files it's pinned
/// to, every one of them (CHANGELOG.md, 0.1.2: "Download sizes are exact.";
/// gpu.rs `download_size`).
#[test]
fn the_pack_downloads_the_size_of_its_pinned_files() {
    let Some(platform) = platform() else { return assert_eq!(download_size(), 0) };
    let pinned = platform.sherpa.size + platform.wheels.iter().map(|(_, w)| w.size).sum::<u64>();
    assert_eq!(download_size(), pinned);
    assert!(pinned > 1_000_000_000, "the pack is over a gigabyte: {pinned}");
}

/// The pack's hosts, the fake's for as long as they're kept: what nothing
/// answers is refused (404). The tests that use them take turns, since
/// the pack's links are the same for each.
struct PackHosts {
    net: Net,
    _turn: std::sync::MutexGuard<'static, ()>,
}

impl std::ops::Deref for PackHosts {
    type Target = Net;
    fn deref(&self) -> &Net {
        &self.net
    }
}

fn pack_hosts() -> PackHosts {
    static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let net = Net::new();
    net.public("github.com").public("files.pythonhosted.org");
    PackHosts { net, _turn: turn }
}

/// A pack being installed, its downloads refused or stopped: nothing it
/// assembled is left, and the pack there before is as it was (gpu.rs
/// `staging_dir`: "Where `install` assembles the pack before moving it
/// into place"; `install`).
#[test]
fn a_failed_install_leaves_the_pack_as_it_was() {
    if !supported() {
        return;
    }
    let _net = pack_hosts();
    let dir = crate::TempDir::new("gpu-failed");
    let pack = dir.join("gpu");
    fs::create_dir_all(lib_dir(&pack)).unwrap();
    fs::write(lib_dir(&pack).join("libcudart.so.12"), b"the library before").unwrap();
    fs::write(marker(&pack), "1").unwrap();
    let error = install(&pack, &AtomicBool::new(false), |_| {}).unwrap_err();
    assert!(!crate::is_canceled(&error), "{error:#}");
    assert!(!staging_dir(&pack).exists(), "what it assembled is left");
    assert_eq!(fs::read(lib_dir(&pack).join("libcudart.so.12")).unwrap(), b"the library before");
    assert_eq!(fs::read_to_string(marker(&pack)).unwrap(), "1");
}

/// A pack's download that isn't the file it's pinned to is refused, as
/// the other downloads are (gpu.rs `Pinned`: "a replaced release asset or
/// wheel is refused rather than loaded into the GPU worker").
#[test]
fn a_pack_download_unlike_its_pin_is_refused() {
    let Some(platform) = platform() else { return };
    let net = pack_hosts();
    net.serves(platform.sherpa.url, b"not ONNX Runtime".to_vec());
    let dir = crate::TempDir::new("gpu-replaced");
    let pack = dir.join("gpu");
    let error = format!("{:#}", install(&pack, &AtomicBool::new(false), |_| {}).unwrap_err());
    assert!(error.contains("A download was damaged or changed on the way, so VODscribe threw it away."), "{error}");
    assert!(!pack.exists() && !staging_dir(&pack).exists());
}

/// Its progress counts across every file of the pack, from the first
/// (CHANGELOG.md, 0.1.2: "Downloads with several files show which one is
/// under way, like \"(3 of 5)\"."; gpu.rs `install`: "Progress counts
/// across every file, so the bar never moves backwards.").
#[test]
fn its_progress_counts_across_the_packs_files() {
    let Some(platform) = platform() else { return };
    let net = pack_hosts();
    net.serves(platform.sherpa.url, b"x".to_vec());
    let dir = crate::TempDir::new("gpu-progress");
    let mut first = None;
    let _ = install(&dir.join("gpu"), &AtomicBool::new(false), |p| {
        first.get_or_insert((p.step.to_string(), p.done, p.total, p.file_size, p.part, p.parts));
    });
    let downloading = "Downloading ONNX Runtime 1.28 with CUDA (sherpa-onnx 1.13.8)".to_string();
    assert_eq!(first, Some((downloading, 1, Some(download_size()), Some(platform.sherpa.size), 1, 5)));
}

/// A canceled install ends as canceled, not failed, and its pack starts
/// over: nothing of it is kept (CHANGELOG.md, 0.1.3: "Tools and model files
/// that finished stay; the GPU add-on's own pack starts over.").
#[test]
fn a_canceled_install_keeps_nothing_of_the_pack() {
    let Some(platform) = platform() else { return };
    let net = pack_hosts();
    let asked = Gate::new();
    net.holds(platform.sherpa.url, &asked);
    let dir = crate::TempDir::new("gpu-cancel");
    let pack = dir.join("gpu");
    let cancel = AtomicBool::new(false);
    let error = std::thread::scope(|s| {
        let installing = s.spawn(|| install(&pack, &cancel, |_| {}));
        asked.reached();
        assert!(staging_dir(&pack).exists(), "it was assembling the pack");
        cancel.store(true, Ordering::Relaxed);
        asked.open();
        installing.join().unwrap().unwrap_err()
    });
    assert!(crate::is_canceled(&error), "{error:#}");
    assert!(!pack.exists() && !staging_dir(&pack).exists());
}

/// Removing the pack takes all of it, and removing none is no error
/// (gpu.rs `uninstall`).
#[test]
fn removing_the_pack_takes_all_of_it() {
    let dir = crate::TempDir::new("gpu-remove");
    let pack = dir.join("gpu");
    uninstall(&pack).unwrap();
    fs::create_dir_all(lib_dir(&pack)).unwrap();
    fs::write(lib_dir(&pack).join("libcudart.so.12"), b"a library").unwrap();
    fs::write(marker(&pack), PACK_VERSION).unwrap();
    uninstall(&pack).unwrap();
    assert!(!pack.exists() && !is_installed(&pack));
}

/// What's in `dir`, by name, and whether each is a link.
fn unpacked(dir: &Path) -> Vec<(String, bool)> {
    let mut names: Vec<(String, bool)> = fs::read_dir(dir)
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            (e.file_name().to_string_lossy().into_owned(), e.file_type().unwrap().is_symlink())
        })
        .collect();
    names.sort();
    names
}

/// From the sherpa-onnx archive only ONNX Runtime's libraries are taken:
/// its files, and its links to a library beside them; not sherpa-onnx's own
/// library, headers, hard links, or links that point elsewhere (gpu.rs
/// `extract_tar_libs`: "Hard links, devices and links pointing elsewhere are
/// skipped, as is sherpa-onnx's own library").
#[test]
fn only_onnx_runtimes_libraries_come_from_the_sherpa_archive() {
    let mut tar = tar::Builder::new(Vec::new());
    let file = |tar: &mut tar::Builder<Vec<u8>>, path: &str, bytes: &[u8]| {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, path, bytes).unwrap();
    };
    file(&mut tar, "sherpa/lib/libonnxruntime.so.1.28.2", b"onnx runtime");
    file(&mut tar, "sherpa/lib/libsherpa-onnx-c-api.so", b"sherpa-onnx's own");
    file(&mut tar, "sherpa/include/onnxruntime_c_api.h", b"a header");
    let link = |tar: &mut tar::Builder<Vec<u8>>, kind: tar::EntryType, path: &str, to: &str| {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(kind);
        header.set_size(0);
        tar.append_link(&mut header, path, to).unwrap();
    };
    link(&mut tar, tar::EntryType::Symlink, "sherpa/lib/libonnxruntime.so", "libonnxruntime.so.1.28.2");
    link(&mut tar, tar::EntryType::Symlink, "sherpa/lib/libonnxruntime_elsewhere.so", "/etc/passwd");
    link(&mut tar, tar::EntryType::Symlink, "sherpa/lib/libonnxruntime_up.so", "../other/libx.so");
    link(&mut tar, tar::EntryType::Link, "sherpa/lib/libonnxruntime_hard.so", "sherpa/lib/libonnxruntime.so.1.28.2");
    let mut bz = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
    io::Write::write_all(&mut bz, &tar.into_inner().unwrap()).unwrap();
    let dir = crate::TempDir::new("gpu-tar");
    let archive = dir.join("sherpa-onnx.tar.bz2");
    fs::write(&archive, bz.finish().unwrap()).unwrap();
    let libs = dir.join("lib");
    fs::create_dir_all(&libs).unwrap();
    extract_tar_libs(&archive, &libs, &[".so"]).unwrap();
    let mut wanted = vec![("libonnxruntime.so.1.28.2".to_string(), false)];
    if cfg!(unix) {
        wanted.insert(0, ("libonnxruntime.so".to_string(), true));
    }
    assert_eq!(unpacked(&libs), wanted);
    assert_eq!(fs::read(libs.join("libonnxruntime.so.1.28.2")).unwrap(), b"onnx runtime");
}

/// From NVIDIA's wheels only the libraries under their `nvidia` folder are
/// taken, never one whose path leaves it, nor a link (gpu.rs `extract_wheel_libs`;
/// `keep_lib`).
#[test]
fn only_nvidias_libraries_come_from_its_wheels() {
    use std::io::Write;
    let mut zip = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (path, bytes) in [
        ("nvidia/cudnn/lib/libcudnn.so.9", &b"cuDNN"[..]),
        ("nvidia/cudnn/include/cudnn.h", b"a header"),
        ("other/lib/libother.so", b"not NVIDIA's"),
        ("nvidia/../../libescaped.so", b"out of its folder"),
    ] {
        zip.start_file(path, stored).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.add_symlink("nvidia/cudnn/lib/libcudnn_link.so", "/etc/passwd", stored).unwrap();
    let dir = crate::TempDir::new("gpu-wheel");
    let wheel = dir.join("wheel.whl");
    fs::write(&wheel, zip.finish().unwrap().into_inner()).unwrap();
    let libs = dir.join("lib");
    fs::create_dir_all(&libs).unwrap();
    extract_wheel_libs(&wheel, &libs, &[".so"]).unwrap();
    assert_eq!(unpacked(&libs), [("libcudnn.so.9".to_string(), false)]);
    assert_eq!(fs::read(libs.join("libcudnn.so.9")).unwrap(), b"cuDNN");
    assert!(!dir.with_file_name("libescaped.so").exists());
}
