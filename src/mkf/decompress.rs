
#[derive(Debug)]
struct YJ1Header {
    signature: String,
    uncompressed_length: u32,
    compressed_length: u32,
    block_count: u16,
    unknown: u8,
    huffman_tree_length: u8,
}

impl YJ1Header {
    fn from(data: &[u8]) -> YJ1Header {
        YJ1Header {
            signature: String::from_utf8(data[0..4].to_vec()).unwrap(),
            uncompressed_length: u32::from_le_bytes([data[4], data[5], data[6], data[7]]),
            compressed_length: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
            block_count: u16::from_le_bytes([data[12], data[13]]),
            unknown: data[14],
            huffman_tree_length: data[15],
        }
    }
}

#[derive(Debug)]
struct YJ1BlockHeader {
    uncompressed_length: u16,
    compressed_length: u16,
    lzss_repeat_table: [u16; 4],
    lzss_offset_code_length_table: [u8; 4],
    lzss_repeat_code_length_table: [u8; 3],
    code_count_code_length_table: [u8; 3],
    code_count_table: [u8; 2],
}

impl YJ1BlockHeader {
    fn from(data: &[u8]) -> YJ1BlockHeader {
        let uncompressed_length = u16::from_le_bytes([data[0], data[1]]);
        let compressed_length = u16::from_le_bytes([data[2], data[3]]);

        if compressed_length == 0 {
            return YJ1BlockHeader {
                uncompressed_length,
                compressed_length,
                lzss_repeat_table: [0; 4],
                lzss_offset_code_length_table: [0; 4],
                lzss_repeat_code_length_table: [0; 3],
                code_count_code_length_table: [0; 3],
                code_count_table: [0; 2],
            };
        }

        YJ1BlockHeader {
            uncompressed_length,
            compressed_length,
            lzss_repeat_table: [
                u16::from_le_bytes([data[4], data[5]]),
                u16::from_le_bytes([data[6], data[7]]),
                u16::from_le_bytes([data[8], data[9]]),
                u16::from_le_bytes([data[10], data[11]]),
            ],
            lzss_offset_code_length_table: [data[12], data[13], data[14], data[15]],
            lzss_repeat_code_length_table: [data[16], data[17], data[18]],
            code_count_code_length_table: [data[19], data[20], data[21]],
            code_count_table: [data[22], data[23]],
        }
    }
}

#[derive(Debug, Clone)]
struct YJ1TreeNode {
    value: u8,
    leaf: bool,
    left: Option<usize>,
    right: Option<usize>,
}

impl YJ1TreeNode {
    fn new() -> YJ1TreeNode {
        YJ1TreeNode {
            value: 0,
            leaf: false,

            left: None,
            right: None,
        }
    }
}

fn yj1_get_bits(src: &[u8], bitptr: &mut u32, count: u32) -> u32 {
    let temp = &src[((*bitptr >> 4) << 1) as usize..];
    let bptr = *bitptr & 0xf;
    let mask: u32;

    *bitptr += count;

    if count > 16 - bptr {
        let count = count + bptr - 16;
        mask = 0xffff >> bptr;

        return ((((temp[0] as u32) | ((temp[1] as u32) << 8)) & mask) << count)
            | (((temp[2] as u32) | ((temp[3] as u32) << 8)) >> (16 - count));
    } else {
        return ((((temp[0] as u16) | ((temp[1] as u16) << 8)) << bptr) >> (16 - count)) as u32;
    }
}

fn yj1_get_loop(src: &[u8], bitptr: &mut u32, header: &YJ1BlockHeader) -> u16 {
    if yj1_get_bits(src, bitptr, 1) != 0 {
        return header.code_count_table[0] as u16;
    } else {
        let temp = yj1_get_bits(src, bitptr, 2);
        if temp != 0 {
            return yj1_get_bits(
                src,
                bitptr,
                header.code_count_code_length_table[temp as usize - 1] as u32,
            ) as u16;
        } else {
            return header.code_count_table[1] as u16;
        }
    }
}

fn yj1_get_count(src: &[u8], bitptr: &mut u32, header: &YJ1BlockHeader) -> u16 {
    let temp = yj1_get_bits(src, bitptr, 2);
    if temp != 0 {
        if yj1_get_bits(src, bitptr, 1) == 1 {
            return yj1_get_bits(
                src,
                bitptr,
                header.lzss_repeat_code_length_table[temp as usize - 1] as u32,
            ) as u16;
        } else {
            return header.lzss_repeat_table[temp as usize];
        }
    } else {
        return header.lzss_repeat_table[0];
    }
}

pub fn decompress(data: Vec<u8>) -> Result<Vec<u8>, &'static str> {
    let header = YJ1Header::from(data.as_slice());
    if header.signature != "YJ_1" {
        return Err("Invalid signature");
    }

    let mut dst_data = vec![0; header.uncompressed_length as usize];
    //println!("chunk header: {:?}", header);
    let mut dst_offset: usize = 0;
    let tree_len = header.huffman_tree_length as usize * 2;
    let mut bitptr: u32 = 0;
    let flag: &[u8] = &data[16 + tree_len..];

    let mut root = vec![YJ1TreeNode::new(); tree_len + 1];

    root[0].leaf = false;
    root[0].value = 0;
    root[0].left = Some(1);
    root[0].right = Some(2);

    for i in 1..=tree_len {
        root[i].leaf = yj1_get_bits(flag, &mut bitptr, 1) == 0;
        root[i].value = data[15 + i];
        if root[i].leaf {
            root[i].left = None;
            root[i].right = None;
        } else {
            root[i].left = Some(((root[i].value as u32) << 1) as usize + 1);
            root[i].right = Some(root[i].left.unwrap() + 1);
        }
    }

    let t = if tree_len & 0xf != 0 {
        (tree_len >> 4) + 1
    } else {
        tree_len >> 4
    } << 1;

    let mut block_offset = 16 + tree_len + t;
    for i in 0..header.block_count as usize {
        let block_header = YJ1BlockHeader::from(&data[block_offset..]);
        let mut header_length = 4;

        if block_header.compressed_length == 0 {
            dst_data[dst_offset..dst_offset + block_header.uncompressed_length as usize]
                .copy_from_slice(
                    &data[block_offset..block_offset + block_header.uncompressed_length as usize],
                );
            block_offset += block_header.uncompressed_length as usize;
            dst_offset += block_header.uncompressed_length as usize;
            continue;
        }

        header_length += 20;        
        bitptr = 0;
        let block_data = &data[block_offset+header_length..];
        
        let mut i = 0;
        loop {
            let mut loop_count = yj1_get_loop(block_data, &mut bitptr, &block_header);
            if loop_count == 0 {
                break;
            }
            i += 1;

            for _ in 0..loop_count {
                let mut node = &root[0];
                while !node.leaf {
                    node = if yj1_get_bits(block_data, &mut bitptr, 1) != 0 {
                        &root[node.right.unwrap()]
                    } else {
                        &root[node.left.unwrap()]
                    };
                }
                dst_data[dst_offset] = node.value;
                dst_offset += 1;
            }

            loop_count = yj1_get_loop(block_data, &mut bitptr, &block_header);
            if loop_count == 0 {
                break;
            }

            for _ in 0..loop_count {
                let count = yj1_get_count(block_data, &mut bitptr, &block_header);
                let mut pos = yj1_get_bits(block_data, &mut bitptr, 2);
                pos = yj1_get_bits(
                    block_data,
                    &mut bitptr,
                    block_header.lzss_offset_code_length_table[pos as usize] as u32,
                );
                for _ in 0..count {                    
                    dst_data[dst_offset] = dst_data[dst_offset - pos as usize];
                    dst_offset += 1;
                }
            }
        }
        
        block_offset += block_header.compressed_length as usize;
    }

    Ok(dst_data)
}
