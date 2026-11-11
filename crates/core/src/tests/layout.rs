//! The library's layout: where `Paths` puts each file, and what clearing the cache keeps.

use super::*;

#[test]
fn clearing_the_cache_keeps_the_library_and_settings() {
    let dir = TempDir::new("cache");
    let paths = Paths::new(dir.join("data"));
    paths.ensure().unwrap();
    for dir in paths.cache_dirs() {
        std::fs::create_dir_all(dir.join("nested")).unwrap();
        std::fs::write(dir.join("nested/file"), b"12345").unwrap();
    }
    std::fs::write(paths.db(), b"library").unwrap();
    std::fs::write(paths.settings(), b"{}").unwrap();
    assert_eq!(paths.cache_size(), 5 * paths.cache_dirs().len() as u64);

    paths.clear_cache().unwrap();
    assert_eq!(paths.cache_size(), 0);
    let empty = |d: &PathBuf| std::fs::read_dir(d).map_or(true, |mut e| e.next().is_none());
    assert!(paths.cache_dirs().iter().all(empty), "cache dirs are gone or empty");
    assert_eq!(std::fs::read(paths.db()).unwrap(), b"library");
    assert_eq!(std::fs::read(paths.settings()).unwrap(), b"{}");
}

/// Every file of the library's is where `Paths` builds it: no code outside
/// this module joins a folder of the library's to a name by hand, from
/// `Paths` or from a folder handed in by its name (`work_dir`, `media`),
/// or finds one beside another (`with_file_name("media")`).
#[test]
fn only_paths_joins_the_librarys_folders() {
    let folders = [
        "root",
        "local",
        "work()",
        "media()",
        "thumbs()",
        "peaks()",
        "storyboards()",
        "emotes()",
        "received()",
        "clips()",
        "clip_thumbs()",
        "source_footage()",
        "bin()",
        "models()",
        "gpu_pack()",
    ];
    // Folders holding the library's own files, as a variable or
    // parameter names them when it's handed one.
    let handed =
        ["work", "media", "received", "clips", "clip_thumbs", "footage", "thumbs", "peaks", "storyboards", "emotes"];
    let by_hand = |line: &str| {
        folders.iter().any(|f| line.contains(&format!(".{f}.join(")))
            || handed.iter().any(|f| {
                line.contains(&format!("with_file_name(\"{f}\")"))
                    || ["", "_dir"].iter().any(|end| {
                        let join = format!("{f}{end}.join(");
                        line.match_indices(&join)
                            .any(|(at, _)| !line[..at].ends_with(|c: char| c.is_alphanumeric() || c == '_' || c == '.'))
                    })
            })
    };
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut found = Vec::new();
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(repo.join("crates"))
        .unwrap()
        .flatten()
        .map(|e| e.path().join("src"))
        .chain([repo.join("apps/desktop/src-tauri/src")])
        .collect();
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
                continue;
            }
            // Tests make folders of their own as they like, in files of
            // their own (`clips/tests.rs`, or `clips/tests/*.rs` for one
            // in parts) or a module's `mod tests`.
            let tests = path.file_name().is_some_and(|n| n == "tests.rs")
                || path.parent().is_some_and(|dir| dir.ends_with("tests"));
            if path.extension().is_none_or(|e| e != "rs") || path.ends_with("crates/core/src/lib.rs") || tests {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let code = text.split("\nmod tests {").next().unwrap_or_default();
            for (n, line) in code.lines().enumerate() {
                if by_hand(line) {
                    found.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
                }
            }
        }
    }
    assert!(found.is_empty(), "joined by hand, not built by Paths:\n{}", found.join("\n"));
}
