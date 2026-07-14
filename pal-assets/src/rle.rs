//! RLE 位图解码器
//!
//! 仙剑的角色精灵、物品图标、瓦片贴图使用 RLE 压缩格式存储。
//! 这是 8-bpp（8 位每像素，256 色）的位图，用索引色值指向调色板。

use crate::palette::Palette;

/// RLE 压缩的位图
#[derive(Debug, Clone)]
pub struct RleBitmap {
    /// 位图宽度（像素）
    pub width: u16,
    /// 位图高度（像素）
    pub height: u16,
    /// 解码后的像素数据（每个像素 1 字节索引色值）
    pub pixels: Vec<u8>,
}

impl RleBitmap {
    /// 从原始 RLE 数据解码
    ///
    /// # 参数
    /// * `data` - RLE 压缩的原始字节数据（可能以 0x00000002 magic 开头）
    ///
    /// # 返回
    /// * `Some(RleBitmap)` - 解码成功
    /// * `None` - 数据无效
    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() < 4 {
            return None;
        }

        let mut offset = 0;

        // 跳过可选的 0x00000002 magic 头
        if data.len() >= 4 && data[0] == 0x02 && data[1] == 0x00
            && data[2] == 0x00 && data[3] == 0x00
        {
            offset = 4;
        }

        // 剩余长度不足以读取宽高
        if data.len() < offset + 4 {
            return None;
        }

        let width = u16::from_le_bytes([data[offset], data[offset + 1]]);
        let height = u16::from_le_bytes([data[offset + 2], data[offset + 3]]);
        offset += 4;

        if width == 0 || height == 0 {
            return None;
        }

        let total_pixels = width as usize * height as usize;
        let mut pixels = Vec::with_capacity(total_pixels);

        // 初始化为透明色（索引 0）
        pixels.resize(total_pixels, 0);

        let mut src_x: usize = 0;
        let mut dst_i: usize = 0;

        while dst_i < total_pixels && offset < data.len() {
            let cmd = data[offset];
            offset += 1;

            if cmd > 0x80 && cmd <= 0x80 + (width as u8) {
                // 透明跳过命令：跳过 (cmd - 0x80) 个像素
                let skip = (cmd - 0x80) as usize;
                dst_i += skip;
                src_x += skip;
                if src_x >= width as usize {
                    src_x -= width as usize;
                    // y 增加，但 dst_i 已经包含这个变化
                }
            } else {
                // 绘制命令：读取 cmd 个像素字节
                let count = cmd as usize;
                if offset + count > data.len() {
                    break;
                }

                for _ in 0..count {
                    if dst_i < total_pixels {
                        pixels[dst_i] = data[offset];
                    }
                    offset += 1;
                    dst_i += 1;
                    src_x += 1;
                    if src_x >= width as usize {
                        src_x = 0;
                    }
                }
            }
        }

        Some(RleBitmap {
            width,
            height,
            pixels,
        })
    }

    /// 将索引像素数据转换为 RGBA 缓冲区
    pub fn to_rgba(&self, palette: &Palette) -> Vec<u8> {
        let mut rgba = Vec::with_capacity(self.pixels.len() * 4);
        for &index in &self.pixels {
            if index == 0 {
                // 透明色
                rgba.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                let (r, g, b) = palette.get_rgb(index);
                rgba.extend_from_slice(&[r, g, b, 255]);
            }
        }
        rgba
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::Palette;

    #[test]
    fn test_invalid_data() {
        assert!(RleBitmap::decode(&[]).is_none());
        assert!(RleBitmap::decode(&[0; 3]).is_none());
    }

    #[test]
    fn test_simple_rle() {
        // 2x2 像素：绘制 4 个像素（全是非透明）
        let data = &[
            0x02, 0x00, 0x00, 0x00, // magic
            0x02, 0x00, // width = 2
            0x02, 0x00, // height = 2
            0x04,       // 绘制 4 个像素
            0x01, 0x02, 0x03, 0x04, // 像素值
        ];
        let bmp = RleBitmap::decode(data).unwrap();
        assert_eq!(bmp.width, 2);
        assert_eq!(bmp.height, 2);
        assert_eq!(bmp.pixels, vec![0x01, 0x02, 0x03, 0x04]);
    }

    #[test]
    fn test_rle_with_skip() {
        // 3x1 像素：绘制 1 个，跳过 1 个，绘制 1 个
        let data = &[
            0x03, 0x00, // width = 3
            0x01, 0x00, // height = 1
            0x01, 0xAA, // 绘制 1 个像素 AA
            0x82,       // 跳过 2 个像素（但后面只剩 1 个位置）
        ];
        let bmp = RleBitmap::decode(data).unwrap();
        assert_eq!(bmp.width, 3);
        assert_eq!(bmp.height, 1);
        assert_eq!(bmp.pixels[0], 0xAA);
        assert_eq!(bmp.pixels[1], 0);
        assert_eq!(bmp.pixels[2], 0);
    }
}
