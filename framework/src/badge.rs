//! The unread count on the app's icon (RFC 0008). macOS and Linux draw the
//! number themselves; Windows shows an overlay icon on the taskbar button,
//! drawn here: a red disc with the count in white, `9+` past nine.

/// The overlay's side, in pixels.
pub(crate) const SIDE: u32 = 32;

/// 3×5 glyphs, a row per byte (the low three bits, left to right).
const fn glyph(c: char) -> [u8; 5] {
    match c {
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b001, 0b001, 0b001],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        _ => [0b000, 0b010, 0b111, 0b010, 0b000], // '+'
    }
}

/// What the badge says for `count`: `None` for none.
pub(crate) fn label(count: u32) -> Option<String> {
    match count {
        0 => None,
        1..=9 => Some(count.to_string()),
        _ => Some("9+".into()),
    }
}

/// The overlay for `count` (at least 1), as RGBA rows.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
pub(crate) fn overlay(count: u32) -> Vec<u8> {
    let text = label(count.max(1)).unwrap_or_default();
    let mut rgba = vec![0u8; (SIDE * SIDE * 4) as usize];
    let centre = (SIDE as f32 - 1.0) / 2.0;
    let radius = SIDE as f32 / 2.0;
    for y in 0..SIDE {
        for x in 0..SIDE {
            let (dx, dy) = (x as f32 - centre, y as f32 - centre);
            if dx * dx + dy * dy <= radius * radius {
                let at = ((y * SIDE + x) * 4) as usize;
                rgba[at..at + 4].copy_from_slice(&[0xE5, 0x39, 0x35, 0xFF]);
            }
        }
    }
    // One digit large; two smaller, a pixel apart.
    let scale: u32 = if text.len() == 1 { 4 } else { 3 };
    let width = text.len() as u32 * 3 * scale + (text.len() as u32 - 1) * scale;
    let (left, top) = ((SIDE - width) / 2, (SIDE - 5 * scale) / 2);
    for (i, c) in text.chars().enumerate() {
        let x0 = left + i as u32 * 4 * scale;
        for (row, bits) in glyph(c).iter().enumerate() {
            for col in 0..3 {
                if bits & (0b100 >> col) == 0 {
                    continue;
                }
                for py in 0..scale {
                    for px in 0..scale {
                        let (x, y) = (x0 + col * scale + px, top + row as u32 * scale + py);
                        let at = ((y * SIDE + x) * 4) as usize;
                        rgba[at..at + 4].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
                    }
                }
            }
        }
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(rgba: &[u8], x: u32, y: u32) -> [u8; 4] {
        let at = ((y * SIDE + x) * 4) as usize;
        rgba[at..at + 4].try_into().unwrap()
    }

    #[test]
    fn the_label_stops_at_nine() {
        assert_eq!(label(0), None);
        assert_eq!(label(3).as_deref(), Some("3"));
        assert_eq!(label(9).as_deref(), Some("9"));
        assert_eq!(label(10).as_deref(), Some("9+"));
    }

    #[test]
    fn a_red_disc_with_white_text_on_transparent_corners() {
        let one = overlay(1);
        assert_eq!(one.len(), (SIDE * SIDE * 4) as usize);
        assert_eq!(pixel(&one, 0, 0)[3], 0, "transparent corner");
        assert_eq!(pixel(&one, 2, 16), [0xE5, 0x39, 0x35, 0xFF], "the disc");
        let white = |rgba: &[u8]| {
            (0..SIDE * SIDE)
                .filter(|i| pixel(rgba, i % SIDE, i / SIDE) == [0xFF; 4])
                .count()
        };
        // "1" is 8 of 15 cells at 4×4; "8" is 13; "9+" is 12 + 5 at 3×3.
        assert_eq!(white(&one), 8 * 16);
        assert_eq!(white(&overlay(8)), 13 * 16);
        assert_eq!(white(&overlay(42)), 17 * 9);
    }
}
