use std::fmt::Debug;
use crate::utils::{ decode_c_structs, Result };
use crate::{ mkf::MKF, utils::open_mkf };
use bincode::Decode;

pub struct MKFs {
    pub rng: MKF, // RNG动画
    pub pat: MKF, // 调色板
    pub fbp: MKF, // 战斗背景sprites
    pub mgo: MKF, // 场景sprites
    pub midi: MKF, // MIDI音乐
    pub data: MKF, // 杂项数据文件
    pub map: MKF, // 地图
    pub gop: MKF, // tile bitmap
    pub sss: MKF, // 脚本数据
}

impl MKFs {
    pub fn open() -> Result<Self> {
        let rng = open_mkf("RNG.MKF")?;
        let pat = open_mkf("PAT.MKF")?;
        let fbp = open_mkf("FBP.MKF")?;
        let mgo = open_mkf("MGO.MKF")?;
        let midi = open_mkf("MIDI.MKF")?;
        let data = open_mkf("DATA.MKF")?;
        let map = open_mkf("MAP.MKF")?;
        let gop = open_mkf("GOP.MKF")?;
        let sss = open_mkf("SSS.MKF")?;

        Ok(Self { rng, pat, fbp, mgo, midi, data, map, gop, sss })
    }
}

#[derive(Debug, Decode, Clone)]
pub struct EventObject {
    pub vanish_time: u16, // vanish time (?)
    pub x: u16, // X coordinate on the map
    pub y: u16, // Y coordinate on the map
    pub layer: u16, // layer value
    pub trigger_script: u16, // Trigger script entry
    pub auto_script: u16, // Auto script entry
    pub state: u16, // state of this object
    pub trigger_mode: u16, // trigger mode
    pub sprite_num: u16, // number of the sprite
    pub sprite_frames: u16, // total number of frames of the sprite
    pub direction: u16, // direction
    pub current_frame_num: u16, // current frame number
    pub script_idle_frame: u16, // count of idle frames, used by trigger script
    pub sprite_ptr_offset: u16, // FIXME: ???
    pub sprite_frames_auto: u16, // total number of frames of the sprite, used by auto script
    pub script_idle_frame_count_auto: u16, // count of idle frames, used by auto script
}

#[derive(Debug, Decode)]
pub struct Scene {
    pub map_num: u16, // number of the map
    pub script_on_enter: u16, // when entering this scene, execute script from here
    pub script_on_teleport: u16, // when teleporting out of this scene, execute script from here
    pub event_object_index: u16, // event objects in this scene begins from number wEventObjectIndex + 1
}

#[derive(Debug)]
#[repr(C)]
pub struct ObjectPlayer {
    pub reserved: [u16; 2], // always zero
    pub script_on_friend_death: u16, // when friends in party dies, execute script from here
    pub script_on_dying: u16, // when dying, execute script from here
}

#[derive(Debug)]
#[repr(C)]
pub struct ObjectItem {
    pub bitmap: u16, // bitmap number in BALL.MKF
    pub price: u16, // price
    pub script_on_use: u16, // script executed when using this item
    pub script_on_equip: u16, // script executed when equipping this item
    pub script_on_throw: u16, // script executed when throwing this item to enemy
    pub flags: u16, // flags
}

#[derive(Debug)]
#[repr(C)]
pub struct ObjectMagic {
    pub magic_number: u16, // magic number, according to DATA.MKF #3
    pub reserved1: u16, // always zero
    pub script_on_success: u16, // when magic succeed, execute script from here
    pub script_on_use: u16, // when use this magic, execute script from here
    pub reserved2: u16, // always zero
    pub flags: u16, // flags
}

#[derive(Debug)]
#[repr(C)]
pub struct ObjectEnemy {
    pub enemy_id: u16, // ID of the enemy, according to DATA.MKF #1.
    pub resistance_to_sorcery: u16, // resistance to sorcery and poison (0 min, 10 max)
    pub script_on_turn_start: u16, // script executed when turn starts
    pub script_on_battle_end: u16, // script executed when battle ends
    pub script_on_ready: u16, // script executed when the enemy is ready
}

#[derive(Debug)]
#[repr(C)]
pub struct ObjectPoison {
    pub poison_level: u16, // level of the poison
    pub color: u16, // color of avatars
    pub player_script: u16, // script executed when player has this poison(per round)
    pub reserved: u16, // always zero
    pub enemy_script: u16, // script executed when enemy has this poison (per round)
}

/*
typedef union tagOBJECT_DOS
{
    WORD              rgwData[6];
    OBJECT_PLAYER     player;
    OBJECT_ITEM_DOS   item;
    OBJECT_MAGIC_DOS  magic;
    OBJECT_ENEMY      enemy;
    OBJECT_POISON     poison;
} OBJECT_DOS, *LPOBJECT_DOS;
*/
#[derive(Decode, Debug)]
#[repr(C)]
pub struct Object {
    data: [u16; 6],
}

#[derive(Decode, Debug)]
pub struct ScriptEntry {
    pub operation: u16, // operation code
    pub operands: [u16; 3], // operands
}

impl Object {
    pub fn cast<T>(&self) -> &T {
        unsafe { &*(self.data.as_ptr() as *const T) }
    }
}

/*
typedef enum tagOBJECTSTATE
{
   kObjStateHidden               = 0,
   kObjStateNormal               = 1,
   kObjStateBlocker              = 2
} OBJECTSTATE, *LPOBJECTSTATE;
*/
pub enum ObjectState {
    Hidden = 0,
    Normal,
    Blocker,
}

pub struct GameData {
    pub script_entries: Vec<ScriptEntry>,
}

impl GameData {
    pub fn load(sss: &mut MKF, data: &mut MKF) -> Result<GameData> {
        let buf = sss.read_chunk(3)?;
        let script_entries = decode_c_structs::<ScriptEntry>(&buf)?;

        Ok(GameData {
            script_entries,
        })
    }
}

pub struct GameState {
    pub objects: Vec<Object>,
    pub scenes: Vec<Scene>,
    pub event_objects: Vec<EventObject>,

    pub entering_scene: bool,
    pub scene_num: u16,
}

impl GameState {
    pub fn load_new_game(sss: &mut MKF) -> Result<Self> {
        let buf = sss.read_chunk(0)?;
        let events = decode_c_structs::<EventObject>(&buf)?;

        let buf = sss.read_chunk(1)?;
        let scenes = decode_c_structs::<Scene>(&buf)?;

        let buf = sss.read_chunk(2)?;
        let objects = decode_c_structs::<Object>(&buf)?;

        Ok(Self {
            objects,
            scenes,
            event_objects: events,
            entering_scene: true,
            scene_num: 1,
        })
    }
}
