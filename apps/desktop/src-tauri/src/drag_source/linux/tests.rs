use super::*;

/// A PNG of `width` by `height` pixels, of `color` and 8 bits, holding `pixels`.
fn png_of(color: png::ColorType, width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(color);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().write_image_data(pixels).unwrap();
    bytes
}

/// The picture's size and its pixels as cairo holds them, row by row, 4 bytes
/// each (premultiplied B, G, R, A on this machine).
fn read(picture: gtk::cairo::ImageSurface) -> (i32, i32, Vec<Vec<u8>>) {
    let (width, height, stride) = (picture.width(), picture.height(), picture.stride() as usize);
    let mut picture = picture;
    let data = picture.data().unwrap();
    let rows = (0..height as usize).map(|y| data[y * stride..y * stride + width as usize * 4].to_vec()).collect();
    (width, height, rows)
}

/// The picture a drag shows is the PNG the page sent, its see-through parts
/// kept: cairo's pixels are premultiplied (`start`: "Its picture, set as it
/// begins"; 0.7.0: "Dragging several shows a pile. The first thing's picture
/// shows over its waveform").
#[cfg(target_endian = "little")]
#[test]
fn a_see_through_png_is_the_drags_picture_as_it_is() {
    let png = png_of(png::ColorType::Rgba, 2, 1, &[10, 20, 30, 128, 255, 0, 0, 255]);
    let (width, height, rows) = read(drag_picture(&png).expect("a picture"));
    assert_eq!((width, height), (2, 1));
    // B, G, R, A, each colour scaled by its alpha.
    assert_eq!(rows, [vec![15, 10, 5, 128, 0, 0, 255, 255]]);
}

/// A grey PNG with see-through parts is the picture too.
#[cfg(target_endian = "little")]
#[test]
fn a_grey_see_through_png_is_the_drags_picture_too() {
    let png = png_of(png::ColorType::GrayscaleAlpha, 1, 2, &[200, 255, 100, 0]);
    let (width, height, rows) = read(drag_picture(&png).expect("a picture"));
    assert_eq!((width, height), (1, 2));
    assert_eq!(rows, [vec![200, 200, 200, 255], vec![0, 0, 0, 0]]);
}

/// A PNG with no see-through parts shows whole, not as nothing (the picture
/// is a PNG the page made; one without alpha is still one).
#[cfg(target_endian = "little")]
#[test]
fn a_png_without_see_through_parts_is_the_drags_picture_whole() {
    let png = png_of(png::ColorType::Rgb, 1, 1, &[10, 20, 30]);
    let (_, _, rows) = read(drag_picture(&png).expect("a picture"));
    assert_eq!(rows, [vec![30, 20, 10, 255]]);
}

/// One that can't be decoded gives none (`start`: "One that can't be decoded:
/// none to see, rather than a theme icon").
#[test]
fn bytes_that_are_not_a_png_are_no_picture() {
    assert!(drag_picture(b"not a picture").is_none());
    assert!(drag_picture(&[]).is_none());
    let mut cut = png_of(png::ColorType::Rgba, 2, 2, &[0; 16]);
    cut.truncate(cut.len() / 2);
    assert!(drag_picture(&cut).is_none(), "a PNG cut short");
}
