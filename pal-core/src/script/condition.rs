//! World-state predicates yielded by trigger scripts.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptCondition {
    ItemCountLess {
        item_id: u16,
        amount: i16,
        target_entry: u16,
    },
    ObjectStateEquals {
        object_id: u16,
        state: i16,
        target_entry: u16,
    },
    SceneEquals {
        scene_number: u16,
        target_entry: u16,
    },
    PartyContainsName {
        name_word_id: u16,
        target_entry: u16,
    },
    PlayerFacesObject {
        object_id: u16,
        range: u16,
        target_entry: u16,
    },
    PartyNotFullHp {
        target_entry: u16,
    },
    ItemNotEquipped {
        item_id: u16,
        amount: u16,
        target_entry: u16,
    },
    PlayerLacksPoison {
        role_id: u16,
        poison_id: u16,
        target_entry: u16,
    },
    EnemyLacksPoison {
        enemy_index: u16,
        poison_id: u16,
        target_entry: u16,
    },
    PlayerNotPoisoned {
        role_id: u16,
        target_entry: u16,
    },
    EnemyHpAbove {
        enemy_index: u16,
        percentage: u16,
        target_entry: u16,
    },
    EnemyNotFirstKind {
        enemy_index: u16,
        target_entry: u16,
    },
    EnemyTurn {
        target_entry: u16,
    },
}
