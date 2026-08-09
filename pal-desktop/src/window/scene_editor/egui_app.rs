//! IDE-style egui shell for the read-only scene inspector.

use std::ops::Range;

use eframe::egui::{
    self, Color32, ColorImage, Key, Modifiers, RichText, Sense, TextureHandle, TextureOptions,
};
use egui_dock::{DockArea, DockState, NodeIndex, Style, TabViewer};
use egui_extras::{Column, TableBuilder};
use encoding_rs::BIG5;
use pal_assets::palette::Palette;
use pal_assets::text::BitmapFont;
use pal_core::script::{
    inspect_script_record, ScriptControlFlow, ScriptOpcode, ScriptRecordInspection,
    ScriptReferenceSource,
};
use serde::{Deserialize, Serialize};

use super::super::dialog_text::{dialog_token, DialogTokenKind};
use super::super::draw::{draw_debug_text, fill_rect, stroke_rect};
use super::super::text_render::{draw_dialog_text, DialogTextMode, DialogTextStyle};
use super::app::{script_object_ids, SceneEditorApp, SelectedScript};
use super::navigation::format_reference;
use super::{CANVAS_HEIGHT, CANVAS_WIDTH};
use crate::renderer::Renderer;

const DOCK_STATE_KEY: &str = "scene_editor_dock_state_v2";
// Render the original 320px-wide dialog layout, then let egui enlarge it with
// nearest-neighbor filtering. This keeps the PAL bitmap glyphs crisp and makes
// them comfortably readable on modern high-DPI displays.
const MESSAGE_WIDTH: usize = 320;
const MESSAGE_HEIGHT: usize = 80;

const TEXT: Color32 = Color32::from_rgb(220, 226, 232);
const MUTED: Color32 = Color32::from_rgb(132, 148, 164);
const ACCENT: Color32 = Color32::from_rgb(74, 214, 178);
const TRIGGER: Color32 = Color32::from_rgb(245, 168, 70);
const AUTO: Color32 = Color32::from_rgb(69, 199, 239);
const ERROR: Color32 = Color32::from_rgb(242, 100, 100);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
enum EditorTab {
    Scenes,
    Objects,
    Map,
    Script,
    Inspector,
    References,
    Message,
}

pub(super) struct EguiSceneEditorApp<L> {
    model: SceneEditorApp<L>,
    dock_state: DockState<EditorTab>,
    map_texture: Option<TextureHandle>,
    message_texture: Option<TextureHandle>,
    message_key: Option<(u16, u16)>,
    scene_filter: String,
    object_filter: String,
    entry_input: String,
}

impl<L> EguiSceneEditorApp<L>
where
    L: FnMut(u16, &pal_core::role::RoleSprites) -> Option<super::super::LoadedScene>,
{
    pub(super) fn new(
        context: &eframe::CreationContext<'_>,
        renderer: Renderer,
        scene: super::super::LoadedScene,
        resources: super::super::SceneEditorResources,
        load_scene: L,
    ) -> Self {
        configure_egui(&context.egui_ctx);
        let dock_state = context
            .storage
            .and_then(|storage| eframe::get_value(storage, DOCK_STATE_KEY))
            .unwrap_or_else(default_dock_state);
        let model = SceneEditorApp::new(renderer, scene, resources, load_scene);
        let entry_input = model
            .navigation
            .current()
            .map_or_else(String::new, |location| format!("{:04X}", location.entry));
        Self {
            model,
            dock_state,
            map_texture: None,
            message_texture: None,
            message_key: None,
            scene_filter: String::new(),
            object_filter: String::new(),
            entry_input,
        }
    }

    fn refresh_textures(&mut self, context: &egui::Context) {
        if self.model.is_dirty() || self.map_texture.is_none() {
            self.model.render_map();
            let image = ColorImage::from_rgba_unmultiplied(
                [self.model.renderer.width, self.model.renderer.height],
                self.model.screen(),
            );
            if let Some(texture) = self.map_texture.as_mut() {
                texture.set(image, TextureOptions::NEAREST);
            } else {
                self.map_texture =
                    Some(context.load_texture("scene-map", image, TextureOptions::NEAREST));
            }
        }

        let key = selected_message(&self.model);
        if key == self.message_key {
            return;
        }
        self.message_key = key;
        self.message_texture = key.and_then(|(entry, message_id)| {
            let message = self.model.text.message(usize::from(message_id))?;
            let image = message_preview_image(
                self.model.renderer.palette(),
                &self.model.font,
                message,
                message_id,
                entry,
            );
            Some(context.load_texture("script-message-preview", image, TextureOptions::NEAREST))
        });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.strong("SCENE");
            let mut scene = self.model.scene.number;
            if ui
                .add(
                    egui::DragValue::new(&mut scene)
                        .range(1..=self.model.scene_count)
                        .speed(1),
                )
                .changed()
            {
                self.model.switch_scene(scene);
            }
            if ui
                .button("Prev")
                .on_hover_text("Previous scene [PageUp]")
                .clicked()
            {
                self.model.switch_scene_by(-1);
            }
            if ui
                .button("Next")
                .on_hover_text("Next scene [PageDown]")
                .clicked()
            {
                self.model.switch_scene_by(1);
            }

            ui.separator();
            if ui
                .add_enabled(
                    self.model.navigation.can_go_back(),
                    egui::Button::new("Back"),
                )
                .on_hover_text("B")
                .clicked()
            {
                self.model.navigate_back();
            }
            if ui
                .add_enabled(
                    self.model.navigation.can_go_forward(),
                    egui::Button::new("Forward"),
                )
                .on_hover_text("F")
                .clicked()
            {
                self.model.navigate_forward();
            }
            if ui.button("Root").on_hover_text("R").clicked() {
                self.model.navigate_root();
            }

            let breadcrumb = self.model.navigation.current().map_or_else(
                || "ROOT -".to_owned(),
                |location| {
                    if location.root == location.entry {
                        format!("ROOT @{:04X}", location.root)
                    } else {
                        format!("ROOT @{:04X}  /  @{:04X}", location.root, location.entry)
                    }
                },
            );
            ui.label(RichText::new(breadcrumb).monospace().color(ACCENT));

            ui.separator();
            ui.label("Go to @");
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.entry_input)
                    .desired_width(62.0)
                    .font(egui::TextStyle::Monospace),
            );
            let submit = response.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
            if (ui.button("Go").clicked() || submit) && !self.entry_input.is_empty() {
                match parse_entry(&self.entry_input)
                    .filter(|entry| self.model.scripts.entry(*entry).is_some())
                {
                    Some(entry) => self.model.navigate_to_entry(entry),
                    None => {
                        self.model.status = Some(format!(
                            "ENTRY '{}' IS OUTSIDE THE SCRIPT TABLE",
                            self.entry_input
                        ))
                    }
                }
            }

            ui.separator();
            if ui.button("Reset layout").clicked() {
                self.dock_state = default_dock_state();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new("READ ONLY").strong().color(TRIGGER));
            });
        });
    }

    fn handle_shortcuts(&mut self, context: &egui::Context) {
        if context.wants_keyboard_input() {
            return;
        }
        let pressed = |key| context.input_mut(|input| input.consume_key(Modifiers::NONE, key));
        if pressed(Key::B) {
            self.model.navigate_back();
        } else if pressed(Key::F) {
            self.model.navigate_forward();
        } else if pressed(Key::R) {
            self.model.navigate_root();
        } else if pressed(Key::O) {
            if let Some(id) = self.model.selected_object_references().first().copied() {
                self.model.locate_object_reference(id);
            }
        } else if pressed(Key::N) {
            self.model.select_next_script_object();
        } else if pressed(Key::Enter) {
            if let Some(record) = self.model.selected_record() {
                self.model.select_instruction(record);
            }
        } else if pressed(Key::PageUp) {
            self.model.switch_scene_by(-1);
        } else if pressed(Key::PageDown) {
            self.model.switch_scene_by(1);
        } else if pressed(Key::ArrowLeft) || pressed(Key::A) {
            self.model.pan_by(-32, 0);
        } else if pressed(Key::ArrowRight) || pressed(Key::D) {
            self.model.pan_by(32, 0);
        } else if pressed(Key::ArrowUp) || pressed(Key::W) {
            self.model.pan_by(0, -16);
        } else if pressed(Key::ArrowDown) || pressed(Key::S) {
            self.model.pan_by(0, 16);
        } else if pressed(Key::Plus) || pressed(Key::Equals) {
            self.model.set_zoom(
                self.model.zoom.saturating_add(1),
                (CANVAS_WIDTH as i32 / 2, CANVAS_HEIGHT as i32 / 2),
            );
        } else if pressed(Key::Minus) {
            self.model.set_zoom(
                self.model.zoom.saturating_sub(1),
                (CANVAS_WIDTH as i32 / 2, CANVAS_HEIGHT as i32 / 2),
            );
        } else if pressed(Key::Home) {
            if let Some(id) = self.model.selected_object {
                self.model.center_on_object(id);
                self.model.mark_dirty();
            }
        }
    }
}

impl<L> eframe::App for EguiSceneEditorApp<L>
where
    L: FnMut(u16, &pal_core::role::RoleSprites) -> Option<super::super::LoadedScene> + 'static,
{
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_shortcuts(context);
        self.refresh_textures(context);
        context.send_viewport_cmd(egui::ViewportCommand::Title(self.model.title()));

        egui::TopBottomPanel::top("scene-editor-toolbar")
            .exact_height(38.0)
            .show(context, |ui| self.toolbar(ui));
        egui::TopBottomPanel::bottom("scene-editor-status")
            .exact_height(24.0)
            .show(context, |ui| {
                ui.horizontal(|ui| {
                    ui.label(self.model.status.as_deref().unwrap_or(
                        "Drag map to pan • wheel to zoom • click flow rows to navigate",
                    ));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.monospace(format!(
                            "VIEW {},{}  ZOOM {}x",
                            self.model.viewport.x, self.model.viewport.y, self.model.zoom
                        ));
                    });
                });
            });
        egui::CentralPanel::default().show(context, |ui| {
            let mut viewer = EditorTabViewer {
                model: &mut self.model,
                map_texture: self.map_texture.as_ref(),
                message_texture: self.message_texture.as_ref(),
                scene_filter: &mut self.scene_filter,
                object_filter: &mut self.object_filter,
            };
            DockArea::new(&mut self.dock_state)
                .style(Style::from_egui(ui.style().as_ref()))
                .show_inside(ui, &mut viewer);
        });

        if self.model.is_dirty() {
            context.request_repaint();
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, DOCK_STATE_KEY, &self.dock_state);
    }
}

struct EditorTabViewer<'a, L> {
    model: &'a mut SceneEditorApp<L>,
    map_texture: Option<&'a TextureHandle>,
    message_texture: Option<&'a TextureHandle>,
    scene_filter: &'a mut String,
    object_filter: &'a mut String,
}

impl<L> TabViewer for EditorTabViewer<'_, L>
where
    L: FnMut(u16, &pal_core::role::RoleSprites) -> Option<super::super::LoadedScene>,
{
    type Tab = EditorTab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        match tab {
            EditorTab::Scenes => "Scenes",
            EditorTab::Objects => "Objects",
            EditorTab::Map => "Map",
            EditorTab::Script => "Script",
            EditorTab::Inspector => "Inspector",
            EditorTab::References => "References",
            EditorTab::Message => "Message",
        }
        .into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match tab {
            EditorTab::Scenes => self.scenes_ui(ui),
            EditorTab::Objects => self.objects_ui(ui),
            EditorTab::Map => self.map_ui(ui),
            EditorTab::Script => self.script_ui(ui),
            EditorTab::Inspector => self.inspector_ui(ui),
            EditorTab::References => self.references_ui(ui),
            EditorTab::Message => self.message_ui(ui),
        }
    }

    fn closeable(&mut self, _tab: &mut Self::Tab) -> bool {
        false
    }

    fn scroll_bars(&self, tab: &Self::Tab) -> [bool; 2] {
        match tab {
            EditorTab::Map | EditorTab::Script => [false, false],
            _ => [true, true],
        }
    }
}

impl<L> EditorTabViewer<'_, L>
where
    L: FnMut(u16, &pal_core::role::RoleSprites) -> Option<super::super::LoadedScene>,
{
    fn scenes_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Scenes");
            ui.label(RichText::new(self.model.scene_count.to_string()).color(MUTED));
        });
        ui.add(
            egui::TextEdit::singleline(self.scene_filter)
                .hint_text("Filter scene number…")
                .desired_width(f32::INFINITY),
        );
        let filter = self.scene_filter.trim().to_owned();
        let current = self.model.scene.number;
        let mut load = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for scene in 1..=self.model.scene_count {
                let label = format!("Scene {scene:03}");
                if !filter.is_empty()
                    && !label
                        .to_ascii_lowercase()
                        .contains(&filter.to_ascii_lowercase())
                {
                    continue;
                }
                if ui.selectable_label(scene == current, label).clicked() {
                    load = Some(scene);
                }
            }
        });
        if let Some(scene) = load {
            self.model.switch_scene(scene);
        }
    }

    fn objects_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading(format!("Scene {} scripts", self.model.scene.number));
        if ui
            .selectable_label(
                self.model.selected_script == Some(SelectedScript::SceneEnter),
                RichText::new(format!("Enter      @{:04X}", self.model.scene.enter_script))
                    .monospace()
                    .color(TRIGGER),
            )
            .clicked()
        {
            self.model.selected_object = None;
            self.model.selected_script = Some(SelectedScript::SceneEnter);
            self.model.reset_script_navigation();
        }
        if ui
            .selectable_label(
                self.model.selected_script == Some(SelectedScript::SceneTeleport),
                RichText::new(format!(
                    "Teleport   @{:04X}",
                    self.model.scene.teleport_script
                ))
                .monospace()
                .color(TRIGGER),
            )
            .clicked()
        {
            self.model.selected_object = None;
            self.model.selected_script = Some(SelectedScript::SceneTeleport);
            self.model.reset_script_navigation();
        }
        ui.separator();
        ui.add(
            egui::TextEdit::singleline(self.object_filter)
                .hint_text("Filter object ID or entry…")
                .desired_width(f32::INFINITY),
        );
        let filter = self.object_filter.trim().to_ascii_lowercase();
        let ids = script_object_ids(&self.model.scene.objects);
        let mut select = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for id in ids {
                let Some(object) = self
                    .model
                    .scene
                    .objects
                    .iter()
                    .find(|object| object.id == id)
                else {
                    continue;
                };
                let label = format!(
                    "#{:<4} T@{:04X}  A@{:04X}  S{}",
                    object.id, object.trigger_script, object.auto_script, object.state
                );
                if !filter.is_empty() && !label.to_ascii_lowercase().contains(&filter) {
                    continue;
                }
                let color = object_color(object);
                if ui
                    .selectable_label(
                        self.model.selected_object == Some(id),
                        RichText::new(label).monospace().color(color),
                    )
                    .clicked()
                {
                    select = Some(id);
                }
            }
        });
        if let Some(id) = select {
            self.model.select_object(id, true);
        }
    }

    fn map_ui(&mut self, ui: &mut egui::Ui) {
        let Some(texture) = self.map_texture else {
            ui.spinner();
            return;
        };
        let available = ui.available_size();
        let source = egui::vec2(CANVAS_WIDTH as f32, CANVAS_HEIGHT as f32);
        let scale = (available.x / source.x)
            .min(available.y / source.y)
            .max(0.05);
        let size = source * scale;
        let response = ui.add(
            egui::Image::new((texture.id(), size))
                .texture_options(TextureOptions::NEAREST)
                .sense(Sense::click_and_drag()),
        );
        if response.dragged() {
            let delta = ui.input(|input| input.pointer.delta());
            let dx = -(delta.x * source.x / response.rect.width() / f32::from(self.model.zoom))
                .round() as i32;
            let dy = -(delta.y * source.y / response.rect.height() / f32::from(self.model.zoom))
                .round() as i32;
            if dx != 0 || dy != 0 {
                self.model.pan_by(dx, dy);
            }
        }
        if response.clicked() {
            if let Some(position) = response.interact_pointer_pos() {
                self.model
                    .select_object_at(map_position(response.rect, position));
            }
        }
        if response.hovered() {
            let scroll = ui.input(|input| input.raw_scroll_delta.y);
            if scroll != 0.0 {
                let anchor = response
                    .hover_pos()
                    .map(|position| map_position(response.rect, position))
                    .unwrap_or((CANVAS_WIDTH as i32 / 2, CANVAS_HEIGHT as i32 / 2));
                let zoom = if scroll > 0.0 {
                    self.model.zoom.saturating_add(1)
                } else {
                    self.model.zoom.saturating_sub(1)
                };
                self.model.set_zoom(zoom, anchor);
            }
        }
        response.on_hover_text("Drag to pan • wheel to zoom • click objects to inspect");
    }

    fn script_ui(&mut self, ui: &mut egui::Ui) {
        let Some(location) = self.model.navigation.current() else {
            ui.centered_and_justified(|ui| ui.label("Script entry is disabled (@0000)"));
            return;
        };
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("ROOT @{:04X}", location.root))
                    .monospace()
                    .color(ACCENT),
            );
            if location.entry != location.root {
                ui.label(
                    RichText::new(format!("CURRENT @{:04X}", location.entry))
                        .monospace()
                        .color(AUTO),
                );
            }
            ui.label(
                RichText::new("Rows continue to the end of SSS.MKF; STOP is not a boundary")
                    .color(MUTED),
            );
        });

        let start = usize::from(location.entry);
        let rows = self.model.scripts.len().saturating_sub(start);
        let selected_entry = self.model.navigation.selected_instruction();
        let scripts = &self.model.scripts;
        let mut action = None;
        TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::exact(66.0))
            .column(Column::remainder().at_least(145.0))
            .column(Column::exact(62.0))
            .column(Column::exact(62.0))
            .column(Column::exact(62.0))
            .column(Column::remainder().at_least(105.0))
            .header(24.0, |mut header| {
                for label in ["ENTRY", "OPCODE", "OP 0", "OP 1", "OP 2", "FLOW"] {
                    header.col(|ui| {
                        ui.strong(label);
                    });
                }
            })
            .body(|body| {
                body.rows(22.0, rows, |mut row| {
                    let index = row.index();
                    let Some(entry) = start
                        .checked_add(index)
                        .and_then(|entry| u16::try_from(entry).ok())
                    else {
                        return;
                    };
                    let Some(record) = inspect_script_record(scripts, entry) else {
                        return;
                    };
                    row.set_selected(selected_entry == Some(entry));
                    let color = flow_color(record);
                    row.col(|ui| {
                        if ui
                            .selectable_label(
                                selected_entry == Some(entry),
                                RichText::new(format!("@{entry:04X}"))
                                    .monospace()
                                    .color(color),
                            )
                            .clicked()
                        {
                            action = Some(record);
                        }
                    });
                    row.col(|ui| {
                        let name = record.opcode.map_or("UNKNOWN", ScriptOpcode::name);
                        if ui
                            .selectable_label(false, RichText::new(name).monospace().color(color))
                            .clicked()
                        {
                            action = Some(record);
                        }
                    });
                    for operand in record.instruction.operands {
                        row.col(|ui| {
                            ui.monospace(format!("{operand:04X}"));
                        });
                    }
                    row.col(|ui| {
                        let (flow, target) = format_flow(record);
                        let response = ui
                            .selectable_label(false, RichText::new(flow).monospace().color(color));
                        if response.clicked() {
                            action = Some(record);
                        }
                        if let Some(target) = target {
                            response.on_hover_text(format!("Navigate to @{target:04X}"));
                        }
                    });
                });
            });
        if let Some(record) = action {
            self.model.select_instruction(record);
        }
    }

    fn inspector_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Selection");
        if let Some(object) = self.model.selected_object().cloned() {
            ui.label(
                RichText::new(format!("OBJECT #{}", object.id))
                    .strong()
                    .color(ACCENT),
            );
            egui::Grid::new("object-fields")
                .num_columns(2)
                .striped(true)
                .show(ui, |ui| {
                    field(
                        ui,
                        "Position",
                        format!("{}, {}", object.world_x, object.world_y),
                    );
                    field(ui, "State", object.state);
                    field(ui, "Layer", object.layer);
                    field(ui, "Trigger mode", object.trigger_mode);
                    field(
                        ui,
                        "Sprite",
                        object
                            .sprite_index
                            .map_or_else(|| "-".to_owned(), |value| value.to_string()),
                    );
                    field(ui, "Frame", object.current_frame);
                    field(ui, "Direction", object.direction as u16);
                    field(ui, "Visible", yes_no(object.is_visible()));
                    field(ui, "Blocker", yes_no(object.is_blocker()));
                    field(ui, "Search", yes_no(object.can_search()));
                    field(ui, "Touch", yes_no(object.can_touch()));
                });
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(
                        self.model.selected_script == Some(SelectedScript::ObjectTrigger),
                        RichText::new(format!("Trigger @{:04X}", object.trigger_script))
                            .monospace()
                            .color(TRIGGER),
                    )
                    .clicked()
                {
                    self.model.selected_script = Some(SelectedScript::ObjectTrigger);
                    self.model.reset_script_navigation();
                }
                if ui
                    .selectable_label(
                        self.model.selected_script == Some(SelectedScript::ObjectAuto),
                        RichText::new(format!("Auto @{:04X}", object.auto_script))
                            .monospace()
                            .color(AUTO),
                    )
                    .clicked()
                {
                    self.model.selected_script = Some(SelectedScript::ObjectAuto);
                    self.model.reset_script_navigation();
                }
            });
        } else {
            ui.label("No event object selected");
        }

        ui.separator();
        ui.heading("Instruction");
        let Some(record) = self.model.selected_record() else {
            ui.label(RichText::new("No instruction selected").color(MUTED));
            return;
        };
        let name = record.opcode.map_or("UNKNOWN", ScriptOpcode::name);
        ui.label(
            RichText::new(format!("@{:04X}  {name}", record.entry))
                .monospace()
                .strong()
                .color(flow_color(record)),
        );
        ui.add_space(4.0);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new("Meaning").strong().color(ACCENT));
            if let Some(opcode) = record.opcode {
                ui.label(opcode_explanation(
                    opcode,
                    record,
                    self.model.selected_object,
                ));
                ui.label(
                    RichText::new(flow_explanation(record))
                        .color(MUTED)
                        .italics(),
                );
                self.instruction_links_ui(ui, record);
                if opcode == ScriptOpcode::PrintMessage {
                    self.inline_message_ui(ui, record);
                }
            } else {
                ui.label(
                    RichText::new("Unknown opcode; no reliable behavior explanation is available.")
                        .color(ERROR)
                        .italics(),
                );
            }
        });

        ui.add_space(4.0);
        ui.collapsing("Raw instruction", |ui| {
            egui::Grid::new("instruction-fields")
                .num_columns(2)
                .striped(true)
                .show(ui, |ui| {
                    field(
                        ui,
                        "Raw opcode",
                        format!("{:04X}", record.instruction.opcode),
                    );
                    for (index, operand) in record.instruction.operands.into_iter().enumerate() {
                        field(
                            ui,
                            &format!("Operand {index}"),
                            format!("{operand:04X}  ({operand})"),
                        );
                    }
                    field(ui, "Control flow", format_flow(record).0);
                });
        });
    }

    fn instruction_links_ui(&mut self, ui: &mut egui::Ui, record: ScriptRecordInspection) {
        let object_ids = self.model.selected_object_references();
        let script_targets = instruction_script_targets(record);
        if object_ids.is_empty() && script_targets.is_empty() {
            return;
        }

        let mut locate_object = None;
        let mut navigate_script = None;
        ui.add_space(5.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Go to").strong().color(MUTED));
            for object_id in object_ids {
                let valid = self
                    .model
                    .script_references
                    .scene_for_object(object_id)
                    .is_some();
                let response = ui.add_enabled(
                    valid,
                    egui::Button::new(
                        RichText::new(format!("Object #{object_id}"))
                            .monospace()
                            .color(ACCENT),
                    )
                    .frame(false),
                );
                if response
                    .on_hover_text(if valid {
                        "Locate this event object"
                    } else {
                        "This event object is not indexed in any scene"
                    })
                    .clicked()
                {
                    locate_object = Some(object_id);
                }
            }
            for (label, entry) in script_targets {
                let valid = self.model.scripts.entry(entry).is_some();
                let response = ui.add_enabled(
                    valid,
                    egui::Button::new(
                        RichText::new(format!("{label} @{entry:04X}"))
                            .monospace()
                            .color(AUTO),
                    )
                    .frame(false),
                );
                if response
                    .on_hover_text(if valid {
                        "Navigate to this script entry"
                    } else {
                        "This entry is outside the script table"
                    })
                    .clicked()
                {
                    navigate_script = Some(entry);
                }
            }
        });

        if let Some(object_id) = locate_object {
            self.model.locate_object_reference(object_id);
        } else if let Some(entry) = navigate_script {
            self.model.navigate_to_entry(entry);
        }
    }

    fn inline_message_ui(&self, ui: &mut egui::Ui, record: ScriptRecordInspection) {
        let message_id = record.instruction.operands[0];
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("MESSAGE #{message_id}"))
                    .monospace()
                    .strong()
                    .color(TRIGGER),
            );
            ui.label(RichText::new("operand 0 / M.MSG index").color(MUTED));
        });

        let Some(message) = self.model.text.message(usize::from(message_id)) else {
            ui.label(
                RichText::new("Message index is outside M.MSG.")
                    .color(ERROR)
                    .italics(),
            );
            return;
        };

        if let Some(text) = decode_dialog_message(message).filter(|text| text.is_ascii()) {
            ui.add(
                egui::Label::new(RichText::new(text).size(15.0).color(TEXT))
                    .wrap()
                    .selectable(true),
            );
        }
        if let Some(texture) = self.message_texture {
            ui.label(RichText::new("Original in-game text").color(MUTED));
            let scale = (ui.available_width() / MESSAGE_WIDTH as f32).clamp(0.5, 2.0);
            ui.add(
                egui::Image::new((
                    texture.id(),
                    egui::vec2(MESSAGE_WIDTH as f32, MESSAGE_HEIGHT as f32) * scale,
                ))
                .texture_options(TextureOptions::NEAREST),
            );
        }
    }

    fn references_ui(&mut self, ui: &mut egui::Ui) {
        let Some(record) = self.model.selected_record() else {
            ui.centered_and_justified(|ui| ui.label("Select a script instruction"));
            return;
        };
        ui.heading(format!("Incoming references to @{:04X}", record.entry));
        let references = self
            .model
            .script_references
            .references_to(record.entry)
            .to_vec();
        let mut follow = None;
        if references.is_empty() {
            ui.label(RichText::new("No indexed incoming references").color(MUTED));
        } else {
            for source in references {
                if ui
                    .button(
                        RichText::new(format_reference(source))
                            .monospace()
                            .color(reference_color(source)),
                    )
                    .clicked()
                {
                    follow = Some(source);
                }
            }
        }
        if let Some(source) = follow {
            self.model.follow_reference(source);
        }

        ui.separator();
        ui.heading("Referenced event objects");
        let object_ids = self.model.selected_object_references();
        if object_ids.is_empty() {
            ui.label(
                RichText::new("This instruction has no recognized object operand").color(MUTED),
            );
        } else {
            for object_id in object_ids {
                if ui.button(format!("Locate object #{object_id}")).clicked() {
                    self.model.locate_object_reference(object_id);
                }
            }
        }
    }

    fn message_ui(&mut self, ui: &mut egui::Ui) {
        let Some((entry, message_id)) = selected_message(self.model) else {
            ui.centered_and_justified(|ui| {
                ui.label("Select a PrintMessage instruction to preview original text")
            });
            return;
        };
        ui.horizontal(|ui| {
            ui.heading(format!("Message {message_id}"));
            ui.label(
                RichText::new(format!("from @{entry:04X}"))
                    .monospace()
                    .color(MUTED),
            );
        });
        if let Some(texture) = self.message_texture {
            let available = ui.available_width();
            let scale = (available / MESSAGE_WIDTH as f32).clamp(0.5, 2.0);
            ui.add(
                egui::Image::new((
                    texture.id(),
                    egui::vec2(MESSAGE_WIDTH as f32, MESSAGE_HEIGHT as f32) * scale,
                ))
                .texture_options(TextureOptions::NEAREST),
            );
        }
        if let Some(message) = self.model.text.message(usize::from(message_id)) {
            if let Some(text) = decode_dialog_message(message).filter(|text| text.is_ascii()) {
                ui.label(RichText::new(text).size(15.0).color(TEXT));
            }
            ui.collapsing("Raw Big5 bytes", |ui| {
                ui.monospace(
                    message
                        .iter()
                        .map(|byte| format!("{byte:02X}"))
                        .collect::<Vec<_>>()
                        .join(" "),
                );
            });
        }
    }
}

fn configure_egui(context: &egui::Context) {
    context.set_theme(egui::Theme::Dark);
    context.set_visuals(egui::Visuals::dark());
    let mut style = (*context.style()).clone();
    style.spacing.item_spacing = egui::vec2(6.0, 5.0);
    style.spacing.button_padding = egui::vec2(8.0, 4.0);
    context.set_style(style);
}

fn default_dock_state() -> DockState<EditorTab> {
    let mut state = DockState::new(vec![EditorTab::Map]);
    let [map_node, _left_node] = state.main_surface_mut().split_left(
        NodeIndex::root(),
        0.18,
        vec![EditorTab::Scenes, EditorTab::Objects],
    );
    let [map_node, _right_node] = state.main_surface_mut().split_right(
        map_node,
        0.76,
        vec![EditorTab::Inspector, EditorTab::References],
    );
    state.main_surface_mut().split_below(
        map_node,
        0.66,
        vec![EditorTab::Script, EditorTab::Message],
    );
    state
}

fn selected_message<L>(model: &SceneEditorApp<L>) -> Option<(u16, u16)>
where
    L: FnMut(u16, &pal_core::role::RoleSprites) -> Option<super::super::LoadedScene>,
{
    let record = model
        .selected_record()
        .filter(|record| record.opcode == Some(ScriptOpcode::PrintMessage))?;
    Some((record.entry, record.instruction.operands[0]))
}

fn map_position(rect: egui::Rect, position: egui::Pos2) -> (i32, i32) {
    let x = ((position.x - rect.left()) * CANVAS_WIDTH as f32 / rect.width())
        .floor()
        .clamp(0.0, CANVAS_WIDTH.saturating_sub(1) as f32) as i32;
    let y = ((position.y - rect.top()) * CANVAS_HEIGHT as f32 / rect.height())
        .floor()
        .clamp(0.0, CANVAS_HEIGHT.saturating_sub(1) as f32) as i32;
    (x, y)
}

fn parse_entry(input: &str) -> Option<u16> {
    let input = input.trim();
    let input = input
        .strip_prefix('@')
        .or_else(|| input.strip_prefix("0x"))
        .or_else(|| input.strip_prefix("0X"))
        .unwrap_or(input);
    (!input.is_empty())
        .then(|| u16::from_str_radix(input, 16).ok())
        .flatten()
}

fn format_flow(record: ScriptRecordInspection) -> (String, Option<u16>) {
    match record.flow {
        ScriptControlFlow::Next => ("NEXT".to_owned(), None),
        ScriptControlFlow::Stop => ("STOP".to_owned(), None),
        ScriptControlFlow::Jump {
            target,
            conditional: true,
        } => (
            format!(
                "BRANCH -> @{target:04X}{}",
                if target <= record.entry { " LOOP" } else { "" }
            ),
            Some(target),
        ),
        ScriptControlFlow::Jump { target, .. } => (
            format!(
                "JUMP -> @{target:04X}{}",
                if target <= record.entry { " LOOP" } else { "" }
            ),
            Some(target),
        ),
        ScriptControlFlow::Call { target, .. } => (format!("CALL -> @{target:04X}"), Some(target)),
        ScriptControlFlow::Random { .. } => ("RANDOM".to_owned(), None),
        ScriptControlFlow::Unknown => ("INVALID".to_owned(), None),
    }
}

fn flow_explanation(record: ScriptRecordInspection) -> String {
    let next = record.entry.wrapping_add(1);
    match record.flow {
        ScriptControlFlow::Next => format!("Continue at @{next:04X} after this instruction."),
        ScriptControlFlow::Stop => "End the current script after this instruction.".to_owned(),
        ScriptControlFlow::Jump {
            target,
            conditional: true,
        } => format!("Jump to @{target:04X} when true; otherwise continue at @{next:04X}."),
        ScriptControlFlow::Jump { target, .. } => format!("Jump directly to @{target:04X}."),
        ScriptControlFlow::Call {
            target,
            return_entry,
        } => format!("Call @{target:04X}, then return to @{return_entry:04X}."),
        ScriptControlFlow::Random { choices } => {
            format!("Choose one of the next {choices} instructions at random.")
        }
        ScriptControlFlow::Unknown => "The resulting control flow is unknown.".to_owned(),
    }
}

fn instruction_script_targets(record: ScriptRecordInspection) -> Vec<(&'static str, u16)> {
    use ScriptOpcode::*;

    let mut targets = Vec::new();
    let mut push = |label, entry| {
        if entry != 0 && !targets.iter().any(|(_, target)| *target == entry) {
            targets.push((label, entry));
        }
    };

    match record.flow {
        ScriptControlFlow::Jump {
            target,
            conditional: true,
        } => push("Branch", target),
        ScriptControlFlow::Jump { target, .. } => push("Jump", target),
        ScriptControlFlow::Call { target, .. } => push("Call", target),
        ScriptControlFlow::Next
        | ScriptControlFlow::Stop
        | ScriptControlFlow::Random { .. }
        | ScriptControlFlow::Unknown => {}
    }

    let Some(opcode) = record.opcode else {
        return targets;
    };
    let [op0, op1, op2] = record.instruction.operands;
    match opcode {
        StopAndReplace => push("Replacement", op0),
        SetObjectAutoScript => push("Auto", op1),
        SetObjectTriggerScript => push("Trigger", op1),
        SetObjectScript => push("Object script", op1),
        SetSceneScripts => {
            push("Scene enter", op1);
            push("Scene teleport", op2);
        }
        StartBattle => {
            push("On lost", op1);
            push("On fled", op2);
        }
        PlaceUsedItemObject | SetEnemyStatus | SummonEnemy => push("On failure", op2),
        FleeBattle | CollectEnemy => push("On failure", op0),
        DivideEnemy => push("On failure", op1),
        _ => {}
    }
    targets
}

fn opcode_explanation(
    opcode: ScriptOpcode,
    record: ScriptRecordInspection,
    current_object: Option<u16>,
) -> String {
    use ScriptOpcode::*;

    let [op0, op1, op2] = record.instruction.operands;
    match opcode {
        PrintMessage => format!("Display message #{op0} from M.MSG."),
        PlayMusic => format!(
            "Play music #{op0}{}.",
            if op1 == 1 { " once" } else { " in a loop" }
        ),
        SetBattleMusic => format!("Use music #{op0} for the next battle."),
        PlaySound => format!("Play sound effect #{op0}."),
        StartBattle => format!("Start battle with enemy team #{op0}."),
        WaitFrames => format!("Wait {op0} scene frame(s)."),
        Delay => format!("Wait {op0} × 80 ms."),
        SetPartyPosition => format!("Place the party at tile ({op0}, {op1}), half {op2}."),
        SetObjectPosition => format!(
            "Place event object {} at ({op1}, {op2}).",
            object_selector_label(op0, current_object)
        ),
        SetObjectState => format!(
            "Set event object {} state to {}.",
            object_selector_label(op0, current_object),
            op1 as i16
        ),
        SetObjectAutoScript => format!(
            "Set event object {} auto script to @{op1:04X}.",
            object_selector_label(op0, current_object)
        ),
        SetObjectTriggerScript => {
            format!(
                "Set event object {} trigger script to @{op1:04X}.",
                object_selector_label(op0, current_object)
            )
        }
        ChangeScene => format!("Switch to scene {op0}."),
        AddItem => format!("Add {} of item #{op0} to the inventory.", op1 as i16),
        RemoveItem => format!("Remove {} of item #{op0} from the inventory.", op1.max(1)),
        OpenBuyMenu => format!("Open store #{op0} for buying."),
        OpenSellMenu => "Open the inventory selling menu.".to_owned(),
        DialogCenter => "Place following dialog in the center of the screen.".to_owned(),
        DialogUpper => format!("Place following dialog at the top, using face #{op0}."),
        DialogLower => format!("Place following dialog at the bottom, using face #{op0}."),
        DialogCenterWindow => "Show the following message in a centered window.".to_owned(),
        Jump => format!("Jump to script entry @{op0:04X}."),
        Call => format!(
            "Call script entry @{op0:04X} for event object {}.",
            object_selector_label(op1, current_object)
        ),
        Stop => "Stop the current script.".to_owned(),
        StopAndAdvance => "Stop and persist the next instruction as the new entry.".to_owned(),
        StopAndReplace => format!("Stop and persist @{op0:04X} as the new entry."),
        _ => format!("{}.", humanize_opcode_name(opcode.name())),
    }
}

fn object_selector_label(selector: u16, current_object: Option<u16>) -> String {
    if selector == 0 || selector == u16::MAX {
        current_object.map_or_else(
            || "selected by the script owner".to_owned(),
            |object_id| format!("#{object_id} (current)"),
        )
    } else {
        format!("#{selector}")
    }
}

fn humanize_opcode_name(name: &str) -> String {
    let mut result = String::with_capacity(name.len() + 8);
    let characters = name.chars().collect::<Vec<_>>();
    for (index, character) in characters.iter().copied().enumerate() {
        let previous = index.checked_sub(1).and_then(|index| characters.get(index));
        let next = characters.get(index + 1);
        let starts_word = character.is_ascii_uppercase()
            && index != 0
            && (previous.is_some_and(char::is_ascii_lowercase)
                || next.is_some_and(char::is_ascii_lowercase));
        if starts_word {
            result.push(' ');
        }
        if index == 0 {
            result.push(character);
        } else {
            result.extend(character.to_lowercase());
        }
    }
    result
}

fn decode_dialog_message(message: &[u8]) -> Option<String> {
    let mut visible = Vec::with_capacity(message.len());
    let mut index = 0;
    while index < message.len() {
        let token = dialog_token(message, index);
        match token.kind {
            DialogTokenKind::Glyph => {
                if message[index] == b'\\' && token.bytes == 2 {
                    visible.push(message[index + 1]);
                } else {
                    visible.extend_from_slice(&message[index..index + token.bytes]);
                }
            }
            DialogTokenKind::Color => {}
            DialogTokenKind::Delay(frames) => {
                visible.extend_from_slice(format!(" [delay {frames}] ").as_bytes());
            }
            DialogTokenKind::Terminate(frames) => {
                if frames != 0 {
                    visible.extend_from_slice(format!(" [wait {frames}] ").as_bytes());
                }
                break;
            }
            DialogTokenKind::Icon(icon) => {
                visible.extend_from_slice(format!(" [icon {icon}] ").as_bytes());
            }
            DialogTokenKind::LineBreak => visible.push(b'\n'),
        }
        index += token.bytes;
    }

    let (decoded, _, _) = BIG5.decode(&visible);
    let decoded = decoded.trim().to_owned();
    (!decoded.is_empty()).then_some(decoded)
}

fn flow_color(record: ScriptRecordInspection) -> Color32 {
    match record.flow {
        ScriptControlFlow::Jump {
            conditional: true, ..
        } => TRIGGER,
        ScriptControlFlow::Jump { .. } => ACCENT,
        ScriptControlFlow::Call { .. } => AUTO,
        ScriptControlFlow::Stop | ScriptControlFlow::Random { .. } => MUTED,
        ScriptControlFlow::Unknown => ERROR,
        ScriptControlFlow::Next => TEXT,
    }
}

fn reference_color(source: ScriptReferenceSource) -> Color32 {
    match source {
        ScriptReferenceSource::SceneEnter { .. }
        | ScriptReferenceSource::SceneTeleport { .. }
        | ScriptReferenceSource::ObjectTrigger { .. } => TRIGGER,
        ScriptReferenceSource::ObjectAuto { .. } => AUTO,
        ScriptReferenceSource::Instruction { .. } => TEXT,
    }
}

fn object_color(object: &pal_core::scene::SceneObject) -> Color32 {
    match (object.trigger_script != 0, object.auto_script != 0) {
        (true, true) => Color32::from_rgb(244, 108, 220),
        (true, false) => TRIGGER,
        (false, true) => AUTO,
        (false, false) => MUTED,
    }
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn field(ui: &mut egui::Ui, label: &str, value: impl ToString) {
    ui.label(RichText::new(label).color(MUTED));
    ui.monospace(value.to_string());
    ui.end_row();
}

fn message_preview_image(
    palette: &Palette,
    font: &BitmapFont,
    message: &[u8],
    message_id: u16,
    entry: u16,
) -> ColorImage {
    let mut renderer = Renderer::new(palette.clone(), MESSAGE_WIDTH, MESSAGE_HEIGHT);
    renderer.clear(8, 12, 16);
    fill_rect(
        &mut renderer,
        0,
        0,
        MESSAGE_WIDTH as i32,
        MESSAGE_HEIGHT as i32,
        [8, 12, 16, 255],
    );
    stroke_rect(
        &mut renderer,
        0,
        0,
        MESSAGE_WIDTH as i32,
        MESSAGE_HEIGHT as i32,
        [76, 92, 104, 255],
    );
    draw_debug_text(
        &mut renderer,
        10,
        5,
        &format!("MESSAGE {message_id} FROM @{entry:04X}"),
        [64, 224, 144, 255],
    );
    for (line, range) in message_preview_ranges(message, MESSAGE_WIDTH - 20, 3)
        .into_iter()
        .enumerate()
    {
        let _ = draw_dialog_text(
            &mut renderer,
            font,
            &[],
            &message[range],
            10,
            18 + line as i32 * 16,
            DialogTextStyle {
                color: 0x4f,
                glyph_limit: usize::MAX,
                mode: DialogTextMode::Normal,
            },
        );
    }
    ColorImage::from_rgba_unmultiplied([MESSAGE_WIDTH, MESSAGE_HEIGHT], renderer.screen())
}

fn message_preview_ranges(
    message: &[u8],
    maximum_width: usize,
    maximum_lines: usize,
) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = 0;
    let mut index = 0;
    let mut width = 0;
    while index < message.len() && ranges.len() < maximum_lines {
        let token = dialog_token(message, index);
        if matches!(
            token.kind,
            DialogTokenKind::Terminate(_) | DialogTokenKind::LineBreak
        ) {
            if index > start {
                ranges.push(start..index);
            }
            break;
        }
        if width + token.width > maximum_width && index > start {
            ranges.push(start..index);
            start = index;
            width = 0;
            if ranges.len() == maximum_lines {
                break;
            }
        }
        width += token.width;
        index += token.bytes;
    }
    if ranges.len() < maximum_lines && index > start {
        ranges.push(start..index);
    }
    ranges
}

#[cfg(test)]
mod tests {
    use pal_assets::palette::{Palette, PaletteColor};
    use pal_assets::script::ScriptEntry;

    use super::*;

    #[test]
    fn parses_hex_entry_addresses() {
        assert_eq!(parse_entry("@10AF"), Some(0x10af));
        assert_eq!(parse_entry("0x20"), Some(0x20));
        assert_eq!(parse_entry(""), None);
        assert_eq!(parse_entry("xyz"), None);
    }

    #[test]
    fn wraps_big5_messages_without_splitting_glyphs() {
        let message = [0xa4, 0x40, 0xa4, 0x41, 0xa4, 0x42];
        assert_eq!(message_preview_ranges(&message, 32, 2), vec![0..4, 4..6]);
    }

    #[test]
    fn decodes_message_text_and_explains_control_flow() {
        assert_eq!(
            decode_dialog_message(b"hello-$12world~05ignored").as_deref(),
            Some("hello [delay 12] world [wait 5]")
        );

        let record = ScriptRecordInspection {
            entry: 0x3995,
            instruction: ScriptEntry {
                opcode: ScriptOpcode::PrintMessage.raw(),
                operands: [1092, 0, 0],
            },
            opcode: Some(ScriptOpcode::PrintMessage),
            flow: ScriptControlFlow::Next,
        };
        assert_eq!(
            opcode_explanation(ScriptOpcode::PrintMessage, record, Some(911)),
            "Display message #1092 from M.MSG."
        );
        assert_eq!(
            flow_explanation(record),
            "Continue at @3996 after this instruction."
        );
        assert_eq!(
            humanize_opcode_name("ShowFbpWithSprite"),
            "Show fbp with sprite"
        );
        assert_eq!(object_selector_label(u16::MAX, Some(911)), "#911 (current)");
    }

    #[test]
    fn exposes_object_script_and_flow_navigation_targets() {
        let object_trigger = ScriptRecordInspection {
            entry: 0x13ab,
            instruction: ScriptEntry {
                opcode: ScriptOpcode::SetObjectTriggerScript.raw(),
                operands: [20, 0x1370, 0],
            },
            opcode: Some(ScriptOpcode::SetObjectTriggerScript),
            flow: ScriptControlFlow::Next,
        };
        assert_eq!(
            instruction_script_targets(object_trigger),
            vec![("Trigger", 0x1370)]
        );

        let call = ScriptRecordInspection {
            entry: 10,
            instruction: ScriptEntry {
                opcode: ScriptOpcode::Call.raw(),
                operands: [20, 7, 0],
            },
            opcode: Some(ScriptOpcode::Call),
            flow: ScriptControlFlow::Call {
                target: 20,
                return_entry: 11,
            },
        };
        assert_eq!(instruction_script_targets(call), vec![("Call", 20)]);
    }

    #[test]
    fn message_preview_renders_original_big5_glyphs() {
        let mut font_data = vec![0; 0x682 + 30];
        font_data[0x682] = 0x80;
        let font = BitmapFont::parse(&[0xa4, 0x40], &font_data).unwrap();
        let mut palette = Palette::default();
        palette.colors[0x4f] = PaletteColor {
            r: 63,
            g: 63,
            b: 63,
        };

        let image = message_preview_image(&palette, &font, &[0xa4, 0x40], 7, 10);

        assert_eq!(image[(10, 18)], Color32::from_rgb(252, 252, 252));
    }

    #[test]
    fn script_rows_distinguish_branch_call_and_invalid_flow() {
        let branch = ScriptRecordInspection {
            entry: 10,
            instruction: ScriptEntry {
                opcode: ScriptOpcode::JumpIfSceneEquals.raw(),
                operands: [1, 5, 0],
            },
            opcode: Some(ScriptOpcode::JumpIfSceneEquals),
            flow: ScriptControlFlow::Jump {
                target: 5,
                conditional: true,
            },
        };
        assert_eq!(format_flow(branch).0, "BRANCH -> @0005 LOOP");

        let call = ScriptRecordInspection {
            flow: ScriptControlFlow::Call {
                target: 20,
                return_entry: 11,
            },
            ..branch
        };
        assert_eq!(format_flow(call).0, "CALL -> @0014");

        let invalid = ScriptRecordInspection {
            opcode: None,
            flow: ScriptControlFlow::Unknown,
            ..branch
        };
        assert_eq!(format_flow(invalid).0, "INVALID");
    }
}
