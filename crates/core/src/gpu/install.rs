//! The specs of `install` carrying on: what an install left is taken up
//! by the next, and nothing is unpacked unchecked. The pack is the kit's
//! (`testkit::Addon`), served by the fake network.

use std::sync::Arc;

use super::*;
use crate::testkit::{Addon, Gate, Net, Recorded, TempDir};

fn never() -> AtomicBool {
    AtomicBool::new(false)
}

/// A pack folder with the kit's pack for it, served, and every request to
/// its files kept.
fn served(name: &str) -> (TempDir, PathBuf, Addon, Net, Recorded) {
    let dir = TempDir::new(name);
    let pack = dir.join("gpu");
    let addon = Addon::at(&pack);
    let net = Net::new();
    let host = addon.files()[0].0.rsplit_once('/').unwrap().0.to_string();
    let recorded = net.records(&host);
    addon.served_by(&net);
    (dir, pack, addon, net, recorded)
}

/// The links asked for, each with the range asked.
fn asked(recorded: &Recorded) -> Vec<(String, Option<String>)> {
    recorded.requests().into_iter().map(|r| (r.url.clone(), r.header("Range").map(String::from))).collect()
}

/// Installs on its own thread, stopped once `gate` is reached and it has
/// `had` bytes across its files (what came before the gate written): as a
/// quit stops it.
fn stopped_at(pack: &Path, gate: &Gate, had: u64) {
    let cancel = Arc::new(AtomicBool::new(false));
    let done = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let (stop, at, seen) = (cancel.clone(), pack.to_path_buf(), done.clone());
    let install = std::thread::spawn(move || install(&at, &stop, |p| seen.store(p.done, Ordering::Relaxed)));
    gate.reached();
    crate::testkit::eventually("what came before the gate written", || done.load(Ordering::Relaxed) >= had);
    cancel.store(true, Ordering::Relaxed);
    assert!(crate::is_canceled(&install.join().unwrap().unwrap_err()));
}

/// Stopped partway through a file, the next install carries on: the parts
/// unpacked aren't downloaded again, and the file it was downloading is
/// asked for from the byte it had (`install`: "the parts it unpacked are
/// kept, and the file it was downloading carries on from the bytes it had").
#[test]
fn an_install_stopped_partway_carries_on_from_what_it_had() {
    let (_dir, pack, addon, net, recorded) = served("gpu-partway");
    let (third, bytes) = addon.files()[2].clone();
    let gate = Gate::new();
    net.holds_partway(&third, 1_000, &gate);
    let before: u64 = addon.files()[..2].iter().map(|(_, b)| b.len() as u64).sum();
    stopped_at(&pack, &gate, before + 1_000);
    assert!(!is_installed(&pack));
    net.let_go();
    install(&pack, &never(), |_| {}).unwrap();
    assert!(is_installed(&pack));
    let urls: Vec<String> = addon.files().iter().map(|(u, _)| u.clone()).collect();
    let mut want: Vec<(String, Option<String>)> = urls.iter().take(3).map(|u| (u.clone(), None)).collect();
    want.push((third.clone(), Some("bytes=1000-".into())));
    want.extend(urls.iter().skip(3).map(|u| (u.clone(), None)));
    assert_eq!(asked(&recorded), want);
    assert!(bytes.len() > 1_000);
    assert!(!staging_dir(&pack).exists(), "staging left");
}

/// A file the install had whole and checked but hadn't unpacked when it
/// was stopped is unpacked from it at the next install, not downloaded
/// again (`install`: "a quit while unpacking unpacks it again from the
/// checked file").
#[test]
fn a_file_checked_but_not_unpacked_is_unpacked_from_it_next_time() {
    let (_dir, pack, addon, net, recorded) = served("gpu-unpack");
    // As a quit leaves it: parts 1 and 2 unpacked, part 3 whole beside them.
    let gate = Gate::new();
    net.holds_partway(&addon.files()[3].0, 10, &gate);
    stopped_at(&pack, &gate, 0);
    net.let_go();
    let staging = staging_dir(&pack);
    std::fs::write(staging.join("part-3.download"), &addon.files()[2].1).unwrap();
    record_unpacked(&staging, &[1, 2]).unwrap();
    let before = recorded.requests().len();
    install(&pack, &never(), |_| {}).unwrap();
    let urls: Vec<String> = recorded.requests()[before..].iter().map(|r| r.url.clone()).collect();
    assert_eq!(urls, [addon.files()[3].0.clone(), addon.files()[4].0.clone()], "only what it lacked");
    assert!(is_installed(&pack));
}

/// A checked file that's been changed since (junk where its bytes were)
/// is never unpacked: it's downloaded again first (`net::download_resumable`).
#[test]
fn a_damaged_download_is_never_unpacked() {
    let (_dir, pack, addon, net, recorded) = served("gpu-damaged");
    let gate = Gate::new();
    net.holds_partway(&addon.files()[0].0, 10, &gate);
    stopped_at(&pack, &gate, 0);
    net.let_go();
    let staging = staging_dir(&pack);
    let mut junk = addon.files()[0].1.clone();
    junk[100] ^= 0xff;
    std::fs::write(staging.join("part-1.download"), junk).unwrap();
    std::fs::remove_file(net::part_of(&staging.join("part-1.download"))).unwrap();
    let before = recorded.requests().len();
    install(&pack, &never(), |_| {}).unwrap();
    assert_eq!(recorded.requests()[before].url, addon.files()[0].0, "unpacked as it was");
    assert!(is_installed(&pack));
}

/// Staging that says nothing of what it unpacked (0.7.3's, whose install
/// deleted it at its start) or another pack version's is deleted, and the
/// pack starts over (`install`: "Staging from another pack version, or
/// from a version that didn't list what it unpacked, is deleted first").
#[test]
fn staging_without_its_list_or_of_another_version_starts_over() {
    for list in [None, Some("1\n1\n2\n3\n")] {
        let (_dir, pack, addon, _net, recorded) = served("gpu-old-staging");
        let staging = staging_dir(&pack);
        std::fs::create_dir_all(lib_dir(&staging)).unwrap();
        std::fs::write(lib_dir(&staging).join("libold.so"), b"old").unwrap();
        std::fs::write(net::part_of(&staging.join("part-1.download")), &addon.files()[0].1[..500]).unwrap();
        if let Some(list) = list {
            std::fs::write(done_list(&staging), list).unwrap();
        }
        install(&pack, &never(), |_| {}).unwrap();
        let ranges: Vec<Option<String>> = asked(&recorded).into_iter().map(|(_, r)| r).collect();
        assert_eq!(ranges, vec![None; 5], "{list:?}: carried on from another's");
        assert!(!lib_dir(&pack).join("libold.so").exists(), "{list:?}: another's library kept");
    }
}

/// What an install had goes with `discard` (Cancel's) and `uninstall`.
#[test]
fn discarding_or_removing_the_pack_deletes_what_its_install_had() {
    let (_dir, pack, addon, net, _recorded) = served("gpu-discard");
    let gate = Gate::new();
    net.holds_partway(&addon.files()[1].0, 10, &gate);
    stopped_at(&pack, &gate, 0);
    assert!(staging_dir(&pack).exists());
    discard(&pack).unwrap();
    assert!(!staging_dir(&pack).exists());
    net.let_go();
    let gate = Gate::new();
    net.holds_partway(&addon.files()[1].0, 10, &gate);
    stopped_at(&pack, &gate, 0);
    uninstall(&pack).unwrap();
    assert!(!staging_dir(&pack).exists() && !pack.exists());
}

/// A quit after a part is listed as unpacked but before its download is
/// deleted leaves that file in staging; the next install deletes it, so
/// the pack holds only what it's made of (`install`: a listed part's
/// download "deleted at the next install").
#[test]
fn a_download_left_after_its_part_is_listed_never_reaches_the_pack() {
    let (_dir, pack, addon, net, _recorded) = served("gpu-left");
    let gate = Gate::new();
    net.holds_partway(&addon.files()[3].0, 10, &gate);
    stopped_at(&pack, &gate, 0);
    net.let_go();
    let staging = staging_dir(&pack);
    assert_eq!(unpacked_parts(&staging), Some(vec![1, 2, 3]));
    // As a quit just after the list was written, before the delete.
    std::fs::write(staging.join("part-3.download"), &addon.files()[2].1).unwrap();
    install(&pack, &never(), |_| {}).unwrap();
    let mut names: Vec<String> =
        std::fs::read_dir(&pack).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    assert_eq!(names, ["installed", "lib"]);
}
