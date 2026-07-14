//! Safe, pure-Rust YJ_1 decompressor.

const FILE_HEADER_SIZE: usize = 16;
const BLOCK_HEADER_SIZE: usize = 24;
const SIGNATURE: &[u8; 4] = b"YJ_1";

#[derive(Clone, Copy, Debug, Default)]
struct TreeNode {
    value: u8,
    leaf: bool,
    left: usize,
    right: usize,
}

#[derive(Debug)]
struct BlockHeader {
    uncompressed_len: usize,
    repeat: [u16; 4],
    offset_bits: [u8; 4],
    repeat_bits: [u8; 3],
    count_bits: [u8; 3],
    count: [u8; 2],
}

impl BlockHeader {
    fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < BLOCK_HEADER_SIZE {
            return None;
        }
        Some(Self {
            uncompressed_len: read_u16(data, 0)? as usize,
            repeat: [
                read_u16(data, 4)?,
                read_u16(data, 6)?,
                read_u16(data, 8)?,
                read_u16(data, 10)?,
            ],
            offset_bits: data[12..16].try_into().ok()?,
            repeat_bits: data[16..19].try_into().ok()?,
            count_bits: data[19..22].try_into().ok()?,
            count: data[22..24].try_into().ok()?,
        })
    }
}

struct BitReader<'a> {
    data: &'a [u8],
    bit: usize,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, bit: 0 }
    }

    // Bits are read MSB-first from little-endian 16-bit words.
    fn read(&mut self, count: u8) -> Option<usize> {
        if count > 16 {
            return None;
        }
        let mut value = 0usize;
        for _ in 0..count {
            let word_offset = (self.bit / 16).checked_mul(2)?;
            let word = read_u16(self.data, word_offset)?;
            let shift = 15 - (self.bit % 16);
            value = (value << 1) | ((word as usize >> shift) & 1);
            self.bit += 1;
        }
        Some(value)
    }
}

/// Return the uncompressed length stored in a YJ_1 header.
pub fn uncompressed_size(source: &[u8]) -> Option<usize> {
    if source.get(..4)? != SIGNATURE {
        return None;
    }
    Some(read_u32(source, 4)? as usize)
}

/// Decompress one complete YJ_1 stream.
pub fn decompress(source: &[u8]) -> Option<Vec<u8>> {
    let output_len = uncompressed_size(source)?;
    let compressed_len = read_u32(source, 8)? as usize;
    let block_count = read_u16(source, 12)? as usize;
    if compressed_len < FILE_HEADER_SIZE || compressed_len > source.len() {
        return None;
    }

    let tree_len = (source[15] as usize).checked_mul(2)?;
    let values_start = FILE_HEADER_SIZE;
    let flags_start = values_start.checked_add(tree_len)?;
    let flags_len = tree_len.div_ceil(16).checked_mul(2)?;
    let mut cursor = flags_start.checked_add(flags_len)?;
    if cursor > compressed_len {
        return None;
    }

    let mut tree = vec![TreeNode::default(); tree_len + 1];
    if tree_len > 0 {
        tree[0].left = 1;
        tree[0].right = 2;
        let mut flags = BitReader::new(source.get(flags_start..cursor)?);
        #[allow(clippy::needless_range_loop)]
        for index in 1..=tree_len {
            let value = *source.get(values_start + index - 1)?;
            let leaf = flags.read(1)? == 0;
            let (left, right) = if leaf {
                (0, 0)
            } else {
                let left = (value as usize).checked_mul(2)?.checked_add(1)?;
                if left + 1 > tree_len {
                    return None;
                }
                (left, left + 1)
            };
            tree[index] = TreeNode {
                value,
                leaf,
                left,
                right,
            };
        }
    }

    let mut output = Vec::with_capacity(output_len);
    for _ in 0..block_count {
        let block_start = cursor;
        let block_data = source.get(block_start..compressed_len)?;
        let uncompressed_len = read_u16(block_data, 0)? as usize;
        let block_len = read_u16(block_data, 2)? as usize;
        let output_start = output.len();

        if block_len == 0 {
            let data_start = block_start.checked_add(4)?;
            cursor = data_start.checked_add(uncompressed_len)?;
            output.extend_from_slice(source.get(data_start..cursor)?);
        } else {
            if block_len < BLOCK_HEADER_SIZE || tree_len == 0 {
                return None;
            }
            cursor = block_start.checked_add(block_len)?;
            if cursor > compressed_len {
                return None;
            }
            let header = BlockHeader::parse(source.get(block_start..cursor)?)?;
            let mut bits = BitReader::new(source.get(block_start + BLOCK_HEADER_SIZE..cursor)?);
            decode_block(&header, &tree, &mut bits, &mut output)?;
        }

        if output.len().checked_sub(output_start)? != uncompressed_len || output.len() > output_len
        {
            return None;
        }
    }

    (output.len() == output_len).then_some(output)
}

fn decode_block(
    header: &BlockHeader,
    tree: &[TreeNode],
    bits: &mut BitReader<'_>,
    output: &mut Vec<u8>,
) -> Option<()> {
    let start = output.len();
    loop {
        let literal_count = read_loop(bits, header)?;
        if literal_count == 0 {
            break;
        }
        for _ in 0..literal_count {
            let mut index = 0usize;
            while !tree.get(index)?.leaf {
                let node = tree[index];
                index = if bits.read(1)? == 0 {
                    node.left
                } else {
                    node.right
                };
            }
            output.push(tree[index].value);
            if output.len() - start > header.uncompressed_len {
                return None;
            }
        }

        let copy_count = read_loop(bits, header)?;
        if copy_count == 0 {
            break;
        }
        for _ in 0..copy_count {
            let count = read_repeat_count(bits, header)?;
            let slot = bits.read(2)?;
            let distance = bits.read(*header.offset_bits.get(slot)?)?;
            if distance == 0 || distance > output.len() {
                return None;
            }
            for _ in 0..count {
                let value = *output.get(output.len() - distance)?;
                output.push(value);
                if output.len() - start > header.uncompressed_len {
                    return None;
                }
            }
        }
    }
    Some(())
}

fn read_loop(bits: &mut BitReader<'_>, header: &BlockHeader) -> Option<usize> {
    if bits.read(1)? != 0 {
        return Some(header.count[0] as usize);
    }
    let slot = bits.read(2)?;
    if slot == 0 {
        Some(header.count[1] as usize)
    } else {
        bits.read(*header.count_bits.get(slot - 1)?)
    }
}

fn read_repeat_count(bits: &mut BitReader<'_>, header: &BlockHeader) -> Option<usize> {
    let slot = bits.read(2)?;
    if slot == 0 {
        return Some(header.repeat[0] as usize);
    }
    if bits.read(1)? != 0 {
        bits.read(*header.repeat_bits.get(slot - 1)?)
    } else {
        Some(*header.repeat.get(slot)? as usize)
    }
}

fn read_u16(data: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        data.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        data.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_stream(data: &[u8]) -> Vec<u8> {
        let mut result = Vec::new();
        result.extend_from_slice(SIGNATURE);
        result.extend_from_slice(&(data.len() as u32).to_le_bytes());
        result.extend_from_slice(&((FILE_HEADER_SIZE + 4 + data.len()) as u32).to_le_bytes());
        result.extend_from_slice(&1u16.to_le_bytes());
        result.extend_from_slice(&[0, 0]);
        result.extend_from_slice(&(data.len() as u16).to_le_bytes());
        result.extend_from_slice(&0u16.to_le_bytes());
        result.extend_from_slice(data);
        result
    }

    #[test]
    fn decompresses_uncompressed_block() {
        let source = raw_stream(b"PAL");
        assert_eq!(uncompressed_size(&source), Some(3));
        assert_eq!(decompress(&source), Some(b"PAL".to_vec()));
    }

    #[test]
    fn rejects_truncated_and_invalid_streams() {
        let mut source = raw_stream(b"PAL");
        source.pop();
        assert!(decompress(&source).is_none());
        assert!(decompress(b"not YJ1").is_none());
    }
}
