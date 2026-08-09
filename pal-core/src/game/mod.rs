//! Deterministic exploration state and update rules.

use std::collections::BTreeMap;

use pal_assets::battle::BattleData;
use pal_assets::magic::Magics;
use pal_assets::objects::GlobalObjects;
use pal_assets::player_roles::{PlayerRole, PlayerRoles, PLAYER_ROLE_COUNT};
use pal_assets::save::{
    OriginalSave, SaveExperience, SaveInventoryEntry, SavePartyMember, SavePoison, SaveTrail,
    SAVE_EXPERIENCE_KINDS, SAVE_INVENTORY_CAPACITY, SAVE_PARTY_CAPACITY, SAVE_POISON_SLOTS,
    SAVE_ROLE_COUNT, SAVE_SCENE_CAPACITY,
};
use pal_assets::scene::{EventObject as AssetEventObject, Scene as AssetScene};
use pal_assets::script::ScriptTable;
use pal_assets::store::Stores;

use crate::battle::{
    add_poison, cure_poison, cure_poison_by_level, BattleEnemy, BattleEvent, BattlePhase,
    BattlePoison, BattleRequest, BattleResult, BattleRewards, BattleScriptSource, BattleState,
    BattleStatus, BattleStatuses, BattleSteal, BATTLE_STATUS_COUNT,
    HIDDEN_EXPERIENCE_CATEGORY_COUNT, HIDDEN_EXP_ATTACK, HIDDEN_EXP_DEFENSE, HIDDEN_EXP_DEXTERITY,
    HIDDEN_EXP_FLEE, HIDDEN_EXP_HEALTH, HIDDEN_EXP_MAGIC, HIDDEN_EXP_MAGIC_POWER,
    MAX_BATTLE_POISONS,
};
use crate::map::tile_to_world;
use crate::map::Map;
use crate::party::{Party, MAX_PARTY_MEMBERS};
use crate::random;
use crate::role::{Direction, Role};
use crate::scene::{
    blocks_position, find_search_trigger, find_touch_trigger, SceneObject, TriggerKind,
    TriggerRequest,
};
use crate::script::{ScriptEvent, ScriptRuntime};

/// Compatibility tick used by script delays and blocking visual effects.
pub const UPDATE_INTERVAL_MS: u64 = 50;
/// Original scene update interval (10 FPS).
pub const EXPLORATION_FRAME_MS: u64 = 100;
/// Original battle update interval (25 FPS).
pub const BATTLE_FRAME_MS: u64 = 40;

// Classic keeps the party leader at this screen-space anchor during normal
// exploration. Scripted viewport movement changes the anchor until opcode
// 0x007F restores it.
const DEFAULT_PARTY_SCREEN_POSITION: (i32, i32) = (160, 112);

/// Per-player changes produced by Classic's victory settlement sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BattlePlayerSettlement {
    pub role_id: u16,
    pub levels_gained: u16,
    pub hidden_growth: [u16; HIDDEN_EXPERIENCE_CATEGORY_COUNT],
    pub learned_magics: Vec<u16>,
}

/// Rewards and presentation details produced while preparing a battle victory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BattleVictorySettlement {
    pub rewards: BattleRewards,
    pub players: Vec<BattlePlayerSettlement>,
}

mod auto_script;
mod battle_integration;
mod equipment_scripts;
mod exploration;
mod field;
mod party_state;
mod scene_state;
mod script_actions;
mod snapshot;
mod state_support;
#[cfg(test)]
mod tests;

pub use exploration::{Camera, CollisionMap, GameInput};
pub use field::{EquippableItem, FieldMagic, StoreItem, ThrowableItem, UsableItem};
use field::{
    ITEM_FLAG_APPLY_TO_ALL, ITEM_FLAG_CONSUMING, ITEM_FLAG_EQUIPPABLE, ITEM_FLAG_ROLE_FIRST,
    ITEM_FLAG_SELLABLE, ITEM_FLAG_THROWABLE, ITEM_FLAG_USABLE, MAGIC_FLAG_APPLY_TO_ALL,
    MAGIC_FLAG_USABLE_OUTSIDE_BATTLE, MAX_INVENTORY,
};
pub use snapshot::GameSnapshot;
use snapshot::{
    SavedPartySlot, SavedPlayerRole, SavedRole, SavedSceneObject, SavedTrailPoint, SnapshotData,
    SNAPSHOT_VERSION,
};
use state_support::{apply_role_attribute, unique_script_entries, valid_role_attribute};
pub use state_support::{AutoScriptError, AutoScriptUpdate};

/// State for the current exploration scene.
pub struct GameState<M = Map> {
    pub scene_number: u16,
    pub map: M,
    pub player: Role,
    pub scene_objects: Vec<SceneObject>,
    pub pending_trigger: Option<TriggerRequest>,
    pub party: Party,
    pub current_music: Option<u16>,
    pub current_battle_music: u16,
    pub current_battlefield: u16,
    pub cash: u32,
    role_experience: [u32; PLAYER_ROLE_COUNT],
    growth_random_state: u32,
    player_roles: Option<PlayerRoles>,
    stores: Option<Stores>,
    global_objects: Option<GlobalObjects>,
    magics: Option<Magics>,
    battle_data: Option<BattleData>,
    active_battle: Option<BattleState>,
    auto_battle: bool,
    player_statuses: [BattleStatuses; PLAYER_ROLE_COUNT],
    player_poisons: [[BattlePoison; MAX_BATTLE_POISONS]; PLAYER_ROLE_COUNT],
    collect_value: u16,
    inventory: Vec<(u16, u16)>,
    item_use_scripts: BTreeMap<u16, u16>,
    item_equip_scripts: BTreeMap<u16, u16>,
    item_throw_scripts: BTreeMap<u16, u16>,
    magic_use_scripts: BTreeMap<u16, u16>,
    magic_success_scripts: BTreeMap<u16, u16>,
    object_script_overrides: BTreeMap<(u16, u16), u16>,
    equipment_effects: BTreeMap<(u16, u16, u16), i16>,
    current_equipment_slot: Option<u16>,
    inactive_objects: BTreeMap<u16, SceneObject>,
    scene_enter_scripts: BTreeMap<u16, u16>,
    scene_teleport_scripts: BTreeMap<u16, u16>,
    scene_maps: BTreeMap<u16, u16>,
    pending_auto_sounds: Vec<u16>,
    pending_auto_events: Vec<ScriptEvent>,
    pending_auto_script_failure: bool,
    auto_instruction_runtime: Option<ScriptRuntime>,
    script_frame: u32,
    chase_range: u16,
    chase_speed_change_cycles: u16,
    viewport_locked: bool,
    party_screen_position: (i32, i32),
    party_followers: Vec<Role>,
    extra_follower_ids: Vec<u16>,
    party_slots: [PartySlotState; MAX_PARTY_MEMBERS],
    party_trail: [TrailPoint; MAX_PARTY_MEMBERS],
    save_scenes: Option<[AssetScene; SAVE_SCENE_CAPACITY]>,
    save_event_objects: Vec<AssetEventObject>,
    save_experience: [[SaveExperience; SAVE_ROLE_COUNT]; SAVE_EXPERIENCE_KINDS],
    save_battle_speed: u16,
    save_layer: u16,
    pub camera: Camera,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrailPoint {
    world_x: i32,
    world_y: i32,
    direction: Direction,
}

/// Runtime counterpart of Classic's fixed `rgParty` slot state.
///
/// Scripts may position or pose an inactive slot immediately before assigning a
/// role to it, so these values cannot live only on the currently rendered roles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PartySlotState {
    world_x: i32,
    world_y: i32,
    direction: Direction,
    anim_frame: u8,
}

impl PartySlotState {
    fn from_role(role: &Role) -> Self {
        Self {
            world_x: role.world_x,
            world_y: role.world_y,
            direction: role.direction,
            anim_frame: role.anim_frame,
        }
    }

    fn apply_to(self, role: &mut Role) {
        role.world_x = self.world_x;
        role.world_y = self.world_y;
        role.direction = self.direction;
        role.anim_frame = self.anim_frame;
    }
}

impl<M: CollisionMap> GameState<M> {
    pub fn new(map: M, player: Role, viewport_width: u32, viewport_height: u32) -> Self {
        let player_slot = PartySlotState::from_role(&player);
        let follower_slot = PartySlotState {
            world_y: player.world_y - 1,
            ..player_slot
        };
        let initial_trail = TrailPoint {
            world_x: player.world_x,
            world_y: player.world_y,
            direction: player.direction,
        };
        let mut state = Self {
            scene_number: 1,
            map,
            player,
            scene_objects: Vec::new(),
            pending_trigger: None,
            party: Party::default(),
            current_music: None,
            current_battle_music: 0,
            current_battlefield: 0,
            cash: 0,
            role_experience: [0; PLAYER_ROLE_COUNT],
            growth_random_state: random::DEFAULT_RANDOM_SEED,
            player_roles: None,
            stores: None,
            global_objects: None,
            magics: None,
            battle_data: None,
            active_battle: None,
            auto_battle: false,
            player_statuses: [BattleStatuses::from_durations([0; BATTLE_STATUS_COUNT]);
                PLAYER_ROLE_COUNT],
            player_poisons: [[BattlePoison {
                object_id: 0,
                script_entry: 0,
            }; MAX_BATTLE_POISONS]; PLAYER_ROLE_COUNT],
            collect_value: 0,
            inventory: Vec::new(),
            item_use_scripts: BTreeMap::new(),
            item_equip_scripts: BTreeMap::new(),
            item_throw_scripts: BTreeMap::new(),
            magic_use_scripts: BTreeMap::new(),
            magic_success_scripts: BTreeMap::new(),
            object_script_overrides: BTreeMap::new(),
            equipment_effects: BTreeMap::new(),
            current_equipment_slot: None,
            inactive_objects: BTreeMap::new(),
            scene_enter_scripts: BTreeMap::new(),
            scene_teleport_scripts: BTreeMap::new(),
            scene_maps: BTreeMap::new(),
            pending_auto_sounds: Vec::new(),
            pending_auto_events: Vec::new(),
            pending_auto_script_failure: false,
            auto_instruction_runtime: None,
            script_frame: 0,
            chase_range: 1,
            chase_speed_change_cycles: 0,
            viewport_locked: false,
            party_screen_position: DEFAULT_PARTY_SCREEN_POSITION,
            party_followers: Vec::new(),
            extra_follower_ids: Vec::new(),
            party_slots: std::array::from_fn(|index| {
                if index == 0 {
                    player_slot
                } else {
                    follower_slot
                }
            }),
            party_trail: [initial_trail; MAX_PARTY_MEMBERS],
            save_scenes: None,
            save_event_objects: Vec::new(),
            save_experience: [[SaveExperience {
                experience: 0,
                reserved: 0,
                level: 0,
                count: 0,
            }; SAVE_ROLE_COUNT]; SAVE_EXPERIENCE_KINDS],
            save_battle_speed: 2,
            save_layer: 0,
            camera: Camera::new(viewport_width, viewport_height),
        };
        state.follow_player();
        state
    }

    pub fn with_scene_objects(mut self, scene_objects: Vec<SceneObject>) -> Self {
        self.scene_objects = scene_objects;
        self
    }

    /// Install the complete global event-object table, excluding objects in
    /// the currently active scene.
    pub fn with_global_objects(mut self, objects: Vec<SceneObject>) -> Self {
        let current_ids = self
            .scene_objects
            .iter()
            .map(|object| object.id)
            .collect::<std::collections::BTreeSet<_>>();
        self.inactive_objects = objects
            .into_iter()
            .filter(|object| !current_ids.contains(&object.id))
            .map(|object| (object.id, object))
            .collect();
        self
    }

    pub fn with_scene_number(mut self, scene_number: u16) -> Self {
        self.scene_number = scene_number;
        self
    }

    /// Seed Classic's single random sequence. A zero seed uses the deterministic fallback.
    pub fn with_random_seed(mut self, seed: u32) -> Self {
        let seed = if seed == 0 {
            random::DEFAULT_RANDOM_SEED
        } else {
            seed
        };
        self.growth_random_state = random::seed(seed);
        self
    }

    /// Return the random state owned by the active battle or the surrounding game.
    pub fn random_state(&self) -> u32 {
        self.active_battle
            .as_ref()
            .map_or(self.growth_random_state, BattleState::random_state)
    }

    /// Synchronize script execution with Classic's single random sequence.
    pub fn set_random_state(&mut self, state: u32) {
        if let Some(battle) = self.active_battle.as_mut() {
            battle.set_random_state(state);
        } else {
            self.growth_random_state = state;
        }
    }

    /// Install static save records that are not otherwise needed by runtime logic.
    pub fn with_original_save_data(
        mut self,
        scenes: &[AssetScene],
        event_objects: &[AssetEventObject],
    ) -> Self {
        if scenes.len() <= SAVE_SCENE_CAPACITY
            && event_objects.len() <= pal_assets::save::SAVE_EVENT_OBJECT_CAPACITY
        {
            let mut save_scenes = [AssetScene {
                map_num: 0,
                script_on_enter: 0,
                script_on_teleport: 0,
                event_object_index: 0,
            }; SAVE_SCENE_CAPACITY];
            save_scenes[..scenes.len()].copy_from_slice(scenes);
            self.save_scenes = Some(save_scenes);
            self.save_event_objects = event_objects.to_vec();
        }
        self
    }

    pub fn with_party(mut self, party: Party) -> Self {
        self.party = party;
        self.rebuild_party_followers();
        self
    }

    pub fn with_player_roles(mut self, player_roles: PlayerRoles) -> Self {
        self.player_roles = Some(player_roles);
        if let Some(roles) = &self.player_roles {
            self.party.sync_from_roles(roles);
            for category in &mut self.save_experience {
                for (experience, role) in category.iter_mut().zip(roles.iter()) {
                    experience.level = role.level;
                }
            }
        }
        self.rebuild_party_followers();
        self
    }

    pub fn with_economy_data(mut self, stores: Stores, global_objects: GlobalObjects) -> Self {
        self.stores = Some(stores);
        self.global_objects = Some(global_objects);
        self
    }

    pub fn with_magic_data(mut self, magics: Magics) -> Self {
        self.magics = Some(magics);
        self
    }

    pub fn with_battle_data(mut self, battle_data: BattleData) -> Self {
        self.battle_data = Some(battle_data);
        self
    }

    pub fn party_followers(&self) -> &[Role] {
        &self.party_followers
    }

    pub fn player_role(&self, role_id: u16) -> Option<&PlayerRole> {
        self.player_roles.as_ref()?.role(usize::from(role_id))
    }

    pub fn effective_player_role(&self, role_id: u16) -> Option<PlayerRole> {
        let mut role = self.player_role(role_id)?.clone();
        for (&(_, effect_role, attribute), &value) in &self.equipment_effects {
            if effect_role == role_id {
                apply_role_attribute(&mut role, attribute, value, false)?;
            }
        }
        Some(role)
    }

    pub fn item_count(&self, item_id: u16) -> u16 {
        let inventory = self.inventory_count(item_id);
        let equipped = self
            .player_roles
            .as_ref()
            .map(|roles| {
                self.party
                    .members()
                    .iter()
                    .filter_map(|member| roles.role(usize::from(member.role_id)))
                    .flat_map(|role| role.equipment)
                    .filter(|&equipped| equipped == item_id)
                    .count()
            })
            .unwrap_or(0);
        inventory.saturating_add(u16::try_from(equipped).unwrap_or(u16::MAX))
    }

    pub fn item_bitmap(&self, item_id: u16) -> Option<u16> {
        Some(self.global_objects.as_ref()?.get(item_id)?.item_bitmap())
    }

    /// Count an item in the active party's equipment slots only.
    pub fn equipped_item_count(&self, item_id: u16) -> u16 {
        let Some(roles) = self.player_roles.as_ref() else {
            return 0;
        };
        let equipped = self
            .party
            .members()
            .iter()
            .filter_map(|member| roles.role(usize::from(member.role_id)))
            .flat_map(|role| role.equipment)
            .filter(|&equipped| equipped == item_id)
            .count();
        u16::try_from(equipped).unwrap_or(u16::MAX)
    }

    /// Report whether any active party member is below maximum HP.
    pub fn party_not_full_hp(&self) -> bool {
        let Some(roles) = self.player_roles.as_ref() else {
            return false;
        };
        self.party.members().iter().any(|member| {
            roles
                .role(usize::from(member.role_id))
                .is_some_and(|role| role.hp < role.max_hp)
        })
    }

    pub fn inventory_count(&self, item_id: u16) -> u16 {
        self.inventory
            .iter()
            .find_map(|&(id, amount)| (id == item_id).then_some(amount))
            .unwrap_or(0)
    }

    /// Return inventory entries in original acquisition-slot order.
    pub fn inventory(&self) -> impl ExactSizeIterator<Item = (u16, u16)> + '_ {
        self.inventory.iter().copied()
    }

    fn set_inventory_amount(&mut self, item_id: u16, amount: u16) -> bool {
        if let Some(index) = self.inventory.iter().position(|&(id, _)| id == item_id) {
            if amount == 0 {
                self.inventory.remove(index);
            } else {
                self.inventory[index].1 = amount;
            }
            return true;
        }
        if amount == 0 {
            return true;
        }
        if self.inventory.len() >= MAX_INVENTORY {
            return false;
        }
        self.inventory.push((item_id, amount));
        true
    }

    pub fn usable_item(&self, item_id: u16) -> Option<UsableItem> {
        let amount = self.inventory_count(item_id);
        let object = self.global_objects.as_ref()?.get(item_id)?;
        let flags = object.item_flags();
        (amount > 0 && flags & ITEM_FLAG_USABLE != 0).then_some(UsableItem {
            item_id,
            amount,
            script_entry: self
                .item_use_scripts
                .get(&item_id)
                .copied()
                .unwrap_or_else(|| object.item_use_script()),
            consuming: flags & ITEM_FLAG_CONSUMING != 0,
            apply_to_all: flags & ITEM_FLAG_APPLY_TO_ALL != 0,
        })
    }

    pub fn usable_inventory(&self) -> Vec<UsableItem> {
        self.inventory()
            .filter_map(|(item_id, _)| self.usable_item(item_id))
            .collect()
    }

    pub fn battle_usable_item(&self, item_id: u16) -> Option<UsableItem> {
        let mut item = self.usable_item(item_id)?;
        let reserved = self.active_battle.as_ref()?.reserved_item_count(item_id);
        item.amount = item.amount.saturating_sub(reserved);
        (item.amount > 0 || !item.consuming).then_some(item)
    }

    pub fn battle_usable_inventory(&self) -> Vec<UsableItem> {
        self.inventory()
            .filter_map(|(item_id, _)| self.battle_usable_item(item_id))
            .collect()
    }

    pub fn throwable_item(&self, item_id: u16) -> Option<ThrowableItem> {
        let amount = self
            .inventory_count(item_id)
            .saturating_sub(self.active_battle.as_ref()?.reserved_item_count(item_id));
        let object = self.global_objects.as_ref()?.get(item_id)?;
        let flags = object.item_flags();
        (amount > 0 && flags & ITEM_FLAG_THROWABLE != 0).then_some(ThrowableItem {
            item_id,
            amount,
            script_entry: self
                .item_throw_scripts
                .get(&item_id)
                .copied()
                .unwrap_or_else(|| object.item_throw_script()),
            apply_to_all: flags & ITEM_FLAG_APPLY_TO_ALL != 0,
        })
    }

    pub fn throwable_inventory(&self) -> Vec<ThrowableItem> {
        self.inventory()
            .filter_map(|(item_id, _)| self.throwable_item(item_id))
            .collect()
    }

    pub fn battle_use_item(
        &mut self,
        item_id: u16,
        target_player: Option<usize>,
    ) -> Option<Vec<BattleEvent>> {
        let item = self.battle_usable_item(item_id)?;
        let battle = self.active_battle.as_mut()?;
        if item.apply_to_all != target_player.is_none()
            || target_player.is_some_and(|target| target >= battle.players.len())
        {
            return None;
        }
        battle.use_item(
            item.item_id,
            target_player,
            item.script_entry,
            item.consuming,
        )
    }

    pub fn battle_throw_item(
        &mut self,
        item_id: u16,
        target_enemy: Option<usize>,
    ) -> Option<Vec<BattleEvent>> {
        let item = self.throwable_item(item_id)?;
        let battle = self.active_battle.as_mut()?;
        if item.apply_to_all != target_enemy.is_none()
            || target_enemy.is_some_and(|target| {
                battle
                    .enemies
                    .get(target)
                    .is_none_or(|enemy| !enemy.is_alive())
            })
        {
            return None;
        }
        battle.throw_item(item.item_id, target_enemy, item.script_entry)
    }

    /// Repeat the active player's previous Classic battle command.
    pub fn repeat_battle_action(&mut self) -> Option<Vec<BattleEvent>> {
        let requirement = self.active_battle.as_ref()?.repeated_item_requirement();
        if let Some((item_id, thrown)) = requirement {
            let available = if thrown {
                self.throwable_item(item_id).is_some()
            } else {
                self.battle_usable_item(item_id).is_some()
            };
            if !available {
                let battle = self.active_battle.as_mut()?;
                return battle.repeat_unavailable_item(thrown);
            }
        }
        self.active_battle.as_mut()?.repeat_last_action()
    }

    pub fn equippable_item(&self, item_id: u16, role_id: u16) -> Option<EquippableItem> {
        let amount = self.inventory_count(item_id);
        let object = self.global_objects.as_ref()?.get(item_id)?;
        let role_flag = ITEM_FLAG_ROLE_FIRST.checked_shl(u32::from(role_id))?;
        let flags = object.item_flags();
        (amount > 0
            && flags & ITEM_FLAG_EQUIPPABLE != 0
            && flags & role_flag != 0
            && self.player_role(role_id).is_some())
        .then_some(EquippableItem {
            item_id,
            amount,
            script_entry: self
                .item_equip_scripts
                .get(&item_id)
                .copied()
                .unwrap_or_else(|| object.item_equip_script()),
        })
    }

    pub fn equippable_inventory(&self) -> Vec<(u16, u16)> {
        let Some(objects) = self.global_objects.as_ref() else {
            return Vec::new();
        };
        self.inventory()
            .filter(|&(item_id, _)| {
                objects
                    .get(item_id)
                    .is_some_and(|object| object.item_flags() & ITEM_FLAG_EQUIPPABLE != 0)
            })
            .collect()
    }

    pub fn item_equip_request(&self, item_id: u16, role_id: u16) -> Option<TriggerRequest> {
        let item = self.equippable_item(item_id, role_id)?;
        (item.script_entry != 0).then_some(TriggerRequest {
            object_id: role_id,
            script_entry: item.script_entry,
            kind: crate::scene::TriggerKind::Equip,
        })
    }

    pub fn finish_item_equip(&mut self, item_id: u16, next_entry: u16) {
        self.item_equip_scripts.insert(item_id, next_entry);
        self.current_equipment_slot = None;
    }

    pub fn field_magics(&self, role_id: u16) -> Vec<FieldMagic> {
        let Some(role) = self.player_role(role_id) else {
            return Vec::new();
        };
        let Some(objects) = self.global_objects.as_ref() else {
            return Vec::new();
        };
        let Some(magics) = self.magics.as_ref() else {
            return Vec::new();
        };
        let mut result = role
            .magic
            .into_iter()
            .filter(|&magic_id| magic_id != 0)
            .filter_map(|magic_id| {
                let object = objects.get(magic_id)?;
                let definition = magics.get(object.magic_number())?;
                let flags = object.magic_flags();
                Some(FieldMagic {
                    magic_id,
                    mp_cost: definition.mp_cost,
                    use_script: self
                        .magic_use_scripts
                        .get(&magic_id)
                        .copied()
                        .unwrap_or_else(|| object.magic_use_script()),
                    success_script: self
                        .magic_success_scripts
                        .get(&magic_id)
                        .copied()
                        .unwrap_or_else(|| object.magic_success_script()),
                    apply_to_all: flags & MAGIC_FLAG_APPLY_TO_ALL != 0,
                    enabled: role.hp > 0
                        && role.mp >= definition.mp_cost
                        && flags & MAGIC_FLAG_USABLE_OUTSIDE_BATTLE != 0,
                })
            })
            .collect::<Vec<_>>();
        result.sort_unstable_by_key(|magic| magic.magic_id);
        result
    }

    pub fn magic_request(
        &self,
        caster_role: u16,
        magic_id: u16,
        target_role: Option<u16>,
        success_phase: bool,
    ) -> Option<TriggerRequest> {
        let magic = self
            .field_magics(caster_role)
            .into_iter()
            .find(|magic| magic.magic_id == magic_id && magic.enabled)?;
        let owner = if magic.apply_to_all {
            target_role.is_none().then_some(0)?
        } else {
            let target = target_role?;
            self.party
                .members()
                .iter()
                .any(|member| member.role_id == target)
                .then_some(target)?
        };
        let script_entry = if success_phase {
            magic.success_script
        } else {
            magic.use_script
        };
        (script_entry != 0).then_some(TriggerRequest {
            object_id: owner,
            script_entry,
            kind: crate::scene::TriggerKind::Magic,
        })
    }

    /// Start a field magic at its first available script phase.
    ///
    /// Classic magic objects may omit the use script. In that case the success
    /// script is the initial phase instead of making the magic unusable.
    pub fn initial_magic_request(
        &self,
        caster_role: u16,
        magic_id: u16,
        target_role: Option<u16>,
    ) -> Option<(TriggerRequest, bool)> {
        let success_phase = self
            .field_magics(caster_role)
            .into_iter()
            .find(|magic| magic.magic_id == magic_id && magic.enabled)?
            .use_script
            == 0;
        self.magic_request(caster_role, magic_id, target_role, success_phase)
            .map(|request| (request, success_phase))
    }

    pub fn finish_magic_script(&mut self, magic_id: u16, next_entry: u16, success_phase: bool) {
        let entries = if success_phase {
            &mut self.magic_success_scripts
        } else {
            &mut self.magic_use_scripts
        };
        entries.insert(magic_id, next_entry);
    }

    pub fn consume_magic_mp(&mut self, role_id: u16, magic_id: u16) -> bool {
        let Some(cost) = self
            .field_magics(role_id)
            .into_iter()
            .find(|magic| magic.magic_id == magic_id && magic.enabled)
            .map(|magic| magic.mp_cost)
        else {
            return false;
        };
        let Some(roles) = self.player_roles.as_mut() else {
            return false;
        };
        let Some(role) = roles.role_mut(usize::from(role_id)) else {
            return false;
        };
        role.mp -= cost;
        self.party.sync_from_roles(roles);
        true
    }

    pub fn item_use_request(&self, item_id: u16, role_id: Option<u16>) -> Option<TriggerRequest> {
        let item = self.usable_item(item_id)?;
        let object_id = if item.apply_to_all {
            role_id.is_none().then_some(0xffff)?
        } else {
            let role_id = role_id?;
            self.party
                .members()
                .iter()
                .any(|member| member.role_id == role_id)
                .then_some(role_id)?
        };
        (item.script_entry != 0).then_some(TriggerRequest {
            object_id,
            script_entry: item.script_entry,
            kind: crate::scene::TriggerKind::Item,
        })
    }

    /// Persist an item's script entry and consume it only after a successful script.
    pub fn finish_item_use(&mut self, item_id: u16, next_entry: u16, succeeded: bool) -> bool {
        let Some(item) = self.usable_item(item_id) else {
            return false;
        };
        self.item_use_scripts.insert(item_id, next_entry);
        if succeeded && item.consuming {
            return self.consume_inventory_item(item_id);
        }
        true
    }

    fn consume_inventory_item(&mut self, item_id: u16) -> bool {
        let amount = self.inventory_count(item_id);
        if amount == 0 {
            return false;
        }
        self.set_inventory_amount(item_id, amount - 1);
        true
    }

    pub fn store_items(&self, store_number: u16) -> Option<Vec<StoreItem>> {
        let store = self.stores.as_ref()?.get(store_number)?;
        let objects = self.global_objects.as_ref()?;
        store
            .items()
            .map(|item_id| {
                let object = objects.get(item_id)?;
                Some(StoreItem {
                    item_id,
                    price: object.item_price(),
                })
            })
            .collect()
    }

    pub fn sellable_inventory(&self) -> Vec<StoreItem> {
        let Some(objects) = self.global_objects.as_ref() else {
            return Vec::new();
        };
        self.inventory()
            .filter_map(|(item_id, _)| {
                let object = objects.get(item_id)?;
                (object.item_flags() & ITEM_FLAG_SELLABLE != 0).then_some(StoreItem {
                    item_id,
                    price: object.item_price() / 2,
                })
            })
            .collect()
    }

    pub fn buy_item(&mut self, item_id: u16) -> bool {
        let Some(object) = self
            .global_objects
            .as_ref()
            .and_then(|objects| objects.get(item_id))
        else {
            return false;
        };
        let price = u32::from(object.item_price());
        let amount = self.inventory_count(item_id);
        if self.cash < price
            || amount >= 99
            || (amount == 0 && self.inventory.len() >= MAX_INVENTORY)
        {
            return false;
        }
        self.cash -= price;
        self.set_inventory_amount(item_id, amount + 1);
        true
    }

    pub fn sell_item(&mut self, item_id: u16) -> bool {
        let Some(object) = self
            .global_objects
            .as_ref()
            .and_then(|objects| objects.get(item_id))
        else {
            return false;
        };
        if object.item_flags() & ITEM_FLAG_SELLABLE == 0 || self.inventory_count(item_id) == 0 {
            return false;
        }
        let price = u32::from(object.item_price() / 2);
        if !self.remove_item(item_id, 1, 1) {
            return false;
        }
        self.cash = self.cash.saturating_add(price);
        true
    }

    pub fn adjust_cash(&mut self, amount: i16) -> bool {
        if amount < 0 {
            let cost = u32::from(amount.unsigned_abs());
            if self.cash < cost {
                return false;
            }
            self.cash -= cost;
        } else {
            self.cash = self.cash.saturating_add(amount as u32);
        }
        true
    }

    /// Remove inventory first, then matching equipment from active members.
    /// A nonzero failure entry makes the operation atomic on insufficiency.
    pub fn remove_item(&mut self, item_id: u16, amount: u16, insufficient_entry: u16) -> bool {
        if item_id == 0 {
            return false;
        }
        let amount = amount.max(1);
        if insufficient_entry != 0 && self.item_count(item_id) < amount {
            return false;
        }

        let inventory_amount = self.inventory_count(item_id);
        let removed_from_inventory = inventory_amount.min(amount);
        self.set_inventory_amount(item_id, inventory_amount - removed_from_inventory);

        let mut remaining = amount - removed_from_inventory;
        let mut removed_slots = Vec::new();
        if remaining > 0 {
            let role_ids = self
                .party
                .members()
                .iter()
                .map(|member| member.role_id)
                .collect::<Vec<_>>();
            if let Some(roles) = self.player_roles.as_mut() {
                'roles: for role_id in role_ids {
                    let Some(role) = roles.role_mut(usize::from(role_id)) else {
                        continue;
                    };
                    for (slot, equipment) in role.equipment.iter_mut().enumerate() {
                        if *equipment == item_id {
                            *equipment = 0;
                            removed_slots.push((role_id, slot));
                            remaining -= 1;
                            if remaining == 0 {
                                break 'roles;
                            }
                        }
                    }
                }
                self.party.sync_from_roles(roles);
            }
        }
        for (role_id, slot) in removed_slots {
            self.equipment_effects
                .retain(|&(effect_slot, effect_role, _), _| {
                    usize::from(effect_slot) != slot || effect_role != role_id
                });
        }
        remaining == 0 || insufficient_entry == 0
    }

    fn set_equipment_effect(
        &mut self,
        role_id: u16,
        slot: u16,
        attribute: u16,
        value: i16,
    ) -> bool {
        if usize::from(slot) >= pal_assets::player_roles::PLAYER_EQUIPMENT_COUNT
            || self.player_role(role_id).is_none()
            || !valid_role_attribute(attribute)
        {
            return false;
        }
        let key = (slot, role_id, attribute);
        if value == 0 {
            self.equipment_effects.remove(&key);
        } else {
            self.equipment_effects.insert(key, value);
        }
        true
    }

    fn equip_item(&mut self, role_id: u16, slot: u16, item_id: u16) -> bool {
        let slot_index = usize::from(slot);
        if slot_index >= pal_assets::player_roles::PLAYER_EQUIPMENT_COUNT
            || self.player_role(role_id).is_none()
        {
            return false;
        }
        let old_item = self.player_role(role_id).unwrap().equipment[slot_index];
        if old_item != item_id && (item_id == 0 || self.inventory_count(item_id) == 0) {
            return false;
        }

        self.equipment_effects
            .retain(|&(effect_slot, effect_role, _), _| {
                effect_slot != slot || effect_role != role_id
            });
        self.current_equipment_slot = Some(slot);

        if old_item != item_id {
            let replacing_only_copy = self.inventory_count(item_id) == 1
                && old_item != 0
                && self.inventory_count(old_item) == 0;
            if replacing_only_copy {
                let Some(index) = self.inventory.iter().position(|&(id, _)| id == item_id) else {
                    return false;
                };
                self.inventory[index] = (old_item, 1);
            } else {
                if !self.consume_inventory_item(item_id) {
                    return false;
                }
                if old_item != 0
                    && !self.set_inventory_amount(
                        old_item,
                        self.inventory_count(old_item).saturating_add(1),
                    )
                {
                    return false;
                }
            }
            let Some(roles) = self.player_roles.as_mut() else {
                return false;
            };
            let Some(role) = roles.role_mut(usize::from(role_id)) else {
                return false;
            };
            role.equipment[slot_index] = item_id;
            self.party.sync_from_roles(roles);
        }
        true
    }

    fn remove_equipment(&mut self, role_id: u16, slot: Option<u16>) -> bool {
        let slots = match slot {
            Some(slot) if usize::from(slot) < pal_assets::player_roles::PLAYER_EQUIPMENT_COUNT => {
                usize::from(slot)..usize::from(slot) + 1
            }
            Some(_) => return false,
            None => 0..pal_assets::player_roles::PLAYER_EQUIPMENT_COUNT,
        };
        let Some(role) = self.player_role(role_id) else {
            return false;
        };
        let removed = slots
            .clone()
            .map(|index| (index, role.equipment[index]))
            .collect::<Vec<_>>();
        for &(index, item_id) in &removed {
            if item_id != 0
                && !self
                    .set_inventory_amount(item_id, self.inventory_count(item_id).saturating_add(1))
            {
                return false;
            }
            self.equipment_effects
                .retain(|&(effect_slot, effect_role, _), _| {
                    usize::from(effect_slot) != index || effect_role != role_id
                });
        }
        let Some(roles) = self.player_roles.as_mut() else {
            return false;
        };
        let Some(role) = roles.role_mut(usize::from(role_id)) else {
            return false;
        };
        for (index, _) in removed {
            role.equipment[index] = 0;
        }
        self.party.sync_from_roles(roles);
        true
    }

    fn change_magic(&mut self, role_id: u16, magic_id: u16, add: bool) -> bool {
        if magic_id == 0 {
            return false;
        }
        let Some(roles) = self.player_roles.as_mut() else {
            return false;
        };
        let Some(role) = roles.role_mut(usize::from(role_id)) else {
            return false;
        };
        if add {
            if !role.magic.contains(&magic_id) {
                let Some(slot) = role.magic.iter_mut().find(|magic| **magic == 0) else {
                    return false;
                };
                *slot = magic_id;
            }
        } else {
            for magic in &mut role.magic {
                if *magic == magic_id {
                    *magic = 0;
                }
            }
        }
        self.party.sync_from_roles(roles);
        true
    }

    pub fn object_state(&self, object_id: u16) -> Option<i16> {
        self.scene_objects
            .iter()
            .find(|object| object.id == object_id)
            .or_else(|| self.inactive_objects.get(&object_id))
            .map(|object| object.state)
    }

    pub fn objects_within_zone(&self, object_id: u16, target_id: u16, range: u16) -> bool {
        let Some(object) = self
            .scene_objects
            .iter()
            .find(|object| object.id == object_id)
        else {
            return false;
        };
        let Some(target) = self
            .scene_objects
            .iter()
            .find(|object| object.id == target_id)
        else {
            return false;
        };
        let x = i64::from(target.world_x) - i64::from(object.world_x);
        let y = i64::from(target.world_y) - i64::from(object.world_y);
        x.abs() + y.abs() * 2 < i64::from(range) * 32 + 16
    }

    pub fn player_faces_object(&mut self, object_id: u16, range: u16) -> bool {
        let Some(index) = self
            .scene_objects
            .iter()
            .position(|object| object.id == object_id)
        else {
            return false;
        };
        let (x_offset, y_offset) = (
            if matches!(self.player.direction, Direction::West | Direction::South) {
                16
            } else {
                -16
            },
            if matches!(self.player.direction, Direction::West | Direction::North) {
                8
            } else {
                -8
            },
        );
        let object = &mut self.scene_objects[index];
        let x = object.world_x + x_offset - self.player.world_x;
        let y = object.world_y + y_offset - self.player.world_y;
        let within_range = i64::from(x).abs() + i64::from(y).abs() * 2 < i64::from(range) * 32 + 16;
        if !within_range || object.state <= 0 {
            return false;
        }
        if range > 0 {
            object.trigger_mode = 5u16.saturating_add(range);
        }
        true
    }

    /// Place a current-scene object one logical movement step in front of the player.
    pub fn place_object_in_front(&mut self, object_id: u16, state: i16) -> bool {
        let Some(index) = self
            .scene_objects
            .iter()
            .position(|object| object.id == object_id)
        else {
            return false;
        };
        let (dx, dy) = self.player.direction.step();
        let target = (self.player.world_x + dx, self.player.world_y + dy);
        let blocked_by_object = self
            .scene_objects
            .iter()
            .enumerate()
            .filter(|(candidate, _)| *candidate != index)
            .any(|(_, object)| {
                object.is_blocker()
                    && u64::from(object.world_x.abs_diff(target.0))
                        + u64::from(object.world_y.abs_diff(target.1)) * 2
                        < 16
            });
        if self.map.is_world_blocked(target.0, target.1) || blocked_by_object {
            return false;
        }
        let object = &mut self.scene_objects[index];
        object.world_x = target.0;
        object.world_y = target.1;
        object.state = state;
        true
    }

    pub fn replace_scene(&mut self, scene_number: u16, map: M, scene_objects: Vec<SceneObject>) {
        for object in std::mem::take(&mut self.scene_objects) {
            self.inactive_objects.insert(object.id, object);
        }
        self.scene_number = scene_number;
        self.map = map;
        self.scene_objects = scene_objects
            .into_iter()
            .map(|fresh| self.inactive_objects.remove(&fresh.id).unwrap_or(fresh))
            .collect();
        self.pending_trigger = None;
        self.viewport_locked = false;
        self.party_screen_position = DEFAULT_PARTY_SCREEN_POSITION;
        self.follow_player();
    }

    /// Resolve and remember the mutable enter-script entry for the current scene.
    pub fn scene_enter_script(&mut self, default_entry: u16) -> u16 {
        *self
            .scene_enter_scripts
            .entry(self.scene_number)
            .or_insert(default_entry)
    }

    pub fn update_scene_enter_script(&mut self, next_entry: u16) {
        self.update_scene_enter_script_for(self.scene_number, next_entry);
    }

    /// Persist an enter-script entry for a scene whose trigger is completing
    /// while a later scene number is already logically selected.
    pub fn update_scene_enter_script_for(&mut self, scene_number: u16, next_entry: u16) {
        self.scene_enter_scripts.insert(scene_number, next_entry);
    }

    pub fn scene_teleport_script(&mut self, default_entry: u16) -> u16 {
        *self
            .scene_teleport_scripts
            .entry(self.scene_number)
            .or_insert(default_entry)
    }

    pub fn scene_map_override(&self, scene_number: u16) -> Option<u16> {
        self.scene_maps.get(&scene_number).copied()
    }

    /// Logical depth offset applied to all party sprites by script opcode 0x006E.
    pub fn party_layer(&self) -> u16 {
        self.save_layer
    }

    pub fn set_party_layer(&mut self, layer: u16) {
        self.save_layer = layer;
    }

    pub fn replace_map(&mut self, map: M) {
        self.map = map;
        self.follow_player();
    }

    /// Build a complete original-format save from the current mutable game state.
    pub fn original_save(
        &self,
        saved_times: u16,
        night_palette: bool,
        screen_wave: u16,
    ) -> Option<OriginalSave> {
        if self.active_battle.is_some() || self.party.members().is_empty() {
            return None;
        }
        let player_roles = self.player_roles.clone()?;
        let mut objects = self.global_objects.clone()?;
        let mut scenes = self.save_scenes?;

        for (&scene_number, &map_num) in &self.scene_maps {
            scenes
                .get_mut(usize::from(scene_number.checked_sub(1)?))?
                .map_num = map_num;
        }
        for (&scene_number, &entry) in &self.scene_enter_scripts {
            scenes
                .get_mut(usize::from(scene_number.checked_sub(1)?))?
                .script_on_enter = entry;
        }
        for (&scene_number, &entry) in &self.scene_teleport_scripts {
            scenes
                .get_mut(usize::from(scene_number.checked_sub(1)?))?
                .script_on_teleport = entry;
        }

        for (&object_id, &entry) in &self.item_use_scripts {
            objects.get_mut(object_id)?.data[2] = entry;
        }
        for (&object_id, &entry) in &self.item_equip_scripts {
            objects.get_mut(object_id)?.data[3] = entry;
        }
        for (&object_id, &entry) in &self.item_throw_scripts {
            objects.get_mut(object_id)?.data[4] = entry;
        }
        for (&object_id, &entry) in &self.magic_success_scripts {
            objects.get_mut(object_id)?.data[2] = entry;
        }
        for (&object_id, &entry) in &self.magic_use_scripts {
            objects.get_mut(object_id)?.data[3] = entry;
        }
        for (&(object_id, field), &entry) in &self.object_script_overrides {
            let field = usize::from(field).checked_add(2)?;
            *objects.get_mut(object_id)?.data.get_mut(field)? = entry;
        }

        let role_ids = self
            .party
            .members()
            .iter()
            .map(|member| member.role_id)
            .chain(self.extra_follower_ids.iter().copied())
            .collect::<Vec<_>>();
        let party_roles = std::iter::once(&self.player)
            .chain(self.party_followers.iter())
            .collect::<Vec<_>>();
        if role_ids.len() != party_roles.len()
            || role_ids.len() > SAVE_PARTY_CAPACITY
            || self.party.members().len() > SAVE_PARTY_CAPACITY
        {
            return None;
        }
        let mut party = [SavePartyMember {
            role_id: 0,
            x: 0,
            y: 0,
            frame: 0,
            image_offset: 0,
        }; SAVE_PARTY_CAPACITY];
        for (slot, (&role_id, role)) in party
            .iter_mut()
            .zip(role_ids.iter().zip(party_roles.iter()))
        {
            if usize::from(role_id) >= SAVE_ROLE_COUNT {
                return None;
            }
            *slot = SavePartyMember {
                role_id,
                x: i16::try_from(role.world_x.checked_sub(self.camera.x)?).ok()?,
                y: i16::try_from(role.world_y.checked_sub(self.camera.y)?).ok()?,
                frame: u16::try_from(role.frame_index()).ok()?,
                image_offset: 0,
            };
        }
        let trail = self.party_trail.map(|point| {
            Some(SaveTrail {
                x: i16::try_from(point.world_x).ok()?,
                y: i16::try_from(point.world_y).ok()?,
                direction: point.direction as u16,
            })
        });
        let trail = trail
            .into_iter()
            .collect::<Option<Vec<_>>>()?
            .try_into()
            .ok()?;

        let mut experience = self.save_experience;
        for (role_id, primary) in experience[0].iter_mut().enumerate() {
            primary.experience = u16::try_from(self.role_experience[role_id]).unwrap_or(u16::MAX);
            primary.level = player_roles.role(role_id)?.level;
        }
        let mut poisons = [[SavePoison {
            poison_id: 0,
            script: 0,
        }; SAVE_PARTY_CAPACITY]; SAVE_POISON_SLOTS];
        for (party_index, member) in self.party.members().iter().enumerate() {
            for (poison_index, poison) in self.player_poisons[usize::from(member.role_id)]
                .iter()
                .enumerate()
                .take(SAVE_POISON_SLOTS)
            {
                poisons[poison_index][party_index] = SavePoison {
                    poison_id: poison.object_id,
                    script: poison.script_entry,
                };
            }
        }
        if self.inventory.len() > SAVE_INVENTORY_CAPACITY {
            return None;
        }
        let mut inventory = [SaveInventoryEntry {
            item_id: 0,
            amount: 0,
            amount_in_use: 0,
        }; SAVE_INVENTORY_CAPACITY];
        for (slot, &(item_id, amount)) in inventory.iter_mut().zip(&self.inventory) {
            *slot = SaveInventoryEntry {
                item_id,
                amount,
                amount_in_use: 0,
            };
        }

        let mut runtime_objects = vec![None; self.save_event_objects.len()];
        for object in self
            .scene_objects
            .iter()
            .chain(self.inactive_objects.values())
        {
            let index = usize::from(object.id.checked_sub(1)?);
            let slot = runtime_objects.get_mut(index)?;
            if slot.replace(object).is_some() {
                return None;
            }
        }
        let event_objects = self
            .save_event_objects
            .iter()
            .copied()
            .zip(runtime_objects)
            .map(|(mut saved, runtime)| {
                let runtime = runtime?;
                saved.vanish_time = runtime.vanish_time;
                saved.x = u16::try_from(runtime.world_x.rem_euclid(1 << 16)).ok()?;
                saved.y = u16::try_from(runtime.world_y.rem_euclid(1 << 16)).ok()?;
                saved.layer = runtime.layer;
                saved.trigger_script = runtime.trigger_script;
                saved.auto_script = runtime.auto_script;
                saved.state = runtime.state;
                saved.trigger_mode = runtime.trigger_mode;
                saved.sprite_num = u16::try_from(runtime.sprite_index.unwrap_or(0)).ok()?;
                saved.sprite_frames = runtime.frames_per_direction;
                saved.direction = runtime.direction as u16;
                saved.current_frame = runtime.current_frame;
                saved.auto_script_idle_frame = runtime.auto_script_idle_frame;
                Some(saved)
            })
            .collect::<Option<Vec<_>>>()?;
        let scene_index = usize::from(self.scene_number.checked_sub(1)?);
        let (scene, next_scene) = (scenes.get(scene_index)?, scenes.get(scene_index + 1)?);
        if scene.event_object_index > next_scene.event_object_index
            || usize::from(next_scene.event_object_index) > event_objects.len()
        {
            return None;
        }

        Some(OriginalSave {
            layout: objects.layout(),
            saved_times,
            viewport_x: i16::try_from(self.camera.x).ok()?,
            viewport_y: i16::try_from(self.camera.y).ok()?,
            party_member_index: u16::try_from(self.party.members().len().checked_sub(1)?).ok()?,
            scene_number: self.scene_number,
            night_palette,
            party_direction: self.player.direction as u16,
            music_number: self.current_music.unwrap_or(0),
            battle_music_number: self.current_battle_music,
            battlefield_number: self.current_battlefield,
            screen_wave,
            battle_speed: self.save_battle_speed,
            collect_value: self.collect_value,
            layer: self.save_layer,
            chase_range: self.chase_range,
            chase_speed_change_cycles: self.chase_speed_change_cycles,
            follower_count: u16::try_from(self.extra_follower_ids.len()).ok()?,
            cash: self.cash,
            party,
            trail,
            experience,
            player_roles,
            poisons,
            inventory,
            scenes,
            objects,
            event_objects,
        })
    }

    pub fn snapshot(&self) -> GameSnapshot {
        GameSnapshot {
            scene_number: self.scene_number,
            player: self.player.clone(),
            scene_objects: self.scene_objects.clone(),
            pending_trigger: self.pending_trigger,
            party: self.party.clone(),
            current_music: self.current_music,
            current_battle_music: self.current_battle_music,
            current_battlefield: self.current_battlefield,
            role_experience: self.role_experience,
            growth_random_state: self.growth_random_state,
            player_statuses: self.player_statuses,
            player_poisons: self.player_poisons,
            collect_value: self.collect_value,
            cash: self.cash,
            inventory: self.inventory.clone(),
            item_use_scripts: self.item_use_scripts.clone(),
            item_equip_scripts: self.item_equip_scripts.clone(),
            item_throw_scripts: self.item_throw_scripts.clone(),
            magic_use_scripts: self.magic_use_scripts.clone(),
            magic_success_scripts: self.magic_success_scripts.clone(),
            object_script_overrides: self.object_script_overrides.clone(),
            equipment_effects: self.equipment_effects.clone(),
            inactive_objects: self.inactive_objects.clone(),
            scene_enter_scripts: self.scene_enter_scripts.clone(),
            scene_teleport_scripts: self.scene_teleport_scripts.clone(),
            scene_maps: self.scene_maps.clone(),
            script_frame: self.script_frame,
            chase_range: self.chase_range,
            chase_speed_change_cycles: self.chase_speed_change_cycles,
            viewport_locked: self.viewport_locked,
            camera_x: self.camera.x,
            camera_y: self.camera.y,
            party_followers: self.party_followers.clone(),
            extra_follower_ids: self.extra_follower_ids.clone(),
            party_slots: self.current_party_slots(),
            party_trail: self.party_trail,
            player_roles: self.player_roles.clone(),
        }
    }

    pub fn encode_snapshot(&self) -> Option<Vec<u8>> {
        let snapshot = self.snapshot();
        let data = SnapshotData {
            version: SNAPSHOT_VERSION,
            scene_number: snapshot.scene_number,
            player: SavedRole::from(&snapshot.player),
            scene_objects: snapshot
                .scene_objects
                .iter()
                .map(SavedSceneObject::from)
                .collect(),
            party: snapshot
                .party
                .members()
                .iter()
                .map(|member| member.role_id)
                .collect(),
            current_music: snapshot.current_music,
            current_battle_music: snapshot.current_battle_music,
            current_battlefield: snapshot.current_battlefield,
            role_experience: snapshot.role_experience,
            growth_random_state: snapshot.growth_random_state,
            player_statuses: snapshot.player_statuses.map(BattleStatuses::durations),
            player_poisons: snapshot
                .player_poisons
                .map(|poisons| poisons.map(|poison| (poison.object_id, poison.script_entry))),
            collect_value: snapshot.collect_value,
            cash: snapshot.cash,
            inventory: snapshot.inventory,
            item_use_scripts: snapshot.item_use_scripts.into_iter().collect(),
            item_equip_scripts: snapshot.item_equip_scripts.into_iter().collect(),
            item_throw_scripts: snapshot.item_throw_scripts.into_iter().collect(),
            magic_use_scripts: snapshot.magic_use_scripts.into_iter().collect(),
            magic_success_scripts: snapshot.magic_success_scripts.into_iter().collect(),
            object_script_overrides: snapshot
                .object_script_overrides
                .into_iter()
                .map(|((object_id, field), entry)| (object_id, field, entry))
                .collect(),
            equipment_effects: snapshot
                .equipment_effects
                .into_iter()
                .map(|((slot, role, attribute), value)| (slot, role, attribute, value))
                .collect(),
            inactive_objects: snapshot
                .inactive_objects
                .values()
                .map(SavedSceneObject::from)
                .collect(),
            scene_enter_scripts: snapshot.scene_enter_scripts.into_iter().collect(),
            scene_teleport_scripts: snapshot.scene_teleport_scripts.into_iter().collect(),
            scene_maps: snapshot.scene_maps.into_iter().collect(),
            script_frame: snapshot.script_frame,
            chase_range: snapshot.chase_range,
            chase_speed_change_cycles: snapshot.chase_speed_change_cycles,
            viewport_locked: snapshot.viewport_locked,
            camera_x: snapshot.camera_x,
            camera_y: snapshot.camera_y,
            party_followers: snapshot
                .party_followers
                .iter()
                .map(SavedRole::from)
                .collect(),
            extra_follower_ids: snapshot.extra_follower_ids,
            party_slots: snapshot
                .party_slots
                .iter()
                .map(SavedPartySlot::from)
                .collect(),
            party_trail: snapshot
                .party_trail
                .iter()
                .map(SavedTrailPoint::from)
                .collect(),
            player_roles: snapshot
                .player_roles
                .as_ref()?
                .iter()
                .map(SavedPlayerRole::from)
                .collect(),
        };
        serde_json::to_vec(&data).ok()
    }

    pub fn decode_snapshot(&self, bytes: &[u8]) -> Option<GameSnapshot> {
        const MAX_SNAPSHOT_BYTES: usize = 4 * 1024 * 1024;
        const MAX_SCENES: usize = 512;
        const MAX_OBJECTS: usize = 8192;
        const MAX_INVENTORY: usize = 1024;

        if bytes.len() > MAX_SNAPSHOT_BYTES {
            return None;
        }
        let data: SnapshotData = serde_json::from_slice(bytes).ok()?;
        if data.version != SNAPSHOT_VERSION
            || data.scene_number == 0
            || data.scene_enter_scripts.len() > MAX_SCENES
            || data.scene_teleport_scripts.len() > MAX_SCENES
            || data.scene_maps.len() > MAX_SCENES
            || data.inventory.len() > MAX_INVENTORY
            || data.item_use_scripts.len() > MAX_INVENTORY
            || data.item_equip_scripts.len() > MAX_INVENTORY
            || data.item_throw_scripts.len() > MAX_INVENTORY
            || data.magic_use_scripts.len() > MAX_INVENTORY
            || data.magic_success_scripts.len() > MAX_INVENTORY
            || data.object_script_overrides.len() > MAX_OBJECTS * 3
            || data.equipment_effects.len() > MAX_INVENTORY
            || data.scene_objects.len() > MAX_OBJECTS
            || data.inactive_objects.len() > MAX_OBJECTS
            || data.scene_objects.len() + data.inactive_objects.len() > MAX_OBJECTS
        {
            return None;
        }
        let inactive_object_count = data.inactive_objects.len();
        let scene_enter_script_count = data.scene_enter_scripts.len();
        let scene_teleport_script_count = data.scene_teleport_scripts.len();
        let scene_map_count = data.scene_maps.len();
        let saved_roles: [PlayerRole; PLAYER_ROLE_COUNT] = data
            .player_roles
            .into_iter()
            .map(SavedPlayerRole::into_role)
            .collect::<Vec<_>>()
            .try_into()
            .ok()?;
        let player_roles = PlayerRoles::from_roles(saved_roles);
        if data.player_poisons.iter().any(|poisons| {
            poisons
                .iter()
                .enumerate()
                .any(|(index, &(object_id, script_entry))| {
                    (object_id == 0 && script_entry != 0)
                        || (object_id != 0
                            && poisons[..index]
                                .iter()
                                .any(|&(other_id, _)| other_id == object_id))
                })
        }) {
            return None;
        }
        let player_statuses = data.player_statuses.map(BattleStatuses::from_durations);
        let player_poisons = data.player_poisons.map(|poisons| {
            poisons.map(|(object_id, script_entry)| BattlePoison {
                object_id,
                script_entry,
            })
        });
        let mut party = Party::default();
        if !party.replace(&data.party, &player_roles) {
            return None;
        }
        let party_followers = data
            .party_followers
            .into_iter()
            .map(SavedRole::into_role)
            .collect::<Option<Vec<_>>>()?;
        if data.extra_follower_ids.len() > 2
            || party.members().len() + data.extra_follower_ids.len() > MAX_PARTY_MEMBERS
            || data.extra_follower_ids.iter().any(|&role_id| {
                usize::from(role_id) >= PLAYER_ROLE_COUNT
                    || party
                        .members()
                        .iter()
                        .any(|member| member.role_id == role_id)
            })
            || party_followers.len()
                != party.members().len().saturating_sub(1) + data.extra_follower_ids.len()
        {
            return None;
        }
        let party_trail = data
            .party_trail
            .into_iter()
            .map(SavedTrailPoint::into_point)
            .collect::<Option<Vec<_>>>()?
            .try_into()
            .ok()?;
        let party_slots = data
            .party_slots
            .into_iter()
            .map(SavedPartySlot::into_slot)
            .collect::<Option<Vec<_>>>()?
            .try_into()
            .ok()?;
        let inventory = data.inventory;
        if inventory.len() > MAX_INVENTORY
            || inventory
                .iter()
                .any(|&(id, amount)| id == 0 || amount == 0 || amount > 99)
            || inventory
                .iter()
                .enumerate()
                .any(|(index, &(id, _))| inventory[..index].iter().any(|&(other, _)| other == id))
        {
            return None;
        }
        let item_use_script_count = data.item_use_scripts.len();
        let item_use_scripts = data
            .item_use_scripts
            .into_iter()
            .collect::<BTreeMap<_, _>>();
        if item_use_scripts.len() != item_use_script_count
            || item_use_scripts.keys().any(|&item_id| item_id == 0)
        {
            return None;
        }
        let item_equip_scripts = unique_script_entries(data.item_equip_scripts)?;
        let item_throw_scripts = unique_script_entries(data.item_throw_scripts)?;
        let magic_use_scripts = unique_script_entries(data.magic_use_scripts)?;
        let magic_success_scripts = unique_script_entries(data.magic_success_scripts)?;
        let object_script_override_count = data.object_script_overrides.len();
        let object_script_overrides = data
            .object_script_overrides
            .into_iter()
            .map(|(object_id, field, entry)| ((object_id, field), entry))
            .collect::<BTreeMap<_, _>>();
        if object_script_overrides.len() != object_script_override_count
            || object_script_overrides.keys().any(|&(object_id, field)| {
                field > 2
                    || self
                        .global_objects
                        .as_ref()
                        .and_then(|objects| objects.get(object_id))
                        .is_none()
            })
        {
            return None;
        }
        let equipment_effect_count = data.equipment_effects.len();
        let equipment_effects = data
            .equipment_effects
            .into_iter()
            .map(|(slot, role, attribute, value)| ((slot, role, attribute), value))
            .collect::<BTreeMap<_, _>>();
        if equipment_effects.len() != equipment_effect_count
            || equipment_effects.keys().any(|&(slot, role, attribute)| {
                usize::from(slot) >= pal_assets::player_roles::PLAYER_EQUIPMENT_COUNT
                    || usize::from(role) >= pal_assets::player_roles::PLAYER_ROLE_COUNT
                    || !valid_role_attribute(attribute)
            })
        {
            return None;
        }
        let inactive_objects = data
            .inactive_objects
            .into_iter()
            .map(|object| {
                let object = object.into_object()?;
                Some((object.id, object))
            })
            .collect::<Option<BTreeMap<_, _>>>()?;
        let scene_enter_scripts = data
            .scene_enter_scripts
            .into_iter()
            .collect::<BTreeMap<_, _>>();
        let scene_teleport_scripts = data
            .scene_teleport_scripts
            .into_iter()
            .collect::<BTreeMap<_, _>>();
        let scene_maps = data.scene_maps.into_iter().collect::<BTreeMap<_, _>>();
        if inactive_objects.len() != inactive_object_count
            || scene_enter_scripts.len() != scene_enter_script_count
            || scene_teleport_scripts.len() != scene_teleport_script_count
            || scene_maps.len() != scene_map_count
            || scene_enter_scripts.contains_key(&0)
            || scene_teleport_scripts.contains_key(&0)
            || scene_maps
                .iter()
                .any(|(&scene, &map)| scene == 0 || map == 0)
        {
            return None;
        }
        Some(GameSnapshot {
            scene_number: data.scene_number,
            player: data.player.into_role()?,
            scene_objects: data
                .scene_objects
                .into_iter()
                .map(SavedSceneObject::into_object)
                .collect::<Option<_>>()?,
            pending_trigger: None,
            party,
            current_music: data.current_music,
            current_battle_music: data.current_battle_music,
            current_battlefield: data.current_battlefield,
            role_experience: data.role_experience,
            growth_random_state: data.growth_random_state,
            player_statuses,
            player_poisons,
            collect_value: data.collect_value,
            cash: data.cash,
            inventory,
            item_use_scripts,
            item_equip_scripts,
            item_throw_scripts,
            magic_use_scripts,
            magic_success_scripts,
            object_script_overrides,
            equipment_effects,
            inactive_objects,
            scene_enter_scripts,
            scene_teleport_scripts,
            scene_maps,
            script_frame: data.script_frame,
            chase_range: data.chase_range,
            chase_speed_change_cycles: data.chase_speed_change_cycles,
            viewport_locked: data.viewport_locked,
            camera_x: data.camera_x,
            camera_y: data.camera_y,
            party_followers,
            extra_follower_ids: data.extra_follower_ids,
            party_slots,
            party_trail,
            player_roles: Some(player_roles),
        })
    }

    /// Restore a snapshot after the platform layer loads its scene map.
    pub fn restore_snapshot(&mut self, snapshot: GameSnapshot, map: M) {
        self.scene_number = snapshot.scene_number;
        self.map = map;
        self.player = snapshot.player;
        self.scene_objects = snapshot.scene_objects;
        self.pending_trigger = snapshot.pending_trigger;
        self.party = snapshot.party;
        self.current_music = snapshot.current_music;
        self.current_battle_music = snapshot.current_battle_music;
        self.current_battlefield = snapshot.current_battlefield;
        self.role_experience = snapshot.role_experience;
        self.growth_random_state = snapshot.growth_random_state;
        self.player_statuses = snapshot.player_statuses;
        self.player_poisons = snapshot.player_poisons;
        self.collect_value = snapshot.collect_value;
        self.active_battle = None;
        self.auto_battle = false;
        self.cash = snapshot.cash;
        self.inventory = snapshot.inventory;
        self.item_use_scripts = snapshot.item_use_scripts;
        self.item_equip_scripts = snapshot.item_equip_scripts;
        self.item_throw_scripts = snapshot.item_throw_scripts;
        self.magic_use_scripts = snapshot.magic_use_scripts;
        self.magic_success_scripts = snapshot.magic_success_scripts;
        self.object_script_overrides = snapshot.object_script_overrides;
        self.equipment_effects = snapshot.equipment_effects;
        self.current_equipment_slot = None;
        self.inactive_objects = snapshot.inactive_objects;
        self.scene_enter_scripts = snapshot.scene_enter_scripts;
        self.scene_teleport_scripts = snapshot.scene_teleport_scripts;
        self.scene_maps = snapshot.scene_maps;
        self.script_frame = snapshot.script_frame;
        self.chase_range = snapshot.chase_range;
        self.chase_speed_change_cycles = snapshot.chase_speed_change_cycles;
        self.viewport_locked = snapshot.viewport_locked;
        self.camera.x = snapshot.camera_x;
        self.camera.y = snapshot.camera_y;
        self.party_screen_position = (
            self.player.world_x - self.camera.x,
            self.player.world_y - self.camera.y,
        );
        self.party_followers = snapshot.party_followers;
        self.extra_follower_ids = snapshot.extra_follower_ids;
        self.party_slots = snapshot.party_slots;
        self.party_trail = snapshot.party_trail;
        self.player_roles = snapshot.player_roles;
        self.pending_auto_sounds.clear();
        self.pending_auto_events.clear();
        self.pending_auto_script_failure = false;
        self.auto_instruction_runtime = None;
        self.follow_player();
    }

    /// Restore mutable game state carried by an original DOS or Win95 `.rpg` save.
    ///
    /// The platform layer supplies render-ready event objects because validating
    /// their sprite frame counts requires `MGO.MKF`.
    pub fn restore_original_save(
        &mut self,
        save: OriginalSave,
        map: M,
        all_event_objects: Vec<SceneObject>,
    ) -> bool {
        if all_event_objects.len() != save.event_objects.len() {
            return false;
        }
        let save_scenes = save.scenes;
        let save_event_objects = save.event_objects.clone();
        let save_experience = save.experience;
        let save_battle_speed = save.battle_speed;
        let save_layer = save.layer;
        let scene_index = match usize::from(save.scene_number).checked_sub(1) {
            Some(index) => index,
            None => return false,
        };
        let (Some(scene), Some(next_scene)) = (
            save.scenes.get(scene_index),
            save.scenes.get(scene_index + 1),
        ) else {
            return false;
        };
        let active_start = usize::from(scene.event_object_index);
        let active_end = usize::from(next_scene.event_object_index);
        if active_start > active_end || active_end > all_event_objects.len() {
            return false;
        }

        let role_ids = save.party[..save.party_member_count()]
            .iter()
            .map(|member| member.role_id)
            .collect::<Vec<_>>();
        let extra_follower_ids = save.party[save.party_member_count()
            ..save.party_member_count() + usize::from(save.follower_count)]
            .iter()
            .map(|member| member.role_id)
            .collect::<Vec<_>>();
        if extra_follower_ids
            .iter()
            .any(|role_id| role_ids.contains(role_id))
        {
            return false;
        }
        let mut party = Party::default();
        if !party.replace(&role_ids, &save.player_roles) {
            return false;
        }
        let Some(direction) = Direction::from_pal(save.party_direction) else {
            return false;
        };
        let Some(leader) = save.player_roles.role(usize::from(role_ids[0])) else {
            return false;
        };
        let mut player = Role {
            sprite_index: usize::from(leader.scene_sprite_num),
            world_x: i32::from(save.viewport_x) + i32::from(save.party[0].x),
            world_y: i32::from(save.viewport_y) + i32::from(save.party[0].y),
            direction,
            anim_frame: 0,
            frames_per_direction: leader.frames_per_direction(),
        };
        let mut party_slots = [PartySlotState::from_role(&player); MAX_PARTY_MEMBERS];
        for (slot, member) in party_slots.iter_mut().zip(&save.party) {
            let frames_per_direction = save
                .player_roles
                .role(usize::from(member.role_id))
                .map_or(3, PlayerRole::frames_per_direction)
                .max(1);
            let absolute_frame = usize::from(member.frame);
            let saved_direction = u16::try_from(absolute_frame / usize::from(frames_per_direction))
                .ok()
                .and_then(Direction::from_pal)
                .unwrap_or(direction);
            *slot = PartySlotState {
                world_x: i32::from(save.viewport_x) + i32::from(member.x),
                world_y: i32::from(save.viewport_y) + i32::from(member.y),
                direction: saved_direction,
                anim_frame: (absolute_frame % usize::from(frames_per_direction)) as u8,
            };
        }
        party_slots[0].apply_to(&mut player);
        let trail = match save
            .trail
            .iter()
            .map(|point| {
                Some(TrailPoint {
                    world_x: i32::from(point.x),
                    world_y: i32::from(point.y),
                    direction: Direction::from_pal(point.direction)?,
                })
            })
            .take(MAX_PARTY_MEMBERS)
            .collect::<Option<Vec<_>>>()
            .and_then(|points| points.try_into().ok())
        {
            Some(trail) => trail,
            None => return false,
        };

        let mut active_objects = Vec::with_capacity(active_end - active_start);
        let mut inactive_objects = BTreeMap::new();
        for (index, object) in all_event_objects.into_iter().enumerate() {
            if (active_start..active_end).contains(&index) {
                active_objects.push(object);
            } else {
                inactive_objects.insert(object.id, object);
            }
        }
        let inventory = save
            .inventory
            .iter()
            .filter(|entry| entry.item_id != 0 && entry.amount != 0)
            .map(|entry| (entry.item_id, entry.amount))
            .collect::<Vec<_>>();
        if inventory.len() > MAX_INVENTORY {
            return false;
        }
        let scene_enter_scripts = save
            .scenes
            .iter()
            .enumerate()
            .filter(|(_, scene)| scene.script_on_enter != 0)
            .filter_map(|(index, scene)| {
                Some((
                    u16::try_from(index).ok()?.checked_add(1)?,
                    scene.script_on_enter,
                ))
            })
            .collect();
        let scene_teleport_scripts = save
            .scenes
            .iter()
            .enumerate()
            .filter(|(_, scene)| scene.script_on_teleport != 0)
            .filter_map(|(index, scene)| {
                Some((
                    u16::try_from(index).ok()?.checked_add(1)?,
                    scene.script_on_teleport,
                ))
            })
            .collect();
        let scene_maps = save
            .scenes
            .iter()
            .enumerate()
            .filter(|(_, scene)| scene.map_num != 0)
            .filter_map(|(index, scene)| {
                Some((u16::try_from(index).ok()?.checked_add(1)?, scene.map_num))
            })
            .collect();
        let role_experience =
            std::array::from_fn(|role| u32::from(save.experience[0][role].experience));
        let mut player_poisons = [[BattlePoison::default(); MAX_BATTLE_POISONS]; PLAYER_ROLE_COUNT];
        for (party_index, &role_id) in role_ids.iter().enumerate() {
            let role_index = usize::from(role_id);
            for poison_slot in 0..MAX_BATTLE_POISONS {
                let poison = save.poisons[poison_slot][party_index];
                if poison.poison_id == 0 {
                    if poison.script != 0 {
                        return false;
                    }
                    continue;
                }
                if save.objects.get(poison.poison_id).is_none()
                    || player_poisons[role_index][..poison_slot]
                        .iter()
                        .any(|other| other.object_id == poison.poison_id)
                {
                    return false;
                }
                player_poisons[role_index][poison_slot] = BattlePoison {
                    object_id: poison.poison_id,
                    script_entry: poison.script,
                };
            }
        }

        self.scene_number = save.scene_number;
        self.map = map;
        self.player = player;
        self.scene_objects = active_objects;
        self.pending_trigger = None;
        self.party = party;
        self.current_music = (save.music_number != 0).then_some(save.music_number);
        self.current_battle_music = save.battle_music_number;
        self.current_battlefield = save.battlefield_number;
        self.role_experience = role_experience;
        self.player_statuses =
            [BattleStatuses::from_durations([0; BATTLE_STATUS_COUNT]); PLAYER_ROLE_COUNT];
        self.player_poisons = player_poisons;
        self.collect_value = save.collect_value;
        self.player_roles = Some(save.player_roles);
        self.global_objects = Some(save.objects);
        self.active_battle = None;
        self.auto_battle = false;
        self.cash = save.cash;
        self.inventory = inventory;
        self.item_use_scripts.clear();
        self.item_equip_scripts.clear();
        self.item_throw_scripts.clear();
        self.magic_use_scripts.clear();
        self.magic_success_scripts.clear();
        self.object_script_overrides.clear();
        self.equipment_effects.clear();
        self.current_equipment_slot = None;
        self.inactive_objects = inactive_objects;
        self.scene_enter_scripts = scene_enter_scripts;
        self.scene_teleport_scripts = scene_teleport_scripts;
        self.scene_maps = scene_maps;
        self.pending_auto_sounds.clear();
        self.pending_auto_events.clear();
        self.pending_auto_script_failure = false;
        self.auto_instruction_runtime = None;
        self.script_frame = 0;
        self.chase_range = save.chase_range;
        self.chase_speed_change_cycles = save.chase_speed_change_cycles;
        self.viewport_locked = false;
        self.party_slots = party_slots;
        self.party_trail = trail;
        self.extra_follower_ids = extra_follower_ids;
        self.save_scenes = Some(save_scenes);
        self.save_event_objects = save_event_objects;
        self.save_experience = save_experience;
        self.save_battle_speed = save_battle_speed;
        self.save_layer = save_layer;
        self.camera.x = i32::from(save.viewport_x);
        self.camera.y = i32::from(save.viewport_y);
        self.party_screen_position = (
            self.player.world_x - self.camera.x,
            self.player.world_y - self.camera.y,
        );
        self.rebuild_party_followers();
        true
    }

    pub fn take_trigger(&mut self) -> Option<TriggerRequest> {
        self.pending_trigger.take()
    }
}

fn walk_scene_object_to(object: &mut SceneObject, target: (i32, i32), speed: i32) -> bool {
    let x_offset = target.0 - object.world_x;
    let y_offset = target.1 - object.world_y;
    object.direction = direction_toward(x_offset, y_offset);
    if x_offset.abs() < speed * 2 || y_offset.abs() < speed * 2 {
        object.world_x = target.0;
        object.world_y = target.1;
        object.current_frame = 0;
        true
    } else {
        let (dx, dy) = object.direction.step_at_speed(speed);
        object.world_x += dx;
        object.world_y += dy;
        object.advance_animation();
        false
    }
}

fn direction_toward(x_offset: i32, y_offset: i32) -> Direction {
    if y_offset < 0 {
        if x_offset < 0 {
            Direction::West
        } else {
            Direction::North
        }
    } else if x_offset < 0 {
        Direction::South
    } else {
        Direction::East
    }
}

fn apply_enemy_script_overrides(
    enemy: &mut BattleEnemy,
    object_script_overrides: &BTreeMap<(u16, u16), u16>,
    magic_use_scripts: &BTreeMap<u16, u16>,
    magic_success_scripts: &BTreeMap<u16, u16>,
    item_use_scripts: &BTreeMap<u16, u16>,
    apply_lifecycle: bool,
) {
    if apply_lifecycle {
        enemy.turn_start_script = object_script_overrides
            .get(&(enemy.object_id, 0))
            .copied()
            .unwrap_or(enemy.turn_start_script);
        enemy.battle_end_script = object_script_overrides
            .get(&(enemy.object_id, 1))
            .copied()
            .unwrap_or(enemy.battle_end_script);
        enemy.ready_script = object_script_overrides
            .get(&(enemy.object_id, 2))
            .copied()
            .unwrap_or(enemy.ready_script);
    }
    if let Some(magic) = enemy.magic.as_mut() {
        magic.use_script = magic_use_scripts
            .get(&magic.object_id)
            .copied()
            .unwrap_or(magic.use_script);
        magic.success_script = magic_success_scripts
            .get(&magic.object_id)
            .copied()
            .unwrap_or(magic.success_script);
    }
    enemy.attack_equivalent_item_script = item_use_scripts
        .get(&enemy.attack_equivalent_item)
        .copied()
        .unwrap_or(enemy.attack_equivalent_item_script);
}

fn poison_face_color(
    poisons: &[BattlePoison; MAX_BATTLE_POISONS],
    objects: &GlobalObjects,
) -> Option<u8> {
    poisons
        .iter()
        .filter(|poison| poison.object_id != 0)
        .filter_map(|poison| objects.get(poison.object_id).copied())
        .filter(|object| object.poison_level() <= 3)
        .max_by_key(|object| object.poison_level())
        .map(|object| object.poison_color() as u8)
}
