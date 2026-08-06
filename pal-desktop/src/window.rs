//! Native window and map framebuffer presentation.

mod battle_render;
mod battle_timing;
mod battle_update;
mod debug_render;
mod dialog;
mod dialog_text;
mod draw;
mod input;
mod menu_render;
mod menu_state;
mod menu_update;
mod opening_intro;
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
use pal_core::script::{ScriptRuntime, ScriptVisual};
use pixels::{Pixels, SurfaceTexture};
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::WindowBuilder;

pub use battle_render::{render_battle, BattleMenuState, BattleRenderResources, BattleRenderState};
use battle_update::{advance_post_battle, update_battle};
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
use input::HeldInput;
#[cfg(test)]
use menu_state::{update_wrapping_selection, InventoryMenu, ShopMenu, ShopMode};
use menu_state::{FieldMenu, OpeningMenu, OpeningMenuAction};
use menu_update::{update_active_menu, MenuUpdateContext};
use opening_intro::{OpeningIntro, OpeningIntroAction, TITLE_MUSIC};
use original_save::{
    latest_original_save_slot, original_save_slots, restore_original_save, RestoreOriginalSaveError,
};
use presentation::{render_game, UiRenderContext};
pub use scene_render::render_tile_map;
#[cfg(test)]
use scene_render::{cover_tile_candidate, covering_tile_y};
use script_driver::{advance_script, auto_script_error_title, ScriptRenderResources};
use session::SessionState;
use snapshot::{restore_snapshot, save_snapshot, RestoreSnapshotError};
pub use types::{GameResources, LoadedScene, Viewport};

const OPENING_MENU_MUSIC: u16 = 4;
const OPENING_INTRO_UPDATE_MS: u64 = 10;
const DIALOG_POLL_INTERVAL_MS: u64 = 8;

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
        opening_background,
        text,
        font,
        dialog_faces,
        dialog_icons,
        ui_sprites,
        item_sprites,
        enemy_battle_sprites,
        player_battle_sprites,
        magic_effect_sprites,
        battle_effects,
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
    let mut script_services = SessionState::new(
        auto_scripts,
        &voc_mkf,
        &midi_mkf,
        &sound_font,
        &magic_effect_sprites,
        &player_battle_sprites,
    );
    let mut opening_intro = Some(
        OpeningIntro::from_resources(&fbp_archive, &rng_archive, &role_sprites)
            .expect("failed to load original opening animation resources"),
    );
    let mut opening_menu = None;
    let mut pending_opening_action = None;
    render_game(
        &mut renderer,
        &game,
        &role_sprites,
        show_collision,
        show_objects,
        scripts.debug_snapshot(),
        UiRenderContext {
            opening_intro: opening_intro.as_ref(),
            opening_menu: opening_menu.as_ref(),
            opening_background: &opening_background,
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
            magic_effect_sprites: &magic_effect_sprites,
            battle_effects: &battle_effects,
            battle_backgrounds: &battle_backgrounds,
            battle_selected_enemy: script_services.battle_selected_enemy,
            battle_command_selected: script_services.battle_command_selected,
            battle_targeting_enemy: script_services.battle_targeting_enemy,
            battle_menu: script_services.battle_menu,
            battle_auto_attack: script_services.battle_auto_attack,
            battle_event: script_services.battle_events.front().copied(),
            battle_event_ticks: script_services.battle_event_ticks,
            battle_kept_effects: &script_services.battle_kept_effects,
            post_battle: script_services.post_battle.as_ref(),
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
                                KeyCode::F5
                                    if opening_menu.is_none()
                                        && !scripts.is_active()
                                        && dialog.is_none() =>
                                {
                                    if save_snapshot(&snapshot_path, &game).is_ok() {
                                        window.set_title("Rust-PAL [Snapshot saved]");
                                    } else {
                                        window.set_title("Rust-PAL [Snapshot save failed]");
                                    }
                                    true
                                }
                                KeyCode::F9
                                    if opening_menu.is_none()
                                        && !scripts.is_active()
                                        && dialog.is_none() =>
                                {
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
                                            script_services.pending_scene_change = None;
                                            script_services.pending_dialog = None;
                                            script_services.pending_script_event = None;
                                            script_services.pending_load_slot = None;
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
                                    opening_intro: opening_intro.as_ref(),
                                    opening_menu: opening_menu.as_ref(),
                                    opening_background: &opening_background,
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
                                    magic_effect_sprites: &magic_effect_sprites,
                                    battle_effects: &battle_effects,
                                    battle_backgrounds: &battle_backgrounds,
                                    battle_selected_enemy: script_services.battle_selected_enemy,
                                    battle_command_selected: script_services
                                        .battle_command_selected,
                                    battle_targeting_enemy: script_services.battle_targeting_enemy,
                                    battle_menu: script_services.battle_menu,
                                    battle_auto_attack: script_services.battle_auto_attack,
                                    battle_event: script_services.battle_events.front().copied(),
                                    battle_event_ticks: script_services.battle_event_ticks,
                                    battle_kept_effects: &script_services.battle_kept_effects,
                                    post_battle: script_services.post_battle.as_ref(),
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
                let frame_elapsed = now
                    .duration_since(last_update)
                    .min(Duration::from_millis(250));
                accumulator += frame_elapsed;
                last_update = now;

                let mut changed = false;
                // Dialog glyphs use the original 8 ms timing quantum. Advance their
                // clock independently so the 50 ms game simulation step does not
                // reveal two default-speed glyphs in one rendered frame.
                if opening_intro.is_none()
                    && opening_menu.is_none()
                    && !script_services.visual.is_blocking()
                    && !script_services.waiting_for_key
                    && dialog.as_ref().is_some_and(|active| !active.awaiting_input)
                {
                    let before = dialog
                        .as_ref()
                        .map(|active| (active.revealed_glyphs, active.awaiting_input));
                    let _ = advance_dialog_playback(
                        &text,
                        dialog.as_mut().expect("dialog was checked above"),
                        &mut script_services.dialog_delay_ms,
                        u32::try_from(frame_elapsed.as_millis()).unwrap_or(u32::MAX),
                        false,
                    );
                    let after = dialog
                        .as_ref()
                        .map(|active| (active.revealed_glyphs, active.awaiting_input));
                    changed |= before != after;
                }
                let update_tick = if opening_intro.is_some() {
                    Duration::from_millis(OPENING_INTRO_UPDATE_MS)
                } else {
                    tick
                };
                while accumulator >= update_tick {
                    let (sampled, any_pressed) = input.sample();
                    if opening_intro.is_some() {
                        let (intro_changed, action) = opening_intro
                            .as_mut()
                            .expect("opening intro was checked above")
                            .update(update_tick.as_millis() as u32, sampled, &rng_archive)
                            .unwrap_or_else(|error| {
                                panic!("failed to play original opening animation: {error}")
                            });
                        changed |= intro_changed;
                        let intro_finished = action == OpeningIntroAction::Finished;
                        match action {
                            OpeningIntroAction::None => {}
                            OpeningIntroAction::PlayTitleMusic => {
                                if !script_services.music.play(TITLE_MUSIC, true, 2) {
                                    window.set_title("Rust-PAL [title music unavailable]");
                                }
                            }
                            OpeningIntroAction::StopTitleMusic => {
                                script_services.music.stop();
                            }
                            OpeningIntroAction::Finished => {
                                opening_intro = None;
                                opening_menu =
                                    Some(OpeningMenu::new(original_save_slots(&original_save_dir)));
                                script_services
                                    .visual
                                    .restore_original_environment(false, 0);
                                if !script_services.music.play(OPENING_MENU_MUSIC, true, 1) {
                                    window.set_title("Rust-PAL [opening music unavailable]");
                                }
                                script_services
                                    .visual
                                    .queue(ScriptVisual::FadeIn { speed: 1 });
                                script_services
                                    .visual
                                    .start_pending(
                                        renderer.screen(),
                                        &palettes,
                                        &fbp_archive,
                                        &rng_archive,
                                        &role_sprites,
                                    )
                                    .unwrap_or_else(|error| {
                                        panic!("failed to start opening menu fade-in: {error}")
                                    });
                                changed = true;
                            }
                        }
                        if intro_finished {
                            accumulator = Duration::ZERO;
                        } else {
                            accumulator -= update_tick;
                        }
                        continue;
                    }
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
                    let selected_load_slot = (!script_services.visual.is_blocking())
                        .then(|| script_services.pending_load_slot.take())
                        .flatten();
                    if let Some(slot) = selected_load_slot {
                        match restore_original_save(
                            &original_save_dir,
                            slot,
                            &mut game,
                            &role_sprites,
                            &mut load_scene,
                        ) {
                            Ok(environment) => {
                                script_services.current_save_slot = Some(environment.slot);
                                script_services.visual.restore_original_environment(
                                    environment.night_palette,
                                    environment.screen_wave,
                                );
                                script_services.visual.prepare_scene_fade_in();
                                dialog = None;
                                script_services.pending_dialog = None;
                                script_services.pending_script_event = None;
                                script_services.pending_scene_change = None;
                                script_services.pending_load_slot = None;
                                script_services.field_menu = None;
                                script_services.inventory_menu = None;
                                script_services.shop_menu = None;
                                script_services.confirmation_menu = None;
                                if let Some(music_id) = game.current_music {
                                    script_services.music.play(music_id, true, 0);
                                } else {
                                    script_services.music.stop();
                                }
                                input = HeldInput::default();
                                window.set_title(&format!("Rust-PAL [save slot {slot} loaded]"));
                            }
                            Err(RestoreOriginalSaveError::SceneUnavailable) => {
                                script_services
                                    .visual
                                    .queue(ScriptVisual::FadeIn { speed: 1 });
                                if let Some(music_id) = game.current_music {
                                    script_services.music.play(music_id, true, 0);
                                }
                                window.set_title("Rust-PAL [save scene unavailable]");
                            }
                            Err(
                                RestoreOriginalSaveError::Unavailable
                                | RestoreOriginalSaveError::Invalid,
                            ) => {
                                script_services
                                    .visual
                                    .queue(ScriptVisual::FadeIn { speed: 1 });
                                if let Some(music_id) = game.current_music {
                                    script_services.music.play(music_id, true, 0);
                                }
                                window.set_title("Rust-PAL [invalid save slot]");
                            }
                        }
                        changed = true;
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
                                script_services.pending_script_event = None;
                                script_services.pending_scene_change = None;
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
                    if visual_was_blocking
                        || selected_load_slot.is_some()
                        || load_last_save_requested
                    {
                        // Blocking script visuals advance independently until completion.
                        if visual_scene_update_due {
                            let update =
                                game.update_auto_scripts_report(&script_services.auto_scripts);
                            changed |= update.changed;
                            if let Some(error) = update.error {
                                window.set_title(&auto_script_error_title(error));
                            }
                            for sound_id in game.take_auto_script_sounds() {
                                if !script_services.sound_effects.play(sound_id) {
                                    window.set_title(&format!(
                                        "Rust-PAL [invalid auto sound {sound_id}]"
                                    ));
                                }
                            }
                        }
                    } else if opening_menu.is_some() {
                        changed = true;
                        if let Some(action) = pending_opening_action.take() {
                            match action {
                                OpeningMenuAction::StartNewGame => {
                                    opening_menu = None;
                                    script_services.pending_dialog = None;
                                    script_services.pending_script_event = None;
                                    script_services.pending_scene_change = None;
                                    script_services.pending_load_slot = None;
                                    script_services.music.stop();
                                    script_services.visual.prepare_scene_fade_in();
                                    let enter_script =
                                        game.scene_enter_script(initial_enter_script);
                                    if enter_script != 0 {
                                        scripts.start(pal_core::scene::TriggerRequest {
                                            object_id: 0xffff,
                                            script_entry: enter_script,
                                            kind: pal_core::scene::TriggerKind::Touch,
                                        });
                                    }
                                    window.set_title("Rust-PAL");
                                }
                                OpeningMenuAction::LoadSlot(slot) => {
                                    match restore_original_save(
                                        &original_save_dir,
                                        slot,
                                        &mut game,
                                        &role_sprites,
                                        &mut load_scene,
                                    ) {
                                        Ok(environment) => {
                                            script_services.current_save_slot =
                                                Some(environment.slot);
                                            script_services.visual.restore_original_environment(
                                                environment.night_palette,
                                                environment.screen_wave,
                                            );
                                            script_services.visual.prepare_scene_fade_in();
                                            dialog = None;
                                            script_services.pending_dialog = None;
                                            script_services.pending_script_event = None;
                                            script_services.pending_scene_change = None;
                                            script_services.pending_load_slot = None;
                                            script_services.field_menu = None;
                                            script_services.inventory_menu = None;
                                            script_services.shop_menu = None;
                                            script_services.confirmation_menu = None;
                                            opening_menu = None;
                                            if let Some(music_id) = game.current_music {
                                                script_services.music.play(music_id, true, 0);
                                            } else {
                                                script_services.music.stop();
                                            }
                                            window.set_title(&format!(
                                                "Rust-PAL [save slot {slot} loaded]"
                                            ));
                                        }
                                        Err(RestoreOriginalSaveError::SceneUnavailable) => {
                                            script_services
                                                .visual
                                                .queue(ScriptVisual::FadeIn { speed: 1 });
                                            window.set_title("Rust-PAL [save scene unavailable]");
                                        }
                                        Err(RestoreOriginalSaveError::Unavailable) => {
                                            script_services
                                                .visual
                                                .queue(ScriptVisual::FadeIn { speed: 1 });
                                            window.set_title("Rust-PAL [empty save slot]");
                                        }
                                        Err(RestoreOriginalSaveError::Invalid) => {
                                            script_services
                                                .visual
                                                .queue(ScriptVisual::FadeIn { speed: 1 });
                                            window.set_title("Rust-PAL [invalid save slot]");
                                        }
                                    }
                                }
                                OpeningMenuAction::None => {}
                            }
                        } else {
                            let action = opening_menu
                                .as_mut()
                                .expect("opening menu was checked above")
                                .update(sampled);
                            if action != OpeningMenuAction::None {
                                if script_services
                                    .visual
                                    .queue(ScriptVisual::FadeOut { speed: 1 })
                                {
                                    pending_opening_action = Some(action);
                                } else {
                                    window.set_title(
                                        "Rust-PAL [opening transition is already active]",
                                    );
                                }
                            }
                        }
                    } else if script_services.waiting_for_key {
                        if any_pressed {
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
                        if awaiting_input && (any_pressed || timed_out) {
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
                                0,
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
                    } else if script_services.post_battle.is_some() {
                        let outcome = advance_post_battle(any_pressed, &mut script_services);
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
                            any_pressed,
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
                                window.set_title("Rust-PAL");
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
                            let update =
                                game.update_auto_scripts_report(&script_services.auto_scripts);
                            changed |= update.changed;
                            if let Some(error) = update.error {
                                window.set_title(&auto_script_error_title(error));
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
                    if opening_menu.is_none()
                        && script_is_paused
                        && script_services.visual.queue_automatic_scene_fade_in()
                    {
                        changed = true;
                    }
                    match script_services.visual.start_pending(
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
                    accumulator -= tick;
                }
                if opening_menu.is_some()
                    || (dialog.is_none()
                        && (script_services.field_menu.is_some()
                            || script_services.inventory_menu.is_some()
                            || script_services.confirmation_menu.is_some()
                            || script_services.shop_menu.is_some()))
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
                            opening_intro: opening_intro.as_ref(),
                            opening_menu: opening_menu.as_ref(),
                            opening_background: &opening_background,
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
                            magic_effect_sprites: &magic_effect_sprites,
                            battle_effects: &battle_effects,
                            battle_backgrounds: &battle_backgrounds,
                            battle_selected_enemy: script_services.battle_selected_enemy,
                            battle_command_selected: script_services.battle_command_selected,
                            battle_targeting_enemy: script_services.battle_targeting_enemy,
                            battle_menu: script_services.battle_menu,
                            battle_auto_attack: script_services.battle_auto_attack,
                            battle_event: script_services.battle_events.front().copied(),
                            battle_event_ticks: script_services.battle_event_ticks,
                            battle_kept_effects: &script_services.battle_kept_effects,
                            post_battle: script_services.post_battle.as_ref(),
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
                let simulation_wait = update_tick.saturating_sub(accumulator);
                let dialog_wait = dialog
                    .as_ref()
                    .is_some_and(|active| !active.awaiting_input)
                    .then_some(Duration::from_millis(DIALOG_POLL_INTERVAL_MS));
                target.set_control_flow(ControlFlow::WaitUntil(
                    now + dialog_wait.map_or(simulation_wait, |wait| simulation_wait.min(wait)),
                ));
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
    use crate::window::dialog_text::{center_window_width_units, dialog_text_width, dialog_title};
    use crate::window::presentation::cycle_dialog_icon_palette;
    use crate::window::text_render::{dialog_color_after, DialogTextMode};

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
    fn each_original_message_remains_exactly_one_dialog_line() {
        let full_width_line = [0xa4, 0x40].repeat(14);
        let text = text_library(&[b"Name:", &full_width_line, b"last"]);
        let mut dialog = ActiveDialog::new(0, DialogPosition::Upper, 0x4f, None, false, 24);
        dialog.message_ids = vec![0, 1, 2];

        let lines = dialog_body_lines(&text, &dialog);
        assert_eq!(lines, [&full_width_line, b"last".as_slice()]);
        assert_eq!(center_window_width_units(&full_width_line), 28);
    }

    #[test]
    fn dialog_title_does_not_consume_one_of_four_body_lines() {
        let text = text_library(&[b"Name:", b"one", b"two", b"three", b"four"]);
        let mut dialog = ActiveDialog::new(0, DialogPosition::Upper, 0x4f, None, false, 24);
        dialog.message_ids = vec![0, 1, 2, 3, 4];
        dialog.awaiting_input = true;
        assert_eq!(dialog_title(&text, &dialog), Some(b"Name:".as_slice()));
        assert_eq!(dialog_body_lines(&text, &dialog).len(), 4);
        assert_eq!(dialog_page_count(&text, &dialog), 1);
    }

    #[test]
    fn dialog_controls_do_not_consume_layout_width() {
        assert_eq!(dialog_text_width(b"A-$03B"), 16);
        assert_eq!(dialog_text_width(br"A\$B"), 24);
        assert_eq!(
            dialog_color_after(b"-cyan", 0x4f, DialogTextMode::Normal),
            0x8d
        );
        assert_eq!(
            dialog_color_after(b"still cyan-", 0x8d, DialogTextMode::Normal),
            0x4f
        );
        assert_eq!(
            dialog_color_after(br#"\"literal"#, 0x4f, DialogTextMode::Normal),
            0x4f
        );
        assert_eq!(
            dialog_color_after(b"\"quoted\"", 0x4f, DialogTextMode::CenterWindow),
            0x4f
        );
    }

    #[test]
    fn dialog_playback_applies_speed_terminal_delay_and_icon_controls() {
        let text = text_library(&[b"A$07BC", b"A~70ignored", b"A)", b"B"]);
        let mut persistent_delay = 24;
        let mut dialog = ActiveDialog::new(0, DialogPosition::Upper, 0x4f, None, false, 24);
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

        let mut terminal = ActiveDialog::new(1, DialogPosition::Upper, 0x4f, None, false, 24);
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

        let mut icon = ActiveDialog::new(2, DialogPosition::Upper, 0x4f, None, false, 24);
        icon.wait_after_reveal = true;
        assert_eq!(
            advance_dialog_playback(&text, &mut icon, &mut persistent_delay, 0, true),
            DialogPlayback::AwaitingInput
        );
        assert_eq!(icon.wait_icon, 1);

        let mut reset_icon = ActiveDialog::new(2, DialogPosition::Upper, 0x4f, None, false, 24);
        reset_icon.message_ids.push(3);
        reset_icon.wait_after_reveal = true;
        assert_eq!(
            advance_dialog_playback(&text, &mut reset_icon, &mut persistent_delay, 0, true),
            DialogPlayback::AwaitingInput
        );
        assert_eq!(reset_icon.wait_icon, 0);
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
