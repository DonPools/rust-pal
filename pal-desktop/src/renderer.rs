//! 256 色 palette 渲染管线
//!
//! 管理像素缓冲区，支持将 RLE 位图按照 256 色调色板渲染到屏幕。

use pal_assets::bitmap::Bitmap;
use pal_assets::palette::Palette;
use pal_assets::rle::RleBitmap;
use pal_assets::text::{BitmapFont, FontGlyph};

/// 渲染管线状态
pub struct Renderer {
    /// 调色板
    palette: Palette,
    /// 屏幕缓冲区（RGBA）
    screen: Vec<u8>,
    /// 显示宽度
    pub width: usize,
    /// 显示高度
    pub height: usize,
    /// 脏标记
    dirty: bool,
}

impl Renderer {
    /// 创建新的渲染器
    pub fn new(palette: Palette, width: usize, height: usize) -> Self {
        let screen = vec![0u8; width * height * 4];
        Renderer {
            palette,
            screen,
            width,
            height,
            dirty: true,
        }
    }

    /// 获取屏幕缓冲区引用
    pub fn screen(&self) -> &[u8] {
        &self.screen
    }

    /// 获取可变屏幕缓冲区引用
    pub fn screen_mut(&mut self) -> &mut [u8] {
        &mut self.screen
    }

    /// 检查是否有更新需要渲染
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// 标记已渲染
    pub fn mark_cleaned(&mut self) {
        self.dirty = false;
    }

    /// 设置调色板
    pub fn set_palette(&mut self, palette: &Palette) {
        self.palette = palette.clone();
        self.dirty = true;
    }

    /// Replace the framebuffer with an already converted RGBA screen.
    pub fn replace_screen(&mut self, rgba: &[u8]) -> bool {
        if rgba.len() != self.screen.len() {
            return false;
        }
        self.screen.copy_from_slice(rgba);
        self.dirty = true;
        true
    }

    /// Draw an opaque indexed-color screen using the active palette.
    pub fn replace_with_indexed(&mut self, indices: &[u8]) -> bool {
        if indices.len() != self.width * self.height {
            return false;
        }
        for (&index, output) in indices.iter().zip(self.screen.chunks_exact_mut(4)) {
            let (r, g, b) = self.palette.get_rgb(index);
            output.copy_from_slice(&[r, g, b, 255]);
        }
        self.dirty = true;
        true
    }

    /// Blend from a previous RGBA screen, where `progress` is in `0..=64`.
    pub fn blend_from(&mut self, previous: &[u8], progress: u8) -> bool {
        if previous.len() != self.screen.len() || progress > 64 {
            return false;
        }
        let current_weight = u16::from(progress);
        let previous_weight = 64 - current_weight;
        for (current, &old) in self.screen.iter_mut().zip(previous) {
            *current = ((u16::from(*current) * current_weight + u16::from(old) * previous_weight)
                / 64) as u8;
        }
        self.dirty = true;
        true
    }

    /// Reveal the current screen from top to bottom over a previous RGBA screen.
    pub fn reveal_from_top(&mut self, previous: &[u8], rows: usize) -> bool {
        if previous.len() != self.screen.len() {
            return false;
        }
        let keep_from = rows.min(self.height) * self.width * 4;
        self.screen[keep_from..].copy_from_slice(&previous[keep_from..]);
        self.dirty = true;
        true
    }

    pub fn apply_brightness(&mut self, brightness: u8) {
        let brightness = u16::from(brightness.min(64));
        for pixel in self.screen.chunks_exact_mut(4) {
            for channel in &mut pixel[..3] {
                *channel = (u16::from(*channel) * brightness / 64) as u8;
            }
        }
        self.dirty = true;
    }

    pub fn apply_color_tint(&mut self, color: (u8, u8, u8), amount: u8) {
        let amount = u16::from(amount.min(64));
        let source = 64 - amount;
        for pixel in self.screen.chunks_exact_mut(4) {
            pixel[0] = ((u16::from(pixel[0]) * source + u16::from(color.0) * amount) / 64) as u8;
            pixel[1] = ((u16::from(pixel[1]) * source + u16::from(color.1) * amount) / 64) as u8;
            pixel[2] = ((u16::from(pixel[2]) * source + u16::from(color.2) * amount) / 64) as u8;
        }
        self.dirty = true;
    }

    /// Apply PAL-style horizontal row displacement to the completed frame.
    pub fn apply_wave(&mut self, level: u16, progression: i16) {
        if level == 0 || self.width == 0 {
            return;
        }
        let source = self.screen.clone();
        for y in 0..self.height {
            let phase = (i32::try_from(y).unwrap_or(i32::MAX) + i32::from(progression)) & 31;
            let triangle = if phase < 16 { phase } else { 31 - phase } - 8;
            let offset = triangle * i32::from(level) / 8;
            for x in 0..self.width {
                let source_x = (i32::try_from(x).unwrap_or(i32::MAX) - offset)
                    .rem_euclid(self.width as i32) as usize;
                let destination = (y * self.width + x) * 4;
                let source_index = (y * self.width + source_x) * 4;
                self.screen[destination..destination + 4]
                    .copy_from_slice(&source[source_index..source_index + 4]);
            }
        }
        self.dirty = true;
    }

    pub fn apply_shake(&mut self, offset_x: i32, offset_y: i32) {
        if offset_x == 0 && offset_y == 0 {
            return;
        }
        let source = self.screen.clone();
        self.clear_black();
        for y in 0..self.height {
            for x in 0..self.width {
                let target_x = i32::try_from(x).unwrap_or(i32::MAX) + offset_x;
                let target_y = i32::try_from(y).unwrap_or(i32::MAX) + offset_y;
                let (Ok(target_x), Ok(target_y)) =
                    (usize::try_from(target_x), usize::try_from(target_y))
                else {
                    continue;
                };
                if target_x >= self.width || target_y >= self.height {
                    continue;
                }
                let source_index = (y * self.width + x) * 4;
                let destination = (target_y * self.width + target_x) * 4;
                self.screen[destination..destination + 4]
                    .copy_from_slice(&source[source_index..source_index + 4]);
            }
        }
        self.dirty = true;
    }

    /// 用纯色清除屏幕
    pub fn clear(&mut self, r: u8, g: u8, b: u8) {
        for pixel in self.screen.chunks_exact_mut(4) {
            pixel[0] = r;
            pixel[1] = g;
            pixel[2] = b;
            pixel[3] = 255;
        }
        self.dirty = true;
    }

    /// 在指定位置绘制 RGBA 位图
    pub fn blit_bitmap(&mut self, bitmap: &Bitmap, dx: i32, dy: i32) {
        bitmap.blit_to(&mut self.screen, self.width, self.height, dx, dy);
        self.dirty = true;
    }

    /// 解码并绘制 RLE 位图
    pub fn blit_rle(&mut self, rle: &RleBitmap, dx: i32, dy: i32) {
        let bitmap = Bitmap::from_decoded_rle(rle, &self.palette);
        self.blit_bitmap(&bitmap, dx, dy);
    }

    /// Darken destination pixels under an RLE mask using PAL's palette-index rule.
    pub fn blit_rle_shadow(&mut self, rle: &RleBitmap, dx: i32, dy: i32) {
        for sy in 0..i32::from(rle.height) {
            let y = dy + sy;
            if y < 0 || y >= self.height as i32 {
                continue;
            }
            for sx in 0..i32::from(rle.width) {
                let source = sy as usize * usize::from(rle.width) + sx as usize;
                if !rle.opaque[source] {
                    continue;
                }
                let x = dx + sx;
                if x < 0 || x >= self.width as i32 {
                    continue;
                }
                let destination = (y as usize * self.width + x as usize) * 4;
                let rgb = (
                    self.screen[destination],
                    self.screen[destination + 1],
                    self.screen[destination + 2],
                );
                let Some(index) = (0u8..=u8::MAX).find(|&index| self.palette.get_rgb(index) == rgb)
                else {
                    continue;
                };
                let shadow = (index & 0xf0) | ((index & 0x0f) >> 1);
                let (r, g, b) = self.palette.get_rgb(shadow);
                self.screen[destination..destination + 4].copy_from_slice(&[r, g, b, 255]);
            }
        }
        self.dirty = true;
    }

    /// 用索引色值绘制单个像素
    pub fn put_pixel(&mut self, x: usize, y: usize, palette_index: u8) {
        if x >= self.width || y >= self.height {
            return;
        }
        let (r, g, b) = self.palette.get_rgb(palette_index);
        let idx = (y * self.width + x) * 4;
        let a = if palette_index == 0 { 0 } else { 255 };
        self.screen[idx] = r;
        self.screen[idx + 1] = g;
        self.screen[idx + 2] = b;
        self.screen[idx + 3] = a;
        self.dirty = true;
    }

    /// Draw one debug pixel in a fixed RGBA color, clipped to the framebuffer.
    pub fn put_rgba(&mut self, x: i32, y: i32, color: [u8; 4]) {
        let (Ok(x), Ok(y)) = (usize::try_from(x), usize::try_from(y)) else {
            return;
        };
        if x >= self.width || y >= self.height {
            return;
        }
        let index = (y * self.width + x) * 4;
        self.screen[index..index + 4].copy_from_slice(&color);
        self.dirty = true;
    }

    /// Draw one 16x15 monochrome PAL font glyph in a palette color.
    pub fn draw_font_glyph(&mut self, glyph: &FontGlyph, x: i32, y: i32, palette_index: u8) {
        let (r, g, b) = self.palette.get_rgb(palette_index);
        for (row, bytes) in glyph.rows.iter().enumerate() {
            for column in 0..16 {
                if bytes[column / 8] & (0x80 >> (column % 8)) != 0 {
                    self.put_rgba(x + column as i32, y + row as i32, [r, g, b, 255]);
                }
            }
        }
    }

    /// Draw Big5-encoded text using the original 16x15 PAL bitmap font.
    pub fn draw_big5_text(
        &mut self,
        font: &BitmapFont,
        text: &[u8],
        x: i32,
        y: i32,
        palette_index: u8,
    ) {
        let mut cursor_x = x;
        let mut index = 0;
        while index < text.len() {
            if text[index] < 0x80 {
                cursor_x += 8;
                index += 1;
                continue;
            }
            let Some(trail) = text.get(index + 1) else {
                break;
            };
            let code = u16::from_be_bytes([text[index], *trail]);
            if let Some(glyph) = font.glyph(code) {
                self.draw_font_glyph(glyph, cursor_x, y, palette_index);
            }
            cursor_x += 16;
            index += 2;
        }
    }

    /// Draw PAL text with the three one-pixel shadow layers used by the DOS UI.
    pub fn draw_big5_text_shadowed(
        &mut self,
        font: &BitmapFont,
        text: &[u8],
        x: i32,
        y: i32,
        palette_index: u8,
    ) {
        self.draw_big5_text(font, text, x + 1, y, 0);
        self.draw_big5_text(font, text, x, y + 1, 0);
        self.draw_big5_text(font, text, x + 1, y + 1, 0);
        self.draw_big5_text(font, text, x, y, palette_index);
    }

    /// 清屏为黑色
    pub fn clear_black(&mut self) {
        self.clear(0, 0, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pal_assets::palette::PaletteColor;

    fn font_with_left_pixels() -> BitmapFont {
        let mut data = vec![0; 0x682 + 30];
        data[0x682] = 0xc0;
        BitmapFont::parse(&[0xb8, 0x67], &data).unwrap()
    }

    fn renderer() -> Renderer {
        let mut palette = Palette::default();
        palette.colors[1] = PaletteColor { r: 63, g: 0, b: 0 };
        Renderer::new(palette, 24, 15)
    }

    #[test]
    fn big5_text_clips_negative_coordinates_and_advances_past_ascii() {
        let font = font_with_left_pixels();
        let mut renderer = renderer();

        renderer.draw_big5_text(&font, &[0xb8, 0x67], -1, 0, 1);
        assert_eq!(&renderer.screen()[0..4], &[252, 0, 0, 255]);

        renderer.clear_black();
        renderer.draw_big5_text(&font, &[b'A', 0xb8, 0x67], 0, 0, 1);
        assert_eq!(&renderer.screen()[8 * 4..9 * 4], &[252, 0, 0, 255]);
        assert_eq!(&renderer.screen()[9 * 4..10 * 4], &[252, 0, 0, 255]);

        renderer.draw_big5_text(&font, &[0xb8], 0, 0, 1);
    }
}
