use sdl2::event::Event;
use sdl2::mixer::LoaderRWops;
use sdl2::mixer::AUDIO_S16LSB;
use sdl2::pixels::Color;
use sdl2::pixels::Palette;
use sdl2::pixels::PixelFormatEnum;

use sdl2::surface::Surface;
use sdl2::TimerSubsystem;
use std::path;
use sdl2::mixer::InitFlag;

mod mkf;
mod rng;
mod sprite;
use sprite::*;

const BASE_PATH: &str = "/home/rocky/Code/Game/PAL95/";

const BITMAPNUM_SPLASH_UP: u32 = 0x26;
const BITMAPNUM_SPLASH_DOWN: u32 = 0x27;
const SPRITENUM_SPLASH_TITLE: u32 = 0x47;
const SPRITENUM_SPLASH_CRANE: u32 = 0x49;
const NUM_RIX_TITLE: u32 = 0x05;

fn open_mkf(filename: &str) -> Result<mkf::MKF> {
    let filepath = path::Path::new(BASE_PATH).join(filename);
    let file = std::fs::File::open(filepath)?;
    let mkf = mkf::open(file)?;

    Ok(mkf)
}

struct Pal {
    sdl: sdl2::Sdl,
    video: sdl2::VideoSubsystem,    
    timer: TimerSubsystem,

    event_pump: sdl2::EventPump,
    canvas: sdl2::render::Canvas<sdl2::video::Window>,
    texture_creator: sdl2::render::TextureCreator<sdl2::video::WindowContext>,

    rng_mkf: mkf::MKF,  // RNG动画
    pat_mkf: mkf::MKF,  // 调色板
    fbp_mkf: mkf::MKF,  // 战斗背景sprites
    mgo_mkf: mkf::MKF,  // 场景sprites
    midi_mkf: mkf::MKF, // MIDI音乐
}

impl Pal {
    fn init() -> Result<Pal> {
        let sdl_ctx = sdl2::init()?;
        let video_subsys = sdl_ctx.video()?;
        let timer_subsys = sdl_ctx.timer()?;
        let window = video_subsys
            .window("PAL95", 640, 480)
            .position_centered()
            .build()?;

        let canvas = window.into_canvas().build()?;
        let texture_creator = canvas.texture_creator();
        let event_pump = sdl_ctx.event_pump()?;

        let frequency = 44_100;
        let format = AUDIO_S16LSB; // signed 16 bit samples, in little-endian byte order
        let channels = 8; // Stereo
        let chunk_size = 1_024;
        sdl2::mixer::open_audio(frequency, format, channels, chunk_size)?;
        let _mixer_context = sdl2::mixer::init(InitFlag::MID)?;

        sdl2::mixer::allocate_channels(8);

        let rng_mkf = open_mkf("RNG.MKF")?;
        let pat_mkf = open_mkf("PAT.MKF")?;
        let fbp_mkf = open_mkf("FBP.MKF")?;
        let mgo_mkf = open_mkf("MGO.MKF")?;
        let midi_mkf = open_mkf("MIDI.MKF")?;

        Ok(Pal {
            sdl: sdl_ctx,
            video: video_subsys,
            timer: timer_subsys,

            canvas,
            event_pump,
            texture_creator,

            rng_mkf,
            pat_mkf,
            fbp_mkf,
            mgo_mkf,
            midi_mkf,
        })
    }

    fn get_colors(&mut self, palette_id: u32) -> Result<Vec<Color>> {
        let buf = self.pat_mkf.read_chunk(palette_id)?;
        let mut colors = Vec::with_capacity(256);
        for i in 0..256 {
            let r = buf[i * 3] << 2;
            let g = buf[i * 3 + 1] << 2;
            let b = buf[i * 3 + 2] << 2;

            colors.push(Color::RGB(r, g, b));
        }

        Ok(colors)
    }

    fn set_palette(&mut self, surface: &mut Surface, palette_id: u32) -> Result<()> {
        let colors = self.get_colors(palette_id)?;
        let palette = Palette::with_colors(&colors)?;
        surface.set_palette(&palette)?;

        Ok(())
    }

    fn create_surface() -> Result<Surface<'static>> {
        let surface = Surface::new(320, 200, PixelFormatEnum::Index8)?;
        Ok(surface)
    }

    /*
    fn play_midi(&mut self, midi_id: u32, loops: i32) -> Result<Music> {
        let chunk = self.midi_mkf.read_chunk(midi_id)?;
        let rw = sdl2::rwops::RWops::from_bytes(&chunk)?;
        let music= rw.load_music()?;

        Ok(music)
    }
    */

    fn play_rng(&mut self, palette_id: u32, rng_id: u32) -> Result<()> {
        let mut surface = Pal::create_surface()?;
        self.set_palette(&mut surface, palette_id)?;

        let rng_frame_count = self.rng_mkf.read_rng_sub_count(rng_id)?;

        for i in 0..rng_frame_count {
            let rng = self.rng_mkf.read_rng_chunk(rng_id, i)?;
            surface.with_lock_mut(|pixels: &mut [u8]| {
                rng::decode_rng(&rng, pixels, i);
            });

            let texture = surface.as_texture(&self.texture_creator)?;
            self.canvas.copy(&texture, None, None)?;
            self.canvas.present();
            for event in self.event_pump.poll_iter() {
                match event {
                    Event::Quit { .. } => {
                        return Ok(());
                    }
                    _ => {}
                }
            }

            self.timer.delay(30);
        }

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

        let mut surface = Pal::create_surface()?;

        let colors = self.get_colors(1)?;
        let mut fadein_colors = vec![Color::RGB(0, 0, 0); 256];

        // 开场的那个从下往上的山是由两个图片拼接的，一个在上面，一个在下面。尺寸是320x200
        let splash_down = self.fbp_mkf.read_chunk_decompressed(BITMAPNUM_SPLASH_DOWN)?;
        let splash_up = self.fbp_mkf.read_chunk_decompressed(BITMAPNUM_SPLASH_UP)?;
        let splash_title = self.mgo_mkf.read_chunk_decompressed(SPRITENUM_SPLASH_TITLE)?;
        let splash_crane = self.mgo_mkf.read_chunk_decompressed(SPRITENUM_SPLASH_CRANE)?;

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
        
        let begin_time = self.timer.ticks();
        let mut h_offset = 0;

        let chunk = self.midi_mkf.read_chunk(NUM_RIX_TITLE)?;
        let rw = sdl2::rwops::RWops::from_bytes(&chunk)?;
        let music= rw.load_music()?;
        music.play(-1)?;

        let mut i = 0;
        'running: loop {
            i += 1;
            let elapsed_time = self.timer.ticks() - begin_time;

            if elapsed_time < 15000 {
                let ratio = elapsed_time as f32 / 15000_f32;
                for i in 0..256 {
                    fadein_colors[i] = Color::RGB(
                        ((colors[i].r as f32) * ratio) as u8,
                        ((colors[i].g as f32) * ratio) as u8,
                        ((colors[i].b as f32) * ratio) as u8,
                    );
                }
                let palette = Palette::with_colors(&fadein_colors)?;
                surface.set_palette(&palette)?;
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

            surface.with_lock_mut(|pixels: &mut [u8]| {
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

            let texture = surface.as_texture(&self.texture_creator)?;
            self.canvas.copy(&texture, None, None)?;
            self.canvas.present();

            for event in self.event_pump.poll_iter() {
                match event {
                    Event::KeyDown { .. } => break 'running,
                    Event::Quit { .. } => break 'running,
                    _ => {}
                }
            }

            self.timer.delay(30);
        }

        Ok(())
    }

    fn run(&mut self) -> Result<()> {
        self.trademark_screen()?;
        self.splash_screen()?;
        Ok(())

        /*
        loop {
            for event in self.event_pump.poll_iter() {
                match event {
                    Event::Quit {..} => {
                        return;
                    },
                    _ => {}
                }
            }
        }
        */
    }
}



fn main() {
    let mut pal = Pal::init().unwrap();
    pal.run().unwrap();
}
