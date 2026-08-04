//! Full-screen indexed pictures stored in `FBP.MKF`.

use crate::{mkf::MkfArchive, yj1};

pub const FBP_WIDTH: usize = 320;
pub const FBP_HEIGHT: usize = 200;
pub const FBP_PIXELS: usize = FBP_WIDTH * FBP_HEIGHT;

#[derive(Debug)]
pub struct FbpArchive {
    archive: MkfArchive,
}

impl FbpArchive {
    pub fn new(data: &[u8]) -> Option<Self> {
        Some(Self {
            archive: MkfArchive::new(data)?,
        })
    }

    pub fn len(&self) -> usize {
        self.archive.chunk_count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn raw_frame(&self, index: usize) -> Option<&[u8]> {
        self.archive.read_chunk(index)
    }

    /// Decode one zero-based picture to exactly 320×200 palette indices.
    pub fn frame(&self, index: usize) -> Option<Vec<u8>> {
        let chunk = self.raw_frame(index)?;
        if chunk.len() == FBP_PIXELS {
            return Some(chunk.to_vec());
        }
        let pixels = yj1::decompress(chunk)?;
        (pixels.len() == FBP_PIXELS).then_some(pixels)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mkf(chunks: &[Vec<u8>]) -> Vec<u8> {
        let table_size = (chunks.len() + 1) * 4;
        let mut offset = table_size as u32;
        let mut data = Vec::new();
        data.extend_from_slice(&offset.to_le_bytes());
        for chunk in chunks {
            offset += chunk.len() as u32;
            data.extend_from_slice(&offset.to_le_bytes());
        }
        for chunk in chunks {
            data.extend_from_slice(chunk);
        }
        data
    }

    fn raw_yj1(data: &[u8]) -> Vec<u8> {
        let mut result = Vec::new();
        result.extend_from_slice(b"YJ_1");
        result.extend_from_slice(&(data.len() as u32).to_le_bytes());
        result.extend_from_slice(&(20u32 + data.len() as u32).to_le_bytes());
        result.extend_from_slice(&1u16.to_le_bytes());
        result.extend_from_slice(&[0, 0]);
        result.extend_from_slice(&(data.len() as u16).to_le_bytes());
        result.extend_from_slice(&0u16.to_le_bytes());
        result.extend_from_slice(data);
        result
    }

    #[test]
    fn reads_raw_and_yj1_pictures() {
        let raw = vec![7; FBP_PIXELS];
        let archive = FbpArchive::new(&mkf(&[raw.clone(), raw_yj1(&raw)])).unwrap();
        assert_eq!(archive.len(), 2);
        assert_eq!(archive.frame(0), Some(raw.clone()));
        assert_eq!(archive.frame(1), Some(raw));
        assert!(archive.frame(2).is_none());
    }

    #[test]
    fn rejects_empty_truncated_and_wrong_sized_pictures() {
        let archive = FbpArchive::new(&mkf(&[Vec::new(), vec![1, 2], raw_yj1(&[3])])).unwrap();
        assert!(archive.frame(0).is_none());
        assert!(archive.frame(1).is_none());
        assert!(archive.frame(2).is_none());
        assert!(FbpArchive::new(&[]).is_none());
    }
}
