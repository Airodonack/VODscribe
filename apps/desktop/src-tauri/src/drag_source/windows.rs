//! OLE's drag of files out of the window, with `dragndrop_source::windows`'s
//! data object and drop source, and the drag's picture.

use anyhow::Result;
use dragndrop_source::windows::{Drag, button_held};
use windows::Win32::Foundation::{COLORREF, HWND, POINT, SIZE};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateDIBSection, DIB_RGB_COLORS, DeleteObject, HBITMAP,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::Ole::{DROPEFFECT_COPY, DROPEFFECT_NONE, DoDragDrop, OleInitialize};
use windows::Win32::UI::Shell::{CLSID_DragDropHelper, IDragSourceHelper, SHDRAGIMAGE};

use super::{Event, Item, ReleasedEarly};

/// Runs OLE's drag of `items` out of `window`, showing `image` (a PNG), on
/// the UI thread, from the press that is still down. Returns when the drag
/// is over on screen; `on` hears how it went, perhaps later, on another
/// thread (once Explorer's copies are written).
pub(crate) fn start(
    window: &crate::WebviewWindow,
    items: Vec<Box<dyn Item>>,
    image: &[u8],
    on: impl Fn(Event) + Send + Sync + 'static,
) -> Result<()> {
    // A drag begun with no button held never ends as one (it's let go at
    // once where the pointer is): the release may have come in the moment
    // between the page asking and this. Refused, so the page treats it as a
    // drag that didn't happen.
    if !button_held() {
        return Err(ReleasedEarly.into());
    }
    super::traced();
    let hwnd: HWND = window.hwnd()?;
    // SAFETY: OLE calls on the UI thread (tao has initialized OLE there for
    // its drop target; a second call only counts up). The helper owns the
    // bitmap once it took it.
    unsafe {
        let _ = OleInitialize(None);
        let drag = Drag::new(hwnd, items, on)?;
        if let Some((bitmap, size)) = picture(image) {
            let shown = SHDRAGIMAGE {
                sizeDragImage: size,
                ptOffset: POINT { x: size.cx / 2, y: size.cy / 2 },
                hbmpDragImage: bitmap,
                crColorKey: COLORREF(0xFFFF_FFFF),
            };
            let helper: windows::core::Result<IDragSourceHelper> =
                CoCreateInstance(&CLSID_DragDropHelper, None, CLSCTX_INPROC_SERVER);
            match helper.and_then(|h| h.InitializeFromBitmap(&shown, drag.data_object())) {
                Ok(()) => super::trace("its picture set"),
                Err(e) => {
                    super::trace(&format!("no picture: {e}"));
                    let _ = DeleteObject(bitmap.into());
                }
            }
        }
        let source = drag.drop_source();
        let mut effect = DROPEFFECT_NONE;
        let hr = DoDragDrop(drag.data_object(), &source, DROPEFFECT_COPY, &mut effect);
        drag.returned(hr, effect);
    }
    Ok(())
}

/// `png` as a bitmap for the drag's picture: 32-bit, premultiplied, top
/// down, as IDragSourceHelper wants it.
fn picture(png: &[u8]) -> Option<(HBITMAP, SIZE)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png));
    decoder.set_transformations(png::Transformations::normalize_to_color8() | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().ok()?;
    let mut pixels = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut pixels).ok()?;
    let channels = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::GrayscaleAlpha => 2,
        _ => return None,
    };
    let (width, height) = (i32::try_from(info.width).ok()?, i32::try_from(info.height).ok()?);
    let header = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width,
        biHeight: -height,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    let bmi = BITMAPINFO { bmiHeader: header, ..Default::default() };
    let mut bits = std::ptr::null_mut();
    // SAFETY: the section is width x height x 4 bytes, written within that.
    unsafe {
        let bitmap = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        let out = std::slice::from_raw_parts_mut(bits as *mut u8, (width * height * 4) as usize);
        for (y, row) in pixels.chunks(info.line_size).take(info.height as usize).enumerate() {
            for (x, px) in row.chunks(channels).take(info.width as usize).enumerate() {
                let (r, g, b, a) =
                    if channels == 4 { (px[0], px[1], px[2], px[3]) } else { (px[0], px[0], px[0], px[1]) };
                let pre = |c: u8| (u16::from(c) * u16::from(a) / 255) as u8;
                let at = (y * info.width as usize + x) * 4;
                out[at..at + 4].copy_from_slice(&[pre(b), pre(g), pre(r), a]);
            }
        }
        Some((bitmap, SIZE { cx: width, cy: height }))
    }
}
