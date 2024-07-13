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

fn open_mkf(filename: &str) -> Result<mkf::MKF, std::io::Error> {
    let filepath = path::Path::new(BASE_PATH).join(filename);
    let file = std::fs::File::open(filepath)?;
    mkf::open(file)
}

fn main() {
    let mut pat_mkf = open_mkf("PAT.MKF").unwrap();
    let buf = pat_mkf.read_chunk(3).unwrap();
    let mut colors = [Color::RGB(0, 0, 0); 256];
    for i in 0..256 {
        let r = buf[i * 3] << 2;
        let g = buf[i * 3 + 1] << 2;
        let b = buf[i * 3 + 2] << 2;

        colors[i] = Color::RGB(r, g, b);
    }
 
    let mut rng_mkf = open_mkf("RNG.MKF").unwrap();
    let palette = Palette::with_colors(&colors).unwrap();
    let mut surface = Surface::new(320, 200, PixelFormatEnum::Index8).unwrap();
    surface.set_palette(&palette).unwrap();

    let sdl_ctx = sdl2::init().unwrap();
    let video_subsys = sdl_ctx.video().unwrap();

    let window = video_subsys.window("PAL95", 640, 480)
        .position_centered()
        .build()
        .unwrap();

    let mut canvas = window.into_canvas().build().unwrap();    
    let texture_creator = canvas.texture_creator();        
    let mut event_pump = sdl_ctx.event_pump().unwrap();
    
    
    let rng_frame_count = rng_mkf.read_rng_sub_count(6).unwrap();    
    
    for i in 0..rng_frame_count {
        let rng = rng_mkf.read_rng_chunk(6, i).unwrap();
        surface.with_lock_mut(|pixels: &mut [u8]| {
            rng::decode_rng(&rng, pixels, i);
        });
        
        let texture = surface.as_texture(&texture_creator).unwrap();
        canvas.copy(&texture, None, None).unwrap();
        canvas.present();

        ::std::thread::sleep(Duration::new(0, 3_000_000_000u32 / 60));
    }  

    'running: loop {
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit {..} => {
                    break 'running
                },
                _ => {}
            }
        }

        ::std::thread::sleep(Duration::new(0, 1_000_000_000u32 / 60));
    }

}
