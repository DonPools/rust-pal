use core::panic;


pub fn decode_rng(src: &[u8], dst: &mut [u8], i: u32) {
    let mut ptr = 0;
    let mut dst_ptr = 0;

    while ptr < src.len() {
        let data = src[ptr];
        ptr += 1;

        if i == 44 {
            println!("data: {:02x} ptr: {}", data, ptr);
        }

        match data {
            0x00 | 0x13 => {
                break;
            }
            0x01 | 0x05 => {}
            0x02 => {
                dst_ptr += 2;
            }
            0x03 => {
                let offset = src[ptr] as usize;
                dst_ptr += (offset + 1) * 2;
                ptr += 1;
            }
            0x04 => {
                let wdata = (src[ptr] as u32 | ((src[ptr + 1] as u32) << 8)) as usize;
                dst_ptr += (wdata + 1) * 2;
                ptr += 2;
            }
            0x0a | 0x09 | 0x08 | 0x07 | 0x06 => {
                let rep = data - 0x05;
                for _ in 0..rep {
                    dst[dst_ptr..dst_ptr + 2].copy_from_slice(&src[ptr..ptr + 2]);
                    ptr += 2;
                    dst_ptr += 2;
                }
            }
            0x0b => {
                let rep = src[ptr] as usize;
                ptr += 1;
                for _ in 0..=rep {
                    dst[dst_ptr..dst_ptr + 2].copy_from_slice(&src[ptr..ptr + 2]);
                    ptr += 2;
                    dst_ptr += 2;
                }
            }
            0x0c => {
                let rep = src[ptr] as u32 | ((src[ptr + 1] as u32) << 8);
                ptr += 2;
                for _ in 0..=rep {
                    dst[dst_ptr..dst_ptr + 2].copy_from_slice(&src[ptr..ptr + 2]);
                    ptr += 2;
                    dst_ptr += 2;
                }
            }
            0x0d | 0x0e | 0x0f | 0x10 => {
                let rep = (data - 0x0b) as usize;
                for _ in 0..rep {
                    dst[dst_ptr..dst_ptr + 2].copy_from_slice(&src[ptr..ptr + 2]);
                    dst_ptr += 2;
                }
                ptr += 2;
            }
            0x11 => {
                let rep = src[ptr] as usize;
                ptr += 1;
                for _ in 0..=rep {
                    dst[dst_ptr..dst_ptr + 2].copy_from_slice(&src[ptr..ptr + 2]);
                    dst_ptr += 2;
                }
                ptr += 2;
            }
            0x12 => {
                let rep = (src[ptr] as u32 | ((src[ptr + 1] as u32) << 8)) as usize; 
                ptr += 2;                
                for _ in 0..=rep {
                    dst[dst_ptr..dst_ptr + 2].copy_from_slice(&src[ptr..ptr + 2]);
                    dst_ptr += 2;
                }
                ptr += 2;        
            }
            _ => {
                panic!("Unknown data: {:02X}", data);
            }
        }
    }
}
