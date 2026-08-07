//! Fixed 8x15 bitmap font used by the Classic text renderer.
//!
//! The bitmap is Ka-Ping Yee's freely usable ISO font, sourced independently
//! from its upstream distribution. Rows use the original least-significant-bit
//! first convention. Only printable ASCII is retained here.

const FIRST: u8 = b' ';
const LAST: u8 = b'~';
const GLYPH_HEIGHT: usize = 15;

// Source: https://lfw.org/font.html (Copyright 2000 Ka-Ping Yee; freely usable
// for any purpose). Base64 only keeps the 95 printable 8x15 glyphs compact.
const PRINTABLE_GLYPHS: &str = "AAAAAAAAAAAAAAAAAAAAAAAYGBgYGBgYABgYAAAAAABsbDYAAAAAAAAAAAAAAAAANjZ/NjZ/NjYAAAAAAAgIPmsLCz5oaGs+CAgAAAAAMxMYCAwEBjIzAAAAAAAcNjYcbD4zM3vOAAAAAAAYGAwAAAAAAAAAAAAAAAAwGBgMDAwMDBgYMAAAAAAMGBgwMDAwMBgYDAAAAAAAAAA2HH8cNgAAAAAAAAAAAAAYGH4YGAAAAAAAAAAAAAAAAAAAABgYDAAAAAAAAAAAAH4AAAAAAAAAAAAAAAAAAAAAABgYAAAAAABgIDAQGAgMBAYCAwAAAAA+Y2Nja2tjY2M+AAAAAAAYHhgYGBgYGBgYAAAAAAA+Y2BgMBgMBgN/AAAAAAA+Y2BgPGBgYGM+AAAAAAAwODw2M38wMDAwAAAAAAB/AwM/YGBgYGM+AAAAAAA8BgMDP2NjY2M+AAAAAAB/YDAwGBgYDAwMAAAAAAA+Y2NjPmNjY2M+AAAAAAA+Y2NjfmBgYDAeAAAAAAAAAAAYGAAAABgYAAAAAAAAAAAYGAAAABgYDAAAAABgMBgMBgYMGDBgAAAAAAAAAAB+AAB+AAAAAAAAAAAGDBgwYGAwGAwGAAAAAAA+Y2AwMBgYABgYAAAAAAA8ZnN7a2t7MwY8AAAAAAA+Y2Njf2NjY2NjAAAAAAA/Y2NjP2NjY2M/AAAAAAA8ZgMDAwMDA2Y8AAAAAAAfM2NjY2NjYzMfAAAAAAB/AwMDPwMDAwN/AAAAAAB/AwMDPwMDAwMDAAAAAAA8ZgMDA3NjY2Z8AAAAAABjY2Njf2NjY2NjAAAAAAA8GBgYGBgYGBg8AAAAAAAwMDAwMDAwMDMeAAAAAABjMxsPBwcPGzNjAAAAAAADAwMDAwMDAwN/AAAAAABjY3d/f2trY2NjAAAAAABjY2dvb3t7c2NjAAAAAAA+Y2NjY2NjY2M+AAAAAAA/Y2NjYz8DAwMDAAAAAAA+Y2NjY2Njb3s+MGAAAAA/Y2NjYz8bM2NjAAAAAAA+YwMDDjhgYGM+AAAAAAB+GBgYGBgYGBgYAAAAAABjY2NjY2NjY2M+AAAAAABjY2NjYzY2HBwIAAAAAABjY2tra2t/NjY2AAAAAABjYzY2HBw2NmNjAAAAAADDw2ZmPDwYGBgYAAAAAAB/MDAYGAwMBgZ/AAAAAAA8DAwMDAwMDAw8AAAAAAADAgYEDAgYEDAgYAAAAAA8MDAwMDAwMDA8AAAAAAgcNmMAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAP8AAAAMDBgAAAAAAAAAAAAAAAAAAAA+YH5jY3NuAAAAAAADAwM7Z2NjY2c7AAAAAAAAAAA+YwMDA2M+AAAAAABgYGBuc2NjY3NuAAAAAAAAAAA+Y2N/A2M+AAAAAAA8ZgYfBgYGBgYGAAAAAAAAAABuc2NjY3NuYGM+AAADAwM7Z2NjY2NjAAAAAAAMDAAMDAwMDAw4AAAAAAAwMAAwMDAwMDAwMDMeAAADAwNjMxsPHzNjAAAAAAAMDAwMDAwMDAw4AAAAAAAAAAA1a2tra2trAAAAAAAAAAA7Z2NjY2NjAAAAAAAAAAA+Y2NjY2M+AAAAAAAAAAA7Z2NjY2c7AwMDAAAAAABuc2NjY3NuYOBgAAAAAAA7ZwMDAwMDAAAAAAAAAAA+Yw44YGM+AAAAAAAADAw+DAwMDAw4AAAAAAAAAABjY2NjY3NuAAAAAAAAAABjYzY2HBwIAAAAAAAAAABja2trPjY2AAAAAAAAAABjNhwcHDZjAAAAAAAAAABjYzY2HBwMDAYDAAAAAAB/YDAYDAZ/AAAAAABwGBgYGA4YGBgYcAAAABgYGBgYGBgYGBgYGAAAAAAOGBgYGHAYGBgYDgAAAAAAAAAAbjsAAAAAAAAA";

pub(super) fn glyph(byte: u8) -> Option<[u8; GLYPH_HEIGHT]> {
    if !(FIRST..=LAST).contains(&byte) {
        return None;
    }
    let offset = usize::from(byte - FIRST) * GLYPH_HEIGHT;
    Some(std::array::from_fn(|row| decode_byte(offset + row)))
}

fn decode_byte(index: usize) -> u8 {
    let group = index / 3;
    let remainder = index % 3;
    let encoded = PRINTABLE_GLYPHS.as_bytes();
    let offset = group * 4;
    let a = base64_value(encoded[offset]);
    let b = base64_value(encoded[offset + 1]);
    let c = base64_value(encoded[offset + 2]);
    let d = base64_value(encoded[offset + 3]);
    match remainder {
        0 => (a << 2) | (b >> 4),
        1 => (b << 4) | (c >> 2),
        _ => (c << 6) | d,
    }
}

const fn base64_value(byte: u8) -> u8 {
    match byte {
        b'A'..=b'Z' => byte - b'A',
        b'a'..=b'z' => byte - b'a' + 26,
        b'0'..=b'9' => byte - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_printable_glyphs_at_the_original_size() {
        assert_eq!(glyph(b' '), Some([0; 15]));
        assert_eq!(
            glyph(b'E'),
            Some([0, 0, 0x7f, 3, 3, 3, 0x3f, 3, 3, 3, 3, 0x7f, 0, 0, 0])
        );
        assert_eq!(glyph(0x1f), None);
        assert_eq!(glyph(0x7f), None);
    }
}
