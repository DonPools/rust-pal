use std::time::Duration;
use chardetng::EncodingDetector;
use encoding_rs::Encoding;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;

use crate::{ game::Game, input::PalKey, sprite::*, utils::*, mkf::MKF };

pub struct MenuItem {
    pub value: u16,
    pub num_word: u32,
    pub enabled: bool,
    pub x: u16,
    pub y: u16,
}

pub const CHUNKNUM_SPRITEUI: u32 = 9;

pub const MAINMENU_BACKGROUND_FBPNUM: u32 = 60;
pub const RIX_NUM_OPENINGMENU: u32 = 4;

pub const MAINMENU_LABEL_NEWGAME: u32 = 7;
pub const MAINMENU_LABEL_LOADGAME: u32 = 8;
pub const LOADMENU_LABEL_SLOT_FIRST: u32 = 43;

pub const MENUITEM_COLOR: u8 = 0x4f;
pub const MENUITEM_COLOR_SELECTED_FIRST: u32 = 0xf9;
pub const MENUITEM_COLOR_SELECTED_TOTALNUM: u32 = 6;

pub struct UI {
    pub font_chars: Vec<char>,
    pub fonts: Vec<Vec<u8>>,
    pub msgs: Vec<String>,
    pub words: Vec<String>,
    pub encoding: &'static Encoding,
    pub sprite: Sprite, // ui sprites
}

impl UI {
    pub fn load(data_mkf: &mut MKF) -> Result<Self> {
        let mut asc_file = open_file("WOR16.ASC")?;
        let bytes = asc_file.seek(SeekFrom::End(0))?;
        let mut buf = vec![0; bytes as usize];

        asc_file.seek(SeekFrom::Start(0))?;
        asc_file.read_exact(&mut buf)?;

        let mut detector = EncodingDetector::new();
        detector.feed(&buf, true);
        let encoding = detector.guess(None, true);
        //println!("{:?}", encoding);

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
        for i in 0..buf.len() / 10 {
            let (s, _, _) = encoding.decode(&buf[i * 10..i * 10 + 10]);
            words.push(s.into_owned());
        }

        let mut sss_mfk = open_mkf("SSS.MKF")?;
        let buf = sss_mfk.read_chunk(3)?;
        let msg_count = buf.len() / 4;
        let mut offsets = vec![0; msg_count];
        for i in 0..msg_count {
            offsets[i] = u32::from_le_bytes(buf[i * 4..i * 4 + 4].try_into().unwrap());
        }

        let mut msg_file = open_file("M.MSG")?;
        let mut msgs = Vec::new();

        for i in 0..msg_count - 1 {
            let mut buf = vec![0; (offsets[i + 1] - offsets[i]) as usize];
            msg_file.seek(SeekFrom::Start(offsets[i] as u64))?;
            msg_file.read_exact(&mut buf)?;
            let (s, _, _) = encoding.decode(&buf);
            let s = s.into_owned();
            msgs.push(s);
        }

        let chunk = data_mkf.read_chunk(CHUNKNUM_SPRITEUI)?;
        let sprite = sprite_get_frames(&chunk)?;

        Ok(Self { font_chars, fonts, words, msgs, encoding, sprite })
    }

    pub fn draw_char(
        &self,
        pixels: &mut [u8],
        dest_width: u32,
        dest_height: u32,
        x: i32,
        y: i32,
        c: char,
        color: u8
    ) {
        let r = self.font_chars.iter().position(|&r| r == c);
        if r.is_none() {
            return;
        }

        let index = r.unwrap();
        let font = &self.fonts[index];

        for i in (0..32).step_by(2) {
            let byte = u16::from_le_bytes(font[i..i + 2].try_into().unwrap());
            let sy = y + ((i / 2) as i32);

            for bit in 0..16 {
                let sx = x + (bit as i32);
                if sx >= (dest_width as i32) || sy >= (dest_height as i32) || sx < 0 || sy < 0 {
                    continue;
                }
                let index = (sy * (dest_width as i32) + sx) as usize;
                if (byte & (1 << bit)) != 0 {
                    pixels[index] = color;
                }
            }
        }
    }

    pub fn get_word(&self, index: usize) -> &str {
        if index >= self.words.len() {
            return "";
        }
        &self.words[index]
    }

    pub fn get_msg(&self, index: usize) -> &str {
        if index >= self.msgs.len() {
            return "";
        }
        &self.msgs[index]
    }
}

impl Game {
    pub fn draw_text(
        ui: &UI,
        pixels: &mut [u8],
        dest_width: u32,
        dest_height: u32,
        x: i32,
        y: i32,
        text: &str,
        color: u8,
        shadow: bool
    ) {
        let mut x = x;
        for c in text.chars() {
            if c.is_numeric() {
                if let Some(i) = c.to_digit(10) {
                    let frame = &ui.sprite[(i as usize) + 29];
                    draw_sprite_frame(
                        frame,
                        pixels,
                        dest_width as usize,
                        dest_height as usize,
                        x as isize,
                        y as isize + 4
                    );
                    x += 8;
                }
            } else {
                if shadow {
                    ui.draw_char(pixels, dest_width, dest_height, x + 1, y + 1, c, 0);
                    ui.draw_char(pixels, dest_width, dest_height, x + 1, y, c, 0);
                    ui.draw_char(pixels, dest_width, dest_height, x, y + 1, c, 0);
                }
                ui.draw_char(pixels, dest_width, dest_height, x, y, c, color);
                x += 16;
            }
        }
    }

    pub fn draw_word(
        ui: &UI,
        pixels: &mut [u8],
        dest_width: u32,
        dest_height: u32,
        x: i32,
        y: i32,
        index: usize,
        color: u8,
        shadow: bool
    ) {
        let text = ui.get_word(index);
        Self::draw_text(ui, pixels, dest_width, dest_height, x, y, text, color, shadow);
    }

    pub fn menu_color_selected(&self) -> u8 {
        let ticks = self.ticks();
        let ticks = ticks / (600 / MENUITEM_COLOR_SELECTED_TOTALNUM);
        let ticks = ticks % MENUITEM_COLOR_SELECTED_TOTALNUM;
        (ticks as u8) + (MENUITEM_COLOR_SELECTED_FIRST as u8)
    }

    pub fn read_menu(&mut self, menu_items: &[MenuItem]) -> Result<u16> {
        let mut selected_index = 0;
        let mut pixels = self.canvas.get_pixels().to_vec();
        loop {
            for i in 0..menu_items.len() {
                let item = &menu_items[i];
                let color = if i == selected_index {
                    self.menu_color_selected()
                } else {
                    MENUITEM_COLOR
                };

                if item.enabled {
                    Self::draw_word(
                        &self.ui,
                        &mut pixels,
                        320,
                        200,
                        item.x as i32,
                        item.y as i32,
                        item.num_word as usize,
                        color,
                        true
                    );
                }
            }

            self.canvas.set_pixels(|_pixels: &mut [u8]| {
                _pixels.copy_from_slice(&pixels);
            });

            self.blit_to_screen()?;
            self.process_event();

            if self.input.is_pressed(PalKey::Down) || self.input.is_pressed(PalKey::Right) {
                selected_index = (menu_items.len() + selected_index + 1) % menu_items.len();
            } else if self.input.is_pressed(PalKey::Up) || self.input.is_pressed(PalKey::Left) {
                selected_index = (menu_items.len() + selected_index - 1) % menu_items.len();
            }

            if self.input.is_pressed(PalKey::Search) {
                return Result::Ok(menu_items[selected_index].value);
            }

            std::thread::sleep(Duration::from_millis(30));
        }
    }

    // len: number of mid box sprites
    pub fn draw_signle_linebox_with_shadow(&mut self, pos: Pos, len: u32) {
        let left_box_frame = &self.ui.sprite[44];
        let mid_box_frame = &self.ui.sprite[45];
        let right_box_frame = &self.ui.sprite[46];

        let mut x = pos.x;
        let y = pos.y;

        self.canvas.set_pixels(|pixels: &mut [u8]| {
            draw_sprite_frame(left_box_frame, pixels, 320, 200, x, y);
            x += left_box_frame.width as isize;
            for _ in 0..len {
                draw_sprite_frame(mid_box_frame, pixels, 320, 200, x, y);
                x += mid_box_frame.width as isize;
            }
            draw_sprite_frame(right_box_frame, pixels, 320, 200, x, y);
        });
    }
}
