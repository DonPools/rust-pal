//! Read-only scene-editor state and interaction logic.

use pal_assets::script::ScriptTable;
use pal_assets::text::{BitmapFont, TextLibrary};
use pal_core::scene::SceneObject;
use pal_core::script::{
    inspect_script_record, ScriptRecordInspection, ScriptReferenceCatalog, ScriptReferenceSource,
};
use winit::keyboard::KeyCode;

use super::super::debug_render::render_object_overlay;
use super::super::draw::draw_line;
use super::super::scene_render::render_tile_map;
use super::super::{LoadedScene, SceneEditorResources, Viewport};
use super::hit_test::{hit_test_object_marker, hit_test_visible_sprite};
use super::navigation::{navigable_target, scene_object_references, ScriptNavigation};
use super::{
    canvas_view_size, clamped_viewport, key_digit, scale_canvas, CANVAS_WIDTH, MAX_ZOOM,
    PANEL_BORDER, PANEL_X, SCENE_EDITOR_HEIGHT,
};
use crate::renderer::Renderer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SelectedScript {
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

pub(super) struct SceneEditorApp<L> {
    pub(super) renderer: Renderer,
    pub(super) scene: LoadedScene,
    pub(super) role_sprites: pal_core::role::RoleSprites,
    pub(super) scripts: ScriptTable,
    pub(super) text: TextLibrary,
    pub(super) font: BitmapFont,
    pub(super) script_references: ScriptReferenceCatalog,
    pub(super) scene_count: u16,
    load_scene: L,
    pub(super) viewport: Viewport,
    pub(super) zoom: u8,
    pub(super) selected_object: Option<u16>,
    pub(super) selected_script: Option<SelectedScript>,
    pub(super) object_scroll: usize,
    pub(super) code_scroll: usize,
    pub(super) reference_scroll: usize,
    pub(super) navigation: ScriptNavigation,
    cursor: (i32, i32),
    drag: Option<DragState>,
    scene_input: Option<String>,
    pub(super) status: Option<String>,
    dirty: bool,
}

impl<L> SceneEditorApp<L>
where
    L: FnMut(u16, &pal_core::role::RoleSprites) -> Option<LoadedScene>,
{
    pub(super) fn new(
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
            text: resources.text,
            font: resources.font,
            script_references: resources.script_references,
            scene_count: resources.scene_count,
            load_scene,
            viewport: Viewport::new(0, 0, CANVAS_WIDTH, SCENE_EDITOR_HEIGHT),
            zoom: 1,
            selected_object: None,
            selected_script: None,
            object_scroll: 0,
            code_scroll: 0,
            reference_scroll: 0,
            navigation: ScriptNavigation::default(),
            cursor: (0, 0),
            drag: None,
            scene_input: None,
            status: None,
            dirty: true,
        };
        app.select_initial_entry();
        app
    }

    pub(super) fn title(&self) -> String {
        format!(
            "Rust-PAL Scene Inspector - scene {}/{}",
            self.scene.number, self.scene_count
        )
    }

    pub(super) fn screen(&self) -> &[u8] {
        self.renderer.screen()
    }

    pub(super) fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub(super) fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub(super) fn reset_pointer(&mut self) {
        self.drag = None;
    }

    pub(super) fn render(&mut self) {
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
        self.draw_message_preview();
        self.dirty = false;
    }

    pub(super) fn selected_object(&self) -> Option<&SceneObject> {
        let id = self.selected_object?;
        self.scene.objects.iter().find(|object| object.id == id)
    }

    pub(super) fn selected_script_entry(&self) -> u16 {
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

    pub(super) fn selected_record(&self) -> Option<ScriptRecordInspection> {
        inspect_script_record(&self.scripts, self.navigation.selected_instruction()?)
    }

    pub(super) fn selected_object_references(&self) -> Vec<u16> {
        self.selected_record().map_or_else(Vec::new, |record| {
            scene_object_references(record, self.selected_object)
        })
    }

    pub(super) fn scene_input_status(&self) -> Option<String> {
        self.scene_input
            .as_ref()
            .map(|input| format!("GO TO SCENE {}  ENTER LOAD  ESC CANCEL", input))
    }

    pub(super) fn reset_script_navigation(&mut self) {
        self.navigation.reset(self.selected_script_entry());
        self.code_scroll = 0;
        self.reference_scroll = 0;
        self.dirty = true;
    }

    pub(super) fn navigate_to_entry(&mut self, entry: u16) {
        self.navigation.navigate(entry);
        self.code_scroll = 0;
        self.reference_scroll = 0;
        self.dirty = true;
    }

    pub(super) fn navigate_back(&mut self) {
        if self.navigation.go_back() {
            self.code_scroll = 0;
            self.reference_scroll = 0;
            self.dirty = true;
        }
    }

    pub(super) fn navigate_forward(&mut self) {
        if self.navigation.go_forward() {
            self.code_scroll = 0;
            self.reference_scroll = 0;
            self.dirty = true;
        }
    }

    pub(super) fn navigate_root(&mut self) {
        if self.navigation.go_root() {
            self.code_scroll = 0;
            self.reference_scroll = 0;
            self.dirty = true;
        }
    }

    pub(super) fn select_instruction(&mut self, record: ScriptRecordInspection) {
        if let Some(target) = navigable_target(record) {
            self.navigate_to_entry(target);
        } else {
            self.navigation.select_instruction(record.entry);
            self.reference_scroll = 0;
            self.dirty = true;
        }
    }

    pub(super) fn follow_reference(&mut self, source: ScriptReferenceSource) {
        match source {
            ScriptReferenceSource::Instruction { entry, .. } => self.navigate_to_entry(entry),
            ScriptReferenceSource::SceneEnter { scene } => {
                if self.ensure_scene(scene) {
                    self.selected_object = None;
                    self.selected_script = Some(SelectedScript::SceneEnter);
                    self.reset_script_navigation();
                }
            }
            ScriptReferenceSource::SceneTeleport { scene } => {
                if self.ensure_scene(scene) {
                    self.selected_object = None;
                    self.selected_script = Some(SelectedScript::SceneTeleport);
                    self.reset_script_navigation();
                }
            }
            ScriptReferenceSource::ObjectTrigger { scene, object_id } => {
                if self.ensure_scene(scene) {
                    self.select_object(object_id, true);
                    self.selected_script = Some(SelectedScript::ObjectTrigger);
                    self.reset_script_navigation();
                }
            }
            ScriptReferenceSource::ObjectAuto { scene, object_id } => {
                if self.ensure_scene(scene) {
                    self.select_object(object_id, true);
                    self.selected_script = Some(SelectedScript::ObjectAuto);
                    self.reset_script_navigation();
                }
            }
        }
    }

    pub(super) fn locate_object_reference(&mut self, object_id: u16) {
        let Some(scene) = self.script_references.scene_for_object(object_id) else {
            self.status = Some(format!("OBJECT #{object_id} HAS NO SCENE"));
            self.dirty = true;
            return;
        };
        if self.ensure_scene(scene) {
            self.select_object(object_id, true);
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
        if let Some(id) = script_object_ids(&self.scene.objects).first().copied() {
            self.center_on_object(id);
        } else if let Some(id) = self.scene.objects.first().map(|object| object.id) {
            self.center_on_object(id);
        } else {
            self.viewport.x = 0;
            self.viewport.y = 0;
        }
        self.clamp_viewport();
        self.reset_script_navigation();
    }

    pub(super) fn select_object(&mut self, id: u16, center: bool) {
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
        self.reveal_selected_in_object_list();
        if center {
            self.center_on_object(id);
        }
        self.reset_script_navigation();
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
        } else if index >= self.object_scroll + super::OBJECT_ROWS {
            self.object_scroll = index + 1 - super::OBJECT_ROWS;
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

    pub(super) fn switch_scene(&mut self, number: u16) {
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

    fn ensure_scene(&mut self, number: u16) -> bool {
        if self.scene.number != number {
            self.switch_scene(number);
        }
        self.scene.number == number
    }

    pub(super) fn switch_scene_by(&mut self, delta: i32) {
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

    pub(super) fn set_zoom(&mut self, zoom: u8, anchor: (i32, i32)) {
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

    pub(super) fn handle_key(&mut self, code: KeyCode) -> bool {
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
            KeyCode::KeyB => self.navigate_back(),
            KeyCode::KeyF => self.navigate_forward(),
            KeyCode::KeyR => self.navigate_root(),
            KeyCode::Enter | KeyCode::NumpadEnter => {
                if let Some(record) = self.selected_record() {
                    self.select_instruction(record);
                }
            }
            KeyCode::KeyO => {
                if let Some(object_id) = self.selected_object_references().first().copied() {
                    self.locate_object_reference(object_id);
                }
            }
            KeyCode::BracketLeft | KeyCode::PageUp => self.switch_scene_by(-1),
            KeyCode::BracketRight | KeyCode::PageDown => self.switch_scene_by(1),
            KeyCode::ArrowLeft | KeyCode::KeyA => self.pan_by(-32, 0),
            KeyCode::ArrowRight | KeyCode::KeyD => self.pan_by(32, 0),
            KeyCode::ArrowUp | KeyCode::KeyW => self.pan_by(0, -16),
            KeyCode::ArrowDown | KeyCode::KeyS => self.pan_by(0, 16),
            KeyCode::Equal | KeyCode::NumpadAdd => self.set_zoom(
                self.zoom.saturating_add(1),
                (CANVAS_WIDTH as i32 / 2, SCENE_EDITOR_HEIGHT as i32 / 2),
            ),
            KeyCode::Minus | KeyCode::NumpadSubtract => self.set_zoom(
                self.zoom.saturating_sub(1),
                (CANVAS_WIDTH as i32 / 2, SCENE_EDITOR_HEIGHT as i32 / 2),
            ),
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

    pub(super) fn cursor_moved(&mut self, cursor: (i32, i32)) {
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

    pub(super) fn pointer_pressed(&mut self) {
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

    pub(super) fn pointer_released(&mut self) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        if drag.moved || self.cursor.0 >= CANVAS_WIDTH as i32 {
            return;
        }
        let hit = hit_test_object_marker(
            &self.scene.objects,
            self.viewport,
            self.zoom,
            self.cursor,
            self.selected_object,
        )
        .or_else(|| {
            hit_test_visible_sprite(
                &self.scene.objects,
                &self.role_sprites,
                self.viewport,
                self.zoom,
                self.cursor,
                self.selected_object,
            )
        });
        if let Some(id) = hit {
            self.select_object(id, false);
        }
    }

    pub(super) fn cursor(&self) -> (i32, i32) {
        self.cursor
    }
}

pub(super) fn script_object_ids(objects: &[SceneObject]) -> Vec<u16> {
    objects
        .iter()
        .filter(|object| object.trigger_script != 0 || object.auto_script != 0)
        .map(|object| object.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use pal_core::role::Direction;

    use super::*;

    fn object(id: u16, trigger_script: u16, auto_script: u16) -> SceneObject {
        SceneObject {
            id,
            world_x: 0,
            world_y: 0,
            layer: 0,
            trigger_script,
            auto_script,
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
        let objects = [object(1, 0, 0), object(2, 10, 0), object(3, 0, 20)];
        assert_eq!(script_object_ids(&objects), vec![2, 3]);
    }
}
