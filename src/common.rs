use sdl2::surface::Surface;

/// Holds any kind of error.
pub type Error = Box<dyn std::error::Error>;
/// Holds a `Result` of any kind of error.
pub type Result<T> = std::result::Result<T, Error>;

pub fn sprite_get_frame(sprite_data: &[u8], frame_index: u32) -> Result<&[u8]> {
    let image_count = sprite_data[0] as u32 | ((sprite_data[1] as u32) << 8);
    if frame_index >= image_count {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Index out of bounds",
        )));
    }

    let frame_index = frame_index << 1;
    let offset = ((sprite_data[frame_index as usize] as u32
        | ((sprite_data[frame_index as usize + 1] as u32) << 8))
        << 1) as usize;

    Ok(&sprite_data[offset..])
}

// RLE图像的宽度
pub fn rle_get_width(bitmap_rle: &[u8]) -> u32 {
    if bitmap_rle.is_empty() {
        return 0;
    }

    let mut bitmap_rle = bitmap_rle;
    if bitmap_rle[0] == 0x02
        && bitmap_rle[1] == 0x00
        && bitmap_rle[2] == 0x00
        && bitmap_rle[3] == 0x00
    {
        bitmap_rle = &bitmap_rle[4..];
    }

    u16::from_le_bytes([bitmap_rle[0], bitmap_rle[1]]) as u32
}

// RLE解码函数
pub fn decode_rle(
    src_rle: &[u8],     // 源RLE数据
    dest: &mut [u8],    // 目标图像缓冲区
    dest_width: usize,  // 目标图像宽度
    dest_height: usize, // 目标图像高度
    x: isize,             // 目标图像起始x坐标
    y: isize,             // 目标图像起始y坐标
) {
    let mut src_rle = src_rle;
    if src_rle[0] == 0x02 && src_rle[1] == 0x00 && src_rle[2] == 0x00 && src_rle[3] == 0x00
    {
        src_rle = &src_rle[4..];
    }

    // 读取RLE图像的宽度和高度
    let rle_width = u16::from_le_bytes([src_rle[0], src_rle[1]]) as isize;
    let rle_height = u16::from_le_bytes([src_rle[2], src_rle[3]]) as isize;

    let mut src_rle = &src_rle[4..];

    // 检查RLE图像是否能被正确显示在目标图像中
    if dest_width == 0
        || dest_height == 0
        || x + rle_width as isize <= 0
        || x >= dest_width as isize
        || y + rle_height as isize <= 0
        || y >= dest_height as isize
    {
        return;
    }

    let maxy = std::cmp::min(y + rle_height as isize, dest_height as isize);
    let maxx = std::cmp::min(x + rle_width as isize, dest_width as isize);
    let offx = dest_width - maxx as usize;

    let mut sy = 0;
    let mut dy = y;

    // 跳过不能显示的上部区域
    while dy < 0 {
        dy += 1;
        sy += 1;
        let mut sx = 0;
        while sx < rle_width {
            let count = src_rle[0];
            src_rle = &src_rle[1..];
            if count < 0x80 {
                src_rle = &src_rle[count as usize..];
                sx += count as isize;
            } else {
                sx += (count & 0x7f) as isize;
            }
        }
    }

    let mut dest_index = (dy * dest_width as isize) as usize;

    // 开始填充目标图像区域
    while dy < maxy {
        let mut dx = x;
        let mut count: u8 = 0;

        // 跳过不能显示的左侧区域
        while dx < 0 {
            count = src_rle[0];
            src_rle = &src_rle[1..];
            if count < 0x80 {
                src_rle = &src_rle[count as usize..];
                dx += count as isize;
            } else {
                dx += (count & 0x7f) as isize;
            }
        }

        let mut sx = if x < 0 {
            if dx > 0 {
                if count < 0x80 {
                    src_rle = &src_rle[(dx as usize)..];
                }
                count = (count & 0x80) | (dx as u8);
                dx = 0;
            } else {
                count = 0;
            }
            -x
        } else {
            0
        };

        dest_index += dx as usize;

        // 写入目标区域
        loop {
            let mut cnt = 0;
            if count < 0x80 {
                for _ in 0..count {
                    cnt += 1;
                    if dx >= maxx {
                        break;
                    }
                    let sval = src_rle[0];
                    src_rle = &src_rle[1..];

                    dest[dest_index] = sval;

                    dest_index += 1;
                    sx += 1;
                    dx += 1;
                }
            } else {
                for _ in 0..(count & 0x7f) {
                    cnt += 1;
                    if dx >= maxx {
                        break;
                    }
                    dest_index += 1;
                    sx += 1;
                    dx += 1;
                }
            }

            if dx < maxx {
                count = src_rle[0];
                src_rle = &src_rle[1..];
            } else {
                if count < 0x80 {
                    src_rle = &src_rle[(count - cnt) as usize..];
                }
                dest_index += offx;
                break;
            }
        }

        // 跳过完整行的剩余部分
        while sx < rle_width {
            count = src_rle[0];
            src_rle = &src_rle[1..];
            if count < 0x80 {
                src_rle = &src_rle[count as usize..];
                sx += count as isize;
            } else {
                sx += (count & 0x7f) as isize;
            }
        }

        dy += 1;
    }
}
