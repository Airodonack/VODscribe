//! The colours come out right: videos of flat stripes of known colours,
//! stored as each common kind of YUV (BT.709 and BT.601, video's limited
//! range and JPEG's full one, tagged or not) and as RGB, are decoded as the
//! player decodes them, drawn by the compositor and read back, and each
//! stripe's middle checked against the colour it was made from; so is the
//! CPU's conversion of the same frame (what the page and the screens without
//! a GPU take). Skipped where there's no ffmpeg on PATH (or none with
//! libx264) or no software GPU adapter, except where VODSCRIBE_TESTS_NEED_ALL
//! is set (CI).

use std::path::Path;
use std::process::Command;

use vodscribe_player::compose::{Compositor, Gpu};
use vodscribe_player::decode::{self, Layout, Video};

/// The stripes, left to right, each `STRIPE` pixels wide.
const STRIPES: [(&str, [u8; 3]); 8] = [
    ("red", [255, 0, 0]),
    ("lime", [0, 255, 0]),
    ("blue", [0, 0, 255]),
    ("yellow", [255, 255, 0]),
    ("magenta", [255, 0, 255]),
    ("cyan", [0, 255, 255]),
    ("white", [255, 255, 255]),
    ("black", [0, 0, 0]),
];
const STRIPE: u32 = 16;

/// How far a channel may be from the colour meant: the 8-bit YUV the
/// colour was stored as, both ways.
const NEAR: i32 = 3;

/// The ways the stripes are stored: a name, the filters after the stripes,
/// and the encoder's options.
const KINDS: [(&str, &str, &[&str]); 5] = [
    (
        "H.264, BT.709, limited",
        "scale=out_color_matrix=bt709:out_range=tv,format=yuv420p",
        &["-c:v", "libx264", "-qp", "0", "-colorspace", "bt709", "-color_range", "tv"],
    ),
    (
        "H.264, untagged (BT.601, limited)",
        "scale=out_color_matrix=bt601:out_range=tv,format=yuv420p,setparams=colorspace=unknown:range=unknown",
        &["-c:v", "libx264", "-qp", "0"],
    ),
    (
        "H.264, BT.601, full",
        "scale=out_color_matrix=bt601:out_range=pc,format=yuv420p",
        &["-c:v", "libx264", "-qp", "0", "-colorspace", "bt470bg", "-color_range", "pc"],
    ),
    (
        "H.264, BT.709, full",
        "scale=out_color_matrix=bt709:out_range=pc,format=yuv420p",
        &["-c:v", "libx264", "-qp", "0", "-colorspace", "bt709", "-color_range", "pc"],
    ),
    ("PNG, RGB", "format=rgb24", &["-c:v", "png"]),
];

#[test]
fn each_kind_of_yuv_comes_out_the_colours_it_was_made_from() {
    let dir = std::env::temp_dir().join(format!("vodscribe-colours-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // Skipped without them, unless VODSCRIBE_TESTS_NEED_ALL is set (CI).
    let need = std::env::var_os("VODSCRIBE_TESTS_NEED_ALL").is_some();
    let mut compositor = match Compositor::new(Gpu::Software) {
        Ok(c) => c,
        Err(e) if !need => {
            eprintln!("skipped: {e:#}");
            return;
        }
        Err(e) => panic!("{e:#}"),
    };
    let ffmpeg = Path::new("ffmpeg");
    let inputs: Vec<String> = STRIPES
        .iter()
        .flat_map(|(name, _)| {
            ["-f".into(), "lavfi".into(), "-i".into(), format!("color=c={name}:s={STRIPE}x{STRIPE}:r=30:d=1")]
        })
        .collect();
    let stacked: String = (0..STRIPES.len()).map(|i| format!("[{i}:v]")).collect();
    let mut failures = Vec::new();
    for (i, (kind, filters, encoder)) in KINDS.iter().enumerate() {
        let path = dir.join(format!("{i}.mkv"));
        let made = Command::new(ffmpeg)
            .args(["-v", "error", "-y"])
            .args(&inputs)
            .arg("-filter_complex")
            .arg(format!("{stacked}hstack=inputs={}[s];[s]{filters}[v]", STRIPES.len()))
            .args(["-map", "[v]", "-frames:v", "1"])
            .args(*encoder)
            .arg(&path)
            .status();
        if !made.is_ok_and(|s| s.success()) {
            assert!(!need, "ffmpeg (with libx264) on PATH couldn't make {kind}");
            eprintln!("skipped: ffmpeg (with libx264) on PATH couldn't make {kind}");
            return;
        }
        let info = decode::probe(ffmpeg, &path).unwrap();
        let picture = info.picture.expect("a picture");
        let frame = Video::new(ffmpeg, &path, picture).from(0.0).unwrap().recv().expect("a frame");
        let image = compositor.render((frame.width, frame.height), Some(&frame), None).unwrap();
        let mut rgb = Vec::new();
        frame.to_rgb(Layout::Rgb, &mut rgb);
        for (n, (name, want)) in STRIPES.iter().enumerate() {
            let (x, y) = (n as u32 * STRIPE + STRIPE / 2, STRIPE / 2);
            let i = (y * frame.width + x) as usize;
            let gpu = &image.rgba[i * 4..i * 4 + 3];
            let cpu = &rgb[i * 3..i * 3 + 3];
            for (by, got) in [("GPU", gpu), ("CPU", cpu)] {
                if got.iter().zip(want).any(|(&g, &w)| (g as i32 - w as i32).abs() > NEAR) {
                    failures.push(format!("{kind} ({:?}), {by}: {name} came out {got:?}", picture.colour));
                }
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
