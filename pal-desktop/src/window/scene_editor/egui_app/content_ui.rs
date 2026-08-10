//! Database, preview, inspector, and reference UI for global content.

use super::super::content::{format_content_id, ContentKind, ContentReference, ContentSelection};
use super::*;

#[derive(Debug, Clone, Copy)]
enum ContentAction {
    Select(ContentSelection),
    NavigateScript(u16),
}

impl<L> EditorTabViewer<'_, L>
where
    L: FnMut(u16, &pal_core::role::RoleSprites) -> Option<super::super::super::LoadedScene>,
{
    pub(super) fn database_ui(&mut self, ui: &mut egui::Ui) {
        let mut switch_kind = None;
        ui.horizontal_wrapped(|ui| {
            for kind in ContentKind::ALL {
                let count = self.model.content_catalog.entries(kind).len();
                if ui
                    .selectable_label(
                        *self.content_kind == kind,
                        format!("{} {count}", kind.title()),
                    )
                    .clicked()
                {
                    switch_kind = Some(kind);
                }
            }
        });
        if let Some(kind) = switch_kind {
            *self.content_kind = kind;
            if self.model.selected_content.map(|selection| selection.kind) != Some(kind) {
                if let Some(first) = self.model.content_catalog.entries(kind).first() {
                    self.model.select_content(first.selection);
                }
            }
        }

        ui.add(
            egui::TextEdit::singleline(self.content_filter)
                .hint_text("名称、繁体原文、十进制或十六进制 ID…")
                .desired_width(f32::INFINITY),
        );
        let entries = self
            .model
            .content_catalog
            .entries(*self.content_kind)
            .iter()
            .filter(|entry| entry.matches(self.content_filter))
            .cloned()
            .collect::<Vec<_>>();
        let selected = self.model.selected_content;
        let mut select = None;
        TableBuilder::new(ui)
            .id_salt(("content-database", *self.content_kind))
            .striped(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::exact(62.0))
            .column(Column::remainder().at_least(90.0))
            .column(Column::remainder().at_least(90.0))
            .header(22.0, |mut header| {
                for label in ["OBJECT", "NAME", "SUMMARY"] {
                    header.col(|ui| {
                        ui.strong(label);
                    });
                }
            })
            .body(|body| {
                body.rows(22.0, entries.len(), |mut row| {
                    let entry = &entries[row.index()];
                    row.set_selected(selected == Some(entry.selection));
                    row.col(|ui| {
                        if ui
                            .selectable_label(
                                selected == Some(entry.selection),
                                RichText::new(format_content_id(entry.selection.object_id))
                                    .monospace()
                                    .color(content_kind_color(entry.selection.kind)),
                            )
                            .clicked()
                        {
                            select = Some(entry.selection);
                        }
                    });
                    row.col(|ui| {
                        if ui
                            .selectable_label(
                                false,
                                RichText::new(&entry.name.simplified).color(TEXT),
                            )
                            .on_hover_text(format!(
                                "原文：{}\n十进制对象 ID：{}",
                                entry.name.traditional, entry.selection.object_id
                            ))
                            .clicked()
                        {
                            select = Some(entry.selection);
                        }
                    });
                    row.col(|ui| {
                        ui.label(
                            RichText::new(content_summary(self.model, entry.selection))
                                .monospace()
                                .color(MUTED),
                        );
                    });
                });
            });
        if let Some(selection) = select {
            self.model.select_content(selection);
        }
    }

    pub(super) fn content_preview_ui(&mut self, ui: &mut egui::Ui) {
        let Some(selection) = self.model.selected_content else {
            ui.centered_and_justified(|ui| ui.label("Select an item, magic, or enemy"));
            return;
        };
        let Some(label) = self.model.content_catalog.label(selection) else {
            return;
        };
        ui.horizontal(|ui| {
            ui.heading(&label.name.simplified);
            ui.label(
                RichText::new(format_content_id(selection.object_id))
                    .monospace()
                    .color(content_kind_color(selection.kind)),
            );
            ui.label(RichText::new(content_resource_summary(self.model, selection)).color(MUTED));
        });
        if label.name.traditional != label.name.simplified {
            ui.label(RichText::new(format!("原文：{}", label.name.traditional)).color(MUTED));
        }
        let Some(texture) = self.content_texture else {
            ui.centered_and_justified(|ui| ui.label("This resource has no decodable preview"));
            return;
        };
        let available = ui.available_size();
        let source = texture.size_vec2();
        let scale = (available.x / source.x)
            .min(available.y / source.y)
            .max(0.1);
        ui.centered_and_justified(|ui| {
            ui.add(
                egui::Image::new((texture.id(), source * scale))
                    .texture_options(TextureOptions::NEAREST),
            );
        });
    }

    pub(super) fn content_inspector_ui(&mut self, ui: &mut egui::Ui, selection: ContentSelection) {
        let Some(label) = self.model.content_catalog.label(selection).cloned() else {
            return;
        };
        let Some(object) = self.model.global_objects.get(selection.object_id).copied() else {
            return;
        };
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(&label.name.simplified)
                        .size(20.0)
                        .strong()
                        .color(content_kind_color(selection.kind)),
                );
                ui.label(
                    RichText::new(format!(
                        "{} OBJECT {} ({})",
                        selection.kind.singular_title(),
                        format_content_id(selection.object_id),
                        selection.object_id
                    ))
                    .monospace()
                    .color(MUTED),
                );
                if label.name.traditional != label.name.simplified {
                    ui.label(
                        RichText::new(format!("原文：{}", label.name.traditional)).color(MUTED),
                    );
                }
            });
            if let Some(texture) = self.content_texture {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                    let source = texture.size_vec2();
                    let scale = (128.0 / source.x).min(80.0 / source.y);
                    ui.add(
                        egui::Image::new((texture.id(), source * scale))
                            .texture_options(TextureOptions::NEAREST),
                    );
                });
            }
        });

        let action = match selection.kind {
            ContentKind::Item => self.item_inspector_ui(ui, selection, object),
            ContentKind::Magic => self.magic_inspector_ui(ui, selection, object),
            ContentKind::Enemy => self.enemy_inspector_ui(ui, selection, object),
        };
        ui.collapsing("Raw global object", |ui| {
            egui::Grid::new("content-raw-object")
                .num_columns(2)
                .striped(true)
                .show(ui, |ui| {
                    for (index, value) in object.data.into_iter().enumerate() {
                        field(
                            ui,
                            &format!("Word {index}"),
                            format!("{value:04X}  ({value})"),
                        );
                    }
                });
        });
        if let Some(action) = action {
            self.apply_content_action(action);
        }
    }

    pub(super) fn content_references_ui(&mut self, ui: &mut egui::Ui, selection: ContentSelection) {
        let Some(label) = self.model.content_catalog.label(selection).cloned() else {
            return;
        };
        ui.heading(format!(
            "References to {} {}",
            label.name.simplified,
            format_content_id(selection.object_id)
        ));
        let references = self
            .model
            .content_references
            .references_to(selection)
            .to_vec();
        let mut action = None;
        if references.is_empty() {
            ui.label(RichText::new("No indexed references").color(MUTED));
        }
        for reference in references {
            match reference {
                ContentReference::Script { entry, opcode } => {
                    let name = ScriptOpcode::from_raw(opcode).map_or("UNKNOWN", ScriptOpcode::name);
                    if ui
                        .button(
                            RichText::new(format!("SCRIPT @{entry:04X}  {name}"))
                                .monospace()
                                .color(AUTO),
                        )
                        .clicked()
                    {
                        action = Some(ContentAction::NavigateScript(entry));
                    }
                }
                ContentReference::BattleScript { entry, team } => {
                    if ui
                        .button(
                            RichText::new(format!("SCRIPT @{entry:04X} STARTS TEAM {team}"))
                                .monospace()
                                .color(TRIGGER),
                        )
                        .clicked()
                    {
                        action = Some(ContentAction::NavigateScript(entry));
                    }
                }
                ContentReference::Store { store, slot } => {
                    ui.label(format!("Store {store}, slot {}", slot + 1));
                }
                ContentReference::PlayerEquipment { role, slot } => {
                    ui.label(format!(
                        "{} initial equipment slot {}",
                        role_name(self.model, role),
                        slot + 1
                    ));
                }
                ContentReference::PlayerMagic {
                    role,
                    slot,
                    cooperative,
                } => {
                    if cooperative {
                        ui.label(format!("{} cooperative magic", role_name(self.model, role)));
                    } else {
                        ui.label(format!(
                            "{} initial magic slot {}",
                            role_name(self.model, role),
                            slot + 1
                        ));
                    }
                }
                ContentReference::LevelUpMagic { role, level } => {
                    ui.label(format!(
                        "{} learns at level {level}",
                        role_name(self.model, role)
                    ));
                }
                ContentReference::EnemyTeam { team, slot } => {
                    ui.label(format!("Enemy team {team}, slot {}", slot + 1));
                }
                ContentReference::EnemyMagic { enemy_object } => {
                    action = action.or_else(|| {
                        content_reference_button(ui, self.model, enemy_object, "Enemy magic")
                    });
                }
                ContentReference::EnemyAttackItem { enemy_object } => {
                    action = action.or_else(|| {
                        content_reference_button(ui, self.model, enemy_object, "Enemy attack item")
                    });
                }
                ContentReference::EnemyStealItem { enemy_object } => {
                    action = action.or_else(|| {
                        content_reference_button(ui, self.model, enemy_object, "Enemy steal item")
                    });
                }
            }
        }
        if let Some(action) = action {
            self.apply_content_action(action);
        }
    }

    fn item_inspector_ui(
        &mut self,
        ui: &mut egui::Ui,
        selection: ContentSelection,
        object: pal_assets::objects::GlobalObject,
    ) -> Option<ContentAction> {
        ui.separator();
        egui::Grid::new("item-fields")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                field(ui, "BALL bitmap", object.item_bitmap());
                field(ui, "Price", object.item_price());
                field(ui, "Flags", format!("0x{:04X}", object.item_flags()));
                field(
                    ui,
                    "Capabilities",
                    item_flag_labels(object.item_flags()).join(", "),
                );
                field(
                    ui,
                    "Usable roles",
                    usable_role_names(self.model, object.item_flags()),
                );
            });
        let action = script_buttons(
            ui,
            [
                ("Use", object.item_use_script()),
                ("Equip", object.item_equip_script()),
                ("Throw", object.item_throw_script()),
            ],
        );
        let descriptions = self.model.content_catalog.descriptions(selection.object_id);
        if !descriptions.is_empty() {
            ui.separator();
            ui.label(RichText::new("说明").strong().color(ACCENT));
            for line in descriptions {
                ui.label(&line.simplified);
            }
            if descriptions
                .iter()
                .any(|line| line.simplified != line.traditional)
            {
                ui.collapsing("繁体原文", |ui| {
                    for line in descriptions {
                        ui.label(&line.traditional);
                    }
                });
            }
        }
        action
    }

    fn magic_inspector_ui(
        &mut self,
        ui: &mut egui::Ui,
        _selection: ContentSelection,
        object: pal_assets::objects::GlobalObject,
    ) -> Option<ContentAction> {
        let definition_id = object.magic_number();
        let definition = self.model.magics.get(definition_id).copied();
        ui.separator();
        egui::Grid::new("magic-fields")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                field(ui, "DATA magic", definition_id);
                field(ui, "Flags", format!("0x{:04X}", object.magic_flags()));
                field(
                    ui,
                    "Availability",
                    magic_flag_labels(object.magic_flags()).join(", "),
                );
                if let Some(magic) = definition {
                    field(ui, "Type", magic_type_name(magic.magic_type));
                    field(ui, "Effect / FIRE", magic.effect);
                    field(ui, "MP cost", magic.mp_cost);
                    field(ui, "Base damage", magic.base_damage as i16);
                    field(ui, "Element", element_name(magic.elemental));
                    field(
                        ui,
                        "Offset",
                        format!("{}, {}", magic.x_offset, magic.y_offset),
                    );
                    field(ui, "Layer / summon", magic.specific);
                    field(ui, "Speed", magic.speed);
                    field(ui, "Fire delay", magic.fire_delay);
                    field(ui, "Extra loops", magic.effect_times);
                    field(ui, "Keep effect", yes_no(magic.keep_effect == u16::MAX));
                    field(
                        ui,
                        "Shake / wave",
                        format!("{} / {}", magic.shake, magic.wave),
                    );
                    field(ui, "Sound", magic.sound);
                }
            });
        script_buttons(
            ui,
            [
                ("Success", object.magic_success_script()),
                ("Use", object.magic_use_script()),
            ],
        )
    }

    fn enemy_inspector_ui(
        &mut self,
        ui: &mut egui::Ui,
        _selection: ContentSelection,
        object: pal_assets::objects::GlobalObject,
    ) -> Option<ContentAction> {
        let enemy_id = object.enemy_id();
        let enemy = self.model.battle_data.enemies.get(enemy_id).copied();
        ui.separator();
        egui::Grid::new("enemy-fields")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                field(ui, "DATA enemy / ABC", enemy_id);
                field(ui, "Sorcery resistance", object.enemy_sorcery_resistance());
                if let Some(enemy) = enemy {
                    field(ui, "Level", enemy.level);
                    field(ui, "HP", enemy.health);
                    field(
                        ui,
                        "Attack / magic",
                        format!("{} / {}", enemy.attack_strength, enemy.magic_strength),
                    );
                    field(
                        ui,
                        "Defense / dexterity",
                        format!("{} / {}", enemy.defense, enemy.dexterity),
                    );
                    field(
                        ui,
                        "Flee / poison resist",
                        format!("{} / {}", enemy.flee_rate, enemy.poison_resistance),
                    );
                    field(ui, "Physical resistance", enemy.physical_resistance);
                    field(
                        ui,
                        "Elemental resistance",
                        format_resistances(enemy.elemental_resistance),
                    );
                    field(
                        ui,
                        "EXP / cash",
                        format!("{} / {}", enemy.experience, enemy.cash),
                    );
                    field(ui, "Dual move", yes_no(enemy.dual_move));
                    field(ui, "Collect value", enemy.collect_value);
                    field(
                        ui,
                        "Frames idle/magic/attack",
                        format!(
                            "{}/{}/{}",
                            enemy.idle_frames, enemy.magic_frames, enemy.attack_frames
                        ),
                    );
                }
            });
        let mut action = script_buttons(
            ui,
            [
                ("Turn start", object.enemy_turn_start_script()),
                ("Battle end", object.enemy_battle_end_script()),
                ("Ready", object.enemy_ready_script()),
            ],
        );
        if let Some(enemy) = enemy {
            ui.separator();
            ui.label(RichText::new("Related content").strong().color(ACCENT));
            for (relationship, object_id) in [
                (format!("Magic ({} / 10)", enemy.magic_rate), enemy.magic),
                (
                    format!("Attack item ({} / 10)", enemy.attack_equivalent_item_rate),
                    enemy.attack_equivalent_item,
                ),
                (
                    format!("Steal item (count {})", enemy.steal_item_count),
                    enemy.steal_item,
                ),
            ] {
                if let Some(selection) = ContentSelection::from_object_id(object_id) {
                    let text = content_link_label(self.model, selection, &relationship);
                    if ui.button(text).clicked() {
                        action = Some(ContentAction::Select(selection));
                    }
                } else {
                    ui.label(format!("{relationship}: -"));
                }
            }
        }
        action
    }

    fn apply_content_action(&mut self, action: ContentAction) {
        match action {
            ContentAction::Select(selection) => self.model.select_content(selection),
            ContentAction::NavigateScript(entry) => self.model.navigate_to_entry(entry),
        }
    }
}

fn script_buttons<const N: usize>(
    ui: &mut egui::Ui,
    scripts: [(&str, u16); N],
) -> Option<ContentAction> {
    let mut action = None;
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Scripts").strong().color(MUTED));
        for (label, entry) in scripts {
            if entry == 0 {
                ui.add_enabled(false, egui::Button::new(format!("{label} -")));
            } else if ui
                .button(
                    RichText::new(format!("{label} @{entry:04X}"))
                        .monospace()
                        .color(AUTO),
                )
                .clicked()
            {
                action = Some(ContentAction::NavigateScript(entry));
            }
        }
    });
    action
}

fn content_reference_button<L>(
    ui: &mut egui::Ui,
    model: &SceneEditorApp<L>,
    enemy_object: u16,
    relationship: &str,
) -> Option<ContentAction>
where
    L: FnMut(u16, &pal_core::role::RoleSprites) -> Option<super::super::super::LoadedScene>,
{
    let selection = ContentSelection::from_object_id(enemy_object)?;
    ui.button(content_link_label(model, selection, relationship))
        .clicked()
        .then_some(ContentAction::Select(selection))
}

fn content_link_label<L>(
    model: &SceneEditorApp<L>,
    selection: ContentSelection,
    relationship: &str,
) -> String {
    let name = model
        .content_catalog
        .label(selection)
        .map_or("?", |label| label.name.simplified.as_str());
    format!(
        "{relationship}: {name} {}",
        format_content_id(selection.object_id)
    )
}

fn content_summary<L>(model: &SceneEditorApp<L>, selection: ContentSelection) -> String {
    let Some(object) = model.global_objects.get(selection.object_id) else {
        return "INVALID".to_owned();
    };
    match selection.kind {
        ContentKind::Item => format!(
            "${}  {}",
            object.item_price(),
            item_flag_abbreviation(object.item_flags())
        ),
        ContentKind::Magic => model.magics.get(object.magic_number()).map_or_else(
            || "INVALID MAGIC".to_owned(),
            |magic| format!("MP {}  DMG {}", magic.mp_cost, magic.base_damage as i16),
        ),
        ContentKind::Enemy => model
            .battle_data
            .enemies
            .get(object.enemy_id())
            .map_or_else(
                || "INVALID ENEMY".to_owned(),
                |enemy| format!("LV {}  HP {}", enemy.level, enemy.health),
            ),
    }
}

fn content_resource_summary<L>(model: &SceneEditorApp<L>, selection: ContentSelection) -> String {
    let Some(object) = model.global_objects.get(selection.object_id) else {
        return "invalid object".to_owned();
    };
    match selection.kind {
        ContentKind::Item => format!("BALL.MKF #{}", object.item_bitmap()),
        ContentKind::Magic => model.magics.get(object.magic_number()).map_or_else(
            || format!("DATA.MKF magic #{}", object.magic_number()),
            |magic| {
                if magic.magic_type == 9 {
                    format!("DATA magic #{} • F.MKF summon", object.magic_number())
                } else {
                    format!(
                        "DATA magic #{} • FIRE.MKF #{}",
                        object.magic_number(),
                        magic.effect
                    )
                }
            },
        ),
        ContentKind::Enemy => format!(
            "DATA enemy #{} • ABC.MKF #{}",
            object.enemy_id(),
            object.enemy_id()
        ),
    }
}

fn content_kind_color(kind: ContentKind) -> Color32 {
    match kind {
        ContentKind::Item => TRIGGER,
        ContentKind::Magic => AUTO,
        ContentKind::Enemy => ERROR,
    }
}

fn item_flag_labels(flags: u16) -> Vec<&'static str> {
    const FLAGS: [(u16, &str); 6] = [
        (1 << 0, "usable"),
        (1 << 1, "equippable"),
        (1 << 2, "throwable"),
        (1 << 3, "consuming"),
        (1 << 4, "all targets"),
        (1 << 5, "sellable"),
    ];
    let labels = FLAGS
        .into_iter()
        .filter_map(|(flag, label)| (flags & flag != 0).then_some(label))
        .collect::<Vec<_>>();
    if labels.is_empty() {
        vec!["none"]
    } else {
        labels
    }
}

fn item_flag_abbreviation(flags: u16) -> String {
    [
        (1 << 0, 'U'),
        (1 << 1, 'E'),
        (1 << 2, 'T'),
        (1 << 3, 'C'),
        (1 << 4, 'A'),
        (1 << 5, 'S'),
    ]
    .into_iter()
    .filter_map(|(flag, label)| (flags & flag != 0).then_some(label))
    .collect()
}

fn magic_flag_labels(flags: u16) -> Vec<&'static str> {
    let labels = [
        (1 << 0, "outside battle"),
        (1 << 1, "in battle"),
        (1 << 3, "targets enemies"),
        (1 << 4, "all targets"),
    ]
    .into_iter()
    .filter_map(|(flag, label)| (flags & flag != 0).then_some(label))
    .collect::<Vec<_>>();
    if labels.is_empty() {
        vec!["none"]
    } else {
        labels
    }
}

fn usable_role_names<L>(model: &SceneEditorApp<L>, flags: u16) -> String {
    let names = model
        .player_roles
        .iter()
        .enumerate()
        .filter(|(role, _)| flags & (1 << (6 + role)) != 0)
        .map(|(role, _)| role_name(model, role as u8))
        .collect::<Vec<_>>();
    if names.is_empty() {
        "-".to_owned()
    } else {
        names.join(", ")
    }
}

fn role_name<L>(model: &SceneEditorApp<L>, role: u8) -> String {
    model
        .player_roles
        .role(usize::from(role))
        .and_then(|role| model.content_catalog.word(role.name_word_id))
        .map_or_else(|| format!("Role {role}"), |name| name.simplified.clone())
}

fn magic_type_name(value: u16) -> &'static str {
    match value {
        0 => "single target",
        1 => "spread all",
        2 => "whole party",
        3 => "battlefield",
        4 => "single ally",
        5 => "all allies",
        8 => "transform",
        9 => "summon",
        _ => "unknown",
    }
}

fn element_name(value: u16) -> &'static str {
    match value {
        0 => "none",
        1 => "wind",
        2 => "thunder",
        3 => "water",
        4 => "fire",
        5 => "earth",
        _ => "unknown",
    }
}

fn format_resistances(values: [u16; 5]) -> String {
    format!(
        "W{} T{} Wa{} F{} E{}",
        values[0], values[1], values[2], values[3], values[4]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_labels_keep_resource_bits_readable() {
        assert_eq!(item_flag_abbreviation(0b11_1111), "UETCAS");
        assert_eq!(
            item_flag_labels((1 << 0) | (1 << 3)),
            ["usable", "consuming"]
        );
        assert_eq!(magic_type_name(9), "summon");
        assert_eq!(element_name(3), "water");
    }
}
