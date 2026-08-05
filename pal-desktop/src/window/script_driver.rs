use pal_assets::text::TextLibrary;
use pal_core::game::{AutoScriptError, GameState};
use pal_core::role::RoleSprites;
use pal_core::scene::TriggerKind;
use pal_core::script::{ScriptCondition, ScriptEvent, ScriptOpcode, ScriptRuntime, ScriptVisual};

use super::dialog::ActiveDialog;
use super::dialog_text::dialog_body_lines;
use super::menu_state::{
    ConfirmationMenu, FieldMenu, InventoryMenu, InventoryMode, ShopMenu, ShopMode,
};
use super::session::SessionState;
use super::LoadedScene;

#[derive(Clone, Copy)]
pub(super) struct ScriptRenderResources<'a> {
    pub(super) text: &'a TextLibrary,
    pub(super) role_sprites: &'a RoleSprites,
}

pub(super) fn advance_script<L>(
    scripts: &mut ScriptRuntime,
    game: &mut GameState,
    dialog: &mut Option<ActiveDialog>,
    resources: ScriptRenderResources<'_>,
    load_scene: &mut L,
    services: &mut SessionState,
    set_title: &mut impl FnMut(&str),
) where
    L: FnMut(u16, Option<u16>, &RoleSprites) -> Option<LoadedScene>,
{
    match scripts.advance() {
        Some(ScriptEvent::Message {
            message_id,
            position,
            font_color,
            face_index,
        }) => {
            let mut next = ActiveDialog::new(
                message_id,
                position,
                font_color,
                face_index,
                services.dialog_delay_ms,
            );
            if let Some(active) = dialog.as_mut() {
                if active.position == position
                    && active.font_color == font_color
                    && active.face_index == face_index
                    && !active.awaiting_input
                {
                    active.message_ids.push(message_id);
                    active.wait_after_reveal =
                        dialog_body_lines(resources.text, active).len() >= (active.page + 1) * 4;
                } else {
                    active.awaiting_input = true;
                    next.wait_after_reveal |= dialog_body_lines(resources.text, &next).len() >= 4;
                    services.pending_dialog = Some(next);
                }
            } else {
                next.wait_after_reveal |= dialog_body_lines(resources.text, &next).len() >= 4;
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
        Some(ScriptEvent::StartBattle(request)) => {
            services.field_menu = None;
            services.inventory_menu = None;
            services.shop_menu = None;
            services.confirmation_menu = None;
            if game.start_battle(request, &services.auto_scripts) {
                services.battle_selected_enemy = game
                    .battle()
                    .and_then(|battle| battle.first_living_enemy())
                    .unwrap_or(0);
                services.battle_command_selected = 0;
                services.battle_pending_throw_item = None;
                services.battle_events.clear();
                services.battle_event_ticks = 0;
                if game.current_battle_music == 0 {
                    services.music.stop();
                } else if !services.music.play(game.current_battle_music, true, 0) {
                    set_title("Rust-PAL [battle music unavailable]");
                    return;
                }
                set_title("Rust-PAL [Battle]");
            } else {
                set_title("Rust-PAL [failed to start battle]");
            }
        }
        Some(ScriptEvent::Teleport { failure_entry }) => {
            let teleport_entry = game.scene_teleport_script(0);
            if teleport_entry == 0 || !scripts.call(teleport_entry, 0xffff) {
                scripts.set_success(false);
                scripts.branch_to(failure_entry);
            }
        }
        Some(ScriptEvent::FadeScene { speed })
            if !services
                .visual
                .queue(ScriptVisual::FadeToCurrentScene { speed }) =>
        {
            set_title("Rust-PAL [visual effect is already active]");
        }
        Some(ScriptEvent::FadeScene { .. }) => {}
        Some(ScriptEvent::Visual(command)) if !services.visual.queue(command) => {
            set_title("Rust-PAL [visual effect is already active]");
        }
        Some(ScriptEvent::Visual(_)) => {}
        Some(ScriptEvent::WaitForKey) => services.waiting_for_key = true,
        Some(ScriptEvent::LoadLastSave) => services.load_last_save_requested = true,
        Some(ScriptEvent::QuitGame) => services.quit_requested = true,
        Some(ScriptEvent::Action(pal_core::script::ScriptAction::ChangeScene { scene_number })) => {
            if scene_number == game.scene_number {
                return;
            }
            let Some(scene) = load_scene(
                scene_number,
                game.scene_map_override(scene_number),
                resources.role_sprites,
            ) else {
                set_title("Rust-PAL [failed to load scene]");
                return;
            };
            game.replace_scene(scene.number, scene.map, scene.objects);
            let enter_script = game.scene_enter_script(scene.enter_script);
            services.pending_enter_script = (enter_script != 0).then_some(enter_script);
            set_title(&format!("Rust-PAL [scene {}]", scene.number));
        }
        Some(ScriptEvent::Action(
            action @ pal_core::script::ScriptAction::SetSceneMap {
                scene_number,
                map_number: _,
            },
        )) => {
            let target_scene = scene_number.unwrap_or(game.scene_number);
            if !game.apply_script_action(action) {
                set_title("Rust-PAL [invalid scene map]");
                return;
            }
            if target_scene == game.scene_number {
                let Some(scene) = load_scene(
                    target_scene,
                    game.scene_map_override(target_scene),
                    resources.role_sprites,
                ) else {
                    set_title("Rust-PAL [failed to reload scene map]");
                    return;
                };
                game.replace_map(scene.map);
            }
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
            | pal_core::script::ScriptAction::RevivePlayer { .. }
            | pal_core::script::ScriptAction::SetPlayerStatus { .. }),
        )) => {
            let succeeded = game.apply_script_action(action);
            scripts.set_success(succeeded);
        }
        Some(ScriptEvent::Action(
            action @ pal_core::script::ScriptAction::SetEnemyStatus { resisted_entry, .. },
        )) if !game.apply_script_action(action) => {
            scripts.branch_to(resisted_entry);
        }
        Some(ScriptEvent::Action(
            action @ pal_core::script::ScriptAction::FleeBattle { failure_entry },
        )) if !game.apply_script_action(action) => {
            scripts.branch_to(failure_entry);
        }
        Some(ScriptEvent::Action(
            pal_core::script::ScriptAction::SetEnemyStatus { .. }
            | pal_core::script::ScriptAction::FleeBattle { .. },
        )) => {}
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
                ScriptCondition::PlayerLacksPoison {
                    role_id,
                    poison_id,
                    target_entry,
                } => (!game.player_has_poison(role_id, poison_id), target_entry),
                ScriptCondition::EnemyLacksPoison {
                    enemy_index,
                    poison_id,
                    target_entry,
                } => (!game.enemy_has_poison(enemy_index, poison_id), target_entry),
                ScriptCondition::PlayerNotPoisoned {
                    role_id,
                    target_entry,
                } => (
                    game.player_poisons(role_id)
                        .is_none_or(|poisons| poisons.iter().all(|poison| poison.object_id == 0)),
                    target_entry,
                ),
                ScriptCondition::EnemyHpAbove {
                    enemy_index,
                    percentage,
                    target_entry,
                } => (game.enemy_hp_above(enemy_index, percentage), target_entry),
                ScriptCondition::EnemyNotFirstKind {
                    enemy_index,
                    target_entry,
                } => (game.enemy_not_first_kind(enemy_index), target_entry),
                ScriptCondition::EnemyTurn { target_entry } => (game.is_enemy_turn(), target_entry),
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
            if trigger.kind == TriggerKind::Battle {
                if !game.finish_battle_script(next_entry, succeeded) {
                    set_title("Rust-PAL [battle script completion failed]");
                }
                return;
            } else if trigger.kind == TriggerKind::Item {
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
            } else if trigger.kind == TriggerKind::Equip {
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
            } else if trigger.kind == TriggerKind::Magic {
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
            } else if let Some(object) = game
                .scene_objects
                .iter_mut()
                .find(|object| object.id == trigger.object_id)
            {
                object.trigger_script = next_entry;
            }
            if let Some(entry) = services.pending_enter_script.take() {
                scripts.start(pal_core::scene::TriggerRequest {
                    object_id: 0xffff,
                    script_entry: entry,
                    kind: TriggerKind::Touch,
                });
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
            if trigger.kind == TriggerKind::Battle {
                game.finish_battle_script(trigger.script_entry, false);
            }
            resume_script_menu_after_error(game, services, trigger.kind);
            set_title(&format!(
                "Rust-PAL [unsupported script {entry} {}]",
                opcode_label(opcode)
            ));
        }
        Some(ScriptEvent::InvalidEntry { trigger, entry }) => {
            if trigger.kind == TriggerKind::Battle {
                game.finish_battle_script(trigger.script_entry, false);
            }
            resume_script_menu_after_error(game, services, trigger.kind);
            set_title(&format!("Rust-PAL [invalid script entry {entry}]"));
        }
        Some(ScriptEvent::InstructionLimit { trigger, entry }) => {
            if trigger.kind == TriggerKind::Battle {
                game.finish_battle_script(trigger.script_entry, false);
            }
            resume_script_menu_after_error(game, services, trigger.kind);
            set_title(&format!("Rust-PAL [script loop at {entry}]"));
        }
        None => {}
    }
}

pub(super) fn auto_script_error_title(error: AutoScriptError) -> String {
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

pub(super) fn opcode_label(raw: u16) -> String {
    ScriptOpcode::from_raw(raw).map_or_else(
        || format!("opcode {raw:04X}"),
        |opcode| format!("{} ({raw:04X})", opcode.mnemonic()),
    )
}

fn resume_inventory_after_item_error(game: &GameState, services: &mut SessionState) {
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

pub(super) fn resume_script_menu_after_error(
    game: &GameState,
    services: &mut SessionState,
    kind: TriggerKind,
) {
    match kind {
        TriggerKind::Item => resume_inventory_after_item_error(game, services),
        TriggerKind::Equip => {
            if let Some(equip) = services.equip.take() {
                services.inventory_menu = Some(InventoryMenu {
                    selected: equip
                        .inventory_selected
                        .min(game.equippable_inventory().len().saturating_sub(1)),
                    mode: InventoryMode::EquipItems,
                });
            }
        }
        TriggerKind::Magic => {
            if let Some(magic) = services.magic.take() {
                services.field_menu = Some(FieldMenu::MagicList {
                    caster: magic.caster_selected,
                    selected: services.magic_selected,
                });
            }
        }
        TriggerKind::Search | TriggerKind::Touch | TriggerKind::Battle => {}
    }
}

pub(super) fn update_trigger_world(
    game: &mut GameState,
    services: &mut SessionState,
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
