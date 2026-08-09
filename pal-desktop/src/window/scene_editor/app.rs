//! Read-only scene-editor state and interaction logic.

use pal_assets::script::ScriptTable;
use pal_assets::text::{BitmapFont, TextLibrary};
use pal_core::scene::SceneObject;
use pal_core::script::{
    inspect_script_record, ScriptRecordInspection, ScriptReferenceCatalog, ScriptReferenceSource,
};

use super::super::debug_render::render_object_overlay;
use super::super::scene_render::render_tile_map;
use super::super::{LoadedScene, SceneEditorResources, Viewport};
use super::hit_test::{hit_test_object_marker, hit_test_visible_sprite};
use super::navigation::{navigable_target, scene_object_references, ScriptNavigation};
use super::{
    canvas_view_size, clamped_viewport, scale_canvas, CANVAS_HEIGHT, CANVAS_WIDTH, MAX_ZOOM,
};
use crate::renderer::Renderer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SelectedScript {
    SceneEnter,
    SceneTeleport,
    ObjectTrigger,
    ObjectAuto,
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
    pub(super) navigation: ScriptNavigation,
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
            viewport: Viewport::new(0, 0, CANVAS_WIDTH, CANVAS_HEIGHT),
            zoom: 1,
            selected_object: None,
            selected_script: None,
            navigation: ScriptNavigation::default(),
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

    pub(super) fn render_map(&mut self) {
        self.viewport.width = canvas_view_size(CANVAS_WIDTH, self.zoom);
        self.viewport.height = canvas_view_size(CANVAS_HEIGHT, self.zoom);
        render_tile_map(
            &mut self.renderer,
            &self.scene.map,
            Some(&self.role_sprites),
            &[],
            0,
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

    pub(super) fn reset_script_navigation(&mut self) {
        self.navigation.reset(self.selected_script_entry());
        self.dirty = true;
    }

    pub(super) fn navigate_to_entry(&mut self, entry: u16) {
        self.navigation.navigate(entry);
        self.dirty = true;
    }

    pub(super) fn navigate_back(&mut self) {
        if self.navigation.go_back() {
            self.dirty = true;
        }
    }

    pub(super) fn navigate_forward(&mut self) {
        if self.navigation.go_forward() {
            self.dirty = true;
        }
    }

    pub(super) fn navigate_root(&mut self) {
        if self.navigation.go_root() {
            self.dirty = true;
        }
    }

    pub(super) fn select_instruction(&mut self, record: ScriptRecordInspection) {
        if let Some(target) = navigable_target(record) {
            self.navigate_to_entry(target);
        } else {
            self.navigation.select_instruction(record.entry);
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
        if center {
            self.center_on_object(id);
        }
        self.reset_script_navigation();
    }

    pub(super) fn center_on_object(&mut self, id: u16) {
        let Some(object) = self.scene.objects.iter().find(|object| object.id == id) else {
            return;
        };
        self.viewport.width = canvas_view_size(CANVAS_WIDTH, self.zoom);
        self.viewport.height = canvas_view_size(CANVAS_HEIGHT, self.zoom);
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

    pub(super) fn pan_by(&mut self, dx: i32, dy: i32) {
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
        self.viewport.height = canvas_view_size(CANVAS_HEIGHT, zoom);
        self.viewport.x = world_x - anchor.0 / new_zoom;
        self.viewport.y = world_y - anchor.1 / new_zoom;
        self.clamp_viewport();
        self.dirty = true;
    }

    pub(super) fn select_next_script_object(&mut self) {
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

    pub(super) fn select_object_at(&mut self, cursor: (i32, i32)) {
        if cursor.0 < 0
            || cursor.1 < 0
            || cursor.0 >= CANVAS_WIDTH as i32
            || cursor.1 >= CANVAS_HEIGHT as i32
        {
            return;
        }
        let hit = hit_test_object_marker(
            &self.scene.objects,
            self.viewport,
            self.zoom,
            cursor,
            self.selected_object,
        )
        .or_else(|| {
            hit_test_visible_sprite(
                &self.scene.objects,
                &self.role_sprites,
                self.viewport,
                self.zoom,
                cursor,
                self.selected_object,
            )
        });
        if let Some(id) = hit {
            self.select_object(id, false);
        }
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
