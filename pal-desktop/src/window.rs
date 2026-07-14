//! winit + pixels 窗口渲染

use pal_assets::bitmap::Bitmap;
use pixels::{Pixels, SurfaceTexture};
use winit::dpi::LogicalSize;
use winit::event::{Event, WindowEvent};
use winit::event_loop::EventLoop;
use winit::window::WindowBuilder;

/// 运行渲染窗口
pub fn run(title: &str, width: u32, height: u32, bitmap: &Bitmap, scale: u32) {
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title(title)
        .with_inner_size(LogicalSize::new(width as f64, height as f64))
        .build(&event_loop)
        .unwrap();

    let window_size = window.inner_size();
    let surface_texture = SurfaceTexture::new(window_size.width, window_size.height, &window);
    let mut pixels = Pixels::new(width, height, surface_texture).unwrap();

    // 将位图按比例放大渲染到像素缓冲区
    {
        let frame = pixels.frame_mut();
        let scale = scale as usize;

        for y in 0..bitmap.height as usize {
            for x in 0..bitmap.width as usize {
                let src_idx = (y * bitmap.width as usize + x) * 4;
                let r = bitmap.rgba[src_idx];
                let g = bitmap.rgba[src_idx + 1];
                let b = bitmap.rgba[src_idx + 2];
                let a = bitmap.rgba[src_idx + 3];

                for sy in 0..scale {
                    for sx in 0..scale {
                        let dy = y * scale + sy;
                        let dx = x * scale + sx;
                        if dx < width as usize && dy < height as usize {
                            let dst_idx = (dy * width as usize + dx) * 4;
                            frame[dst_idx] = r;
                            frame[dst_idx + 1] = g;
                            frame[dst_idx + 2] = b;
                            frame[dst_idx + 3] = a;
                        }
                    }
                }
            }
        }
    }

    if let Err(e) = pixels.render() {
        eprintln!("Render error: {}", e);
        return;
    }

    event_loop.run(move |event, _target| {
        if let Event::WindowEvent { event: WindowEvent::CloseRequested, .. } = event {
            std::process::exit(0);
        }
    });
}
