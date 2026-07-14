//! MKF archive parser.
//!
//! The first little-endian `u32` is both the offset-table size and the
//! absolute start offset of chunk 0. Every adjacent pair of table entries
//! defines one chunk.

/// An in-memory MKF archive.
#[derive(Debug)]
pub struct MkfArchive {
    offsets: Vec<u32>,
    data: Vec<u8>,
}

impl MkfArchive {
    /// Parse a complete MKF file and validate all chunk boundaries.
    pub fn new(data: &[u8]) -> Option<Self> {
        let table_size = u32::from_le_bytes(data.get(..4)?.try_into().ok()?) as usize;
        if table_size < 8 || table_size > data.len() || !table_size.is_multiple_of(4) {
            return None;
        }

        let chunk_count = (table_size - 4) / 4;
        let offsets: Vec<u32> = (0..=chunk_count)
            .map(|index| {
                let start = index * 4;
                u32::from_le_bytes(data[start..start + 4].try_into().unwrap())
            })
            .collect();

        if offsets[0] as usize != table_size
            || offsets
                .windows(2)
                .any(|pair| pair[0] > pair[1] || pair[1] as usize > data.len())
        {
            return None;
        }

        Some(Self {
            offsets,
            data: data.to_vec(),
        })
    }

    /// Return the number of chunks in the archive.
    pub fn chunk_count(&self) -> usize {
        self.offsets.len() - 1
    }

    /// Borrow one zero-based chunk.
    pub fn read_chunk(&self, index: usize) -> Option<&[u8]> {
        if index >= self.chunk_count() {
            return None;
        }
        let start = self.offsets[index] as usize;
        let end = self.offsets[index + 1] as usize;
        Some(&self.data[start..end])
    }

    /// Clone one zero-based chunk.
    pub fn read_chunk_owned(&self, index: usize) -> Option<Vec<u8>> {
        self.read_chunk(index).map(<[u8]>::to_vec)
    }

    /// Return all chunk sizes in bytes.
    pub fn chunk_sizes(&self) -> Vec<usize> {
        self.offsets
            .windows(2)
            .map(|pair| (pair[1] - pair[0]) as usize)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_mkf(chunks: &[&[u8]]) -> Vec<u8> {
        let table_size = (chunks.len() + 1) * 4;
        let mut offsets = Vec::with_capacity(chunks.len() + 1);
        let mut offset = table_size as u32;
        offsets.push(offset);
        for chunk in chunks {
            offset += chunk.len() as u32;
            offsets.push(offset);
        }

        let mut data = Vec::new();
        for offset in offsets {
            data.extend_from_slice(&offset.to_le_bytes());
        }
        for chunk in chunks {
            data.extend_from_slice(chunk);
        }
        data
    }

    #[test]
    fn reads_chunks_from_adjacent_boundaries() {
        let data = make_mkf(&[b"first", b"", b"second"]);
        let archive = MkfArchive::new(&data).unwrap();
        assert_eq!(archive.chunk_count(), 3);
        assert_eq!(archive.chunk_sizes(), [5, 0, 6]);
        assert_eq!(archive.read_chunk(0), Some(b"first".as_slice()));
        assert_eq!(archive.read_chunk(1), Some(b"".as_slice()));
        assert_eq!(archive.read_chunk(2), Some(b"second".as_slice()));
        assert!(archive.read_chunk(3).is_none());
    }

    #[test]
    fn rejects_invalid_tables() {
        assert!(MkfArchive::new(&[]).is_none());
        assert!(MkfArchive::new(&[0; 8]).is_none());

        let non_monotonic = [12, 0, 0, 0, 11, 0, 0, 0, 10, 0, 0, 0];
        assert!(MkfArchive::new(&non_monotonic).is_none());

        let past_end = [8, 0, 0, 0, 20, 0, 0, 0];
        assert!(MkfArchive::new(&past_end).is_none());
    }
}
