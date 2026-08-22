//! Docked original-save browser and supported-field editor UI.

use std::path::PathBuf;

use pal_assets::objects::ObjectLayout;
use pal_assets::player_roles::PLAYER_ROLE_COUNT;

use super::super::content::{ContentKind, ContentSelection};
use super::super::save::{SaveChangeKind, SaveSlotState};
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SavePage {
    Overview,
    Inventory,
    Roles,
    Advanced,
}

impl SavePage {
    const ALL: [Self; 4] = [Self::Overview, Self::Inventory, Self::Roles, Self::Advanced];

    fn title(self) -> &'static str {
        match self {
            Self::Overview => "概览",
            Self::Inventory => "背包",
            Self::Roles => "角色",
            Self::Advanced => "高级",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SaveUiAction {
    Open(PathBuf),
    Reload,
    Save(PathBuf),
}

impl<L> EditorTabViewer<'_, L>
where
    L: FnMut(u16, &pal_core::role::RoleSprites) -> Option<super::super::super::LoadedScene>,
{
    pub(super) fn saves_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Original saves");
            if ui.button("Refresh").clicked() {
                self.model.save_editor.refresh_slots();
            }
        });
        ui.separator();

        let current_path = self.model.save_editor.current_path().map(PathBuf::from);
        let slots = self.model.save_editor.slots.clone();
        for slot in slots {
            let selected = current_path.as_ref() == slot.path.as_ref();
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("SLOT {}", slot.number)).strong());
                match &slot.state {
                    SaveSlotState::Empty => {
                        ui.label(RichText::new("EMPTY").monospace().color(MUTED));
                    }
                    SaveSlotState::Invalid => {
                        ui.label(RichText::new("INVALID").monospace().color(ERROR));
                        if let Some(path) = &slot.path {
                            ui.label(path.display().to_string());
                        }
                    }
                    SaveSlotState::Valid {
                        layout,
                        saved_times,
                        scene_number,
                    } => {
                        let label = format!(
                            "{}  SAVE {}  SCENE {}",
                            layout_name(*layout),
                            saved_times,
                            scene_number
                        );
                        if ui.selectable_label(selected, label).clicked() {
                            if let Some(path) = slot.path {
                                *self.save_action = Some(SaveUiAction::Open(path));
                            }
                        }
                    }
                }
            });
        }

        ui.separator();
        ui.label(RichText::new("FILE").strong().color(MUTED));
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(self.save_open_path)
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace),
            );
            if ui.button("Open").clicked() && !self.save_open_path.trim().is_empty() {
                *self.save_action = Some(SaveUiAction::Open(PathBuf::from(
                    self.save_open_path.trim(),
                )));
            }
        });

        let Some(document) = self.model.save_editor.document.as_ref() else {
            ui.add_space(10.0);
            ui.label(RichText::new("No valid .rpg save is open").color(MUTED));
            return;
        };
        let document_path = document.path.clone();
        let is_dirty = document.is_dirty();
        ui.add_space(10.0);
        ui.label(RichText::new("CURRENT").strong().color(MUTED));
        ui.monospace(document_path.display().to_string());
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(if is_dirty { "MODIFIED" } else { "PARSED" })
                    .strong()
                    .color(if is_dirty { TRIGGER } else { ACCENT }),
            );
            if ui.button("Reload").clicked() {
                *self.save_action = Some(SaveUiAction::Reload);
            }
            if ui
                .add_enabled(is_dirty, egui::Button::new("Save"))
                .clicked()
            {
                *self.save_action = Some(SaveUiAction::Save(document_path.clone()));
            }
        });

        ui.label(RichText::new("SAVE AS").strong().color(MUTED));
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(self.save_as_path)
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace),
            );
            if ui.button("Save as").clicked() && !self.save_as_path.trim().is_empty() {
                *self.save_action =
                    Some(SaveUiAction::Save(PathBuf::from(self.save_as_path.trim())));
            }
        });

        let changes = self.model.save_editor.changes();
        if !changes.is_empty() {
            ui.separator();
            ui.label(RichText::new(format!("CHANGES {}", changes.len())).strong());
            for change in changes {
                let color = match change.kind {
                    SaveChangeKind::Cash => ACCENT,
                    SaveChangeKind::Inventory => TRIGGER,
                    SaveChangeKind::Role => AUTO,
                    SaveChangeKind::Magic => Color32::from_rgb(244, 108, 220),
                };
                ui.label(RichText::new(change.text).color(color));
            }
        }
    }

    pub(super) fn save_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for page in SavePage::ALL {
                if ui
                    .selectable_label(*self.save_page == page, page.title())
                    .clicked()
                {
                    *self.save_page = page;
                }
            }
            ui.separator();
            if let Some(document) = self.model.save_editor.document.as_ref() {
                ui.monospace(document.path.file_name().map_or_else(
                    || document.path.display().to_string(),
                    |name| name.to_string_lossy().into_owned(),
                ));
                if document.is_dirty() {
                    ui.label(RichText::new("MODIFIED").strong().color(TRIGGER));
                }
            }
        });
        ui.separator();
        if self.model.save_editor.document.is_none() {
            ui.centered_and_justified(|ui| ui.label("Open a valid .rpg save in the Saves panel"));
            return;
        }
        match *self.save_page {
            SavePage::Overview => self.save_overview_ui(ui),
            SavePage::Inventory => self.save_inventory_ui(ui),
            SavePage::Roles => self.save_roles_ui(ui),
            SavePage::Advanced => self.save_advanced_ui(ui),
        }
    }

    fn save_overview_ui(&mut self, ui: &mut egui::Ui) {
        let save = self
            .model
            .save_editor
            .document
            .as_ref()
            .expect("save presence checked")
            .save
            .clone();
        ui.heading("运行状态");
        egui::Grid::new("save-overview-fields")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                field(ui, "格式", layout_name(save.layout));
                field(ui, "保存次数", save.saved_times);
                field(
                    ui,
                    "场景",
                    format!("{} / 0x{:04X}", save.scene_number, save.scene_number),
                );
                field(
                    ui,
                    "Viewport",
                    format!("{}, {}", save.viewport_x, save.viewport_y),
                );
                field(ui, "队伍人数", save.party_member_count());
                field(ui, "额外跟随者", save.follower_count);
                field(
                    ui,
                    "调色板",
                    if save.night_palette {
                        "夜间"
                    } else {
                        "日间"
                    },
                );
                field(ui, "方向", save.party_direction);
                field(ui, "音乐", save.music_number);
                field(ui, "战斗音乐", save.battle_music_number);
                field(ui, "战场", save.battlefield_number);
            });

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("金钱").strong());
            let mut cash = save.cash;
            if ui
                .add(
                    egui::DragValue::new(&mut cash)
                        .range(0..=u32::MAX)
                        .speed(100),
                )
                .changed()
            {
                self.model.save_editor.set_cash(cash);
            }
        });

        ui.separator();
        ui.heading("队伍（只读）");
        TableBuilder::new(ui)
            .id_salt("save-party")
            .striped(true)
            .column(Column::exact(48.0))
            .column(Column::remainder().at_least(100.0))
            .column(Column::remainder().at_least(100.0))
            .column(Column::exact(70.0))
            .header(22.0, |mut header| {
                for label in ["SLOT", "ROLE", "POSITION", "FRAME"] {
                    header.col(|ui| {
                        ui.strong(label);
                    });
                }
            })
            .body(|body| {
                let count = save.party_member_count() + usize::from(save.follower_count);
                body.rows(22.0, count, |mut row| {
                    let index = row.index();
                    let member = save.party[index];
                    row.col(|ui| {
                        ui.monospace((index + 1).to_string());
                    });
                    row.col(|ui| {
                        ui.label(role_label(self.model, &save, usize::from(member.role_id)));
                    });
                    row.col(|ui| {
                        ui.monospace(format!("{}, {}", member.x, member.y));
                    });
                    row.col(|ui| {
                        ui.monospace(member.frame.to_string());
                    });
                });
            });
    }

    fn save_inventory_ui(&mut self, ui: &mut egui::Ui) {
        let save = self
            .model
            .save_editor
            .document
            .as_ref()
            .expect("save presence checked")
            .save
            .clone();
        let entries = save
            .inventory
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, entry)| entry.item_id != 0 && entry.amount != 0)
            .collect::<Vec<_>>();
        ui.horizontal(|ui| {
            ui.heading("背包");
            ui.label(RichText::new(format!("{} / 256", entries.len())).color(MUTED));
        });

        let mut amount_change = None;
        let mut remove = None;
        TableBuilder::new(ui)
            .id_salt("save-inventory")
            .striped(true)
            .column(Column::exact(64.0))
            .column(Column::remainder().at_least(120.0))
            .column(Column::exact(88.0))
            .column(Column::exact(72.0))
            .column(Column::exact(34.0))
            .header(22.0, |mut header| {
                for label in ["OBJECT", "NAME", "AMOUNT", "IN USE", ""] {
                    header.col(|ui| {
                        ui.strong(label);
                    });
                }
            })
            .body(|body| {
                body.rows(24.0, entries.len(), |mut row| {
                    let (index, entry) = entries[row.index()];
                    row.col(|ui| {
                        ui.monospace(format!("{:04X}", entry.item_id));
                    });
                    row.col(|ui| {
                        ui.label(content_name(self.model, ContentKind::Item, entry.item_id));
                    });
                    row.col(|ui| {
                        let mut amount = entry.amount;
                        if ui
                            .add(egui::DragValue::new(&mut amount).range(1..=99).speed(1))
                            .changed()
                        {
                            amount_change = Some((index, amount));
                        }
                    });
                    row.col(|ui| {
                        ui.monospace(entry.amount_in_use.to_string());
                    });
                    row.col(|ui| {
                        if ui.button("×").on_hover_text("删除物品").clicked() {
                            remove = Some(index);
                        }
                    });
                });
            });
        if let Some((index, amount)) = amount_change {
            if let Err(error) = self.model.save_editor.set_inventory_amount(index, amount) {
                self.model.save_editor.status = Some(error);
            }
        }
        if let Some(index) = remove {
            if let Err(error) = self.model.save_editor.remove_inventory(index) {
                self.model.save_editor.status = Some(error);
            }
        }

        ui.separator();
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("save-add-item")
                .width(260.0)
                .selected_text(content_name(
                    self.model,
                    ContentKind::Item,
                    *self.save_add_item,
                ))
                .show_ui(ui, |ui| {
                    for entry in self.model.content_catalog.entries(ContentKind::Item) {
                        ui.selectable_value(
                            self.save_add_item,
                            entry.selection.object_id,
                            format!(
                                "{:04X}  {}",
                                entry.selection.object_id, entry.name.simplified
                            ),
                        );
                    }
                });
            ui.label("数量");
            ui.add(egui::DragValue::new(self.save_add_amount).range(1..=99));
            if ui.button("Add").clicked() {
                if let Err(error) = self
                    .model
                    .save_editor
                    .add_inventory(*self.save_add_item, *self.save_add_amount)
                {
                    self.model.save_editor.status = Some(error);
                }
            }
        });
    }

    fn save_roles_ui(&mut self, ui: &mut egui::Ui) {
        let save = self
            .model
            .save_editor
            .document
            .as_ref()
            .expect("save presence checked")
            .save
            .clone();
        ui.horizontal_wrapped(|ui| {
            for role_index in 0..PLAYER_ROLE_COUNT {
                if ui
                    .selectable_label(
                        *self.save_role == role_index,
                        role_label(self.model, &save, role_index),
                    )
                    .clicked()
                {
                    *self.save_role = role_index;
                }
            }
        });
        let role_index = (*self.save_role).min(PLAYER_ROLE_COUNT - 1);
        let Some(mut role) = save.player_roles.role(role_index).cloned() else {
            return;
        };
        let mut experience = save.experience[0][role_index].experience;
        ui.separator();
        ui.heading(role_label(self.model, &save, role_index));

        let mut role_changed = false;
        egui::Grid::new("save-role-primary")
            .num_columns(4)
            .striped(true)
            .show(ui, |ui| {
                role_changed |= edit_u16(ui, "等级", &mut role.level, 1..=99);
                let experience_response = edit_u16(ui, "经验", &mut experience, 0..=u16::MAX);
                role_changed |= edit_u16(ui, "HP", &mut role.hp, 0..=u16::MAX);
                role_changed |= edit_u16(ui, "HP 上限", &mut role.max_hp, 0..=u16::MAX);
                role_changed |= edit_u16(ui, "MP", &mut role.mp, 0..=u16::MAX);
                role_changed |= edit_u16(ui, "MP 上限", &mut role.max_mp, 0..=u16::MAX);
                role_changed |= edit_u16(ui, "武术", &mut role.attack_strength, 0..=u16::MAX);
                role_changed |= edit_u16(ui, "灵力", &mut role.magic_strength, 0..=u16::MAX);
                role_changed |= edit_u16(ui, "防御", &mut role.defense, 0..=u16::MAX);
                role_changed |= edit_u16(ui, "身法", &mut role.dexterity, 0..=u16::MAX);
                role_changed |= edit_u16(ui, "吉运", &mut role.flee_rate, 0..=u16::MAX);
                role_changed |= edit_u16(ui, "毒抗", &mut role.poison_resistance, 0..=u16::MAX);
                ui.label("全体攻击");
                role_changed |= ui.checkbox(&mut role.attack_all, "").changed();
                ui.end_row();
                if experience_response {
                    if let Err(error) = self
                        .model
                        .save_editor
                        .set_role_experience(role_index, experience)
                    {
                        self.model.save_editor.status = Some(error);
                    }
                }
            });
        ui.label(RichText::new("五灵抗性").strong().color(MUTED));
        ui.horizontal(|ui| {
            for (label, resistance) in ["风", "雷", "水", "火", "土"]
                .into_iter()
                .zip(&mut role.elemental_resistance)
            {
                ui.label(label);
                role_changed |= ui
                    .add(egui::DragValue::new(resistance).range(0..=u16::MAX))
                    .changed();
            }
        });
        if role_changed {
            if let Err(error) = self
                .model
                .save_editor
                .replace_editable_role(role_index, &role)
            {
                self.model.save_editor.status = Some(error);
            }
        }

        ui.separator();
        ui.heading("装备（只读）");
        egui::Grid::new("save-role-equipment")
            .num_columns(3)
            .striped(true)
            .show(ui, |ui| {
                for (slot, item_id) in role.equipment.iter().copied().enumerate() {
                    ui.monospace(format!("{}", slot + 1));
                    ui.monospace(format!("{item_id:04X}"));
                    ui.label(if item_id == 0 {
                        "-".to_owned()
                    } else {
                        content_name(self.model, ContentKind::Item, item_id)
                    });
                    ui.end_row();
                }
            });

        ui.separator();
        ui.heading("仙术");
        let magics = role
            .magic
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, magic)| *magic != 0)
            .collect::<Vec<_>>();
        let mut remove_magic = None;
        for (slot, magic_id) in magics {
            ui.horizontal(|ui| {
                ui.monospace(format!("{magic_id:04X}"));
                ui.label(content_name(self.model, ContentKind::Magic, magic_id));
                if ui.button("×").on_hover_text("遗忘仙术").clicked() {
                    remove_magic = Some(slot);
                }
            });
        }
        if let Some(slot) = remove_magic {
            if let Err(error) = self.model.save_editor.remove_magic(role_index, slot) {
                self.model.save_editor.status = Some(error);
            }
        }
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("save-add-magic")
                .width(260.0)
                .selected_text(content_name(
                    self.model,
                    ContentKind::Magic,
                    *self.save_add_magic,
                ))
                .show_ui(ui, |ui| {
                    for entry in self.model.content_catalog.entries(ContentKind::Magic) {
                        ui.selectable_value(
                            self.save_add_magic,
                            entry.selection.object_id,
                            format!(
                                "{:04X}  {}",
                                entry.selection.object_id, entry.name.simplified
                            ),
                        );
                    }
                });
            if ui.button("Learn").clicked() {
                if let Err(error) = self
                    .model
                    .save_editor
                    .add_magic(role_index, *self.save_add_magic)
                {
                    self.model.save_editor.status = Some(error);
                }
            }
        });
    }

    fn save_advanced_ui(&mut self, ui: &mut egui::Ui) {
        let save = self
            .model
            .save_editor
            .document
            .as_ref()
            .expect("save presence checked")
            .save
            .clone();
        ui.label(RichText::new("READ ONLY").strong().color(TRIGGER));
        ui.collapsing("运行头", |ui| {
            egui::Grid::new("save-raw-header")
                .num_columns(2)
                .striped(true)
                .show(ui, |ui| {
                    field(ui, "saved_times", hex_dec(save.saved_times));
                    field(ui, "viewport_x", signed_hex_dec(save.viewport_x));
                    field(ui, "viewport_y", signed_hex_dec(save.viewport_y));
                    field(ui, "party_member_index", hex_dec(save.party_member_index));
                    field(ui, "scene_number", hex_dec(save.scene_number));
                    field(ui, "party_direction", hex_dec(save.party_direction));
                    field(ui, "music_number", hex_dec(save.music_number));
                    field(ui, "battle_music_number", hex_dec(save.battle_music_number));
                    field(ui, "battlefield_number", hex_dec(save.battlefield_number));
                    field(ui, "screen_wave", hex_dec(save.screen_wave));
                    field(ui, "battle_speed", hex_dec(save.battle_speed));
                    field(ui, "collect_value", hex_dec(save.collect_value));
                    field(ui, "layer", hex_dec(save.layer));
                    field(ui, "chase_range", hex_dec(save.chase_range));
                    field(
                        ui,
                        "chase_speed_change_cycles",
                        hex_dec(save.chase_speed_change_cycles),
                    );
                    field(ui, "follower_count", hex_dec(save.follower_count));
                    for (index, value) in save.reserved.into_iter().enumerate() {
                        field(ui, &format!("reserved[{index}]"), hex_dec(value));
                    }
                    field(ui, "cash", format!("{} / 0x{:08X}", save.cash, save.cash));
                });
        });
        ui.collapsing("场景表（300）", |ui| {
            TableBuilder::new(ui)
                .id_salt("save-raw-scenes")
                .max_scroll_height(360.0)
                .striped(true)
                .column(Column::exact(54.0))
                .column(Column::remainder())
                .column(Column::remainder())
                .column(Column::remainder())
                .column(Column::remainder())
                .header(22.0, |mut header| {
                    for label in ["SCENE", "MAP", "ENTER", "TELEPORT", "OBJECT START"] {
                        header.col(|ui| {
                            ui.strong(label);
                        });
                    }
                })
                .body(|body| {
                    body.rows(22.0, save.scenes.len(), |mut row| {
                        let index = row.index();
                        let scene = save.scenes[index];
                        for value in [
                            format!("{:03}", index + 1),
                            hex_dec(scene.map_num),
                            hex_dec(scene.script_on_enter),
                            hex_dec(scene.script_on_teleport),
                            hex_dec(scene.event_object_index),
                        ] {
                            row.col(|ui| {
                                ui.monospace(value);
                            });
                        }
                    });
                });
        });
        ui.collapsing(
            format!("全局对象表（{}）", save.objects.len()),
            |ui| {
                TableBuilder::new(ui)
                    .id_salt("save-raw-objects")
                    .max_scroll_height(360.0)
                    .striped(true)
                    .column(Column::exact(64.0))
                    .columns(Column::remainder(), 7)
                    .header(22.0, |mut header| {
                        header.col(|ui| {
                            ui.strong("OBJECT");
                        });
                        for word in 0..7 {
                            header.col(|ui| {
                                ui.strong(format!("W{word}"));
                            });
                        }
                    })
                    .body(|body| {
                        body.rows(22.0, save.objects.len(), |mut row| {
                            let object = save
                                .objects
                                .get(
                                    u16::try_from(row.index()).expect("save object index fits u16"),
                                )
                                .expect("save object row exists");
                            row.col(|ui| {
                                ui.monospace(format!("{:04X}", object.id));
                            });
                            for word in object.data {
                                row.col(|ui| {
                                    ui.monospace(format!("{word:04X}"));
                                });
                            }
                        });
                    });
            },
        );
        ui.collapsing(
            format!("事件对象表（{}）", save.event_objects.len()),
            |ui| {
                TableBuilder::new(ui)
                    .id_salt("save-raw-events")
                    .max_scroll_height(420.0)
                    .striped(true)
                    .column(Column::exact(60.0))
                    .column(Column::remainder())
                    .column(Column::remainder())
                    .column(Column::remainder())
                    .column(Column::remainder())
                    .column(Column::remainder())
                    .column(Column::remainder())
                    .column(Column::remainder())
                    .header(22.0, |mut header| {
                        for label in [
                            "OBJECT", "X", "Y", "STATE", "TRIGGER", "AUTO", "SPRITE", "FRAME",
                        ] {
                            header.col(|ui| {
                                ui.strong(label);
                            });
                        }
                    })
                    .body(|body| {
                        body.rows(22.0, save.event_objects.len(), |mut row| {
                            let index = row.index();
                            let event = save.event_objects[index];
                            for value in [
                                format!("{:04X}", index + 1),
                                hex_dec(event.x),
                                hex_dec(event.y),
                                signed_hex_dec(event.state),
                                hex_dec(event.trigger_script),
                                hex_dec(event.auto_script),
                                hex_dec(event.sprite_num),
                                hex_dec(event.current_frame),
                            ] {
                                row.col(|ui| {
                                    ui.monospace(value);
                                });
                            }
                        });
                    });
            },
        );
    }
}

fn edit_u16(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut u16,
    range: std::ops::RangeInclusive<u16>,
) -> bool {
    ui.label(label);
    let changed = ui.add(egui::DragValue::new(value).range(range)).changed();
    ui.end_row();
    changed
}

fn role_label<L>(
    model: &SceneEditorApp<L>,
    save: &pal_assets::save::OriginalSave,
    role: usize,
) -> String {
    save.player_roles
        .role(role)
        .and_then(|role| model.content_catalog.word(role.name_word_id))
        .map_or_else(|| format!("角色 #{role}"), |name| name.simplified.clone())
}

fn content_name<L>(model: &SceneEditorApp<L>, kind: ContentKind, object_id: u16) -> String {
    model
        .content_catalog
        .label(ContentSelection { kind, object_id })
        .map_or_else(
            || format!("对象 #{object_id:04X}"),
            |label| label.name.simplified.clone(),
        )
}

fn layout_name(layout: ObjectLayout) -> &'static str {
    match layout {
        ObjectLayout::Dos => "DOS",
        ObjectLayout::Win95 => "Win95",
    }
}

fn hex_dec(value: u16) -> String {
    format!("{value} / 0x{value:04X}")
}

fn signed_hex_dec(value: i16) -> String {
    format!("{value} / 0x{:04X}", value as u16)
}
