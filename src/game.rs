use std::time::Duration;
use std::time::Instant;

use minifb::{Key, Window, WindowOptions};

use crate::canvas::*;
use crate::input::InputState;
use crate::input::PalKey;
use crate::mkf;
use crate::sprite::*;
use crate::textmgr::*;
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

#[derive(PartialEq)]
pub enum GameState {
    TRADEMARK,
    SPLASH,
    OPENINGMENU,
    GAMELOOP,
}

pub struct Game {
    pub window: Window,
    pub canvas: Canvas,
    pub text: TextMgr,
    pub input_state: InputState,

    pub start_time: Instant,
    pub game_state: GameState,

    pub rng_mkf: mkf::MKF,  // RNG动画
    pub pat_mkf: mkf::MKF,  // 调色板
    pub fbp_mkf: mkf::MKF,  // 战斗背景sprites
    pub mgo_mkf: mkf::MKF,  // 场景sprites
    pub midi_mkf: mkf::MKF, // MIDI音乐
}

impl Game {
    pub fn init() -> Result<Self> {
        let window = Window::new(
            "PAL95 - Rust Edition",
            WIDTH,
            HEIGHT,
            WindowOptions {
                resize: true,
                scale: minifb::Scale::X2,
                ..WindowOptions::default()
            },
        )?;

        let text = TextMgr::load()?;

        let rng_mkf = open_mkf("RNG.MKF")?;
        let pat_mkf = open_mkf("PAT.MKF")?;
        let fbp_mkf = open_mkf("FBP.MKF")?;
        let mgo_mkf = open_mkf("MGO.MKF")?;
        let midi_mkf = open_mkf("MIDI.MKF")?;

        Ok(Self {
            window,
            canvas: Canvas::new(WIDTH, HEIGHT),
            text,
            input_state: InputState::new(),
            start_time: Instant::now(),

            game_state: GameState::TRADEMARK,

            rng_mkf,
            pat_mkf,
            fbp_mkf,
            mgo_mkf,
            midi_mkf,
        })
    }

    pub fn get_palette(&mut self, palette_id: u32) -> Result<Palette> {
        let buf = self.pat_mkf.read_chunk(palette_id)?;
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
        self.game_state = GameState::SPLASH;
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
            .fbp_mkf
            .read_chunk_decompressed(BITMAPNUM_SPLASH_DOWN)?;
        let splash_up = self.fbp_mkf.read_chunk_decompressed(BITMAPNUM_SPLASH_UP)?;
        let splash_title = self
            .mgo_mkf
            .read_chunk_decompressed(SPRITENUM_SPLASH_TITLE)?;
        let splash_crane = self
            .mgo_mkf
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

            if self.input_state.is_any_pressed() {                
                self.game_state = GameState::OPENINGMENU;
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
            .fbp_mkf
            .read_chunk_decompressed(MAINMENU_BACKGROUND_FBPNUM)?;

        loop {
            self.canvas.set_pixels(|pixels: &mut [u8]| {
                pixels.copy_from_slice(&bg_bitmap);
            });

            let menu_selected = self.read_menu(&menu_items)?;
            if menu_selected == 1 {
                let mut menu_items = Vec::<MenuItem>::new();
                for i in 0..5 {
                    menu_items.push(MenuItem {
                        value: i + 1,
                        num_word: LOADMENU_LABEL_SLOT_FIRST + i as u32,
                        enabled: true,
                        x: 210,
                        y: 17 + 38 * i,
                    });
                }
                self.read_menu(&menu_items)?;
            }
        }

        self.game_state = GameState::GAMELOOP;

        Ok(())
    }

    pub fn run(&mut self) -> Result<()> {
        loop {
            match self.game_state {
                GameState::TRADEMARK => self.trademark_screen()?,
                GameState::SPLASH => self.splash_screen()?,
                GameState::OPENINGMENU => self.opening_menu_screen()?,
                _ => break,
            }
        }

        Ok(())
    }
}
