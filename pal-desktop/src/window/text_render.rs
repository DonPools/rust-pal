use pal_assets::rle::RleBitmap;
use pal_assets::text::BitmapFont;

use crate::debug_overlay::glyph;
use crate::renderer::Renderer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DialogTextMode {
    Normal,
    CenterWindow,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct DialogTextStyle {
    pub(super) color: u8,
    pub(super) glyph_limit: usize,
    pub(super) mode: DialogTextMode,
}

pub(super) fn draw_dialog_text(
    renderer: &mut Renderer,
    font: &BitmapFont,
    ui_sprites: &[RleBitmap],
    text: &[u8],
    x: i32,
    y: i32,
    style: DialogTextStyle,
) -> (i32, usize, u8) {
    let mut cursor_x = x;
    let mut color = style.color;
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
                b'"' if style.mode == DialogTextMode::Normal => {
                    color = if color == 0x2d { 0x4f } else { 0x2d };
                }
                b'"' => {}
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
                    if glyphs >= style.glyph_limit {
                        break;
                    }
                    if byte >= 0x80 {
                        let Some(&trail) = text.get(index + 1) else {
                            break;
                        };
                        if let Some(glyph) = font.glyph(u16::from_be_bytes([byte, trail])) {
                            draw_dialog_glyph(renderer, glyph, cursor_x, y, color, style.mode);
                        }
                        cursor_x += 16;
                        glyphs += 1;
                        index += 2;
                        continue;
                    }
                    draw_dialog_ascii(renderer, ui_sprites, byte, cursor_x, y, color, style.mode);
                    cursor_x += 8;
                    glyphs += 1;
                }
            }
            if matches!(byte, b'-' | b'\'' | b'@' | b'"' | b'(' | b')') {
                index += 1;
                continue;
            }
        } else {
            if glyphs >= style.glyph_limit {
                break;
            }
            draw_dialog_ascii(renderer, ui_sprites, byte, cursor_x, y, color, style.mode);
            cursor_x += 8;
            glyphs += 1;
            escaped = false;
        }
        index += 1;
    }
    (cursor_x, glyphs, color)
}

pub(super) fn dialog_color_after(text: &[u8], mut color: u8, mode: DialogTextMode) -> u8 {
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
            b'"' if mode == DialogTextMode::Normal => {
                color = if color == 0x2d { 0x4f } else { 0x2d };
            }
            b'"' => {}
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
    ui_sprites: &[RleBitmap],
    byte: u8,
    x: i32,
    y: i32,
    palette_index: u8,
    mode: DialogTextMode,
) {
    if !byte.is_ascii_graphic() {
        return;
    }
    if mode == DialogTextMode::CenterWindow && byte.is_ascii_digit() {
        if let Some(digit) = ui_sprites.get(19 + usize::from(byte - b'0')) {
            renderer.blit_rle(digit, x, y + 4);
            return;
        }
    }
    let color = dialog_glyph_color(palette_index, mode);
    if mode == DialogTextMode::Normal {
        draw_ascii_pixels(renderer, byte, x + 1, y, 0);
        draw_ascii_pixels(renderer, byte, x, y + 1, 0);
        draw_ascii_pixels(renderer, byte, x + 1, y + 1, 0);
    }
    draw_ascii_pixels(renderer, byte, x, y, color);
}

fn draw_dialog_glyph(
    renderer: &mut Renderer,
    glyph: &pal_assets::text::FontGlyph,
    x: i32,
    y: i32,
    palette_index: u8,
    mode: DialogTextMode,
) {
    if mode == DialogTextMode::Normal {
        renderer.draw_font_glyph(glyph, x + 1, y, 0);
        renderer.draw_font_glyph(glyph, x, y + 1, 0);
        renderer.draw_font_glyph(glyph, x + 1, y + 1, 0);
    }
    renderer.draw_font_glyph(glyph, x, y, dialog_glyph_color(palette_index, mode));
}

fn dialog_glyph_color(palette_index: u8, mode: DialogTextMode) -> u8 {
    if mode == DialogTextMode::CenterWindow && palette_index == 0x4f {
        0
    } else {
        palette_index
    }
}

fn draw_ascii_pixels(renderer: &mut Renderer, byte: u8, x: i32, y: i32, color: u8) {
    const SOURCE_WIDTH: usize = 5;
    const SOURCE_HEIGHT: usize = 7;
    const DRAW_WIDTH: usize = 7;
    const DRAW_HEIGHT: usize = 11;
    let source = glyph(char::from(byte));
    for row in 0..DRAW_HEIGHT {
        let source_row = row * SOURCE_HEIGHT / DRAW_HEIGHT;
        let bits = source[source_row];
        for column in 0..DRAW_WIDTH {
            let source_column = column * SOURCE_WIDTH / DRAW_WIDTH;
            if bits & (0b1_0000 >> source_column) != 0 {
                put_palette_pixel(renderer, x + column as i32, y + row as i32 + 2, color);
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
    renderer.put_opaque_pixel(x, y, palette_index);
}

#[cfg(test)]
mod tests {
    use super::*;
    use pal_assets::palette::{Palette, PaletteColor};

    fn one_pixel_font() -> BitmapFont {
        let mut data = vec![0; 0x682 + 30];
        data[0x682] = 0x80;
        BitmapFont::parse(&[0xb8, 0x67], &data).unwrap()
    }

    fn pixel(renderer: &Renderer, x: usize, y: usize) -> &[u8] {
        let offset = (y * renderer.width + x) * 4;
        &renderer.screen()[offset..offset + 4]
    }

    #[test]
    fn normal_dialog_glyphs_use_the_original_three_pixel_shadow() {
        let mut palette = Palette::default();
        palette.colors[0x4f] = PaletteColor {
            r: 63,
            g: 63,
            b: 63,
        };
        let mut renderer = Renderer::new(palette, 3, 3);
        renderer.clear(40, 50, 60);

        draw_dialog_text(
            &mut renderer,
            &one_pixel_font(),
            &[],
            &[0xb8, 0x67],
            0,
            0,
            DialogTextStyle {
                color: 0x4f,
                glyph_limit: 1,
                mode: DialogTextMode::Normal,
            },
        );

        assert_eq!(pixel(&renderer, 0, 0), [252, 252, 252, 255]);
        assert_eq!(pixel(&renderer, 1, 0), [0, 0, 0, 255]);
        assert_eq!(pixel(&renderer, 0, 1), [0, 0, 0, 255]);
        assert_eq!(pixel(&renderer, 1, 1), [0, 0, 0, 255]);
    }

    #[test]
    fn center_window_text_uses_black_without_a_shadow() {
        let mut renderer = Renderer::new(Palette::default(), 3, 3);
        renderer.clear(40, 50, 60);

        draw_dialog_text(
            &mut renderer,
            &one_pixel_font(),
            &[],
            &[0xb8, 0x67],
            0,
            0,
            DialogTextStyle {
                color: 0x4f,
                glyph_limit: 1,
                mode: DialogTextMode::CenterWindow,
            },
        );

        assert_eq!(pixel(&renderer, 0, 0), [0, 0, 0, 255]);
        assert_eq!(pixel(&renderer, 1, 0), [40, 50, 60, 255]);
    }

    #[test]
    fn ascii_dialog_glyphs_fill_an_eight_by_fifteen_cell() {
        let mut renderer = Renderer::new(Palette::default(), 8, 15);
        renderer.clear(40, 50, 60);

        draw_dialog_ascii(
            &mut renderer,
            &[],
            b'E',
            0,
            0,
            0x4f,
            DialogTextMode::CenterWindow,
        );

        assert_eq!(pixel(&renderer, 6, 2), [0, 0, 0, 255]);
        assert_eq!(pixel(&renderer, 7, 2), [40, 50, 60, 255]);
        assert_eq!(pixel(&renderer, 6, 12), [0, 0, 0, 255]);
    }
}
