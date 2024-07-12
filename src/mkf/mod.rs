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

    fn read_chunk_offset(&mut self, index: u32) -> Result<(u32, u32), std::io::Error> {
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

        Ok((offset, next_offset))
    }

    pub fn read_chunk(&mut self, index: u32) -> Result<Vec<u8>, std::io::Error> {
        let (offset, next_offset) = self.read_chunk_offset(index)?;

        let size = next_offset - offset;
        let mut data = vec![0; size as usize];
        self.file.seek(SeekFrom::Start(offset.into()))?;
        self.file.read_exact(&mut data)?;


        Ok(data)
    }

    pub fn read_chunk_decompressed(&mut self, index: u32) -> Result<Vec<u8>, std::io::Error> {
        let data = self.read_chunk(index)?;
        match decompress::decompress(data) {
            Ok(decompressed_data) => {
                return Ok(decompressed_data);
            },
            Err(e) => {
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e));
            }
        }
    }

    pub fn read_rng_sub_count(&mut self, index: u32) -> Result<u32, std::io::Error> {
        let (offset, _) = self.read_chunk_offset(index)?;
        let mut buf = [0; 4];    
        self.file.seek(SeekFrom::Start(offset.into()))?;
        self.file.read_exact(&mut buf)?;
        
        let t = u32::from_le_bytes(buf);
        // sub chunk count
        Ok((t - 4) / 4)
    }

    pub fn read_rng_chunk(&mut self, index: u32, frame_index :u32) -> Result<Vec<u8>, std::io::Error> {
        let (offset, _) = self.read_chunk_offset(index)?;
        let mut buf = [0; 4];    
        self.file.seek(SeekFrom::Start(offset.into()))?;
        self.file.read_exact(&mut buf)?;
        
        // sub chunk count
        let chunk_count = u32::from_le_bytes(buf);
        if frame_index >= chunk_count {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Index out of bounds"));
        }

        self.file.seek(SeekFrom::Start((offset + frame_index * 4) as u64))?;

        self.file.read_exact(&mut buf)?;
        let sub_offset = u32::from_le_bytes(buf);

        self.file.read_exact(&mut buf)?;
        let sub_next_offset = u32::from_le_bytes(buf);

        let chunk_size = sub_next_offset - sub_offset;
        if chunk_size == 0 {
            return Ok(vec![]);
        }

        let mut data = vec![0; chunk_size as usize];

        let chunk_offset = offset + sub_offset;
        println!("i: {} sub_offset: {} chunk_size: {}", frame_index, chunk_offset, chunk_size);
        self.file.seek(SeekFrom::Start(chunk_offset.into()))?;
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

pub fn open(mut file: File) -> Result<MKF, std::io::Error>{
    let buf = &mut [0; 4];

    file.read_exact(buf)?;
    let t = u32::from_le_bytes(*buf);
    let chunk_count = (t - 4) >> 2;

    Ok(MKF { file, chunk_count})
}