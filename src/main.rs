use sdl2::event;
use sdl2::event::Event;
use sdl2::pixels::Color;
use sdl2::surface::Surface;
use std::path;
use std::time::Duration;
use sdl2::pixels::PixelFormatEnum;
use sdl2::pixels::Palette;

mod mkf;
mod rng;

const BASE_PATH: &str = "/home/rocky/Code/Game/PAL95/";


/// Holds any kind of error.
pub type Error = Box<dyn std::error::Error>;
/// Holds a `Result` of any kind of error.
pub type Result<T> = std::result::Result<T, Error>;


struct Video {
    sdl_ctx: sdl2::Sdl,
    video_subsys: sdl2::VideoSubsystem,
    event_pump: sdl2::EventPump,
    canvas: sdl2::render::Canvas<sdl2::video::Window>,    
    surface: sdl2::surface::Surface<'static>,
    texture_creator: sdl2::render::TextureCreator<sdl2::video::WindowContext>,    
}

impl Video {
    fn init() -> Video {
        let sdl_ctx = sdl2::init().unwrap();
        let video_subsys = sdl_ctx.video().unwrap();

        let window = video_subsys.window("PAL95", 640, 480)
            .position_centered()
            .build()
            .unwrap();

        let canvas = window.into_canvas().build().unwrap();    
        let texture_creator = canvas.texture_creator();        
        let surface = Surface::new(320, 200, PixelFormatEnum::Index8).unwrap();
        let event_pump = sdl_ctx.event_pump().unwrap();

        Video {
            sdl_ctx,
            video_subsys,
            canvas,
            event_pump,
            surface,
            texture_creator,
        }
    }
}

struct Pal {
    video: Video,

    rng_mkf: mkf::MKF, // RNG动画
    pat_mkf: mkf::MKF, // 调色板
}


impl Pal {
    fn init() -> Pal {
        let rng_mkf = Pal::open_mkf("RNG.MKF").unwrap();
        let pat_mkf = Pal::open_mkf("PAT.MKF").unwrap();
        Pal {
            video: Video::init(),
            rng_mkf,
            pat_mkf,
        }
    }

    fn open_mkf(filename: &str) -> Result<mkf::MKF> {
        let filepath = path::Path::new(BASE_PATH).join(filename);
        let file = std::fs::File::open(filepath)?;
        let mkf = mkf::open(file)?;
    
        Ok(mkf)
    }

    fn set_palette(&mut self, palette_id: u32) -> Result<()> {
        let buf = self.pat_mkf.read_chunk(palette_id)?;
        let mut colors = [Color::RGB(0, 0, 0); 256];
        for i in 0..256 {
            let r = buf[i * 3] << 2;
            let g = buf[i * 3 + 1] << 2;
            let b = buf[i * 3 + 2] << 2;

            colors[i] = Color::RGB(r, g, b);
        }
        let palette = Palette::with_colors(&colors)?;
        self.video.surface.set_palette(&palette)?;

        Ok(())
    }

    fn play_rng(&mut self, palette_id: u32, rng_id: u32) -> Result<()> {
        self.set_palette(palette_id)?;

        let rng_frame_count = self.rng_mkf.read_rng_sub_count(rng_id)?;
        
        for i in 0..rng_frame_count {
            let rng = self.rng_mkf.read_rng_chunk(rng_id, i)?;
            self.video.surface.with_lock_mut(|pixels: &mut [u8]| {
                rng::decode_rng(&rng, pixels, i);
            });
            
            let texture = self.video.surface.as_texture(&self.video.texture_creator)?;
            self.video.canvas.copy(&texture, None, None)?;
            self.video.canvas.present();
            for event in self.video.event_pump.poll_iter() {
                match event {
                    Event::Quit {..} => {
                        return Ok(());
                    },
                    _ => {}
                }
            }

            ::std::thread::sleep(Duration::new(0, 3_000_000_000u32 / 60));
        }  

        Ok(())
    }

    fn trademark_screen(&mut self) -> Result<()> {
        self.play_rng(3, 6)?;
        Ok(())
    }

    fn run(&mut self) {
        self.trademark_screen().unwrap();
    }
}



fn main() {
    let mut pal = Pal::init();
    pal.run();
}
