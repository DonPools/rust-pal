//! Native window and map framebuffer presentation.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::audio::{BackgroundMusic, SoundEffects};
use crate::renderer::Renderer;
use pal_assets::rle::RleBitmap;
use pal_assets::script::ScriptTable;
use pal_assets::text::{BitmapFont, TextLibrary};
use pal_core::game::{
    AutoScriptError, Camera, GameInput, GameState, StoreItem, UPDATE_INTERVAL_MS,
};
use pal_core::map::{Map, MAP_COLUMNS, MAP_ROWS};
use pal_core::role::{Direction, Role, RoleSprites};
use pal_core::scene::SceneObject;
use pal_core::script::{DialogPosition, ScriptCondition, ScriptEvent, ScriptRuntime};
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

pub struct LoadedScene {
    pub number: u16,
    pub map: Map,
    pub objects: Vec<SceneObject>,
    pub enter_script: u16,
    pub teleport_script: u16,
}

pub struct GameResources {
    pub role_sprites: RoleSprites,
    pub script_table: ScriptTable,
    pub initial_enter_script: u16,
    pub text: TextLibrary,
    pub font: BitmapFont,
    pub voc_mkf: Vec<u8>,
    pub midi_mkf: Vec<u8>,
    pub sound_font: Vec<u8>,
    pub snapshot_path: PathBuf,
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
    fn set_key(&mut self, key: KeyCode, pressed: bool, repeat: bool) {
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
            KeyCode::Enter | KeyCode::Space if pressed && !repeat => self.confirm = true,
            KeyCode::Escape | KeyCode::Backspace if pressed && !repeat => self.cancel = true,
            _ => {}
        }
    }

    fn sample(&mut self) -> GameInput {
        let input = GameInput {
            direction: self.active_direction,
            confirm: self.confirm,
            cancel: self.cancel,
        };
        self.confirm = false;
        self.cancel = false;
        input
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

#[derive(Debug, Clone, Copy)]
struct ActiveDialog {
    message_id: u16,
    position: DialogPosition,
    page: usize,
}

const INVENTORY_VISIBLE_ROWS: usize = 8;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct InventoryMenu {
    selected: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ConfirmationMenu {
    no_entry: u16,
    selected_yes: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShopMode {
    Buy { store_number: u16 },
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ShopMenu {
    mode: ShopMode,
    selected: usize,
    confirming: bool,
}

impl ShopMenu {
    fn items(self, game: &GameState) -> Vec<StoreItem> {
        match self.mode {
            ShopMode::Buy { store_number } => game.store_items(store_number).unwrap_or_default(),
            ShopMode::Sell => game.sellable_inventory(),
        }
    }

    fn update_selection(&mut self, direction: Option<Direction>, item_count: usize) {
        if item_count == 0 {
            self.selected = 0;
            return;
        }
        match direction {
            Some(Direction::North) => self.selected = self.selected.saturating_sub(1),
            Some(Direction::South) => self.selected = (self.selected + 1).min(item_count - 1),
            _ => {}
        }
    }
}

impl InventoryMenu {
    fn update(&mut self, direction: Option<Direction>, item_count: usize) {
        if item_count == 0 {
            self.selected = 0;
            return;
        }
        match direction {
            Some(Direction::North) => self.selected = self.selected.saturating_sub(1),
            Some(Direction::South) => self.selected = (self.selected + 1).min(item_count - 1),
            _ => {}
        }
    }

    fn first_visible(self, item_count: usize) -> usize {
        self.selected
            .min(item_count.saturating_sub(1))
            .saturating_sub(INVENTORY_VISIBLE_ROWS - 1)
    }
}

#[derive(Clone, Copy)]
struct UiRenderContext<'a> {
    dialog: Option<&'a ActiveDialog>,
    inventory_menu: Option<&'a InventoryMenu>,
    confirmation_menu: Option<&'a ConfirmationMenu>,
    shop_menu: Option<&'a ShopMenu>,
    text: &'a TextLibrary,
    font: &'a BitmapFont,
}

struct ScriptServices {
    pending_enter_script: Option<u16>,
    confirmation_menu: Option<ConfirmationMenu>,
    shop_menu: Option<ShopMenu>,
    auto_scripts: ScriptTable,
    sound_effects: SoundEffects,
    music: BackgroundMusic,
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
    let mut show_collision = false;
    let auto_scripts = script_table.clone();
    let mut scripts = ScriptRuntime::new(script_table);
    let mut dialog = None;
    let mut inventory_menu = None;
    let mut script_services = ScriptServices {
        pending_enter_script: None,
        confirmation_menu: None,
        shop_menu: None,
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
        UiRenderContext {
            dialog: dialog.as_ref(),
            inventory_menu: inventory_menu.as_ref(),
            confirmation_menu: script_services.confirmation_menu.as_ref(),
            shop_menu: script_services.shop_menu.as_ref(),
            text: &text,
            font: &font,
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
                                    window.set_title(if show_collision {
                                        "Rust-PAL [Collision Debug]"
                                    } else {
                                        "Rust-PAL"
                                    });
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
                                            inventory_menu = None;
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
                                UiRenderContext {
                                    dialog: dialog.as_ref(),
                                    inventory_menu: inventory_menu.as_ref(),
                                    confirmation_menu: script_services.confirmation_menu.as_ref(),
                                    shop_menu: script_services.shop_menu.as_ref(),
                                    text: &text,
                                    font: &font,
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
                    let sampled = input.sample();
                    if let Some(active_dialog) = dialog.as_mut() {
                        if sampled.confirm || sampled.cancel {
                            changed = true;
                            let page_count = dialog_page_count(&text, active_dialog.message_id);
                            if active_dialog.page + 1 < page_count {
                                active_dialog.page += 1;
                            } else {
                                dialog = None;
                                advance_script(
                                    &mut scripts,
                                    &mut game,
                                    &mut dialog,
                                    &role_sprites,
                                    &mut load_scene,
                                    &mut script_services,
                                    &mut |title| window.set_title(title),
                                );
                            }
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
                                &role_sprites,
                                &mut load_scene,
                                &mut script_services,
                                &mut |title| window.set_title(title),
                            );
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
                                } else if sampled.confirm {
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
                                    menu.confirming = false;
                                    let remaining = menu.items(&game).len();
                                    menu.selected = menu.selected.min(remaining.saturating_sub(1));
                                }
                            } else if sampled.cancel {
                                close_shop = true;
                            } else {
                                menu.update_selection(sampled.direction, items.len());
                                if sampled.confirm && items.get(menu.selected).is_some() {
                                    menu.confirming = true;
                                }
                            }
                        }
                        if close_shop {
                            script_services.shop_menu = None;
                            advance_script(
                                &mut scripts,
                                &mut game,
                                &mut dialog,
                                &role_sprites,
                                &mut load_scene,
                                &mut script_services,
                                &mut |title| window.set_title(title),
                            );
                        }
                    } else if let Some(menu) = inventory_menu.as_mut() {
                        changed = sampled.cancel || sampled.direction.is_some();
                        if sampled.cancel {
                            inventory_menu = None;
                            window.set_title("Rust-PAL");
                        } else {
                            menu.update(sampled.direction, game.inventory().len());
                        }
                    } else if scripts.is_active() {
                        advance_script(
                            &mut scripts,
                            &mut game,
                            &mut dialog,
                            &role_sprites,
                            &mut load_scene,
                            &mut script_services,
                            &mut |title| window.set_title(title),
                        );
                        changed = true;
                    } else {
                        if sampled.cancel {
                            inventory_menu = Some(InventoryMenu::default());
                            window.set_title("Rust-PAL [Inventory]");
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
                                        &role_sprites,
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
                        UiRenderContext {
                            dialog: dialog.as_ref(),
                            inventory_menu: inventory_menu.as_ref(),
                            confirmation_menu: script_services.confirmation_menu.as_ref(),
                            shop_menu: script_services.shop_menu.as_ref(),
                            text: &text,
                            font: &font,
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
        } => format!("Rust-PAL [object {object_id} auto script {entry} opcode {opcode:04x}]"),
        AutoScriptError::InstructionLimit { object_id, entry } => {
            format!("Rust-PAL [object {object_id} auto script loop at {entry}]")
        }
    }
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
    if let Some(dialog) = ui.dialog {
        render_dialog(renderer, ui.text, ui.font, dialog);
    } else if let Some(menu) = ui.confirmation_menu {
        render_confirmation_menu(renderer, ui.text, ui.font, *menu);
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
    role_sprites: &RoleSprites,
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
        }) => {
            *dialog = Some(ActiveDialog {
                message_id,
                position,
                page: 0,
            });
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
            });
            set_title("Rust-PAL [Buy]");
        }
        Some(ScriptEvent::OpenSellMenu) => {
            services.shop_menu = Some(ShopMenu {
                mode: ShopMode::Sell,
                selected: 0,
                confirming: false,
            });
            set_title("Rust-PAL [Sell]");
        }
        Some(ScriptEvent::Action(pal_core::script::ScriptAction::ChangeScene { scene_number })) => {
            if scene_number == game.scene_number {
                return;
            }
            let Some(scene) = load_scene(scene_number, role_sprites) else {
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
        }) => {
            if trigger.object_id == 0xffff {
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
        }
        Some(ScriptEvent::Unsupported {
            trigger: _,
            entry,
            opcode,
        }) => {
            set_title(&format!(
                "Rust-PAL [unsupported script {entry} opcode {opcode:04x}]"
            ));
        }
        Some(ScriptEvent::InvalidEntry { trigger: _, entry }) => {
            set_title(&format!("Rust-PAL [invalid script entry {entry}]"));
        }
        Some(ScriptEvent::InstructionLimit { trigger: _, entry }) => {
            set_title(&format!("Rust-PAL [script loop at {entry}]"));
        }
        None => {}
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
    dialog: &ActiveDialog,
) {
    let y = match dialog.position {
        DialogPosition::Upper => 8,
        DialogPosition::Lower => 130,
        DialogPosition::Center | DialogPosition::CenterWindow => 68,
    };
    fill_rect(renderer, 8, y, 304, 62, [8, 8, 12, 255]);
    fill_rect(renderer, 8, y, 304, 1, [224, 224, 208, 255]);
    fill_rect(renderer, 8, y + 61, 304, 1, [224, 224, 208, 255]);
    fill_rect(renderer, 8, y, 1, 62, [224, 224, 208, 255]);
    fill_rect(renderer, 311, y, 1, 62, [224, 224, 208, 255]);

    let Some(message) = text.message(usize::from(dialog.message_id)) else {
        return;
    };
    for (line, bytes) in wrap_big5_lines(message, 272)
        .into_iter()
        .skip(dialog.page * 3)
        .take(3)
        .enumerate()
    {
        renderer.draw_big5_text(font, bytes, 20, y + 8 + line as i32 * 16, 0x4f);
    }
}

fn render_confirmation_menu(
    renderer: &mut Renderer,
    text: &TextLibrary,
    font: &BitmapFont,
    menu: ConfirmationMenu,
) {
    const NO_WORD: usize = 19;
    const YES_WORD: usize = 20;
    const X: i32 = 120;
    const Y: i32 = 92;
    fill_rect(renderer, X, Y, 80, 32, [8, 8, 12, 255]);
    stroke_rect(renderer, X, Y, 80, 32, [224, 224, 208, 255]);
    for (index, (word_id, selected)) in
        [(NO_WORD, !menu.selected_yes), (YES_WORD, menu.selected_yes)]
            .into_iter()
            .enumerate()
    {
        let Some(label) = text.word(word_id) else {
            continue;
        };
        let x = X + 10 + index as i32 * 38;
        if selected {
            fill_rect(renderer, x - 4, Y + 6, 34, 20, [48, 48, 56, 255]);
        }
        renderer.draw_big5_text(font, label, x, Y + 8, if selected { 0x2d } else { 0x4f });
    }
}

fn render_shop_menu(
    renderer: &mut Renderer,
    game: &GameState,
    text: &TextLibrary,
    font: &BitmapFont,
    menu: ShopMenu,
) {
    const PANEL_X: i32 = 112;
    const PANEL_Y: i32 = 8;
    const PANEL_WIDTH: i32 = 200;
    const ROW_HEIGHT: i32 = 18;
    const CASH_WORD: usize = 21;

    let items = menu.items(game);
    fill_rect(
        renderer,
        PANEL_X,
        PANEL_Y,
        PANEL_WIDTH,
        184,
        [8, 8, 12, 255],
    );
    stroke_rect(
        renderer,
        PANEL_X,
        PANEL_Y,
        PANEL_WIDTH,
        184,
        [224, 224, 208, 255],
    );
    for (row, item) in items.iter().take(9).enumerate() {
        let y = PANEL_Y + 10 + row as i32 * ROW_HEIGHT;
        if row == menu.selected {
            stroke_rect(
                renderer,
                PANEL_X + 5,
                y - 3,
                PANEL_WIDTH - 10,
                17,
                [224, 192, 64, 255],
            );
        }
        if let Some(name) = text.word(usize::from(item.item_id)) {
            renderer.draw_big5_text(font, name, PANEL_X + 12, y, 0x4f);
        }
        draw_number(
            renderer,
            u32::from(item.price),
            PANEL_X + PANEL_WIDTH - 12,
            y + 4,
            [240, 224, 96, 255],
        );
    }
    if let Some(cash_label) = text.word(CASH_WORD) {
        renderer.draw_big5_text(font, cash_label, PANEL_X + 12, PANEL_Y + 164, 0x4f);
    }
    draw_number(
        renderer,
        game.cash,
        PANEL_X + PANEL_WIDTH - 12,
        PANEL_Y + 168,
        [240, 224, 96, 255],
    );
    if menu.confirming {
        render_confirmation_menu(
            renderer,
            text,
            font,
            ConfirmationMenu {
                no_entry: 0,
                selected_yes: true,
            },
        );
    }
}

fn render_inventory_menu(
    renderer: &mut Renderer,
    game: &GameState,
    text: &TextLibrary,
    font: &BitmapFont,
    menu: InventoryMenu,
) {
    const PANEL_X: i32 = 132;
    const PANEL_Y: i32 = 12;
    const PANEL_WIDTH: i32 = 180;
    const ROW_HEIGHT: i32 = 20;

    let inventory = game.inventory().collect::<Vec<_>>();
    let first = menu.first_visible(inventory.len());
    fill_rect(
        renderer,
        PANEL_X,
        PANEL_Y,
        PANEL_WIDTH,
        176,
        [8, 8, 12, 255],
    );
    stroke_rect(
        renderer,
        PANEL_X,
        PANEL_Y,
        PANEL_WIDTH,
        176,
        [224, 224, 208, 255],
    );

    for (row, &(item_id, amount)) in inventory
        .iter()
        .skip(first)
        .take(INVENTORY_VISIBLE_ROWS)
        .enumerate()
    {
        let y = PANEL_Y + 8 + row as i32 * ROW_HEIGHT;
        if first + row == menu.selected {
            stroke_rect(
                renderer,
                PANEL_X + 5,
                y - 3,
                PANEL_WIDTH - 10,
                19,
                [224, 192, 64, 255],
            );
        }
        if let Some(name) = text.word(usize::from(item_id)) {
            renderer.draw_big5_text(font, name, PANEL_X + 12, y, 0x4f);
        }
        draw_number(
            renderer,
            u32::from(amount),
            PANEL_X + PANEL_WIDTH - 14,
            y + 4,
            [240, 240, 224, 255],
        );
    }
}

fn stroke_rect(renderer: &mut Renderer, x: i32, y: i32, width: i32, height: i32, color: [u8; 4]) {
    fill_rect(renderer, x, y, width, 1, color);
    fill_rect(renderer, x, y + height - 1, width, 1, color);
    fill_rect(renderer, x, y, 1, height, color);
    fill_rect(renderer, x + width - 1, y, 1, height, color);
}

fn draw_number(renderer: &mut Renderer, value: u32, right_x: i32, y: i32, color: [u8; 4]) {
    const DIGITS: [[u8; 5]; 10] = [
        [0b111, 0b101, 0b101, 0b101, 0b111],
        [0b010, 0b110, 0b010, 0b010, 0b111],
        [0b111, 0b001, 0b111, 0b100, 0b111],
        [0b111, 0b001, 0b111, 0b001, 0b111],
        [0b101, 0b101, 0b111, 0b001, 0b001],
        [0b111, 0b100, 0b111, 0b001, 0b111],
        [0b111, 0b100, 0b111, 0b101, 0b111],
        [0b111, 0b001, 0b010, 0b010, 0b010],
        [0b111, 0b101, 0b111, 0b101, 0b111],
        [0b111, 0b101, 0b111, 0b001, 0b111],
    ];

    let mut digits = [0u8; 10];
    let mut count = 0;
    let mut remaining = value;
    loop {
        digits[digits.len() - 1 - count] = (remaining % 10) as u8;
        count += 1;
        remaining /= 10;
        if remaining == 0 {
            break;
        }
    }
    let start_x = right_x - count as i32 * 5 + 1;
    for (position, &digit) in digits[digits.len() - count..].iter().enumerate() {
        for (row, bits) in DIGITS[usize::from(digit)].iter().enumerate() {
            for column in 0..3 {
                if bits & (0b100 >> column) != 0 {
                    renderer.put_rgba(
                        start_x + position as i32 * 5 + column,
                        y + row as i32,
                        color,
                    );
                }
            }
        }
    }
}

fn dialog_page_count(text: &TextLibrary, message_id: u16) -> usize {
    let line_count = text
        .message(usize::from(message_id))
        .map(|message| wrap_big5_lines(message, 272).len())
        .unwrap_or(0);
    line_count.max(1).div_ceil(3)
}

fn fill_rect(renderer: &mut Renderer, x: i32, y: i32, width: i32, height: i32, color: [u8; 4]) {
    for row in y..y + height {
        for column in x..x + width {
            renderer.put_rgba(column, row, color);
        }
    }
}

fn wrap_big5_lines(text: &[u8], max_width: usize) -> Vec<&[u8]> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut index = 0;
    let mut width = 0;
    while index < text.len() {
        let (bytes, character_width) = if text[index] >= 0x80 && index + 1 < text.len() {
            (2, 16)
        } else {
            (1, 8)
        };
        if width + character_width > max_width && index > start {
            lines.push(&text[start..index]);
            start = index;
            width = 0;
        }
        index += bytes;
        width += character_width;
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

/// Render the map and depth-sort characters with elevated covering tiles.
pub fn render_tile_map(
    renderer: &mut Renderer,
    map: &Map,
    role_sprites: Option<&RoleSprites>,
    roles: &[Role],
    scene_objects: &[SceneObject],
    viewport: Viewport,
) {
    renderer.clear_black();
    let bounds = RenderBounds::for_viewport(viewport);
    render_tile_layer(renderer, map, viewport, bounds, false);
    render_tile_layer(renderer, map, viewport, bounds, true);
    render_depth_sorted_sprites(
        renderer,
        map,
        role_sprites,
        roles,
        scene_objects,
        viewport,
        bounds,
    );
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

const BLOCKED_COLOR: [u8; 4] = [255, 48, 48, 255];
const WALKABLE_COLOR: [u8; 4] = [32, 224, 96, 255];
const PLAYER_COLLISION_COLOR: [u8; 4] = [255, 224, 32, 255];

fn render_collision_overlay(renderer: &mut Renderer, map: &Map, player: &Role, viewport: Viewport) {
    let bounds = RenderBounds::for_viewport(viewport);
    for y in bounds.start_y..bounds.end_y {
        for h in 0..2i32 {
            for x in bounds.start_x..bounds.end_x {
                let (Ok(x_index), Ok(y_index)) = (usize::try_from(x), usize::try_from(y)) else {
                    continue;
                };
                if x_index >= MAP_COLUMNS || y_index >= MAP_ROWS {
                    continue;
                }

                let center_x = x * 32 + h * 16 - viewport.x;
                let center_y = y * 16 + h * 8 - viewport.y;
                let color = if map.is_tile_blocked(x_index, y_index, h as usize) {
                    BLOCKED_COLOR
                } else {
                    WALKABLE_COLOR
                };
                draw_diamond(renderer, center_x, center_y, color);
            }
        }
    }

    let player_x = player.world_x - viewport.x;
    let player_y = player.world_y - viewport.y;
    draw_line(
        renderer,
        player_x - 3,
        player_y,
        player_x + 3,
        player_y,
        PLAYER_COLLISION_COLOR,
    );
    draw_line(
        renderer,
        player_x,
        player_y - 3,
        player_x,
        player_y + 3,
        PLAYER_COLLISION_COLOR,
    );
}

fn draw_diamond(renderer: &mut Renderer, center_x: i32, center_y: i32, color: [u8; 4]) {
    let left = (center_x - 16, center_y);
    let top = (center_x, center_y - 8);
    let right = (center_x + 16, center_y);
    let bottom = (center_x, center_y + 8);
    for (start, end) in [(left, top), (top, right), (right, bottom), (bottom, left)] {
        draw_line(renderer, start.0, start.1, end.0, end.1, color);
    }
}

fn draw_line(renderer: &mut Renderer, mut x0: i32, mut y0: i32, x1: i32, y1: i32, color: [u8; 4]) {
    let dx = (x1 - x0).abs();
    let step_x = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let step_y = if y0 < y1 { 1 } else { -1 };
    let mut error = dx + dy;

    loop {
        renderer.put_rgba(x0, y0, color);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let doubled_error = error * 2;
        if doubled_error >= dy {
            error += dy;
            x0 += step_x;
        }
        if doubled_error <= dx {
            error += dx;
            y0 += step_y;
        }
    }
}

struct DepthSprite {
    bitmap: RleBitmap,
    x: i32,
    y: i32,
    depth: i32,
}

fn render_depth_sorted_sprites(
    renderer: &mut Renderer,
    map: &Map,
    role_sprites: Option<&RoleSprites>,
    roles: &[Role],
    scene_objects: &[SceneObject],
    viewport: Viewport,
    bounds: RenderBounds,
) {
    let mut sprites = Vec::new();

    if let Some(role_sprites) = role_sprites {
        for role in roles {
            let (anchor_x, anchor_y) = role.screen_anchor();
            if anchor_y < bounds.start_y * 16 || anchor_y >= bounds.end_y * 16 {
                continue;
            }
            if let Some(bitmap) = role_sprites.decode_role_frame(role) {
                add_covering_tiles(
                    &mut sprites,
                    map,
                    viewport,
                    role.world_x - i32::from(bitmap.width) / 2 - 3,
                    role.world_y + 4,
                    bitmap.width,
                    bitmap.height,
                );
                sprites.push(DepthSprite {
                    x: anchor_x - bitmap.width as i32 / 2 - viewport.x,
                    y: anchor_y - bitmap.height as i32 - viewport.y,
                    // PAL sorts party sprites six logical pixels below the
                    // rendered foot anchor.
                    depth: anchor_y + 6,
                    bitmap,
                });
            }
        }

        for object in scene_objects.iter().filter(|object| object.is_visible()) {
            let (Some(sprite_index), Some(frame_index)) =
                (object.sprite_index, object.frame_index())
            else {
                continue;
            };
            let Some(bitmap) = role_sprites.decode_frame(sprite_index, frame_index) else {
                continue;
            };
            let layer = i32::from(object.layer) * 8 + 2;
            let draw_x = object.world_x - i32::from(bitmap.width) / 2;
            let draw_y = object.world_y + 7 - i32::from(bitmap.height);
            add_covering_tiles(
                &mut sprites,
                map,
                viewport,
                draw_x - layer / 2,
                object.world_y + 7,
                bitmap.width,
                bitmap.height,
            );
            sprites.push(DepthSprite {
                bitmap,
                x: draw_x - viewport.x,
                y: draw_y - viewport.y,
                depth: object.world_y + i32::from(object.layer) * 8 + 9,
            });
        }
    }

    sprites.sort_by_key(|sprite| sprite.depth);
    for sprite in sprites {
        renderer.blit_rle(&sprite.bitmap, sprite.x, sprite.y);
    }
}

fn add_covering_tiles(
    sprites: &mut Vec<DepthSprite>,
    map: &Map,
    viewport: Viewport,
    sx: i32,
    sy: i32,
    sprite_width: u16,
    sprite_height: u16,
) {
    let width = i32::from(sprite_width);
    let height = i32::from(sprite_height);
    let half = i32::from(sx % 32 != 0);

    for scan_y in (sy - height - 15) / 16..=sy / 16 {
        let first_x = (sx - width / 2) / 32;
        let last_x = (sx + width / 2) / 32;
        for scan_x in first_x..=last_x {
            let first_candidate = if scan_x == first_x { 0 } else { 3 };
            for candidate in first_candidate..5 {
                let (tile_x, tile_y, tile_half) =
                    cover_tile_candidate(scan_x, scan_y, half, candidate);
                let (Ok(x_index), Ok(y_index), Ok(half_index)) = (
                    usize::try_from(tile_x),
                    usize::try_from(tile_y),
                    usize::try_from(tile_half),
                ) else {
                    continue;
                };
                if x_index >= MAP_COLUMNS || y_index >= MAP_ROWS || half_index >= 2 {
                    continue;
                }

                for top in [false, true] {
                    let Some(tile_height) = map.tile_height(x_index, y_index, half_index, top)
                    else {
                        continue;
                    };
                    if tile_height == 0
                        || (tile_y + i32::from(tile_height)) * 16 + tile_half * 8 < sy
                    {
                        continue;
                    }
                    let bitmap = if top {
                        map.decode_top_tile(x_index, y_index, half_index)
                    } else {
                        map.decode_bottom_tile(x_index, y_index, half_index)
                    };
                    if let Some(bitmap) = bitmap {
                        let layer = i32::from(top);
                        sprites.push(DepthSprite {
                            x: tile_x * 32 + tile_half * 16 - 16 - viewport.x,
                            y: covering_tile_y(tile_y, tile_half, bitmap.height, viewport.y),
                            depth: tile_y * 16
                                + tile_half * 8
                                + 7
                                + layer
                                + i32::from(tile_height) * 8,
                            bitmap,
                        });
                    }
                }
            }
        }
    }
}

fn cover_tile_candidate(x: i32, y: i32, half: i32, candidate: i32) -> (i32, i32, i32) {
    match candidate {
        0 => (x, y, half),
        1 => (x - 1, y, half),
        2 if half != 0 => (x, y + 1, 0),
        2 => (x - 1, y, 1),
        3 => (x + 1, y, half),
        4 if half != 0 => (x + 1, y + 1, 0),
        4 => (x, y, 1),
        _ => unreachable!("cover tile candidate must be in 0..5"),
    }
}

fn covering_tile_y(tile_y: i32, half: i32, bitmap_height: u16, viewport_y: i32) -> i32 {
    tile_y * 16 + half * 8 + 7 - i32::from(bitmap_height) - viewport_y
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn long_dialog_text_spans_multiple_three_line_pages() {
        let text = [b'A'; 109];
        let lines = wrap_big5_lines(&text, 272);
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[..3].iter().map(|line| line.len()).sum::<usize>(), 102);
        assert_eq!(lines[3].len(), 7);
    }

    #[test]
    fn inventory_menu_clamps_selection_and_scrolls_visible_rows() {
        let mut menu = InventoryMenu::default();
        menu.update(Some(Direction::North), 10);
        assert_eq!(menu.selected, 0);
        menu.update(Some(Direction::South), 10);
        assert_eq!(menu.selected, 1);
        for _ in 0..20 {
            menu.update(Some(Direction::South), 10);
        }
        assert_eq!(menu.selected, 9);
        assert_eq!(menu.first_visible(10), 2);

        menu.update(None, 0);
        assert_eq!(menu.selected, 0);
        assert_eq!(menu.first_visible(0), 0);
    }
}
