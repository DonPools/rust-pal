//! MKF 文件格式解析器
//!
//! MKF 是仙剑奇侠传系列使用的基础包格式，所有游戏资源
//!（图片、地图、音频、文本等）都以 chunk 形式打包在 MKF 文件中。
//!
//! ## 格式说明
//!
//! ```text
//! offset 0:   [u32; n+1]   索引表（小端序）
//!               - 首值 = 整个索引表字节数
//!               - chunk 数量 = (首值 - 4) / 4
//!               - 每个值对应一个 chunk 在文件中的绝对偏移
//! offset N:   [u8; size]   各个 chunk 数据
//!               - 每个 chunk 大小 = next_offset - current_offset
//! ```
//!
//! ## 示例
//!
//! ```rust
//! use pal_assets::mkf::MkfArchive;
//!
//! let data = std::fs::read("data/ABC.MKF").unwrap();
//! let archive = MkfArchive::new(&data).unwrap();
//! assert!(archive.chunk_count() > 0);
//! let chunk = archive.read_chunk(0).unwrap();
//! ```

use binrw::BinRead;

/// MKF 文件索引表头部
#[derive(BinRead, Debug)]
struct MkfHeader {
    /// 索引表总大小（字节），包含自身
    /// chunk 数量 = (table_size - 4) / 4
    table_size: u32,
}

/// MKF 归档文件
#[derive(Debug)]
pub struct MkfArchive {
    /// chunk 偏移表
    offsets: Vec<u32>,
    /// 完整的 MKF 文件数据
    data: Vec<u8>,
}

impl MkfArchive {
    /// 从字节数据中加载 MKF 归档
    ///
    /// # 参数
    /// * `data` - 完整的 MKF 文件字节数据
    ///
    /// # 返回
    /// * `Some(MkfArchive)` - 解析成功
    /// * `None` - 数据不完整或格式错误
    pub fn new(data: &[u8]) -> Option<Self> {
        if data.len() < 4 {
            return None;
        }

        // 用 binrw 读取头部
        let mut cursor = std::io::Cursor::new(data);
        let header = MkfHeader::read_le(&mut cursor).ok()?;

        let table_size = header.table_size as usize;
        if table_size < 4 || table_size > data.len() {
            return None;
        }

        // chunk 数量
        let chunk_count = (table_size - 4) / 4;
        if chunk_count == 0 {
            return None;
        }

        // 读取偏移表
        let offsets: Vec<u32> = (0..chunk_count)
            .map(|i| {
                let offset = 4 + i * 4;
                u32::from_le_bytes(
                    data[offset..offset + 4].try_into().unwrap(),
                )
            })
            .collect();

        Some(MkfArchive {
            offsets,
            data: data.to_vec(),
        })
    }

    /// 返回 MKF 文件中的 chunk 数量
    pub fn chunk_count(&self) -> usize {
        self.offsets.len()
    }

    /// 读取指定索引的 chunk 数据
    ///
    /// # 参数
    /// * `index` - chunk 索引（从 0 开始）
    ///
    /// # 返回
    /// * `Some(&[u8])` - chunk 数据
    /// * `None` - 索引超出范围
    pub fn read_chunk(&self, index: usize) -> Option<&[u8]> {
        if index >= self.offsets.len() {
            return None;
        }

        let start = self.offsets[index] as usize;

        // 计算结束偏移：下一个 chunk 的偏移，或文件末尾
        let end = if index + 1 < self.offsets.len() {
            self.offsets[index + 1] as usize
        } else {
            self.data.len()
        };

        if start > self.data.len() || end > self.data.len() || start > end {
            return None;
        }

        Some(&self.data[start..end])
    }

    /// 读取指定索引的 chunk 并返回其所有权的 Vec<u8>
    pub fn read_chunk_owned(&self, index: usize) -> Option<Vec<u8>> {
        self.read_chunk(index).map(|data| data.to_vec())
    }

    /// 返回每个 chunk 的大小（字节）
    pub fn chunk_sizes(&self) -> Vec<usize> {
        (0..self.offsets.len())
            .map(|i| {
                let start = self.offsets[i] as usize;
                let end = if i + 1 < self.offsets.len() {
                    self.offsets[i + 1] as usize
                } else {
                    self.data.len()
                };
                end.saturating_sub(start)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_data() {
        assert!(MkfArchive::new(&[]).is_none());
        assert!(MkfArchive::new(&[0; 3]).is_none());
    }

    #[test]
    fn test_single_chunk() {
        // 索引表: 4 字节 (table_size=4) + 4 字节 (offset=0x08)
        // chunk: 4 字节数据
        let mut raw = Vec::new();
        raw.extend_from_slice(&8u32.to_le_bytes()); // table_size
        raw.extend_from_slice(&8u32.to_le_bytes()); // offset 0x08
        raw.extend_from_slice(b"ABCD"); // chunk 数据

        let archive = MkfArchive::new(&raw).unwrap();
        assert_eq!(archive.chunk_count(), 1);
        assert_eq!(archive.read_chunk(0).unwrap(), b"ABCD");
    }

    #[test]
    fn test_multiple_chunks() {
        // 索引表: 4 + 2*4 = 12 字节
        let mut raw = Vec::new();
        raw.extend_from_slice(&12u32.to_le_bytes()); // table_size
        raw.extend_from_slice(&12u32.to_le_bytes()); // offset 0x0C
        raw.extend_from_slice(&16u32.to_le_bytes()); // offset 0x10
        raw.extend_from_slice(b"AAAA"); // chunk 0
        raw.extend_from_slice(b"BBBB"); // chunk 1

        let archive = MkfArchive::new(&raw).unwrap();
        assert_eq!(archive.chunk_count(), 2);
        assert_eq!(archive.read_chunk(0).unwrap(), b"AAAA");
        assert_eq!(archive.read_chunk(1).unwrap(), b"BBBB");
    }

    #[test]
    fn test_out_of_range() {
        let mut raw = Vec::new();
        raw.extend_from_slice(&8u32.to_le_bytes());
        raw.extend_from_slice(&8u32.to_le_bytes());
        raw.extend_from_slice(b"TEST");

        let archive = MkfArchive::new(&raw).unwrap();
        assert!(archive.read_chunk(1).is_none());
    }
}
