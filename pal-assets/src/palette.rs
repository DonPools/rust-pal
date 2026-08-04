//! 调色板解析器
//!
//! 仙剑使用 8 位色（8-bpp）渲染，每个像素是 1 字节的索引值，
//! 指向一个包含 256 种颜色的调色板。

/// 一个 RGB 颜色（6 位精度，范围 0-63）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaletteColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl PaletteColor {
    /// 转换为 8 位 RGB（左移 2 位）
    pub fn to_rgb8(&self) -> (u8, u8, u8) {
        (self.r << 2, self.g << 2, self.b << 2)
    }
}

/// 256 色调色板
#[derive(Debug, Clone)]
pub struct Palette {
    pub colors: [PaletteColor; 256],
}

/// Day palette and the optional second 768-byte night palette in one PAT chunk.
#[derive(Debug, Clone)]
pub struct PaletteSet {
    pub day: Palette,
    pub night: Option<Palette>,
}

impl Default for Palette {
    fn default() -> Self {
        Palette {
            colors: [PaletteColor { r: 0, g: 0, b: 0 }; 256],
        }
    }
}

impl Palette {
    /// 从 768 字节的原始数据加载调色板
    ///
    /// 每个颜色 3 字节 (R, G, B)，范围 0-63，共 256 色
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 768 {
            return None;
        }

        let mut colors = [PaletteColor { r: 0, g: 0, b: 0 }; 256];
        for i in 0..256 {
            colors[i] = PaletteColor {
                r: data[i * 3] & 0x3F,
                g: data[i * 3 + 1] & 0x3F,
                b: data[i * 3 + 2] & 0x3F,
            };
        }

        Some(Palette { colors })
    }

    /// 获取某个索引的 8 位 RGB 值
    pub fn get_rgb(&self, index: u8) -> (u8, u8, u8) {
        self.colors[index as usize].to_rgb8()
    }

    /// 获取某个索引的 RGBA 值（alpha = 255，索引 0 为透明）
    pub fn get_rgba(&self, index: u8) -> [u8; 4] {
        if index == 0 {
            return [0, 0, 0, 0];
        }
        let (r, g, b) = self.get_rgb(index);
        [r, g, b, 255]
    }

    /// 将 8 位索引像素缓冲区转换为 RGBA 缓冲区
    pub fn apply_to_pixels(&self, pixels: &[u8]) -> Vec<u8> {
        let mut rgba = Vec::with_capacity(pixels.len() * 4);
        for &index in pixels {
            rgba.extend_from_slice(&self.get_rgba(index));
        }
        rgba
    }
}

impl PaletteSet {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        let day = Palette::from_bytes(data)?;
        let night = if data.len() > 768 {
            Some(Palette::from_bytes(data.get(768..)?)?)
        } else {
            None
        };
        Some(Self { day, night })
    }

    pub fn select(&self, night: bool) -> &Palette {
        if night {
            self.night.as_ref().unwrap_or(&self.day)
        } else {
            &self.day
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_bytes() {
        let mut data = vec![0u8; 768];
        // 颜色 0: 黑色 (透明)
        // 颜色 1: R=63, G=0, B=0 → 红色
        data[3] = 63;
        data[4] = 0;
        data[5] = 0;

        let palette = Palette::from_bytes(&data).unwrap();
        assert_eq!(palette.colors[0], PaletteColor { r: 0, g: 0, b: 0 });
        assert_eq!(palette.colors[1], PaletteColor { r: 63, g: 0, b: 0 });
        assert_eq!(palette.get_rgb(1), (252, 0, 0));
    }

    #[test]
    fn test_invalid_length() {
        assert!(Palette::from_bytes(&[]).is_none());
        assert!(Palette::from_bytes(&[0; 767]).is_none());
    }

    #[test]
    fn test_index_zero_transparent() {
        let data = vec![0u8; 768];
        let palette = Palette::from_bytes(&data).unwrap();
        assert_eq!(palette.get_rgba(0), [0, 0, 0, 0]);
        assert_eq!(palette.get_rgba(1), [0, 0, 0, 255]);
    }

    #[test]
    fn palette_set_selects_night_or_falls_back_to_day() {
        let mut paired = vec![0; 1536];
        paired[3] = 1;
        paired[768 + 3] = 2;
        let set = PaletteSet::from_bytes(&paired).unwrap();
        assert_eq!(set.select(false).colors[1].r, 1);
        assert_eq!(set.select(true).colors[1].r, 2);

        let single = PaletteSet::from_bytes(&paired[..768]).unwrap();
        assert!(single.night.is_none());
        assert_eq!(single.select(true).colors[1].r, 1);
        assert!(PaletteSet::from_bytes(&paired[..767]).is_none());
    }
}
