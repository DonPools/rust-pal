use std::path::PathBuf;

use pal_assets::battle::{BattleEffects, BattleSpriteArchive};
use pal_assets::bitmap::Bitmap;
use pal_assets::fbp::FbpArchive;
use pal_assets::palette::PaletteSet;
use pal_assets::rle::RleBitmap;
use pal_assets::rng::RngArchive;
use pal_assets::script::ScriptTable;
use pal_assets::text::{BitmapFont, TextLibrary};
use pal_core::game::Camera;
use pal_core::map::Map;
use pal_core::role::RoleSprites;
use pal_core::scene::SceneObject;

#[derive(Debug, Clone, Copy)]
pub struct Viewport {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Viewport {
    pub fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

impl From<Camera> for Viewport {
    fn from(camera: Camera) -> Self {
        Self::new(camera.x, camera.y, camera.width, camera.height)
    }
}

pub struct LoadedScene {
    pub number: u16,
    pub map: Map,
    pub objects: Vec<SceneObject>,
    pub enter_script: u16,
    pub teleport_script: u16,
}

pub struct GameResources {
    pub role_sprites: RoleSprites,
    pub script_table: ScriptTable,
    pub initial_enter_script: u16,
    pub opening_background: Bitmap,
    pub text: TextLibrary,
    pub font: BitmapFont,
    pub dialog_faces: Vec<Option<RleBitmap>>,
    pub dialog_icons: Vec<RleBitmap>,
    pub ui_sprites: Vec<RleBitmap>,
    pub item_sprites: Vec<Option<RleBitmap>>,
    pub enemy_battle_sprites: BattleSpriteArchive,
    pub player_battle_sprites: BattleSpriteArchive,
    pub magic_effect_sprites: BattleSpriteArchive,
    pub battle_effects: BattleEffects,
    pub battle_backgrounds: Vec<Option<Bitmap>>,
    pub status_background: Bitmap,
    pub equip_background: Bitmap,
    pub voc_mkf: Vec<u8>,
    pub midi_mkf: Vec<u8>,
    pub sound_font: Vec<u8>,
    pub palettes: Vec<PaletteSet>,
    pub fbp_archive: FbpArchive,
    pub rng_archive: RngArchive,
    pub original_save_dir: PathBuf,
    pub snapshot_path: PathBuf,
}
