//! Native window and map framebuffer presentation.

use crate::renderer::Renderer;
use pal_core::map::{Map, MAP_COLUMNS, MAP_ROWS};
use pixels::{Pixels, SurfaceTexture};
use winit::dpi::LogicalSize;
use winit::event::{Event, WindowEvent};
use winit::event_loop::EventLoop;
use winit::window::WindowBuilder;

pub fn run_game_window(
    mut renderer: Renderer,
    map: Map,
    width: u32,
    height: u32,
    scroll_x: i32,
    scroll_y: i32,
) {
    let event_loop = EventLoop::new().expect("failed to create event loop");
    let window = Box::leak(Box::new(
        WindowBuilder::new()
            .with_title("Rust-PAL")
            .with_inner_size(LogicalSize::new(width as f64 * 2.0, height as f64 * 2.0))
            .with_min_inner_size(LogicalSize::new(width as f64, height as f64))
            .build(&event_loop)
            .expect("failed to create window"),
    ));

    let size = window.inner_size();
    let surface = SurfaceTexture::new(size.width, size.height, &*window);
    let mut pixels = Pixels::new(width, height, surface).expect("failed to create pixel surface");
    render_tile_map(&mut renderer, &map, scroll_x, scroll_y, width, height);

    event_loop
        .run(move |event, target| match event {
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => target.exit(),
                WindowEvent::Resized(size) if size.width > 0 && size.height > 0 => {
                    if let Err(error) = pixels.resize_surface(size.width, size.height) {
                        eprintln!("surface resize failed: {error}");
                        target.exit();
                    }
                }
                WindowEvent::RedrawRequested => {
                    pixels.frame_mut().copy_from_slice(renderer.screen());
                    if let Err(error) = pixels.render() {
                        eprintln!("render failed: {error}");
                        target.exit();
                    } else {
                        renderer.mark_cleaned();
                    }
                }
                _ => {}
            },
            Event::AboutToWait if renderer.is_dirty() => window.request_redraw(),
            _ => {}
        })
        .expect("event loop failed");
}

/// Render the visible map layers into the framebuffer.
pub fn render_tile_map(
    renderer: &mut Renderer,
    map: &Map,
    scroll_x: i32,
    scroll_y: i32,
    view_width: u32,
    view_height: u32,
) {
    renderer.clear_black();

    let start_y = scroll_y.div_euclid(16) - 1;
    let end_y = (scroll_y + view_height as i32).div_euclid(16) + 2;
    let start_x = scroll_x.div_euclid(32) - 1;
    let end_x = (scroll_x + view_width as i32).div_euclid(32) + 2;

    for top in [false, true] {
        for y in start_y..end_y {
            for h in 0..2i32 {
                let screen_y = y * 16 + h * 8 - 8 - scroll_y;
                for x in start_x..end_x {
                    let (Ok(x_index), Ok(y_index)) = (usize::try_from(x), usize::try_from(y))
                    else {
                        continue;
                    };
                    if x_index >= MAP_COLUMNS || y_index >= MAP_ROWS {
                        continue;
                    }

                    let bitmap = if top {
                        map.decode_top_tile(x_index, y_index, h as usize)
                    } else {
                        map.decode_bottom_tile(x_index, y_index, h as usize)
                    };
                    if let Some(bitmap) = bitmap {
                        let screen_x = x * 32 + h * 16 - 16 - scroll_x;
                        renderer.blit_rle(&bitmap, screen_x, screen_y);
                    }
                }
            }
        }
    }
}
