use pal_assets::rle::RleBitmap;
use pal_assets::text::BitmapFont;

use crate::debug_overlay::glyph;
use crate::renderer::Renderer;

pub(super) fn draw_dialog_text(
    renderer: &mut Renderer,
    font: &BitmapFont,
    text: &[u8],
    x: i32,
    y: i32,
    initial_color: u8,
    glyph_limit: usize,
) -> (i32, usize, u8) {
    let mut cursor_x = x;
    let mut color = initial_color;
    let mut index = 0;
    let mut escaped = false;
    let mut glyphs = 0;
    while index < text.len() {
        let byte = text[index];
        if !escaped {
            match byte {
                b'-' => color = if color == 0x8d { 0x4f } else { 0x8d },
                b'\'' => color = if color == 0x1a { 0x4f } else { 0x1a },
                b'@' => color = if color == 0x17 { 0x4f } else { 0x17 },
                b'"' => color = if color == 0x2d { 0x4f } else { 0x2d },
                b'(' | b')' => {}
                b'$' => {
                    index += (text.len() - index).min(3);
                    continue;
                }
                b'~' | b'\r' | b'\n' => break,
                b'\\' => {
                    escaped = true;
                    index += 1;
                    continue;
                }
                _ => {
                    if glyphs >= glyph_limit {
                        break;
                    }
                    if byte >= 0x80 {
                        let Some(&trail) = text.get(index + 1) else {
                            break;
                        };
                        if let Some(glyph) = font.glyph(u16::from_be_bytes([byte, trail])) {
                            renderer.draw_font_glyph(glyph, cursor_x, y, color);
                        }
                        cursor_x += 16;
                        glyphs += 1;
                        index += 2;
                        continue;
                    }
                    draw_dialog_ascii(renderer, byte, cursor_x, y, color);
                    cursor_x += 8;
                    glyphs += 1;
                }
            }
            if matches!(byte, b'-' | b'\'' | b'@' | b'"' | b'(' | b')') {
                index += 1;
                continue;
            }
        } else {
            if glyphs >= glyph_limit {
                break;
            }
            draw_dialog_ascii(renderer, byte, cursor_x, y, color);
            cursor_x += 8;
            glyphs += 1;
            escaped = false;
        }
        index += 1;
    }
    (cursor_x, glyphs, color)
}

pub(super) fn dialog_color_after(text: &[u8], mut color: u8) -> u8 {
    let mut index = 0;
    let mut escaped = false;
    while index < text.len() {
        let byte = text[index];
        if escaped {
            escaped = false;
            index += 1;
            continue;
        }
        match byte {
            b'-' => color = if color == 0x8d { 0x4f } else { 0x8d },
            b'\'' => color = if color == 0x1a { 0x4f } else { 0x1a },
            b'@' => color = if color == 0x17 { 0x4f } else { 0x17 },
            b'"' => color = if color == 0x2d { 0x4f } else { 0x2d },
            b'$' => {
                index += (text.len() - index).min(3);
                continue;
            }
            b'~' | b'\r' | b'\n' => break,
            b'\\' => escaped = true,
            byte if byte >= 0x80 => {
                index += (text.len() - index).min(2);
                continue;
            }
            _ => {}
        }
        index += 1;
    }
    color
}

pub(super) fn draw_dialog_ascii(
    renderer: &mut Renderer,
    byte: u8,
    x: i32,
    y: i32,
    palette_index: u8,
) {
    if !byte.is_ascii_graphic() {
        return;
    }
    let color = if byte.is_ascii_digit() {
        0x2d
    } else {
        palette_index
    };
    for (row, bits) in glyph(char::from(byte)).iter().enumerate() {
        for column in 0..5 {
            if bits & (0b1_0000 >> column) != 0 {
                put_palette_pixel(renderer, x + column, y + row as i32 + 4, color);
            }
        }
    }
}

pub(super) fn draw_dialog_wait_icon(
    renderer: &mut Renderer,
    icons: &[RleBitmap],
    icon: u8,
    x: i32,
    y: i32,
) {
    if let Some(bitmap) = icons.get(usize::from(icon)) {
        renderer.blit_rle(bitmap, x, y);
        return;
    }
    for (row, (offset, width)) in [(2, 1), (1, 3), (0, 5), (1, 3), (2, 1)]
        .into_iter()
        .enumerate()
    {
        for column in 0..width {
            put_palette_pixel(renderer, x + 2 + offset + column, y + 5 + row as i32, 0xf9);
        }
    }
}

fn put_palette_pixel(renderer: &mut Renderer, x: i32, y: i32, palette_index: u8) {
    let (Ok(x), Ok(y)) = (usize::try_from(x), usize::try_from(y)) else {
        return;
    };
    renderer.put_pixel(x, y, palette_index);
}
