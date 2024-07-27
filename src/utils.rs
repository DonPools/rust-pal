use std::path;
use std::fs::File;

use crate::game::Game;
use crate::mkf;

pub type Error = Box<dyn std::error::Error>;
pub type Result<T> = std::result::Result<T, Error>;

// Direction
pub enum Dir
{
   South = 0,
   West,
   North,
   East,
   Unknown
}

const BASE_PATH: &str = "/home/rocky/Code/Game/PAL95/";

pub fn open_file(filename: &str) -> Result<File> {
    let filepath = path::Path::new(BASE_PATH).join(filename);
    let file = std::fs::File::open(filepath)?;

    Ok(file)
}

pub fn open_mkf(filename: &str) -> Result<mkf::MKF> {
    let file = open_file(filename)?;
    let mkf = mkf::open(file)?;

    Ok(mkf)
}

pub fn reverse_bits(u8: u8) -> u8 {
    let mut u8 = u8;
    u8 = (u8 & 0xF0) >> 4 | (u8 & 0x0F) << 4;
    u8 = (u8 & 0xCC) >> 2 | (u8 & 0x33) << 2;
    u8 = (u8 & 0xAA) >> 1 | (u8 & 0x55) << 1;
    u8
}

impl Game {
    pub fn ticks (&self) -> u32 {
        self.start_time.elapsed().as_millis() as u32
    }
}
