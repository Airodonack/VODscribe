//! The compositor's drawing, read back from a software adapter: YUV turned
//! into the colours meant, on the CPU and the GPU alike, and a frame fitted
//! on black with an overlay over it.

use super::*;
use crate::Rect;
use vodscribe_core::local::{Colour, Matrix};

const BT709: Colour = Colour { matrix: Matrix::Bt709, full_range: false };
const JPEG: Colour = Colour { matrix: Matrix::Bt601, full_range: true };

/// A frame `w` by `h` of luma `luma(x, y)` and of chroma
/// `chroma(x, y)` for each two by two pixels from (x, y).
fn frame(
    id: u64,
    (w, h): (u32, u32),
    colour: Colour,
    luma: impl Fn(u32, u32) -> u8,
    chroma: impl Fn(u32, u32) -> [u8; 2],
) -> Frame {
    let points = |w: u32, h: u32, k: u32| (0..h).flat_map(move |y| (0..w).map(move |x| (x * k, y * k)));
    let mut yuv: Vec<u8> = points(w, h, 1).map(|(x, y)| luma(x, y)).collect();
    let (cw, ch) = decode::chroma(w, h);
    yuv.extend(points(cw, ch, 2).map(|(x, y)| chroma(x, y)[0]));
    yuv.extend(points(cw, ch, 2).map(|(x, y)| chroma(x, y)[1]));
    Frame { id, pts: 0.0, width: w, height: h, yuv, colour }
}

/// A frame of one colour, `yuv`.
fn flat(id: u64, size: (u32, u32), colour: Colour, [y, u, v]: [u8; 3]) -> Frame {
    frame(id, size, colour, |_, _| y, |_, _| [u, v])
}

fn at(image: &Image, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * image.width + x) * 4) as usize;
    image.rgba[i..i + 4].try_into().unwrap()
}

fn near(got: [u8; 4], want: [u8; 3], within: i32) -> bool {
    got[3] == 255 && got.iter().zip(want).all(|(&g, w)| (g as i32 - w as i32).abs() <= within)
}

/// A software adapter, or None where there isn't one (skipped then,
/// unless VODSCRIBE_TESTS_NEED_ALL is set, as in CI).
fn compositor() -> Option<Compositor> {
    match Compositor::new(Gpu::Software) {
        Ok(c) => Some(c),
        Err(e) if std::env::var_os("VODSCRIBE_TESTS_NEED_ALL").is_none() => {
            eprintln!("skipped: {e:#}");
            None
        }
        Err(e) => panic!("{e:#}"),
    }
}

/// The CPU turns each matrix and range of YUV into the colours meant, in
/// either layout (decode.rs `Frame::to_rgb`: "The picture as RGB, laid out
/// as `layout` says ... For what can't take YUV: the page's copy of the
/// frame ... and the screens that draw without a GPU").
#[test]
fn the_cpu_turns_yuv_into_the_colours_meant() {
    // Each matrix and range's red, green, blue, white and black.
    let cases = [
        (BT709, [63, 102, 240], [255, 0, 0]),
        (BT709, [173, 42, 26], [0, 255, 0]),
        (BT709, [32, 240, 118], [0, 0, 255]),
        (BT709, [235, 128, 128], [255, 255, 255]),
        (BT709, [16, 128, 128], [0, 0, 0]),
        (Colour::UNTAGGED, [81, 90, 240], [255, 0, 0]),
        (Colour::UNTAGGED, [145, 54, 34], [0, 255, 0]),
        (Colour::UNTAGGED, [41, 240, 110], [0, 0, 255]),
        (JPEG, [76, 85, 255], [255, 0, 0]),
        (JPEG, [150, 44, 21], [0, 255, 0]),
        (JPEG, [255, 128, 128], [255, 255, 255]),
        (JPEG, [0, 128, 128], [0, 0, 0]),
    ];
    for (colour, yuv, want) in cases {
        let mut rgb = Vec::new();
        flat(0, (2, 2), colour, yuv).to_rgb(decode::Layout::Rgb, &mut rgb);
        let got = [rgb[0], rgb[1], rgb[2], 255];
        assert!(near(got, want, 2), "{colour:?} {yuv:?}: {got:?}, not {want:?}");
        let mut bgrx = Vec::new();
        flat(0, (2, 2), colour, yuv).to_rgb(decode::Layout::Bgrx, &mut bgrx);
        assert_eq!([bgrx[2], bgrx[1], bgrx[0]], [rgb[0], rgb[1], rgb[2]], "the same, laid out the other way");
    }
}

/// What the GPU draws of a frame at its own size is what the CPU makes of
/// it, odd sizes too (compose.rs: "stills, and tests that check frames by
/// their pixels, come from the same drawing as the screen"; decode.rs
/// `rgb_matrix`: "The shader and `Frame::to_rgb` both convert with it"),
/// and never on a real GPU (compose.rs `Gpu::Software`: "tests use it, so
/// they never take a machine's real GPU").
#[test]
fn a_frame_at_its_own_size_reads_back_as_the_cpu_turns_it() {
    let Some(mut c) = compositor() else { return };
    assert_eq!(c.adapter().device_type, wgpu::DeviceType::Cpu, "never a real GPU in tests");
    // Luma that changes from pixel to pixel; one chroma, as the GPU
    // blends chroma between its samples where the CPU doesn't.
    // An odd size too: its chroma planes are half each side rounded up.
    for (id, size) in [(1, (64, 36)), (5, (33, 17))] {
        let f = frame(id, size, BT709, |x, y| (16 + (x * 3 + y * 5) % 220) as u8, |_, _| [90, 200]);
        let image = c.render(size, Some(&f), None).unwrap();
        let mut rgb = Vec::new();
        f.to_rgb(decode::Layout::Rgb, &mut rgb);
        assert_eq!(image.rgba.len() / 4, rgb.len() / 3);
        for (i, (got, want)) in image.rgba.as_chunks::<4>().0.iter().zip(rgb.as_chunks::<3>().0).enumerate() {
            assert!(near(*got, *want, 1), "{size:?}, pixel {i}: {got:?}, not {want:?}");
        }
    }
}

/// The GPU turns YUV into the colours meant, limited and full range
/// (compose.rs: "the picture, opaque, from its three planes of YUV turned
/// into RGB by its matrix").
#[test]
fn the_gpu_turns_yuv_into_the_colours_meant() {
    let Some(mut c) = compositor() else { return };
    // Four quarters: red, green, blue and white in BT.709's limited range.
    let quarters = [[63, 102, 240], [173, 42, 26], [32, 240, 118], [235, 128, 128]];
    let quarter = |x: u32, y: u32| quarters[(x / 16 + y / 16 * 2) as usize];
    let f = frame(3, (32, 32), BT709, |x, y| quarter(x, y)[0], |x, y| [quarter(x, y)[1], quarter(x, y)[2]]);
    let image = c.render((32, 32), Some(&f), None).unwrap();
    for ((x, y), want) in [((8, 8), [255, 0, 0]), ((24, 8), [0, 255, 0]), ((8, 24), [0, 0, 255]), ((24, 24), [255; 3])]
    {
        assert!(near(at(&image, x, y), want, 2), "at {x}, {y}: {:?}, not {want:?}", at(&image, x, y));
    }
    // Full range (a JPEG's): red.
    let image = c.render((8, 8), Some(&flat(4, (8, 8), JPEG, [76, 85, 255])), None).unwrap();
    assert!(near(at(&image, 4, 4), [255, 0, 0], 2), "{:?}", at(&image, 4, 4));
}

/// A frame is fitted and centred in its box on black, and an overlay
/// blended over it (compose.rs: "the picture fitted and centred on black,
/// then what's drawn over it (captions, notes: `Overlay`)").
#[test]
fn a_frame_is_fitted_on_black_and_the_overlay_blended_over_it() {
    let Some(mut c) = compositor() else { return };
    // A wide frame in a square box: black bars above and below.
    // (200, 100, 50) in full-range BT.601.
    let f = flat(2, (40, 20), JPEG, [124, 86, 182]);
    let image = c.render((40, 40), Some(&f), None).unwrap();
    assert_eq!(at(&image, 20, 2), [0, 0, 0, 255], "bar above");
    assert!(near(at(&image, 20, 20), [200, 100, 50], 1), "picture: {:?}", at(&image, 20, 20));
    assert_eq!(at(&image, 20, 37), [0, 0, 0, 255], "bar below");
    // Half-transparent white over the top-left quarter (premultiplied).
    let overlay = Overlay::new(Rect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 }, (1, 1), vec![128, 128, 128, 128]);
    let image = c.render((40, 40), Some(&f), Some((&overlay, 2.0))).unwrap();
    assert!(near(at(&image, 5, 15), [228, 178, 153], 2), "{:?}", at(&image, 5, 15));
    assert!(near(at(&image, 30, 15), [200, 100, 50], 1), "outside it, as it was");
}
