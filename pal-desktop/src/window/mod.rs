//! Native desktop window integration and presentation facade.

use std::time::Instant;

use crate::debug_overlay::DebugOverlay;
use crate::renderer::Renderer;
use pal_core::game::GameState;
use pal_core::role::RoleSprites;
use pixels::{Pixels, SurfaceTexture};
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::window::{Window, WindowBuilder};

use app::DesktopApp;
use battle_debug_overlay::BattleDebugOverlay;
use game_viewport::{GameSurfaceLayout, GameViewportRenderer, BATTLE_ASSIST_LOGICAL_WIDTH};
use minimap::MiniMapOverlay;

mod app;
mod ascii_font;
mod battle_debug_overlay;
mod battle_render;
mod battle_timing;
mod battle_update;
mod clock;
mod debug_render;
mod dialog;
mod dialog_text;
mod draw;
mod game_viewport;
mod input;
mod menu_render;
mod menu_state;
mod menu_update;
mod minimap;
mod opening_animation;
mod original_save;
mod presentation;
#[cfg(test)]
mod regression_tests;
mod scene_render;
mod script_driver;
mod session;
mod snapshot;
mod state;
mod text_render;
mod types;
mod visual;

pub use battle_render::{render_battle, BattleMenuState, BattleRenderResources, BattleRenderState};
pub use scene_render::render_tile_map;
pub use types::{GameResources, LoadedScene, Viewport};

pub(super) const UI_TIME_QUANTUM_MS: u64 = 10;
pub fn run_game_window<L>(
    renderer: Renderer,
    game: GameState,
    resources: GameResources,
    load_scene: L,
) where
    L: FnMut(u16, Option<u16>, &RoleSprites) -> Option<LoadedScene> + 'static,
{
    let viewport = Viewport::from(game.camera);
    let event_loop = EventLoop::new().expect("failed to create event loop");
    // The pixels surface borrows the window for the process-long winit loop.
    // Leaking this single top-level window makes that lifetime explicit.
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
    let game_viewport_renderer = GameViewportRenderer::new(
        pixels.device(),
        pixels.texture(),
        pixels.surface_texture_format(),
    );
    let mut debug_overlay = DebugOverlay::new(
        pixels.device(),
        pixels.surface_texture_format(),
        window.scale_factor(),
    );
    let mut minimap_overlay = MiniMapOverlay::new(
        pixels.device(),
        pixels.surface_texture_format(),
        window.scale_factor(),
    );
    let mut battle_debug_overlay = BattleDebugOverlay::new(
        pixels.device(),
        pixels.surface_texture_format(),
        window.scale_factor(),
    );
    let mut app = DesktopApp::new(renderer, game, resources, load_scene);
    let mut battle_assist_reserved_scale = None;
    app.render_frame(0);

    event_loop
        .run(move |event, target| match event {
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => target.exit(),
                WindowEvent::Focused(false) => app.reset_input(),
                WindowEvent::KeyboardInput { event, .. } => {
                    if let PhysicalKey::Code(code) = event.physical_key {
                        let pressed = event.state == ElementState::Pressed;
                        app.handle_key_event(code, pressed, event.repeat, &mut |title| {
                            window.set_title(title)
                        });
                    }
                }
                WindowEvent::Resized(size) if size.width > 0 && size.height > 0 => {
                    if let Err(error) = pixels.resize_surface(size.width, size.height) {
                        eprintln!("surface resize failed: {error}");
                        target.exit();
                    } else {
                        window.request_redraw();
                    }
                }
                WindowEvent::RedrawRequested => {
                    pixels.frame_mut().copy_from_slice(app.screen());
                    let surface_size = window.inner_size();
                    let reserved_game_scale =
                        if window.fullscreen().is_none() && !window.is_maximized() {
                            battle_assist_reserved_scale
                        } else {
                            None
                        };
                    let surface_layout = GameSurfaceLayout::new(
                        surface_size.width,
                        surface_size.height,
                        viewport.width,
                        viewport.height,
                        window.scale_factor(),
                        app.battle_assist_requested(),
                        reserved_game_scale,
                    );
                    let show_minimap = app
                        .minimap_frame(
                            window.scale_factor(),
                            surface_size.width,
                            surface_size.height,
                        )
                        .map(|frame| {
                            minimap_overlay.update(pixels.device(), pixels.queue(), frame);
                            true
                        })
                        .unwrap_or(false);
                    if app.show_script_debug() {
                        debug_overlay.update(
                            pixels.device(),
                            pixels.queue(),
                            app.debug_snapshot(
                                surface_size.width,
                                surface_size.height,
                                window.scale_factor(),
                            ),
                        );
                    }
                    let show_battle_assist = surface_layout
                        .battle_assist
                        .and_then(|panel| {
                            app.battle_debug_snapshot(
                                panel.width,
                                panel.height,
                                window.scale_factor(),
                            )
                            .map(|snapshot| (panel, snapshot))
                        })
                        .map(|(panel, snapshot)| {
                            battle_debug_overlay.update(pixels.device(), pixels.queue(), snapshot);
                            panel
                        });
                    let render_result = pixels.render_with(|encoder, render_target, context| {
                        if show_battle_assist.is_some() {
                            game_viewport_renderer.render(
                                encoder,
                                render_target,
                                surface_layout.game,
                            );
                        } else {
                            context.scaling_renderer.render(encoder, render_target);
                        }
                        if show_minimap {
                            minimap_overlay.render(
                                encoder,
                                render_target,
                                surface_size.width,
                                surface_size.height,
                            );
                        }
                        if app.show_script_debug() {
                            debug_overlay.render(
                                encoder,
                                render_target,
                                surface_size.width,
                                surface_size.height,
                            );
                        }
                        if let Some(panel) = show_battle_assist {
                            battle_debug_overlay.render(encoder, render_target, panel);
                        }
                        Ok(())
                    });
                    if let Err(error) = render_result {
                        eprintln!("render failed: {error}");
                        target.exit();
                    } else {
                        app.mark_presented();
                    }
                }
                _ => {}
            },
            Event::AboutToWait => {
                let control = app.advance(Instant::now(), &mut |title| window.set_title(title));
                battle_assist_reserved_scale = reserve_battle_assist_width(
                    window,
                    app.battle_assist_requested(),
                    battle_assist_reserved_scale,
                    viewport.width,
                    viewport.height,
                );
                if control.exit_requested {
                    target.exit();
                }
                if app.is_dirty() {
                    window.request_redraw();
                }
                target.set_control_flow(ControlFlow::WaitUntil(control.wait_until));
            }
            _ => {}
        })
        .expect("event loop failed");
}

fn reserve_battle_assist_width(
    window: &Window,
    requested: bool,
    reserved_scale: Option<u32>,
    game_width: u32,
    game_height: u32,
) -> Option<u32> {
    if requested == reserved_scale.is_some()
        || window.fullscreen().is_some()
        || window.is_maximized()
    {
        return reserved_scale;
    }
    let scale_factor = window.scale_factor();
    let physical_size = window.inner_size();
    let size = physical_size.to_logical::<f64>(scale_factor);
    let width = if requested {
        size.width + BATTLE_ASSIST_LOGICAL_WIDTH
    } else {
        (size.width - BATTLE_ASSIST_LOGICAL_WIDTH).max(f64::from(game_width))
    };
    let _ = window.request_inner_size(LogicalSize::new(width, size.height));
    if requested {
        Some(
            (physical_size.width / game_width.max(1))
                .min(physical_size.height / game_height.max(1))
                .max(1),
        )
    } else {
        None
    }
}
