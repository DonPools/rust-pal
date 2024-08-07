use crate::utils::*;
use std::vec;

pub struct SpriteFrame {
    pub width: u32,
    pub height: u32,
    data: Vec<u16>,
}

pub type Sprite = Vec<SpriteFrame>;

pub fn sprite_get_count(sprite_data: &[u8]) -> u32 {
    if sprite_data.len() == 0 {
        return 0;
    }

    (sprite_data[0] as u32) | ((sprite_data[1] as u32) << 8)
}

pub fn sprite_get_frame(sprite_data: &[u8], frame_index: u32) -> Result<SpriteFrame> {
    let image_count = (sprite_data[0] as u32) | ((sprite_data[1] as u32) << 8);
    if frame_index >= image_count {
        return Err(
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Index out of bounds"))
        );
    }

    let frame_index = frame_index << 1;
    let offset = (((sprite_data[frame_index as usize] as u32) |
        ((sprite_data[(frame_index as usize) + 1] as u32) << 8)) <<
        1) as usize;

    if offset >= sprite_data.len() {
        return Err(
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "Index out of bounds"))
        );
    }

    decode_rle_sprite_frame(&sprite_data[offset..])
}

// Get all frames of a sprite
pub fn sprite_get_frames(sprite_data: &[u8]) -> Result<Sprite> {
    let mut sprites = Vec::new();
    let count = sprite_get_count(&sprite_data);
    for j in 0..count {
        if let Ok(sprite) = sprite_get_frame(sprite_data, j) {
            sprites.push(sprite);
        } else {
            break;
        }
    }

    Ok(sprites)
}

fn decode_rle_sprite_frame(src_rle: &[u8]) -> Result<SpriteFrame> {
    let mut src_rle = src_rle;
    if src_rle[0] == 0x02 && src_rle[1] == 0x00 && src_rle[2] == 0x00 && src_rle[3] == 0x00 {
        src_rle = &src_rle[4..];
    }

    let width = u16::from_le_bytes([src_rle[0], src_rle[1]]) as u32;
    let height = u16::from_le_bytes([src_rle[2], src_rle[3]]) as u32;

    let src_rle = &src_rle[4..];

    let mut data = vec![0 as u16; (width * height) as usize];
    let mut ptr = 0;
    let mut dst_ptr = 0;

    while ptr < src_rle.len() {
        let mut count = src_rle[ptr];
        ptr += 1;
        let dst_data: Vec<u16>;
        if count < 0x80 {
            count = if ptr + (count as usize) > src_rle.len() {
                (src_rle.len() - ptr) as u8
            } else {
                count
            };
            //println!("ptr + cnt: {}, src_rle.len(): {}", ptr + cnt, src_rle.len());
            dst_data = src_rle[ptr..ptr + (count as usize)]
                .iter()
                .map(|&x| x as u16)
                .collect::<Vec<u16>>();
            ptr += count as usize;
        } else {
            count = count & 0x7f;
            dst_data = vec![0x100 as u16; count as usize];
        }

        let (cnt, finish) = if dst_ptr + (count as usize) > data.len() {
            (data.len() - dst_ptr, true)
        } else {
            (count as usize, false)
        };

        data[dst_ptr..dst_ptr + (cnt as usize)].copy_from_slice(&dst_data[..cnt]);
        dst_ptr += cnt as usize;
        if finish {
            break;
        }
    }

    Ok(SpriteFrame { width, height, data })
}

pub fn draw_sprite_frame(
    frame: &SpriteFrame,
    pixels: &mut [u8], // 目标图像缓冲区
    dest_width: usize, // 目标图像宽度
    dest_height: usize, // 目标图像高度
    x: isize, // 目标图像起始x坐标
    y: isize // 目标图像起始y坐标
) {
    if
        dest_width == 0 ||
        dest_height == 0 ||
        x + (frame.width as isize) <= 0 ||
        x >= (dest_width as isize) ||
        y + (frame.height as isize) <= 0 ||
        y >= (dest_height as isize)
    {
        return;
    }

    let maxy = std::cmp::min(y + (frame.height as isize), dest_height as isize);
    let maxx = std::cmp::min(x + (frame.width as isize), dest_width as isize);

    let sy = std::cmp::max(y, 0);
    let sx = std::cmp::max(x, 0);

    for dy in sy..maxy {
        for dx in sx..maxx {
            let src_index = ((dy - y) * (frame.width as isize) + (dx - x)) as usize;
            let dest_index = (dy * (dest_width as isize) + dx) as usize;

            let src_pixel = frame.data[src_index];
            if src_pixel < 0x100 {
                pixels[dest_index] = src_pixel as u8;
            }
        }
    }
}
