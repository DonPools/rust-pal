//! Scene-editor panel rendering and panel-local interaction.

use std::ops::Range;

use pal_core::script::{
    inspect_script_records, ScriptControlFlow, ScriptOpcode, ScriptRecordInspection,
    ScriptReferenceSource,
};

use super::super::dialog_text::{dialog_token, DialogTokenKind};
use super::super::draw::{draw_debug_text, draw_line, fill_rect, stroke_rect};
use super::super::text_render::{draw_dialog_text, DialogTextMode, DialogTextStyle};
use super::app::{script_object_ids, SceneEditorApp, SelectedScript};
use super::navigation::{format_reference, scene_object_references};
use super::{
    CODE_FIRST_LINE, CODE_LAST_LINE, CODE_ROWS, LINE_HEIGHT, OBJECT_FIRST_LINE, OBJECT_LAST_LINE,
    OBJECT_ROWS, PANEL_BACKGROUND, PANEL_BORDER, PANEL_WIDTH, PANEL_X, REFERENCE_FIRST_LINE,
    REFERENCE_LAST_LINE, REFERENCE_ROWS, SCENE_EDITOR_HEIGHT, SCENE_EDITOR_WIDTH,
};

const TEXT: [u8; 4] = [224, 232, 228, 255];
const MUTED: [u8; 4] = [132, 148, 156, 255];
const ACCENT: [u8; 4] = [64, 224, 144, 255];
const TRIGGER: [u8; 4] = [255, 168, 48, 255];
const AUTO: [u8; 4] = [48, 208, 255, 255];
const SELECTED: [u8; 4] = [42, 60, 68, 255];
const ERROR: [u8; 4] = [255, 96, 96, 255];

impl<L> SceneEditorApp<L>
where
    L: FnMut(u16, &pal_core::role::RoleSprites) -> Option<super::super::LoadedScene>,
{
    pub(super) fn draw_panel(&mut self) {
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
        let status = self.scene_input_status().unwrap_or_else(|| {
            self.status
                .as_deref()
                .unwrap_or("READ ONLY  CLICK FLOW TO NAVIGATE")
                .to_owned()
        });
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

        self.draw_object_list();
        self.separator(17);
        self.draw_selection_summary();
        self.separator(21);
        self.draw_navigation_bar();
        self.separator(24);
        self.draw_code_rows();
        self.separator(37);
        self.draw_instruction_details();
        self.panel_line(44, "B BACK F FORWARD R ROOT O OBJECT", MUTED);
    }

    fn draw_object_list(&mut self) {
        let script_ids = script_object_ids(&self.scene.objects);
        self.object_scroll = self
            .object_scroll
            .min(script_ids.len().saturating_sub(OBJECT_ROWS));
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
    }

    fn draw_selection_summary(&mut self) {
        if let Some(object) = self.selected_object().cloned() {
            self.panel_line(
                18,
                &format!(
                    "OBJECT #{} POS {},{} STATE {} LAYER {}",
                    object.id, object.world_x, object.world_y, object.state, object.layer
                ),
                ACCENT,
            );
            self.panel_line(
                19,
                &format!(
                    "MODE {} SPRITE {} VIS {} BLOCK {}",
                    object.trigger_mode,
                    object
                        .sprite_index
                        .map_or("-".to_owned(), |value| value.to_string()),
                    yes_no(object.is_visible()),
                    yes_no(object.is_blocker())
                ),
                TEXT,
            );
            self.selectable_half_line(
                20,
                self.selected_script == Some(SelectedScript::ObjectTrigger),
                self.selected_script == Some(SelectedScript::ObjectAuto),
                &format!("TRIGGER @{:04X}", object.trigger_script),
                &format!("AUTO @{:04X}", object.auto_script),
            );
        } else {
            let source = match self.selected_script {
                Some(SelectedScript::SceneEnter) => "SCENE ENTER SCRIPT",
                Some(SelectedScript::SceneTeleport) => "SCENE TELEPORT SCRIPT",
                _ => "NO OBJECT SELECTED",
            };
            self.panel_line(18, source, ACCENT);
            self.panel_line(19, "CLICK A MARKER OR OBJECT ROW", MUTED);
            self.panel_line(20, "SCENE SCRIPTS STAY ABOVE", MUTED);
        }
    }

    fn draw_navigation_bar(&mut self) {
        self.panel_line(
            22,
            &format!(
                "BACK {}  FORWARD {}  ROOT",
                yes_no(self.navigation.can_go_back()),
                yes_no(self.navigation.can_go_forward())
            ),
            TEXT,
        );
        let breadcrumb = self.navigation.current().map_or_else(
            || "ROOT -  CURRENT -".to_owned(),
            |location| {
                if location.root == location.entry {
                    format!("ROOT @{:04X}", location.root)
                } else {
                    format!(
                        "ROOT @{:04X}  CURRENT @{:04X}",
                        location.root, location.entry
                    )
                }
            },
        );
        self.panel_line(23, &breadcrumb, ACCENT);
    }

    fn draw_code_rows(&mut self) {
        let Some(location) = self.navigation.current() else {
            self.code_scroll = 0;
            self.panel_line(25, "SCRIPT DISABLED  ENTRY @0000", MUTED);
            for line in CODE_FIRST_LINE..CODE_LAST_LINE {
                self.panel_line(line, "-", MUTED);
            }
            return;
        };
        let max_offset = self
            .scripts
            .len()
            .saturating_sub(usize::from(location.entry).saturating_add(1));
        self.code_scroll = self.code_scroll.min(max_offset);
        self.panel_line(
            25,
            &format!("CODE @{:04X}  OFFSET {}", location.entry, self.code_scroll),
            ACCENT,
        );
        let records =
            inspect_script_records(&self.scripts, location.entry, self.code_scroll, CODE_ROWS);
        for row in 0..CODE_ROWS {
            let line = CODE_FIRST_LINE + row;
            let Some(record) = records.get(row).copied() else {
                self.panel_line(line, "END OF SCRIPT TABLE", MUTED);
                continue;
            };
            let selected = self.navigation.selected_instruction() == Some(record.entry);
            self.selectable_line(
                line,
                selected,
                &format_script_record(record),
                flow_color(record),
            );
        }
    }

    fn draw_instruction_details(&mut self) {
        let Some(record) = self.selected_record() else {
            self.panel_line(38, "SELECTED INSTRUCTION -", MUTED);
            self.panel_line(39, "INCOMING 0", MUTED);
            for line in REFERENCE_FIRST_LINE..REFERENCE_LAST_LINE {
                self.panel_line(line, "-", MUTED);
            }
            self.panel_line(43, "OBJECT REFS -", MUTED);
            return;
        };
        let mnemonic = record.opcode.map_or("UNKNOWN", ScriptOpcode::name);
        self.panel_line(
            38,
            &format!("SELECT @{:04X} {}", record.entry, mnemonic),
            ACCENT,
        );
        let references = self.script_references.references_to(record.entry).to_vec();
        self.reference_scroll = self
            .reference_scroll
            .min(references.len().saturating_sub(REFERENCE_ROWS));
        self.panel_line(
            39,
            &format!(
                "INCOMING {}  OFFSET {}",
                references.len(),
                self.reference_scroll
            ),
            TEXT,
        );
        for row in 0..REFERENCE_ROWS {
            let line = REFERENCE_FIRST_LINE + row;
            let Some(source) = references.get(self.reference_scroll + row).copied() else {
                self.panel_line(line, "-", MUTED);
                continue;
            };
            self.panel_line(line, &format_reference(source), reference_color(source));
        }
        let object_ids = scene_object_references(record, self.selected_object);
        let text = if object_ids.is_empty() {
            "OBJECT REFS -".to_owned()
        } else {
            format!(
                "OBJECT REFS {}  CLICK TO LOCATE",
                object_ids
                    .iter()
                    .take(2)
                    .map(|id| format!("#{id}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        };
        self.panel_line(43, &text, if object_ids.is_empty() { MUTED } else { AUTO });
    }

    pub(super) fn draw_message_preview(&mut self) {
        let Some(record) = self
            .selected_record()
            .filter(|record| record.opcode == Some(ScriptOpcode::PrintMessage))
        else {
            return;
        };
        let message_id = record.instruction.operands[0];
        let Some(message) = self.text.message(usize::from(message_id)) else {
            return;
        };
        draw_message_preview_content(
            &mut self.renderer,
            &self.font,
            message,
            message_id,
            record.entry,
        );
    }

    pub(super) fn handle_panel_click(&mut self, cursor: (i32, i32)) {
        let local_x = cursor.0 - PANEL_X;
        let line = usize::try_from(cursor.1 / LINE_HEIGHT).unwrap_or(usize::MAX);
        match line {
            1 if local_x < 42 => self.switch_scene_by(-1),
            1 if local_x < 92 => self.switch_scene_by(1),
            1 if local_x < 145 => self.set_zoom(
                self.zoom.saturating_sub(1),
                (
                    super::CANVAS_WIDTH as i32 / 2,
                    SCENE_EDITOR_HEIGHT as i32 / 2,
                ),
            ),
            1 if local_x < 180 => self.set_zoom(
                self.zoom.saturating_add(1),
                (
                    super::CANVAS_WIDTH as i32 / 2,
                    SCENE_EDITOR_HEIGHT as i32 / 2,
                ),
            ),
            1 => {}
            5 => {
                self.selected_object = None;
                self.selected_script = Some(SelectedScript::SceneEnter);
                self.reset_script_navigation();
            }
            6 => {
                self.selected_object = None;
                self.selected_script = Some(SelectedScript::SceneTeleport);
                self.reset_script_navigation();
            }
            OBJECT_FIRST_LINE..OBJECT_LAST_LINE => {
                let ids = script_object_ids(&self.scene.objects);
                let index = self.object_scroll + line - OBJECT_FIRST_LINE;
                if let Some(&id) = ids.get(index) {
                    self.select_object(id, true);
                }
            }
            20 if self.selected_object.is_some() => {
                self.selected_script = Some(if local_x < PANEL_WIDTH / 2 {
                    SelectedScript::ObjectTrigger
                } else {
                    SelectedScript::ObjectAuto
                });
                self.reset_script_navigation();
            }
            22 if local_x < 65 => self.navigate_back(),
            22 if local_x < 155 => self.navigate_forward(),
            22 => self.navigate_root(),
            CODE_FIRST_LINE..CODE_LAST_LINE => {
                if let Some(location) = self.navigation.current() {
                    let row = line - CODE_FIRST_LINE;
                    if let Some(record) = inspect_script_records(
                        &self.scripts,
                        location.entry,
                        self.code_scroll,
                        CODE_ROWS,
                    )
                    .get(row)
                    .copied()
                    {
                        self.select_instruction(record);
                    }
                }
            }
            REFERENCE_FIRST_LINE..REFERENCE_LAST_LINE => {
                let Some(entry) = self.navigation.selected_instruction() else {
                    return;
                };
                let index = self.reference_scroll + line - REFERENCE_FIRST_LINE;
                if let Some(source) = self
                    .script_references
                    .references_to(entry)
                    .get(index)
                    .copied()
                {
                    self.follow_reference(source);
                }
            }
            43 => {
                let references = self.selected_object_references();
                let index = usize::from(local_x >= PANEL_WIDTH / 2);
                if let Some(object_id) = references.get(index).or(references.first()).copied() {
                    self.locate_object_reference(object_id);
                }
            }
            _ => {}
        }
    }

    pub(super) fn mouse_wheel(&mut self, amount: i32) {
        if amount == 0 {
            return;
        }
        let cursor = self.cursor();
        if cursor.0 < super::CANVAS_WIDTH as i32 {
            let next = if amount > 0 {
                self.zoom.saturating_add(1)
            } else {
                self.zoom.saturating_sub(1)
            };
            self.set_zoom(next, cursor);
            return;
        }
        let line = usize::try_from(cursor.1 / LINE_HEIGHT).unwrap_or(usize::MAX);
        if (OBJECT_FIRST_LINE..OBJECT_LAST_LINE).contains(&line) {
            let maximum = script_object_ids(&self.scene.objects)
                .len()
                .saturating_sub(OBJECT_ROWS);
            self.object_scroll = scroll_offset(self.object_scroll, amount, maximum);
        } else if (CODE_FIRST_LINE..CODE_LAST_LINE).contains(&line) {
            let maximum = self.navigation.current().map_or(0, |location| {
                self.scripts
                    .len()
                    .saturating_sub(usize::from(location.entry).saturating_add(1))
            });
            self.code_scroll = scroll_offset(self.code_scroll, amount, maximum);
        } else if (REFERENCE_FIRST_LINE..REFERENCE_LAST_LINE).contains(&line) {
            let maximum = self.navigation.selected_instruction().map_or(0, |entry| {
                self.script_references
                    .references_to(entry)
                    .len()
                    .saturating_sub(REFERENCE_ROWS)
            });
            self.reference_scroll = scroll_offset(self.reference_scroll, amount, maximum);
        }
        self.mark_dirty();
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
}

fn draw_message_preview_content(
    renderer: &mut crate::renderer::Renderer,
    font: &pal_assets::text::BitmapFont,
    message: &[u8],
    message_id: u16,
    entry: u16,
) {
    let ranges = message_preview_ranges(message, 448, 2);
    let top = SCENE_EDITOR_HEIGHT as i32 - 52;
    fill_rect(
        renderer,
        4,
        top,
        super::CANVAS_WIDTH as i32 - 8,
        48,
        [8, 12, 16, 238],
    );
    stroke_rect(
        renderer,
        4,
        top,
        super::CANVAS_WIDTH as i32 - 8,
        48,
        PANEL_BORDER,
    );
    draw_debug_text(
        renderer,
        10,
        top + 4,
        &format!("MESSAGE {} FROM @{:04X}", message_id, entry),
        ACCENT,
    );
    for (line, range) in ranges.into_iter().enumerate() {
        let _ = draw_dialog_text(
            renderer,
            font,
            &[],
            &message[range],
            10,
            top + 14 + line as i32 * 16,
            DialogTextStyle {
                color: 0x4f,
                glyph_limit: usize::MAX,
                mode: DialogTextMode::Normal,
            },
        );
    }
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

fn format_script_record(record: ScriptRecordInspection) -> String {
    let mnemonic = record.opcode.map_or("UNKNOWN", ScriptOpcode::name);
    let mnemonic = truncate_ascii(mnemonic, 12);
    let flow = match record.flow {
        ScriptControlFlow::Next => ".",
        ScriptControlFlow::Stop => "STOP",
        ScriptControlFlow::Jump {
            conditional: true, ..
        } => "BRANCH",
        ScriptControlFlow::Jump { .. } => "JUMP",
        ScriptControlFlow::Call { .. } => "CALL",
        ScriptControlFlow::Random { .. } => "RANDOM",
        ScriptControlFlow::Unknown => "INVALID",
    };
    let target = match record.flow {
        ScriptControlFlow::Jump { target, .. } | ScriptControlFlow::Call { target, .. } => {
            format!(
                " TO@{target:04X}{}",
                if target <= record.entry { " LOOP" } else { "" }
            )
        }
        _ => String::new(),
    };
    format!(
        "@{:04X} {:<12} {:04X} {:04X} {:04X} {}{}",
        record.entry,
        mnemonic,
        record.instruction.operands[0],
        record.instruction.operands[1],
        record.instruction.operands[2],
        flow,
        target
    )
}

fn flow_color(record: ScriptRecordInspection) -> [u8; 4] {
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

fn reference_color(source: ScriptReferenceSource) -> [u8; 4] {
    match source {
        ScriptReferenceSource::SceneEnter { .. }
        | ScriptReferenceSource::SceneTeleport { .. }
        | ScriptReferenceSource::ObjectTrigger { .. } => TRIGGER,
        ScriptReferenceSource::ObjectAuto { .. } => AUTO,
        ScriptReferenceSource::Instruction { .. } => TEXT,
    }
}

fn object_list_color(object: &pal_core::scene::SceneObject) -> [u8; 4] {
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

fn truncate_ascii(value: &str, maximum: usize) -> String {
    value.chars().take(maximum).collect()
}

#[cfg(test)]
mod tests {
    use pal_assets::palette::{Palette, PaletteColor};
    use pal_assets::script::ScriptEntry;
    use pal_assets::text::BitmapFont;

    use super::*;

    #[test]
    fn wraps_big5_messages_without_splitting_glyphs() {
        let message = [0xa4, 0x40, 0xa4, 0x41, 0xa4, 0x42];
        assert_eq!(message_preview_ranges(&message, 32, 2), vec![0..4, 4..6]);
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
        let mut renderer = crate::renderer::Renderer::new(
            palette,
            super::super::SCENE_EDITOR_WIDTH as usize,
            super::super::SCENE_EDITOR_HEIGHT as usize,
        );

        draw_message_preview_content(&mut renderer, &font, &[0xa4, 0x40], 7, 10);

        let y = (SCENE_EDITOR_HEIGHT as usize - 52) + 14;
        let index = (y * renderer.width + 10) * 4;
        assert_eq!(&renderer.screen()[index..index + 4], &[252, 252, 252, 255]);
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
        assert!(format_script_record(branch).contains("BRANCH TO@0005"));

        let call = ScriptRecordInspection {
            flow: ScriptControlFlow::Call {
                target: 20,
                return_entry: 11,
            },
            ..branch
        };
        assert!(format_script_record(call).contains("CALL TO@0014"));

        let invalid = ScriptRecordInspection {
            opcode: None,
            flow: ScriptControlFlow::Unknown,
            ..branch
        };
        assert!(format_script_record(invalid).contains("INVALID"));
    }
}
