use std::time::Duration;
use std::time::Instant;

use minifb::{Window, WindowOptions};

use crate::canvas::*;
use crate::input::InputState;
use crate::mkf;
use crate::sprite::*;
use crate::text::*;
use crate::ui::*;
use crate::utils::*;

// RNG
const BITMAPNUM_SPLASH_UP: u32 = 0x26;
const BITMAPNUM_SPLASH_DOWN: u32 = 0x27;

// RLE SPRITE
const SPRITENUM_SPLASH_TITLE: u32 = 0x47;
const SPRITENUM_SPLASH_CRANE: u32 = 0x49;

// MIDI
const NUM_RIX_TITLE: u32 = 0x05;

const WIDTH: usize = 320;
const HEIGHT: usize = 200;

pub struct MKFs {
    pub rng: mkf::MKF,  // RNG动画
    pub pat: mkf::MKF,  // 调色板
    pub fbp: mkf::MKF,  // 战斗背景sprites
    pub mgo: mkf::MKF,  // 场景sprites
    pub midi: mkf::MKF, // MIDI音乐
    pub fp: mkf::MKF,   // 杂项数据文件
    pub map: mkf::MKF,  // 地图
    pub gop: mkf::MKF,  // tile bitmap
}

impl MKFs {
    pub fn open() -> Result<Self> {
        let rng = open_mkf("RNG.MKF")?;
        let pat = open_mkf("PAT.MKF")?;
        let fbp = open_mkf("FBP.MKF")?;
        let mgo = open_mkf("MGO.MKF")?;
        let midi = open_mkf("MIDI.MKF")?;
        let fp = open_mkf("DATA.MKF")?;
        let map = open_mkf("MAP.MKF")?;
        let gop = open_mkf("GOP.MKF")?;

        Ok(Self {
            rng,
            pat,
            fbp,
            mgo,
            midi,
            fp,
            map,
            gop,
        })
    }
}

pub struct Game {
    pub window: Window,
    pub canvas: Canvas,
    pub text: Text,
    pub input: InputState,

    pub start_time: Instant, // for tick
    pub mkf: MKFs,
    pub ui_sprites: Vec<Sprite>,
}

impl Game {
    pub fn new() -> Result<Self> {
        let window = Window::new(
            "PAL95(DOS) - Rust Edition",
            WIDTH,
            HEIGHT,
            WindowOptions {
                resize: true,
                scale: minifb::Scale::X2,
                ..WindowOptions::default()
            },
        )?;

        let text = Text::load()?;
        let mkf = MKFs::open()?;

        Ok(Self {
            window,
            canvas: Canvas::new(WIDTH, HEIGHT),
            text,
            input: InputState::new(),
            start_time: Instant::now(),
            mkf,
            ui_sprites: Vec::new(),
        })
    }

    pub fn init(&mut self) -> Result<()> {
        self.init_ui()?;
        Ok(())
    }

    pub fn get_palette(&mut self, palette_id: u32) -> Result<Palette> {
        let buf = self.mkf.pat.read_chunk(palette_id)?;
        let mut colors = Vec::<Color>::with_capacity(256);
        for i in 0..256 {
            let r = buf[i * 3] << 2;
            let g = buf[i * 3 + 1] << 2;
            let b = buf[i * 3 + 2] << 2;

            colors.push(Color::from_rgb(r, g, b));
        }

        Ok(Palette::with_colors(colors))
    }

    pub fn set_palette(&mut self, palette_id: u32) -> Result<()> {
        let pal = self.get_palette(palette_id)?;
        self.canvas.set_palette(&pal);

        Ok(())
    }

    pub fn blit_to_screen(&mut self) -> Result<()> {
        self.window
            .update_with_buffer(self.canvas.get_buffer(), WIDTH, HEIGHT)?;

        Ok(())
    }

    fn trademark_screen(&mut self) -> Result<()> {
        self.play_rng(3, 6)?;
        Ok(())
    }

    fn splash_screen(&mut self) -> Result<()> {
        #[derive(Clone)]
        struct Crane {
            x: isize,
            y: isize,
            sprite_id: u32,
        }

        let pal = self.get_palette(1)?;
        let mut fadein_pal = Palette::new();

        // 开场的那个从下往上的山是由两个图片拼接的，一个在上面，一个在下面。尺寸是320x200
        let splash_down = self
            .mkf
            .fbp
            .read_chunk_decompressed(BITMAPNUM_SPLASH_DOWN)?;
        let splash_up = self.mkf.fbp.read_chunk_decompressed(BITMAPNUM_SPLASH_UP)?;
        let splash_title = self
            .mkf
            .mgo
            .read_chunk_decompressed(SPRITENUM_SPLASH_TITLE)?;
        let splash_crane = self
            .mkf
            .mgo
            .read_chunk_decompressed(SPRITENUM_SPLASH_CRANE)?;

        let mut crane_sprites = Vec::<Sprite>::new();
        for i in 0..8 {
            let crane_sprite = sprite_get_frame(&splash_crane, i)?;
            crane_sprites.push(crane_sprite);
        }

        let mut title_sprite = sprite_get_frame(&splash_title, 0)?;
        let title_height = title_sprite.height;
        title_sprite.height = 0;

        let mut cranes = Vec::<Crane>::with_capacity(8);
        for _ in 0..cranes.capacity() {
            cranes.push(Crane {
                x: (rand::random::<usize>() % 320 + 320) as isize,
                y: (rand::random::<usize>() % 80 + 80) as isize,
                sprite_id: rand::random::<u32>() % 8,
            });
        }

        let begin_time = Instant::now();
        let mut h_offset = 0;

        /*
        let chunk = self.midi_mkf.read_chunk()?;
        let rw = sdl2::rwops::RWops::from_bytes(&chunk)?;
        let music = rw.load_music()?;
        music.play(-1)?;
        */
        //self.play_midi(NUM_RIX_TITLE, -1)?;

        let mut i = 0;
        'running: loop {
            i += 1;
            let elapsed_time = Instant::now() - begin_time;

            if elapsed_time < Duration::from_millis(15000) {
                let ratio = elapsed_time.as_secs_f32() / 15_f32;
                for i in 0..256 {
                    let (r, g, b) = pal.colors[i].to_rgb();
                    fadein_pal.colors[i] = Color::from_rgb(
                        ((r as f32) * ratio) as u8,
                        ((g as f32) * ratio) as u8,
                        ((b as f32) * ratio) as u8,
                    );
                }

                self.canvas.set_palette(&fadein_pal);
            }

            if h_offset < 200 {
                h_offset += 1;
            }

            if i % 5 == 0 {
                for crane in cranes.iter_mut() {
                    crane.x -= 2;
                    crane.sprite_id = (crane.sprite_id + 1) % 8;
                }

                if title_sprite.height < title_height {
                    title_sprite.height += 3;
                    if title_sprite.height > title_height {
                        title_sprite.height = title_height
                    }
                }
            }

            self.canvas.set_pixels(|pixels: &mut [u8]| {
                pixels[0..h_offset * 320]
                    .copy_from_slice(&splash_up[(200 - h_offset) * 320..200 * 320]);
                pixels[h_offset * 320..200 * 320]
                    .copy_from_slice(&splash_down[0..((200 - h_offset) * 320)]);

                for crane in cranes.iter() {
                    let sprite = &crane_sprites[crane.sprite_id as usize];
                    draw_sprite(sprite, pixels, 320, 200, crane.x, crane.y);
                }
                draw_sprite(&title_sprite, pixels, 320, 200, 250, 5);
            });

            self.blit_to_screen()?;
            self.process_event();

            if self.input.is_any_pressed() {
                break 'running;
            }

            std::thread::sleep(Duration::from_millis(30));
        }

        Ok(())
    }

    fn opening_menu_screen(&mut self) -> Result<()> {
        self.set_palette(0)?;

        let menu_items = [
            MenuItem {
                value: 0,
                num_word: MAINMENU_LABEL_NEWGAME,
                enabled: true,
                x: 125,
                y: 95,
            },
            MenuItem {
                value: 1,
                num_word: MAINMENU_LABEL_LOADGAME,
                enabled: true,
                x: 125,
                y: 112,
            },
        ];

        let bg_bitmap = self
            .mkf
            .fbp
            .read_chunk_decompressed(MAINMENU_BACKGROUND_FBPNUM)?;

        'running: loop {
            self.canvas.set_pixels(|pixels: &mut [u8]| {
                pixels.copy_from_slice(&bg_bitmap);
            });

            let menu_selected = self.read_menu(&menu_items)?;
            if menu_selected == 1 {
                // 存档选择
                let mut menu_items = Vec::<MenuItem>::new();
                for i in 0..5 {
                    menu_items.push(MenuItem {
                        value: i + 1,
                        num_word: LOADMENU_LABEL_SLOT_FIRST + i as u32,
                        enabled: true,
                        x: 210,
                        y: 17 + 38 * i,
                    });
                    self.draw_signle_linebox_with_shadow(
                        Pos{x: 195, y: 7 + 38 * i as isize},
                        6,
                    );
                }
                
                self.read_menu(&menu_items)?;
            } else {
                break 'running;
            }            
        }

        Ok(())
    }

    pub fn run(&mut self) -> Result<()> {
        //self.trademark_screen()?;
        //self.splash_screen()?;
        //self.opening_menu_screen()?;
        self.mainloop()?;

        Ok(())
    }
}
