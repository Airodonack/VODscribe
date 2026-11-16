//! FileGroupDescriptorW, Windows' list of the virtual files a drag offers,
//! read from its bytes: a count, then that many FILEDESCRIPTORW records.
//! Read by hand rather than through the windows crate's struct so that it's
//! tested everywhere, and a short or lying block can't be read past its end.

use crate::Item;

/// The length of one FILEDESCRIPTORW: flags, CLSID, SIZEL, POINTL,
/// attributes, three FILETIMEs, the size's two halves, MAX_PATH UTF-16 units.
pub(crate) const RECORD: usize = 4 + 16 + 8 + 8 + 4 + 3 * 8 + 4 + 4 + 260 * 2;

const FLAGS: usize = 0;
const ATTRIBUTES: usize = 36;
const SIZE_HIGH: usize = 64;
const SIZE_LOW: usize = 68;
const NAME: usize = 72;

/// dwFileAttributes is valid.
const FD_ATTRIBUTES: u32 = 0x4;
/// nFileSizeHigh and nFileSizeLow are valid.
const FD_FILESIZE: u32 = 0x40;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;

/// The files in descriptor block `block`, folders left out. Each keeps its
/// place in the block (FileContents' index), folders counted.
pub(crate) fn items(block: &[u8]) -> Vec<Item> {
    let Some(count) = u32_at(block, 0) else { return vec![] };
    block[4..].chunks_exact(RECORD).take(count as usize).enumerate().filter_map(|(i, r)| item(i, r)).collect()
}

/// Record `r`, the `index`th, unless it's a folder or has no name.
fn item(index: usize, r: &[u8]) -> Option<Item> {
    let flags = u32_at(r, FLAGS)?;
    if flags & FD_ATTRIBUTES != 0 && u32_at(r, ATTRIBUTES)? & FILE_ATTRIBUTE_DIRECTORY != 0 {
        return None;
    }
    let units: Vec<u16> =
        r[NAME..].chunks_exact(2).map(|u| u16::from_le_bytes([u[0], u[1]])).take_while(|&u| u != 0).collect();
    let path = String::from_utf16_lossy(&units);
    // A file in a folder of the drag's comes as "folder\name".
    let name = path.rsplit(['\\', '/']).next().unwrap_or_default().to_string();
    let size = match flags & FD_FILESIZE {
        0 => None,
        _ => Some(u64::from(u32_at(r, SIZE_HIGH)?) << 32 | u64::from(u32_at(r, SIZE_LOW)?)),
    };
    (!name.is_empty()).then_some(Item { name, size, index })
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A FILEDESCRIPTORW's bytes.
    pub(crate) fn record(name: &str, flags: u32, attributes: u32, size: u64) -> Vec<u8> {
        let mut r = vec![0u8; RECORD];
        r[FLAGS..FLAGS + 4].copy_from_slice(&flags.to_le_bytes());
        r[ATTRIBUTES..ATTRIBUTES + 4].copy_from_slice(&attributes.to_le_bytes());
        r[SIZE_HIGH..SIZE_HIGH + 4].copy_from_slice(&((size >> 32) as u32).to_le_bytes());
        r[SIZE_LOW..SIZE_LOW + 4].copy_from_slice(&(size as u32).to_le_bytes());
        for (i, u) in name.encode_utf16().take(259).enumerate() {
            r[NAME + 2 * i..NAME + 2 * i + 2].copy_from_slice(&u.to_le_bytes());
        }
        r
    }

    fn block(count: u32, records: &[Vec<u8>]) -> Vec<u8> {
        let mut b = count.to_le_bytes().to_vec();
        records.iter().for_each(|r| b.extend_from_slice(r));
        b
    }

    fn named(name: &str, size: Option<u64>, index: usize) -> Item {
        Item { name: name.into(), size, index }
    }

    /// 0.7.2: "Drop in videos from zips and apps. A video dragged from a zip in File Explorer ...".
    /// `items`: "a count, then that many FILEDESCRIPTORW records": a file's name is read from its
    /// record.
    #[test]
    fn a_files_name_is_read_from_its_record() {
        let b = block(2, &[record("a b.mp4", 0, 0, 0), record("c.mkv", 0, 0, 0)]);
        assert_eq!(items(&b), [named("a b.mp4", None, 0), named("c.mkv", None, 1)]);
    }

    /// `items`: "A file in a folder of the drag's comes as "folder\name"": the name is the last part,
    /// whichever separator.
    #[test]
    fn a_file_in_a_folder_of_the_drag_is_named_without_the_folder() {
        let b = block(2, &[record("clips\\a b.mp4", 0, 0, 0), record("clips/d.mp4", 0, 0, 0)]);
        assert_eq!(items(&b), [named("a b.mp4", None, 0), named("d.mp4", None, 1)]);
    }

    /// `FD_FILESIZE`: "nFileSizeHigh and nFileSizeLow are valid": the size is both halves when the
    /// record says so, and unknown when it doesn't.
    #[test]
    fn the_size_is_both_halves_when_the_record_says_so_and_unknown_otherwise() {
        let big = 5 << 32 | 7;
        let b = block(2, &[record("a.mp4", FD_FILESIZE, 0, big), record("b.mp4", 0, 0, 99)]);
        assert_eq!(items(&b), [named("a.mp4", Some(big), 0), named("b.mp4", None, 1)]);
    }

    /// `items`: "The files in descriptor block `block`, folders left out. Each keeps its place in the
    /// block (FileContents' index), folders counted."
    #[test]
    fn a_folder_is_left_out_but_counted_in_the_places() {
        let b = block(
            2,
            &[record("clips", FD_ATTRIBUTES, FILE_ATTRIBUTE_DIRECTORY, 0), record("clips/d.mp4", FD_FILESIZE, 0, 3)],
        );
        assert_eq!(items(&b), [named("d.mp4", Some(3), 1)]);
    }

    /// `FD_ATTRIBUTES`: "dwFileAttributes is valid": a file whose attributes are given, and aren't a
    /// folder's, is a file.
    #[test]
    fn a_file_whose_attributes_are_given_is_kept() {
        let b = block(1, &[record("a.mp4", FD_ATTRIBUTES, 0x80, 0)]);
        assert_eq!(items(&b), [named("a.mp4", None, 0)]);
    }

    /// `item`: "unless it's a folder or has no name": a nameless record is left out, and counted.
    #[test]
    fn a_nameless_record_is_left_out_but_counted_in_the_places() {
        let b = block(2, &[record("", 0, 0, 0), record("d.mp4", 0, 0, 0)]);
        assert_eq!(items(&b), [named("d.mp4", None, 1)]);
    }

    /// `FD_ATTRIBUTES`: "dwFileAttributes is valid": without it the attributes aren't looked at, so
    /// a record that doesn't say them is a file.
    #[test]
    fn attributes_a_record_doesnt_say_are_not_read() {
        let b = block(1, &[record("a.mp4", 0, FILE_ATTRIBUTE_DIRECTORY, 0)]);
        assert_eq!(items(&b), [named("a.mp4", None, 0)]);
    }

    /// Module doc: "a short or lying block can't be read past its end": nothing, or a short count's
    /// worth, comes of a block with no room for its count.
    #[test]
    fn a_block_too_short_for_its_count_has_no_items() {
        assert_eq!(items(&[]), []);
        assert_eq!(items(&[1, 0]), []);
    }

    /// Module doc: "a short or lying block can't be read past its end": a count past the records
    /// there are reads only those.
    #[test]
    fn a_count_past_the_records_there_are_reads_only_those() {
        let one = record("e.mp4", 0, 0, 0);
        assert_eq!(items(&block(9, std::slice::from_ref(&one))), [named("e.mp4", None, 0)]);
    }

    /// Module doc, as above: a cut record is left out, and a count of none reads none.
    #[test]
    fn a_cut_record_and_a_count_of_none_are_read_as_none() {
        let one = record("e.mp4", 0, 0, 0);
        assert_eq!(items(&block(1, &[one[..RECORD - 1].to_vec()])), []);
        assert_eq!(items(&block(0, &[one])), []);
    }

    /// A count below the records there are reads only that many.
    #[test]
    fn a_count_below_the_records_there_are_reads_only_that_many() {
        let b = block(1, &[record("a.mp4", 0, 0, 0), record("b.mp4", 0, 0, 0)]);
        assert_eq!(items(&b), [named("a.mp4", None, 0)]);
    }

    /// `RECORD`: "MAX_PATH UTF-16 units": a name filling all of them, with no NUL, ends at the
    /// record.
    #[test]
    fn a_name_filling_max_path_ends_at_the_record() {
        let long = "x".repeat(300);
        let mut r = record(&long, 0, 0, 0);
        r[RECORD - 2..].copy_from_slice(&u16::from(b'y').to_le_bytes());
        let got = items(&block(1, &[r]));
        assert_eq!(got[0].name.len(), 260);
    }
}
