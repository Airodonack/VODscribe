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

    #[test]
    fn the_name_comes_without_its_folder_and_the_size_if_said() {
        let big = 5 << 32 | 7;
        let b =
            block(2, &[record("clips\\a b.mp4", FD_FILESIZE | FD_ATTRIBUTES, 0x80, big), record("c.mkv", 0, 0, 99)]);
        assert_eq!(items(&b), [named("a b.mp4", Some(big), 0), named("c.mkv", None, 1)]);
    }

    #[test]
    fn folders_and_nameless_records_are_left_out_but_counted() {
        let b = block(
            3,
            &[
                record("clips", FD_ATTRIBUTES, FILE_ATTRIBUTE_DIRECTORY, 0),
                record("", 0, 0, 0),
                record("clips/d.mp4", FD_FILESIZE, 0, 3),
            ],
        );
        assert_eq!(items(&b), [named("d.mp4", Some(3), 2)]);
    }

    #[test]
    fn a_short_or_overstated_block_is_read_only_as_far_as_it_goes() {
        assert_eq!(items(&[]), []);
        assert_eq!(items(&[1, 0]), []);
        let one = record("e.mp4", 0, 0, 0);
        assert_eq!(items(&block(9, std::slice::from_ref(&one))), [named("e.mp4", None, 0)], "a count past its end");
        assert_eq!(items(&block(1, &[one[..RECORD - 1].to_vec()])), [], "a cut record");
        assert_eq!(items(&block(0, &[one])), [], "none said");
    }

    #[test]
    fn a_name_filling_max_path_ends_at_the_record() {
        let long = "x".repeat(300);
        let mut r = record(&long, 0, 0, 0);
        // No terminating zero at all.
        r[RECORD - 2..].copy_from_slice(&u16::from(b'y').to_le_bytes());
        let got = items(&block(1, &[r]));
        assert_eq!(got[0].name.len(), 260);
    }
}
