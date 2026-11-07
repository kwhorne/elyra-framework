//! An image for the app's windows and its tray ([RFC 0028]): a PNG, read
//! once when the app is built.
//!
//! ```ignore
//! App::new()
//!     .icon(Icon::png(include_bytes!("../icons/app.png")))
//!     .tray(
//!         TrayConfig::new()
//!             .icon(Icon::png(include_bytes!("../icons/tray.png")))
//!             .template(),
//!     )
//! ```
//!
//! A PNG that can't be read isn't a panic: [`App::problems`] says so, and
//! [`App::run`] stops before anything starts.
//!
//! [RFC 0028]: https://github.com/kwhorne/elyra-framework/blob/main/docs/proposals/0028-tray-and-icons.md
//! [`App::problems`]: crate::App::problems
//! [`App::run`]: crate::App::run

use std::sync::Arc;

/// An image, as RGBA pixels: see the [module docs](self).
#[derive(Clone)]
pub struct Icon(Result<Arc<Pixels>, String>);

/// An icon's pixels, rows of RGBA.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Pixels {
    pub(crate) rgba: Vec<u8>,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl Icon {
    /// A PNG, of any colour type and depth. Square and at least 32 px
    /// suits a tray; 256 px or more, a window.
    pub fn png(bytes: &[u8]) -> Self {
        Self(decode(bytes).map(Arc::new))
    }

    /// RGBA pixels, `width` × `height`, a row at a time.
    pub fn rgba(rgba: Vec<u8>, width: u32, height: u32) -> Self {
        let expected = width as usize * height as usize * 4;
        if width == 0 || height == 0 || rgba.len() != expected {
            return Self(Err(format!(
                "{width}×{height} RGBA is {expected} bytes, not {}",
                rgba.len()
            )));
        }
        Self(Ok(Arc::new(Pixels {
            rgba,
            width,
            height,
        })))
    }

    /// What's wrong with it, when it couldn't be read.
    pub fn problem(&self) -> Option<&str> {
        self.0.as_ref().err().map(String::as_str)
    }

    /// Its size in pixels, when it could be read.
    pub fn size(&self) -> Option<(u32, u32)> {
        self.pixels().map(|p| (p.width, p.height))
    }

    pub(crate) fn pixels(&self) -> Option<&Pixels> {
        self.0.as_deref().ok()
    }

    pub(crate) fn shared(&self) -> Option<std::sync::Arc<Pixels>> {
        self.0.as_ref().ok().cloned()
    }
}

impl std::fmt::Debug for Icon {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Ok(p) => write!(f, "Icon({}×{})", p.width, p.height),
            Err(e) => write!(f, "Icon(unreadable: {e})"),
        }
    }
}

fn decode(bytes: &[u8]) -> Result<Pixels, String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    // Palettes expanded, 16 bits down to 8: one byte per channel after.
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| format!("not a PNG: {e}"))?;
    let size = reader.output_buffer_size().ok_or("the PNG is too large")?;
    let mut buf = vec![0; size];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("a broken PNG: {e}"))?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|&[r, g, b]| [r, g, b, 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => buf
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|&[g, a]| [g, g, g, a])
            .collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err("a palette PNG that wasn't expanded".into()),
    };
    Ok(Pixels {
        rgba,
        width: info.width,
        height: info.height,
    })
}

/// `icon` with a red dot in its lower-right corner: the unread mark where
/// the tray has no text beside its icon (Windows).
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
pub(crate) fn with_dot(icon: &Pixels) -> Pixels {
    let mut rgba = icon.rgba.clone();
    let (w, h) = (icon.width as f32, icon.height as f32);
    let radius = w.min(h) * 0.22;
    let (cx, cy) = (w - radius - w * 0.02, h - radius - h * 0.02);
    // A ring of the icon's own transparency around the dot keeps it apart
    // from what's under it.
    let ring = radius + w.min(h) * 0.06;
    for y in 0..icon.height {
        for x in 0..icon.width {
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            let d = (dx * dx + dy * dy).sqrt();
            let at = ((y * icon.width + x) * 4) as usize;
            if d <= radius {
                rgba[at..at + 4].copy_from_slice(&[0xE5, 0x39, 0x35, 0xFF]);
            } else if d <= ring {
                rgba[at + 3] = 0;
            }
        }
    }
    Pixels {
        rgba,
        width: icon.width,
        height: icon.height,
    }
}

/// The icon used when the app gives none: a rounded square in Elyra blue.
pub(crate) fn fallback() -> Pixels {
    const SIDE: u32 = 32;
    let mut rgba = Vec::with_capacity((SIDE * SIDE * 4) as usize);
    for y in 0..SIDE {
        for x in 0..SIDE {
            // Corners cut at a 6 px radius.
            let corner = |v: u32| {
                if v < 6 {
                    6 - v
                } else if v >= SIDE - 6 {
                    v + 7 - SIDE
                } else {
                    0
                }
            };
            let (cx, cy) = (corner(x) as f32, corner(y) as f32);
            let inside = cx == 0.0 || cy == 0.0 || cx * cx + cy * cy <= 36.0;
            rgba.extend_from_slice(if inside {
                &[59, 130, 246, 255]
            } else {
                &[0, 0, 0, 0]
            });
        }
    }
    Pixels {
        rgba,
        width: SIDE,
        height: SIDE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(color: png::ColorType, depth: png::BitDepth, w: u32, h: u32, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, w, h);
            encoder.set_color(color);
            encoder.set_depth(depth);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(data).unwrap();
        }
        out
    }

    #[test]
    fn an_rgba_png_is_read_as_it_is() {
        let px = [1, 2, 3, 4, 5, 6, 7, 8];
        let icon = Icon::png(&encode(
            png::ColorType::Rgba,
            png::BitDepth::Eight,
            2,
            1,
            &px,
        ));
        assert_eq!(icon.problem(), None);
        assert_eq!(icon.size(), Some((2, 1)));
        assert_eq!(icon.pixels().unwrap().rgba, px);
    }

    #[test]
    fn rgb_and_grey_pngs_become_rgba() {
        let rgb = Icon::png(&encode(
            png::ColorType::Rgb,
            png::BitDepth::Eight,
            1,
            1,
            &[9, 8, 7],
        ));
        assert_eq!(rgb.pixels().unwrap().rgba, [9, 8, 7, 255]);
        let grey = Icon::png(&encode(
            png::ColorType::Grayscale,
            png::BitDepth::Eight,
            1,
            1,
            &[40],
        ));
        assert_eq!(grey.pixels().unwrap().rgba, [40, 40, 40, 255]);
        let grey_alpha = Icon::png(&encode(
            png::ColorType::GrayscaleAlpha,
            png::BitDepth::Eight,
            1,
            1,
            &[40, 100],
        ));
        assert_eq!(grey_alpha.pixels().unwrap().rgba, [40, 40, 40, 100]);
    }

    #[test]
    fn sixteen_bits_come_down_to_eight() {
        let icon = Icon::png(&encode(
            png::ColorType::Rgba,
            png::BitDepth::Sixteen,
            1,
            1,
            &[0xAB, 0, 0xCD, 0, 0xEF, 0, 0xFF, 0xFF],
        ));
        assert_eq!(icon.pixels().unwrap().rgba, [0xAB, 0xCD, 0xEF, 0xFF]);
    }

    #[test]
    fn what_isnt_a_png_is_a_problem_not_a_panic() {
        let icon = Icon::png(b"<svg/>");
        assert!(icon.problem().unwrap().starts_with("not a PNG"), "{icon:?}");
        assert_eq!(icon.size(), None);
    }

    #[test]
    fn rgba_of_the_wrong_length_is_a_problem() {
        assert_eq!(Icon::rgba(vec![0; 16], 2, 2).problem(), None);
        assert_eq!(
            Icon::rgba(vec![0; 15], 2, 2).problem(),
            Some("2×2 RGBA is 16 bytes, not 15")
        );
        assert!(Icon::rgba(vec![], 0, 0).problem().is_some());
    }

    #[test]
    fn the_dot_is_red_in_the_lower_right_and_the_rest_is_kept() {
        let white = Pixels {
            rgba: [255u8; 4].repeat(32 * 32),
            width: 32,
            height: 32,
        };
        let dotted = with_dot(&white);
        let at = |x: u32, y: u32| {
            let i = ((y * 32 + x) * 4) as usize;
            dotted.rgba[i..i + 4].to_vec()
        };
        assert_eq!(at(25, 25), [0xE5, 0x39, 0x35, 0xFF], "the dot");
        assert_eq!(at(2, 2), [255; 4], "the icon, untouched");
        assert_eq!(at(25, 16)[3], 0, "a clear ring around the dot");
    }

    #[test]
    fn the_fallback_is_a_rounded_blue_square() {
        let p = fallback();
        assert_eq!((p.width, p.height), (32, 32));
        assert_eq!(&p.rgba[..4], &[0, 0, 0, 0], "a cut corner");
        let centre = ((16 * 32 + 16) * 4) as usize;
        assert_eq!(&p.rgba[centre..centre + 4], &[59, 130, 246, 255]);
    }
}
