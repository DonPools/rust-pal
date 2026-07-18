pub(super) const MAX_INVENTORY: usize = 1024;
pub(super) const ITEM_FLAG_USABLE: u16 = 1 << 0;
pub(super) const ITEM_FLAG_EQUIPPABLE: u16 = 1 << 1;
pub(super) const ITEM_FLAG_CONSUMING: u16 = 1 << 3;
pub(super) const ITEM_FLAG_APPLY_TO_ALL: u16 = 1 << 4;
pub(super) const ITEM_FLAG_SELLABLE: u16 = 1 << 5;
pub(super) const ITEM_FLAG_ROLE_FIRST: u16 = 1 << 6;
pub(super) const MAGIC_FLAG_USABLE_OUTSIDE_BATTLE: u16 = 1 << 0;
pub(super) const MAGIC_FLAG_APPLY_TO_ALL: u16 = 1 << 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreItem {
    pub item_id: u16,
    pub price: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsableItem {
    pub item_id: u16,
    pub amount: u16,
    pub script_entry: u16,
    pub consuming: bool,
    pub apply_to_all: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EquippableItem {
    pub item_id: u16,
    pub amount: u16,
    pub script_entry: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldMagic {
    pub magic_id: u16,
    pub mp_cost: u16,
    pub use_script: u16,
    pub success_script: u16,
    pub apply_to_all: bool,
    pub enabled: bool,
}
