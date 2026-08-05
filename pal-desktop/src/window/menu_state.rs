use pal_core::game::{GameState, StoreItem};
use pal_core::role::Direction;

pub(super) const INVENTORY_COLUMNS: usize = 3;
pub(super) const INVENTORY_VISIBLE_ROWS: usize = 7;

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
    BattleUseTarget { item_id: u16, selected: usize },
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
