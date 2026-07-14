//! 位图模块
//!
//! 封装 RLE 位图、调色板和索引像素数据的操作。

use crate::palette::Palette;
use crate::rle::RleBitmap;

/// 完全解码的位图（RGBA 格式）
#[derive(Debug, Clone)]
pub struct Bitmap {
    pub width: u16,
    pub height: u16,
    /// RGBA 像素数据（每个像素 4 字节）
    pub rgba: Vec<u8>,
}

impl Bitmap {
    /// 从 RLE 数据 + 调色板解码为 RGBA 位图
    pub fn from_rle(data: &[u8], palette: &Palette) -> Option<Self> {
        let rle = RleBitmap::decode(data)?;
        let rgba = rle.to_rgba(palette);
        Some(Bitmap {
            width: rle.width,
            height: rle.height,
            rgba,
        })
    }

    /// 从索引像素数据 + 调色板创建 RGBA 位图
    pub fn from_indexed(
        pixels: Vec<u8>,
        width: u16,
        height: u16,
        palette: &Palette,
    ) -> Self {
        let rgba = palette.apply_to_pixels(&pixels);
        Bitmap {
            width,
            height,
            rgba,
        }
    }

    /// 将位图绘制到 RGBA 目标缓冲区
    /// 支持透明混合（alpha blending）
    pub fn blit_to(
        &self,
        target: &mut [u8],
        target_width: usize,
        target_height: usize,
        dx: i32,
        dy: i32,
    ) {
        for sy in 0..self.height as i32 {
            let ty = dy + sy;
            if ty < 0 || ty >= target_height as i32 {
                continue;
            }
            for sx in 0..self.width as i32 {
                let tx = dx + sx;
                if tx < 0 || tx >= target_width as i32 {
                    continue;
                }

                let src_idx = (sy * self.width as i32 + sx) as usize * 4;
                let dst_idx = (ty * target_width as i32 + tx) as usize * 4;

                let a = self.rgba[src_idx + 3];
                if a == 0 {
                    continue; // 完全透明
                }

                let r = self.rgba[src_idx];
                let g = self.rgba[src_idx + 1];
                let b = self.rgba[src_idx + 2];

                if a == 255 {
                    // 完全不透明，直接覆盖
                    target[dst_idx] = r;
                    target[dst_idx + 1] = g;
                    target[dst_idx + 2] = b;
                    target[dst_idx + 3] = 255;
                } else {
                    // 半透明，alpha blending
                    let dr = target[dst_idx] as u32 * (255 - a) as u32 / 255;
                    let dg = target[dst_idx + 1] as u32 * (255 - a) as u32 / 255;
                    let db = target[dst_idx + 2] as u32 * (255 - a) as u32 / 255;
                    target[dst_idx] = (r as u32 * a as u32 / 255 + dr) as u8;
                    target[dst_idx + 1] = (g as u32 * a as u32 / 255 + dg) as u8;
                    target[dst_idx + 2] = (b as u32 * a as u32 / 255 + db) as u8;
                    target[dst_idx + 3] = 255;
                }
            }
        }
    }
}
