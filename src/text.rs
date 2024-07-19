use crate::utils::open_mkf;
use crate::utils::{open_file, reverse_bits, Result};
use chardetng::EncodingDetector;

use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;

pub struct Text {
    pub font_chars: Vec<char>,
    fonts: Vec<Vec<u8>>,
    msgs: Vec<String>,
    words: Vec<String>,
}

impl Text {
    pub fn load() -> Result<Text> {
        let mut asc_file = open_file("WOR16.ASC")?;
        let bytes = asc_file.seek(SeekFrom::End(0))?;
        let mut buf = vec![0; bytes as usize];
        println!("{} bytes", bytes);

        asc_file.seek(SeekFrom::Start(0))?;
        asc_file.read_exact(&mut buf)?;

        let mut detector = EncodingDetector::new();
        detector.feed(&buf, true);
        let encoding = detector.guess(None, true);
        println!("{:?}", encoding);

        let (decoded, _, _) = encoding.decode(&buf);
        let font_chars: Vec<char> = decoded.chars().collect();
        let n_chars = font_chars.len();

        // 16*16 font
        let mut font_file = open_file("WOR16.FON")?;
        font_file.seek(SeekFrom::Start(0x682))?;        

        let mut fonts = vec![vec![0; 32]; n_chars];
        for i in 0..n_chars {
            font_file.read_exact(&mut fonts[i][0..30])?;
            for j in 0..30 {
                fonts[i][j] = reverse_bits(fonts[i][j]);
            }
        }

        let mut word_file = open_file("WORD.DAT")?;
        let bytes = word_file.seek(SeekFrom::End(0))?;
        let mut buf = vec![0; bytes as usize];
        word_file.seek(SeekFrom::Start(0))?;
        word_file.read_exact(&mut buf)?;

        let mut words: Vec<String> = Vec::new();
        for i in 0..(buf.len() / 10)  {
            let (s, _, _) = encoding.decode(&buf[i*10..i*10+10]);
            words.push(s.into_owned());
        }

        let mut sss_mfk = open_mkf("SSS.MKF")?;
        let buf = sss_mfk.read_chunk(3)?;
        let msg_count = buf.len() / 4;
        let mut offsets = vec![0; msg_count];
        for i in 0..msg_count {
            offsets[i] = u32::from_le_bytes(buf[i*4..i*4+4].try_into().unwrap());
        }

        let mut msg_file = open_file("M.MSG")?;
        let mut msgs = Vec::new();

        for i in 0..msg_count-1 {
            let mut buf = vec![0; (offsets[i+1] - offsets[i]) as usize];
            msg_file.seek(SeekFrom::Start(offsets[i] as u64))?;
            msg_file.read_exact(&mut buf)?;
            let (s, _, _) = encoding.decode(&buf);
            let s = s.into_owned();
            println!("{}", s);
            msgs.push(s);
        }

        Ok(Text {
            font_chars,
            fonts,
            words,
            msgs: Vec::new(),            
        })
    }

    pub fn draw_char(
        &self,
        pixels: &mut [u8],
        dest_width: u32,
        dest_height: u32,
        x: i32,
        y: i32,
        c: char,
        color: u8,
    ) -> Result<()> {
        let r = self.font_chars.iter().position(|&r| r == c);
        if r.is_none() {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid character",
            )));
        }

        let index = r.unwrap();
        let font = &self.fonts[index];

        for i in (0..32).step_by(2) {
            let byte = u16::from_le_bytes(font[i..i + 2].try_into().unwrap());
            let sy = y + (i / 2) as i32;

            for bit in 0..16 {
                let sx = x + bit as i32;                
                if sx >= dest_width as i32 || sy >= dest_height as i32 || sx < 0 || sy < 0 {
                    continue;
                }
                let index = (sy * dest_width as i32 + sx) as usize;
                if byte & (1 << bit) != 0 {
                    pixels[index] = color;
                }
            }
        }

        Ok(())
    }
}
