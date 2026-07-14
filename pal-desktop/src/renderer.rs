//! 256 色 palette 渲染管线
//!
//! 管理像素缓冲区，支持将 RLE 位图按照 256 色调色板渲染到屏幕。

use pal_assets::bitmap::Bitmap;
use pal_assets::palette::Palette;
use pal_assets::rle::RleBitmap;

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
        let bitmap = Bitmap::from_indexed(rle.pixels.clone(), rle.width, rle.height, &self.palette);
        self.blit_bitmap(&bitmap, dx, dy);
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

    /// 清屏为黑色
    pub fn clear_black(&mut self) {
        self.clear(0, 0, 0);
    }
}
