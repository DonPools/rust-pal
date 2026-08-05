//! Native window and map framebuffer presentation.

mod battle_render;
mod battle_update;
mod debug_render;
mod dialog;
mod dialog_text;
mod draw;
mod input;
mod menu_render;
mod menu_state;
mod menu_update;
mod original_save;
mod presentation;
mod scene_render;
mod script_driver;
mod session;
mod snapshot;
mod text_render;
mod types;
mod visual;

use std::time::{Duration, Instant};

use crate::debug_overlay::{DebugOverlay, DebugSnapshot};
use crate::renderer::Renderer;
#[cfg(test)]
use pal_assets::text::TextLibrary;
use pal_core::game::{GameState, UPDATE_INTERVAL_MS};
#[cfg(test)]
use pal_core::role::Direction;
use pal_core::role::RoleSprites;
#[cfg(test)]
use pal_core::scene::SceneObject;
#[cfg(test)]
use pal_core::script::DialogPosition;
use pal_core::script::ScriptRuntime;
use pixels::{Pixels, SurfaceTexture};
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::WindowBuilder;

pub use battle_render::{render_battle, BattleRenderResources, BattleRenderState};
use battle_update::update_battle;
use debug_render::{debug_object_snapshot, focused_debug_object};
#[cfg(test)]
use debug_render::{
    object_debug_color, OBJECT_AUTO_COLOR, OBJECT_BOTH_COLOR, OBJECT_FOCUS_COLOR,
    OBJECT_HIDDEN_COLOR, OBJECT_INERT_COLOR, OBJECT_TRIGGER_COLOR,
};
#[cfg(test)]
use dialog::ActiveDialog;
use dialog::{advance_dialog_playback, DialogPlayback};
#[cfg(test)]
use dialog_text::dialog_body_lines;
use dialog_text::dialog_page_count;
#[cfg(test)]
use dialog_text::wrap_big5_lines;
use input::HeldInput;
use menu_state::FieldMenu;
#[cfg(test)]
use menu_state::{update_wrapping_selection, InventoryMenu, ShopMenu, ShopMode};
use menu_update::{update_active_menu, MenuUpdateContext};
use original_save::{latest_original_save_slot, restore_original_save, RestoreOriginalSaveError};
use presentation::{render_game, UiRenderContext};
pub use scene_render::render_tile_map;
#[cfg(test)]
use scene_render::{cover_tile_candidate, covering_tile_y};
use script_driver::{advance_script, auto_script_error_title, ScriptRenderResources};
use session::SessionState;
use snapshot::{restore_snapshot, save_snapshot, RestoreSnapshotError};
pub use types::{GameResources, LoadedScene, Viewport};

pub fn run_game_window<L>(
    mut renderer: Renderer,
    mut game: GameState,
    resources: GameResources,
    mut load_scene: L,
) where
    L: FnMut(u16, Option<u16>, &RoleSprites) -> Option<LoadedScene> + 'static,
{
    let GameResources {
        role_sprites,
        script_table,
        initial_enter_script,
        text,
        font,
        dialog_faces,
        dialog_icons,
        ui_sprites,
        item_sprites,
        enemy_battle_sprites,
        player_battle_sprites,
        battle_backgrounds,
        status_background,
        equip_background,
        voc_mkf,
        midi_mkf,
        sound_font,
        palettes,
        fbp_archive,
        rng_archive,
        original_save_dir,
        snapshot_path,
    } = resources;
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
    let mut debug_overlay = DebugOverlay::new(
        pixels.device(),
        pixels.surface_texture_format(),
        window.scale_factor(),
    );
    let mut show_collision = false;
    let mut show_objects = false;
    let mut show_script = false;
    let auto_scripts = script_table.clone();
    let mut battle_scripts = ScriptRuntime::new(script_table.clone());
    let mut scripts = ScriptRuntime::new(script_table);
    let mut dialog = None;
    let mut script_services = SessionState::new(auto_scripts, &voc_mkf, &midi_mkf, &sound_font);
    let initial_enter_script = game.scene_enter_script(initial_enter_script);
    if initial_enter_script != 0 {
        scripts.start(pal_core::scene::TriggerRequest {
            object_id: 0xffff,
            script_entry: initial_enter_script,
            kind: pal_core::scene::TriggerKind::Touch,
        });
    }
    render_game(
        &mut renderer,
        &game,
        &role_sprites,
        show_collision,
        show_objects,
        scripts.debug_snapshot(),
        UiRenderContext {
            dialog: dialog.as_ref(),
            field_menu: script_services.field_menu.as_ref(),
            inventory_menu: script_services.inventory_menu.as_ref(),
            confirmation_menu: script_services.confirmation_menu.as_ref(),
            shop_menu: script_services.shop_menu.as_ref(),
            music_enabled: script_services.music.enabled(),
            music_volume: script_services.music.volume(),
            sound_enabled: script_services.sound_effects.enabled(),
            sound_volume: script_services.sound_effects.volume(),
            text: &text,
            font: &font,
            dialog_faces: &dialog_faces,
            dialog_icons: &dialog_icons,
            ui_sprites: &ui_sprites,
            item_sprites: &item_sprites,
            enemy_battle_sprites: &enemy_battle_sprites,
            player_battle_sprites: &player_battle_sprites,
            battle_backgrounds: &battle_backgrounds,
            battle_selected_enemy: script_services.battle_selected_enemy,
            battle_command_selected: script_services.battle_command_selected,
            battle_event: script_services.battle_events.front().copied(),
            battle_event_ticks: script_services.battle_event_ticks,
            status_background: &status_background,
            equip_background: &equip_background,
            ui_ticks: 0,
            palettes: &palettes,
            visual: &script_services.visual,
        },
    );

    let tick = Duration::from_millis(UPDATE_INTERVAL_MS);
    let mut last_update = Instant::now();
    let mut accumulator = Duration::ZERO;
    let mut input = HeldInput::default();
    let mut ui_ticks = 0u64;

    event_loop
        .run(move |event, target| match event {
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => target.exit(),
                WindowEvent::Focused(false) => input = HeldInput::default(),
                WindowEvent::KeyboardInput { event, .. } => {
                    if let PhysicalKey::Code(code) = event.physical_key {
                        let pressed = event.state == ElementState::Pressed;
                        let handled = pressed
                            && !event.repeat
                            && match code {
                                KeyCode::F3 => {
                                    show_collision = !show_collision;
                                    update_debug_title(
                                        window,
                                        show_collision,
                                        show_objects,
                                        show_script,
                                    );
                                    true
                                }
                                KeyCode::F4 => {
                                    show_objects = !show_objects;
                                    update_debug_title(
                                        window,
                                        show_collision,
                                        show_objects,
                                        show_script,
                                    );
                                    true
                                }
                                KeyCode::F6 => {
                                    show_script = !show_script;
                                    update_debug_title(
                                        window,
                                        show_collision,
                                        show_objects,
                                        show_script,
                                    );
                                    true
                                }
                                KeyCode::F5 if !scripts.is_active() && dialog.is_none() => {
                                    if save_snapshot(&snapshot_path, &game).is_ok() {
                                        window.set_title("Rust-PAL [Snapshot saved]");
                                    } else {
                                        window.set_title("Rust-PAL [Snapshot save failed]");
                                    }
                                    true
                                }
                                KeyCode::F9 if !scripts.is_active() && dialog.is_none() => {
                                    match restore_snapshot(
                                        &snapshot_path,
                                        &mut game,
                                        &role_sprites,
                                        &mut load_scene,
                                    ) {
                                        Ok(()) => {
                                            if let Some(music_id) = game.current_music {
                                                if !script_services.music.play(music_id, true, 0) {
                                                    window.set_title(
                                                        "Rust-PAL [snapshot music unavailable]",
                                                    );
                                                }
                                            } else {
                                                script_services.music.stop();
                                            }
                                            script_services.inventory_menu = None;
                                            script_services.field_menu = None;
                                            script_services.shop_menu = None;
                                            script_services.confirmation_menu = None;
                                            script_services.pending_enter_script = None;
                                            input = HeldInput::default();
                                            window.set_title("Rust-PAL [Snapshot restored]");
                                        }
                                        Err(RestoreSnapshotError::SceneUnavailable) => {
                                            window
                                                .set_title("Rust-PAL [Snapshot scene unavailable]");
                                        }
                                        Err(RestoreSnapshotError::Unavailable) => {
                                            window.set_title("Rust-PAL [No snapshot]");
                                        }
                                    }
                                    true
                                }
                                _ => false,
                            };
                        if handled {
                            render_game(
                                &mut renderer,
                                &game,
                                &role_sprites,
                                show_collision,
                                show_objects,
                                scripts.debug_snapshot(),
                                UiRenderContext {
                                    dialog: dialog.as_ref(),
                                    field_menu: script_services.field_menu.as_ref(),
                                    inventory_menu: script_services.inventory_menu.as_ref(),
                                    confirmation_menu: script_services.confirmation_menu.as_ref(),
                                    shop_menu: script_services.shop_menu.as_ref(),
                                    music_enabled: script_services.music.enabled(),
                                    music_volume: script_services.music.volume(),
                                    sound_enabled: script_services.sound_effects.enabled(),
                                    sound_volume: script_services.sound_effects.volume(),
                                    text: &text,
                                    font: &font,
                                    dialog_faces: &dialog_faces,
                                    dialog_icons: &dialog_icons,
                                    ui_sprites: &ui_sprites,
                                    item_sprites: &item_sprites,
                                    enemy_battle_sprites: &enemy_battle_sprites,
                                    player_battle_sprites: &player_battle_sprites,
                                    battle_backgrounds: &battle_backgrounds,
                                    battle_selected_enemy: script_services.battle_selected_enemy,
                                    battle_command_selected: script_services
                                        .battle_command_selected,
                                    battle_event: script_services.battle_events.front().copied(),
                                    battle_event_ticks: script_services.battle_event_ticks,
                                    status_background: &status_background,
                                    equip_background: &equip_background,
                                    ui_ticks,
                                    palettes: &palettes,
                                    visual: &script_services.visual,
                                },
                            );
                        } else {
                            input.set_key(code, pressed, event.repeat);
                        }
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
                    pixels.frame_mut().copy_from_slice(renderer.screen());
                    let surface_size = window.inner_size();
                    if show_script {
                        let script = scripts.debug_snapshot();
                        debug_overlay.update(
                            pixels.device(),
                            pixels.queue(),
                            DebugSnapshot {
                                scene_number: game.scene_number,
                                object_count: game.scene_objects.len(),
                                player_x: game.player.world_x,
                                player_y: game.player.world_y,
                                camera_x: game.camera.x,
                                camera_y: game.camera.y,
                                virtual_width: renderer.width as u32,
                                virtual_height: renderer.height as u32,
                                surface_width: surface_size.width,
                                surface_height: surface_size.height,
                                scale_factor: window.scale_factor(),
                                focused_object: focused_debug_object(&game, script)
                                    .map(debug_object_snapshot),
                                script,
                            },
                        );
                    }
                    let render_result = pixels.render_with(|encoder, render_target, context| {
                        context.scaling_renderer.render(encoder, render_target);
                        if show_script {
                            debug_overlay.render(
                                encoder,
                                render_target,
                                surface_size.width,
                                surface_size.height,
                            );
                        }
                        Ok(())
                    });
                    if let Err(error) = render_result {
                        eprintln!("render failed: {error}");
                        target.exit();
                    } else {
                        renderer.mark_cleaned();
                    }
                }
                _ => {}
            },
            Event::AboutToWait => {
                script_services.music.poll();
                let now = Instant::now();
                accumulator += now
                    .duration_since(last_update)
                    .min(Duration::from_millis(250));
                last_update = now;

                let mut changed = false;
                while accumulator >= tick {
                    let sampled = input.sample();
                    let visual_was_blocking = script_services.visual.is_blocking();
                    let visual_scene_update_due = script_services.visual.scene_update_due();
                    if script_services.visual.needs_update() {
                        match script_services.visual.update(
                            renderer.screen(),
                            &palettes,
                            &fbp_archive,
                            &rng_archive,
                            &role_sprites,
                        ) {
                            Ok(visual_changed) => changed |= visual_changed,
                            Err(error) => {
                                window.set_title(&format!("Rust-PAL [visual error: {error}]"));
                            }
                        }
                    }
                    if script_services.quit_requested {
                        target.exit();
                    }
                    let load_last_save_requested =
                        std::mem::take(&mut script_services.load_last_save_requested);
                    if load_last_save_requested {
                        let slot = script_services
                            .current_save_slot
                            .or_else(|| latest_original_save_slot(&original_save_dir));
                        match slot.map(|slot| {
                            restore_original_save(
                                &original_save_dir,
                                slot,
                                &mut game,
                                &role_sprites,
                                &mut load_scene,
                            )
                        }) {
                            Some(Ok(environment)) => {
                                script_services.current_save_slot = Some(environment.slot);
                                script_services.visual.restore_original_environment(
                                    environment.night_palette,
                                    environment.screen_wave,
                                );
                                dialog = None;
                                script_services.pending_dialog = None;
                                script_services.field_menu = None;
                                script_services.inventory_menu = None;
                                script_services.shop_menu = None;
                                script_services.confirmation_menu = None;
                                if let Some(music_id) = game.current_music {
                                    script_services.music.play(music_id, true, 0);
                                } else {
                                    script_services.music.stop();
                                }
                                window.set_title("Rust-PAL [Original save loaded]");
                            }
                            Some(Err(RestoreOriginalSaveError::SceneUnavailable)) => {
                                window.set_title("Rust-PAL [Original save scene unavailable]")
                            }
                            Some(Err(
                                RestoreOriginalSaveError::Unavailable
                                | RestoreOriginalSaveError::Invalid,
                            )) => window.set_title("Rust-PAL [Invalid original save]"),
                            None => window.set_title("Rust-PAL [No original save]"),
                        }
                        changed = true;
                    }
                    if visual_was_blocking || load_last_save_requested {
                        // Blocking script visuals advance independently until completion.
                        if visual_scene_update_due {
                            match game.update_auto_scripts(&script_services.auto_scripts) {
                                Ok(auto_changed) => changed |= auto_changed,
                                Err(error) => {
                                    window.set_title(&auto_script_error_title(error));
                                }
                            }
                            for sound_id in game.take_auto_script_sounds() {
                                if !script_services.sound_effects.play(sound_id) {
                                    window.set_title(&format!(
                                        "Rust-PAL [invalid auto sound {sound_id}]"
                                    ));
                                }
                            }
                        }
                    } else if script_services.waiting_for_key {
                        if sampled.confirm || sampled.cancel || sampled.direction_pressed.is_some()
                        {
                            script_services.waiting_for_key = false;
                            let active_scripts = if battle_scripts.is_active() {
                                &mut battle_scripts
                            } else {
                                &mut scripts
                            };
                            advance_script(
                                active_scripts,
                                &mut game,
                                &mut dialog,
                                ScriptRenderResources {
                                    text: &text,
                                    role_sprites: &role_sprites,
                                },
                                &mut load_scene,
                                &mut script_services,
                                &mut |title| window.set_title(title),
                            );
                            changed = true;
                        }
                    } else if dialog.is_some() {
                        let awaiting_input = dialog
                            .as_ref()
                            .is_some_and(|active_dialog| active_dialog.awaiting_input);
                        if awaiting_input {
                            let active_dialog = dialog.as_mut().expect("dialog was checked above");
                            active_dialog.wait_palette_ticks =
                                active_dialog.wait_palette_ticks.wrapping_add(1);
                            changed = true;
                        }
                        let timed_out = if awaiting_input {
                            dialog
                                .as_mut()
                                .and_then(|active_dialog| active_dialog.auto_wait_ticks.as_mut())
                                .is_some_and(|ticks| {
                                    *ticks = ticks.saturating_sub(1);
                                    *ticks == 0
                                })
                        } else {
                            false
                        };
                        if awaiting_input && (sampled.confirm || sampled.cancel || timed_out) {
                            changed = true;
                            let active_dialog = dialog.as_mut().expect("dialog was checked above");
                            let page_count = dialog_page_count(&text, active_dialog);
                            if active_dialog.page + 1 < page_count {
                                active_dialog.page += 1;
                                active_dialog.awaiting_input = false;
                                active_dialog.auto_wait_ticks = None;
                                active_dialog.wait_palette_ticks = 0;
                            } else if let Some(pending) = script_services.pending_dialog.take() {
                                dialog = Some(pending);
                            } else {
                                dialog = None;
                                let active_scripts = if battle_scripts.is_active() {
                                    &mut battle_scripts
                                } else {
                                    &mut scripts
                                };
                                advance_script(
                                    active_scripts,
                                    &mut game,
                                    &mut dialog,
                                    ScriptRenderResources {
                                        text: &text,
                                        role_sprites: &role_sprites,
                                    },
                                    &mut load_scene,
                                    &mut script_services,
                                    &mut |title| window.set_title(title),
                                );
                            }
                        } else if !awaiting_input {
                            let playback = advance_dialog_playback(
                                &text,
                                dialog.as_mut().expect("dialog was checked above"),
                                &mut script_services.dialog_delay_ms,
                                UPDATE_INTERVAL_MS as u32,
                                sampled.confirm || sampled.cancel,
                            );
                            changed = true;
                            if matches!(
                                playback,
                                DialogPlayback::ContinueScript | DialogPlayback::AutoClose
                            ) {
                                if playback == DialogPlayback::AutoClose {
                                    dialog = None;
                                }
                                let active_scripts = if battle_scripts.is_active() {
                                    &mut battle_scripts
                                } else {
                                    &mut scripts
                                };
                                advance_script(
                                    active_scripts,
                                    &mut game,
                                    &mut dialog,
                                    ScriptRenderResources {
                                        text: &text,
                                        role_sprites: &role_sprites,
                                    },
                                    &mut load_scene,
                                    &mut script_services,
                                    &mut |title| window.set_title(title),
                                );
                            }
                        }
                    } else if game.battle().is_some() {
                        if battle_scripts.is_active() && script_services.battle_events.is_empty() {
                            advance_script(
                                &mut battle_scripts,
                                &mut game,
                                &mut dialog,
                                ScriptRenderResources {
                                    text: &text,
                                    role_sprites: &role_sprites,
                                },
                                &mut load_scene,
                                &mut script_services,
                                &mut |title| window.set_title(title),
                            );
                            changed = true;
                            accumulator -= tick;
                            continue;
                        }
                        let outcome = update_battle(
                            sampled,
                            &mut game,
                            &mut script_services,
                            &mut battle_scripts,
                        );
                        changed = true;
                        if let Some(outcome) = outcome {
                            if !scripts.resolve_battle(outcome.result) {
                                window.set_title("Rust-PAL [battle script resume failed]");
                            } else {
                                if let Some(music_id) = game.current_music {
                                    script_services.music.play(music_id, true, 0);
                                } else {
                                    script_services.music.stop();
                                }
                                window.set_title(&format!(
                                    "Rust-PAL [Battle {:?}: +{} EXP +{} cash]",
                                    outcome.result,
                                    outcome.rewards.experience,
                                    outcome.rewards.cash
                                ));
                                advance_script(
                                    &mut scripts,
                                    &mut game,
                                    &mut dialog,
                                    ScriptRenderResources {
                                        text: &text,
                                        role_sprites: &role_sprites,
                                    },
                                    &mut load_scene,
                                    &mut script_services,
                                    &mut |title| window.set_title(title),
                                );
                            }
                        }
                    } else if let Some(menu_changed) = update_active_menu(&mut MenuUpdateContext {
                        input: sampled,
                        scripts: &mut scripts,
                        game: &mut game,
                        dialog: &mut dialog,
                        text: &text,
                        role_sprites: &role_sprites,
                        load_scene: &mut load_scene,
                        services: &mut script_services,
                        snapshot_path: &snapshot_path,
                        original_save_dir: &original_save_dir,
                        set_title: &mut |title: &str| window.set_title(title),
                        exit: &mut || target.exit(),
                    }) {
                        changed = menu_changed;
                    } else if scripts.is_active() {
                        advance_script(
                            &mut scripts,
                            &mut game,
                            &mut dialog,
                            ScriptRenderResources {
                                text: &text,
                                role_sprites: &role_sprites,
                            },
                            &mut load_scene,
                            &mut script_services,
                            &mut |title| window.set_title(title),
                        );
                        changed = true;
                    } else {
                        if sampled.cancel {
                            script_services.field_menu = Some(FieldMenu::Main {
                                selected: script_services.main_menu_selected,
                            });
                            window.set_title("Rust-PAL [Menu]");
                            changed = true;
                            accumulator -= tick;
                            continue;
                        }
                        let tick_changed = game.update(sampled);
                        changed |= tick_changed;
                        if tick_changed {
                            if let Some(trigger) = game.take_trigger() {
                                if scripts.start(trigger) {
                                    advance_script(
                                        &mut scripts,
                                        &mut game,
                                        &mut dialog,
                                        ScriptRenderResources {
                                            text: &text,
                                            role_sprites: &role_sprites,
                                        },
                                        &mut load_scene,
                                        &mut script_services,
                                        &mut |title| window.set_title(title),
                                    );
                                }
                            }
                        }
                        if !scripts.is_active() {
                            match game.update_auto_scripts(&script_services.auto_scripts) {
                                Ok(auto_changed) => changed |= auto_changed,
                                Err(error) => window.set_title(&auto_script_error_title(error)),
                            }
                            for sound_id in game.take_auto_script_sounds() {
                                if !script_services.sound_effects.play(sound_id) {
                                    window.set_title(&format!(
                                        "Rust-PAL [invalid auto sound {sound_id}]"
                                    ));
                                }
                            }
                        }
                    }
                    let script_is_paused = (!scripts.is_active() && !battle_scripts.is_active())
                        || dialog.is_some()
                        || script_services.waiting_for_key
                        || script_services.field_menu.is_some()
                        || script_services.inventory_menu.is_some()
                        || script_services.confirmation_menu.is_some()
                        || script_services.shop_menu.is_some();
                    if script_is_paused && script_services.visual.queue_automatic_scene_fade_in() {
                        changed = true;
                    }
                    accumulator -= tick;
                }
                if dialog.is_none()
                    && (script_services.field_menu.is_some()
                        || script_services.inventory_menu.is_some()
                        || script_services.confirmation_menu.is_some()
                        || script_services.shop_menu.is_some())
                {
                    changed = true;
                }
                if changed {
                    ui_ticks = ui_ticks.wrapping_add(1);
                    render_game(
                        &mut renderer,
                        &game,
                        &role_sprites,
                        show_collision,
                        show_objects,
                        scripts.debug_snapshot(),
                        UiRenderContext {
                            dialog: dialog.as_ref(),
                            field_menu: script_services.field_menu.as_ref(),
                            inventory_menu: script_services.inventory_menu.as_ref(),
                            confirmation_menu: script_services.confirmation_menu.as_ref(),
                            shop_menu: script_services.shop_menu.as_ref(),
                            music_enabled: script_services.music.enabled(),
                            music_volume: script_services.music.volume(),
                            sound_enabled: script_services.sound_effects.enabled(),
                            sound_volume: script_services.sound_effects.volume(),
                            text: &text,
                            font: &font,
                            dialog_faces: &dialog_faces,
                            dialog_icons: &dialog_icons,
                            ui_sprites: &ui_sprites,
                            item_sprites: &item_sprites,
                            enemy_battle_sprites: &enemy_battle_sprites,
                            player_battle_sprites: &player_battle_sprites,
                            battle_backgrounds: &battle_backgrounds,
                            battle_selected_enemy: script_services.battle_selected_enemy,
                            battle_command_selected: script_services.battle_command_selected,
                            battle_event: script_services.battle_events.front().copied(),
                            battle_event_ticks: script_services.battle_event_ticks,
                            status_background: &status_background,
                            equip_background: &equip_background,
                            ui_ticks,
                            palettes: &palettes,
                            visual: &script_services.visual,
                        },
                    );
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

fn update_debug_title(
    window: &winit::window::Window,
    collision: bool,
    objects: bool,
    script: bool,
) {
    let title = match (collision, objects, script) {
        (false, false, false) => "Rust-PAL".to_owned(),
        (true, false, false) => "Rust-PAL [Collision]".to_owned(),
        (false, true, false) => "Rust-PAL [Objects]".to_owned(),
        (false, false, true) => "Rust-PAL [Script]".to_owned(),
        _ => "Rust-PAL [Debug]".to_owned(),
    };
    window.set_title(&title);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::dialog_text::{dialog_text_width, dialog_title};
    use crate::window::presentation::cycle_dialog_icon_palette;
    use crate::window::text_render::dialog_color_after;

    fn text_library(messages: &[&[u8]]) -> TextLibrary {
        let word_data = [b' '; 10];
        let mut message_data = Vec::new();
        let mut message_index = Vec::new();
        message_index.extend_from_slice(&0u32.to_le_bytes());
        for message in messages {
            message_data.extend_from_slice(message);
            message_index.extend_from_slice(&(message_data.len() as u32).to_le_bytes());
        }
        TextLibrary::parse(&word_data, &message_data, &message_index).unwrap()
    }

    fn debug_object() -> SceneObject {
        SceneObject {
            id: 1,
            world_x: 100,
            world_y: 80,
            layer: 0,
            trigger_script: 10,
            auto_script: 20,
            state: 1,
            trigger_mode: 1,
            sprite_index: None,
            frames_per_direction: 0,
            sprite_frame_count: 0,
            direction: Direction::South,
            current_frame: 0,
            vanish_time: 0,
            auto_script_idle_frame: 0,
        }
    }

    #[test]
    fn object_debug_colors_distinguish_script_roles_and_focus() {
        let mut object = debug_object();
        assert_eq!(object_debug_color(&object, false), OBJECT_BOTH_COLOR);

        object.auto_script = 0;
        assert_eq!(object_debug_color(&object, false), OBJECT_TRIGGER_COLOR);
        object.trigger_script = 0;
        object.auto_script = 20;
        assert_eq!(object_debug_color(&object, false), OBJECT_AUTO_COLOR);
        object.auto_script = 0;
        assert_eq!(object_debug_color(&object, false), OBJECT_INERT_COLOR);
        object.state = 0;
        assert_eq!(object_debug_color(&object, false), OBJECT_HIDDEN_COLOR);
        assert_eq!(object_debug_color(&object, true), OBJECT_FOCUS_COLOR);
    }

    #[test]
    fn covering_tiles_are_bottom_aligned_to_their_logical_tile() {
        assert_eq!(covering_tile_y(10, 0, 15, 0), 152);
        assert_eq!(covering_tile_y(10, 1, 31, 20), 124);
    }

    #[test]
    fn cover_tile_candidates_match_pal_scan_pattern() {
        assert_eq!(cover_tile_candidate(10, 20, 0, 0), (10, 20, 0));
        assert_eq!(cover_tile_candidate(10, 20, 0, 2), (9, 20, 1));
        assert_eq!(cover_tile_candidate(10, 20, 1, 2), (10, 21, 0));
        assert_eq!(cover_tile_candidate(10, 20, 0, 4), (10, 20, 1));
        assert_eq!(cover_tile_candidate(10, 20, 1, 4), (11, 21, 0));
    }

    #[test]
    fn wraps_big5_without_splitting_double_byte_characters() {
        let text = [0xb8, 0x67, 0xc5, 0xe7, b'A'];
        let lines = wrap_big5_lines(&text, 24);
        assert_eq!(lines, [&text[..2], &text[2..]]);
    }

    #[test]
    fn long_dialog_text_wraps_without_losing_bytes() {
        let text = [b'A'; 109];
        let lines = wrap_big5_lines(&text, 272);
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[..3].iter().map(|line| line.len()).sum::<usize>(), 102);
        assert_eq!(lines[3].len(), 7);
    }

    #[test]
    fn dialog_title_does_not_consume_one_of_four_body_lines() {
        let text = text_library(&[b"Name:", b"one", b"two", b"three", b"four"]);
        let mut dialog = ActiveDialog::new(0, DialogPosition::Upper, 0x4f, None, 24);
        dialog.message_ids = vec![0, 1, 2, 3, 4];
        dialog.awaiting_input = true;
        assert_eq!(dialog_title(&text, &dialog), Some(b"Name:".as_slice()));
        assert_eq!(dialog_body_lines(&text, &dialog).len(), 4);
        assert_eq!(dialog_page_count(&text, &dialog), 1);
    }

    #[test]
    fn dialog_controls_do_not_consume_layout_width() {
        assert_eq!(dialog_text_width(b"A-$03B"), 16);
        let lines = wrap_big5_lines(b"A-$03B", 8);
        assert_eq!(lines, [b"A-$03".as_slice(), b"B".as_slice()]);
        assert_eq!(dialog_text_width(br"A\$B"), 24);
        assert_eq!(wrap_big5_lines(b"A~70ignored", 80), [b"A~70".as_slice()]);
        assert_eq!(dialog_color_after(b"-cyan", 0x4f), 0x8d);
        assert_eq!(dialog_color_after(b"still cyan-", 0x8d), 0x4f);
        assert_eq!(dialog_color_after(br#"\"literal"#, 0x4f), 0x4f);
    }

    #[test]
    fn dialog_playback_applies_speed_terminal_delay_and_icon_controls() {
        let text = text_library(&[b"A$07BC", b"A~70ignored", b"A)"]);
        let mut persistent_delay = 24;
        let mut dialog = ActiveDialog::new(0, DialogPosition::Upper, 0x4f, None, 24);
        assert_eq!(
            advance_dialog_playback(&text, &mut dialog, &mut persistent_delay, 24, false),
            DialogPlayback::Revealing
        );
        assert_eq!(dialog.revealed_glyphs, 1);
        assert_eq!(persistent_delay, 80);
        assert_eq!(
            advance_dialog_playback(&text, &mut dialog, &mut persistent_delay, 79, false),
            DialogPlayback::Revealing
        );
        assert_eq!(dialog.revealed_glyphs, 1);
        assert_eq!(
            advance_dialog_playback(&text, &mut dialog, &mut persistent_delay, 1, false),
            DialogPlayback::Revealing
        );
        assert_eq!(dialog.revealed_glyphs, 2);

        let mut terminal = ActiveDialog::new(1, DialogPosition::Upper, 0x4f, None, 24);
        assert_eq!(
            advance_dialog_playback(&text, &mut terminal, &mut persistent_delay, 0, true),
            DialogPlayback::Revealing
        );
        assert_eq!(terminal.terminal_wait_ms, Some(800));
        assert_eq!(
            advance_dialog_playback(&text, &mut terminal, &mut persistent_delay, 799, false),
            DialogPlayback::Revealing
        );
        assert_eq!(
            advance_dialog_playback(&text, &mut terminal, &mut persistent_delay, 1, false),
            DialogPlayback::AutoClose
        );

        let mut icon = ActiveDialog::new(2, DialogPosition::Upper, 0x4f, None, 24);
        icon.wait_after_reveal = true;
        assert_eq!(
            advance_dialog_playback(&text, &mut icon, &mut persistent_delay, 0, true),
            DialogPlayback::AwaitingInput
        );
        assert_eq!(icon.wait_icon, 1);
    }

    #[test]
    fn dialog_wait_palette_cycles_original_six_icon_colors() {
        let mut palette = pal_assets::palette::Palette::default();
        for (index, color) in palette.colors[0xf9..=0xfe].iter_mut().enumerate() {
            color.r = index as u8;
        }
        cycle_dialog_icon_palette(&mut palette, 2);
        assert_eq!(
            palette.colors[0xf9..=0xfe]
                .iter()
                .map(|color| color.r)
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4, 5, 0]
        );
    }

    #[test]
    fn inventory_menu_uses_original_three_column_navigation_and_scrolling() {
        let mut menu = InventoryMenu::default();
        menu.update(Some(Direction::North), 10);
        assert_eq!(menu.selected, 0);
        menu.update(Some(Direction::South), 10);
        assert_eq!(menu.selected, 3);
        menu.update(Some(Direction::East), 10);
        assert_eq!(menu.selected, 4);
        menu.update(Some(Direction::West), 10);
        assert_eq!(menu.selected, 3);
        menu.update(Some(Direction::North), 10);
        assert_eq!(menu.selected, 0);

        menu.selected = 8;
        menu.update(Some(Direction::South), 10);
        assert_eq!(menu.selected, 9);
        menu.selected = 29;
        assert_eq!(menu.first_visible(30), 15);

        menu.update(None, 0);
        assert_eq!(menu.selected, 0);
        assert_eq!(menu.first_visible(0), 0);
    }

    #[test]
    fn main_and_target_menus_wrap_at_both_ends() {
        let mut selected = 0;
        update_wrapping_selection(&mut selected, Some(Direction::North), 4);
        assert_eq!(selected, 3);
        update_wrapping_selection(&mut selected, Some(Direction::South), 4);
        assert_eq!(selected, 0);
        update_wrapping_selection(&mut selected, Some(Direction::West), 4);
        assert_eq!(selected, 3);
        update_wrapping_selection(&mut selected, Some(Direction::East), 4);
        assert_eq!(selected, 0);

        let mut shop = ShopMenu {
            mode: ShopMode::Sell,
            selected: 0,
            confirming: false,
            selected_yes: false,
        };
        shop.update_selection(Some(Direction::North), 3);
        assert_eq!(shop.selected, 2);
        shop.update_selection(Some(Direction::South), 3);
        assert_eq!(shop.selected, 0);
    }
}
