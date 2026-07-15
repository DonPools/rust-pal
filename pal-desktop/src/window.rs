//! Native window and map framebuffer presentation.

use std::time::{Duration, Instant};

use crate::renderer::Renderer;
use pal_core::game::{Camera, GameInput, GameState, UPDATE_INTERVAL_MS};
use pal_core::map::{Map, MAP_COLUMNS, MAP_ROWS};
use pal_core::role::{Direction, Role, RoleSprites};
use pixels::{Pixels, SurfaceTexture};
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::WindowBuilder;

#[derive(Debug, Clone, Copy)]
pub struct Viewport {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Viewport {
    pub fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

impl From<Camera> for Viewport {
    fn from(camera: Camera) -> Self {
        Self::new(camera.x, camera.y, camera.width, camera.height)
    }
}

#[derive(Default)]
struct HeldInput {
    south: bool,
    west: bool,
    north: bool,
    east: bool,
    active_direction: Option<Direction>,
    confirm: bool,
    cancel: bool,
}

impl HeldInput {
    fn set_key(&mut self, key: KeyCode, pressed: bool) {
        let direction = match key {
            KeyCode::ArrowDown | KeyCode::KeyS => Some(Direction::South),
            KeyCode::ArrowLeft | KeyCode::KeyA => Some(Direction::West),
            KeyCode::ArrowUp | KeyCode::KeyW => Some(Direction::North),
            KeyCode::ArrowRight | KeyCode::KeyD => Some(Direction::East),
            _ => None,
        };
        if let Some(direction) = direction {
            *self.direction_held_mut(direction) = pressed;
            if pressed {
                self.active_direction = Some(direction);
            } else if self.active_direction == Some(direction) {
                self.active_direction = self.first_held_direction();
            }
        }

        match key {
            KeyCode::Enter | KeyCode::Space => self.confirm = pressed,
            KeyCode::Escape | KeyCode::Backspace => self.cancel = pressed,
            _ => {}
        }
    }

    fn sample(&self) -> GameInput {
        GameInput {
            direction: self.active_direction,
            confirm: self.confirm,
            cancel: self.cancel,
        }
    }

    fn direction_held_mut(&mut self, direction: Direction) -> &mut bool {
        match direction {
            Direction::South => &mut self.south,
            Direction::West => &mut self.west,
            Direction::North => &mut self.north,
            Direction::East => &mut self.east,
        }
    }

    fn first_held_direction(&self) -> Option<Direction> {
        [
            (Direction::South, self.south),
            (Direction::West, self.west),
            (Direction::North, self.north),
            (Direction::East, self.east),
        ]
        .into_iter()
        .find_map(|(direction, held)| held.then_some(direction))
    }
}

pub fn run_game_window(mut renderer: Renderer, mut game: GameState, role_sprites: RoleSprites) {
    let viewport = Viewport::from(game.camera);
    let event_loop = EventLoop::new().expect("failed to create event loop");
    let window = Box::leak(Box::new(
        WindowBuilder::new()
            .with_title("Rust-PAL")
            .with_inner_size(LogicalSize::new(
                viewport.width as f64 * 2.0,
                viewport.height as f64 * 2.0,
            ))
            .with_min_inner_size(LogicalSize::new(
                viewport.width as f64,
                viewport.height as f64,
            ))
            .build(&event_loop)
            .expect("failed to create window"),
    ));

    let size = window.inner_size();
    let surface = SurfaceTexture::new(size.width, size.height, &*window);
    let mut pixels = Pixels::new(viewport.width, viewport.height, surface)
        .expect("failed to create pixel surface");
    render_game(&mut renderer, &game, &role_sprites);

    let tick = Duration::from_millis(UPDATE_INTERVAL_MS);
    let mut last_update = Instant::now();
    let mut accumulator = Duration::ZERO;
    let mut input = HeldInput::default();

    event_loop
        .run(move |event, target| match event {
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => target.exit(),
                WindowEvent::Focused(false) => input = HeldInput::default(),
                WindowEvent::KeyboardInput { event, .. } => {
                    if let PhysicalKey::Code(code) = event.physical_key {
                        input.set_key(code, event.state == ElementState::Pressed);
                    }
                }
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
            Event::AboutToWait => {
                let now = Instant::now();
                accumulator += now
                    .duration_since(last_update)
                    .min(Duration::from_millis(250));
                last_update = now;

                let mut changed = false;
                while accumulator >= tick {
                    changed |= game.update(input.sample());
                    accumulator -= tick;
                }
                if changed {
                    render_game(&mut renderer, &game, &role_sprites);
                }
                if renderer.is_dirty() {
                    window.request_redraw();
                }
                target.set_control_flow(ControlFlow::WaitUntil(now + (tick - accumulator)));
            }
            _ => {}
        })
        .expect("event loop failed");
}

fn render_game(renderer: &mut Renderer, game: &GameState, role_sprites: &RoleSprites) {
    render_tile_map(
        renderer,
        &game.map,
        Some(role_sprites),
        std::slice::from_ref(&game.player),
        Viewport::from(game.camera),
    );
}

/// Render bottom tiles, scene characters, then covering top tiles.
pub fn render_tile_map(
    renderer: &mut Renderer,
    map: &Map,
    role_sprites: Option<&RoleSprites>,
    roles: &[Role],
    viewport: Viewport,
) {
    renderer.clear_black();
    let bounds = RenderBounds::for_viewport(viewport);
    render_tile_layer(renderer, map, viewport, bounds, false);
    render_roles(renderer, role_sprites, roles, viewport, bounds);
    render_tile_layer(renderer, map, viewport, bounds, true);
}

#[derive(Clone, Copy)]
struct RenderBounds {
    start_x: i32,
    end_x: i32,
    start_y: i32,
    end_y: i32,
}

impl RenderBounds {
    fn for_viewport(viewport: Viewport) -> Self {
        Self {
            start_x: viewport.x.div_euclid(32) - 1,
            end_x: (viewport.x + viewport.width as i32).div_euclid(32) + 2,
            start_y: viewport.y.div_euclid(16) - 1,
            end_y: (viewport.y + viewport.height as i32).div_euclid(16) + 2,
        }
    }
}

fn render_roles(
    renderer: &mut Renderer,
    role_sprites: Option<&RoleSprites>,
    roles: &[Role],
    viewport: Viewport,
    bounds: RenderBounds,
) {
    let Some(sprites) = role_sprites else {
        return;
    };
    for role in roles {
        let (anchor_x, anchor_y) = role.screen_anchor();
        if anchor_y < bounds.start_y * 16 || anchor_y >= bounds.end_y * 16 {
            continue;
        }
        if let Some(bitmap) = sprites.decode_role_frame(role) {
            renderer.blit_rle(
                &bitmap,
                anchor_x - bitmap.width as i32 / 2 - viewport.x,
                anchor_y - bitmap.height as i32 - viewport.y,
            );
        }
    }
}

fn render_tile_layer(
    renderer: &mut Renderer,
    map: &Map,
    viewport: Viewport,
    bounds: RenderBounds,
    top: bool,
) {
    for y in bounds.start_y..bounds.end_y {
        for h in 0..2i32 {
            render_tile_row(renderer, map, viewport, bounds, y, h, top);
        }
    }
}

fn render_tile_row(
    renderer: &mut Renderer,
    map: &Map,
    viewport: Viewport,
    bounds: RenderBounds,
    y: i32,
    h: i32,
    top: bool,
) {
    for x in bounds.start_x..bounds.end_x {
        let (Ok(x_index), Ok(y_index)) = (usize::try_from(x), usize::try_from(y)) else {
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
            renderer.blit_rle(
                &bitmap,
                x * 32 + h * 16 - 16 - viewport.x,
                y * 16 + h * 8 - 8 - viewport.y,
            );
        }
    }
}
