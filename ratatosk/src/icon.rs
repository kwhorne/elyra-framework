//! The app's icon at the sizes a platform asks for, from the one source
//! image `[bundle].icon` names (an SVG or a PNG), drawn in Rust: no `sips`,
//! `rsvg-convert` or ImageMagick on the machine that bundles.
//!
//! Windows reads the sizes as an `.ico` (and the same images linked into the
//! `.exe`, [`crate::windows_res`]); Linux as PNGs in the `hicolor` theme and
//! beside an AppImage.

use std::path::Path;

use resvg::tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};
use resvg::usvg;

/// The sizes Windows draws an app's icon at: Explorer's views, the taskbar,
/// Alt-Tab, the Start menu, at every display scale.
pub const WINDOWS_SIZES: &[u32] = &[16, 20, 24, 32, 40, 48, 64, 256];

/// One square image of the icon.
pub struct Image(Pixmap);

impl Image {
    pub fn size(&self) -> u32 {
        self.0.width()
    }

    /// As a PNG file.
    pub fn png(&self) -> Result<Vec<u8>, String> {
        self.0.encode_png().map_err(|e| format!("encode png: {e}"))
    }

    /// Its pixels, row by row from the top, as straight (not premultiplied)
    /// RGBA.
    fn rgba(&self) -> Vec<[u8; 4]> {
        self.0
            .pixels()
            .iter()
            .map(|p| {
                let c = p.demultiply();
                [c.red(), c.green(), c.blue(), c.alpha()]
            })
            .collect()
    }
}

/// The icon at each of `sizes`, from `src`. An SVG is drawn at each size, so
/// a 16 px icon is drawn for 16 px, not shrunk from a large one; a PNG is
/// scaled. A source that isn't square is fitted and centred.
pub fn render(src: &Path, sizes: &[u32]) -> Result<Vec<Image>, String> {
    let bytes = std::fs::read(src).map_err(|e| format!("{}: {e}", src.display()))?;
    let svg = src
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("svg") || e.eq_ignore_ascii_case("svgz"));
    if svg {
        let tree = usvg::Tree::from_data(&bytes, &usvg::Options::default())
            .map_err(|e| format!("{}: {e}", src.display()))?;
        sizes.iter().map(|&px| svg_at(&tree, px)).collect()
    } else {
        let source = Pixmap::decode_png(&bytes)
            .map_err(|e| format!("{}: {e} (the icon must be an SVG or a PNG)", src.display()))?;
        sizes.iter().map(|&px| scaled(&source, px)).collect()
    }
}

/// Where a `w`×`h` picture goes in a `px` square: its scale, and the offset
/// that centres it.
fn fit(w: f32, h: f32, px: u32) -> (f32, f32, f32) {
    let px = px as f32;
    let scale = (px / w).min(px / h);
    (scale, (px - w * scale) / 2.0, (px - h * scale) / 2.0)
}

fn blank(px: u32) -> Result<Pixmap, String> {
    Pixmap::new(px, px).ok_or_else(|| format!("can't make a {px} px icon"))
}

fn svg_at(tree: &usvg::Tree, px: u32) -> Result<Image, String> {
    let size = tree.size();
    let (scale, dx, dy) = fit(size.width(), size.height(), px);
    let mut pixmap = blank(px)?;
    resvg::render(
        tree,
        Transform::from_row(scale, 0.0, 0.0, scale, dx, dy),
        &mut pixmap.as_mut(),
    );
    Ok(Image(pixmap))
}

/// `source` scaled into a `px` square. Shrunk by halves first, so a large
/// source keeps its detail at 16 px instead of skipping most of its pixels.
fn scaled(source: &Pixmap, px: u32) -> Result<Image, String> {
    let mut current = source.clone();
    while current.width().min(current.height()) >= px * 2 {
        let (w, h) = (current.width() / 2, current.height() / 2);
        let mut half = Pixmap::new(w.max(1), h.max(1)).ok_or("can't scale the icon")?;
        draw(&mut half, &current, 0.5, 0.0, 0.0);
        current = half;
    }
    let (scale, dx, dy) = fit(current.width() as f32, current.height() as f32, px);
    let mut pixmap = blank(px)?;
    draw(&mut pixmap, &current, scale, dx, dy);
    Ok(Image(pixmap))
}

fn draw(onto: &mut Pixmap, from: &Pixmap, scale: f32, dx: f32, dy: f32) {
    let paint = PixmapPaint {
        quality: FilterQuality::Bicubic,
        ..PixmapPaint::default()
    };
    onto.draw_pixmap(
        0,
        0,
        from.as_ref(),
        &paint,
        Transform::from_row(scale, 0.0, 0.0, scale, dx, dy),
        None,
    );
}

/// A plain icon, for where one must be and the app has none (an AppImage):
/// a square in Elyra's blue, as a PNG.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn plain(px: u32) -> Result<Vec<u8>, String> {
    let mut pixmap = blank(px)?;
    let inset = px as f32 / 8.0;
    let rect = resvg::tiny_skia::Rect::from_xywh(
        inset,
        inset,
        px as f32 - 2.0 * inset,
        px as f32 - 2.0 * inset,
    )
    .ok_or("can't draw the plain icon")?;
    let mut paint = resvg::tiny_skia::Paint::default();
    paint.set_color_rgba8(0x3b, 0x6e, 0xf5, 0xff);
    pixmap.fill_rect(rect, &paint, Transform::identity(), None);
    Image(pixmap).png()
}

/// Each image as Windows stores an icon's: a PNG from `png_from` px up (as
/// Windows Vista and later read a 256 px one), a bitmap below.
pub fn windows_images(images: &[Image], png_from: u32) -> Result<Vec<(u32, Vec<u8>)>, String> {
    images
        .iter()
        .map(|image| {
            let data = if image.size() >= png_from {
                image.png()?
            } else {
                dib(image)
            };
            Ok((image.size(), data))
        })
        .collect()
}

/// An `.ico` file of `images` (from [`windows_images`]).
pub fn ico(images: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes()); // reserved
    out.extend_from_slice(&1u16.to_le_bytes()); // an icon, not a cursor
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (size, data) in images {
        out.extend_from_slice(&entry_head(*size));
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in images {
        out.extend_from_slice(data);
    }
    out
}

/// The first eight bytes of an icon's directory entry, alike in an `.ico`
/// and in an `.exe`'s icon group: width and height (0 is 256), no palette,
/// one plane, 32 bits a pixel.
pub fn entry_head(size: u32) -> [u8; 8] {
    let side = if size >= 256 { 0 } else { size as u8 };
    let mut head = [side, side, 0, 0, 0, 0, 0, 0];
    head[4..6].copy_from_slice(&1u16.to_le_bytes());
    head[6..8].copy_from_slice(&32u16.to_le_bytes());
    head
}

/// An icon image as a device-independent bitmap: the header, the colour
/// rows bottom up as BGRA, then the 1-bit mask (set where it's transparent)
/// that Windows still reads beside the alpha.
fn dib(image: &Image) -> Vec<u8> {
    let side = image.size();
    let pixels = image.rgba();
    let mask_row = side.div_ceil(32) as usize * 4;
    let colour_bytes = (side * side * 4) as usize;
    let mut out = Vec::with_capacity(40 + colour_bytes + mask_row * side as usize);
    for value in [40u32, side, side * 2] {
        out.extend_from_slice(&value.to_le_bytes()); // size, width, height (colour + mask)
    }
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&32u16.to_le_bytes()); // bits a pixel
    out.extend_from_slice(&0u32.to_le_bytes()); // uncompressed
    out.extend_from_slice(&((colour_bytes + mask_row * side as usize) as u32).to_le_bytes());
    out.extend_from_slice(&[0; 16]); // resolution and palette: unused
    let row = |y: u32| &pixels[(y * side) as usize..((y + 1) * side) as usize];
    for y in (0..side).rev() {
        for [r, g, b, a] in row(y) {
            out.extend_from_slice(&[*b, *g, *r, *a]);
        }
    }
    for y in (0..side).rev() {
        let mut bits = vec![0u8; mask_row];
        for (x, [_, _, _, a]) in row(y).iter().enumerate() {
            if *a == 0 {
                bits[x / 8] |= 0x80 >> (x % 8);
            }
        }
        out.extend_from_slice(&bits);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="32"><rect x="0" y="0" width="64" height="32" fill="#c25b3a"/></svg>"##;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rata-icon-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn pixel(image: &Image, x: u32, y: u32) -> [u8; 4] {
        image.rgba()[(y * image.size() + x) as usize]
    }

    #[test]
    fn an_svg_is_drawn_at_each_size_fitted_and_centred() {
        let dir = scratch("svg");
        let src = dir.join("icon.svg");
        std::fs::write(&src, SVG).unwrap();
        let images = render(&src, &[16, 256]).unwrap();
        assert_eq!(
            images.iter().map(Image::size).collect::<Vec<_>>(),
            [16, 256]
        );
        // 2:1, so a band across the middle; above and below it, nothing.
        let big = &images[1];
        assert_eq!(pixel(big, 128, 128), [0xc2, 0x5b, 0x3a, 255]);
        assert_eq!(pixel(big, 128, 10)[3], 0);
        assert_eq!(pixel(big, 128, 250)[3], 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_png_is_scaled_down_and_up() {
        let dir = scratch("png");
        let src = dir.join("icon.png");
        let mut source = Pixmap::new(300, 300).unwrap();
        source.fill(resvg::tiny_skia::Color::from_rgba8(10, 120, 200, 255));
        std::fs::write(&src, source.encode_png().unwrap()).unwrap();
        for image in render(&src, &[16, 48, 512]).unwrap() {
            let mid = image.size() / 2;
            assert_eq!(
                pixel(&image, mid, mid),
                [10, 120, 200, 255],
                "{} px",
                image.size()
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_source_that_isnt_an_image_says_so() {
        let dir = scratch("bad");
        let src = dir.join("icon.jpg");
        std::fs::write(&src, b"\xff\xd8\xff not really").unwrap();
        let err = render(&src, &[16]).err().unwrap();
        assert!(err.contains("SVG or a PNG"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_ico_lists_each_image_where_it_is() {
        let dir = scratch("ico");
        let src = dir.join("icon.svg");
        std::fs::write(&src, SVG).unwrap();
        let images = windows_images(&render(&src, WINDOWS_SIZES).unwrap(), 256).unwrap();
        let file = ico(&images);
        let u16_at = |i: usize| u16::from_le_bytes([file[i], file[i + 1]]);
        let u32_at = |i: usize| u32::from_le_bytes(file[i..i + 4].try_into().unwrap());
        assert_eq!((u16_at(0), u16_at(2)), (0, 1));
        assert_eq!(u16_at(4) as usize, WINDOWS_SIZES.len());
        for (n, &size) in WINDOWS_SIZES.iter().enumerate() {
            let entry = 6 + 16 * n;
            assert_eq!(file[entry] as u32, if size == 256 { 0 } else { size });
            let (len, at) = (u32_at(entry + 8) as usize, u32_at(entry + 12) as usize);
            let data = &file[at..at + len];
            if size == 256 {
                assert_eq!(&data[..8], b"\x89PNG\r\n\x1a\n");
            } else {
                // A bitmap: its header, twice as tall (the mask), the rows and the mask.
                assert_eq!(u32_at(at), 40);
                assert_eq!(u32_at(at + 4), size);
                assert_eq!(u32_at(at + 8), size * 2);
                let mask = size.div_ceil(32) as usize * 4 * size as usize;
                assert_eq!(len, 40 + (size * size * 4) as usize + mask);
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_plain_icon_is_a_png() {
        let png = plain(64).unwrap();
        let pixmap = Pixmap::decode_png(&png).unwrap();
        assert_eq!((pixmap.width(), pixmap.height()), (64, 64));
        assert_eq!(pixmap.pixel(32, 32).unwrap().alpha(), 255);
        assert_eq!(pixmap.pixel(1, 1).unwrap().alpha(), 0);
    }

    #[test]
    fn a_bitmap_is_bottom_up_bgra_with_its_mask() {
        let mut pixmap = Pixmap::new(2, 2).unwrap();
        // Top left opaque red; the rest transparent.
        pixmap.pixels_mut()[0] = resvg::tiny_skia::ColorU8::from_rgba(255, 0, 0, 255).premultiply();
        let data = dib(&Image(pixmap));
        // Rows bottom up: the bottom row first (transparent), then the top.
        assert_eq!(&data[40..48], &[0; 8]);
        assert_eq!(&data[48..52], &[0, 0, 255, 255]);
        // The mask: rows of four bytes, bottom up; set where transparent.
        assert_eq!(&data[56..60], &[0b1100_0000, 0, 0, 0]);
        assert_eq!(&data[60..64], &[0b0100_0000, 0, 0, 0]);
    }
}
