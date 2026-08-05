use std::collections::BTreeMap;

use pal_assets::player_roles::{PlayerRole, PlayerRoles};
use serde::{Deserialize, Serialize};

use crate::battle::{BattlePoison, BattleStatuses, BATTLE_STATUS_COUNT, MAX_BATTLE_POISONS};

use super::{Direction, Party, Role, SceneObject, TrailPoint, TriggerRequest, MAX_PARTY_MEMBERS};

/// In-memory development snapshot of platform-independent mutable game state.
#[derive(Debug, Clone)]
pub struct GameSnapshot {
    pub(super) scene_number: u16,
    pub(super) player: Role,
    pub(super) scene_objects: Vec<SceneObject>,
    pub(super) pending_trigger: Option<TriggerRequest>,
    pub(super) party: Party,
    pub(super) current_music: Option<u16>,
    pub(super) current_battle_music: u16,
    pub(super) current_battlefield: u16,
    pub(super) role_experience: [u32; pal_assets::player_roles::PLAYER_ROLE_COUNT],
    pub(super) growth_random_state: u32,
    pub(super) player_statuses: [BattleStatuses; pal_assets::player_roles::PLAYER_ROLE_COUNT],
    pub(super) player_poisons:
        [[BattlePoison; MAX_BATTLE_POISONS]; pal_assets::player_roles::PLAYER_ROLE_COUNT],
    pub(super) collect_value: u16,
    pub(super) cash: u32,
    pub(super) inventory: Vec<(u16, u16)>,
    pub(super) item_use_scripts: BTreeMap<u16, u16>,
    pub(super) item_equip_scripts: BTreeMap<u16, u16>,
    pub(super) item_throw_scripts: BTreeMap<u16, u16>,
    pub(super) magic_use_scripts: BTreeMap<u16, u16>,
    pub(super) magic_success_scripts: BTreeMap<u16, u16>,
    pub(super) equipment_effects: BTreeMap<(u16, u16, u16), i16>,
    pub(super) inactive_objects: BTreeMap<u16, SceneObject>,
    pub(super) scene_enter_scripts: BTreeMap<u16, u16>,
    pub(super) scene_teleport_scripts: BTreeMap<u16, u16>,
    pub(super) scene_maps: BTreeMap<u16, u16>,
    pub(super) script_frame: u32,
    pub(super) viewport_locked: bool,
    pub(super) camera_x: i32,
    pub(super) camera_y: i32,
    pub(super) party_followers: Vec<Role>,
    pub(super) extra_follower_ids: Vec<u16>,
    pub(super) party_trail: [TrailPoint; MAX_PARTY_MEMBERS],
    pub(super) player_roles: Option<PlayerRoles>,
}

impl GameSnapshot {
    pub fn scene_number(&self) -> u16 {
        self.scene_number
    }

    pub fn scene_map_override(&self) -> Option<u16> {
        self.scene_maps.get(&self.scene_number).copied()
    }
}

// Version 18 preserves mutable battle item throw-script entries.
pub(super) const SNAPSHOT_VERSION: u16 = 18;

#[derive(Serialize, Deserialize)]
pub(super) struct SnapshotData {
    pub(super) version: u16,
    pub(super) scene_number: u16,
    pub(super) player: SavedRole,
    pub(super) scene_objects: Vec<SavedSceneObject>,
    pub(super) party: Vec<u16>,
    pub(super) current_music: Option<u16>,
    pub(super) current_battle_music: u16,
    pub(super) current_battlefield: u16,
    pub(super) role_experience: [u32; pal_assets::player_roles::PLAYER_ROLE_COUNT],
    pub(super) growth_random_state: u32,
    pub(super) player_statuses:
        [[u16; BATTLE_STATUS_COUNT]; pal_assets::player_roles::PLAYER_ROLE_COUNT],
    pub(super) player_poisons:
        [[(u16, u16); MAX_BATTLE_POISONS]; pal_assets::player_roles::PLAYER_ROLE_COUNT],
    pub(super) collect_value: u16,
    pub(super) cash: u32,
    pub(super) inventory: Vec<(u16, u16)>,
    pub(super) item_use_scripts: Vec<(u16, u16)>,
    pub(super) item_equip_scripts: Vec<(u16, u16)>,
    pub(super) item_throw_scripts: Vec<(u16, u16)>,
    pub(super) magic_use_scripts: Vec<(u16, u16)>,
    pub(super) magic_success_scripts: Vec<(u16, u16)>,
    pub(super) equipment_effects: Vec<(u16, u16, u16, i16)>,
    pub(super) inactive_objects: Vec<SavedSceneObject>,
    pub(super) scene_enter_scripts: Vec<(u16, u16)>,
    pub(super) scene_teleport_scripts: Vec<(u16, u16)>,
    pub(super) scene_maps: Vec<(u16, u16)>,
    pub(super) script_frame: u32,
    pub(super) viewport_locked: bool,
    pub(super) camera_x: i32,
    pub(super) camera_y: i32,
    pub(super) party_followers: Vec<SavedRole>,
    pub(super) extra_follower_ids: Vec<u16>,
    pub(super) party_trail: Vec<SavedTrailPoint>,
    pub(super) player_roles: Vec<SavedPlayerRole>,
}

#[derive(Serialize, Deserialize)]
pub(super) struct SavedPlayerRole {
    avatar: u16,
    battle_sprite_num: u16,
    scene_sprite_num: u16,
    name_word_id: u16,
    attack_all: bool,
    level: u16,
    max_hp: u16,
    max_mp: u16,
    hp: u16,
    mp: u16,
    equipment: [u16; 6],
    attack_strength: u16,
    magic_strength: u16,
    defense: u16,
    dexterity: u16,
    flee_rate: u16,
    poison_resistance: u16,
    elemental_resistance: [u16; 5],
    covered_by: u16,
    magic: [u16; 32],
    walk_frames: u16,
    cooperative_magic: u16,
    unknown_5: u16,
    unknown_6: u16,
    death_sound: u16,
    attack_sound: u16,
    weapon_sound: u16,
    critical_sound: u16,
    magic_sound: u16,
    cover_sound: u16,
    dying_sound: u16,
}

#[derive(Serialize, Deserialize)]
pub(super) struct SavedTrailPoint {
    world_x: i32,
    world_y: i32,
    direction: u16,
}

#[derive(Serialize, Deserialize)]
pub(super) struct SavedRole {
    sprite_index: usize,
    world_x: i32,
    world_y: i32,
    direction: u16,
    anim_frame: u8,
    frames_per_direction: u8,
}

#[derive(Serialize, Deserialize)]
pub(super) struct SavedSceneObject {
    id: u16,
    world_x: i32,
    world_y: i32,
    layer: i16,
    trigger_script: u16,
    auto_script: u16,
    state: i16,
    trigger_mode: u16,
    sprite_index: Option<usize>,
    frames_per_direction: u16,
    sprite_frame_count: usize,
    direction: u16,
    current_frame: u16,
    vanish_time: i16,
    auto_script_idle_frame: u16,
}

impl From<&Role> for SavedRole {
    fn from(role: &Role) -> Self {
        Self {
            sprite_index: role.sprite_index,
            world_x: role.world_x,
            world_y: role.world_y,
            direction: role.direction as u16,
            anim_frame: role.anim_frame,
            frames_per_direction: role.frames_per_direction,
        }
    }
}

impl SavedRole {
    pub(super) fn into_role(self) -> Option<Role> {
        Some(Role {
            sprite_index: self.sprite_index,
            world_x: self.world_x,
            world_y: self.world_y,
            direction: Direction::from_pal(self.direction)?,
            anim_frame: self.anim_frame,
            frames_per_direction: self.frames_per_direction,
        })
    }
}

impl From<&PlayerRole> for SavedPlayerRole {
    fn from(role: &PlayerRole) -> Self {
        Self {
            avatar: role.avatar,
            battle_sprite_num: role.battle_sprite_num,
            scene_sprite_num: role.scene_sprite_num,
            name_word_id: role.name_word_id,
            attack_all: role.attack_all,
            level: role.level,
            max_hp: role.max_hp,
            max_mp: role.max_mp,
            hp: role.hp,
            mp: role.mp,
            equipment: role.equipment,
            attack_strength: role.attack_strength,
            magic_strength: role.magic_strength,
            defense: role.defense,
            dexterity: role.dexterity,
            flee_rate: role.flee_rate,
            poison_resistance: role.poison_resistance,
            elemental_resistance: role.elemental_resistance,
            covered_by: role.covered_by,
            magic: role.magic,
            walk_frames: role.walk_frames,
            cooperative_magic: role.cooperative_magic,
            unknown_5: role.unknown_5,
            unknown_6: role.unknown_6,
            death_sound: role.death_sound,
            attack_sound: role.attack_sound,
            weapon_sound: role.weapon_sound,
            critical_sound: role.critical_sound,
            magic_sound: role.magic_sound,
            cover_sound: role.cover_sound,
            dying_sound: role.dying_sound,
        }
    }
}

impl SavedPlayerRole {
    pub(super) fn into_role(self) -> PlayerRole {
        PlayerRole {
            avatar: self.avatar,
            battle_sprite_num: self.battle_sprite_num,
            scene_sprite_num: self.scene_sprite_num,
            name_word_id: self.name_word_id,
            attack_all: self.attack_all,
            level: self.level,
            max_hp: self.max_hp,
            max_mp: self.max_mp,
            hp: self.hp,
            mp: self.mp,
            equipment: self.equipment,
            attack_strength: self.attack_strength,
            magic_strength: self.magic_strength,
            defense: self.defense,
            dexterity: self.dexterity,
            flee_rate: self.flee_rate,
            poison_resistance: self.poison_resistance,
            elemental_resistance: self.elemental_resistance,
            covered_by: self.covered_by,
            magic: self.magic,
            walk_frames: self.walk_frames,
            cooperative_magic: self.cooperative_magic,
            unknown_5: self.unknown_5,
            unknown_6: self.unknown_6,
            death_sound: self.death_sound,
            attack_sound: self.attack_sound,
            weapon_sound: self.weapon_sound,
            critical_sound: self.critical_sound,
            magic_sound: self.magic_sound,
            cover_sound: self.cover_sound,
            dying_sound: self.dying_sound,
        }
    }
}

impl From<&TrailPoint> for SavedTrailPoint {
    fn from(point: &TrailPoint) -> Self {
        Self {
            world_x: point.world_x,
            world_y: point.world_y,
            direction: point.direction as u16,
        }
    }
}

impl SavedTrailPoint {
    pub(super) fn into_point(self) -> Option<TrailPoint> {
        Some(TrailPoint {
            world_x: self.world_x,
            world_y: self.world_y,
            direction: Direction::from_pal(self.direction)?,
        })
    }
}

impl From<&SceneObject> for SavedSceneObject {
    fn from(object: &SceneObject) -> Self {
        Self {
            id: object.id,
            world_x: object.world_x,
            world_y: object.world_y,
            layer: object.layer,
            trigger_script: object.trigger_script,
            auto_script: object.auto_script,
            state: object.state,
            trigger_mode: object.trigger_mode,
            sprite_index: object.sprite_index,
            frames_per_direction: object.frames_per_direction,
            sprite_frame_count: object.sprite_frame_count,
            direction: object.direction as u16,
            current_frame: object.current_frame,
            vanish_time: object.vanish_time,
            auto_script_idle_frame: object.auto_script_idle_frame,
        }
    }
}

impl SavedSceneObject {
    pub(super) fn into_object(self) -> Option<SceneObject> {
        Some(SceneObject {
            id: self.id,
            world_x: self.world_x,
            world_y: self.world_y,
            layer: self.layer,
            trigger_script: self.trigger_script,
            auto_script: self.auto_script,
            state: self.state,
            trigger_mode: self.trigger_mode,
            sprite_index: self.sprite_index,
            frames_per_direction: self.frames_per_direction,
            sprite_frame_count: self.sprite_frame_count,
            direction: Direction::from_pal(self.direction)?,
            current_frame: self.current_frame,
            vanish_time: self.vanish_time,
            auto_script_idle_frame: self.auto_script_idle_frame,
        })
    }
}
