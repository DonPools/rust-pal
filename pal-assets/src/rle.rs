//! Decoder for PAL's 8-bit indexed RLE bitmaps.

use crate::palette::Palette;

#[derive(Debug, Clone)]
pub struct RleBitmap {
    pub width: u16,
    pub height: u16,
    /// Decoded indexed pixels. Index 0 represents transparent pixels.
    pub pixels: Vec<u8>,
}

impl RleBitmap {
    /// Decode one complete bitmap. Truncated or overflowing commands fail.
    pub fn decode(data: &[u8]) -> Option<Self> {
        let mut offset = if data.get(..4) == Some([0x02, 0, 0, 0].as_slice()) {
            4
        } else {
            0
        };

        let width = read_u16(data, offset)?;
        let height = read_u16(data, offset + 2)?;
        offset += 4;
        if width == 0 || height == 0 {
            return None;
        }

        let total_pixels = (width as usize).checked_mul(height as usize)?;
        let mut pixels = vec![0; total_pixels];
        let mut destination = 0usize;

        while destination < total_pixels {
            let command = *data.get(offset)?;
            offset += 1;

            if command & 0x80 != 0 && command as usize <= 0x80 + width as usize {
                destination = destination.checked_add((command - 0x80) as usize)?;
                if destination > total_pixels {
                    return None;
                }
            } else {
                let count = command as usize;
                let source_end = offset.checked_add(count)?;
                let destination_end = destination.checked_add(count)?;
                if destination_end > total_pixels {
                    return None;
                }
                pixels[destination..destination_end].copy_from_slice(data.get(offset..source_end)?);
                offset = source_end;
                destination = destination_end;
            }
        }

        Some(Self {
            width,
            height,
            pixels,
        })
    }

    pub fn to_rgba(&self, palette: &Palette) -> Vec<u8> {
        palette.apply_to_pixels(&self.pixels)
    }
}

fn read_u16(data: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        data.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_literals_and_transparency() {
        let data = [
            3, 0, 2, 0, // 3x2
            2, 1, 2,    // two literal pixels
            0x82, // two transparent pixels
            2, 3, 4, // two literal pixels
        ];
        let bitmap = RleBitmap::decode(&data).unwrap();
        assert_eq!(bitmap.pixels, [1, 2, 0, 0, 3, 4]);
    }

    #[test]
    fn skips_optional_magic() {
        let data = [2, 0, 0, 0, 2, 0, 1, 0, 2, 7, 8];
        let bitmap = RleBitmap::decode(&data).unwrap();
        assert_eq!((bitmap.width, bitmap.height), (2, 1));
        assert_eq!(bitmap.pixels, [7, 8]);
    }

    #[test]
    fn rejects_invalid_data() {
        assert!(RleBitmap::decode(&[]).is_none());
        assert!(RleBitmap::decode(&[0; 4]).is_none());
        assert!(RleBitmap::decode(&[1, 0, 1, 0, 1]).is_none());
        assert!(RleBitmap::decode(&[1, 0, 1, 0, 2, 1, 2]).is_none());
        assert!(RleBitmap::decode(&[1, 0, 1, 0, 0x82]).is_none());
    }
}
