use std::fs::File;
use std::path;

use bincode::{config, decode_from_slice, Decode};

use crate::game::Game;
use crate::mkf;

pub type Error = Box<dyn std::error::Error>;
pub type Result<T> = std::result::Result<T, Error>;

// Direction
#[derive(Debug, PartialEq, Clone)]
pub enum Dir {
    South = 0,
    West,
    North,
    East,
    Unknown,
}

impl Dir {
    pub fn from_u8(u8: u8) -> Self {
        match u8 {
            0 => Dir::South,
            1 => Dir::West,
            2 => Dir::North,
            3 => Dir::East,
            _ => Dir::Unknown,
        }
    }
}

pub struct Pos {
    pub x: isize,
    pub y: isize,
}

#[derive(Clone, Copy)]
pub struct Rect {
    pub x: isize,
    pub y: isize,
    pub w: usize,
    pub h: usize,
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
    pub fn ticks(&self) -> u32 {
        self.start_time.elapsed().as_millis() as u32
    }
}

pub fn decode_c_structs<T: Decode>(buf: &[u8]) -> Result<Vec<T>> {
    let c = config::standard()
        .with_little_endian()
        .with_fixed_int_encoding();

    let mut buf = buf;
    let mut objects = Vec::<T>::new();

    loop {
        let (obj, size): (T, usize) = decode_from_slice(&buf, c)?;
        objects.push(obj);
        buf = &buf[size..];
        if buf.len() < size {
            break;
        }
    }

    Ok(objects)
}
