//! Standalone, read-only scene and script inspector.

use pal_assets::script::ScriptTable;
use pal_core::map::{MAP_PIXEL_HEIGHT, MAP_PIXEL_WIDTH};
use pal_core::scene::SceneObject;
use pal_core::script::{inspect_script_records, ScriptControlFlow, ScriptRecordInspection};
use pixels::{Pixels, SurfaceTexture};
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Event, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::WindowBuilder;

use super::debug_render::render_object_overlay;
use super::draw::{draw_debug_text, draw_line, fill_rect, stroke_rect};
use super::scene_render::render_tile_map;
use super::{LoadedScene, SceneEditorResources, Viewport};
use crate::renderer::Renderer;

pub const SCENE_EDITOR_WIDTH: u32 = 800;
pub const SCENE_EDITOR_HEIGHT: u32 = 450;

const CANVAS_WIDTH: u32 = 480;
const PANEL_X: i32 = CANVAS_WIDTH as i32;
const PANEL_WIDTH: i32 = SCENE_EDITOR_WIDTH as i32 - PANEL_X;
const LINE_HEIGHT: i32 = 10;
const OBJECT_FIRST_LINE: usize = 9;
const OBJECT_ROWS: usize = 10;
const OBJECT_LAST_LINE: usize = OBJECT_FIRST_LINE + OBJECT_ROWS;
const SCRIPT_FIRST_LINE: usize = 29;
const SCRIPT_ROWS: usize = 15;
const MAX_ZOOM: u8 = 3;

const PANEL_BACKGROUND: [u8; 4] = [12, 16, 22, 255];
const PANEL_BORDER: [u8; 4] = [76, 92, 104, 255];
const TEXT: [u8; 4] = [224, 232, 228, 255];
const MUTED: [u8; 4] = [132, 148, 156, 255];
const ACCENT: [u8; 4] = [64, 224, 144, 255];
const TRIGGER: [u8; 4] = [255, 168, 48, 255];
const AUTO: [u8; 4] = [48, 208, 255, 255];
const SELECTED: [u8; 4] = [42, 60, 68, 255];
const ERROR: [u8; 4] = [255, 96, 96, 255];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectedScript {
    SceneEnter,
    SceneTeleport,
    ObjectTrigger,
    ObjectAuto,
}

#[derive(Debug, Clone, Copy)]
struct DragState {
    start_cursor: (i32, i32),
    start_viewport: (i32, i32),
    moved: bool,
}

struct SceneEditorApp<L> {
    renderer: Renderer,
    scene: LoadedScene,
    role_sprites: pal_core::role::RoleSprites,
    scripts: ScriptTable,
    scene_count: u16,
    load_scene: L,
    viewport: Viewport,
    zoom: u8,
    selected_object: Option<u16>,
    selected_script: Option<SelectedScript>,
    object_scroll: usize,
    script_scroll: usize,
    cursor: (i32, i32),
    drag: Option<DragState>,
    scene_input: Option<String>,
    status: Option<String>,
    dirty: bool,
}

impl<L> SceneEditorApp<L>
where
    L: FnMut(u16, &pal_core::role::RoleSprites) -> Option<LoadedScene>,
{
    fn new(
        renderer: Renderer,
        scene: LoadedScene,
        resources: SceneEditorResources,
        load_scene: L,
    ) -> Self {
        let mut app = Self {
            renderer,
            scene,
            role_sprites: resources.role_sprites,
            scripts: resources.script_table,
            scene_count: resources.scene_count,
            load_scene,
            viewport: Viewport::new(0, 0, CANVAS_WIDTH, SCENE_EDITOR_HEIGHT),
            zoom: 1,
            selected_object: None,
            selected_script: None,
            object_scroll: 0,
            script_scroll: 0,
            cursor: (0, 0),
            drag: None,
            scene_input: None,
            status: None,
            dirty: true,
        };
        app.select_initial_entry();
        app
    }

    fn title(&self) -> String {
        format!(
            "Rust-PAL Scene Inspector - scene {}/{}",
            self.scene.number, self.scene_count
        )
    }

    fn screen(&self) -> &[u8] {
        self.renderer.screen()
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn render(&mut self) {
        self.viewport.width = canvas_view_size(CANVAS_WIDTH, self.zoom);
        self.viewport.height = canvas_view_size(SCENE_EDITOR_HEIGHT, self.zoom);
        render_tile_map(
            &mut self.renderer,
            &self.scene.map,
            Some(&self.role_sprites),
            &[],
            &self.scene.objects,
            self.viewport,
        );
        render_object_overlay(
            &mut self.renderer,
            &self.scene.objects,
            self.viewport,
            self.selected_object,
        );
        scale_canvas(&mut self.renderer, self.zoom);
        draw_line(
            &mut self.renderer,
            PANEL_X - 1,
            0,
            PANEL_X - 1,
            SCENE_EDITOR_HEIGHT as i32 - 1,
            PANEL_BORDER,
        );
        self.draw_panel();
        self.dirty = false;
    }

    fn draw_panel(&mut self) {
        fill_rect(
            &mut self.renderer,
            PANEL_X,
            0,
            PANEL_WIDTH,
            SCENE_EDITOR_HEIGHT as i32,
            PANEL_BACKGROUND,
        );
        stroke_rect(
            &mut self.renderer,
            PANEL_X,
            0,
            PANEL_WIDTH,
            SCENE_EDITOR_HEIGHT as i32,
            PANEL_BORDER,
        );

        self.panel_line(
            0,
            &format!(
                "SCENE {}/{}  OBJECTS {}",
                self.scene.number,
                self.scene_count,
                self.scene.objects.len()
            ),
            ACCENT,
        );
        self.panel_line(
            1,
            &format!("PREV   NEXT   DOWN   UP   ZOOM {}X", self.zoom),
            TEXT,
        );
        self.panel_line(2, "DRAG PAN  WHEEL ZOOM  G GO TO SCENE", MUTED);
        let status = self.scene_input.as_ref().map_or_else(
            || {
                self.status
                    .as_deref()
                    .unwrap_or("READ ONLY  ORIGINAL RESOURCE STATE")
                    .to_owned()
            },
            |input| format!("GO TO SCENE {}  ENTER LOAD  ESC CANCEL", input),
        );
        self.panel_line(3, &status, MUTED);
        self.separator(4);
        self.selectable_line(
            5,
            self.selected_script == Some(SelectedScript::SceneEnter),
            &format!("SCENE ENTER     @{:04X}", self.scene.enter_script),
            TRIGGER,
        );
        self.selectable_line(
            6,
            self.selected_script == Some(SelectedScript::SceneTeleport),
            &format!("SCENE TELEPORT  @{:04X}", self.scene.teleport_script),
            TRIGGER,
        );
        self.separator(7);

        let script_ids = script_object_ids(&self.scene.objects);
        let max_scroll = script_ids.len().saturating_sub(OBJECT_ROWS);
        self.object_scroll = self.object_scroll.min(max_scroll);
        self.panel_line(
            8,
            &format!(
                "SCRIPT OBJECTS {}  OFFSET {}",
                script_ids.len(),
                self.object_scroll
            ),
            ACCENT,
        );
        for row in 0..OBJECT_ROWS {
            let line = OBJECT_FIRST_LINE + row;
            let Some(&id) = script_ids.get(self.object_scroll + row) else {
                self.panel_line(line, "-", MUTED);
                continue;
            };
            let object = self
                .scene
                .objects
                .iter()
                .find(|object| object.id == id)
                .expect("script object id came from the current scene");
            self.selectable_line(
                line,
                self.selected_object == Some(id),
                &format!(
                    "#{:<4} T@{:04X} A@{:04X} S{}",
                    id, object.trigger_script, object.auto_script, object.state
                ),
                object_list_color(object),
            );
        }
        self.separator(19);

        if let Some(object) = self.selected_object().cloned() {
            self.panel_line(
                20,
                &format!(
                    "OBJECT #{}  POS {},{}",
                    object.id, object.world_x, object.world_y
                ),
                ACCENT,
            );
            self.panel_line(
                21,
                &format!(
                    "STATE {} LAYER {} MODE {} VANISH {}",
                    object.state, object.layer, object.trigger_mode, object.vanish_time
                ),
                TEXT,
            );
            self.panel_line(
                22,
                &format!(
                    "SPRITE {} FRAME {} DIR {}",
                    object
                        .sprite_index
                        .map_or("-".to_owned(), |value| value.to_string()),
                    object.current_frame,
                    object.direction as u16
                ),
                TEXT,
            );
            self.panel_line(
                23,
                &format!(
                    "VISIBLE {} BLOCK {} SEARCH {} TOUCH {}",
                    yes_no(object.is_visible()),
                    yes_no(object.is_blocker()),
                    yes_no(object.can_search()),
                    yes_no(object.can_touch())
                ),
                TEXT,
            );
            self.panel_line(
                24,
                &format!(
                    "TRIGGER @{:04X}  AUTO @{:04X}",
                    object.trigger_script, object.auto_script
                ),
                TEXT,
            );
            self.panel_line(25, "CLICK TAB BELOW TO INSPECT", MUTED);
            self.selectable_half_line(
                26,
                self.selected_script == Some(SelectedScript::ObjectTrigger),
                self.selected_script == Some(SelectedScript::ObjectAuto),
                "TRIGGER",
                "AUTO",
            );
        } else {
            let source = match self.selected_script {
                Some(SelectedScript::SceneEnter) => "SCENE ENTER SCRIPT",
                Some(SelectedScript::SceneTeleport) => "SCENE TELEPORT SCRIPT",
                _ => "NO OBJECT SELECTED",
            };
            self.panel_line(20, source, ACCENT);
            self.panel_line(21, "CLICK A MARKER OR OBJECT ROW", MUTED);
            self.panel_line(22, "HIDDEN OBJECTS KEEP THEIR MARKERS", MUTED);
            self.panel_line(23, "SCRIPTS ARE NEVER EXECUTED HERE", MUTED);
            self.panel_line(24, "", TEXT);
            self.panel_line(25, "", TEXT);
            self.panel_line(26, "", TEXT);
        }
        self.separator(27);

        let entry = self.selected_script_entry();
        if entry == 0 {
            self.script_scroll = 0;
            self.panel_line(28, "SCRIPT DISABLED  ENTRY @0000", MUTED);
            for line in SCRIPT_FIRST_LINE..SCRIPT_FIRST_LINE + SCRIPT_ROWS {
                self.panel_line(line, "-", MUTED);
            }
        } else {
            let max_offset = self
                .scripts
                .len()
                .saturating_sub(usize::from(entry).saturating_add(1));
            self.script_scroll = self.script_scroll.min(max_offset);
            self.panel_line(
                28,
                &format!("SCRIPT @{:04X}  OFFSET {}", entry, self.script_scroll),
                ACCENT,
            );
            let records =
                inspect_script_records(&self.scripts, entry, self.script_scroll, SCRIPT_ROWS);
            for row in 0..SCRIPT_ROWS {
                let line = SCRIPT_FIRST_LINE + row;
                let Some(record) = records.get(row) else {
                    self.panel_line(line, "END OF SCRIPT TABLE", MUTED);
                    continue;
                };
                let color = if record.opcode.is_none() { ERROR } else { TEXT };
                self.panel_line(line, &format_script_record(*record), color);
            }
        }
        self.panel_line(44, "ESC CLOSE  WASD PAN  TAB NEXT OBJECT", MUTED);
    }

    fn panel_line(&mut self, line: usize, text: &str, color: [u8; 4]) {
        draw_debug_text(
            &mut self.renderer,
            PANEL_X + 8,
            line as i32 * LINE_HEIGHT + 1,
            text,
            color,
        );
    }

    fn selectable_line(&mut self, line: usize, selected: bool, text: &str, color: [u8; 4]) {
        if selected {
            fill_rect(
                &mut self.renderer,
                PANEL_X + 2,
                line as i32 * LINE_HEIGHT,
                PANEL_WIDTH - 4,
                LINE_HEIGHT,
                SELECTED,
            );
        }
        self.panel_line(line, text, color);
    }

    fn selectable_half_line(
        &mut self,
        line: usize,
        left_selected: bool,
        right_selected: bool,
        left: &str,
        right: &str,
    ) {
        let half = PANEL_WIDTH / 2;
        if left_selected {
            fill_rect(
                &mut self.renderer,
                PANEL_X + 2,
                line as i32 * LINE_HEIGHT,
                half - 2,
                LINE_HEIGHT,
                SELECTED,
            );
        }
        if right_selected {
            fill_rect(
                &mut self.renderer,
                PANEL_X + half,
                line as i32 * LINE_HEIGHT,
                half - 2,
                LINE_HEIGHT,
                SELECTED,
            );
        }
        self.panel_line(line, left, TRIGGER);
        draw_debug_text(
            &mut self.renderer,
            PANEL_X + half + 8,
            line as i32 * LINE_HEIGHT + 1,
            right,
            AUTO,
        );
    }

    fn separator(&mut self, line: usize) {
        let y = line as i32 * LINE_HEIGHT + LINE_HEIGHT / 2;
        draw_line(
            &mut self.renderer,
            PANEL_X + 4,
            y,
            SCENE_EDITOR_WIDTH as i32 - 5,
            y,
            PANEL_BORDER,
        );
    }

    fn selected_object(&self) -> Option<&SceneObject> {
        let id = self.selected_object?;
        self.scene.objects.iter().find(|object| object.id == id)
    }

    fn selected_script_entry(&self) -> u16 {
        match self.selected_script {
            Some(SelectedScript::SceneEnter) => self.scene.enter_script,
            Some(SelectedScript::SceneTeleport) => self.scene.teleport_script,
            Some(SelectedScript::ObjectTrigger) => self
                .selected_object()
                .map_or(0, |object| object.trigger_script),
            Some(SelectedScript::ObjectAuto) => self
                .selected_object()
                .map_or(0, |object| object.auto_script),
            None => 0,
        }
    }

    fn select_initial_entry(&mut self) {
        self.selected_object = None;
        self.selected_script = if self.scene.enter_script != 0 {
            Some(SelectedScript::SceneEnter)
        } else if self.scene.teleport_script != 0 {
            Some(SelectedScript::SceneTeleport)
        } else {
            None
        };
        self.object_scroll = 0;
        self.script_scroll = 0;
        if let Some(id) = script_object_ids(&self.scene.objects).first().copied() {
            self.center_on_object(id);
        } else if let Some(id) = self.scene.objects.first().map(|object| object.id) {
            self.center_on_object(id);
        } else {
            self.viewport.x = 0;
            self.viewport.y = 0;
        }
        self.clamp_viewport();
        self.dirty = true;
    }

    fn select_object(&mut self, id: u16, center: bool) {
        let Some(object) = self.scene.objects.iter().find(|object| object.id == id) else {
            return;
        };
        self.selected_object = Some(id);
        self.selected_script = if object.trigger_script != 0 {
            Some(SelectedScript::ObjectTrigger)
        } else if object.auto_script != 0 {
            Some(SelectedScript::ObjectAuto)
        } else {
            None
        };
        self.script_scroll = 0;
        self.reveal_selected_in_object_list();
        if center {
            self.center_on_object(id);
        }
        self.dirty = true;
    }

    fn reveal_selected_in_object_list(&mut self) {
        let Some(id) = self.selected_object else {
            return;
        };
        let ids = script_object_ids(&self.scene.objects);
        let Some(index) = ids.iter().position(|candidate| *candidate == id) else {
            return;
        };
        if index < self.object_scroll {
            self.object_scroll = index;
        } else if index >= self.object_scroll + OBJECT_ROWS {
            self.object_scroll = index + 1 - OBJECT_ROWS;
        }
    }

    fn center_on_object(&mut self, id: u16) {
        let Some(object) = self.scene.objects.iter().find(|object| object.id == id) else {
            return;
        };
        self.viewport.width = canvas_view_size(CANVAS_WIDTH, self.zoom);
        self.viewport.height = canvas_view_size(SCENE_EDITOR_HEIGHT, self.zoom);
        self.viewport.x = object.world_x - self.viewport.width as i32 / 2;
        self.viewport.y = object.world_y - self.viewport.height as i32 / 2;
        self.clamp_viewport();
    }

    fn switch_scene(&mut self, number: u16) {
        if number == 0 || number > self.scene_count || number == self.scene.number {
            return;
        }
        match (self.load_scene)(number, &self.role_sprites) {
            Some(scene) => {
                self.scene = scene;
                self.status = None;
                self.select_initial_entry();
            }
            None => {
                self.status = Some(format!("FAILED TO LOAD SCENE {number}"));
                self.dirty = true;
            }
        }
    }

    fn switch_scene_by(&mut self, delta: i32) {
        let next =
            (i32::from(self.scene.number) + delta).clamp(1, i32::from(self.scene_count)) as u16;
        self.switch_scene(next);
    }

    fn pan_by(&mut self, dx: i32, dy: i32) {
        self.viewport.x += dx;
        self.viewport.y += dy;
        self.clamp_viewport();
        self.dirty = true;
    }

    fn clamp_viewport(&mut self) {
        let (x, y) = clamped_viewport(
            self.viewport.x,
            self.viewport.y,
            self.viewport.width,
            self.viewport.height,
        );
        self.viewport.x = x;
        self.viewport.y = y;
    }

    fn set_zoom(&mut self, zoom: u8, anchor: (i32, i32)) {
        let zoom = zoom.clamp(1, MAX_ZOOM);
        if zoom == self.zoom {
            return;
        }
        let old_zoom = i32::from(self.zoom);
        let new_zoom = i32::from(zoom);
        let world_x = self.viewport.x + anchor.0 / old_zoom;
        let world_y = self.viewport.y + anchor.1 / old_zoom;
        self.zoom = zoom;
        self.viewport.width = canvas_view_size(CANVAS_WIDTH, zoom);
        self.viewport.height = canvas_view_size(SCENE_EDITOR_HEIGHT, zoom);
        self.viewport.x = world_x - anchor.0 / new_zoom;
        self.viewport.y = world_y - anchor.1 / new_zoom;
        self.clamp_viewport();
        self.dirty = true;
    }

    fn handle_key(&mut self, code: KeyCode) -> bool {
        if self.scene_input.is_some() {
            match code {
                KeyCode::Escape => {
                    self.scene_input = None;
                    self.dirty = true;
                }
                KeyCode::Backspace => {
                    if let Some(input) = self.scene_input.as_mut() {
                        input.pop();
                    }
                    self.dirty = true;
                }
                KeyCode::Enter | KeyCode::NumpadEnter => {
                    let number = self
                        .scene_input
                        .as_deref()
                        .and_then(|input| input.parse::<u16>().ok());
                    self.scene_input = None;
                    match number.filter(|number| (1..=self.scene_count).contains(number)) {
                        Some(number) => self.switch_scene(number),
                        None => {
                            self.status =
                                Some(format!("SCENE MUST BE BETWEEN 1 AND {}", self.scene_count));
                            self.dirty = true;
                        }
                    }
                }
                _ => {
                    if let Some(digit) = key_digit(code) {
                        if let Some(input) = self.scene_input.as_mut() {
                            if input.len() < 5 {
                                input.push(digit);
                            }
                        }
                        self.dirty = true;
                    }
                }
            }
            return false;
        }
        match code {
            KeyCode::Escape => return true,
            KeyCode::KeyG => {
                self.scene_input = Some(String::new());
                self.status = None;
                self.dirty = true;
            }
            KeyCode::BracketLeft | KeyCode::PageUp => self.switch_scene_by(-1),
            KeyCode::BracketRight | KeyCode::PageDown => self.switch_scene_by(1),
            KeyCode::ArrowLeft | KeyCode::KeyA => self.pan_by(-32, 0),
            KeyCode::ArrowRight | KeyCode::KeyD => self.pan_by(32, 0),
            KeyCode::ArrowUp | KeyCode::KeyW => self.pan_by(0, -16),
            KeyCode::ArrowDown | KeyCode::KeyS => self.pan_by(0, 16),
            KeyCode::Equal | KeyCode::NumpadAdd => {
                self.set_zoom(self.zoom.saturating_add(1), (CANVAS_WIDTH as i32 / 2, 225))
            }
            KeyCode::Minus | KeyCode::NumpadSubtract => {
                self.set_zoom(self.zoom.saturating_sub(1), (CANVAS_WIDTH as i32 / 2, 225))
            }
            KeyCode::Home => {
                if let Some(id) = self.selected_object {
                    self.center_on_object(id);
                } else {
                    self.select_initial_entry();
                }
                self.dirty = true;
            }
            KeyCode::Tab => self.select_next_script_object(),
            _ => {}
        }
        false
    }

    fn select_next_script_object(&mut self) {
        let ids = script_object_ids(&self.scene.objects);
        if ids.is_empty() {
            return;
        }
        let next = self
            .selected_object
            .and_then(|current| ids.iter().position(|id| *id == current))
            .map_or(0, |index| (index + 1) % ids.len());
        self.select_object(ids[next], true);
    }

    fn cursor_moved(&mut self, cursor: (i32, i32)) {
        self.cursor = cursor;
        let Some(mut drag) = self.drag else {
            return;
        };
        let dx = cursor.0 - drag.start_cursor.0;
        let dy = cursor.1 - drag.start_cursor.1;
        drag.moved |= dx.abs() >= 3 || dy.abs() >= 3;
        self.viewport.x = drag.start_viewport.0 - dx / i32::from(self.zoom);
        self.viewport.y = drag.start_viewport.1 - dy / i32::from(self.zoom);
        self.clamp_viewport();
        self.drag = Some(drag);
        self.dirty = true;
    }

    fn pointer_pressed(&mut self) {
        if self.cursor.0 < CANVAS_WIDTH as i32 {
            self.drag = Some(DragState {
                start_cursor: self.cursor,
                start_viewport: (self.viewport.x, self.viewport.y),
                moved: false,
            });
        } else {
            self.handle_panel_click(self.cursor);
        }
    }

    fn pointer_released(&mut self) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        if drag.moved || self.cursor.0 >= CANVAS_WIDTH as i32 {
            return;
        }
        let hit = hit_test_object(
            &self.scene.objects,
            self.viewport,
            self.zoom,
            self.cursor,
            self.selected_object,
        )
        .or_else(|| self.hit_test_visible_sprite());
        if let Some(id) = hit {
            self.select_object(id, false);
        }
    }

    fn hit_test_visible_sprite(&self) -> Option<u16> {
        let zoom = i32::from(self.zoom.max(1));
        let mut candidates = self
            .scene
            .objects
            .iter()
            .filter(|object| object.is_visible())
            .filter_map(|object| {
                let bitmap = self
                    .role_sprites
                    .decode_frame(object.sprite_index?, object.frame_index()?)?;
                let left = (object.world_x - i32::from(bitmap.width) / 2 - self.viewport.x) * zoom;
                let top = (object.world_y + 7 - i32::from(bitmap.height) - self.viewport.y) * zoom;
                let right = left + i32::from(bitmap.width) * zoom;
                let bottom = top + i32::from(bitmap.height) * zoom;
                (self.cursor.0 >= left
                    && self.cursor.0 < right
                    && self.cursor.1 >= top
                    && self.cursor.1 < bottom)
                    .then_some((object.world_y + i32::from(object.layer) * 8, object.id))
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
        cycle_candidate_ids(&candidates, self.selected_object)
    }

    fn handle_panel_click(&mut self, cursor: (i32, i32)) {
        let local_x = cursor.0 - PANEL_X;
        let line = usize::try_from(cursor.1 / LINE_HEIGHT).unwrap_or(usize::MAX);
        match line {
            1 if local_x < 42 => self.switch_scene_by(-1),
            1 if local_x < 92 => self.switch_scene_by(1),
            1 if local_x < 145 => self.set_zoom(
                self.zoom.saturating_sub(1),
                (CANVAS_WIDTH as i32 / 2, SCENE_EDITOR_HEIGHT as i32 / 2),
            ),
            1 if local_x < 180 => self.set_zoom(
                self.zoom.saturating_add(1),
                (CANVAS_WIDTH as i32 / 2, SCENE_EDITOR_HEIGHT as i32 / 2),
            ),
            1 => {}
            5 => {
                self.selected_object = None;
                self.selected_script = Some(SelectedScript::SceneEnter);
                self.script_scroll = 0;
                self.dirty = true;
            }
            6 => {
                self.selected_object = None;
                self.selected_script = Some(SelectedScript::SceneTeleport);
                self.script_scroll = 0;
                self.dirty = true;
            }
            OBJECT_FIRST_LINE..OBJECT_LAST_LINE => {
                let ids = script_object_ids(&self.scene.objects);
                let index = self.object_scroll + line - OBJECT_FIRST_LINE;
                if let Some(&id) = ids.get(index) {
                    self.select_object(id, true);
                }
            }
            26 if self.selected_object.is_some() => {
                self.selected_script = Some(if local_x < PANEL_WIDTH / 2 {
                    SelectedScript::ObjectTrigger
                } else {
                    SelectedScript::ObjectAuto
                });
                self.script_scroll = 0;
                self.dirty = true;
            }
            _ => {}
        }
    }

    fn mouse_wheel(&mut self, amount: i32) {
        if amount == 0 {
            return;
        }
        if self.cursor.0 < CANVAS_WIDTH as i32 {
            let next = if amount > 0 {
                self.zoom.saturating_add(1)
            } else {
                self.zoom.saturating_sub(1)
            };
            self.set_zoom(next, self.cursor);
            return;
        }
        let line = usize::try_from(self.cursor.1 / LINE_HEIGHT).unwrap_or(usize::MAX);
        if (OBJECT_FIRST_LINE..OBJECT_LAST_LINE).contains(&line) {
            let count = script_object_ids(&self.scene.objects).len();
            let max = count.saturating_sub(OBJECT_ROWS);
            self.object_scroll = scroll_offset(self.object_scroll, amount, max);
        } else if line >= SCRIPT_FIRST_LINE {
            let entry = self.selected_script_entry();
            let max = self
                .scripts
                .len()
                .saturating_sub(usize::from(entry).saturating_add(1));
            self.script_scroll = scroll_offset(self.script_scroll, amount, max);
        }
        self.dirty = true;
    }
}

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
                WindowEvent::Focused(false) => app.drag = None,
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

fn canvas_view_size(canvas: u32, zoom: u8) -> u32 {
    canvas.div_ceil(u32::from(zoom.max(1)))
}

fn clamped_viewport(x: i32, y: i32, width: u32, height: u32) -> (i32, i32) {
    let max_x = (MAP_PIXEL_WIDTH - width as i32).max(0);
    let max_y = (MAP_PIXEL_HEIGHT - height as i32).max(0);
    (x.clamp(0, max_x), y.clamp(0, max_y))
}

fn scale_canvas(renderer: &mut Renderer, zoom: u8) {
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

fn script_object_ids(objects: &[SceneObject]) -> Vec<u16> {
    objects
        .iter()
        .filter(|object| object.trigger_script != 0 || object.auto_script != 0)
        .map(|object| object.id)
        .collect()
}

fn object_list_color(object: &SceneObject) -> [u8; 4] {
    match (object.trigger_script != 0, object.auto_script != 0) {
        (true, true) => [255, 104, 224, 255],
        (true, false) => TRIGGER,
        (false, true) => AUTO,
        (false, false) => MUTED,
    }
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "Y"
    } else {
        "N"
    }
}

fn scroll_offset(current: usize, amount: i32, maximum: usize) -> usize {
    if amount > 0 {
        current.saturating_sub(1)
    } else {
        current.saturating_add(1).min(maximum)
    }
}

fn key_digit(code: KeyCode) -> Option<char> {
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

fn cycle_candidate_ids(candidates: &[(i32, u16)], current: Option<u16>) -> Option<u16> {
    if candidates.is_empty() {
        return None;
    }
    current
        .and_then(|id| candidates.iter().position(|candidate| candidate.1 == id))
        .map_or_else(
            || Some(candidates[0].1),
            |index| Some(candidates[(index + 1) % candidates.len()].1),
        )
}

fn hit_test_object(
    objects: &[SceneObject],
    viewport: Viewport,
    zoom: u8,
    cursor: (i32, i32),
    current: Option<u16>,
) -> Option<u16> {
    let zoom = i32::from(zoom.max(1));
    let mut candidates = objects
        .iter()
        .filter_map(|object| {
            let x = (object.world_x - viewport.x) * zoom;
            let y = (object.world_y - viewport.y) * zoom;
            let dx = x - cursor.0;
            let dy = y - cursor.1;
            let distance = i64::from(dx) * i64::from(dx) + i64::from(dy) * i64::from(dy);
            (distance <= 14 * 14).then_some((
                distance,
                object.world_y + i32::from(object.layer) * 8,
                object.id,
            ))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| right.1.cmp(&left.1))
            .then_with(|| left.2.cmp(&right.2))
    });
    if candidates.is_empty() {
        return None;
    }
    current
        .and_then(|id| candidates.iter().position(|candidate| candidate.2 == id))
        .map_or_else(
            || Some(candidates[0].2),
            |index| Some(candidates[(index + 1) % candidates.len()].2),
        )
}

fn format_script_record(record: ScriptRecordInspection) -> String {
    let mnemonic = record.opcode.map_or("UNKNOWN", |opcode| opcode.name());
    let mnemonic = truncate_ascii(mnemonic, 16);
    let flow = match record.flow {
        ScriptControlFlow::Next => String::new(),
        ScriptControlFlow::Stop => "STOP".to_owned(),
        ScriptControlFlow::Jump {
            target,
            conditional,
        } => format!(
            "{}@{:04X}{}",
            if conditional { "IF" } else { "TO" },
            target,
            if target <= record.entry { " LOOP" } else { "" }
        ),
        ScriptControlFlow::Call { target, .. } => format!("CALL@{target:04X}"),
        ScriptControlFlow::Random { choices } => format!("RANDOM {choices}"),
        ScriptControlFlow::Unknown => "INVALID".to_owned(),
    };
    format!(
        "@{:04X} {:<16} {:04X} {:04X} {:04X} {}",
        record.entry,
        mnemonic,
        record.instruction.operands[0],
        record.instruction.operands[1],
        record.instruction.operands[2],
        flow
    )
}

fn truncate_ascii(value: &str, maximum: usize) -> String {
    value.chars().take(maximum).collect()
}

#[cfg(test)]
mod tests {
    use pal_core::role::Direction;

    use super::*;

    fn object(id: u16, x: i32, y: i32, trigger: u16, auto: u16) -> SceneObject {
        SceneObject {
            id,
            world_x: x,
            world_y: y,
            layer: 0,
            trigger_script: trigger,
            auto_script: auto,
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
    fn script_object_list_excludes_inert_objects() {
        let objects = [
            object(1, 0, 0, 0, 0),
            object(2, 0, 0, 10, 0),
            object(3, 0, 0, 0, 20),
        ];
        assert_eq!(script_object_ids(&objects), vec![2, 3]);
    }

    #[test]
    fn hit_testing_includes_hidden_or_sprite_less_objects_and_cycles_overlaps() {
        let mut first = object(5, 100, 80, 10, 0);
        first.state = 0;
        let second = object(6, 100, 80, 0, 20);
        let viewport = Viewport::new(50, 40, 240, 225);
        let cursor = ((100 - 50) * 2, (80 - 40) * 2);

        assert_eq!(
            hit_test_object(&[first.clone(), second.clone()], viewport, 2, cursor, None),
            Some(5)
        );
        assert_eq!(
            hit_test_object(&[first, second], viewport, 2, cursor, Some(5)),
            Some(6)
        );
    }

    #[test]
    fn viewport_and_scroll_helpers_clamp_to_valid_ranges() {
        assert_eq!(clamped_viewport(-10, -20, 480, 450), (0, 0));
        assert_eq!(
            clamped_viewport(i32::MAX, i32::MAX, 480, 450),
            (MAP_PIXEL_WIDTH - 480, MAP_PIXEL_HEIGHT - 450)
        );
        assert_eq!(scroll_offset(0, 1, 10), 0);
        assert_eq!(scroll_offset(10, -1, 10), 10);
        assert_eq!(canvas_view_size(480, 3), 160);
        assert_eq!(key_digit(KeyCode::Digit7), Some('7'));
        assert_eq!(key_digit(KeyCode::KeyA), None);
        assert_eq!(cycle_candidate_ids(&[(10, 5), (9, 6)], Some(5)), Some(6));
    }

    #[test]
    fn script_rows_show_loop_and_invalid_annotations() {
        let loop_record = ScriptRecordInspection {
            entry: 10,
            instruction: pal_assets::script::ScriptEntry {
                opcode: pal_core::script::ScriptOpcode::Jump.raw(),
                operands: [5, 0, 0],
            },
            opcode: Some(pal_core::script::ScriptOpcode::Jump),
            flow: ScriptControlFlow::Jump {
                target: 5,
                conditional: false,
            },
        };
        assert!(format_script_record(loop_record).contains("LOOP"));

        let invalid = ScriptRecordInspection {
            opcode: None,
            flow: ScriptControlFlow::Unknown,
            ..loop_record
        };
        assert!(format_script_record(invalid).contains("INVALID"));
    }
}
