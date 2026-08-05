use std::path::Path;

use pal_assets::text::TextLibrary;
use pal_core::game::{GameInput, GameState};
use pal_core::role::{Direction, RoleSprites};
use pal_core::script::ScriptRuntime;

use crate::audio::{AUDIO_VOLUME_MAX, AUDIO_VOLUME_STEP};

use super::dialog::ActiveDialog;
use super::menu_state::{
    update_wrapping_selection, EquipSession, FieldMenu, InventoryMenu, InventoryMode,
    ItemUseSession, MagicSession, ShopMode,
};
use super::original_save::{
    latest_original_save_slot, restore_original_save, RestoreOriginalSaveError,
};
use super::script_driver::{advance_script, ScriptRenderResources};
use super::session::SessionState;
use super::snapshot::{restore_snapshot, save_snapshot, RestoreSnapshotError};
use super::LoadedScene;

pub(super) struct MenuUpdateContext<'a, L, S, E> {
    pub(super) input: GameInput,
    pub(super) scripts: &'a mut ScriptRuntime,
    pub(super) game: &'a mut GameState,
    pub(super) dialog: &'a mut Option<ActiveDialog>,
    pub(super) text: &'a TextLibrary,
    pub(super) role_sprites: &'a RoleSprites,
    pub(super) load_scene: &'a mut L,
    pub(super) services: &'a mut SessionState,
    pub(super) snapshot_path: &'a Path,
    pub(super) original_save_dir: &'a Path,
    pub(super) set_title: &'a mut S,
    pub(super) exit: &'a mut E,
}

impl<L, S, E> MenuUpdateContext<'_, L, S, E>
where
    L: FnMut(u16, Option<u16>, &RoleSprites) -> Option<LoadedScene>,
    S: FnMut(&str),
    E: FnMut(),
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
            self.services.music.play(music_id, true, 0);
        } else {
            self.services.music.stop();
        }
    }

    fn restore_latest_original_save(&mut self) -> Option<Result<(), RestoreOriginalSaveError>> {
        let slot = latest_original_save_slot(self.original_save_dir)?;
        Some(
            restore_original_save(
                self.original_save_dir,
                slot,
                self.game,
                self.role_sprites,
                self.load_scene,
            )
            .map(|environment| {
                self.services.current_save_slot = Some(environment.slot);
                self.services.visual.restore_original_environment(
                    environment.night_palette,
                    environment.screen_wave,
                );
                self.sync_music();
            }),
        )
    }
}

pub(super) fn update_active_menu<L, S, E>(
    context: &mut MenuUpdateContext<'_, L, S, E>,
) -> Option<bool>
where
    L: FnMut(u16, Option<u16>, &RoleSprites) -> Option<LoadedScene>,
    S: FnMut(&str),
    E: FnMut(),
{
    let changed =
        context.input.confirm || context.input.cancel || context.input.direction_pressed.is_some();
    if context.services.confirmation_menu.is_some() {
        update_confirmation_menu(context);
    } else if context.services.field_menu.is_some() {
        update_field_menu(context);
    } else if context.services.shop_menu.is_some() {
        update_shop_menu(context);
    } else if context.services.inventory_menu.is_some() {
        update_inventory_menu(context);
    } else {
        return None;
    }
    Some(changed)
}

fn update_confirmation_menu<L, S, E>(context: &mut MenuUpdateContext<'_, L, S, E>)
where
    L: FnMut(u16, Option<u16>, &RoleSprites) -> Option<LoadedScene>,
    S: FnMut(&str),
    E: FnMut(),
{
    let mut menu = context
        .services
        .confirmation_menu
        .take()
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
        context.services.confirmation_menu = Some(menu);
    }
}

fn update_field_menu<L, S, E>(context: &mut MenuUpdateContext<'_, L, S, E>)
where
    L: FnMut(u16, Option<u16>, &RoleSprites) -> Option<LoadedScene>,
    S: FnMut(&str),
    E: FnMut(),
{
    let mut menu = context
        .services
        .field_menu
        .take()
        .expect("field menu was checked above");
    let mut keep_menu = true;
    match &mut menu {
        FieldMenu::Main { selected } => {
            update_wrapping_selection(selected, context.input.direction_pressed, 4);
            context.services.main_menu_selected = *selected;
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
                            .magic_caster_selected
                            .min(context.game.party.members().len().saturating_sub(1));
                        menu = FieldMenu::MagicCaster { selected };
                        (context.set_title)("Rust-PAL [Magic]");
                    }
                    2 => {
                        menu = FieldMenu::InventoryAction {
                            selected: context.services.inventory_action_selected,
                        };
                        (context.set_title)("Rust-PAL [Inventory]");
                    }
                    3 => {
                        menu = FieldMenu::System {
                            selected: context.services.system_selected,
                        };
                        (context.set_title)("Rust-PAL [System]");
                    }
                    _ => unreachable!(),
                }
            }
        }
        FieldMenu::InventoryAction { selected } => {
            update_wrapping_selection(selected, context.input.direction_pressed, 2);
            context.services.inventory_action_selected = *selected;
            if context.input.cancel {
                keep_menu = false;
                (context.set_title)("Rust-PAL");
            } else if context.input.confirm {
                keep_menu = false;
                context.services.inventory_menu = Some(InventoryMenu {
                    selected: context.services.inventory_selected,
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
            context.services.magic_caster_selected = *selected;
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
                        selected: context.services.magic_selected,
                    };
                    (context.set_title)("Rust-PAL [Magic list]");
                }
            }
        }
        FieldMenu::MagicList { caster, selected } => {
            let role_id = context.game.party.members()[*caster].role_id;
            let magics = context.game.field_magics(role_id);
            update_wrapping_selection(selected, context.input.direction_pressed, magics.len());
            context.services.magic_selected = *selected;
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
                                context.services.magic = Some(MagicSession {
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
                            selected: context.services.magic_target_selected,
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
            context.services.magic_target_selected = *selected;
            if context.input.cancel {
                menu = FieldMenu::MagicList {
                    caster: *caster,
                    selected: context.services.magic_selected,
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
                        context.services.magic = Some(MagicSession {
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
            match context.input.direction_pressed {
                Some(direction @ (Direction::North | Direction::South)) => {
                    update_wrapping_selection(selected, Some(direction), 5);
                }
                Some(direction @ (Direction::West | Direction::East)) => match *selected {
                    2 => {
                        let volume =
                            adjusted_audio_volume(context.services.music.volume(), direction);
                        context.services.music.set_volume(volume);
                    }
                    3 => {
                        let volume = adjusted_audio_volume(
                            context.services.sound_effects.volume(),
                            direction,
                        );
                        context.services.sound_effects.set_volume(volume);
                    }
                    _ => {}
                },
                None => {}
            }
            context.services.system_selected = *selected;
            if context.input.cancel {
                menu = FieldMenu::Main {
                    selected: context.services.main_menu_selected,
                };
                (context.set_title)("Rust-PAL [Menu]");
            } else if context.input.confirm {
                match *selected {
                    0 => {
                        if save_snapshot(context.snapshot_path, context.game).is_ok() {
                            (context.set_title)("Rust-PAL [Saved]");
                        } else {
                            (context.set_title)("Rust-PAL [Save failed]");
                        }
                        keep_menu = false;
                    }
                    1 => match context.restore_latest_original_save() {
                        Some(Ok(())) => {
                            (context.set_title)("Rust-PAL [Original save loaded]");
                            keep_menu = false;
                        }
                        Some(Err(RestoreOriginalSaveError::SceneUnavailable)) => {
                            (context.set_title)("Rust-PAL [Original save scene unavailable]");
                        }
                        Some(Err(
                            RestoreOriginalSaveError::Unavailable
                            | RestoreOriginalSaveError::Invalid,
                        )) => {
                            (context.set_title)("Rust-PAL [Invalid original save]");
                        }
                        None => match restore_snapshot(
                            context.snapshot_path,
                            context.game,
                            context.role_sprites,
                            context.load_scene,
                        ) {
                            Ok(()) => {
                                context.sync_music();
                                (context.set_title)("Rust-PAL [Loaded]");
                                keep_menu = false;
                            }
                            Err(RestoreSnapshotError::Unavailable) => {
                                (context.set_title)("Rust-PAL [No save]");
                            }
                            Err(RestoreSnapshotError::SceneUnavailable) => {
                                (context.set_title)("Rust-PAL [Save scene unavailable]");
                            }
                        },
                    },
                    2 => {
                        let enabled = !context.services.music.enabled();
                        context.services.music.set_enabled(enabled);
                        if enabled {
                            context.sync_music();
                        }
                    }
                    3 => {
                        let enabled = !context.services.sound_effects.enabled();
                        context.services.sound_effects.set_enabled(enabled);
                    }
                    4 => {
                        keep_menu = false;
                        (context.exit)();
                    }
                    _ => unreachable!(),
                }
            }
        }
    }
    if keep_menu {
        context.services.field_menu = Some(menu);
    }
}

fn adjusted_audio_volume(volume: u8, direction: Direction) -> u8 {
    match direction {
        Direction::West => volume.saturating_sub(AUDIO_VOLUME_STEP),
        Direction::East => volume
            .saturating_add(AUDIO_VOLUME_STEP)
            .min(AUDIO_VOLUME_MAX),
        Direction::North | Direction::South => volume,
    }
}

fn update_shop_menu<L, S, E>(context: &mut MenuUpdateContext<'_, L, S, E>)
where
    L: FnMut(u16, Option<u16>, &RoleSprites) -> Option<LoadedScene>,
    S: FnMut(&str),
    E: FnMut(),
{
    let mut menu = context
        .services
        .shop_menu
        .take()
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
    context.services.shop_menu = Some(menu);
}

fn update_inventory_menu<L, S, E>(context: &mut MenuUpdateContext<'_, L, S, E>)
where
    L: FnMut(u16, Option<u16>, &RoleSprites) -> Option<LoadedScene>,
    S: FnMut(&str),
    E: FnMut(),
{
    let mut menu = context
        .services
        .inventory_menu
        .take()
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
                                menu.mode = InventoryMode::Target {
                                    item_id,
                                    selected: context
                                        .services
                                        .item_target_selected
                                        .min(context.game.party.members().len().saturating_sub(1)),
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
            context.services.item_target_selected = selected;
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
            context.services.item_target_selected = selected;
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
        InventoryMode::BattleUseItems
        | InventoryMode::BattleThrowItems
        | InventoryMode::BattleUseTarget { .. } => {
            close_menu = true;
        }
    }
    context.services.inventory_selected = menu.selected;
    if let Some((item_id, role_selected, request)) = equip_request {
        if context.scripts.start(request) {
            context.services.equip = Some(EquipSession {
                item_id,
                inventory_selected: menu.selected,
                role_selected,
            });
            (context.set_title)("Rust-PAL [Equipping]");
            context.advance_script();
        } else {
            context.services.inventory_menu = Some(menu);
        }
    } else if let Some((item_id, request, apply_to_all)) = item_request {
        if context.scripts.start(request) {
            context.services.item_use = Some(ItemUseSession {
                item_id,
                inventory_selected: menu.selected,
                apply_to_all,
            });
            (context.set_title)("Rust-PAL [Using item]");
            context.advance_script();
        } else {
            context.services.inventory_menu = Some(menu);
        }
    } else if !close_menu {
        context.services.inventory_menu = Some(menu);
    } else {
        (context.set_title)("Rust-PAL");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_audio_volume_steps_and_clamps() {
        assert_eq!(adjusted_audio_volume(0, Direction::West), 0);
        assert_eq!(adjusted_audio_volume(50, Direction::West), 40);
        assert_eq!(adjusted_audio_volume(50, Direction::East), 60);
        assert_eq!(
            adjusted_audio_volume(AUDIO_VOLUME_MAX, Direction::East),
            AUDIO_VOLUME_MAX
        );
    }
}
