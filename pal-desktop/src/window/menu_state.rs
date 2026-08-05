use pal_core::game::{GameInput, GameState, StoreItem};
use pal_core::role::Direction;

use super::original_save::OriginalSaveSlot;

pub(super) const INVENTORY_COLUMNS: usize = 3;
pub(super) const INVENTORY_VISIBLE_ROWS: usize = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OpeningMenuPage {
    Main,
    SaveSlots,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct OpeningMenu {
    pub(super) page: OpeningMenuPage,
    pub(super) main_selected: usize,
    pub(super) slot_selected: usize,
    pub(super) slots: [OriginalSaveSlot; 5],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OpeningMenuAction {
    None,
    StartNewGame,
    LoadSlot(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SaveSlotMode {
    Save,
    Load,
}

impl OpeningMenu {
    pub(super) fn new(slots: [OriginalSaveSlot; 5]) -> Self {
        Self {
            page: OpeningMenuPage::Main,
            main_selected: 0,
            slot_selected: 0,
            slots,
        }
    }

    pub(super) fn update(&mut self, input: GameInput) -> OpeningMenuAction {
        match self.page {
            OpeningMenuPage::Main => {
                update_wrapping_selection(&mut self.main_selected, input.direction_pressed, 2);
                if input.cancel || (input.confirm && self.main_selected == 0) {
                    OpeningMenuAction::StartNewGame
                } else if input.confirm {
                    self.page = OpeningMenuPage::SaveSlots;
                    OpeningMenuAction::None
                } else {
                    OpeningMenuAction::None
                }
            }
            OpeningMenuPage::SaveSlots => {
                update_wrapping_selection(&mut self.slot_selected, input.direction_pressed, 5);
                if input.cancel {
                    self.page = OpeningMenuPage::Main;
                    self.main_selected = 0;
                    OpeningMenuAction::None
                } else if input.confirm {
                    OpeningMenuAction::LoadSlot(self.slots[self.slot_selected].slot)
                } else {
                    OpeningMenuAction::None
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FieldMenu {
    Main {
        selected: usize,
    },
    InventoryAction {
        selected: usize,
    },
    Status {
        selected: usize,
    },
    MagicCaster {
        selected: usize,
    },
    MagicList {
        caster: usize,
        selected: usize,
    },
    MagicTarget {
        caster: usize,
        magic_id: u16,
        selected: usize,
    },
    System {
        selected: usize,
    },
    SaveSlots {
        mode: SaveSlotMode,
        selected: usize,
        slots: [OriginalSaveSlot; 5],
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct InventoryMenu {
    pub(super) selected: usize,
    pub(super) mode: InventoryMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InventoryMode {
    Items,
    EquipItems,
    EquipTarget { item_id: u16, selected: usize },
    Target { item_id: u16, selected: usize },
    BattleUseItems,
    BattleThrowItems,
}

impl Default for InventoryMenu {
    fn default() -> Self {
        Self {
            selected: 0,
            mode: InventoryMode::Items,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ItemUseSession {
    pub(super) item_id: u16,
    pub(super) inventory_selected: usize,
    pub(super) apply_to_all: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct EquipSession {
    pub(super) item_id: u16,
    pub(super) inventory_selected: usize,
    pub(super) role_selected: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct MagicSession {
    pub(super) caster_selected: usize,
    pub(super) magic_id: u16,
    pub(super) target_selected: Option<usize>,
    pub(super) success_phase: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ConfirmationMenu {
    pub(super) no_entry: u16,
    pub(super) selected_yes: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ShopMode {
    Buy { store_number: u16 },
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ShopMenu {
    pub(super) mode: ShopMode,
    pub(super) selected: usize,
    pub(super) confirming: bool,
    pub(super) selected_yes: bool,
}

impl ShopMenu {
    pub(super) fn items(self, game: &GameState) -> Vec<StoreItem> {
        match self.mode {
            ShopMode::Buy { store_number } => game.store_items(store_number).unwrap_or_default(),
            ShopMode::Sell => game.sellable_inventory(),
        }
    }

    pub(super) fn update_selection(&mut self, direction: Option<Direction>, item_count: usize) {
        update_wrapping_selection(&mut self.selected, direction, item_count);
    }
}

impl InventoryMenu {
    pub(super) fn update(&mut self, direction: Option<Direction>, item_count: usize) {
        if item_count == 0 {
            self.selected = 0;
            return;
        }
        match direction {
            Some(Direction::North) => {
                self.selected = self.selected.saturating_sub(INVENTORY_COLUMNS)
            }
            Some(Direction::South) => {
                self.selected = (self.selected + INVENTORY_COLUMNS).min(item_count - 1)
            }
            Some(Direction::West) => self.selected = self.selected.saturating_sub(1),
            Some(Direction::East) => self.selected = (self.selected + 1).min(item_count - 1),
            _ => {}
        }
    }

    pub(super) fn first_visible(self, item_count: usize) -> usize {
        let selected_row = self.selected.min(item_count.saturating_sub(1)) / INVENTORY_COLUMNS;
        selected_row.saturating_sub(INVENTORY_VISIBLE_ROWS.div_ceil(2)) * INVENTORY_COLUMNS
    }
}

pub(super) fn update_wrapping_selection(
    selected: &mut usize,
    direction: Option<Direction>,
    count: usize,
) {
    if count == 0 {
        *selected = 0;
        return;
    }
    match direction {
        Some(Direction::North | Direction::West) => {
            *selected = if *selected == 0 {
                count - 1
            } else {
                *selected - 1
            };
        }
        Some(Direction::South | Direction::East) => *selected = (*selected + 1) % count,
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(direction: Option<Direction>, confirm: bool, cancel: bool) -> GameInput {
        GameInput {
            direction: None,
            direction_pressed: direction,
            confirm,
            cancel,
            ..GameInput::default()
        }
    }

    #[test]
    fn opening_menu_selects_new_game_and_save_slots_with_original_cancel_behavior() {
        let slots = std::array::from_fn(|index| OriginalSaveSlot {
            slot: index as u8 + 1,
            saved_times: index as u16,
            available: true,
        });
        let mut menu = OpeningMenu::new(slots);

        assert_eq!(
            menu.update(input(Some(Direction::South), false, false)),
            OpeningMenuAction::None
        );
        assert_eq!(menu.main_selected, 1);
        assert_eq!(
            menu.update(input(None, true, false)),
            OpeningMenuAction::None
        );
        assert_eq!(menu.page, OpeningMenuPage::SaveSlots);

        menu.update(input(Some(Direction::South), false, false));
        assert_eq!(
            menu.update(input(None, true, false)),
            OpeningMenuAction::LoadSlot(2)
        );
        assert_eq!(
            menu.update(input(None, false, true)),
            OpeningMenuAction::None
        );
        assert_eq!(menu.page, OpeningMenuPage::Main);
        assert_eq!(menu.main_selected, 0);
        assert_eq!(
            menu.update(input(None, false, true)),
            OpeningMenuAction::StartNewGame
        );
    }
}
