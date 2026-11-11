//! The specs of the crate root: `Progress`, and the files in `tests/` that hold the rest.

use super::*;

mod layout;

#[test]
fn a_file_placed_within_an_install() {
    let p = Progress::file("Downloading b", 30, Some(100)).within(250, Some(400), 2, 3);
    assert_eq!((p.done, p.total, p.file_done, p.file_size, p.part, p.parts), (280, Some(400), 30, Some(100), 2, 3));
    let alone = Progress::file("Downloading yt-dlp", 7, None);
    assert_eq!((alone.done, alone.total, alone.part, alone.parts), (7, None, 1, 1));
}
