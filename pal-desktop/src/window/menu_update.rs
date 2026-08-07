use std::path::Path;

use pal_assets::text::TextLibrary;
use pal_core::game::{GameInput, GameState};
use pal_core::role::{Direction, RoleSprites};
use pal_core::script::{ScriptRuntime, ScriptVisual};

use super::dialog::ActiveDialog;
use super::menu_state::{
    update_wrapping_selection, ActiveMenu, EquipSession, FieldMenu, InventoryMenu, InventoryMode,
    ItemUseSession, MagicSession, SaveSlotMode, ShopMode, SystemAudioKind,
};
use super::original_save::{
    next_saved_times, original_save_slots, save_original_game, SaveOriginalGameError,
};
use super::script_driver::{advance_script, ScriptRenderResources};
use super::session::DesktopSession;
use super::LoadedScene;

pub(super) struct MenuUpdateContext<'a, L, S> {
    pub(super) input: GameInput,
    pub(super) scripts: &'a mut ScriptRuntime,
    pub(super) game: &'a mut GameState,
    pub(super) dialog: &'a mut Option<ActiveDialog>,
    pub(super) text: &'a TextLibrary,
    pub(super) role_sprites: &'a RoleSprites,
    pub(super) load_scene: &'a mut L,
    pub(super) services: &'a mut DesktopSession,
    pub(super) original_save_dir: &'a Path,
    pub(super) set_title: &'a mut S,
}

impl<L, S> MenuUpdateContext<'_, L, S>
where
    L: FnMut(u16, Option<u16>, &RoleSprites) -> Option<LoadedScene>,
    S: FnMut(&str),
{
    fn advance_script(&mut self) {
        advance_script(
            self.scripts,
            self.game,
            self.dialog,
            ScriptRenderResources {
                text: self.text,
                role_sprites: self.role_sprites,
            },
            self.load_scene,
            self.services,
            self.set_title,
        );
    }

    fn sync_music(&mut self) {
        if let Some(music_id) = self.game.current_music {
            self.services.audio.music.play(music_id, true, 0);
        } else {
            self.services.audio.music.stop();
        }
    }
}

pub(super) fn update_active_menu<L, S>(context: &mut MenuUpdateContext<'_, L, S>) -> Option<bool>
where
    L: FnMut(u16, Option<u16>, &RoleSprites) -> Option<LoadedScene>,
    S: FnMut(&str),
{
    let changed =
        context.input.confirm || context.input.cancel || context.input.direction_pressed.is_some();
    match context.services.menus.active_menu.as_ref()? {
        ActiveMenu::Confirmation(_) => update_confirmation_menu(context),
        ActiveMenu::Field(_) => update_field_menu(context),
        ActiveMenu::Shop(_) => update_shop_menu(context),
        ActiveMenu::Inventory(_) => update_inventory_menu(context),
    }
    Some(changed)
}

fn update_confirmation_menu<L, S>(context: &mut MenuUpdateContext<'_, L, S>)
where
    L: FnMut(u16, Option<u16>, &RoleSprites) -> Option<LoadedScene>,
    S: FnMut(&str),
{
    let mut menu = context
        .services
        .take_confirmation_menu()
        .expect("confirmation menu was checked above");
    if matches!(
        context.input.direction_pressed,
        Some(Direction::West | Direction::North)
    ) {
        menu.selected_yes = false;
    } else if matches!(
        context.input.direction_pressed,
        Some(Direction::East | Direction::South)
    ) {
        menu.selected_yes = true;
    }
    if context.input.confirm || context.input.cancel {
        if context.input.cancel || !menu.selected_yes {
            context.scripts.branch_to(menu.no_entry);
        }
        context.advance_script();
    } else {
        context.services.set_confirmation_menu(menu);
    }
}

fn update_field_menu<L, S>(context: &mut MenuUpdateContext<'_, L, S>)
where
    L: FnMut(u16, Option<u16>, &RoleSprites) -> Option<LoadedScene>,
    S: FnMut(&str),
{
    let mut menu = context
        .services
        .take_field_menu()
        .expect("field menu was checked above");
    let mut keep_menu = true;
    match &mut menu {
        FieldMenu::Main { selected } => {
            update_wrapping_selection(selected, context.input.direction_pressed, 4);
            context.services.menus.main_menu_selected = *selected;
            if context.input.cancel {
                keep_menu = false;
                (context.set_title)("Rust-PAL");
            } else if context.input.confirm {
                match *selected {
                    0 => {
                        menu = FieldMenu::Status { selected: 0 };
                        (context.set_title)("Rust-PAL [Status]");
                    }
                    1 => {
                        let selected = context
                            .services
                            .menus
                            .magic_caster_selected
                            .min(context.game.party.members().len().saturating_sub(1));
                        menu = FieldMenu::MagicCaster { selected };
                        (context.set_title)("Rust-PAL [Magic]");
                    }
                    2 => {
                        menu = FieldMenu::InventoryAction {
                            selected: context.services.menus.inventory_action_selected,
                        };
                        (context.set_title)("Rust-PAL [Inventory]");
                    }
                    3 => {
                        menu = FieldMenu::System {
                            selected: context.services.menus.system_selected,
                        };
                        (context.set_title)("Rust-PAL [System]");
                    }
                    _ => unreachable!(),
                }
            }
        }
        FieldMenu::InventoryAction { selected } => {
            update_wrapping_selection(selected, context.input.direction_pressed, 2);
            context.services.menus.inventory_action_selected = *selected;
            if context.input.cancel {
                keep_menu = false;
                (context.set_title)("Rust-PAL");
            } else if context.input.confirm {
                keep_menu = false;
                context.services.set_inventory_menu(InventoryMenu {
                    selected: context.services.menus.inventory_selected,
                    mode: if *selected == 0 {
                        InventoryMode::EquipItems
                    } else {
                        InventoryMode::Items
                    },
                });
                (context.set_title)(if *selected == 0 {
                    "Rust-PAL [Equip item]"
                } else {
                    "Rust-PAL [Use item]"
                });
            }
        }
        FieldMenu::Status { selected } => {
            update_wrapping_selection(
                selected,
                context.input.direction_pressed,
                context.game.party.members().len(),
            );
            if context.input.cancel {
                keep_menu = false;
                (context.set_title)("Rust-PAL");
            }
        }
        FieldMenu::MagicCaster { selected } => {
            update_wrapping_selection(
                selected,
                context.input.direction_pressed,
                context.game.party.members().len(),
            );
            context.services.menus.magic_caster_selected = *selected;
            if context.input.cancel {
                keep_menu = false;
                (context.set_title)("Rust-PAL");
            } else if context.input.confirm {
                let role_id = context.game.party.members()[*selected].role_id;
                if context
                    .game
                    .player_role(role_id)
                    .is_some_and(|role| role.hp > 0)
                {
                    menu = FieldMenu::MagicList {
                        caster: *selected,
                        selected: context.services.menus.magic_selected,
                    };
                    (context.set_title)("Rust-PAL [Magic list]");
                }
            }
        }
        FieldMenu::MagicList { caster, selected } => {
            let role_id = context.game.party.members()[*caster].role_id;
            let magics = context.game.field_magics(role_id);
            update_wrapping_selection(selected, context.input.direction_pressed, magics.len());
            context.services.menus.magic_selected = *selected;
            if context.input.cancel {
                keep_menu = false;
                (context.set_title)("Rust-PAL");
            } else if context.input.confirm {
                if let Some(magic) = magics.get(*selected).filter(|magic| magic.enabled) {
                    if magic.apply_to_all {
                        if let Some(request) =
                            context
                                .game
                                .magic_request(role_id, magic.magic_id, None, false)
                        {
                            keep_menu = false;
                            if context.scripts.start(request) {
                                context.services.menus.magic = Some(MagicSession {
                                    caster_selected: *caster,
                                    magic_id: magic.magic_id,
                                    target_selected: None,
                                    success_phase: false,
                                });
                                context.advance_script();
                            }
                        }
                    } else {
                        menu = FieldMenu::MagicTarget {
                            caster: *caster,
                            magic_id: magic.magic_id,
                            selected: context.services.menus.magic_target_selected,
                        };
                        (context.set_title)("Rust-PAL [Magic target]");
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
                context.input.direction_pressed,
                context.game.party.members().len(),
            );
            context.services.menus.magic_target_selected = *selected;
            if context.input.cancel {
                menu = FieldMenu::MagicList {
                    caster: *caster,
                    selected: context.services.menus.magic_selected,
                };
                (context.set_title)("Rust-PAL [Magic list]");
            } else if context.input.confirm {
                let caster_role = context.game.party.members()[*caster].role_id;
                let target_role = context.game.party.members()[*selected].role_id;
                if let Some(request) =
                    context
                        .game
                        .magic_request(caster_role, *magic_id, Some(target_role), false)
                {
                    keep_menu = false;
                    if context.scripts.start(request) {
                        context.services.menus.magic = Some(MagicSession {
                            caster_selected: *caster,
                            magic_id: *magic_id,
                            target_selected: Some(*selected),
                            success_phase: false,
                        });
                        context.advance_script();
                    }
                }
            }
        }
        FieldMenu::System { selected } => {
            update_wrapping_selection(selected, context.input.direction_pressed, 5);
            context.services.menus.system_selected = *selected;
            if context.input.cancel {
                menu = FieldMenu::Main {
                    selected: context.services.menus.main_menu_selected,
                };
                (context.set_title)("Rust-PAL [Menu]");
            } else if context.input.confirm {
                match *selected {
                    0 => {
                        menu = FieldMenu::SaveSlots {
                            mode: SaveSlotMode::Save,
                            selected: context
                                .services
                                .persistence
                                .current_save_slot
                                .map_or(0, |slot| usize::from(slot.saturating_sub(1))),
                            slots: original_save_slots(context.original_save_dir),
                        };
                        (context.set_title)("Rust-PAL [Save slot]");
                    }
                    1 => {
                        menu = FieldMenu::SaveSlots {
                            mode: SaveSlotMode::Load,
                            selected: context
                                .services
                                .persistence
                                .current_save_slot
                                .map_or(0, |slot| usize::from(slot.saturating_sub(1))),
                            slots: original_save_slots(context.original_save_dir),
                        };
                        (context.set_title)("Rust-PAL [Load slot]");
                    }
                    2 => {
                        menu = FieldMenu::SystemAudio {
                            parent_selected: *selected,
                            kind: SystemAudioKind::Music,
                            selected_enabled: context.services.audio.music.enabled(),
                        };
                        (context.set_title)("Rust-PAL [Music switch]");
                    }
                    3 => {
                        menu = FieldMenu::SystemAudio {
                            parent_selected: *selected,
                            kind: SystemAudioKind::Sound,
                            selected_enabled: context.services.audio.sound_effects.enabled(),
                        };
                        (context.set_title)("Rust-PAL [Sound switch]");
                    }
                    4 => {
                        menu = FieldMenu::SystemQuit {
                            selected_yes: false,
                        };
                        (context.set_title)("Rust-PAL [Quit confirm]");
                    }
                    _ => unreachable!(),
                }
            }
        }
        FieldMenu::SystemAudio {
            parent_selected,
            kind,
            selected_enabled,
        } => {
            update_binary_selection(selected_enabled, context.input.direction_pressed);
            if context.input.cancel {
                menu = FieldMenu::System {
                    selected: *parent_selected,
                };
                (context.set_title)("Rust-PAL [System]");
            } else if context.input.confirm {
                match kind {
                    SystemAudioKind::Music => {
                        context.services.audio.music.set_enabled(*selected_enabled);
                        if *selected_enabled {
                            context.sync_music();
                        }
                    }
                    SystemAudioKind::Sound => context
                        .services
                        .audio
                        .sound_effects
                        .set_enabled(*selected_enabled),
                }
                menu = FieldMenu::System {
                    selected: *parent_selected,
                };
                (context.set_title)("Rust-PAL [System]");
            }
        }
        FieldMenu::SystemQuit { selected_yes } => {
            update_binary_selection(selected_yes, context.input.direction_pressed);
            if context.input.cancel || (context.input.confirm && !*selected_yes) {
                menu = FieldMenu::System { selected: 4 };
                (context.set_title)("Rust-PAL [System]");
            } else if context.input.confirm {
                if context
                    .services
                    .visual
                    .queue(ScriptVisual::FadeOut { speed: 2 })
                {
                    context.services.audio.music.stop();
                    context.services.persistence.quit_requested = true;
                    keep_menu = false;
                    (context.set_title)("Rust-PAL [Quitting]");
                } else {
                    (context.set_title)("Rust-PAL [quit transition unavailable]");
                }
            }
        }
        FieldMenu::SaveSlots {
            mode,
            selected,
            slots,
        } => {
            update_wrapping_selection(selected, context.input.direction_pressed, slots.len());
            if context.input.cancel {
                menu = FieldMenu::System {
                    selected: match mode {
                        SaveSlotMode::Save => 0,
                        SaveSlotMode::Load => 1,
                    },
                };
                (context.set_title)("Rust-PAL [System]");
            } else if context.input.confirm {
                let slot = slots[*selected].slot;
                match mode {
                    SaveSlotMode::Save => {
                        let saved_times =
                            next_saved_times(&original_save_slots(context.original_save_dir));
                        let night_palette = context.services.visual.night_palette();
                        let screen_wave = context.services.visual.screen_wave();
                        match save_original_game(
                            context.original_save_dir,
                            slot,
                            context.game,
                            saved_times,
                            night_palette,
                            screen_wave,
                        ) {
                            Ok(()) => {
                                context.services.persistence.current_save_slot = Some(slot);
                                keep_menu = false;
                                (context.set_title)(&format!(
                                    "Rust-PAL [save slot {slot} written]"
                                ));
                            }
                            Err(SaveOriginalGameError::Unavailable) => {
                                (context.set_title)("Rust-PAL [Save state unavailable]");
                            }
                            Err(SaveOriginalGameError::Io(_)) => {
                                (context.set_title)("Rust-PAL [Save failed]");
                            }
                        }
                    }
                    SaveSlotMode::Load if !slots[*selected].available => {
                        (context.set_title)("Rust-PAL [empty save slot]");
                    }
                    SaveSlotMode::Load => {
                        if context
                            .services
                            .visual
                            .queue(ScriptVisual::FadeOut { speed: 1 })
                        {
                            context.services.audio.music.stop();
                            context.services.persistence.pending_load_slot = Some(slot);
                            keep_menu = false;
                            (context.set_title)(&format!("Rust-PAL [loading save slot {slot}]"));
                        } else {
                            (context.set_title)("Rust-PAL [load transition unavailable]");
                        }
                    }
                }
            }
        }
    }
    if keep_menu {
        context.services.set_field_menu(menu);
    }
}

fn update_binary_selection(selected: &mut bool, direction: Option<Direction>) {
    match direction {
        Some(Direction::West | Direction::North) => *selected = false,
        Some(Direction::East | Direction::South) => *selected = true,
        None => {}
    }
}

fn update_shop_menu<L, S>(context: &mut MenuUpdateContext<'_, L, S>)
where
    L: FnMut(u16, Option<u16>, &RoleSprites) -> Option<LoadedScene>,
    S: FnMut(&str),
{
    let mut menu = context
        .services
        .take_shop_menu()
        .expect("shop menu was checked above");
    let items = menu.items(context.game);
    if menu.confirming {
        if context.input.cancel {
            menu.confirming = false;
        } else {
            if matches!(
                context.input.direction_pressed,
                Some(Direction::West | Direction::North)
            ) {
                menu.selected_yes = false;
            } else if matches!(
                context.input.direction_pressed,
                Some(Direction::East | Direction::South)
            ) {
                menu.selected_yes = true;
            }
            if context.input.confirm && menu.selected_yes {
                if let Some(item) = items.get(menu.selected) {
                    match menu.mode {
                        ShopMode::Buy { .. } => {
                            context.game.buy_item(item.item_id);
                        }
                        ShopMode::Sell => {
                            context.game.sell_item(item.item_id);
                        }
                    }
                }
            }
            if context.input.confirm {
                menu.confirming = false;
                let remaining = menu.items(context.game).len();
                menu.selected = menu.selected.min(remaining.saturating_sub(1));
            }
        }
    } else if context.input.cancel {
        context.advance_script();
        return;
    } else {
        menu.update_selection(context.input.direction_pressed, items.len());
        if context.input.confirm {
            if let Some(item) = items.get(menu.selected) {
                match menu.mode {
                    ShopMode::Buy { .. } => {
                        if context.game.cash >= u32::from(item.price) {
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
    context.services.set_shop_menu(menu);
}

fn update_inventory_menu<L, S>(context: &mut MenuUpdateContext<'_, L, S>)
where
    L: FnMut(u16, Option<u16>, &RoleSprites) -> Option<LoadedScene>,
    S: FnMut(&str),
{
    let mut menu = context
        .services
        .take_inventory_menu()
        .expect("inventory menu was checked above");
    let mut close_menu = false;
    let mut item_request = None;
    let mut equip_request = None;
    match menu.mode {
        InventoryMode::Items => {
            let inventory = context.game.inventory().collect::<Vec<_>>();
            if context.input.cancel {
                close_menu = true;
            } else {
                menu.update(context.input.direction_pressed, inventory.len());
                if context.input.confirm {
                    if let Some(&(item_id, _)) = inventory.get(menu.selected) {
                        if let Some(item) = context.game.usable_item(item_id) {
                            if item.apply_to_all {
                                item_request = context
                                    .game
                                    .item_use_request(item_id, None)
                                    .map(|request| (item_id, request, true));
                            } else {
                                menu.mode =
                                    InventoryMode::Target {
                                        item_id,
                                        selected: context.services.menus.item_target_selected.min(
                                            context.game.party.members().len().saturating_sub(1),
                                        ),
                                    };
                                (context.set_title)("Rust-PAL [Item target]");
                            }
                        }
                    }
                }
            }
        }
        InventoryMode::EquipItems => {
            let inventory = context.game.equippable_inventory();
            menu.selected = menu.selected.min(inventory.len().saturating_sub(1));
            if context.input.cancel {
                close_menu = true;
            } else {
                menu.update(context.input.direction_pressed, inventory.len());
                if context.input.confirm {
                    if let Some(&(item_id, _)) = inventory.get(menu.selected) {
                        menu.mode = InventoryMode::EquipTarget {
                            item_id,
                            selected: context
                                .services
                                .menus
                                .item_target_selected
                                .min(context.game.party.members().len().saturating_sub(1)),
                        };
                        (context.set_title)("Rust-PAL [Equip target]");
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
                context.input.direction_pressed,
                context.game.party.members().len(),
            );
            context.services.menus.item_target_selected = selected;
            menu.mode = InventoryMode::EquipTarget { item_id, selected };
            if context.input.cancel {
                menu.mode = InventoryMode::EquipItems;
                (context.set_title)("Rust-PAL [Equip item]");
            } else if context.input.confirm {
                equip_request = context
                    .game
                    .party
                    .members()
                    .get(selected)
                    .and_then(|member| context.game.item_equip_request(item_id, member.role_id))
                    .map(|request| (item_id, selected, request));
            }
        }
        InventoryMode::Target {
            item_id,
            mut selected,
        } => {
            update_wrapping_selection(
                &mut selected,
                context.input.direction_pressed,
                context.game.party.members().len(),
            );
            context.services.menus.item_target_selected = selected;
            menu.mode = InventoryMode::Target { item_id, selected };
            if context.input.cancel {
                menu.mode = InventoryMode::Items;
                (context.set_title)("Rust-PAL [Inventory]");
            } else if context.input.confirm {
                item_request = context
                    .game
                    .party
                    .members()
                    .get(selected)
                    .and_then(|member| context.game.item_use_request(item_id, Some(member.role_id)))
                    .map(|request| (item_id, request, false));
            }
        }
        InventoryMode::BattleUseItems | InventoryMode::BattleThrowItems => {
            close_menu = true;
        }
    }
    context.services.menus.inventory_selected = menu.selected;
    if let Some((item_id, role_selected, request)) = equip_request {
        if context.scripts.start(request) {
            context.services.menus.equip = Some(EquipSession {
                item_id,
                inventory_selected: menu.selected,
                role_selected,
            });
            (context.set_title)("Rust-PAL [Equipping]");
            context.advance_script();
        } else {
            context.services.set_inventory_menu(menu);
        }
    } else if let Some((item_id, request, apply_to_all)) = item_request {
        if context.scripts.start(request) {
            context.services.menus.item_use = Some(ItemUseSession {
                item_id,
                inventory_selected: menu.selected,
                apply_to_all,
            });
            (context.set_title)("Rust-PAL [Using item]");
            context.advance_script();
        } else {
            context.services.set_inventory_menu(menu);
        }
    } else if !close_menu {
        context.services.set_inventory_menu(menu);
    } else {
        (context.set_title)("Rust-PAL");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_binary_menu_uses_no_yes_direction_order() {
        let mut selected = true;
        update_binary_selection(&mut selected, Some(Direction::West));
        assert!(!selected);
        update_binary_selection(&mut selected, Some(Direction::North));
        assert!(!selected);
        update_binary_selection(&mut selected, Some(Direction::East));
        assert!(selected);
        update_binary_selection(&mut selected, Some(Direction::South));
        assert!(selected);
    }
}
