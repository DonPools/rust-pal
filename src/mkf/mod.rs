mod decompress;

use std::{fs::{File}, io::{Read, Seek, SeekFrom}};

#[derive(Debug)]
pub struct MKF {
    file: File,
    chunk_count: u32,
}

impl MKF {
    pub fn chunk_count(&self) -> u32 {
        self.chunk_count
    }

    pub fn read_chunk(&mut self, index: u32) -> Result<Vec<u8>, std::io::Error> {
        if index >= self.chunk_count {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Index out of bounds"));
        }

        let mut buf = [0; 4];
        self.file.seek(SeekFrom::Start(index as u64 * 4).into())?;

        self.file.read_exact(&mut buf)?;
        let offset = u32::from_le_bytes(buf);
        
        self.file.read_exact(&mut buf)?;
        let next_offset = u32::from_le_bytes(buf);

        if next_offset <= 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Chunk is empty"));
        }

        let size = next_offset - offset;
        let mut data = vec![0; size as usize];
        self.file.seek(SeekFrom::Start(offset.into()))?;
        self.file.read_exact(&mut data)?;

        match decompress::decompress(data) {
            Ok(decompressed_data) => {
                return Ok(decompressed_data);
            },
            Err(e) => {
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e));
            }
        }
    }
}

pub fn open(path: &str) -> Result<MKF, std::io::Error>{
    let mut file = File::open(path)?;
    let buf = &mut [0; 4];

    file.read_exact(buf)?;
    let t = u32::from_le_bytes(*buf);
    let chunk_count = (t - 4) >> 2;

    Ok(MKF { file, chunk_count})
}