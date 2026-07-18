//! Native window and map framebuffer presentation.

mod debug_render;
mod dialog_text;
mod draw;
mod input;
mod menu_render;
mod menu_state;
mod scene_render;
mod text_render;
mod types;

use std::time::{Duration, Instant};

use crate::audio::{BackgroundMusic, SoundEffects};
use crate::debug_overlay::{DebugOverlay, DebugSnapshot};
use crate::renderer::Renderer;
use pal_assets::rle::RleBitmap;
use pal_assets::script::ScriptTable;
use pal_assets::text::{BitmapFont, TextLibrary};
use pal_core::game::{AutoScriptError, GameState, UPDATE_INTERVAL_MS};
use pal_core::role::{Direction, RoleSprites};
#[cfg(test)]
use pal_core::scene::SceneObject;
use pal_core::script::{
    DialogPosition, ScriptCondition, ScriptDebugSnapshot, ScriptEvent, ScriptOpcode, ScriptRuntime,
};
use pixels::{Pixels, SurfaceTexture};
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::WindowBuilder;

use debug_render::{
    debug_object_snapshot, focused_debug_object, render_collision_overlay, render_object_overlay,
};
#[cfg(test)]
use debug_render::{
    object_debug_color, OBJECT_AUTO_COLOR, OBJECT_BOTH_COLOR, OBJECT_FOCUS_COLOR,
    OBJECT_HIDDEN_COLOR, OBJECT_INERT_COLOR, OBJECT_TRIGGER_COLOR,
};
#[cfg(test)]
use dialog_text::wrap_big5_lines;
use dialog_text::{
    dialog_body_lines, dialog_layout, dialog_page_count, dialog_text_width, dialog_title,
};
use draw::{fill_rect, stroke_rect};
use input::HeldInput;
use menu_render::{
    render_confirmation_menu, render_field_menu, render_inventory_menu, render_shop_menu,
};
use menu_state::{
    update_wrapping_selection, ConfirmationMenu, EquipSession, FieldMenu, InventoryMenu,
    InventoryMode, ItemUseSession, MagicSession, ShopMenu, ShopMode,
};
pub use scene_render::render_tile_map;
#[cfg(test)]
use scene_render::{cover_tile_candidate, covering_tile_y};
use text_render::{draw_dialog_text, draw_dialog_wait_icon};
pub use types::{GameResources, LoadedScene, Viewport};

#[derive(Debug, Clone)]
struct ActiveDialog {
    pub(super) message_ids: Vec<u16>,
    pub(super) position: DialogPosition,
    pub(super) font_color: u8,
    pub(super) face_index: Option<u16>,
    pub(super) page: usize,
    pub(super) awaiting_input: bool,
    pub(super) auto_wait_ticks: Option<u16>,
}

#[derive(Clone, Copy)]
struct UiRenderContext<'a> {
    dialog: Option<&'a ActiveDialog>,
    field_menu: Option<&'a FieldMenu>,
    inventory_menu: Option<&'a InventoryMenu>,
    confirmation_menu: Option<&'a ConfirmationMenu>,
    shop_menu: Option<&'a ShopMenu>,
    text: &'a TextLibrary,
    font: &'a BitmapFont,
    dialog_faces: &'a [Option<RleBitmap>],
}

struct ScriptServices {
    pending_enter_script: Option<u16>,
    pending_dialog: Option<ActiveDialog>,
    field_menu: Option<FieldMenu>,
    main_menu_selected: usize,
    inventory_action_selected: usize,
    inventory_selected: usize,
    item_target_selected: usize,
    magic_caster_selected: usize,
    magic_selected: usize,
    magic_target_selected: usize,
    system_selected: usize,
    confirmation_menu: Option<ConfirmationMenu>,
    shop_menu: Option<ShopMenu>,
    inventory_menu: Option<InventoryMenu>,
    item_use: Option<ItemUseSession>,
    equip: Option<EquipSession>,
    magic: Option<MagicSession>,
    auto_scripts: ScriptTable,
    sound_effects: SoundEffects,
    music: BackgroundMusic,
}

#[derive(Clone, Copy)]
struct ScriptRenderResources<'a> {
    text: &'a TextLibrary,
    role_sprites: &'a RoleSprites,
}

pub fn run_game_window<L>(
    mut renderer: Renderer,
    mut game: GameState,
    resources: GameResources,
    mut load_scene: L,
) where
    L: FnMut(u16, &RoleSprites) -> Option<LoadedScene> + 'static,
{
    let GameResources {
        role_sprites,
        script_table,
        initial_enter_script,
        text,
        font,
        dialog_faces,
        voc_mkf,
        midi_mkf,
        sound_font,
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
    let mut scripts = ScriptRuntime::new(script_table);
    let mut dialog = None;
    let mut script_services = ScriptServices {
        pending_enter_script: None,
        pending_dialog: None,
        field_menu: None,
        main_menu_selected: 0,
        inventory_action_selected: 0,
        inventory_selected: 0,
        item_target_selected: 0,
        magic_caster_selected: 0,
        magic_selected: 0,
        magic_target_selected: 0,
        system_selected: 0,
        confirmation_menu: None,
        shop_menu: None,
        inventory_menu: None,
        item_use: None,
        equip: None,
        magic: None,
        auto_scripts,
        sound_effects: SoundEffects::new(&voc_mkf).expect("failed to load VOC sound effects"),
        music: BackgroundMusic::new(&midi_mkf, &sound_font)
            .expect("failed to load MIDI music and SoundFont"),
    };
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
            text: &text,
            font: &font,
            dialog_faces: &dialog_faces,
        },
    );

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
                                    if game.encode_snapshot().is_some_and(|bytes| {
                                        write_snapshot(&snapshot_path, &bytes).is_ok()
                                    }) {
                                        window.set_title("Rust-PAL [Snapshot saved]");
                                    } else {
                                        window.set_title("Rust-PAL [Snapshot save failed]");
                                    }
                                    true
                                }
                                KeyCode::F9 if !scripts.is_active() && dialog.is_none() => {
                                    let saved = std::fs::read(&snapshot_path)
                                        .ok()
                                        .and_then(|bytes| game.decode_snapshot(&bytes));
                                    if let Some(saved) = saved {
                                        if let Some(scene) =
                                            load_scene(saved.scene_number(), &role_sprites)
                                        {
                                            game.restore_snapshot(saved, scene.map);
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
                                        } else {
                                            window
                                                .set_title("Rust-PAL [Snapshot scene unavailable]");
                                        }
                                    } else {
                                        window.set_title("Rust-PAL [No snapshot]");
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
                                    text: &text,
                                    font: &font,
                                    dialog_faces: &dialog_faces,
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
                let now = Instant::now();
                accumulator += now
                    .duration_since(last_update)
                    .min(Duration::from_millis(250));
                last_update = now;

                let mut changed = false;
                while accumulator >= tick {
                    let sampled = input.sample();
                    if dialog.is_some() {
                        let awaiting_input = dialog
                            .as_ref()
                            .is_some_and(|active_dialog| active_dialog.awaiting_input);
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
                            } else if let Some(pending) = script_services.pending_dialog.take() {
                                dialog = Some(pending);
                            } else {
                                dialog = None;
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
                        } else if !awaiting_input {
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
                        }
                    } else if let Some(menu) = script_services.confirmation_menu.as_mut() {
                        changed = sampled.confirm || sampled.cancel || sampled.direction.is_some();
                        if matches!(sampled.direction, Some(Direction::West | Direction::North)) {
                            menu.selected_yes = false;
                        } else if matches!(
                            sampled.direction,
                            Some(Direction::East | Direction::South)
                        ) {
                            menu.selected_yes = true;
                        }
                        if sampled.confirm || sampled.cancel {
                            let no_entry = menu.no_entry;
                            let selected_no = sampled.cancel || !menu.selected_yes;
                            script_services.confirmation_menu = None;
                            if selected_no {
                                scripts.branch_to(no_entry);
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
                    } else if let Some(mut menu) = script_services.field_menu.take() {
                        changed = sampled.confirm || sampled.cancel || sampled.direction.is_some();
                        let mut keep_menu = true;
                        match &mut menu {
                            FieldMenu::Main { selected } => {
                                update_wrapping_selection(selected, sampled.direction, 4);
                                script_services.main_menu_selected = *selected;
                                if sampled.cancel {
                                    keep_menu = false;
                                    window.set_title("Rust-PAL");
                                } else if sampled.confirm {
                                    match *selected {
                                        0 => {
                                            menu = FieldMenu::Status { selected: 0 };
                                            window.set_title("Rust-PAL [Status]");
                                        }
                                        1 => {
                                            let selected = script_services
                                                .magic_caster_selected
                                                .min(game.party.members().len().saturating_sub(1));
                                            menu = FieldMenu::MagicCaster { selected };
                                            window.set_title("Rust-PAL [Magic]");
                                        }
                                        2 => {
                                            menu = FieldMenu::InventoryAction {
                                                selected: script_services.inventory_action_selected,
                                            };
                                            window.set_title("Rust-PAL [Inventory]");
                                        }
                                        3 => {
                                            menu = FieldMenu::System {
                                                selected: script_services.system_selected,
                                            };
                                            window.set_title("Rust-PAL [System]");
                                        }
                                        _ => unreachable!(),
                                    }
                                }
                            }
                            FieldMenu::InventoryAction { selected } => {
                                update_wrapping_selection(selected, sampled.direction, 2);
                                script_services.inventory_action_selected = *selected;
                                if sampled.cancel {
                                    keep_menu = false;
                                    window.set_title("Rust-PAL");
                                } else if sampled.confirm {
                                    keep_menu = false;
                                    let selected_index = script_services.inventory_selected;
                                    script_services.inventory_menu = Some(InventoryMenu {
                                        selected: selected_index,
                                        mode: if *selected == 0 {
                                            InventoryMode::EquipItems
                                        } else {
                                            InventoryMode::Items
                                        },
                                    });
                                    window.set_title(if *selected == 0 {
                                        "Rust-PAL [Equip item]"
                                    } else {
                                        "Rust-PAL [Use item]"
                                    });
                                }
                            }
                            FieldMenu::Status { selected } => {
                                update_wrapping_selection(
                                    selected,
                                    sampled.direction,
                                    game.party.members().len(),
                                );
                                if sampled.cancel {
                                    keep_menu = false;
                                    window.set_title("Rust-PAL");
                                }
                            }
                            FieldMenu::MagicCaster { selected } => {
                                let member_count = game.party.members().len();
                                update_wrapping_selection(
                                    selected,
                                    sampled.direction,
                                    member_count,
                                );
                                script_services.magic_caster_selected = *selected;
                                if sampled.cancel {
                                    keep_menu = false;
                                    window.set_title("Rust-PAL");
                                } else if sampled.confirm {
                                    let role_id = game.party.members()[*selected].role_id;
                                    if game.player_role(role_id).is_some_and(|role| role.hp > 0) {
                                        menu = FieldMenu::MagicList {
                                            caster: *selected,
                                            selected: script_services.magic_selected,
                                        };
                                        window.set_title("Rust-PAL [Magic list]");
                                    }
                                }
                            }
                            FieldMenu::MagicList { caster, selected } => {
                                let role_id = game.party.members()[*caster].role_id;
                                let magics = game.field_magics(role_id);
                                update_wrapping_selection(
                                    selected,
                                    sampled.direction,
                                    magics.len(),
                                );
                                script_services.magic_selected = *selected;
                                if sampled.cancel {
                                    keep_menu = false;
                                    window.set_title("Rust-PAL");
                                } else if sampled.confirm {
                                    if let Some(magic) =
                                        magics.get(*selected).filter(|magic| magic.enabled)
                                    {
                                        if magic.apply_to_all {
                                            if let Some(request) = game.magic_request(
                                                role_id,
                                                magic.magic_id,
                                                None,
                                                false,
                                            ) {
                                                keep_menu = false;
                                                if scripts.start(request) {
                                                    script_services.magic = Some(MagicSession {
                                                        caster_selected: *caster,
                                                        magic_id: magic.magic_id,
                                                        target_selected: None,
                                                        success_phase: false,
                                                    });
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
                                        } else {
                                            menu = FieldMenu::MagicTarget {
                                                caster: *caster,
                                                magic_id: magic.magic_id,
                                                selected: script_services.magic_target_selected,
                                            };
                                            window.set_title("Rust-PAL [Magic target]");
                                        }
                                    }
                                }
                            }
                            FieldMenu::MagicTarget {
                                caster,
                                magic_id,
                                selected,
                            } => {
                                update_wrapping_selection(
                                    selected,
                                    sampled.direction,
                                    game.party.members().len(),
                                );
                                script_services.magic_target_selected = *selected;
                                if sampled.cancel {
                                    menu = FieldMenu::MagicList {
                                        caster: *caster,
                                        selected: script_services.magic_selected,
                                    };
                                    window.set_title("Rust-PAL [Magic list]");
                                } else if sampled.confirm {
                                    let caster_role = game.party.members()[*caster].role_id;
                                    let target_role = game.party.members()[*selected].role_id;
                                    if let Some(request) = game.magic_request(
                                        caster_role,
                                        *magic_id,
                                        Some(target_role),
                                        false,
                                    ) {
                                        keep_menu = false;
                                        if scripts.start(request) {
                                            script_services.magic = Some(MagicSession {
                                                caster_selected: *caster,
                                                magic_id: *magic_id,
                                                target_selected: Some(*selected),
                                                success_phase: false,
                                            });
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
                            }
                            FieldMenu::System { selected } => {
                                update_wrapping_selection(selected, sampled.direction, 5);
                                script_services.system_selected = *selected;
                                if sampled.cancel {
                                    menu = FieldMenu::Main {
                                        selected: script_services.main_menu_selected,
                                    };
                                    window.set_title("Rust-PAL [Menu]");
                                } else if sampled.confirm {
                                    match *selected {
                                        0 => {
                                            if game.encode_snapshot().is_some_and(|bytes| {
                                                write_snapshot(&snapshot_path, &bytes).is_ok()
                                            }) {
                                                window.set_title("Rust-PAL [Saved]");
                                            } else {
                                                window.set_title("Rust-PAL [Save failed]");
                                            }
                                            keep_menu = false;
                                        }
                                        1 => {
                                            let saved = std::fs::read(&snapshot_path)
                                                .ok()
                                                .and_then(|bytes| game.decode_snapshot(&bytes));
                                            if let Some(saved) = saved.and_then(|saved| {
                                                Some((
                                                    load_scene(
                                                        saved.scene_number(),
                                                        &role_sprites,
                                                    )?,
                                                    saved,
                                                ))
                                            }) {
                                                game.restore_snapshot(saved.1, saved.0.map);
                                                window.set_title("Rust-PAL [Loaded]");
                                                keep_menu = false;
                                            } else {
                                                window.set_title("Rust-PAL [No save]");
                                            }
                                        }
                                        2 => {
                                            let enabled = !script_services.music.enabled();
                                            script_services.music.set_enabled(enabled);
                                            if enabled {
                                                if let Some(music_id) = game.current_music {
                                                    script_services.music.play(music_id, true, 0);
                                                }
                                            }
                                        }
                                        3 => {
                                            let enabled = !script_services.sound_effects.enabled();
                                            script_services.sound_effects.set_enabled(enabled);
                                        }
                                        4 => {
                                            keep_menu = false;
                                            target.exit();
                                        }
                                        _ => unreachable!(),
                                    }
                                }
                            }
                        }
                        if keep_menu {
                            script_services.field_menu = Some(menu);
                        }
                    } else if script_services.shop_menu.is_some() {
                        let mut close_shop = false;
                        {
                            let menu = script_services
                                .shop_menu
                                .as_mut()
                                .expect("shop menu was checked above");
                            let items = menu.items(&game);
                            changed =
                                sampled.confirm || sampled.cancel || sampled.direction.is_some();
                            if menu.confirming {
                                if sampled.cancel {
                                    menu.confirming = false;
                                } else {
                                    if matches!(
                                        sampled.direction,
                                        Some(Direction::West | Direction::North)
                                    ) {
                                        menu.selected_yes = false;
                                    } else if matches!(
                                        sampled.direction,
                                        Some(Direction::East | Direction::South)
                                    ) {
                                        menu.selected_yes = true;
                                    }
                                    if sampled.confirm && menu.selected_yes {
                                        if let Some(item) = items.get(menu.selected) {
                                            match menu.mode {
                                                ShopMode::Buy { .. } => {
                                                    game.buy_item(item.item_id);
                                                }
                                                ShopMode::Sell => {
                                                    game.sell_item(item.item_id);
                                                }
                                            }
                                        }
                                    }
                                    if sampled.confirm {
                                        menu.confirming = false;
                                        let remaining = menu.items(&game).len();
                                        menu.selected =
                                            menu.selected.min(remaining.saturating_sub(1));
                                    }
                                }
                            } else if sampled.cancel {
                                close_shop = true;
                            } else {
                                menu.update_selection(sampled.direction, items.len());
                                if sampled.confirm {
                                    if let Some(item) = items.get(menu.selected) {
                                        match menu.mode {
                                            ShopMode::Buy { .. } => {
                                                if game.cash >= u32::from(item.price) {
                                                    menu.confirming = true;
                                                    menu.selected_yes = false;
                                                }
                                            }
                                            ShopMode::Sell => {
                                                menu.confirming = true;
                                                menu.selected_yes = false;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        if close_shop {
                            script_services.shop_menu = None;
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
                    } else if script_services.inventory_menu.is_some() {
                        let mut menu = script_services
                            .inventory_menu
                            .take()
                            .expect("inventory menu was checked above");
                        changed = sampled.confirm || sampled.cancel || sampled.direction.is_some();
                        let mut close_menu = false;
                        let mut item_request = None;
                        let mut equip_request = None;
                        match menu.mode {
                            InventoryMode::Items => {
                                let inventory = game.inventory().collect::<Vec<_>>();
                                if sampled.cancel {
                                    close_menu = true;
                                } else {
                                    menu.update(sampled.direction, inventory.len());
                                    if sampled.confirm {
                                        if let Some(&(item_id, _)) = inventory.get(menu.selected) {
                                            if let Some(item) = game.usable_item(item_id) {
                                                if item.apply_to_all {
                                                    item_request = game
                                                        .item_use_request(item_id, None)
                                                        .map(|request| (item_id, request, true));
                                                } else {
                                                    menu.mode = InventoryMode::Target {
                                                        item_id,
                                                        selected: script_services
                                                            .item_target_selected
                                                            .min(
                                                                game.party
                                                                    .members()
                                                                    .len()
                                                                    .saturating_sub(1),
                                                            ),
                                                    };
                                                    window.set_title("Rust-PAL [Item target]");
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            InventoryMode::EquipItems => {
                                let inventory = game.equippable_inventory();
                                menu.selected =
                                    menu.selected.min(inventory.len().saturating_sub(1));
                                if sampled.cancel {
                                    close_menu = true;
                                } else {
                                    menu.update(sampled.direction, inventory.len());
                                    if sampled.confirm {
                                        if let Some(&(item_id, _)) = inventory.get(menu.selected) {
                                            menu.mode = InventoryMode::EquipTarget {
                                                item_id,
                                                selected: script_services.item_target_selected.min(
                                                    game.party.members().len().saturating_sub(1),
                                                ),
                                            };
                                            window.set_title("Rust-PAL [Equip target]");
                                        }
                                    }
                                }
                            }
                            InventoryMode::EquipTarget {
                                item_id,
                                mut selected,
                            } => {
                                update_wrapping_selection(
                                    &mut selected,
                                    sampled.direction,
                                    game.party.members().len(),
                                );
                                script_services.item_target_selected = selected;
                                menu.mode = InventoryMode::EquipTarget { item_id, selected };
                                if sampled.cancel {
                                    menu.mode = InventoryMode::EquipItems;
                                    window.set_title("Rust-PAL [Equip item]");
                                } else if sampled.confirm {
                                    equip_request = game
                                        .party
                                        .members()
                                        .get(selected)
                                        .and_then(|member| {
                                            game.item_equip_request(item_id, member.role_id)
                                        })
                                        .map(|request| (item_id, selected, request));
                                }
                            }
                            InventoryMode::Target {
                                item_id,
                                mut selected,
                            } => {
                                let member_count = game.party.members().len();
                                update_wrapping_selection(
                                    &mut selected,
                                    sampled.direction,
                                    member_count,
                                );
                                script_services.item_target_selected = selected;
                                menu.mode = InventoryMode::Target { item_id, selected };
                                if sampled.cancel {
                                    menu.mode = InventoryMode::Items;
                                    window.set_title("Rust-PAL [Inventory]");
                                } else if sampled.confirm {
                                    item_request = game
                                        .party
                                        .members()
                                        .get(selected)
                                        .and_then(|member| {
                                            game.item_use_request(item_id, Some(member.role_id))
                                        })
                                        .map(|request| (item_id, request, false));
                                }
                            }
                        }
                        script_services.inventory_selected = menu.selected;
                        if let Some((item_id, role_selected, request)) = equip_request {
                            if scripts.start(request) {
                                script_services.equip = Some(EquipSession {
                                    item_id,
                                    inventory_selected: menu.selected,
                                    role_selected,
                                });
                                window.set_title("Rust-PAL [Equipping]");
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
                            } else {
                                script_services.inventory_menu = Some(menu);
                            }
                        } else if let Some((item_id, request, apply_to_all)) = item_request {
                            if scripts.start(request) {
                                script_services.item_use = Some(ItemUseSession {
                                    item_id,
                                    inventory_selected: menu.selected,
                                    apply_to_all,
                                });
                                window.set_title("Rust-PAL [Using item]");
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
                            } else {
                                script_services.inventory_menu = Some(menu);
                            }
                        } else if !close_menu {
                            script_services.inventory_menu = Some(menu);
                        } else {
                            window.set_title("Rust-PAL");
                        }
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
                    accumulator -= tick;
                }
                if changed {
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
                            text: &text,
                            font: &font,
                            dialog_faces: &dialog_faces,
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

fn auto_script_error_title(error: AutoScriptError) -> String {
    match error {
        AutoScriptError::InvalidEntry { object_id, entry } => {
            format!("Rust-PAL [object {object_id} invalid auto script {entry}]")
        }
        AutoScriptError::MissingObject {
            object_id,
            entry,
            target_id,
        } => {
            format!("Rust-PAL [object {object_id} auto script {entry} missing object {target_id}]")
        }
        AutoScriptError::Unsupported {
            object_id,
            entry,
            opcode,
        } => format!(
            "Rust-PAL [object {object_id} auto script {entry} {}]",
            opcode_label(opcode)
        ),
        AutoScriptError::InstructionLimit { object_id, entry } => {
            format!("Rust-PAL [object {object_id} auto script loop at {entry}]")
        }
    }
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

fn write_snapshot(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, bytes)?;
    match std::fs::rename(&temporary, path) {
        Ok(()) => Ok(()),
        Err(_error) if path.exists() => {
            std::fs::remove_file(path)?;
            std::fs::rename(temporary, path)
        }
        Err(error) => Err(error),
    }
}

fn render_game(
    renderer: &mut Renderer,
    game: &GameState,
    role_sprites: &RoleSprites,
    show_collision: bool,
    show_objects: bool,
    script: ScriptDebugSnapshot,
    ui: UiRenderContext<'_>,
) {
    let viewport = Viewport::from(game.camera);
    let roles = std::iter::once(&game.player)
        .chain(game.party_followers())
        .cloned()
        .collect::<Vec<_>>();
    render_tile_map(
        renderer,
        &game.map,
        Some(role_sprites),
        &roles,
        &game.scene_objects,
        viewport,
    );
    if show_collision {
        render_collision_overlay(renderer, &game.map, &game.player, viewport);
    }
    if show_objects {
        let focused_object_id = focused_debug_object(game, script).map(|object| object.id);
        render_object_overlay(renderer, &game.scene_objects, viewport, focused_object_id);
    }
    if let Some(dialog) = ui.dialog {
        render_dialog(renderer, ui.text, ui.font, ui.dialog_faces, dialog);
    } else if let Some(menu) = ui.confirmation_menu {
        render_confirmation_menu(renderer, ui.text, ui.font, *menu);
    } else if let Some(menu) = ui.field_menu {
        render_field_menu(renderer, game, ui.text, ui.font, *menu);
    } else if let Some(menu) = ui.shop_menu {
        render_shop_menu(renderer, game, ui.text, ui.font, *menu);
    } else if let Some(menu) = ui.inventory_menu {
        render_inventory_menu(renderer, game, ui.text, ui.font, *menu);
    }
}

fn advance_script<L>(
    scripts: &mut ScriptRuntime,
    game: &mut GameState,
    dialog: &mut Option<ActiveDialog>,
    resources: ScriptRenderResources<'_>,
    load_scene: &mut L,
    services: &mut ScriptServices,
    set_title: &mut impl FnMut(&str),
) where
    L: FnMut(u16, &RoleSprites) -> Option<LoadedScene>,
{
    match scripts.advance() {
        Some(ScriptEvent::Message {
            message_id,
            position,
            font_color,
            face_index,
        }) => {
            let mut next = ActiveDialog {
                message_ids: vec![message_id],
                position,
                font_color,
                face_index,
                page: 0,
                awaiting_input: position == DialogPosition::CenterWindow,
                auto_wait_ticks: (position == DialogPosition::CenterWindow).then_some(28),
            };
            if let Some(active) = dialog.as_mut() {
                if active.position == position
                    && active.font_color == font_color
                    && active.face_index == face_index
                    && !active.awaiting_input
                {
                    active.message_ids.push(message_id);
                    active.awaiting_input = dialog_body_lines(resources.text, active).len() >= 4;
                } else {
                    active.awaiting_input = true;
                    next.awaiting_input |= dialog_body_lines(resources.text, &next).len() >= 4;
                    services.pending_dialog = Some(next);
                }
            } else {
                next.awaiting_input |= dialog_body_lines(resources.text, &next).len() >= 4;
                *dialog = Some(next);
            }
            set_title("Rust-PAL [Dialog]");
        }
        Some(ScriptEvent::Waiting) => {
            update_trigger_world(game, services, set_title);
        }
        Some(ScriptEvent::Delay) => {}
        Some(ScriptEvent::Confirm { no_entry }) => {
            services.confirmation_menu = Some(ConfirmationMenu {
                no_entry,
                selected_yes: false,
            });
            set_title("Rust-PAL [Confirm]");
        }
        Some(ScriptEvent::OpenBuyMenu { store_number }) => {
            if game.store_items(store_number).is_none() {
                set_title("Rust-PAL [invalid store]");
                return;
            }
            services.shop_menu = Some(ShopMenu {
                mode: ShopMode::Buy { store_number },
                selected: 0,
                confirming: false,
                selected_yes: false,
            });
            set_title("Rust-PAL [Buy]");
        }
        Some(ScriptEvent::OpenSellMenu) => {
            services.shop_menu = Some(ShopMenu {
                mode: ShopMode::Sell,
                selected: 0,
                confirming: false,
                selected_yes: false,
            });
            set_title("Rust-PAL [Sell]");
        }
        Some(ScriptEvent::FadeScene { .. }) => {}
        Some(ScriptEvent::Action(pal_core::script::ScriptAction::ChangeScene { scene_number })) => {
            if scene_number == game.scene_number {
                return;
            }
            let Some(scene) = load_scene(scene_number, resources.role_sprites) else {
                set_title("Rust-PAL [failed to load scene]");
                return;
            };
            game.replace_scene(scene.number, scene.map, scene.objects);
            let enter_script = game.scene_enter_script(scene.enter_script);
            services.pending_enter_script = (enter_script != 0).then_some(enter_script);
            set_title(&format!("Rust-PAL [scene {}]", scene.number));
        }
        Some(ScriptEvent::Action(pal_core::script::ScriptAction::PlaySound { sound_id }))
            if !services.sound_effects.play(sound_id) =>
        {
            set_title(&format!("Rust-PAL [invalid sound {sound_id}]"));
        }
        Some(ScriptEvent::Action(pal_core::script::ScriptAction::PlaySound { .. })) => {}
        Some(ScriptEvent::Action(pal_core::script::ScriptAction::PlayMusic {
            music_id,
            looped,
            fade_seconds,
        })) => {
            if services.music.play(music_id, looped, fade_seconds) {
                game.apply_script_action(pal_core::script::ScriptAction::PlayMusic {
                    music_id,
                    looped,
                    fade_seconds,
                });
            } else {
                set_title(&format!("Rust-PAL [invalid music {music_id}]"));
            }
        }
        Some(ScriptEvent::Action(pal_core::script::ScriptAction::AdjustCash {
            amount,
            insufficient_entry,
        })) if !game.adjust_cash(amount) => {
            scripts.branch_to(insufficient_entry);
        }
        Some(ScriptEvent::Action(pal_core::script::ScriptAction::AdjustCash { .. })) => {}
        Some(ScriptEvent::Action(pal_core::script::ScriptAction::RemoveItem {
            item_id,
            amount,
            insufficient_entry,
        })) => {
            if let (false, 1..) = (
                game.remove_item(item_id, amount, insufficient_entry),
                insufficient_entry,
            ) {
                scripts.branch_to(insufficient_entry);
            }
        }
        Some(ScriptEvent::Action(
            action @ (pal_core::script::ScriptAction::AdjustPlayerHealth { .. }
            | pal_core::script::ScriptAction::RevivePlayer { .. }),
        )) => {
            let succeeded = game.apply_script_action(action);
            scripts.set_success(succeeded);
        }
        Some(ScriptEvent::Action(
            action @ pal_core::script::ScriptAction::PlaceObjectInFront { blocked_entry, .. },
        )) if !game.apply_script_action(action) => {
            scripts.set_success(false);
            scripts.branch_to(blocked_entry);
        }
        Some(ScriptEvent::Action(pal_core::script::ScriptAction::PlaceObjectInFront {
            ..
        })) => {}
        Some(ScriptEvent::Action(pal_core::script::ScriptAction::WalkObjectTo {
            object_id,
            tile_x,
            tile_y,
            half,
            speed,
            repeat_entry,
        })) => match game.walk_object_to(object_id, tile_x, tile_y, half, speed) {
            Some(true) => {}
            Some(false) => {
                scripts.branch_to(repeat_entry);
            }
            None => set_title("Rust-PAL [script walk target is unavailable]"),
        },
        Some(ScriptEvent::Action(pal_core::script::ScriptAction::WalkPlayerTo {
            tile_x,
            tile_y,
            half,
            speed,
            repeat_entry,
        })) => match game.walk_player_to(tile_x, tile_y, half, speed) {
            Some(true) => {}
            Some(false) => {
                scripts.branch_to(repeat_entry);
            }
            None => set_title("Rust-PAL [script party walk target is unavailable]"),
        },
        Some(ScriptEvent::Action(pal_core::script::ScriptAction::RideObjectTo {
            object_id,
            tile_x,
            tile_y,
            half,
            speed,
            repeat_entry,
        })) => {
            match game.ride_object_to(object_id, tile_x, tile_y, half, speed) {
                Some(true) => {}
                Some(false) => {
                    scripts.branch_to(repeat_entry);
                }
                None => set_title("Rust-PAL [script ride target is unavailable]"),
            }
            update_trigger_world(game, services, set_title);
        }
        Some(ScriptEvent::Action(
            action @ pal_core::script::ScriptAction::MoveViewport { x, y, frames },
        )) => {
            game.apply_script_action(action);
            if (x != 0 || y != 0) && frames != -1 {
                update_trigger_world(game, services, set_title);
            }
        }
        Some(ScriptEvent::Condition(condition)) => {
            let (matches, target_entry) = match condition {
                ScriptCondition::ItemCountLess {
                    item_id,
                    amount,
                    target_entry,
                } => (
                    i32::from(game.item_count(item_id)) < i32::from(amount),
                    target_entry,
                ),
                ScriptCondition::ObjectStateEquals {
                    object_id,
                    state,
                    target_entry,
                } => (game.object_state(object_id) == Some(state), target_entry),
                ScriptCondition::SceneEquals {
                    scene_number,
                    target_entry,
                } => (game.scene_number == scene_number, target_entry),
                ScriptCondition::PartyContainsName {
                    name_word_id,
                    target_entry,
                } => (game.party_contains_name(name_word_id), target_entry),
                ScriptCondition::PlayerFacesObject {
                    object_id,
                    range,
                    target_entry,
                } => (!game.player_faces_object(object_id, range), target_entry),
                ScriptCondition::PartyNotFullHp { target_entry } => {
                    (game.party_not_full_hp(), target_entry)
                }
                ScriptCondition::ItemNotEquipped {
                    item_id,
                    amount,
                    target_entry,
                } => (game.equipped_item_count(item_id) < amount, target_entry),
            };
            if matches {
                scripts.branch_to(target_entry);
            }
        }
        Some(ScriptEvent::Action(action)) if !game.apply_script_action(action) => {
            set_title("Rust-PAL [script target is unavailable]");
        }
        Some(ScriptEvent::Action(_)) => {}
        Some(ScriptEvent::Completed {
            trigger,
            next_entry,
            succeeded,
        }) => {
            if trigger.kind == pal_core::scene::TriggerKind::Item {
                if let Some(item_use) = services.item_use.take() {
                    game.finish_item_use(item_use.item_id, next_entry, succeeded);
                    let selected = item_use
                        .inventory_selected
                        .min(game.inventory().len().saturating_sub(1));
                    services.inventory_selected = selected;
                    if item_use.apply_to_all {
                        services.inventory_menu = None;
                        set_title("Rust-PAL");
                    } else if game.usable_item(item_use.item_id).is_some() {
                        let target_selected = game
                            .party
                            .members()
                            .iter()
                            .position(|member| member.role_id == trigger.object_id)
                            .unwrap_or(0);
                        services.item_target_selected = target_selected;
                        services.inventory_menu = Some(InventoryMenu {
                            selected,
                            mode: InventoryMode::Target {
                                item_id: item_use.item_id,
                                selected: target_selected,
                            },
                        });
                        set_title("Rust-PAL [Item target]");
                    } else {
                        services.inventory_menu = Some(InventoryMenu {
                            selected,
                            mode: InventoryMode::Items,
                        });
                        set_title("Rust-PAL [Use item]");
                    }
                }
                return;
            } else if trigger.kind == pal_core::scene::TriggerKind::Equip {
                if let Some(equip) = services.equip.take() {
                    game.finish_item_equip(equip.item_id, next_entry);
                    let selected = equip
                        .inventory_selected
                        .min(game.equippable_inventory().len().saturating_sub(1));
                    services.inventory_selected = selected;
                    services.item_target_selected = equip.role_selected;
                    services.inventory_menu = Some(InventoryMenu {
                        selected,
                        mode: InventoryMode::EquipItems,
                    });
                    set_title("Rust-PAL [Equip item]");
                }
                return;
            } else if trigger.kind == pal_core::scene::TriggerKind::Magic {
                if let Some(mut magic) = services.magic.take() {
                    game.finish_magic_script(magic.magic_id, next_entry, magic.success_phase);
                    let caster_role = game.party.members()[magic.caster_selected].role_id;
                    if succeeded && !magic.success_phase {
                        let target_role = magic
                            .target_selected
                            .map(|selected| game.party.members()[selected].role_id);
                        if let Some(request) =
                            game.magic_request(caster_role, magic.magic_id, target_role, true)
                        {
                            magic.success_phase = true;
                            services.magic = Some(magic);
                            scripts.start(request);
                            set_title("Rust-PAL [Casting]");
                            return;
                        }
                    }
                    if succeeded {
                        game.consume_magic_mp(caster_role, magic.magic_id);
                    }
                    let available = game
                        .field_magics(caster_role)
                        .into_iter()
                        .any(|field_magic| {
                            field_magic.magic_id == magic.magic_id && field_magic.enabled
                        });
                    if available {
                        if let Some(selected) = magic.target_selected {
                            services.field_menu = Some(FieldMenu::MagicTarget {
                                caster: magic.caster_selected,
                                magic_id: magic.magic_id,
                                selected,
                            });
                            set_title("Rust-PAL [Magic target]");
                        } else {
                            services.field_menu = Some(FieldMenu::MagicList {
                                caster: magic.caster_selected,
                                selected: services.magic_selected,
                            });
                            set_title("Rust-PAL [Magic list]");
                        }
                    } else {
                        set_title("Rust-PAL");
                    }
                }
                return;
            } else if trigger.object_id == 0xffff {
                game.update_scene_enter_script(next_entry);
            } else {
                if let Some(object) = game
                    .scene_objects
                    .iter_mut()
                    .find(|object| object.id == trigger.object_id)
                {
                    object.trigger_script = next_entry;
                }
            }
            if let Some(entry) = services.pending_enter_script.take() {
                let trigger = pal_core::scene::TriggerRequest {
                    object_id: 0xffff,
                    script_entry: entry,
                    kind: pal_core::scene::TriggerKind::Touch,
                };
                scripts.start(trigger);
            } else {
                set_title("Rust-PAL");
            }
            if let Some(active) = dialog.as_mut() {
                active.awaiting_input = true;
            }
        }
        Some(ScriptEvent::Unsupported {
            trigger,
            entry,
            opcode,
        }) => {
            resume_script_menu_after_error(game, services, trigger.kind);
            set_title(&format!(
                "Rust-PAL [unsupported script {entry} {}]",
                opcode_label(opcode)
            ));
        }
        Some(ScriptEvent::InvalidEntry { trigger, entry }) => {
            resume_script_menu_after_error(game, services, trigger.kind);
            set_title(&format!("Rust-PAL [invalid script entry {entry}]"));
        }
        Some(ScriptEvent::InstructionLimit { trigger, entry }) => {
            resume_script_menu_after_error(game, services, trigger.kind);
            set_title(&format!("Rust-PAL [script loop at {entry}]"));
        }
        None => {}
    }
}

fn opcode_label(raw: u16) -> String {
    ScriptOpcode::from_raw(raw).map_or_else(
        || format!("opcode {raw:04X}"),
        |opcode| format!("{} ({raw:04X})", opcode.mnemonic()),
    )
}

fn resume_inventory_after_item_error(game: &GameState, services: &mut ScriptServices) {
    let Some(item_use) = services.item_use.take() else {
        return;
    };
    let selected = item_use
        .inventory_selected
        .min(game.inventory().len().saturating_sub(1));
    services.inventory_selected = selected;
    services.inventory_menu = Some(InventoryMenu {
        selected,
        mode: InventoryMode::Items,
    });
}

fn resume_script_menu_after_error(
    game: &GameState,
    services: &mut ScriptServices,
    kind: pal_core::scene::TriggerKind,
) {
    match kind {
        pal_core::scene::TriggerKind::Item => resume_inventory_after_item_error(game, services),
        pal_core::scene::TriggerKind::Equip => {
            if let Some(equip) = services.equip.take() {
                services.inventory_menu = Some(InventoryMenu {
                    selected: equip
                        .inventory_selected
                        .min(game.equippable_inventory().len().saturating_sub(1)),
                    mode: InventoryMode::EquipItems,
                });
            }
        }
        pal_core::scene::TriggerKind::Magic => {
            if let Some(magic) = services.magic.take() {
                services.field_menu = Some(FieldMenu::MagicList {
                    caster: magic.caster_selected,
                    selected: services.magic_selected,
                });
            }
        }
        pal_core::scene::TriggerKind::Search | pal_core::scene::TriggerKind::Touch => {}
    }
}

fn update_trigger_world(
    game: &mut GameState,
    services: &mut ScriptServices,
    set_title: &mut impl FnMut(&str),
) {
    match game.update_auto_scripts(&services.auto_scripts) {
        Ok(_) => {}
        Err(error) => set_title(&auto_script_error_title(error)),
    }
    for sound_id in game.take_auto_script_sounds() {
        if !services.sound_effects.play(sound_id) {
            set_title(&format!("Rust-PAL [invalid auto sound {sound_id}]"));
        }
    }
}

fn render_dialog(
    renderer: &mut Renderer,
    text: &TextLibrary,
    font: &BitmapFont,
    faces: &[Option<RleBitmap>],
    dialog: &ActiveDialog,
) {
    let layout = dialog_layout(dialog);
    if let Some(face_index) = dialog.face_index {
        if let Some(Some(face)) = faces.get(usize::from(face_index)) {
            let (center_x, center_y) = match dialog.position {
                DialogPosition::Upper => (48, 55),
                DialogPosition::Lower => (270, 144),
                _ => (0, 0),
            };
            if center_x != 0 {
                renderer.blit_rle(
                    face,
                    center_x - i32::from(face.width) / 2,
                    center_y - i32::from(face.height) / 2,
                );
            }
        }
    }

    if let Some(title) = dialog_title(text, dialog) {
        draw_dialog_text(renderer, font, title, layout.title_x, layout.title_y, 0x8c);
    }

    let lines = dialog_body_lines(text, dialog);
    let visible = lines
        .iter()
        .skip(dialog.page * 4)
        .take(4)
        .copied()
        .collect::<Vec<_>>();
    if dialog.position == DialogPosition::CenterWindow {
        render_center_dialog_window(renderer, font, dialog, &visible);
        return;
    }

    let mut last_end = None;
    for (line, bytes) in visible.iter().enumerate() {
        let y = layout.text_y + line as i32 * 18;
        let end = draw_dialog_text(renderer, font, bytes, layout.text_x, y, dialog.font_color);
        last_end = Some((end, y));
    }
    if dialog.awaiting_input {
        if let Some((x, y)) = last_end {
            draw_dialog_wait_icon(renderer, x + 2, y + 5);
        }
    }
}

fn render_center_dialog_window(
    renderer: &mut Renderer,
    font: &BitmapFont,
    dialog: &ActiveDialog,
    lines: &[&[u8]],
) {
    let content_width = lines
        .iter()
        .map(|line| dialog_text_width(line))
        .max()
        .unwrap_or(0)
        .clamp(16, 280) as i32;
    let width = content_width + 24;
    let height = lines.len().max(1) as i32 * 18 + 18;
    let x = (320 - width) / 2;
    let y = 40;
    fill_rect(renderer, x + 6, y + 6, width, height, [0, 0, 0, 160]);
    fill_rect(renderer, x, y, width, height, [16, 20, 24, 255]);
    stroke_rect(renderer, x, y, width, height, [232, 224, 192, 255]);
    stroke_rect(
        renderer,
        x + 2,
        y + 2,
        width - 4,
        height - 4,
        [72, 88, 96, 255],
    );
    let mut last_end = None;
    for (line, bytes) in lines.iter().enumerate() {
        let text_x = x + (width - dialog_text_width(bytes) as i32) / 2;
        let text_y = y + 10 + line as i32 * 18;
        let end = draw_dialog_text(renderer, font, bytes, text_x, text_y, dialog.font_color);
        last_end = Some((end, text_y));
    }
    if dialog.awaiting_input {
        if let Some((end, text_y)) = last_end {
            draw_dialog_wait_icon(renderer, end + 2, text_y + 5);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let dialog = ActiveDialog {
            message_ids: vec![0, 1, 2, 3, 4],
            position: DialogPosition::Upper,
            font_color: 0x4f,
            face_index: None,
            page: 0,
            awaiting_input: true,
            auto_wait_ticks: None,
        };
        assert_eq!(dialog_title(&text, &dialog), Some(b"Name:".as_slice()));
        assert_eq!(dialog_body_lines(&text, &dialog).len(), 4);
        assert_eq!(dialog_page_count(&text, &dialog), 1);
    }

    #[test]
    fn dialog_controls_do_not_consume_layout_width() {
        assert_eq!(dialog_text_width(b"A-$03B"), 16);
        let lines = wrap_big5_lines(b"A-$03B", 8);
        assert_eq!(lines, [b"A-$03".as_slice(), b"B".as_slice()]);
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
