//! Standalone, read-only scene and script inspector.

mod app;
mod hit_test;
mod navigation;
mod panel;

use pal_core::map::{MAP_PIXEL_HEIGHT, MAP_PIXEL_WIDTH};
use pixels::{Pixels, SurfaceTexture};
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Event, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::WindowBuilder;

use self::app::SceneEditorApp;
use super::{LoadedScene, SceneEditorResources};
use crate::renderer::Renderer;

pub const SCENE_EDITOR_WIDTH: u32 = 800;
pub const SCENE_EDITOR_HEIGHT: u32 = 450;

pub(super) const CANVAS_WIDTH: u32 = 480;
pub(super) const PANEL_X: i32 = CANVAS_WIDTH as i32;
pub(super) const PANEL_WIDTH: i32 = SCENE_EDITOR_WIDTH as i32 - PANEL_X;
pub(super) const LINE_HEIGHT: i32 = 10;
pub(super) const OBJECT_FIRST_LINE: usize = 9;
pub(super) const OBJECT_ROWS: usize = 8;
pub(super) const OBJECT_LAST_LINE: usize = OBJECT_FIRST_LINE + OBJECT_ROWS;
pub(super) const CODE_FIRST_LINE: usize = 26;
pub(super) const CODE_ROWS: usize = 11;
pub(super) const CODE_LAST_LINE: usize = CODE_FIRST_LINE + CODE_ROWS;
pub(super) const REFERENCE_FIRST_LINE: usize = 40;
pub(super) const REFERENCE_ROWS: usize = 3;
pub(super) const REFERENCE_LAST_LINE: usize = REFERENCE_FIRST_LINE + REFERENCE_ROWS;
pub(super) const MAX_ZOOM: u8 = 3;

pub(super) const PANEL_BACKGROUND: [u8; 4] = [12, 16, 22, 255];
pub(super) const PANEL_BORDER: [u8; 4] = [76, 92, 104, 255];

/// Run the scene inspector on a separate event loop from the normal game.
pub fn run_scene_editor_window<L>(
    renderer: Renderer,
    initial_scene: LoadedScene,
    resources: SceneEditorResources,
    load_scene: L,
) where
    L: FnMut(u16, &pal_core::role::RoleSprites) -> Option<LoadedScene> + 'static,
{
    let event_loop = EventLoop::new().expect("failed to create scene editor event loop");
    let window = Box::leak(Box::new(
        WindowBuilder::new()
            .with_title("Rust-PAL Scene Inspector")
            .with_inner_size(LogicalSize::new(
                f64::from(SCENE_EDITOR_WIDTH),
                f64::from(SCENE_EDITOR_HEIGHT),
            ))
            .with_min_inner_size(LogicalSize::new(640.0, 360.0))
            .build(&event_loop)
            .expect("failed to create scene editor window"),
    ));
    let size = window.inner_size();
    let surface = SurfaceTexture::new(size.width, size.height, &*window);
    let mut pixels = Pixels::new(SCENE_EDITOR_WIDTH, SCENE_EDITOR_HEIGHT, surface)
        .expect("failed to create scene editor pixel surface");
    let mut app = SceneEditorApp::new(renderer, initial_scene, resources, load_scene);
    app.render();
    window.set_title(&app.title());

    event_loop
        .run(move |event, target| match event {
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => target.exit(),
                WindowEvent::Focused(false) => app.reset_pointer(),
                WindowEvent::KeyboardInput { event, .. }
                    if event.state == ElementState::Pressed =>
                {
                    if let PhysicalKey::Code(code) = event.physical_key {
                        if app.handle_key(code) {
                            target.exit();
                        } else {
                            window.set_title(&app.title());
                            window.request_redraw();
                        }
                    }
                }
                WindowEvent::CursorMoved { position, .. } => {
                    let pixel = pixels
                        .window_pos_to_pixel((position.x as f32, position.y as f32))
                        .unwrap_or_else(|position| pixels.clamp_pixel_pos(position));
                    app.cursor_moved((pixel.0 as i32, pixel.1 as i32));
                    if app.is_dirty() {
                        window.request_redraw();
                    }
                }
                WindowEvent::MouseInput {
                    state,
                    button: MouseButton::Left,
                    ..
                } => {
                    if state == ElementState::Pressed {
                        app.pointer_pressed();
                    } else {
                        app.pointer_released();
                    }
                    window.set_title(&app.title());
                    window.request_redraw();
                }
                WindowEvent::MouseWheel { delta, .. } => {
                    let amount = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y.signum() as i32,
                        MouseScrollDelta::PixelDelta(position) => position.y.signum() as i32,
                    };
                    app.mouse_wheel(amount);
                    window.request_redraw();
                }
                WindowEvent::Resized(size) if size.width > 0 && size.height > 0 => {
                    if let Err(error) = pixels.resize_surface(size.width, size.height) {
                        eprintln!("scene editor surface resize failed: {error}");
                        target.exit();
                    } else {
                        window.request_redraw();
                    }
                }
                WindowEvent::RedrawRequested => {
                    if app.is_dirty() {
                        app.render();
                    }
                    pixels.frame_mut().copy_from_slice(app.screen());
                    if let Err(error) = pixels.render() {
                        eprintln!("scene editor render failed: {error}");
                        target.exit();
                    }
                }
                _ => {}
            },
            Event::AboutToWait => target.set_control_flow(ControlFlow::Wait),
            _ => {}
        })
        .expect("scene editor event loop failed");
}

pub(super) fn canvas_view_size(canvas: u32, zoom: u8) -> u32 {
    canvas.div_ceil(u32::from(zoom.max(1)))
}

pub(super) fn clamped_viewport(x: i32, y: i32, width: u32, height: u32) -> (i32, i32) {
    let max_x = (MAP_PIXEL_WIDTH - width as i32).max(0);
    let max_y = (MAP_PIXEL_HEIGHT - height as i32).max(0);
    (x.clamp(0, max_x), y.clamp(0, max_y))
}

pub(super) fn scale_canvas(renderer: &mut Renderer, zoom: u8) {
    if zoom <= 1 {
        return;
    }
    let source = renderer.screen().to_vec();
    let source_stride = renderer.width * 4;
    let output_stride = renderer.width * 4;
    let output = renderer.screen_mut();
    for y in 0..SCENE_EDITOR_HEIGHT as usize {
        let source_y = y / usize::from(zoom);
        for x in 0..CANVAS_WIDTH as usize {
            let source_x = x / usize::from(zoom);
            let source_index = source_y * source_stride + source_x * 4;
            let output_index = y * output_stride + x * 4;
            output[output_index..output_index + 4]
                .copy_from_slice(&source[source_index..source_index + 4]);
        }
    }
}

pub(super) fn key_digit(code: KeyCode) -> Option<char> {
    match code {
        KeyCode::Digit0 | KeyCode::Numpad0 => Some('0'),
        KeyCode::Digit1 | KeyCode::Numpad1 => Some('1'),
        KeyCode::Digit2 | KeyCode::Numpad2 => Some('2'),
        KeyCode::Digit3 | KeyCode::Numpad3 => Some('3'),
        KeyCode::Digit4 | KeyCode::Numpad4 => Some('4'),
        KeyCode::Digit5 | KeyCode::Numpad5 => Some('5'),
        KeyCode::Digit6 | KeyCode::Numpad6 => Some('6'),
        KeyCode::Digit7 | KeyCode::Numpad7 => Some('7'),
        KeyCode::Digit8 | KeyCode::Numpad8 => Some('8'),
        KeyCode::Digit9 | KeyCode::Numpad9 => Some('9'),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_and_key_helpers_clamp_to_valid_ranges() {
        assert_eq!(clamped_viewport(-10, -20, 480, 450), (0, 0));
        assert_eq!(
            clamped_viewport(i32::MAX, i32::MAX, 480, 450),
            (MAP_PIXEL_WIDTH - 480, MAP_PIXEL_HEIGHT - 450)
        );
        assert_eq!(canvas_view_size(480, 3), 160);
        assert_eq!(key_digit(KeyCode::Digit7), Some('7'));
        assert_eq!(key_digit(KeyCode::KeyA), None);
    }
}
