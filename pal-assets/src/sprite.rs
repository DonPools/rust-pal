//! PAL sprite offset-table parser.

use crate::rle::RleBitmap;

/// A sprite containing raw RLE frames.
#[derive(Debug, Clone)]
pub struct Sprite {
    frames: Vec<Vec<u8>>,
}

impl Sprite {
    /// Parse a GOP/MGO sprite chunk.
    ///
    /// The first word is the first frame's word offset, and is one greater
    /// than the frame count. Some original assets leave the last table word
    /// as padding, so the final frame is bounded by the chunk itself.
    pub fn from_gop_chunk(data: &[u8]) -> Option<Self> {
        let first_offset_words = read_u16(data, 0)? as usize;
        if first_offset_words < 2 {
            return None;
        }

        let frame_count = first_offset_words - 1;
        let table_len = first_offset_words.checked_mul(2)?;
        if table_len > data.len() {
            return None;
        }

        let offsets: Vec<usize> = (0..frame_count)
            .map(|index| (read_u16(data, index * 2)? as usize).checked_mul(2))
            .collect::<Option<_>>()?;
        if offsets[0] != table_len
            || offsets
                .windows(2)
                .any(|pair| pair[0] > pair[1] || pair[1] > data.len())
            || offsets.last().copied()? > data.len()
        {
            return None;
        }

        let ends = offsets.iter().copied().skip(1).chain([data.len()]);
        let frames = offsets
            .iter()
            .copied()
            .zip(ends)
            .map(|(start, end)| data[start..end].to_vec())
            .collect();
        Some(Self { frames })
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    pub fn decode_frame(&self, index: usize) -> Option<RleBitmap> {
        RleBitmap::decode(self.frame_data(index)?)
    }

    pub fn frame_data(&self, index: usize) -> Option<&[u8]> {
        self.frames.get(index).map(Vec::as_slice)
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
    fn parses_frames_with_padding_word() {
        let mut data = Vec::new();
        for offset in [3u16, 5, 0] {
            data.extend_from_slice(&offset.to_le_bytes());
        }
        data.extend_from_slice(b"ABCD");
        data.extend_from_slice(b"EFGH");

        let sprite = Sprite::from_gop_chunk(&data).unwrap();
        assert_eq!(sprite.frame_count(), 2);
        assert_eq!(sprite.frame_data(0), Some(b"ABCD".as_slice()));
        assert_eq!(sprite.frame_data(1), Some(b"EFGH".as_slice()));
    }

    #[test]
    fn rejects_invalid_offsets() {
        assert!(Sprite::from_gop_chunk(&[]).is_none());
        assert!(Sprite::from_gop_chunk(&[1, 0]).is_none());

        let invalid = [3, 0, 2, 0, 0, 0, 0, 0, 0, 0];
        assert!(Sprite::from_gop_chunk(&invalid).is_none());
    }
}
