//! Deterministic exploration state and update rules.

use std::collections::BTreeMap;

use pal_assets::battle::BattleData;
use pal_assets::magic::Magics;
use pal_assets::objects::GlobalObjects;
use pal_assets::player_roles::{PlayerRole, PlayerRoles, PLAYER_ROLE_COUNT};
use pal_assets::save::OriginalSave;
use pal_assets::script::ScriptTable;
use pal_assets::store::Stores;

use crate::battle::{
    add_poison, cure_poison, cure_poison_by_level, BattleEvent, BattlePhase, BattlePoison,
    BattleRequest, BattleResult, BattleRewards, BattleScriptSource, BattleState, BattleStatus,
    BattleStatuses, BATTLE_STATUS_COUNT, MAX_BATTLE_POISONS,
};
use crate::map::tile_to_world;
use crate::map::Map;
use crate::party::{Party, MAX_PARTY_MEMBERS};
use crate::role::{Direction, Role};
use crate::scene::{
    blocks_position, find_search_trigger, find_touch_trigger, SceneObject, TriggerKind,
    TriggerRequest,
};
use crate::script::{ScriptAction, ScriptOpcode};

pub const UPDATE_INTERVAL_MS: u64 = 50;
mod exploration;
mod field;
mod snapshot;
mod state_support;

pub use exploration::{Camera, CollisionMap, GameInput};
pub use field::{EquippableItem, FieldMagic, StoreItem, UsableItem};
use field::{
    ITEM_FLAG_APPLY_TO_ALL, ITEM_FLAG_CONSUMING, ITEM_FLAG_EQUIPPABLE, ITEM_FLAG_ROLE_FIRST,
    ITEM_FLAG_SELLABLE, ITEM_FLAG_USABLE, MAGIC_FLAG_APPLY_TO_ALL,
    MAGIC_FLAG_USABLE_OUTSIDE_BATTLE, MAX_INVENTORY,
};
pub use snapshot::GameSnapshot;
use snapshot::{
    SavedPlayerRole, SavedRole, SavedSceneObject, SavedTrailPoint, SnapshotData, SNAPSHOT_VERSION,
};
pub use state_support::AutoScriptError;
use state_support::{apply_role_attribute, unique_script_entries, valid_role_attribute};

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
    player_statuses: [BattleStatuses; PLAYER_ROLE_COUNT],
    player_poisons: [[BattlePoison; MAX_BATTLE_POISONS]; PLAYER_ROLE_COUNT],
    collect_value: u16,
    inventory: Vec<(u16, u16)>,
    item_use_scripts: BTreeMap<u16, u16>,
    item_equip_scripts: BTreeMap<u16, u16>,
    magic_use_scripts: BTreeMap<u16, u16>,
    magic_success_scripts: BTreeMap<u16, u16>,
    equipment_effects: BTreeMap<(u16, u16, u16), i16>,
    current_equipment_slot: Option<u16>,
    inactive_objects: BTreeMap<u16, SceneObject>,
    scene_enter_scripts: BTreeMap<u16, u16>,
    scene_teleport_scripts: BTreeMap<u16, u16>,
    scene_maps: BTreeMap<u16, u16>,
    pending_auto_sounds: Vec<u16>,
    script_frame: u32,
    viewport_locked: bool,
    party_followers: Vec<Role>,
    extra_follower_ids: Vec<u16>,
    party_trail: [TrailPoint; MAX_PARTY_MEMBERS],
    pub camera: Camera,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrailPoint {
    world_x: i32,
    world_y: i32,
    direction: Direction,
}

impl<M: CollisionMap> GameState<M> {
    pub fn new(map: M, player: Role, viewport_width: u32, viewport_height: u32) -> Self {
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
            growth_random_state: 0xa341_316c,
            player_roles: None,
            stores: None,
            global_objects: None,
            magics: None,
            battle_data: None,
            active_battle: None,
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
            magic_use_scripts: BTreeMap::new(),
            magic_success_scripts: BTreeMap::new(),
            equipment_effects: BTreeMap::new(),
            current_equipment_slot: None,
            inactive_objects: BTreeMap::new(),
            scene_enter_scripts: BTreeMap::new(),
            scene_teleport_scripts: BTreeMap::new(),
            scene_maps: BTreeMap::new(),
            pending_auto_sounds: Vec::new(),
            script_frame: 0,
            viewport_locked: false,
            party_followers: Vec::new(),
            extra_follower_ids: Vec::new(),
            party_trail: [initial_trail; MAX_PARTY_MEMBERS],
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

    pub fn with_party(mut self, party: Party) -> Self {
        self.party = party;
        self.rebuild_party_followers();
        self
    }

    pub fn with_player_roles(mut self, player_roles: PlayerRoles) -> Self {
        self.player_roles = Some(player_roles);
        if let Some(roles) = &self.player_roles {
            self.party.sync_from_roles(roles);
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

    pub fn battle(&self) -> Option<&BattleState> {
        self.active_battle.as_ref()
    }

    pub fn battle_mut(&mut self) -> Option<&mut BattleState> {
        self.active_battle.as_mut()
    }

    pub fn is_enemy_turn(&self) -> bool {
        self.active_battle
            .as_ref()
            .is_some_and(BattleState::is_enemy_turn)
    }

    pub fn advance_battle_resolution(&mut self) -> Vec<BattleEvent> {
        self.active_battle
            .as_mut()
            .map(BattleState::advance_resolution)
            .unwrap_or_default()
    }

    pub fn take_battle_script(&mut self) -> Option<TriggerRequest> {
        let request = self.active_battle.as_mut()?.take_script_request()?;
        Some(TriggerRequest {
            object_id: request.object_id,
            script_entry: request.entry,
            kind: TriggerKind::Battle,
        })
    }

    pub fn finish_battle_script(&mut self, next_entry: u16, succeeded: bool) -> bool {
        let (source, updates) = {
            let Some(battle) = self.active_battle.as_mut() else {
                return false;
            };
            let Some(source) = battle.active_script_request().map(|request| request.source) else {
                return false;
            };
            if !battle.complete_script_with_result(next_entry, succeeded) {
                return false;
            }
            (
                source,
                battle
                    .players
                    .iter()
                    .map(|player| (player.role_id, player.statuses, player.poisons))
                    .collect::<Vec<_>>(),
            )
        };
        match source {
            BattleScriptSource::EnemyMagicUse { magic_object, .. } => {
                self.magic_use_scripts.insert(magic_object, next_entry);
            }
            BattleScriptSource::EnemyMagicSuccess { magic_object, .. } => {
                self.magic_success_scripts.insert(magic_object, next_entry);
            }
            BattleScriptSource::EnemyAttackItem { item_object, .. } => {
                self.item_use_scripts.insert(item_object, next_entry);
            }
            BattleScriptSource::EnemyTurnStart { .. }
            | BattleScriptSource::EnemyReady { .. }
            | BattleScriptSource::EnemyBattleEnd { .. }
            | BattleScriptSource::PlayerPoison { .. }
            | BattleScriptSource::EnemyPoison { .. } => {}
        }
        for (role_id, statuses, poisons) in updates {
            let role = usize::from(role_id);
            let (Some(saved_statuses), Some(saved_poisons)) = (
                self.player_statuses.get_mut(role),
                self.player_poisons.get_mut(role),
            ) else {
                return false;
            };
            *saved_statuses = statuses;
            *saved_poisons = poisons;
        }
        true
    }

    pub fn player_experience(&self, role_id: u16) -> Option<u32> {
        self.role_experience.get(usize::from(role_id)).copied()
    }

    pub fn player_status_duration(&self, role_id: u16, status: BattleStatus) -> Option<u16> {
        if let Some(player) = self.active_battle.as_ref().and_then(|battle| {
            battle
                .players
                .iter()
                .find(|player| player.role_id == role_id)
        }) {
            return Some(player.statuses.duration(status));
        }
        Some(
            self.player_statuses
                .get(usize::from(role_id))?
                .duration(status),
        )
    }

    pub fn player_poisons(&self, role_id: u16) -> Option<&[BattlePoison; MAX_BATTLE_POISONS]> {
        if let Some(player) = self.active_battle.as_ref().and_then(|battle| {
            battle
                .players
                .iter()
                .find(|player| player.role_id == role_id)
        }) {
            return Some(&player.poisons);
        }
        self.player_poisons.get(usize::from(role_id))
    }

    pub fn player_has_poison(&self, role_id: u16, poison_id: u16) -> bool {
        poison_id != 0
            && self
                .player_poisons(role_id)
                .is_some_and(|poisons| poisons.iter().any(|poison| poison.object_id == poison_id))
    }

    pub fn enemy_has_poison(&self, enemy_index: u16, poison_id: u16) -> bool {
        poison_id != 0
            && self
                .active_battle
                .as_ref()
                .and_then(|battle| battle.enemies.get(usize::from(enemy_index)))
                .is_some_and(|enemy| {
                    enemy
                        .poisons
                        .iter()
                        .any(|poison| poison.object_id == poison_id)
                })
    }

    pub fn enemy_hp_above(&self, enemy_index: u16, percentage: u16) -> bool {
        self.active_battle
            .as_ref()
            .and_then(|battle| battle.enemy_hp_above(usize::from(enemy_index), percentage))
            .unwrap_or(false)
    }

    pub fn enemy_not_first_kind(&self, enemy_index: u16) -> bool {
        self.active_battle
            .as_ref()
            .and_then(|battle| battle.enemy_not_first_kind(usize::from(enemy_index)))
            .unwrap_or(false)
    }

    pub fn collect_value(&self) -> u16 {
        self.collect_value
    }

    fn set_player_status(&mut self, role_id: u16, status: u16, rounds: u16) -> bool {
        let Some(status) = BattleStatus::from_raw(status) else {
            return false;
        };
        if let Some(player) = self.active_battle.as_mut().and_then(|battle| {
            battle
                .players
                .iter_mut()
                .find(|player| player.role_id == role_id)
        }) {
            if !player
                .statuses
                .set_for_player(status, rounds, player.is_alive())
            {
                return false;
            }
            let Some(saved) = self.player_statuses.get_mut(usize::from(role_id)) else {
                return false;
            };
            *saved = player.statuses;
            return true;
        }
        let Some(alive) = self.player_role(role_id).map(|role| role.hp != 0) else {
            return false;
        };
        let Some(statuses) = self.player_statuses.get_mut(usize::from(role_id)) else {
            return false;
        };
        if !statuses.set_for_player(status, rounds, alive) {
            return false;
        }
        if let Some(player) = self.active_battle.as_mut().and_then(|battle| {
            battle
                .players
                .iter_mut()
                .find(|player| player.role_id == role_id)
        }) {
            player.statuses = *statuses;
        }
        true
    }

    fn remove_player_status(&mut self, role_id: u16, status: u16) -> bool {
        let Some(status) = BattleStatus::from_raw(status) else {
            return false;
        };
        if let Some(player) = self.active_battle.as_mut().and_then(|battle| {
            battle
                .players
                .iter_mut()
                .find(|player| player.role_id == role_id)
        }) {
            player.statuses.remove_from_player(status);
            let Some(saved) = self.player_statuses.get_mut(usize::from(role_id)) else {
                return false;
            };
            *saved = player.statuses;
            return true;
        }
        let Some(statuses) = self.player_statuses.get_mut(usize::from(role_id)) else {
            return false;
        };
        statuses.remove_from_player(status);
        if let Some(player) = self.active_battle.as_mut().and_then(|battle| {
            battle
                .players
                .iter_mut()
                .find(|player| player.role_id == role_id)
        }) {
            player.statuses = *statuses;
        }
        true
    }

    fn poison_player(&mut self, role_id: u16, poison_id: u16, apply_to_all: bool) -> bool {
        let Some(poison_object) = self
            .global_objects
            .as_ref()
            .and_then(|objects| objects.get(poison_id))
            .copied()
        else {
            return false;
        };
        let targets = if apply_to_all {
            self.party
                .members()
                .iter()
                .map(|member| member.role_id)
                .collect::<Vec<_>>()
        } else if self
            .party
            .members()
            .iter()
            .any(|member| member.role_id == role_id)
        {
            vec![role_id]
        } else {
            return false;
        };
        for target in targets {
            let Some(resistance) = self
                .effective_player_role(target)
                .map(|role| role.poison_resistance.min(100))
            else {
                return false;
            };
            if self.growth_random(100) < u32::from(resistance) {
                continue;
            }
            let target_index = usize::from(target);
            let already_present = self.player_poisons[target_index]
                .iter()
                .any(|poison| poison.object_id == poison_id);
            let added = add_poison(
                &mut self.player_poisons[target_index],
                poison_id,
                poison_object.poison_player_script(),
            );
            if let Some(battle) = self.active_battle.as_mut() {
                if let Some(player) = battle
                    .players
                    .iter_mut()
                    .find(|player| player.role_id == target)
                {
                    player.poisons = self.player_poisons[target_index];
                }
                if added && !already_present {
                    battle.queue_player_poison_script(
                        target,
                        poison_id,
                        poison_object.poison_player_script(),
                    );
                }
            }
        }
        true
    }

    fn cure_player_poison(&mut self, role_id: u16, poison_id: u16, apply_to_all: bool) -> bool {
        if poison_id == 0 {
            return false;
        }
        let Some(targets) = self.poison_targets(role_id, apply_to_all) else {
            return false;
        };
        for target in targets {
            let target_index = usize::from(target);
            cure_poison(&mut self.player_poisons[target_index], poison_id);
            if let Some(player) = self.active_battle.as_mut().and_then(|battle| {
                battle
                    .players
                    .iter_mut()
                    .find(|player| player.role_id == target)
            }) {
                player.poisons = self.player_poisons[target_index];
            }
        }
        true
    }

    fn cure_player_poison_by_level(
        &mut self,
        role_id: u16,
        maximum_level: u16,
        apply_to_all: bool,
    ) -> bool {
        let Some(targets) = self.poison_targets(role_id, apply_to_all) else {
            return false;
        };
        let Some(objects) = self.global_objects.as_ref() else {
            return false;
        };
        for target in targets {
            let target_index = usize::from(target);
            cure_poison_by_level(
                &mut self.player_poisons[target_index],
                maximum_level,
                objects,
            );
            if let Some(player) = self.active_battle.as_mut().and_then(|battle| {
                battle
                    .players
                    .iter_mut()
                    .find(|player| player.role_id == target)
            }) {
                player.poisons = self.player_poisons[target_index];
            }
        }
        true
    }

    fn poison_targets(&self, role_id: u16, apply_to_all: bool) -> Option<Vec<u16>> {
        if apply_to_all {
            return Some(
                self.party
                    .members()
                    .iter()
                    .map(|member| member.role_id)
                    .collect(),
            );
        }
        self.party
            .members()
            .iter()
            .any(|member| member.role_id == role_id)
            .then_some(vec![role_id])
    }

    /// Create a battle from the current party and script-selected battle configuration.
    pub fn start_battle(&mut self, request: BattleRequest, scripts: &ScriptTable) -> bool {
        if self.active_battle.is_some() {
            return false;
        }
        if !self.refresh_equipment_effects(scripts) {
            return false;
        }
        let role_ids = self
            .party
            .members()
            .iter()
            .map(|member| member.role_id)
            .collect::<Vec<_>>();
        let Some(roles) = role_ids
            .iter()
            .map(|&role_id| Some((role_id, self.effective_player_role(role_id)?)))
            .collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        let Some(data) = self.battle_data.as_ref() else {
            return false;
        };
        let Some(objects) = self.global_objects.as_ref() else {
            return false;
        };
        let Some(magics) = self.magics.as_ref() else {
            return false;
        };
        let Some(mut battle) = BattleState::new(
            request,
            self.current_battlefield,
            self.current_battle_music,
            roles.iter().map(|(role_id, role)| (*role_id, role)),
            data,
            objects,
            magics,
        ) else {
            return false;
        };
        for enemy in &mut battle.enemies {
            if let Some(magic) = enemy.magic.as_mut() {
                magic.use_script = self
                    .magic_use_scripts
                    .get(&magic.object_id)
                    .copied()
                    .unwrap_or(magic.use_script);
                magic.success_script = self
                    .magic_success_scripts
                    .get(&magic.object_id)
                    .copied()
                    .unwrap_or(magic.success_script);
            }
            enemy.attack_equivalent_item_script = self
                .item_use_scripts
                .get(&enemy.attack_equivalent_item)
                .copied()
                .unwrap_or(enemy.attack_equivalent_item_script);
        }
        for player in &mut battle.players {
            let role_index = usize::from(player.role_id);
            let (Some(statuses), Some(poisons)) = (
                self.player_statuses.get(role_index),
                self.player_poisons.get(role_index),
            ) else {
                return false;
            };
            player.statuses = *statuses;
            player.poisons = *poisons;
        }
        battle.refresh_player_effects();
        self.active_battle = Some(battle);
        true
    }

    /// Apply final HP and cash changes, then leave the finished battle state.
    pub fn settle_battle(&mut self) -> Option<(BattleResult, BattleRewards)> {
        let result = match self.active_battle.as_ref()?.phase() {
            BattlePhase::Finished(result) => result,
            BattlePhase::AwaitingCommand => return None,
        };
        let battle = self.active_battle.take()?;
        let objects = self.global_objects.as_ref()?;
        for player in &battle.players {
            let role_index = usize::from(player.role_id);
            *self.player_statuses.get_mut(role_index)? = player.statuses;
            *self.player_poisons.get_mut(role_index)? = player.poisons;
            cure_poison_by_level(&mut self.player_poisons[role_index], 3, objects);
        }
        for statuses in &mut self.player_statuses {
            for status in BattleStatus::ALL {
                statuses.remove_from_player(status);
            }
        }
        if let Some(roles) = self.player_roles.as_mut() {
            for player in &battle.players {
                let role = roles.role_mut(usize::from(player.role_id))?;
                role.hp = player.hp;
                role.mp = player.mp;
            }
            self.party.sync_from_roles(roles);
        }
        let rewards = battle.settled_rewards()?;
        if result == BattleResult::Won {
            self.cash = self.cash.saturating_add(rewards.cash);
            let living_roles = battle
                .players
                .iter()
                .filter_map(|player| player.is_alive().then_some(player.role_id))
                .collect::<Vec<_>>();
            self.award_battle_experience(&living_roles, rewards.experience);
        }
        Some((result, rewards))
    }

    fn award_battle_experience(&mut self, role_ids: &[u16], gained: u32) {
        for &role_id in role_ids {
            let role_index = usize::from(role_id);
            let Some(current) = self.role_experience.get_mut(role_index) else {
                continue;
            };
            *current = current.saturating_add(gained);
            while let Some(level) = self
                .player_roles
                .as_ref()
                .and_then(|roles| roles.role(role_index))
                .map(|role| role.level)
            {
                if level >= 99 {
                    break;
                }
                let Some(required) = self
                    .battle_data
                    .as_ref()
                    .and_then(|data| data.level_up_experience.for_level(level))
                    .map(u32::from)
                    .filter(|&required| required > 0)
                else {
                    break;
                };
                if self.role_experience[role_index] < required {
                    break;
                }
                self.role_experience[role_index] -= required;
                if !self.level_up_role(role_id) {
                    break;
                }
            }
        }
    }

    /// Re-run equipped-item scripts so battle attributes match current equipment.
    fn refresh_equipment_effects(&mut self, scripts: &ScriptTable) -> bool {
        const MAX_EQUIPMENT_SCRIPT_INSTRUCTIONS: usize = 4096;

        let Some(roles) = self.player_roles.as_ref() else {
            return false;
        };
        let Some(objects) = self.global_objects.as_ref() else {
            return false;
        };
        let equipped = roles
            .iter()
            .enumerate()
            .flat_map(|(role_id, role)| {
                role.equipment
                    .iter()
                    .copied()
                    .enumerate()
                    .map(move |(slot, item_id)| (role_id, slot, item_id))
            })
            .filter(|&(_, _, item_id)| item_id != 0)
            .map(|(role_id, slot, item_id)| {
                let object = objects.get(item_id)?;
                let script_entry = self
                    .item_equip_scripts
                    .get(&item_id)
                    .copied()
                    .unwrap_or_else(|| object.item_equip_script());
                Some((
                    u16::try_from(role_id).ok()?,
                    u16::try_from(slot).ok()?,
                    item_id,
                    script_entry,
                ))
            })
            .collect::<Option<Vec<_>>>();
        let Some(equipped) = equipped else {
            return false;
        };

        let saved_roles = self.player_roles.clone();
        let saved_inventory = self.inventory.clone();
        let saved_effects = self.equipment_effects.clone();
        let saved_equip_scripts = self.item_equip_scripts.clone();
        let saved_current_slot = self.current_equipment_slot;
        let saved_statuses = self.player_statuses;
        self.equipment_effects.clear();
        self.current_equipment_slot = None;
        for (role_id, slot, item_id, script_entry) in equipped {
            if script_entry == 0 {
                continue;
            }
            let mut entry = script_entry;
            let mut next_entry = script_entry;
            let mut call_stack = Vec::new();
            let mut completed = false;
            for _ in 0..MAX_EQUIPMENT_SCRIPT_INSTRUCTIONS {
                let Some(instruction) = scripts.entry(entry).copied() else {
                    break;
                };
                let Some(opcode) = ScriptOpcode::from_raw(instruction.opcode) else {
                    break;
                };
                use ScriptOpcode::*;
                let applied = match opcode {
                    Stop => {
                        if let Some(return_entry) = call_stack.pop() {
                            entry = return_entry;
                            continue;
                        }
                        completed = true;
                        break;
                    }
                    StopAndAdvance => {
                        if let Some(return_entry) = call_stack.pop() {
                            entry = return_entry;
                            continue;
                        }
                        next_entry = entry.wrapping_add(1);
                        completed = true;
                        break;
                    }
                    StopAndReplace => {
                        if let Some(return_entry) = call_stack.pop() {
                            entry = return_entry;
                            continue;
                        }
                        next_entry = instruction.operands[0];
                        completed = true;
                        break;
                    }
                    Jump if instruction.operands[1] == 0 => {
                        entry = instruction.operands[0];
                        continue;
                    }
                    Call if instruction.operands[0] != 0 => {
                        call_stack.push(entry.wrapping_add(1));
                        entry = instruction.operands[0];
                        continue;
                    }
                    EquipItem if instruction.operands[0] >= 0x0b => {
                        self.apply_script_action(ScriptAction::EquipItem {
                            role_id,
                            slot: instruction.operands[0] - 0x0b,
                            item_id: instruction.operands[1],
                        })
                    }
                    SetEquipmentEffect if instruction.operands[0] >= 0x0b => self
                        .apply_script_action(ScriptAction::SetEquipmentEffect {
                            role_id,
                            attribute: instruction.operands[1],
                            slot: instruction.operands[0] - 0x0b,
                            value: instruction.operands[2] as i16,
                        }),
                    AdjustPlayerAttribute | SetPlayerAttribute => {
                        let target_role = instruction.operands[2].checked_sub(1).unwrap_or(role_id);
                        self.apply_script_action(ScriptAction::ChangePlayerAttribute {
                            role_id: target_role,
                            attribute: instruction.operands[0],
                            value: instruction.operands[1] as i16,
                            absolute: opcode == SetPlayerAttribute,
                        })
                    }
                    SetPlayerStatus => self.set_player_status(
                        role_id,
                        instruction.operands[0],
                        instruction.operands[1],
                    ),
                    _ => false,
                };
                if !applied {
                    break;
                }
                entry = entry.wrapping_add(1);
            }
            if !completed
                || self
                    .player_role(role_id)
                    .is_none_or(|role| role.equipment[usize::from(slot)] != item_id)
            {
                self.player_roles = saved_roles;
                self.inventory = saved_inventory;
                self.equipment_effects = saved_effects;
                self.item_equip_scripts = saved_equip_scripts;
                self.current_equipment_slot = saved_current_slot;
                self.player_statuses = saved_statuses;
                if let Some(roles) = self.player_roles.as_ref() {
                    self.party.sync_from_roles(roles);
                }
                return false;
            }
            self.item_equip_scripts.insert(item_id, next_entry);
            self.current_equipment_slot = None;
        }
        true
    }

    fn level_up_role(&mut self, role_id: u16) -> bool {
        let role_index = usize::from(role_id);
        let hp = 10 + self.growth_random(8);
        let mp = 8 + self.growth_random(6);
        let attack = 4 + self.growth_random(2);
        let magic = 4 + self.growth_random(2);
        let defense = 2 + self.growth_random(2);
        let dexterity = 2 + self.growth_random(2);
        let Some(roles) = self.player_roles.as_mut() else {
            return false;
        };
        let Some(role) = roles.role_mut(role_index) else {
            return false;
        };
        role.level = role.level.saturating_add(1).min(99);
        role.max_hp = role.max_hp.saturating_add(hp as u16).min(999);
        role.max_mp = role.max_mp.saturating_add(mp as u16).min(999);
        role.attack_strength = role.attack_strength.saturating_add(attack as u16).min(999);
        role.magic_strength = role.magic_strength.saturating_add(magic as u16).min(999);
        role.defense = role.defense.saturating_add(defense as u16).min(999);
        role.dexterity = role.dexterity.saturating_add(dexterity as u16).min(999);
        role.flee_rate = role.flee_rate.saturating_add(2).min(999);
        role.hp = role.max_hp;
        role.mp = role.max_mp;
        let new_level = role.level;

        if role_index < pal_assets::battle::LEVEL_UP_ROLE_COUNT {
            let learned = self
                .battle_data
                .as_ref()
                .into_iter()
                .flat_map(|data| data.level_up_magics.iter())
                .map(|set| set.roles[role_index])
                .filter(|entry| entry.level == new_level && entry.magic != 0)
                .map(|entry| entry.magic)
                .collect::<Vec<_>>();
            for magic in learned {
                if role.magic.contains(&magic) {
                    continue;
                }
                if let Some(slot) = role.magic.iter_mut().find(|slot| **slot == 0) {
                    *slot = magic;
                }
            }
        }
        self.party.sync_from_roles(roles);
        true
    }

    fn growth_random(&mut self, upper_exclusive: u32) -> u32 {
        let mut x = self.growth_random_state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.growth_random_state = x;
        x % upper_exclusive.max(1)
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
        self.scene_enter_scripts
            .insert(self.scene_number, next_entry);
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

    pub fn replace_map(&mut self, map: M) {
        self.map = map;
        self.follow_player();
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
            magic_use_scripts: self.magic_use_scripts.clone(),
            magic_success_scripts: self.magic_success_scripts.clone(),
            equipment_effects: self.equipment_effects.clone(),
            inactive_objects: self.inactive_objects.clone(),
            scene_enter_scripts: self.scene_enter_scripts.clone(),
            scene_teleport_scripts: self.scene_teleport_scripts.clone(),
            scene_maps: self.scene_maps.clone(),
            script_frame: self.script_frame,
            viewport_locked: self.viewport_locked,
            camera_x: self.camera.x,
            camera_y: self.camera.y,
            party_followers: self.party_followers.clone(),
            extra_follower_ids: self.extra_follower_ids.clone(),
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
            magic_use_scripts: snapshot.magic_use_scripts.into_iter().collect(),
            magic_success_scripts: snapshot.magic_success_scripts.into_iter().collect(),
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
            viewport_locked: snapshot.viewport_locked,
            camera_x: snapshot.camera_x,
            camera_y: snapshot.camera_y,
            party_followers: snapshot
                .party_followers
                .iter()
                .map(SavedRole::from)
                .collect(),
            extra_follower_ids: snapshot.extra_follower_ids,
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
            || data.magic_use_scripts.len() > MAX_INVENTORY
            || data.magic_success_scripts.len() > MAX_INVENTORY
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
        let magic_use_scripts = unique_script_entries(data.magic_use_scripts)?;
        let magic_success_scripts = unique_script_entries(data.magic_success_scripts)?;
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
            magic_use_scripts,
            magic_success_scripts,
            equipment_effects,
            inactive_objects,
            scene_enter_scripts,
            scene_teleport_scripts,
            scene_maps,
            script_frame: data.script_frame,
            viewport_locked: data.viewport_locked,
            camera_x: data.camera_x,
            camera_y: data.camera_y,
            party_followers,
            extra_follower_ids: data.extra_follower_ids,
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
        self.cash = snapshot.cash;
        self.inventory = snapshot.inventory;
        self.item_use_scripts = snapshot.item_use_scripts;
        self.item_equip_scripts = snapshot.item_equip_scripts;
        self.magic_use_scripts = snapshot.magic_use_scripts;
        self.magic_success_scripts = snapshot.magic_success_scripts;
        self.equipment_effects = snapshot.equipment_effects;
        self.current_equipment_slot = None;
        self.inactive_objects = snapshot.inactive_objects;
        self.scene_enter_scripts = snapshot.scene_enter_scripts;
        self.scene_teleport_scripts = snapshot.scene_teleport_scripts;
        self.scene_maps = snapshot.scene_maps;
        self.script_frame = snapshot.script_frame;
        self.viewport_locked = snapshot.viewport_locked;
        self.camera.x = snapshot.camera_x;
        self.camera.y = snapshot.camera_y;
        self.party_followers = snapshot.party_followers;
        self.extra_follower_ids = snapshot.extra_follower_ids;
        self.party_trail = snapshot.party_trail;
        self.player_roles = snapshot.player_roles;
        self.pending_auto_sounds.clear();
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
        let player = Role {
            sprite_index: usize::from(leader.scene_sprite_num),
            world_x: i32::from(save.viewport_x) + i32::from(save.party[0].x),
            world_y: i32::from(save.viewport_y) + i32::from(save.party[0].y),
            direction,
            anim_frame: 0,
            frames_per_direction: leader.frames_per_direction(),
        };
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
        self.growth_random_state = 0xa341_316c;
        self.player_statuses =
            [BattleStatuses::from_durations([0; BATTLE_STATUS_COUNT]); PLAYER_ROLE_COUNT];
        self.player_poisons = player_poisons;
        self.collect_value = save.collect_value;
        self.player_roles = Some(save.player_roles);
        self.global_objects = Some(save.objects);
        self.active_battle = None;
        self.cash = save.cash;
        self.inventory = inventory;
        self.item_use_scripts.clear();
        self.item_equip_scripts.clear();
        self.magic_use_scripts.clear();
        self.magic_success_scripts.clear();
        self.equipment_effects.clear();
        self.current_equipment_slot = None;
        self.inactive_objects = inactive_objects;
        self.scene_enter_scripts = scene_enter_scripts;
        self.scene_teleport_scripts = scene_teleport_scripts;
        self.scene_maps = scene_maps;
        self.pending_auto_sounds.clear();
        self.script_frame = 0;
        self.viewport_locked = false;
        self.party_trail = trail;
        self.extra_follower_ids = extra_follower_ids;
        self.camera.x = i32::from(save.viewport_x);
        self.camera.y = i32::from(save.viewport_y);
        self.rebuild_party_followers();
        true
    }

    pub fn take_trigger(&mut self) -> Option<TriggerRequest> {
        self.pending_trigger.take()
    }

    /// Apply a platform-independent world mutation yielded by the script runtime.
    pub fn apply_script_action(&mut self, action: ScriptAction) -> bool {
        match action {
            ScriptAction::AddItem { item_id, amount } => {
                if item_id == 0 {
                    return false;
                }
                let amount = if amount == 0 { 1 } else { amount };
                let current = i32::from(self.inventory_count(item_id));
                let updated = (current + i32::from(amount)).clamp(0, 99) as u16;
                if !self.set_inventory_amount(item_id, updated) {
                    return false;
                }
            }
            ScriptAction::RemoveItem {
                item_id,
                amount,
                insufficient_entry,
            } => return self.remove_item(item_id, amount, insufficient_entry),
            ScriptAction::AdjustCash { amount, .. } => return self.adjust_cash(amount),
            ScriptAction::PlayMusic { music_id, .. } => {
                self.current_music = (music_id != 0).then_some(music_id);
            }
            ScriptAction::PlaySound { .. } => {}
            ScriptAction::SetBattleMusic { music_id } => {
                self.current_battle_music = music_id;
            }
            ScriptAction::SetBattlefield { battlefield_id } => {
                self.current_battlefield = battlefield_id;
            }
            ScriptAction::MoveObject {
                object_id,
                direction,
            } => {
                let Some(object) = self.object_mut(object_id) else {
                    return false;
                };
                object.direction = direction;
                let (dx, dy) = direction.step_at_speed(2);
                object.world_x += dx;
                object.world_y += dy;
                object.advance_animation();
            }
            ScriptAction::SetObjectPose {
                object_id,
                direction,
                frame,
            } => {
                let Some(object) = self.object_mut(object_id) else {
                    return false;
                };
                if let Some(direction) = direction {
                    object.direction = direction;
                }
                if let Some(frame) = frame {
                    object.current_frame = frame;
                }
            }
            ScriptAction::SetObjectPosition { object_id, x, y } => {
                let Some(object) = self.object_mut(object_id) else {
                    return false;
                };
                object.world_x = x;
                object.world_y = y;
            }
            ScriptAction::SetObjectPositionRelativeToPlayer { object_id, dx, dy } => {
                let position = (self.player.world_x + dx, self.player.world_y + dy);
                let Some(object) = self.object_mut(object_id) else {
                    return false;
                };
                object.world_x = position.0;
                object.world_y = position.1;
            }
            ScriptAction::PlaceObjectInFront {
                object_id, state, ..
            } => return self.place_object_in_front(object_id, state),
            ScriptAction::OffsetObject { object_id, dx, dy } => {
                let Some(object) = self.object_mut(object_id) else {
                    return false;
                };
                object.world_x += dx;
                object.world_y += dy;
                object.advance_animation();
            }
            ScriptAction::MoveObjectBy { object_id, dx, dy } => {
                let Some(object) = self.object_mut(object_id) else {
                    return false;
                };
                object.world_x += dx;
                object.world_y += dy;
            }
            ScriptAction::SetObjectLayer { object_id, layer } => {
                let Some(object) = self.object_mut(object_id) else {
                    return false;
                };
                object.layer = layer;
            }
            ScriptAction::SetObjectState { object_id, state } => {
                let Some(object) = self.object_mut(object_id) else {
                    return false;
                };
                object.state = state;
            }
            ScriptAction::SetObjectVanishTime {
                object_id,
                vanish_time,
            } => {
                let Some(object) = self.object_mut(object_id) else {
                    return false;
                };
                object.vanish_time = vanish_time;
            }
            ScriptAction::HideObjectTemporarily {
                object_id,
                vanish_time,
            } => {
                let Some(object) = self.object_mut(object_id) else {
                    return false;
                };
                object.state = object.state.saturating_neg();
                object.vanish_time = vanish_time;
            }
            ScriptAction::SyncObjectState {
                object_id,
                source_object_id,
                state,
            } => {
                let Some(source_state) = self.object_state(source_object_id) else {
                    return false;
                };
                if source_state == state {
                    let Some(object) = self.object_mut(object_id) else {
                        return false;
                    };
                    object.state = state;
                }
            }
            ScriptAction::AnimateObject { object_id } => {
                let Some(object) = self.object_mut(object_id) else {
                    return false;
                };
                object.advance_animation();
            }
            ScriptAction::SetObjectTriggerScript {
                object_id,
                script_entry,
            } => {
                let Some(object) = self.object_mut(object_id) else {
                    return false;
                };
                object.trigger_script = script_entry;
            }
            ScriptAction::SetObjectAutoScript {
                object_id,
                script_entry,
            } => {
                let Some(object) = self.object_mut(object_id) else {
                    return false;
                };
                object.auto_script = script_entry;
                object.auto_script_idle_frame = 0;
            }
            ScriptAction::SetObjectTriggerMode {
                object_id,
                trigger_mode,
            } => {
                let Some(object) = self.object_mut(object_id) else {
                    return false;
                };
                object.trigger_mode = trigger_mode;
            }
            ScriptAction::SetObjectStates {
                first_object_id,
                last_object_id,
                state,
            } => {
                let expected = usize::from(last_object_id - first_object_id) + 1;
                let current_count = self
                    .scene_objects
                    .iter()
                    .filter(|object| (first_object_id..=last_object_id).contains(&object.id))
                    .count();
                let inactive_count = self
                    .inactive_objects
                    .range(first_object_id..=last_object_id)
                    .count();
                if current_count + inactive_count != expected {
                    return false;
                }
                for object in &mut self.scene_objects {
                    if (first_object_id..=last_object_id).contains(&object.id) {
                        object.state = state;
                    }
                }
                for (_, object) in self
                    .inactive_objects
                    .range_mut(first_object_id..=last_object_id)
                {
                    object.state = state;
                }
            }
            ScriptAction::SetPlayerPose {
                direction,
                frame,
                party_index,
            } => {
                self.player.direction = direction;
                if party_index == 0 {
                    self.player.anim_frame = frame;
                } else if let Some(follower) = self
                    .party_followers
                    .get_mut(usize::from(party_index.saturating_sub(1)))
                {
                    follower.direction = direction;
                    follower.anim_frame = frame;
                }
            }
            ScriptAction::SetPlayerSprite { sprite_index } => {
                self.player.sprite_index = sprite_index;
                self.player.anim_frame = 0;
            }
            ScriptAction::AdjustPlayerHealth {
                role_id,
                hp,
                mp,
                apply_to_all,
            } => return self.adjust_player_health(role_id, hp, mp, apply_to_all),
            ScriptAction::RevivePlayer {
                role_id,
                hp_tenths,
                apply_to_all,
            } => return self.revive_player(role_id, hp_tenths, apply_to_all),
            ScriptAction::DamageEnemy {
                enemy_index,
                amount,
                apply_to_all,
            } => {
                return self.active_battle.as_mut().is_some_and(|battle| {
                    battle.damage_enemy(usize::from(enemy_index), amount, apply_to_all)
                });
            }
            ScriptAction::PoisonEnemy {
                enemy_index,
                poison_id,
                apply_to_all,
            } => {
                let Some(script_entry) = self
                    .global_objects
                    .as_ref()
                    .and_then(|objects| objects.get(poison_id))
                    .map(|object| object.poison_enemy_script())
                else {
                    return false;
                };
                return self.active_battle.as_mut().is_some_and(|battle| {
                    battle.poison_enemy(
                        usize::from(enemy_index),
                        poison_id,
                        script_entry,
                        apply_to_all,
                    )
                });
            }
            ScriptAction::PoisonPlayer {
                role_id,
                poison_id,
                apply_to_all,
            } => return self.poison_player(role_id, poison_id, apply_to_all),
            ScriptAction::CureEnemyPoison {
                enemy_index,
                poison_id,
                apply_to_all,
            } => {
                return self.active_battle.as_mut().is_some_and(|battle| {
                    battle.cure_enemy_poison(usize::from(enemy_index), poison_id, apply_to_all)
                });
            }
            ScriptAction::CurePlayerPoison {
                role_id,
                poison_id,
                apply_to_all,
            } => return self.cure_player_poison(role_id, poison_id, apply_to_all),
            ScriptAction::CurePlayerPoisonByLevel {
                role_id,
                maximum_level,
                apply_to_all,
            } => {
                return self.cure_player_poison_by_level(role_id, maximum_level, apply_to_all);
            }
            ScriptAction::SetPlayerStatus {
                role_id,
                status,
                rounds,
            } => return self.set_player_status(role_id, status, rounds),
            ScriptAction::SetEnemyStatus {
                enemy_index,
                status,
                rounds,
                ..
            } => {
                let Some(status) = BattleStatus::from_raw(status) else {
                    return false;
                };
                return self
                    .active_battle
                    .as_mut()
                    .and_then(|battle| {
                        battle.set_enemy_status(usize::from(enemy_index), status, rounds)
                    })
                    .unwrap_or(false);
            }
            ScriptAction::RemovePlayerStatus { role_id, status } => {
                return self.remove_player_status(role_id, status);
            }
            ScriptAction::DrainEnemyHp {
                enemy_index,
                amount,
            } => {
                return self
                    .active_battle
                    .as_mut()
                    .is_some_and(|battle| battle.drain_enemy_hp(usize::from(enemy_index), amount));
            }
            ScriptAction::FleeBattle { .. } => {
                return self
                    .active_battle
                    .as_mut()
                    .and_then(BattleState::flee)
                    .is_some();
            }
            ScriptAction::HalvePlayerHp { role_id } => {
                return self.set_player_hp(role_id, true);
            }
            ScriptAction::HalveEnemyHp {
                enemy_index,
                maximum_damage,
            } => {
                return self.active_battle.as_mut().is_some_and(|battle| {
                    battle.halve_enemy_hp(usize::from(enemy_index), maximum_damage)
                });
            }
            ScriptAction::KillPlayer { role_id } => return self.set_player_hp(role_id, false),
            ScriptAction::KillEnemy { enemy_index } => {
                return self
                    .active_battle
                    .as_mut()
                    .is_some_and(|battle| battle.kill_enemy(usize::from(enemy_index)));
            }
            ScriptAction::SetEnemyMagic {
                enemy_index,
                magic_object,
                rate,
            } => {
                let persisted_use = self.magic_use_scripts.get(&magic_object).copied();
                let persisted_success = self.magic_success_scripts.get(&magic_object).copied();
                let (Some(battle), Some(objects), Some(magics)) = (
                    self.active_battle.as_mut(),
                    self.global_objects.as_ref(),
                    self.magics.as_ref(),
                ) else {
                    return false;
                };
                if !battle.set_enemy_magic(
                    usize::from(enemy_index),
                    magic_object,
                    rate,
                    objects,
                    magics,
                ) {
                    return false;
                }
                if let Some(magic) = battle
                    .enemies
                    .get_mut(usize::from(enemy_index))
                    .and_then(|enemy| enemy.magic.as_mut())
                {
                    magic.use_script = persisted_use.unwrap_or(magic.use_script);
                    magic.success_script = persisted_success.unwrap_or(magic.success_script);
                }
                return true;
            }
            ScriptAction::EnemyEscape => {
                return self
                    .active_battle
                    .as_mut()
                    .is_some_and(BattleState::enemy_escape);
            }
            ScriptAction::SetBattleResult { result } => {
                return self
                    .active_battle
                    .as_mut()
                    .is_some_and(|battle| battle.set_script_result(result));
            }
            ScriptAction::SetEquipmentEffect {
                role_id,
                attribute,
                slot,
                value,
            } => return self.set_equipment_effect(role_id, slot, attribute, value),
            ScriptAction::EquipItem {
                role_id,
                slot,
                item_id,
            } => return self.equip_item(role_id, slot, item_id),
            ScriptAction::ChangePlayerAttribute {
                role_id,
                attribute,
                value,
                absolute,
            } => {
                if absolute {
                    if let Some(slot) = self.current_equipment_slot {
                        return self.set_equipment_effect(role_id, slot, attribute, value);
                    }
                }
                let Some(roles) = self.player_roles.as_mut() else {
                    return false;
                };
                let Some(role) = roles.role_mut(usize::from(role_id)) else {
                    return false;
                };
                if apply_role_attribute(role, attribute, value, absolute).is_none() {
                    return false;
                }
                self.party.sync_from_roles(roles);
            }
            ScriptAction::RemoveEquipment { role_id, slot } => {
                return self.remove_equipment(role_id, slot);
            }
            ScriptAction::ChangeMagic {
                role_id,
                magic_id,
                add,
            } => return self.change_magic(role_id, magic_id, add),
            ScriptAction::OffsetPlayer { dx, dy } => {
                self.shift_party(dx, dy);
            }
            ScriptAction::SetPlayerPosition {
                tile_x,
                tile_y,
                half,
            } => {
                let Some((x, y)) =
                    tile_to_world(usize::from(tile_x), usize::from(tile_y), usize::from(half))
                else {
                    return false;
                };
                self.player.world_x = x;
                self.player.world_y = y;
                self.place_party();
                self.follow_player();
            }
            ScriptAction::ChangeScene { .. } => return false,
            ScriptAction::SetSceneScripts {
                scene_number,
                enter_script,
                teleport_script,
            } => {
                if scene_number == 0 {
                    return false;
                }
                if let Some(entry) = enter_script {
                    self.scene_enter_scripts.insert(scene_number, entry);
                }
                if let Some(entry) = teleport_script {
                    self.scene_teleport_scripts.insert(scene_number, entry);
                }
            }
            ScriptAction::SetSceneMap {
                scene_number,
                map_number,
            } => {
                let scene_number = scene_number.unwrap_or(self.scene_number);
                if scene_number == 0 || map_number == 0 {
                    return false;
                }
                self.scene_maps.insert(scene_number, map_number);
            }
            ScriptAction::WalkObjectTo { .. } => return false,
            ScriptAction::WalkPlayerTo { .. } => return false,
            ScriptAction::RideObjectTo { .. } => return false,
            ScriptAction::MoveViewport { x, y, frames } => {
                if x == 0 && y == 0 {
                    self.viewport_locked = false;
                    self.follow_player();
                } else if frames == -1 {
                    self.viewport_locked = true;
                    self.camera.x = i32::from(x) * 32 - 160;
                    self.camera.y = i32::from(y) * 16 - 112;
                } else {
                    self.viewport_locked = true;
                    self.camera.x += i32::from(x);
                    self.camera.y += i32::from(y);
                }
            }
            ScriptAction::CollapseParty => self.collapse_party(),
            ScriptAction::SetParty { members } => {
                let role_ids = members.into_iter().flatten().collect::<Vec<_>>();
                let Some(player_roles) = self.player_roles.as_ref() else {
                    return false;
                };
                if !self.party.replace(&role_ids, player_roles) {
                    return false;
                }
                let leader = self
                    .party
                    .leader()
                    .expect("a successfully replaced party is non-empty");
                self.player.sprite_index = usize::from(leader.attributes.scene_sprite_num);
                self.player.frames_per_direction = leader.attributes.frames_per_direction();
                self.player.anim_frame = 0;
                self.extra_follower_ids.clear();
                self.rebuild_party_followers();
                self.collapse_party();
            }
            ScriptAction::SetPartyFollowers { followers } => {
                let follower_ids = followers.into_iter().flatten().collect::<Vec<_>>();
                let Some(player_roles) = self.player_roles.as_ref() else {
                    return false;
                };
                if follower_ids.len() > 2
                    || self.party.members().len() + follower_ids.len() > MAX_PARTY_MEMBERS
                    || follower_ids.iter().enumerate().any(|(index, &role_id)| {
                        player_roles.role(usize::from(role_id)).is_none()
                            || self
                                .party
                                .members()
                                .iter()
                                .any(|member| member.role_id == role_id)
                            || follower_ids[..index].contains(&role_id)
                    })
                {
                    return false;
                }
                self.extra_follower_ids = follower_ids;
                self.rebuild_party_followers();
                self.collapse_party();
            }
        }
        true
    }

    fn adjust_player_health(&mut self, role_id: u16, hp: i16, mp: i16, apply_to_all: bool) -> bool {
        let role_ids = if apply_to_all {
            self.party
                .members()
                .iter()
                .map(|member| member.role_id)
                .collect::<Vec<_>>()
        } else {
            vec![role_id]
        };
        if let Some(battle) = self.active_battle.as_mut() {
            let mut changed = false;
            let mut updated = Vec::new();
            for role_id in role_ids {
                let Some(player) = battle
                    .players
                    .iter_mut()
                    .find(|player| player.role_id == role_id)
                else {
                    continue;
                };
                if !player.is_alive() {
                    continue;
                }
                let new_hp = (i32::from(player.hp) + i32::from(hp))
                    .clamp(0, i32::from(player.max_hp)) as u16;
                let new_mp = (i32::from(player.mp) + i32::from(mp))
                    .clamp(0, i32::from(player.max_mp)) as u16;
                changed |= player.hp != new_hp || player.mp != new_mp;
                player.hp = new_hp;
                player.mp = new_mp;
                updated.push((role_id, new_hp, new_mp));
            }
            let Some(roles) = self.player_roles.as_mut() else {
                return false;
            };
            for (role_id, hp, mp) in updated {
                let Some(role) = roles.role_mut(usize::from(role_id)) else {
                    return false;
                };
                role.hp = hp;
                role.mp = mp;
            }
            self.party.sync_from_roles(roles);
            return changed;
        }
        let Some(roles) = self.player_roles.as_mut() else {
            return false;
        };
        let mut changed = false;
        for role_id in role_ids {
            let Some(role) = roles.role_mut(usize::from(role_id)) else {
                continue;
            };
            if role.hp == 0 {
                continue;
            }
            let new_hp = (i32::from(role.hp) + i32::from(hp)).clamp(0, i32::from(role.max_hp));
            let new_mp = (i32::from(role.mp) + i32::from(mp)).clamp(0, i32::from(role.max_mp));
            changed |= i32::from(role.hp) != new_hp || i32::from(role.mp) != new_mp;
            role.hp = new_hp as u16;
            role.mp = new_mp as u16;
        }
        self.party.sync_from_roles(roles);
        changed
    }

    fn set_player_hp(&mut self, role_id: u16, halve: bool) -> bool {
        let battle_hp = self.active_battle.as_mut().and_then(|battle| {
            let player = battle
                .players
                .iter_mut()
                .find(|player| player.role_id == role_id)?;
            player.hp = if halve { player.hp / 2 } else { 0 };
            Some(player.hp)
        });
        let Some(roles) = self.player_roles.as_mut() else {
            return false;
        };
        let Some(role) = roles.role_mut(usize::from(role_id)) else {
            return false;
        };
        role.hp = battle_hp.unwrap_or(if halve { role.hp / 2 } else { 0 });
        self.party.sync_from_roles(roles);
        true
    }

    fn revive_player(&mut self, role_id: u16, hp_tenths: u16, apply_to_all: bool) -> bool {
        let role_ids = if apply_to_all {
            self.party
                .members()
                .iter()
                .map(|member| member.role_id)
                .collect::<Vec<_>>()
        } else {
            vec![role_id]
        };
        let revived = if let Some(battle) = self.active_battle.as_mut() {
            let mut revived = Vec::new();
            for role_id in role_ids {
                let Some(player) = battle
                    .players
                    .iter_mut()
                    .find(|player| player.role_id == role_id)
                else {
                    continue;
                };
                if player.is_alive() {
                    continue;
                }
                player.hp = u32::from(player.max_hp)
                    .saturating_mul(u32::from(hp_tenths))
                    .checked_div(10)
                    .unwrap_or(0)
                    .min(u32::from(u16::MAX)) as u16;
                revived.push((role_id, player.hp));
            }
            let Some(roles) = self.player_roles.as_mut() else {
                return false;
            };
            for &(role_id, hp) in &revived {
                let Some(role) = roles.role_mut(usize::from(role_id)) else {
                    return false;
                };
                role.hp = hp;
            }
            self.party.sync_from_roles(roles);
            revived
        } else {
            let Some(roles) = self.player_roles.as_mut() else {
                return false;
            };
            let mut revived = Vec::new();
            for role_id in role_ids {
                let Some(role) = roles.role_mut(usize::from(role_id)) else {
                    continue;
                };
                if role.hp != 0 {
                    continue;
                }
                role.hp = u32::from(role.max_hp)
                    .saturating_mul(u32::from(hp_tenths))
                    .checked_div(10)
                    .unwrap_or(0)
                    .min(u32::from(u16::MAX)) as u16;
                revived.push((role_id, role.hp));
            }
            self.party.sync_from_roles(roles);
            revived
        };
        for &(role_id, hp) in &revived {
            let role_index = usize::from(role_id);
            if let Some(objects) = self.global_objects.as_ref() {
                cure_poison_by_level(&mut self.player_poisons[role_index], 3, objects);
            }
            for status in BattleStatus::ALL {
                self.player_statuses[role_index].remove_from_player(status);
            }
            if let Some(player) = self.active_battle.as_mut().and_then(|battle| {
                battle
                    .players
                    .iter_mut()
                    .find(|player| player.role_id == role_id)
            }) {
                player.hp = hp;
                player.statuses = self.player_statuses[role_index];
                player.poisons = self.player_poisons[role_index];
            }
        }
        !revived.is_empty()
    }

    /// Move an event object one script tick toward a PAL tile position.
    pub fn walk_object_to(
        &mut self,
        object_id: u16,
        tile_x: u16,
        tile_y: u16,
        half: u16,
        speed: u8,
    ) -> Option<bool> {
        let target = tile_to_world(usize::from(tile_x), usize::from(tile_y), usize::from(half))?;
        let object = self.object_mut(object_id)?;
        Some(walk_scene_object_to(object, target, i32::from(speed)))
    }

    pub fn walk_player_to(
        &mut self,
        tile_x: u16,
        tile_y: u16,
        half: u16,
        speed: u8,
    ) -> Option<bool> {
        let target = tile_to_world(usize::from(tile_x), usize::from(tile_y), usize::from(half))?;
        let old_position = (self.player.world_x, self.player.world_y);
        let trail_direction = self.player.direction;
        let x_offset = target.0 - self.player.world_x;
        let y_offset = target.1 - self.player.world_y;
        self.player.direction = direction_toward(x_offset, y_offset);
        let speed = i32::from(speed);
        let completed = if x_offset.abs() < speed * 2 || y_offset.abs() < speed * 2 {
            self.player.world_x = target.0;
            self.player.world_y = target.1;
            self.player.anim_frame = 0;
            true
        } else {
            let (dx, dy) = self.player.direction.step_at_speed(speed);
            self.player.world_x += dx;
            self.player.world_y += dy;
            self.player.anim_frame =
                (self.player.anim_frame + 1) % self.player.frames_per_direction.max(1);
            false
        };
        if old_position != (self.player.world_x, self.player.world_y) {
            self.record_party_step(old_position, trail_direction);
        }
        self.follow_player();
        Some(completed)
    }

    pub fn ride_object_to(
        &mut self,
        object_id: u16,
        tile_x: u16,
        tile_y: u16,
        half: u16,
        speed: u8,
    ) -> Option<bool> {
        let target = tile_to_world(usize::from(tile_x), usize::from(tile_y), usize::from(half))?;
        self.object_state(object_id)?;
        let x_offset = target.0 - self.player.world_x;
        let y_offset = target.1 - self.player.world_y;
        self.player.direction = direction_toward(x_offset, y_offset);
        let speed = i32::from(speed);
        let dx = x_offset.clamp(-speed * 2, speed * 2);
        let dy = y_offset.clamp(-speed, speed);
        self.player.world_x += dx;
        self.player.world_y += dy;
        self.player.anim_frame =
            (self.player.anim_frame + 1) % self.player.frames_per_direction.max(1);
        let completed = (self.player.world_x, self.player.world_y) == target;
        if completed {
            self.player.anim_frame = 0;
        }
        {
            let object = self.object_mut(object_id)?;
            object.world_x += dx;
            object.world_y += dy;
            object.advance_animation();
            if completed {
                object.current_frame = 0;
            }
        }
        self.record_party_step(
            (self.player.world_x, self.player.world_y),
            self.player.direction,
        );
        self.follow_player();
        Some(completed)
    }

    pub fn take_auto_script_sounds(&mut self) -> Vec<u16> {
        std::mem::take(&mut self.pending_auto_sounds)
    }

    pub fn party_contains_name(&self, name_word_id: u16) -> bool {
        self.party
            .members()
            .iter()
            .any(|member| member.attributes.name_word_id == name_word_id)
    }

    fn object_mut(&mut self, id: u16) -> Option<&mut SceneObject> {
        if let Some(object) = self.scene_objects.iter_mut().find(|object| object.id == id) {
            return Some(object);
        }
        self.inactive_objects.get_mut(&id)
    }

    /// Advance one fixed update and report whether visible state changed.
    pub fn update(&mut self, input: GameInput) -> bool {
        if self.pending_trigger.is_some() {
            return false;
        }
        let mut changed = false;
        for object in &mut self.scene_objects {
            if object.update_vanish_time() {
                changed = true;
                continue;
            }
            if object.state < 0
                && (object.world_x < self.camera.x
                    || object.world_x > self.camera.x + 320
                    || object.world_y < self.camera.y
                    || object.world_y > self.camera.y + 320)
            {
                object.state = object.state.saturating_abs();
                object.current_frame = 0;
                changed = true;
            }
        }
        self.pending_trigger = find_touch_trigger(
            &mut self.scene_objects,
            self.player.world_x,
            self.player.world_y,
        );
        if self.pending_trigger.is_none() && input.confirm {
            self.pending_trigger = find_search_trigger(
                &mut self.scene_objects,
                self.player.world_x,
                self.player.world_y,
                self.player.direction,
            );
        }
        if self.pending_trigger.is_some() {
            return true;
        }
        let Some(direction) = input.direction else {
            if self.player.anim_frame == 0 {
                return changed;
            }
            self.player.anim_frame = 0;
            for follower in &mut self.party_followers {
                follower.anim_frame = 0;
            }
            return true;
        };

        changed |= self.player.direction != direction;
        self.player.direction = direction;
        let (dx, dy) = direction.step();
        let target = (self.player.world_x + dx, self.player.world_y + dy);
        if !self.map.is_world_blocked(target.0, target.1)
            && !blocks_position(&self.scene_objects, target.0, target.1)
        {
            let old_position = (self.player.world_x, self.player.world_y);
            self.player.world_x = target.0;
            self.player.world_y = target.1;
            self.player.anim_frame =
                (self.player.anim_frame + 1) % self.player.frames_per_direction.max(1);
            self.record_party_step(old_position, direction);
            self.follow_player();
            changed = true;
        } else if self.player.anim_frame != 0 {
            self.player.anim_frame = 0;
            changed = true;
        }
        changed
    }

    /// Run one auto-script instruction for each active event object.
    pub fn update_auto_scripts(&mut self, scripts: &ScriptTable) -> Result<bool, AutoScriptError> {
        self.script_frame = self.script_frame.wrapping_add(1);
        let mut changed = false;
        let mut first_error = None;
        for index in 0..self.scene_objects.len() {
            if self.scene_objects[index].state <= 0
                || self.scene_objects[index].vanish_time != 0
                || self.scene_objects[index].auto_script == 0
            {
                continue;
            }
            match self.advance_auto_script(index, scripts) {
                Ok(object_changed) => changed |= object_changed,
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            };
        }
        if let Some(error) = first_error {
            Err(error)
        } else {
            Ok(changed)
        }
    }

    fn advance_auto_script(
        &mut self,
        object_index: usize,
        scripts: &ScriptTable,
    ) -> Result<bool, AutoScriptError> {
        const MAX_AUTO_JUMPS: usize = 1024;

        for _ in 0..MAX_AUTO_JUMPS {
            let object_id = self.scene_objects[object_index].id;
            let script_entry = self.scene_objects[object_index].auto_script;
            let entry =
                scripts
                    .entry(script_entry)
                    .copied()
                    .ok_or(AutoScriptError::InvalidEntry {
                        object_id,
                        entry: script_entry,
                    })?;
            let Some(opcode) = ScriptOpcode::from_raw(entry.opcode) else {
                return Err(AutoScriptError::Unsupported {
                    object_id,
                    entry: script_entry,
                    opcode: entry.opcode,
                });
            };
            use ScriptOpcode::*;
            match opcode {
                Stop => return Ok(false),
                StopAndAdvance => {
                    self.scene_objects[object_index].auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                StopAndReplace => {
                    let object = &mut self.scene_objects[object_index];
                    if entry.operands[1] == 0 {
                        object.auto_script = entry.operands[0];
                    } else if object.auto_script_idle_frame.wrapping_add(1) < entry.operands[1] {
                        object.auto_script_idle_frame =
                            object.auto_script_idle_frame.wrapping_add(1);
                        object.auto_script = entry.operands[0];
                    } else {
                        object.auto_script_idle_frame = 0;
                        object.auto_script = script_entry.wrapping_add(1);
                    }
                    return Ok(true);
                }
                Jump => {
                    let object = &mut self.scene_objects[object_index];
                    if entry.operands[1] == 0 {
                        object.auto_script = entry.operands[0];
                        continue;
                    } else if object.auto_script_idle_frame.wrapping_add(1) < entry.operands[1] {
                        object.auto_script_idle_frame =
                            object.auto_script_idle_frame.wrapping_add(1);
                        object.auto_script = entry.operands[0];
                        continue;
                    }
                    object.auto_script_idle_frame = 0;
                    object.auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                Call => {
                    let called_object_id = if entry.operands[1] == 0 {
                        object_id
                    } else {
                        entry.operands[1]
                    };
                    self.run_immediate_auto_subscript(
                        scripts,
                        entry.operands[0],
                        called_object_id,
                        0,
                    )?;
                    self.scene_objects[object_index].auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                JumpByChance => {
                    let roll = ((u32::from(object_id)
                        .wrapping_mul(1_103_515_245)
                        .wrapping_add(u32::from(script_entry))
                        .wrapping_add(self.script_frame.wrapping_mul(12_345))
                        >> 16)
                        % 100
                        + 1) as u16;
                    let object = &mut self.scene_objects[object_index];
                    if roll >= entry.operands[0] {
                        if entry.operands[1] != 0 {
                            object.auto_script = entry.operands[1];
                            continue;
                        }
                    } else {
                        object.auto_script = script_entry.wrapping_add(1);
                    }
                    return Ok(true);
                }
                WaitFrames => {
                    let object = &mut self.scene_objects[object_index];
                    object.auto_script_idle_frame = object.auto_script_idle_frame.wrapping_add(1);
                    if object.auto_script_idle_frame >= entry.operands[0] {
                        object.auto_script_idle_frame = 0;
                        object.auto_script = script_entry.wrapping_add(1);
                    }
                    return Ok(true);
                }
                SetObjectPose => {
                    let object = &mut self.scene_objects[object_index];
                    if entry.operands[0] != 0xffff {
                        object.direction = Direction::from_pal(entry.operands[0]).ok_or(
                            AutoScriptError::Unsupported {
                                object_id,
                                entry: script_entry,
                                opcode: entry.opcode,
                            },
                        )?;
                    }
                    if entry.operands[1] != 0xffff {
                        object.current_frame = entry.operands[1];
                    }
                    object.auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                WalkObjectSouth | WalkObjectWest | WalkObjectNorth | WalkObjectEast => {
                    let direction = Direction::from_pal(opcode.raw() - WalkObjectSouth.raw())
                        .expect("valid walk opcode");
                    let object = &mut self.scene_objects[object_index];
                    object.direction = direction;
                    let (dx, dy) = direction.step_at_speed(2);
                    object.world_x += dx;
                    object.world_y += dy;
                    object.advance_animation();
                    object.auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                WalkObjectTo | WalkObjectToSlow => {
                    let target = tile_to_world(
                        usize::from(entry.operands[0]),
                        usize::from(entry.operands[1]),
                        usize::from(entry.operands[2]),
                    )
                    .ok_or(AutoScriptError::Unsupported {
                        object_id,
                        entry: script_entry,
                        opcode: entry.opcode,
                    })?;
                    let object = &mut self.scene_objects[object_index];
                    let should_move = opcode == WalkObjectTo
                        || !self
                            .script_frame
                            .wrapping_add(u32::from(object_id))
                            .is_multiple_of(2);
                    if should_move
                        && walk_scene_object_to(
                            object,
                            target,
                            if opcode == WalkObjectTo { 3 } else { 2 },
                        )
                    {
                        object.auto_script = script_entry.wrapping_add(1);
                    }
                    return Ok(true);
                }
                SetObjectGesture => {
                    let object = &mut self.scene_objects[object_index];
                    object.direction = Direction::South;
                    object.current_frame = entry.operands[0];
                    object.auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                SetObjectTriggerScript => {
                    let target_id = if entry.operands[0] == 0 || entry.operands[0] == 0xffff {
                        object_id
                    } else {
                        entry.operands[0]
                    };
                    let Some(target) = self.object_mut(target_id) else {
                        return Err(AutoScriptError::MissingObject {
                            object_id,
                            entry: script_entry,
                            target_id,
                        });
                    };
                    target.trigger_script = entry.operands[1];
                    self.scene_objects[object_index].auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                SetObjectTriggerMode => {
                    let target_id = if entry.operands[0] == 0 || entry.operands[0] == 0xffff {
                        object_id
                    } else {
                        entry.operands[0]
                    };
                    let Some(target) = self.object_mut(target_id) else {
                        return Err(AutoScriptError::MissingObject {
                            object_id,
                            entry: script_entry,
                            target_id,
                        });
                    };
                    target.trigger_mode = entry.operands[1];
                    self.scene_objects[object_index].auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                SetObjectState => {
                    let target_id = if entry.operands[0] == 0 || entry.operands[0] == 0xffff {
                        object_id
                    } else {
                        entry.operands[0]
                    };
                    let Some(target) = self.object_mut(target_id) else {
                        return Err(AutoScriptError::MissingObject {
                            object_id,
                            entry: script_entry,
                            target_id,
                        });
                    };
                    target.state = entry.operands[1] as i16;
                    self.scene_objects[object_index].auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                PlaySound => {
                    self.pending_auto_sounds.push(entry.operands[0]);
                    self.scene_objects[object_index].auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                HideObjectShort => {
                    let object = &mut self.scene_objects[object_index];
                    object.vanish_time = -15;
                    object.auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                ChasePlayer => {
                    self.chase_player(
                        object_index,
                        if entry.operands[1] == 0 {
                            4
                        } else {
                            entry.operands[1]
                        },
                        if entry.operands[0] == 0 {
                            8
                        } else {
                            entry.operands[0]
                        },
                        entry.operands[2] != 0,
                    );
                    self.scene_objects[object_index].auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                OffsetObjectAndAnimate => {
                    let target_id = if entry.operands[0] == 0 || entry.operands[0] == 0xffff {
                        object_id
                    } else {
                        entry.operands[0]
                    };
                    let Some(target) = self.object_mut(target_id) else {
                        return Err(AutoScriptError::MissingObject {
                            object_id,
                            entry: script_entry,
                            target_id,
                        });
                    };
                    target.world_x += i32::from(entry.operands[1] as i16);
                    target.world_y += i32::from(entry.operands[2] as i16);
                    target.advance_animation();
                    self.scene_objects[object_index].auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                SyncObjectState => {
                    let source_id = if entry.operands[0] == 0 || entry.operands[0] == 0xffff {
                        object_id
                    } else {
                        entry.operands[0]
                    };
                    let Some(source_state) = self.object_state(source_id) else {
                        return Err(AutoScriptError::MissingObject {
                            object_id,
                            entry: script_entry,
                            target_id: source_id,
                        });
                    };
                    if source_state == entry.operands[1] as i16 {
                        self.scene_objects[object_index].state = source_state;
                    }
                    self.scene_objects[object_index].auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                AnimateObject => {
                    let object = &mut self.scene_objects[object_index];
                    object.advance_animation();
                    object.auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                AutoScriptNoOp => {
                    self.scene_objects[object_index].auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                PrintMessage => {
                    self.scene_objects[object_index].auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                // Known instructions that the auto-script scheduler does not implement yet.
                Redraw
                | StartBattle
                | AdvanceEntry
                | Confirm
                | SetObjectPositionRelative
                | SetObjectPosition
                | SetPartyMemberPose
                | SetSelectedObjectPose
                | SetEquipmentEffect
                | EquipItem
                | AdjustPlayerAttribute
                | SetPlayerAttribute
                | AdjustPlayerHp
                | AdjustPlayerMp
                | AdjustPlayerHpMp
                | AdjustCash
                | AddItem
                | RemoveItem
                | DamageEnemy
                | RevivePlayer
                | RemoveEquipment
                | SetObjectAutoScript
                | OpenBuyMenu
                | OpenSellMenu
                | PoisonEnemy
                | PoisonPlayer
                | CureEnemyPoison
                | CurePlayerPoison
                | CurePoisonByLevel
                | SetPlayerStatus
                | SetEnemyStatus
                | RemovePlayerStatus
                | AdjustTemporaryPlayerStat
                | SetTemporaryBattleSprite
                | CollectEnemy
                | TransmuteCollectedEnemies
                | ShakeScreen
                | SelectRngAnimation
                | PlayRngAnimation
                | TeleportParty
                | DrainEnemyHp
                | FleeBattle
                | DialogCenter
                | DialogUpper
                | DialogLower
                | DialogCenterWindow
                | RideObjectSlow
                | MarkScriptFailed
                | SimulatePlayerMagic
                | PlayMusic
                | RideObject
                | SetBattleMusic
                | SetPartyPosition
                | SetBattlefield
                | WaitForKey
                | LoadLastSave
                | FadeToRed
                | FadeOut
                | FadeIn
                | HideObject
                | UseDayPalette
                | UseNightPalette
                | AddMagic
                | RemoveMagic
                | ScaleMagicByMp
                | JumpIfItemCountLess
                | ChangeScene
                | HalvePlayerHp
                | HalveEnemyHp
                | HideBattleActor
                | JumpIfPlayerLacksPoison
                | JumpIfEnemyLacksPoison
                | KillPlayer
                | KillEnemy
                | JumpIfPlayerNotPoisoned
                | PauseEnemyChase
                | SpeedUpEnemyChase
                | JumpIfEnemyHpAbove
                | SetPlayerSprite
                | ThrowWeapon
                | EnemyCastMagic
                | JumpIfEnemyTurn
                | EnemyEscape
                | StealEnemy
                | BlowEnemiesAway
                | SetSceneScripts
                | OffsetParty
                | WalkParty
                | SetScreenWave
                | FadeScene
                | JumpIfPartyNotFullHp
                | SetParty
                | ShowFbp
                | StopMusic
                | NoOp
                | JumpIfPartyContainsPlayer
                | WalkPartyFast
                | WalkPartyFastest
                | WalkObjectHalfSpeed
                | OffsetObject
                | SetObjectLayer
                | MoveViewport
                | ToggleDayNightPalette
                | JumpIfNotFacingObject
                | WalkObjectFast
                | JumpIfObjectOutsideZone
                | PlaceUsedItemObject
                | Delay
                | JumpIfItemNotEquipped
                | ScaleMagicByCash
                | SetBattleResult
                | EnableAutoBattle
                | SetPalette
                | FadeColor
                | LevelUpPlayer
                | RestoreScreen
                | HalveCash
                | SetObjectScript
                | JumpIfEnemyNotFirstKind
                | PlayerMagicAnimation
                | FadeSceneWithUpdate
                | JumpIfObjectStateEquals
                | JumpIfSceneEquals
                | PlayEndingAnimation
                | RideObjectFast
                | SetPartyFollower
                | SetSceneMap
                | SetObjectStates
                | FadeToCurrentScene
                | DivideEnemy
                | SummonEnemy
                | TransformEnemy
                | QuitGame
                | CollapseParty
                | RandomSelect
                | PlayCdMusic
                | ScrollFbp
                | ShowFbpWithSprite
                | BackupScreen => {
                    return Err(AutoScriptError::Unsupported {
                        object_id,
                        entry: script_entry,
                        opcode: opcode.raw(),
                    })
                }
            }
        }
        let object = &self.scene_objects[object_index];
        Err(AutoScriptError::InstructionLimit {
            object_id: object.id,
            entry: object.auto_script,
        })
    }

    fn run_immediate_auto_subscript(
        &mut self,
        scripts: &ScriptTable,
        mut script_entry: u16,
        object_id: u16,
        depth: usize,
    ) -> Result<(), AutoScriptError> {
        const MAX_CALL_DEPTH: usize = 32;
        const MAX_INSTRUCTIONS: usize = 1024;

        if depth >= MAX_CALL_DEPTH {
            return Err(AutoScriptError::InstructionLimit {
                object_id,
                entry: script_entry,
            });
        }
        for _ in 0..MAX_INSTRUCTIONS {
            let entry =
                scripts
                    .entry(script_entry)
                    .copied()
                    .ok_or(AutoScriptError::InvalidEntry {
                        object_id,
                        entry: script_entry,
                    })?;
            let Some(opcode) = ScriptOpcode::from_raw(entry.opcode) else {
                return Err(AutoScriptError::Unsupported {
                    object_id,
                    entry: script_entry,
                    opcode: entry.opcode,
                });
            };
            use ScriptOpcode::*;
            match opcode {
                Stop | StopAndAdvance | StopAndReplace => return Ok(()),
                Jump => {
                    script_entry = entry.operands[0];
                    continue;
                }
                Call => {
                    let called_object_id = if entry.operands[1] == 0 {
                        object_id
                    } else {
                        entry.operands[1]
                    };
                    self.run_immediate_auto_subscript(
                        scripts,
                        entry.operands[0],
                        called_object_id,
                        depth + 1,
                    )?;
                }
                AutoScriptNoOp => {}
                SetObjectPose => {
                    let Some(object) = self.object_mut(object_id) else {
                        return Err(AutoScriptError::MissingObject {
                            object_id,
                            entry: script_entry,
                            target_id: object_id,
                        });
                    };
                    if entry.operands[0] != 0xffff {
                        object.direction = Direction::from_pal(entry.operands[0]).ok_or(
                            AutoScriptError::Unsupported {
                                object_id,
                                entry: script_entry,
                                opcode: entry.opcode,
                            },
                        )?;
                    }
                    if entry.operands[1] != 0xffff {
                        object.current_frame = entry.operands[1];
                    }
                }
                SetObjectGesture => {
                    let Some(object) = self.object_mut(object_id) else {
                        return Err(AutoScriptError::MissingObject {
                            object_id,
                            entry: script_entry,
                            target_id: object_id,
                        });
                    };
                    object.direction = Direction::South;
                    object.current_frame = entry.operands[0];
                }
                PlaySound => self.pending_auto_sounds.push(entry.operands[0]),
                SetObjectState => {
                    let target_id = if entry.operands[0] == 0 || entry.operands[0] == 0xffff {
                        object_id
                    } else {
                        entry.operands[0]
                    };
                    let Some(target) = self.object_mut(target_id) else {
                        return Err(AutoScriptError::MissingObject {
                            object_id,
                            entry: script_entry,
                            target_id,
                        });
                    };
                    target.state = entry.operands[1] as i16;
                }
                HideObjectShort => {
                    let Some(object) = self.object_mut(object_id) else {
                        return Err(AutoScriptError::MissingObject {
                            object_id,
                            entry: script_entry,
                            target_id: object_id,
                        });
                    };
                    object.vanish_time = -15;
                }
                OffsetObjectAndAnimate => {
                    let target_id = if entry.operands[0] == 0 || entry.operands[0] == 0xffff {
                        object_id
                    } else {
                        entry.operands[0]
                    };
                    let Some(target) = self.object_mut(target_id) else {
                        return Err(AutoScriptError::MissingObject {
                            object_id,
                            entry: script_entry,
                            target_id,
                        });
                    };
                    target.world_x += i32::from(entry.operands[1] as i16);
                    target.world_y += i32::from(entry.operands[2] as i16);
                    target.advance_animation();
                }
                AnimateObject => {
                    let Some(object) = self.object_mut(object_id) else {
                        return Err(AutoScriptError::MissingObject {
                            object_id,
                            entry: script_entry,
                            target_id: object_id,
                        });
                    };
                    object.advance_animation();
                }
                // Known instructions that immediate auto-subscripts do not implement yet.
                Redraw
                | JumpByChance
                | StartBattle
                | AdvanceEntry
                | WaitFrames
                | Confirm
                | WalkObjectSouth
                | WalkObjectWest
                | WalkObjectNorth
                | WalkObjectEast
                | WalkObjectTo
                | WalkObjectToSlow
                | SetObjectPositionRelative
                | SetObjectPosition
                | SetPartyMemberPose
                | SetSelectedObjectPose
                | SetEquipmentEffect
                | EquipItem
                | AdjustPlayerAttribute
                | SetPlayerAttribute
                | AdjustPlayerHp
                | AdjustPlayerMp
                | AdjustPlayerHpMp
                | AdjustCash
                | AddItem
                | RemoveItem
                | DamageEnemy
                | RevivePlayer
                | RemoveEquipment
                | SetObjectAutoScript
                | SetObjectTriggerScript
                | OpenBuyMenu
                | OpenSellMenu
                | PoisonEnemy
                | PoisonPlayer
                | CureEnemyPoison
                | CurePlayerPoison
                | CurePoisonByLevel
                | SetPlayerStatus
                | SetEnemyStatus
                | RemovePlayerStatus
                | AdjustTemporaryPlayerStat
                | SetTemporaryBattleSprite
                | CollectEnemy
                | TransmuteCollectedEnemies
                | ShakeScreen
                | SelectRngAnimation
                | PlayRngAnimation
                | TeleportParty
                | DrainEnemyHp
                | FleeBattle
                | DialogCenter
                | DialogUpper
                | DialogLower
                | DialogCenterWindow
                | RideObjectSlow
                | SetObjectTriggerMode
                | MarkScriptFailed
                | SimulatePlayerMagic
                | PlayMusic
                | RideObject
                | SetBattleMusic
                | SetPartyPosition
                | SetBattlefield
                | ChasePlayer
                | WaitForKey
                | LoadLastSave
                | FadeToRed
                | FadeOut
                | FadeIn
                | HideObject
                | UseDayPalette
                | UseNightPalette
                | AddMagic
                | RemoveMagic
                | ScaleMagicByMp
                | JumpIfItemCountLess
                | ChangeScene
                | HalvePlayerHp
                | HalveEnemyHp
                | HideBattleActor
                | JumpIfPlayerLacksPoison
                | JumpIfEnemyLacksPoison
                | KillPlayer
                | KillEnemy
                | JumpIfPlayerNotPoisoned
                | PauseEnemyChase
                | SpeedUpEnemyChase
                | JumpIfEnemyHpAbove
                | SetPlayerSprite
                | ThrowWeapon
                | EnemyCastMagic
                | JumpIfEnemyTurn
                | EnemyEscape
                | StealEnemy
                | BlowEnemiesAway
                | SetSceneScripts
                | OffsetParty
                | SyncObjectState
                | WalkParty
                | SetScreenWave
                | FadeScene
                | JumpIfPartyNotFullHp
                | SetParty
                | ShowFbp
                | StopMusic
                | NoOp
                | JumpIfPartyContainsPlayer
                | WalkPartyFast
                | WalkPartyFastest
                | WalkObjectHalfSpeed
                | OffsetObject
                | SetObjectLayer
                | MoveViewport
                | ToggleDayNightPalette
                | JumpIfNotFacingObject
                | WalkObjectFast
                | JumpIfObjectOutsideZone
                | PlaceUsedItemObject
                | Delay
                | JumpIfItemNotEquipped
                | ScaleMagicByCash
                | SetBattleResult
                | EnableAutoBattle
                | SetPalette
                | FadeColor
                | LevelUpPlayer
                | RestoreScreen
                | HalveCash
                | SetObjectScript
                | JumpIfEnemyNotFirstKind
                | PlayerMagicAnimation
                | FadeSceneWithUpdate
                | JumpIfObjectStateEquals
                | JumpIfSceneEquals
                | PlayEndingAnimation
                | RideObjectFast
                | SetPartyFollower
                | SetSceneMap
                | SetObjectStates
                | FadeToCurrentScene
                | DivideEnemy
                | SummonEnemy
                | TransformEnemy
                | QuitGame
                | CollapseParty
                | RandomSelect
                | PlayCdMusic
                | ScrollFbp
                | ShowFbpWithSprite
                | BackupScreen
                | PrintMessage => {
                    return Err(AutoScriptError::Unsupported {
                        object_id,
                        entry: script_entry,
                        opcode: opcode.raw(),
                    })
                }
            }
            script_entry = script_entry.wrapping_add(1);
        }
        Err(AutoScriptError::InstructionLimit {
            object_id,
            entry: script_entry,
        })
    }

    fn chase_player(&mut self, object_index: usize, speed: u16, range: u16, floating: bool) {
        let player = (self.player.world_x, self.player.world_y);
        let object = &mut self.scene_objects[object_index];
        let x_offset = player.0 - object.world_x;
        let y_offset = player.1 - object.world_y;
        if i64::from(x_offset.abs()) + i64::from(y_offset.abs()) * 2 < i64::from(range) * 32 {
            object.direction = direction_toward(x_offset, y_offset);
            let (dx, dy) = object.direction.step_at_speed(i32::from(speed));
            let target = (object.world_x + dx, object.world_y + dy);
            if floating || !self.map.is_world_blocked(target.0, target.1) {
                object.world_x = target.0;
                object.world_y = target.1;
            }
        }
        object.advance_animation();
    }

    fn rebuild_party_followers(&mut self) {
        let mut follower_roles = self
            .party
            .members()
            .iter()
            .skip(1)
            .map(|member| member.attributes.clone())
            .collect::<Vec<_>>();
        if let Some(roles) = &self.player_roles {
            follower_roles.extend(
                self.extra_follower_ids
                    .iter()
                    .filter_map(|&role_id| roles.role(usize::from(role_id)).cloned()),
            );
        }
        self.party_followers = follower_roles
            .into_iter()
            .map(|attributes| Role {
                sprite_index: usize::from(attributes.scene_sprite_num),
                world_x: self.player.world_x,
                world_y: self.player.world_y,
                direction: self.player.direction,
                anim_frame: 0,
                frames_per_direction: attributes.frames_per_direction(),
            })
            .collect();
        self.update_party_followers(false);
    }

    fn record_party_step(&mut self, old_position: (i32, i32), direction: Direction) {
        self.party_trail.rotate_right(1);
        self.party_trail[0] = TrailPoint {
            world_x: old_position.0,
            world_y: old_position.1,
            direction,
        };
        self.update_party_followers(true);
    }

    fn update_party_followers(&mut self, walking: bool) {
        let base = self.party_trail[1];
        let facing = self.party_trail[2].direction;
        for (index, follower) in self.party_followers.iter_mut().enumerate() {
            let party_index = index + 1;
            let (dx, dy) = if party_index == 2 {
                (
                    if matches!(base.direction, Direction::East | Direction::West) {
                        -16
                    } else {
                        16
                    },
                    8,
                )
            } else {
                (
                    if matches!(base.direction, Direction::West | Direction::South) {
                        16
                    } else {
                        -16
                    },
                    if matches!(base.direction, Direction::West | Direction::North) {
                        8
                    } else {
                        -8
                    },
                )
            };
            follower.world_x = base.world_x + dx;
            follower.world_y = base.world_y + dy;
            follower.direction = facing;
            follower.anim_frame = if walking {
                (follower.anim_frame + 1) % follower.frames_per_direction.max(1)
            } else {
                0
            };
        }
    }

    fn collapse_party(&mut self) {
        let point = TrailPoint {
            world_x: self.player.world_x,
            world_y: self.player.world_y,
            direction: self.player.direction,
        };
        self.party_trail.fill(point);
        for follower in &mut self.party_followers {
            follower.world_x = self.player.world_x;
            follower.world_y = self.player.world_y - 1;
            follower.direction = self.player.direction;
            follower.anim_frame = 0;
        }
    }

    fn place_party(&mut self) {
        let (dx, dy) = (
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
        for (index, point) in self.party_trail.iter_mut().enumerate() {
            point.world_x = self.player.world_x + dx * index as i32;
            point.world_y = self.player.world_y + dy * index as i32;
            point.direction = self.player.direction;
        }
        for (index, follower) in self.party_followers.iter_mut().enumerate() {
            let distance = index as i32 + 1;
            follower.world_x = self.player.world_x + dx * distance;
            follower.world_y = self.player.world_y + dy * distance;
            follower.direction = self.player.direction;
            follower.anim_frame = 0;
        }
    }

    fn shift_party(&mut self, dx: i32, dy: i32) {
        let old_position = (self.player.world_x, self.player.world_y);
        self.player.world_x += dx;
        self.player.world_y += dy;
        if dx != 0 || dy != 0 {
            self.record_party_step(old_position, self.player.direction);
        }
        self.follow_player();
    }

    fn follow_player(&mut self) {
        if self.viewport_locked {
            return;
        }
        self.camera.follow(
            (self.player.world_x, self.player.world_y),
            self.map.world_size(),
        );
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

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn blocking_object(world_x: i32, world_y: i32) -> SceneObject {
        SceneObject {
            id: 1,
            world_x,
            world_y,
            layer: 0,
            trigger_script: 0,
            auto_script: 0,
            state: 2,
            trigger_mode: 0,
            sprite_index: Some(1),
            frames_per_direction: 3,
            sprite_frame_count: 12,
            direction: Direction::South,
            current_frame: 0,
            vanish_time: 0,
            auto_script_idle_frame: 0,
        }
    }

    struct TestMap {
        blocked: HashSet<(i32, i32)>,
        size: (i32, i32),
    }

    impl CollisionMap for TestMap {
        fn is_world_blocked(&self, world_x: i32, world_y: i32) -> bool {
            self.blocked.contains(&(world_x, world_y))
        }

        fn world_size(&self) -> (i32, i32) {
            self.size
        }
    }

    fn test_map() -> TestMap {
        TestMap {
            blocked: HashSet::new(),
            size: (1000, 800),
        }
    }

    fn state(blocked: &[(i32, i32)]) -> GameState<TestMap> {
        GameState::new(
            TestMap {
                blocked: blocked.iter().copied().collect(),
                size: (1000, 800),
            },
            Role {
                sprite_index: 0,
                world_x: 320,
                world_y: 240,
                direction: Direction::South,
                anim_frame: 0,
                frames_per_direction: 4,
            },
            320,
            200,
        )
    }

    fn battle_data_for_growth() -> BattleData {
        let mut chunks = vec![Vec::new(); 15];
        chunks[1] = vec![0; 70];
        chunks[1][22..24].copy_from_slice(&1000u16.to_le_bytes());
        chunks[2] = [1, u16::MAX, u16::MAX, u16::MAX, u16::MAX]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect();
        chunks[5] = vec![0; 12];
        chunks[6] = vec![0; 20];
        chunks[6][0..2].copy_from_slice(&2u16.to_le_bytes());
        chunks[6][2..4].copy_from_slice(&9u16.to_le_bytes());
        chunks[13] = vec![0; 100];
        chunks[14] = vec![0; 200];
        chunks[14][2..4].copy_from_slice(&10u16.to_le_bytes());

        let table_size = (chunks.len() + 1) * 4;
        let mut offset = table_size as u32;
        let mut archive = Vec::new();
        archive.extend_from_slice(&offset.to_le_bytes());
        for chunk in &chunks {
            offset += chunk.len() as u32;
            archive.extend_from_slice(&offset.to_le_bytes());
        }
        for chunk in chunks {
            archive.extend_from_slice(&chunk);
        }
        BattleData::parse(&archive).unwrap()
    }

    #[test]
    fn battle_experience_levels_living_roles_and_teaches_magic() {
        let mut role_data = vec![0; 900];
        for (array, value) in [
            (6, 1u16),
            (7, 100),
            (8, 50),
            (9, 20),
            (10, 10),
            (17, 30),
            (18, 20),
            (19, 25),
            (20, 15),
            (21, 12),
        ] {
            let offset = array * 12;
            role_data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        let roles = PlayerRoles::parse(&role_data).unwrap();
        let party = Party::single(0, &roles).unwrap();
        let mut state = state(&[])
            .with_party(party)
            .with_player_roles(roles)
            .with_battle_data(battle_data_for_growth());

        state.award_battle_experience(&[0], 25);

        let role = state.player_role(0).unwrap();
        assert_eq!(role.level, 2);
        assert_eq!(state.player_experience(0), Some(15));
        assert!(role.max_hp >= 110);
        assert_eq!(role.hp, role.max_hp);
        assert_eq!(role.mp, role.max_mp);
        assert_eq!(role.magic[0], 9);
        assert_eq!(state.party.leader().unwrap().attributes.level, 2);
    }

    #[test]
    fn battle_settlement_clears_temporary_statuses_and_low_level_poisons() {
        let mut role_data = vec![0; 900];
        for (array, value) in [(7, 100u16), (9, 100)] {
            let offset = array * PLAYER_ROLE_COUNT * 2;
            role_data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        let roles = PlayerRoles::parse(&role_data).unwrap();
        let party = Party::single(0, &roles).unwrap();
        let objects = GlobalObjects::parse(
            &[
                [0u16; 6],
                [0, 0, 0, 0, 0, 0],
                [2, 0, 0, 0, 0, 0],
                [4, 0, 0, 0, 0, 0],
            ]
            .into_iter()
            .flatten()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
            pal_assets::objects::ObjectLayout::Dos,
        )
        .unwrap();
        let stores = Stores::parse(&[0; 18]).unwrap();
        let magics = Magics::parse(&[0; 32]).unwrap();
        let scripts = ScriptTable::parse(&[0; 8]).unwrap();
        let mut state = state(&[])
            .with_party(party)
            .with_player_roles(roles)
            .with_economy_data(stores, objects)
            .with_magic_data(magics)
            .with_battle_data(battle_data_for_growth());
        assert!(state.player_statuses[0].set_for_player(BattleStatus::Protect, 5, true));
        assert!(state.player_statuses[0].set_for_player(BattleStatus::Haste, 1000, true));
        state.player_poisons[0][0] = BattlePoison {
            object_id: 2,
            script_entry: 20,
        };
        state.player_poisons[0][1] = BattlePoison {
            object_id: 3,
            script_entry: 30,
        };

        assert!(state.start_battle(
            BattleRequest {
                enemy_team: 0,
                lost_entry: 0,
                flee_entry: 0,
                is_boss: true,
            },
            &scripts,
        ));
        assert!(state.battle_mut().unwrap().set_script_result(0));
        assert_eq!(
            state.advance_battle_resolution(),
            vec![BattleEvent::Finished(BattleResult::Terminated)]
        );
        assert_eq!(
            state.settle_battle(),
            Some((BattleResult::Terminated, BattleRewards::default()))
        );
        assert_eq!(
            state.player_status_duration(0, BattleStatus::Protect),
            Some(0)
        );
        assert_eq!(
            state.player_status_duration(0, BattleStatus::Haste),
            Some(1000)
        );
        assert_eq!(state.player_poisons[0][0].object_id, 3);
        assert_eq!(state.player_poisons[0][1], BattlePoison::default());
    }

    #[test]
    fn walking_updates_position_animation_and_camera() {
        let mut state = state(&[]);
        assert!(state.update(GameInput {
            direction: Some(Direction::East),
            ..GameInput::default()
        }));
        assert_eq!((state.player.world_x, state.player.world_y), (336, 248));
        assert_eq!(state.player.anim_frame, 1);
        assert_eq!((state.camera.x, state.camera.y), (176, 148));
    }

    #[test]
    fn blocked_walking_only_changes_facing() {
        let mut state = state(&[(336, 232)]);
        assert!(state.update(GameInput {
            direction: Some(Direction::North),
            ..GameInput::default()
        }));
        assert_eq!((state.player.world_x, state.player.world_y), (320, 240));
        assert_eq!(state.player.direction, Direction::North);
        assert_eq!(state.player.anim_frame, 0);
    }

    #[test]
    fn blocked_walking_resets_an_active_animation() {
        let mut state = state(&[(336, 248)]);
        state.player.anim_frame = 2;
        assert!(state.update(GameInput {
            direction: Some(Direction::East),
            ..GameInput::default()
        }));
        assert_eq!((state.player.world_x, state.player.world_y), (320, 240));
        assert_eq!(state.player.anim_frame, 0);
    }

    #[test]
    fn event_object_blockers_prevent_walking() {
        let mut state = state(&[]).with_scene_objects(vec![blocking_object(336, 248)]);
        assert!(state.update(GameInput {
            direction: Some(Direction::East),
            ..GameInput::default()
        }));
        assert_eq!((state.player.world_x, state.player.world_y), (320, 240));
        assert_eq!(state.player.direction, Direction::East);
    }

    #[test]
    fn script_actions_update_object_trigger_fields() {
        let mut state = state(&[]).with_scene_objects(vec![blocking_object(320, 240)]);
        assert!(
            state.apply_script_action(ScriptAction::SetObjectAutoScript {
                object_id: 1,
                script_entry: 0x135d,
            })
        );
        assert!(
            state.apply_script_action(ScriptAction::SetObjectTriggerScript {
                object_id: 1,
                script_entry: 0x119e,
            })
        );
        assert!(
            state.apply_script_action(ScriptAction::SetObjectTriggerMode {
                object_id: 1,
                trigger_mode: 2,
            })
        );
        assert_eq!(state.scene_objects[0].auto_script, 0x135d);
        assert_eq!(state.scene_objects[0].trigger_script, 0x119e);
        assert_eq!(state.scene_objects[0].trigger_mode, 2);
    }

    #[test]
    fn touch_trigger_is_queued_and_pauses_exploration_until_consumed() {
        let mut object = blocking_object(330, 240);
        object.state = 1;
        object.trigger_mode = 4;
        object.trigger_script = 99;
        let mut state = state(&[]).with_scene_objects(vec![object]);

        assert!(state.update(GameInput {
            direction: Some(Direction::East),
            ..GameInput::default()
        }));
        assert_eq!((state.player.world_x, state.player.world_y), (320, 240));
        assert_eq!(state.pending_trigger.unwrap().script_entry, 99);
        assert!(!state.update(GameInput {
            direction: Some(Direction::East),
            ..GameInput::default()
        }));
        assert_eq!(state.take_trigger().unwrap().object_id, 1);
        assert!(state.pending_trigger.is_none());
    }

    #[test]
    fn confirm_queues_search_trigger_in_front_of_player() {
        let mut object = blocking_object(336, 248);
        object.state = 1;
        object.trigger_mode = 1;
        object.trigger_script = 77;
        let mut state = state(&[]).with_scene_objects(vec![object]);
        state.player.direction = Direction::East;

        assert!(state.update(GameInput {
            confirm: true,
            ..GameInput::default()
        }));
        let trigger = state.take_trigger().unwrap();
        assert_eq!(trigger.object_id, 1);
        assert_eq!(trigger.script_entry, 77);
        assert_eq!(state.scene_objects[0].direction, Direction::West);
    }

    #[test]
    fn script_actions_mutate_world_state_and_follow_player() {
        let mut state = state(&[]).with_scene_objects(vec![blocking_object(320, 200)]);
        assert!(state.apply_script_action(ScriptAction::MoveObject {
            object_id: 1,
            direction: Direction::East,
        }));
        assert_eq!(
            (
                state.scene_objects[0].world_x,
                state.scene_objects[0].world_y
            ),
            (324, 202)
        );
        assert_eq!(state.scene_objects[0].current_frame, 1);

        assert!(state.apply_script_action(ScriptAction::SetObjectState {
            object_id: 1,
            state: -1,
        }));
        assert_eq!(state.scene_objects[0].state, -1);
        assert!(state.apply_script_action(ScriptAction::OffsetPlayer { dx: 32, dy: 16 }));
        assert_eq!((state.player.world_x, state.player.world_y), (352, 256));
        assert_eq!((state.camera.x, state.camera.y), (192, 156));
        assert!(!state.apply_script_action(ScriptAction::SetObjectState {
            object_id: 99,
            state: 0,
        }));

        assert!(state.apply_script_action(ScriptAction::AddItem {
            item_id: 99,
            amount: 0,
        }));
        assert_eq!(state.item_count(99), 1);
        assert!(state.apply_script_action(ScriptAction::AddItem {
            item_id: 99,
            amount: -1,
        }));
        assert_eq!(state.item_count(99), 0);
        assert!(state.apply_script_action(ScriptAction::AddItem {
            item_id: 42,
            amount: 3,
        }));
        assert!(state.apply_script_action(ScriptAction::AddItem {
            item_id: 7,
            amount: 2,
        }));
        assert_eq!(state.inventory().collect::<Vec<_>>(), vec![(42, 3), (7, 2)]);
        assert!(state.apply_script_action(ScriptAction::PlayMusic {
            music_id: 6,
            looped: true,
            fade_seconds: 0,
        }));
        assert_eq!(state.current_music, Some(6));
        assert!(state.apply_script_action(ScriptAction::PlayMusic {
            music_id: 0,
            looped: false,
            fade_seconds: 0,
        }));
        assert_eq!(state.current_music, None);
        assert!(state.apply_script_action(ScriptAction::PlaySound { sound_id: 1 }));
        assert!(state.apply_script_action(ScriptAction::AdjustCash {
            amount: 20,
            insufficient_entry: 0,
        }));
        assert!(state.apply_script_action(ScriptAction::AdjustCash {
            amount: -7,
            insufficient_entry: 0,
        }));
        assert_eq!(state.cash, 13);
        assert!(!state.apply_script_action(ScriptAction::AdjustCash {
            amount: -14,
            insufficient_entry: 0,
        }));
        assert_eq!(state.cash, 13);
        assert!(state.apply_script_action(ScriptAction::SetPlayerPosition {
            tile_x: 10,
            tile_y: 12,
            half: 1,
        }));
        assert_eq!((state.player.world_x, state.player.world_y), (336, 200));
    }

    #[test]
    fn party_script_action_replaces_members_and_updates_leader_sprite() {
        let mut role_data = vec![0; 900];
        role_data[28..30].copy_from_slice(&42u16.to_le_bytes());
        role_data[772..774].copy_from_slice(&4u16.to_le_bytes());
        let player_roles = PlayerRoles::parse(&role_data).unwrap();
        let party = Party::single(0, &player_roles).unwrap();
        let mut state = state(&[]).with_party(party).with_player_roles(player_roles);

        assert!(state.apply_script_action(ScriptAction::SetParty {
            members: [Some(2), Some(1), None],
        }));
        assert_eq!(
            state
                .party
                .members()
                .iter()
                .map(|member| member.role_id)
                .collect::<Vec<_>>(),
            vec![2, 1]
        );
        assert_eq!(state.player.sprite_index, 42);
        assert_eq!(state.player.frames_per_direction, 4);
        assert_eq!(state.party_followers().len(), 1);
        assert_eq!(
            (
                state.party_followers()[0].world_x,
                state.party_followers()[0].world_y
            ),
            (320, 239)
        );
        assert!(state.apply_script_action(ScriptAction::SetPartyFollowers {
            followers: [Some(3), None],
        }));
        assert_eq!(state.party_followers().len(), 2);
        assert!(!state.apply_script_action(ScriptAction::SetPartyFollowers {
            followers: [Some(2), None],
        }));
        assert!(state.apply_script_action(ScriptAction::SetSceneMap {
            scene_number: None,
            map_number: 17,
        }));
        assert_eq!(state.scene_map_override(state.scene_number), Some(17));
        let snapshot = state
            .decode_snapshot(&state.encode_snapshot().unwrap())
            .unwrap();
        assert_eq!(snapshot.scene_map_override(), Some(17));
        state.restore_snapshot(snapshot, test_map());
        assert_eq!(state.party_followers().len(), 2);

        assert!(state.update(GameInput {
            direction: Some(Direction::East),
            ..GameInput::default()
        }));
        assert_ne!(
            (
                state.party_followers()[0].world_x,
                state.party_followers()[0].world_y
            ),
            (state.player.world_x, state.player.world_y - 1)
        );
        assert!(state.apply_script_action(ScriptAction::CollapseParty));
        assert_eq!(
            (
                state.party_followers()[0].world_x,
                state.party_followers()[0].world_y
            ),
            (state.player.world_x, state.player.world_y - 1)
        );
        assert!(state.apply_script_action(ScriptAction::SetPlayerPosition {
            tile_x: 10,
            tile_y: 12,
            half: 1,
        }));
        assert_eq!(
            (
                state.party_followers()[0].world_x,
                state.party_followers()[0].world_y
            ),
            (320, 192)
        );

        assert!(!state.apply_script_action(ScriptAction::SetParty {
            members: [Some(2), Some(2), None],
        }));
        assert_eq!(state.party.members().len(), 2);
    }

    #[test]
    fn batch_object_state_requires_a_complete_global_range() {
        let mut first = blocking_object(1, 1);
        first.id = 4;
        let mut second = blocking_object(2, 2);
        second.id = 5;
        let mut state = state(&[]).with_scene_objects(vec![first, second]);
        assert!(state.apply_script_action(ScriptAction::SetObjectStates {
            first_object_id: 4,
            last_object_id: 5,
            state: -2,
        }));
        assert_eq!(state.object_state(4), Some(-2));
        assert_eq!(state.object_state(5), Some(-2));
        assert!(!state.apply_script_action(ScriptAction::SetObjectStates {
            first_object_id: 4,
            last_object_id: 6,
            state: 1,
        }));
        assert_eq!(state.object_state(4), Some(-2));
    }

    #[test]
    fn item_removal_counts_and_unequips_active_party_items() {
        let mut role_data = vec![0; 900];
        let role_zero_weapon = 11 * PLAYER_ROLE_COUNT * 2;
        role_data[role_zero_weapon..role_zero_weapon + 2].copy_from_slice(&99u16.to_le_bytes());
        let roles = PlayerRoles::parse(&role_data).unwrap();
        let party = Party::single(0, &roles).unwrap();
        let mut state = state(&[]).with_party(party).with_player_roles(roles);
        assert!(state.apply_script_action(ScriptAction::AddItem {
            item_id: 99,
            amount: 1,
        }));
        assert_eq!(state.inventory_count(99), 1);
        assert_eq!(state.item_count(99), 2);

        assert!(!state.remove_item(99, 3, 77));
        assert_eq!(state.inventory_count(99), 1);
        assert_eq!(state.player_role(0).unwrap().equipment[0], 99);

        assert!(state.remove_item(99, 2, 77));
        assert_eq!(state.inventory_count(99), 0);
        assert_eq!(state.item_count(99), 0);
        assert_eq!(state.player_role(0).unwrap().equipment[0], 0);
        assert_eq!(state.party.members()[0].attributes.equipment[0], 0);
    }

    #[test]
    fn party_health_and_equipped_item_conditions_use_active_mutable_roles() {
        let mut role_data = vec![0; 900];
        let mut set_role_value = |array: usize, role: usize, value: u16| {
            let offset = array * PLAYER_ROLE_COUNT * 2 + role * 2;
            role_data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        };
        for role in 0..3 {
            set_role_value(7, role, 100);
            set_role_value(9, role, if role == 1 { 75 } else { 100 });
            set_role_value(11, role, 274);
        }
        let roles = PlayerRoles::parse(&role_data).unwrap();
        let mut party = Party::single(0, &roles).unwrap();
        assert!(party.add(1, &roles));
        let mut state = state(&[]).with_party(party).with_player_roles(roles);

        assert!(state.party_not_full_hp());
        assert_eq!(state.equipped_item_count(274), 2);
        state.player_roles.as_mut().unwrap().role_mut(1).unwrap().hp = 100;
        assert!(!state.party_not_full_hp());
        state
            .player_roles
            .as_mut()
            .unwrap()
            .role_mut(1)
            .unwrap()
            .equipment[0] = 0;
        assert_eq!(state.equipped_item_count(274), 1);
    }

    #[test]
    fn equipment_scripts_preserve_inventory_slots_and_refresh_effective_stats() {
        let mut roles = PlayerRoles::parse(&vec![0; 900]).unwrap();
        let role = roles.role_mut(0).unwrap();
        role.attack_strength = 10;
        role.hp = 20;
        let party = Party::single(0, &roles).unwrap();

        let mut object_data = vec![0; 24];
        for (index, word) in [
            0u16,
            0,
            0,
            42,
            0,
            ITEM_FLAG_EQUIPPABLE | ITEM_FLAG_ROLE_FIRST,
        ]
        .into_iter()
        .enumerate()
        {
            object_data[12 + index * 2..14 + index * 2].copy_from_slice(&word.to_le_bytes());
        }
        let objects =
            GlobalObjects::parse(&object_data, pal_assets::objects::ObjectLayout::Dos).unwrap();
        let stores = Stores::parse(&[0; 18]).unwrap();
        let mut state = state(&[])
            .with_party(party)
            .with_player_roles(roles)
            .with_economy_data(stores, objects);
        assert!(state.apply_script_action(ScriptAction::AddItem {
            item_id: 1,
            amount: 1,
        }));
        assert_eq!(state.equippable_item(1, 0).unwrap().script_entry, 42);
        assert_eq!(
            state.item_equip_request(1, 0).unwrap().kind,
            crate::scene::TriggerKind::Equip
        );

        assert!(state.apply_script_action(ScriptAction::EquipItem {
            role_id: 0,
            slot: 0,
            item_id: 1,
        }));
        assert!(state.apply_script_action(ScriptAction::SetEquipmentEffect {
            role_id: 0,
            attribute: 17,
            slot: 0,
            value: 5,
        }));
        assert!(
            state.apply_script_action(ScriptAction::ChangePlayerAttribute {
                role_id: 0,
                attribute: 4,
                value: 1,
                absolute: true,
            })
        );
        assert_eq!(state.inventory_count(1), 0);
        assert_eq!(state.player_role(0).unwrap().equipment[0], 1);
        let effective = state.effective_player_role(0).unwrap();
        assert_eq!(effective.attack_strength, 15);
        assert!(effective.attack_all);

        state.finish_item_equip(1, 43);
        assert!(state.apply_script_action(ScriptAction::RemoveEquipment {
            role_id: 0,
            slot: Some(0),
        }));
        assert_eq!(state.inventory().collect::<Vec<_>>(), vec![(1, 1)]);
        assert_eq!(state.effective_player_role(0).unwrap().attack_strength, 10);
    }

    #[test]
    fn battle_equipment_refresh_replays_existing_equipment_without_stacking() {
        let mut roles = PlayerRoles::parse(&vec![0; 900]).unwrap();
        let role = roles.role_mut(0).unwrap();
        role.equipment[0] = 1;
        role.attack_strength = 10;
        let party = Party::single(0, &roles).unwrap();

        let mut object_data = vec![0; 24];
        for (index, word) in [0u16, 0, 0, 1, 0, 0].into_iter().enumerate() {
            object_data[12 + index * 2..14 + index * 2].copy_from_slice(&word.to_le_bytes());
        }
        let objects =
            GlobalObjects::parse(&object_data, pal_assets::objects::ObjectLayout::Dos).unwrap();
        let stores = Stores::parse(&[0; 18]).unwrap();
        let script_data = [
            [ScriptOpcode::Stop.raw(), 0, 0, 0],
            [ScriptOpcode::EquipItem.raw(), 0x0b, 1, 0],
            [ScriptOpcode::SetEquipmentEffect.raw(), 0x0b, 17, 5],
            [ScriptOpcode::Stop.raw(), 0, 0, 0],
        ]
        .into_iter()
        .flatten()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
        let scripts = ScriptTable::parse(&script_data).unwrap();
        let mut state = state(&[])
            .with_party(party)
            .with_player_roles(roles)
            .with_economy_data(stores, objects);

        assert!(state.refresh_equipment_effects(&scripts));
        assert_eq!(state.effective_player_role(0).unwrap().attack_strength, 15);
        assert!(state.refresh_equipment_effects(&scripts));
        assert_eq!(state.effective_player_role(0).unwrap().attack_strength, 15);
        assert_eq!(state.player_role(0).unwrap().equipment[0], 1);
        assert_eq!(state.inventory_count(1), 0);

        let invalid_script_data = [
            [ScriptOpcode::Stop.raw(), 0, 0, 0],
            [ScriptOpcode::EquipItem.raw(), 0x0b, 1, 0],
            [ScriptOpcode::AddItem.raw(), 2, 1, 0],
            [ScriptOpcode::Stop.raw(), 0, 0, 0],
        ]
        .into_iter()
        .flatten()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
        let invalid_scripts = ScriptTable::parse(&invalid_script_data).unwrap();
        assert!(!state.refresh_equipment_effects(&invalid_scripts));
        assert_eq!(state.effective_player_role(0).unwrap().attack_strength, 15);
        assert_eq!(state.player_role(0).unwrap().equipment[0], 1);
        assert_eq!(state.inventory_count(1), 0);
    }

    #[test]
    fn field_magic_uses_object_scripts_and_consumes_mp_after_success() {
        let mut roles = PlayerRoles::parse(&vec![0; 900]).unwrap();
        let role = roles.role_mut(0).unwrap();
        role.hp = 20;
        role.mp = 10;
        role.magic[0] = 1;
        let party = Party::single(0, &roles).unwrap();

        let mut object_data = vec![0; 24];
        for (index, word) in [0u16, 0, 44, 43, 0, MAGIC_FLAG_USABLE_OUTSIDE_BATTLE]
            .into_iter()
            .enumerate()
        {
            object_data[12 + index * 2..14 + index * 2].copy_from_slice(&word.to_le_bytes());
        }
        let objects =
            GlobalObjects::parse(&object_data, pal_assets::objects::ObjectLayout::Dos).unwrap();
        let stores = Stores::parse(&[0; 18]).unwrap();
        let mut magic_data = vec![0; 32];
        magic_data[24..26].copy_from_slice(&3u16.to_le_bytes());
        let magics = Magics::parse(&magic_data).unwrap();
        let mut state = state(&[])
            .with_party(party)
            .with_player_roles(roles)
            .with_economy_data(stores, objects)
            .with_magic_data(magics);

        let magic = state.field_magics(0)[0];
        assert_eq!((magic.magic_id, magic.mp_cost), (1, 3));
        assert!(magic.enabled);
        assert_eq!(
            state
                .magic_request(0, 1, Some(0), false)
                .unwrap()
                .script_entry,
            43
        );
        assert_eq!(
            state
                .magic_request(0, 1, Some(0), true)
                .unwrap()
                .script_entry,
            44
        );
        state.finish_magic_script(1, 45, false);
        assert!(state.consume_magic_mp(0, 1));
        assert_eq!(state.player_role(0).unwrap().mp, 7);
        assert!(state.apply_script_action(ScriptAction::ChangeMagic {
            role_id: 0,
            magic_id: 1,
            add: false,
        }));
        assert!(state.field_magics(0).is_empty());
    }

    #[test]
    fn item_use_validates_targets_applies_recovery_and_consumes_on_success() {
        let mut role_data = vec![0; 900];
        let mut set_role_value = |array: usize, role: usize, value: u16| {
            let offset = array * PLAYER_ROLE_COUNT * 2 + role * 2;
            role_data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        };
        for role in 0..2 {
            set_role_value(7, role, 100);
            set_role_value(8, role, 80);
            set_role_value(9, role, if role == 0 { 40 } else { 0 });
            set_role_value(10, role, 20);
        }
        let roles = PlayerRoles::parse(&role_data).unwrap();
        let mut party = Party::single(0, &roles).unwrap();
        assert!(party.add(1, &roles));
        let stores = Stores::parse(&[0; 18]).unwrap();
        let objects = GlobalObjects::parse(
            &[
                [0u16; 6],
                [0, 0, 123, 0, 0, ITEM_FLAG_USABLE | ITEM_FLAG_CONSUMING],
                [
                    0,
                    0,
                    200,
                    0,
                    0,
                    ITEM_FLAG_USABLE | ITEM_FLAG_CONSUMING | ITEM_FLAG_APPLY_TO_ALL,
                ],
            ]
            .into_iter()
            .flatten()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
            pal_assets::objects::ObjectLayout::Dos,
        )
        .unwrap();
        let mut state = state(&[])
            .with_party(party)
            .with_player_roles(roles)
            .with_economy_data(stores, objects);
        for item_id in [1, 2] {
            assert!(state.apply_script_action(ScriptAction::AddItem { item_id, amount: 1 }));
        }

        let request = state.item_use_request(1, Some(0)).unwrap();
        assert_eq!(request.object_id, 0);
        assert_eq!(request.script_entry, 123);
        assert_eq!(request.kind, crate::scene::TriggerKind::Item);
        assert!(state.item_use_request(1, Some(5)).is_none());
        assert!(state.item_use_request(1, None).is_none());
        assert!(state.item_use_request(2, Some(0)).is_none());
        assert_eq!(state.item_use_request(2, None).unwrap().object_id, 0xffff);

        assert!(state.apply_script_action(ScriptAction::AdjustPlayerHealth {
            role_id: 0,
            hp: 50,
            mp: 30,
            apply_to_all: false,
        }));
        assert_eq!(state.player_role(0).unwrap().hp, 90);
        assert_eq!(state.player_role(0).unwrap().mp, 50);
        assert!(state.finish_item_use(1, 321, false));
        assert_eq!(state.inventory_count(1), 1);
        assert_eq!(
            state.item_use_request(1, Some(0)).unwrap().script_entry,
            321
        );
        let saved = state
            .decode_snapshot(&state.encode_snapshot().unwrap())
            .unwrap();
        state.restore_snapshot(saved, test_map());
        assert_eq!(
            state.item_use_request(1, Some(0)).unwrap().script_entry,
            321
        );

        assert!(state.apply_script_action(ScriptAction::RevivePlayer {
            role_id: 0xffff,
            hp_tenths: 3,
            apply_to_all: true,
        }));
        assert_eq!(state.player_role(1).unwrap().hp, 30);
        assert_eq!(state.party.members()[1].attributes.hp, 30);

        assert!(state.finish_item_use(1, 321, true));
        assert_eq!(state.inventory_count(1), 0);
        assert!(state.item_use_request(1, Some(0)).is_none());
    }

    #[test]
    fn recovery_rejects_dead_or_full_targets_without_mutating_them() {
        let mut role_data = vec![0; 900];
        for (array, value) in [(7, 100u16), (8, 80), (9, 100), (10, 80)] {
            let offset = array * PLAYER_ROLE_COUNT * 2;
            role_data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        let roles = PlayerRoles::parse(&role_data).unwrap();
        let party = Party::single(0, &roles).unwrap();
        let mut state = state(&[]).with_party(party).with_player_roles(roles);
        assert!(
            !state.apply_script_action(ScriptAction::AdjustPlayerHealth {
                role_id: 0,
                hp: 50,
                mp: 50,
                apply_to_all: false,
            })
        );
        assert!(!state.apply_script_action(ScriptAction::RevivePlayer {
            role_id: 0,
            hp_tenths: 5,
            apply_to_all: false,
        }));
        state.player_roles.as_mut().unwrap().role_mut(0).unwrap().hp = 0;
        assert!(
            !state.apply_script_action(ScriptAction::AdjustPlayerHealth {
                role_id: 0,
                hp: 50,
                mp: 0,
                apply_to_all: false,
            })
        );
        assert_eq!(state.player_role(0).unwrap().hp, 0);
    }

    #[test]
    fn store_transactions_use_prices_flags_and_inventory() {
        let stores = Stores::parse(
            &[2u16, 0, 0, 0, 0, 0, 0, 0, 0]
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let objects = GlobalObjects::parse(
            &[[0u16; 6], [0u16; 6], [0, 100, 0, 0, 0, ITEM_FLAG_SELLABLE]]
                .into_iter()
                .flatten()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
            pal_assets::objects::ObjectLayout::Dos,
        )
        .unwrap();
        let mut state = state(&[]).with_economy_data(stores, objects);
        state.cash = 120;
        assert_eq!(
            state.store_items(0).unwrap(),
            vec![StoreItem {
                item_id: 2,
                price: 100,
            }]
        );
        assert!(state.buy_item(2));
        assert_eq!(state.cash, 20);
        assert_eq!(state.inventory_count(2), 1);
        assert!(!state.buy_item(2));
        assert!(state.sell_item(2));
        assert_eq!(state.cash, 70);
        assert_eq!(state.inventory_count(2), 0);
    }

    #[test]
    fn player_facing_condition_checks_current_scene_range_and_state() {
        let mut object = blocking_object(304, 248);
        object.state = 1;
        let mut state = state(&[]).with_scene_objects(vec![object]);
        state.player.direction = Direction::South;
        assert!(state.player_faces_object(1, 0));
        state.player.direction = Direction::East;
        assert!(!state.player_faces_object(1, 0));

        state.scene_objects[0].world_x = 320;
        state.scene_objects[0].world_y = 240;
        assert!(state.player_faces_object(1, 1));
        assert_eq!(state.scene_objects[0].trigger_mode, 6);
        state.scene_objects[0].state = 0;
        assert!(!state.player_faces_object(1, 1));
        assert!(!state.player_faces_object(99, 1));
    }

    #[test]
    fn item_object_placement_requires_current_scene_and_clear_space() {
        let mut item_object = blocking_object(0, 0);
        item_object.id = 7;
        item_object.state = 0;
        let mut game = state(&[]).with_scene_objects(vec![item_object.clone()]);

        assert!(game.place_object_in_front(7, 2));
        assert_eq!(
            (game.scene_objects[0].world_x, game.scene_objects[0].world_y),
            (304, 248)
        );
        assert_eq!(game.object_state(7), Some(2));
        assert!(!game.place_object_in_front(99, 2));

        let mut blocked_by_map = state(&[(304, 248)]).with_scene_objects(vec![item_object.clone()]);
        assert!(!blocked_by_map.place_object_in_front(7, 2));
        assert_eq!(blocked_by_map.object_state(7), Some(0));

        let blocker = blocking_object(304, 248);
        let mut blocked_by_object = state(&[]).with_scene_objects(vec![item_object, blocker]);
        assert!(!blocked_by_object.place_object_in_front(7, 2));
        assert_eq!(blocked_by_object.object_state(7), Some(0));
    }

    #[test]
    fn inactive_global_objects_can_be_modified_before_scene_load() {
        let mut current = blocking_object(1, 1);
        current.id = 1;
        let mut second = blocking_object(2, 2);
        second.id = 2;
        let mut third = blocking_object(3, 3);
        third.id = 3;
        let mut state = state(&[])
            .with_scene_objects(vec![current.clone()])
            .with_global_objects(vec![current, second.clone(), third]);

        assert!(state.apply_script_action(ScriptAction::SetObjectState {
            object_id: 2,
            state: -1,
        }));
        assert!(
            state.apply_script_action(ScriptAction::SetObjectTriggerScript {
                object_id: 3,
                script_entry: 77,
            })
        );
        assert!(state.apply_script_action(ScriptAction::SetObjectStates {
            first_object_id: 1,
            last_object_id: 3,
            state: -2,
        }));
        assert_eq!(state.object_state(1), Some(-2));
        assert_eq!(state.object_state(2), Some(-2));
        assert_eq!(state.object_state(3), Some(-2));

        state.replace_scene(2, test_map(), vec![second]);
        assert_eq!(state.scene_objects[0].state, -2);
        let mut fresh_third = blocking_object(3, 3);
        fresh_third.id = 3;
        state.replace_scene(3, test_map(), vec![fresh_third]);
        assert_eq!(state.scene_objects[0].trigger_script, 77);
    }

    #[test]
    fn scene_object_state_survives_leaving_and_reentering() {
        let mut first = blocking_object(320, 200);
        first.trigger_script = 10;
        let mut state = state(&[])
            .with_scene_number(1)
            .with_scene_objects(vec![first]);
        assert!(state.apply_script_action(ScriptAction::SetObjectPosition {
            object_id: 1,
            x: 444,
            y: 222,
        }));
        assert!(state.apply_script_action(ScriptAction::SetObjectState {
            object_id: 1,
            state: -1,
        }));
        state.scene_objects[0].trigger_script = 11;
        state.scene_objects[0].current_frame = 3;

        let mut second = blocking_object(50, 60);
        second.id = 2;
        state.replace_scene(2, test_map(), vec![second]);
        state.scene_objects[0].world_x = 77;
        assert_eq!(state.inactive_objects.len(), 1);
        assert!(
            state.apply_script_action(ScriptAction::SetObjectTriggerScript {
                object_id: 1,
                script_entry: 12,
            })
        );
        assert_eq!(state.object_state(1), Some(-1));

        let mut fresh_first = blocking_object(1, 2);
        fresh_first.trigger_script = 10;
        state.replace_scene(1, test_map(), vec![fresh_first]);
        let restored = &state.scene_objects[0];
        assert_eq!((restored.world_x, restored.world_y), (444, 222));
        assert_eq!(restored.state, -1);
        assert_eq!(restored.trigger_script, 12);
        assert_eq!(restored.current_frame, 3);

        let mut fresh_second = blocking_object(5, 6);
        fresh_second.id = 2;
        state.replace_scene(2, test_map(), vec![fresh_second]);
        assert_eq!(state.scene_objects[0].world_x, 77);
    }

    #[test]
    fn snapshot_restores_global_and_scene_state() {
        let mut state = state(&[])
            .with_scene_number(4)
            .with_scene_objects(vec![blocking_object(320, 200)]);
        assert!(state.apply_script_action(ScriptAction::AddItem {
            item_id: 8,
            amount: 2,
        }));
        state.player.world_x = 500;
        state.scene_objects[0].state = -1;
        let snapshot = state.snapshot();

        state.player.world_x = 12;
        state.scene_objects[0].state = 2;
        assert!(state.apply_script_action(ScriptAction::AddItem {
            item_id: 8,
            amount: -2,
        }));
        state.restore_snapshot(snapshot, test_map());

        assert_eq!(state.scene_number, 4);
        assert_eq!(state.player.world_x, 500);
        assert_eq!(state.scene_objects[0].state, -1);
        assert_eq!(state.item_count(8), 2);
        assert_eq!((state.camera.x, state.camera.y), (340, 140));
    }

    #[test]
    fn scene_enter_script_entries_persist_per_scene_and_in_snapshots() {
        let mut state = state(&[]).with_scene_number(4);
        assert_eq!(state.scene_enter_script(100), 100);
        state.update_scene_enter_script(101);
        assert_eq!(state.scene_enter_script(999), 101);

        state.replace_scene(5, test_map(), Vec::new());
        assert_eq!(state.scene_enter_script(200), 200);
        state.update_scene_enter_script(201);
        let snapshot = state.snapshot();

        state.update_scene_enter_script(202);
        state.replace_scene(4, test_map(), Vec::new());
        assert_eq!(state.scene_enter_script(100), 101);

        state.restore_snapshot(snapshot, test_map());
        assert_eq!(state.scene_number, 5);
        assert_eq!(state.scene_enter_script(200), 201);
    }

    #[test]
    fn player_statuses_and_poisons_round_trip_and_follow_cure_rules() {
        let mut role_data = vec![0; 900];
        let hp_offset = 9 * PLAYER_ROLE_COUNT * 2;
        role_data[hp_offset..hp_offset + 2].copy_from_slice(&100u16.to_le_bytes());
        let roles = PlayerRoles::parse(&role_data).unwrap();
        let party = Party::single(0, &roles).unwrap();
        let objects = GlobalObjects::parse(
            &[[0u16; 6], [2, 4, 77, 0, 88, 0]]
                .into_iter()
                .flatten()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
            pal_assets::objects::ObjectLayout::Dos,
        )
        .unwrap();
        let stores = Stores::parse(&[0; 18]).unwrap();
        let mut state = state(&[])
            .with_party(party)
            .with_player_roles(roles)
            .with_economy_data(stores, objects);

        assert!(state.apply_script_action(ScriptAction::SetPlayerStatus {
            role_id: 0,
            status: BattleStatus::Protect as u16,
            rounds: 5,
        }));
        assert!(!state.apply_script_action(ScriptAction::SetPlayerStatus {
            role_id: 0,
            status: BattleStatus::Puppet as u16,
            rounds: 3,
        }));
        assert!(state.apply_script_action(ScriptAction::PoisonPlayer {
            role_id: 0,
            poison_id: 1,
            apply_to_all: false,
        }));
        assert_eq!(
            state.player_status_duration(0, BattleStatus::Protect),
            Some(5)
        );
        assert_eq!(state.player_poisons(0).unwrap()[0].object_id, 1);
        assert_eq!(state.player_poisons(0).unwrap()[0].script_entry, 77);

        let snapshot = state
            .decode_snapshot(&state.encode_snapshot().unwrap())
            .unwrap();
        state.remove_player_status(0, BattleStatus::Protect as u16);
        state.cure_player_poison(0, 1, false);
        state.restore_snapshot(snapshot, test_map());
        assert_eq!(
            state.player_status_duration(0, BattleStatus::Protect),
            Some(5)
        );
        assert_eq!(state.player_poisons(0).unwrap()[0].object_id, 1);
        assert!(
            state.apply_script_action(ScriptAction::CurePlayerPoisonByLevel {
                role_id: 0,
                maximum_level: 2,
                apply_to_all: false,
            })
        );
        assert_eq!(state.player_poisons(0).unwrap()[0], BattlePoison::default());
    }

    #[test]
    fn disk_snapshot_round_trips_and_rejects_invalid_data() {
        let player_roles = PlayerRoles::parse(&vec![0; 900]).unwrap();
        let mut party = Party::single(0, &player_roles).unwrap();
        assert!(party.add(1, &player_roles));
        let mut state = state(&[])
            .with_scene_number(3)
            .with_scene_objects(vec![blocking_object(40, 50)])
            .with_party(party)
            .with_player_roles(player_roles);
        assert!(state.apply_script_action(ScriptAction::AddItem {
            item_id: 9,
            amount: 4,
        }));
        assert!(state.apply_script_action(ScriptAction::AddItem {
            item_id: 7,
            amount: 2,
        }));
        assert!(state.adjust_cash(123));
        state.update_scene_enter_script(321);
        state.player_roles.as_mut().unwrap().role_mut(0).unwrap().hp = 321;
        assert!(state.apply_script_action(ScriptAction::SetEquipmentEffect {
            role_id: 0,
            attribute: 17,
            slot: 0,
            value: 7,
        }));
        state.finish_item_equip(9, 44);
        state.finish_magic_script(88, 55, false);
        state.finish_magic_script(88, 66, true);
        assert!(state.apply_script_action(ScriptAction::SetBattleMusic { music_id: 7 }));
        assert!(state.apply_script_action(ScriptAction::SetBattlefield { battlefield_id: 21 }));
        state.role_experience[0] = 42;

        let encoded = state.encode_snapshot().unwrap();
        let decoded = state.decode_snapshot(&encoded).unwrap();
        assert_eq!(decoded.scene_number(), 3);
        state.restore_snapshot(decoded, test_map());
        assert_eq!(state.party_followers().len(), 1);
        assert_eq!(state.item_count(9), 4);
        assert_eq!(state.inventory().collect::<Vec<_>>(), vec![(9, 4), (7, 2)]);
        assert_eq!(state.cash, 123);
        assert_eq!(state.current_battle_music, 7);
        assert_eq!(state.current_battlefield, 21);
        assert_eq!(state.player_experience(0), Some(42));
        assert_eq!(state.player_role(0).unwrap().hp, 321);
        assert_eq!(state.effective_player_role(0).unwrap().attack_strength, 7);
        assert_eq!(state.scene_enter_script(0), 321);
        assert_eq!(
            (
                state.scene_objects[0].world_x,
                state.scene_objects[0].world_y
            ),
            (40, 50)
        );

        assert!(state.decode_snapshot(b"not json").is_none());
        assert!(state
            .decode_snapshot(&vec![b' '; 4 * 1024 * 1024 + 1])
            .is_none());
        let wrong_version = String::from_utf8(encoded)
            .unwrap()
            .replace("\"version\":17", "\"version\":16");
        assert!(state.decode_snapshot(wrong_version.as_bytes()).is_none());
    }

    #[test]
    fn auto_script_moves_object_and_enables_scene_exit() {
        let script_data = [
            [0x0000, 0, 0, 0],
            [0x0010, 4, 6, 0],
            [0x0049, 4, 1, 0],
            [0x0049, 0xffff, 0, 0],
            [0x0000, 0, 0, 0],
        ]
        .into_iter()
        .flat_map(|entry| entry.into_iter().flat_map(u16::to_le_bytes))
        .collect::<Vec<_>>();
        let scripts = ScriptTable::parse(&script_data).unwrap();
        let mut door = blocking_object(128, 96);
        door.id = 4;
        door.state = 0;
        let mut mover = blocking_object(100, 100);
        mover.id = 11;
        mover.auto_script = 1;
        let mut state = state(&[]).with_scene_objects(vec![door, mover]);

        for _ in 0..3 {
            assert!(state.update_auto_scripts(&scripts).unwrap());
        }
        assert_eq!(state.object_state(4), Some(1));
        assert_eq!(state.object_state(11), Some(0));
        assert_eq!(
            (
                state.scene_objects[1].world_x,
                state.scene_objects[1].world_y
            ),
            (128, 96)
        );
    }

    #[test]
    fn trigger_walk_moves_one_step_until_reaching_the_target() {
        let mut object = blocking_object(80, 80);
        object.id = 7;
        let mut state = state(&[]).with_scene_objects(vec![object]);

        assert_eq!(state.walk_object_to(7, 4, 6, 0, 3), Some(false));
        assert_eq!(
            (
                state.scene_objects[0].world_x,
                state.scene_objects[0].world_y
            ),
            (86, 83)
        );
        while state.walk_object_to(7, 4, 6, 0, 3) == Some(false) {}
        assert_eq!(
            (
                state.scene_objects[0].world_x,
                state.scene_objects[0].world_y
            ),
            (128, 96)
        );
        assert_eq!(state.scene_objects[0].current_frame, 0);
    }

    #[test]
    fn party_walk_moves_over_multiple_ticks_and_updates_camera() {
        let mut state = state(&[]);
        assert_eq!(state.walk_player_to(12, 16, 0, 2), Some(false));
        assert_eq!((state.player.world_x, state.player.world_y), (324, 242));
        assert_eq!(state.player.direction, Direction::East);
        assert_eq!((state.camera.x, state.camera.y), (164, 142));

        while state.walk_player_to(12, 16, 0, 2) == Some(false) {}
        assert_eq!((state.player.world_x, state.player.world_y), (384, 256));
        assert_eq!(state.player.anim_frame, 0);
    }

    #[test]
    fn scripted_viewport_stays_locked_until_restored() {
        let mut state = state(&[]);
        assert!(state.apply_script_action(ScriptAction::MoveViewport {
            x: 2,
            y: 3,
            frames: 1,
        }));
        let scripted_camera = (state.camera.x, state.camera.y);
        assert_eq!(scripted_camera, (162, 143));
        assert!(state.apply_script_action(ScriptAction::OffsetPlayer { dx: 32, dy: 16 }));
        assert_eq!((state.camera.x, state.camera.y), scripted_camera);

        assert!(state.apply_script_action(ScriptAction::MoveViewport {
            x: 0,
            y: 0,
            frames: 1,
        }));
        assert_eq!((state.camera.x, state.camera.y), (192, 156));
    }

    #[test]
    fn direct_viewport_position_matches_pal_tile_coordinates() {
        let mut state = state(&[]);
        assert!(state.apply_script_action(ScriptAction::MoveViewport {
            x: 1,
            y: 1,
            frames: -1,
        }));
        assert_eq!((state.camera.x, state.camera.y), (-128, -96));
    }

    #[test]
    fn object_transform_actions_preserve_animation_and_toggle_visibility() {
        let mut object = blocking_object(80, 80);
        object.id = 7;
        object.state = 2;
        object.current_frame = 3;
        let mut state = state(&[]).with_scene_objects(vec![object]);

        assert!(state.apply_script_action(ScriptAction::MoveObjectBy {
            object_id: 7,
            dx: -4,
            dy: 2,
        }));
        assert!(state.apply_script_action(ScriptAction::SetObjectLayer {
            object_id: 7,
            layer: -10,
        }));
        assert!(
            state.apply_script_action(ScriptAction::HideObjectTemporarily {
                object_id: 7,
                vanish_time: 150,
            })
        );
        let object = &state.scene_objects[0];
        assert_eq!((object.world_x, object.world_y), (76, 82));
        assert_eq!(object.current_frame, 3);
        assert_eq!(object.layer, -10);
        assert_eq!(object.state, -2);
        assert_eq!(object.vanish_time, 150);
    }

    #[test]
    fn hidden_object_returns_after_its_timer_expires_offscreen() {
        let mut object = blocking_object(700, 700);
        object.state = -2;
        object.vanish_time = 1;
        object.current_frame = 3;
        let mut state = state(&[]).with_scene_objects(vec![object]);

        assert!(state.update(GameInput::default()));
        assert_eq!(state.scene_objects[0].vanish_time, 0);
        assert_eq!(state.scene_objects[0].state, -2);
        assert!(state.update(GameInput::default()));
        assert_eq!(state.scene_objects[0].state, 2);
        assert_eq!(state.scene_objects[0].current_frame, 0);
    }

    #[test]
    fn non_leader_pose_only_changes_the_party_direction() {
        let mut state = state(&[]);
        state.player.anim_frame = 3;
        assert!(state.apply_script_action(ScriptAction::SetPlayerPose {
            direction: Direction::West,
            frame: 1,
            party_index: 2,
        }));
        assert_eq!(state.player.direction, Direction::West);
        assert_eq!(state.player.anim_frame, 3);

        assert!(state.apply_script_action(ScriptAction::SetPlayerPose {
            direction: Direction::South,
            frame: 1,
            party_index: 0,
        }));
        assert_eq!(state.player.anim_frame, 1);
    }

    #[test]
    fn auto_scripts_animate_and_queue_sound_effects() {
        let script_data = [
            [0x0000, 0, 0, 0],
            [0x0087, 0, 0, 0],
            [0x0047, 48, 0, 0],
            [0x0000, 0, 0, 0],
        ]
        .into_iter()
        .flat_map(|entry| entry.into_iter().flat_map(u16::to_le_bytes))
        .collect::<Vec<_>>();
        let scripts = ScriptTable::parse(&script_data).unwrap();
        let mut object = blocking_object(100, 100);
        object.auto_script = 1;
        let mut state = state(&[]).with_scene_objects(vec![object]);

        assert!(state.update_auto_scripts(&scripts).unwrap());
        assert_eq!(state.scene_objects[0].current_frame, 1);
        assert!(state.update_auto_scripts(&scripts).unwrap());
        assert_eq!(state.take_auto_script_sounds(), vec![48]);
        assert!(state.take_auto_script_sounds().is_empty());
    }

    #[test]
    fn auto_script_calls_immediate_world_subscript() {
        let script_data = [
            [0x0000, 0, 0, 0],
            [0x0004, 4, 2, 0],
            [0x0000, 0, 0, 0],
            [0x0000, 0, 0, 0],
            [0x0049, 0xffff, 1, 0],
            [0x0014, 2, 0, 0],
            [0x0000, 0, 0, 0],
        ]
        .into_iter()
        .flat_map(|entry| entry.into_iter().flat_map(u16::to_le_bytes))
        .collect::<Vec<_>>();
        let scripts = ScriptTable::parse(&script_data).unwrap();
        let mut caller = blocking_object(80, 80);
        caller.auto_script = 1;
        let mut target = blocking_object(100, 100);
        target.id = 2;
        target.state = 2;
        let mut state = state(&[]).with_scene_objects(vec![caller, target]);

        assert!(state.update_auto_scripts(&scripts).unwrap());
        assert_eq!(state.scene_objects[0].auto_script, 2);
        assert_eq!(state.scene_objects[1].state, 1);
        assert_eq!(state.scene_objects[1].direction, Direction::South);
        assert_eq!(state.scene_objects[1].current_frame, 2);
    }

    #[test]
    fn one_auto_script_error_does_not_freeze_later_objects() {
        let script_data = [[0x0000, 0, 0, 0], [0x1234, 0, 0, 0], [0x000b, 0, 0, 0]]
            .into_iter()
            .flat_map(|entry| entry.into_iter().flat_map(u16::to_le_bytes))
            .collect::<Vec<_>>();
        let scripts = ScriptTable::parse(&script_data).unwrap();
        let mut broken = blocking_object(80, 80);
        broken.auto_script = 1;
        let mut mover = blocking_object(100, 100);
        mover.id = 2;
        mover.auto_script = 2;
        let mut state = state(&[]).with_scene_objects(vec![broken, mover]);

        assert_eq!(
            state.update_auto_scripts(&scripts),
            Err(AutoScriptError::Unsupported {
                object_id: 1,
                entry: 1,
                opcode: 0x1234,
            })
        );
        assert_ne!(
            (
                state.scene_objects[1].world_x,
                state.scene_objects[1].world_y
            ),
            (100, 100)
        );
        assert_eq!(state.scene_objects[1].auto_script, 3);
    }

    #[test]
    fn auto_random_and_slow_walk_do_not_consume_idle_wait_frames() {
        let script_data = [
            [0x0000, 0, 0, 0],
            [0x0006, 101, 0, 0],
            [0x0011, 4, 6, 0],
            [0x0009, 3, 0, 0],
        ]
        .into_iter()
        .flat_map(|entry| entry.into_iter().flat_map(u16::to_le_bytes))
        .collect::<Vec<_>>();
        let scripts = ScriptTable::parse(&script_data).unwrap();
        let mut object = blocking_object(80, 80);
        object.auto_script = 1;
        let mut state = state(&[]).with_scene_objects(vec![object]);

        assert!(state.update_auto_scripts(&scripts).unwrap());
        assert_eq!(state.scene_objects[0].auto_script_idle_frame, 0);
        assert!(state.update_auto_scripts(&scripts).unwrap());
        assert_eq!(state.scene_objects[0].auto_script_idle_frame, 0);
    }

    #[test]
    fn idle_resets_animation_without_moving() {
        let mut state = state(&[]);
        state.player.anim_frame = 2;
        assert!(state.update(GameInput::default()));
        assert_eq!((state.player.world_x, state.player.world_y), (320, 240));
        assert_eq!(state.player.anim_frame, 0);
        assert!(!state.update(GameInput::default()));
    }

    #[test]
    fn camera_clamps_to_world_edges() {
        let mut camera = Camera::new(320, 200);
        camera.follow((10, 10), (1000, 800));
        assert_eq!((camera.x, camera.y), (0, 0));
        camera.follow((990, 790), (1000, 800));
        assert_eq!((camera.x, camera.y), (680, 600));
    }

    #[test]
    fn camera_handles_a_world_smaller_than_the_viewport() {
        let mut camera = Camera::new(320, 200);
        camera.follow((50, 40), (100, 80));
        assert_eq!((camera.x, camera.y), (0, 0));
    }

    #[test]
    fn original_save_restores_party_economy_scripts_and_viewport() {
        use pal_assets::save::{OriginalSave, DOS_SAVE_FIXED_SIZE};

        fn write_u16(data: &mut [u8], offset: usize, value: u16) {
            data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        fn write_i16(data: &mut [u8], offset: usize, value: i16) {
            data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }

        let mut bytes = vec![0; DOS_SAVE_FIXED_SIZE];
        write_i16(&mut bytes, 2, 100);
        write_i16(&mut bytes, 4, 50);
        write_u16(&mut bytes, 6, 0);
        write_u16(&mut bytes, 8, 1);
        write_u16(&mut bytes, 12, Direction::South as u16);
        write_u16(&mut bytes, 14, 31);
        write_u16(&mut bytes, 16, 5);
        write_u16(&mut bytes, 18, 9);
        write_u16(&mut bytes, 24, 17);
        bytes[40..44].copy_from_slice(&1234u32.to_le_bytes());
        write_u16(&mut bytes, 44, 0);
        write_i16(&mut bytes, 46, 160);
        write_i16(&mut bytes, 48, 112);
        write_u16(&mut bytes, 124, 42);
        write_u16(&mut bytes, 1408, 1);
        write_u16(&mut bytes, 1410, 77);
        write_u16(&mut bytes, 1728, 99);
        write_u16(&mut bytes, 1730, 3);
        write_u16(&mut bytes, 3264, 12);
        write_u16(&mut bytes, 3266, 123);
        write_u16(&mut bytes, 3268, 456);

        let save = OriginalSave::parse(&bytes).unwrap();
        let mut state = state(&[]);
        assert!(state.restore_original_save(save, test_map(), Vec::new()));
        assert_eq!(state.scene_number, 1);
        assert_eq!((state.camera.x, state.camera.y), (100, 50));
        assert_eq!((state.player.world_x, state.player.world_y), (260, 162));
        assert_eq!(state.party.members()[0].role_id, 0);
        assert_eq!(state.current_music, Some(31));
        assert_eq!(state.current_battle_music, 5);
        assert_eq!(state.current_battlefield, 9);
        assert_eq!(state.cash, 1234);
        assert_eq!(state.collect_value(), 17);
        assert_eq!(state.player_poisons(0).unwrap()[0].object_id, 1);
        assert_eq!(state.player_poisons(0).unwrap()[0].script_entry, 77);
        assert_eq!(state.inventory_count(99), 3);
        assert_eq!(state.player_experience(0), Some(42));
        assert_eq!(state.scene_enter_script(0), 123);
        assert_eq!(state.scene_teleport_script(0), 456);
    }
}
