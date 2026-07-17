//! Deterministic exploration state and update rules.

use std::collections::BTreeMap;

use pal_assets::objects::GlobalObjects;
use pal_assets::player_roles::{PlayerRole, PlayerRoles, PLAYER_ROLE_COUNT};
use pal_assets::script::ScriptTable;
use pal_assets::store::Stores;
use serde::{Deserialize, Serialize};

use crate::map::tile_to_world;
use crate::map::{Map, MAP_PIXEL_HEIGHT, MAP_PIXEL_WIDTH};
use crate::party::{Party, MAX_PARTY_MEMBERS};
use crate::role::{Direction, Role};
use crate::scene::{
    blocks_position, find_search_trigger, find_touch_trigger, SceneObject, TriggerRequest,
};
use crate::script::ScriptAction;

pub const UPDATE_INTERVAL_MS: u64 = 50;
const MAX_INVENTORY: usize = 1024;
const ITEM_FLAG_SELLABLE: u16 = 1 << 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreItem {
    pub item_id: u16,
    pub price: u16,
}

/// Platform-independent commands sampled for one fixed update.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct GameInput {
    pub direction: Option<Direction>,
    pub confirm: bool,
    pub cancel: bool,
}

/// Collision boundary required by the exploration state.
pub trait CollisionMap {
    fn is_world_blocked(&self, world_x: i32, world_y: i32) -> bool;
    fn world_size(&self) -> (i32, i32);
}

impl CollisionMap for Map {
    fn is_world_blocked(&self, world_x: i32, world_y: i32) -> bool {
        self.is_world_blocked(world_x, world_y)
    }

    fn world_size(&self) -> (i32, i32) {
        (MAP_PIXEL_WIDTH, MAP_PIXEL_HEIGHT)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Camera {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Camera {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            x: 0,
            y: 0,
            width,
            height,
        }
    }

    pub fn follow(&mut self, target: (i32, i32), world_size: (i32, i32)) {
        let max_x = (world_size.0 - self.width as i32).max(0);
        let max_y = (world_size.1 - self.height as i32).max(0);
        self.x = (target.0 - self.width as i32 / 2).clamp(0, max_x);
        self.y = (target.1 - self.height as i32 / 2).clamp(0, max_y);
    }
}

/// State for the current exploration scene.
pub struct GameState<M = Map> {
    pub scene_number: u16,
    pub map: M,
    pub player: Role,
    pub scene_objects: Vec<SceneObject>,
    pub pending_trigger: Option<TriggerRequest>,
    pub party: Party,
    pub current_music: Option<u16>,
    pub cash: u32,
    player_roles: Option<PlayerRoles>,
    stores: Option<Stores>,
    global_objects: Option<GlobalObjects>,
    inventory: BTreeMap<u16, u16>,
    inactive_objects: BTreeMap<u16, SceneObject>,
    scene_enter_scripts: BTreeMap<u16, u16>,
    scene_teleport_scripts: BTreeMap<u16, u16>,
    pending_auto_sounds: Vec<u16>,
    script_frame: u32,
    viewport_locked: bool,
    party_followers: Vec<Role>,
    party_trail: [TrailPoint; MAX_PARTY_MEMBERS],
    pub camera: Camera,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrailPoint {
    world_x: i32,
    world_y: i32,
    direction: Direction,
}

/// In-memory development snapshot of platform-independent mutable game state.
#[derive(Debug, Clone)]
pub struct GameSnapshot {
    scene_number: u16,
    player: Role,
    scene_objects: Vec<SceneObject>,
    pending_trigger: Option<TriggerRequest>,
    party: Party,
    current_music: Option<u16>,
    cash: u32,
    inventory: BTreeMap<u16, u16>,
    inactive_objects: BTreeMap<u16, SceneObject>,
    scene_enter_scripts: BTreeMap<u16, u16>,
    scene_teleport_scripts: BTreeMap<u16, u16>,
    script_frame: u32,
    viewport_locked: bool,
    camera_x: i32,
    camera_y: i32,
    party_followers: Vec<Role>,
    party_trail: [TrailPoint; MAX_PARTY_MEMBERS],
    player_roles: Option<PlayerRoles>,
}

impl GameSnapshot {
    pub fn scene_number(&self) -> u16 {
        self.scene_number
    }
}

// Version 9 stores all six mutable player-role records. Version 8 added the
// scripted viewport mode and position.
const SNAPSHOT_VERSION: u16 = 9;

#[derive(Serialize, Deserialize)]
struct SnapshotData {
    version: u16,
    scene_number: u16,
    player: SavedRole,
    scene_objects: Vec<SavedSceneObject>,
    party: Vec<u16>,
    current_music: Option<u16>,
    cash: u32,
    inventory: Vec<(u16, u16)>,
    inactive_objects: Vec<SavedSceneObject>,
    scene_enter_scripts: Vec<(u16, u16)>,
    scene_teleport_scripts: Vec<(u16, u16)>,
    script_frame: u32,
    viewport_locked: bool,
    camera_x: i32,
    camera_y: i32,
    party_followers: Vec<SavedRole>,
    party_trail: Vec<SavedTrailPoint>,
    player_roles: Vec<SavedPlayerRole>,
}

#[derive(Serialize, Deserialize)]
struct SavedPlayerRole {
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
}

#[derive(Serialize, Deserialize)]
struct SavedTrailPoint {
    world_x: i32,
    world_y: i32,
    direction: u16,
}

#[derive(Serialize, Deserialize)]
struct SavedRole {
    sprite_index: usize,
    world_x: i32,
    world_y: i32,
    direction: u16,
    anim_frame: u8,
    frames_per_direction: u8,
}

#[derive(Serialize, Deserialize)]
struct SavedSceneObject {
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
    fn into_role(self) -> Option<Role> {
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
        }
    }
}

impl SavedPlayerRole {
    fn into_role(self) -> PlayerRole {
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
    fn into_point(self) -> Option<TrailPoint> {
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
    fn into_object(self) -> Option<SceneObject> {
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
            cash: 0,
            player_roles: None,
            stores: None,
            global_objects: None,
            inventory: BTreeMap::new(),
            inactive_objects: BTreeMap::new(),
            scene_enter_scripts: BTreeMap::new(),
            scene_teleport_scripts: BTreeMap::new(),
            pending_auto_sounds: Vec::new(),
            script_frame: 0,
            viewport_locked: false,
            party_followers: Vec::new(),
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

    pub fn party_followers(&self) -> &[Role] {
        &self.party_followers
    }

    pub fn player_role(&self, role_id: u16) -> Option<&PlayerRole> {
        self.player_roles.as_ref()?.role(usize::from(role_id))
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

    pub fn inventory_count(&self, item_id: u16) -> u16 {
        self.inventory.get(&item_id).copied().unwrap_or(0)
    }

    /// Return inventory entries in stable object-ID order.
    pub fn inventory(&self) -> impl ExactSizeIterator<Item = (u16, u16)> + '_ {
        self.inventory
            .iter()
            .map(|(&item_id, &amount)| (item_id, amount))
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
        self.inventory.insert(item_id, amount + 1);
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
        if removed_from_inventory == inventory_amount {
            self.inventory.remove(&item_id);
        } else {
            self.inventory
                .insert(item_id, inventory_amount - removed_from_inventory);
        }

        let mut remaining = amount - removed_from_inventory;
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
                    for equipment in &mut role.equipment {
                        if *equipment == item_id {
                            *equipment = 0;
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
        remaining == 0 || insufficient_entry == 0
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

    pub fn snapshot(&self) -> GameSnapshot {
        GameSnapshot {
            scene_number: self.scene_number,
            player: self.player.clone(),
            scene_objects: self.scene_objects.clone(),
            pending_trigger: self.pending_trigger,
            party: self.party.clone(),
            current_music: self.current_music,
            cash: self.cash,
            inventory: self.inventory.clone(),
            inactive_objects: self.inactive_objects.clone(),
            scene_enter_scripts: self.scene_enter_scripts.clone(),
            scene_teleport_scripts: self.scene_teleport_scripts.clone(),
            script_frame: self.script_frame,
            viewport_locked: self.viewport_locked,
            camera_x: self.camera.x,
            camera_y: self.camera.y,
            party_followers: self.party_followers.clone(),
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
            cash: snapshot.cash,
            inventory: snapshot.inventory.into_iter().collect(),
            inactive_objects: snapshot
                .inactive_objects
                .values()
                .map(SavedSceneObject::from)
                .collect(),
            scene_enter_scripts: snapshot.scene_enter_scripts.into_iter().collect(),
            scene_teleport_scripts: snapshot.scene_teleport_scripts.into_iter().collect(),
            script_frame: snapshot.script_frame,
            viewport_locked: snapshot.viewport_locked,
            camera_x: snapshot.camera_x,
            camera_y: snapshot.camera_y,
            party_followers: snapshot
                .party_followers
                .iter()
                .map(SavedRole::from)
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
            || data.inventory.len() > MAX_INVENTORY
            || data.scene_objects.len() > MAX_OBJECTS
            || data.inactive_objects.len() > MAX_OBJECTS
            || data.scene_objects.len() + data.inactive_objects.len() > MAX_OBJECTS
        {
            return None;
        }
        let inactive_object_count = data.inactive_objects.len();
        let scene_enter_script_count = data.scene_enter_scripts.len();
        let scene_teleport_script_count = data.scene_teleport_scripts.len();
        let saved_roles: [PlayerRole; PLAYER_ROLE_COUNT] = data
            .player_roles
            .into_iter()
            .map(SavedPlayerRole::into_role)
            .collect::<Vec<_>>()
            .try_into()
            .ok()?;
        let player_roles = PlayerRoles::from_roles(saved_roles);
        let mut party = Party::default();
        if !party.replace(&data.party, &player_roles) {
            return None;
        }
        let party_followers = data
            .party_followers
            .into_iter()
            .map(SavedRole::into_role)
            .collect::<Option<Vec<_>>>()?;
        if party_followers.len() != party.members().len().saturating_sub(1) {
            return None;
        }
        let party_trail = data
            .party_trail
            .into_iter()
            .map(SavedTrailPoint::into_point)
            .collect::<Option<Vec<_>>>()?
            .try_into()
            .ok()?;
        let inventory = data.inventory.into_iter().collect::<BTreeMap<_, _>>();
        if inventory.len() > MAX_INVENTORY
            || inventory
                .iter()
                .any(|(&id, &amount)| id == 0 || amount == 0 || amount > 99)
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
        if inactive_objects.len() != inactive_object_count
            || scene_enter_scripts.len() != scene_enter_script_count
            || scene_teleport_scripts.len() != scene_teleport_script_count
            || scene_enter_scripts.contains_key(&0)
            || scene_teleport_scripts.contains_key(&0)
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
            cash: data.cash,
            inventory,
            inactive_objects,
            scene_enter_scripts,
            scene_teleport_scripts,
            script_frame: data.script_frame,
            viewport_locked: data.viewport_locked,
            camera_x: data.camera_x,
            camera_y: data.camera_y,
            party_followers,
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
        self.cash = snapshot.cash;
        self.inventory = snapshot.inventory;
        self.inactive_objects = snapshot.inactive_objects;
        self.scene_enter_scripts = snapshot.scene_enter_scripts;
        self.scene_teleport_scripts = snapshot.scene_teleport_scripts;
        self.script_frame = snapshot.script_frame;
        self.viewport_locked = snapshot.viewport_locked;
        self.camera.x = snapshot.camera_x;
        self.camera.y = snapshot.camera_y;
        self.party_followers = snapshot.party_followers;
        self.party_trail = snapshot.party_trail;
        self.player_roles = snapshot.player_roles;
        self.pending_auto_sounds.clear();
        self.follow_player();
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
                if updated == 0 {
                    self.inventory.remove(&item_id);
                } else {
                    self.inventory.insert(item_id, updated);
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
                self.rebuild_party_followers();
                self.collapse_party();
            }
        }
        true
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
            match entry.opcode {
                0x0000 => return Ok(false),
                0x0001 => {
                    self.scene_objects[object_index].auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                0x0002 => {
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
                0x0003 => {
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
                0x0004 => {
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
                0x0006 => {
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
                0x0009 => {
                    let object = &mut self.scene_objects[object_index];
                    object.auto_script_idle_frame = object.auto_script_idle_frame.wrapping_add(1);
                    if object.auto_script_idle_frame >= entry.operands[0] {
                        object.auto_script_idle_frame = 0;
                        object.auto_script = script_entry.wrapping_add(1);
                    }
                    return Ok(true);
                }
                0x000f => {
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
                0x000b..=0x000e => {
                    let direction =
                        Direction::from_pal(entry.opcode - 0x000b).expect("valid range");
                    let object = &mut self.scene_objects[object_index];
                    object.direction = direction;
                    let (dx, dy) = direction.step_at_speed(2);
                    object.world_x += dx;
                    object.world_y += dy;
                    object.advance_animation();
                    object.auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                0x0010 | 0x0011 => {
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
                    let should_move = entry.opcode == 0x0010
                        || !self
                            .script_frame
                            .wrapping_add(u32::from(object_id))
                            .is_multiple_of(2);
                    if should_move
                        && walk_scene_object_to(
                            object,
                            target,
                            if entry.opcode == 0x0010 { 3 } else { 2 },
                        )
                    {
                        object.auto_script = script_entry.wrapping_add(1);
                    }
                    return Ok(true);
                }
                0x0014 => {
                    let object = &mut self.scene_objects[object_index];
                    object.direction = Direction::South;
                    object.current_frame = entry.operands[0];
                    object.auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                0x0025 => {
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
                0x0040 => {
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
                0x0049 => {
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
                0x0047 => {
                    self.pending_auto_sounds.push(entry.operands[0]);
                    self.scene_objects[object_index].auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                0x004b => {
                    let object = &mut self.scene_objects[object_index];
                    object.vanish_time = -15;
                    object.auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                0x004c => {
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
                0x006c => {
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
                0x006f => {
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
                0x0087 => {
                    let object = &mut self.scene_objects[object_index];
                    object.advance_animation();
                    object.auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                0xffff => {
                    self.scene_objects[object_index].auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                opcode => {
                    return Err(AutoScriptError::Unsupported {
                        object_id,
                        entry: script_entry,
                        opcode,
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
            match entry.opcode {
                0x0000..=0x0002 => return Ok(()),
                0x0003 => {
                    script_entry = entry.operands[0];
                    continue;
                }
                0x0004 => {
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
                0x000f => {
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
                0x0014 => {
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
                0x0047 => self.pending_auto_sounds.push(entry.operands[0]),
                0x0049 => {
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
                0x004b => {
                    let Some(object) = self.object_mut(object_id) else {
                        return Err(AutoScriptError::MissingObject {
                            object_id,
                            entry: script_entry,
                            target_id: object_id,
                        });
                    };
                    object.vanish_time = -15;
                }
                0x006c => {
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
                0x0087 => {
                    let Some(object) = self.object_mut(object_id) else {
                        return Err(AutoScriptError::MissingObject {
                            object_id,
                            entry: script_entry,
                            target_id: object_id,
                        });
                    };
                    object.advance_animation();
                }
                opcode => {
                    return Err(AutoScriptError::Unsupported {
                        object_id,
                        entry: script_entry,
                        opcode,
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
        self.party_followers = self
            .party
            .members()
            .iter()
            .skip(1)
            .map(|member| Role {
                sprite_index: usize::from(member.attributes.scene_sprite_num),
                world_x: self.player.world_x,
                world_y: self.player.world_y,
                direction: self.player.direction,
                anim_frame: 0,
                frames_per_direction: member.attributes.frames_per_direction(),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoScriptError {
    InvalidEntry {
        object_id: u16,
        entry: u16,
    },
    MissingObject {
        object_id: u16,
        entry: u16,
        target_id: u16,
    },
    Unsupported {
        object_id: u16,
        entry: u16,
        opcode: u16,
    },
    InstructionLimit {
        object_id: u16,
        entry: u16,
    },
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
        assert_eq!(state.inventory().collect::<Vec<_>>(), vec![(7, 2), (42, 3)]);
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
        assert!(state.adjust_cash(123));
        state.update_scene_enter_script(321);
        state.player_roles.as_mut().unwrap().role_mut(0).unwrap().hp = 321;

        let encoded = state.encode_snapshot().unwrap();
        let decoded = state.decode_snapshot(&encoded).unwrap();
        assert_eq!(decoded.scene_number(), 3);
        state.restore_snapshot(decoded, test_map());
        assert_eq!(state.party_followers().len(), 1);
        assert_eq!(state.item_count(9), 4);
        assert_eq!(state.cash, 123);
        assert_eq!(state.player_role(0).unwrap().hp, 321);
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
            .replace("\"version\":9", "\"version\":8");
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
}
