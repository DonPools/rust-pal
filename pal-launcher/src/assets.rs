use std::path::Path;

use pal_assets::battle::{BattleData, BattleSpriteArchive};
use pal_assets::bitmap::Bitmap;
use pal_assets::magic::Magics;
use pal_assets::midi::MidiSong;
use pal_assets::mkf::MkfArchive;
use pal_assets::objects::{GlobalObjects, ObjectLayout};
use pal_assets::palette::Palette;
use pal_assets::player_roles::PlayerRoles;
use pal_assets::rle::RleBitmap;
use pal_assets::scene::SceneData;
use pal_assets::script::ScriptTable;
use pal_assets::sprite::{sprite_from_yj1_chunk, Sprite};
use pal_assets::store::Stores;
use pal_assets::text::{BitmapFont, TextLibrary};
use pal_assets::voc::VocClip;
use pal_assets::yj1;
use pal_core::map::Map;
use pal_core::role::RoleSprites;
use pal_core::scene::SceneObject;
use pal_desktop::window::LoadedScene;

pub(super) fn load_palette(data_dir: &Path, palette_num: usize) -> Option<Palette> {
    let data = std::fs::read(data_dir.join("PAT.MKF")).ok()?;
    let archive = MkfArchive::new(&data)?;
    Palette::from_bytes(archive.read_chunk(palette_num)?)
}

pub(super) fn load_map(data_dir: &Path, map_num: usize) -> Option<Map> {
    let map_mkf = std::fs::read(data_dir.join("MAP.MKF")).ok()?;
    let gop_mkf = std::fs::read(data_dir.join("GOP.MKF")).ok()?;
    Map::load(map_num, &map_mkf, &gop_mkf)
}

pub(super) fn load_scene_data(data_dir: &Path) -> Option<SceneData> {
    let data = std::fs::read(data_dir.join("SSS.MKF")).ok()?;
    SceneData::parse(&data)
}

pub(super) fn load_text_resources(data_dir: &Path) -> Option<(TextLibrary, BitmapFont)> {
    let sss_data = std::fs::read(data_dir.join("SSS.MKF")).ok()?;
    let sss = MkfArchive::new(&sss_data)?;
    let word_data = std::fs::read(data_dir.join("WORD.DAT")).ok()?;
    let message_data = std::fs::read(data_dir.join("M.MSG")).ok()?;
    let code_table = std::fs::read(data_dir.join("WOR16.ASC")).ok()?;
    let font_data = std::fs::read(data_dir.join("WOR16.FON")).ok()?;
    Some((
        TextLibrary::parse(&word_data, &message_data, sss.read_chunk(3)?)?,
        BitmapFont::parse(&code_table, &font_data)?,
    ))
}

pub(super) fn load_script_table(data_dir: &Path) -> Option<ScriptTable> {
    let data = std::fs::read(data_dir.join("SSS.MKF")).ok()?;
    let archive = MkfArchive::new(&data)?;
    ScriptTable::parse(archive.read_chunk(4)?)
}

pub(super) fn load_sound_effects(data_dir: &Path) -> Option<Vec<u8>> {
    std::fs::read(data_dir.join("VOC.MKF")).ok()
}

pub(super) fn load_music(data_dir: &Path) -> Option<Vec<u8>> {
    std::fs::read(data_dir.join("MIDI.MKF")).ok()
}

pub(super) fn load_sound_font(data_dir: &Path) -> Option<Vec<u8>> {
    std::fs::read(data_dir.join("TimGM6mb.sf2")).ok()
}

pub(super) fn load_role_sprites(data_dir: &Path) -> Option<RoleSprites> {
    let data = std::fs::read(data_dir.join("MGO.MKF")).ok()?;
    RoleSprites::load(&data)
}

pub(super) fn load_enemy_battle_sprites(data_dir: &Path) -> Option<BattleSpriteArchive> {
    BattleSpriteArchive::load(&std::fs::read(data_dir.join("ABC.MKF")).ok()?)
}

pub(super) fn load_player_battle_sprites(data_dir: &Path) -> Option<BattleSpriteArchive> {
    BattleSpriteArchive::load(&std::fs::read(data_dir.join("F.MKF")).ok()?)
}

pub(super) fn load_dialog_faces(data_dir: &Path) -> Option<Vec<Option<RleBitmap>>> {
    let data = std::fs::read(data_dir.join("RGM.MKF")).ok()?;
    let archive = MkfArchive::new(&data)?;
    Some(
        (0..archive.chunk_count())
            .map(|index| RleBitmap::decode(archive.read_chunk(index)?))
            .collect(),
    )
}

pub(super) fn load_ui_sprites(data_dir: &Path) -> Option<Vec<RleBitmap>> {
    let data = std::fs::read(data_dir.join("DATA.MKF")).ok()?;
    let archive = MkfArchive::new(&data)?;
    let chunk = archive.read_chunk(9)?;
    let sprite = Sprite::from_gop_chunk(chunk).or_else(|| sprite_from_yj1_chunk(chunk))?;
    let frames = sprite.decode_frames()?;
    (frames.len() > 70).then_some(frames)
}

pub(super) fn load_item_sprites(data_dir: &Path) -> Option<Vec<Option<RleBitmap>>> {
    let data = std::fs::read(data_dir.join("BALL.MKF")).ok()?;
    let archive = MkfArchive::new(&data)?;
    Some(
        (0..archive.chunk_count())
            .map(|index| {
                let chunk = archive.read_chunk(index)?;
                RleBitmap::decode(chunk).or_else(|| RleBitmap::decode(&yj1::decompress(chunk)?))
            })
            .collect(),
    )
}

pub(super) fn load_fbp_background(
    data_dir: &Path,
    palette: &Palette,
    index: usize,
) -> Option<Bitmap> {
    let data = std::fs::read(data_dir.join("FBP.MKF")).ok()?;
    let archive = MkfArchive::new(&data)?;
    let chunk = archive.read_chunk(index)?;
    let pixels = if chunk.len() == 320 * 200 {
        chunk.to_vec()
    } else {
        yj1::decompress(chunk)?
    };
    (pixels.len() == 320 * 200).then(|| Bitmap::from_indexed(pixels, 320, 200, palette))
}

pub(super) fn load_battle_backgrounds(
    data_dir: &Path,
    palette: &Palette,
) -> Option<Vec<Option<Bitmap>>> {
    let data = std::fs::read(data_dir.join("FBP.MKF")).ok()?;
    let archive = MkfArchive::new(&data)?;
    Some(
        (0..archive.chunk_count())
            .map(|index| {
                let chunk = archive.read_chunk(index)?;
                let pixels = if chunk.len() == 320 * 200 {
                    Some(chunk.to_vec())
                } else {
                    yj1::decompress(chunk)
                }?;
                (pixels.len() == 320 * 200).then(|| Bitmap::from_indexed(pixels, 320, 200, palette))
            })
            .collect(),
    )
}

pub(super) fn load_runtime_scene(
    data_dir: &Path,
    scene_data: &SceneData,
    number: u16,
    sprites: &RoleSprites,
) -> Option<LoadedScene> {
    let scene = scene_data.scene(usize::from(number))?;
    Some(LoadedScene {
        number,
        map: load_map(data_dir, usize::from(scene.scene.map_num))?,
        objects: create_scene_objects(&scene, sprites)?,
        enter_script: scene.scene.script_on_enter,
        teleport_script: scene.scene.script_on_teleport,
    })
}

pub(super) fn load_player_roles(data_dir: &Path) -> Option<PlayerRoles> {
    let data = std::fs::read(data_dir.join("DATA.MKF")).ok()?;
    let archive = MkfArchive::new(&data)?;
    PlayerRoles::parse(archive.read_chunk(3)?)
}

pub(super) fn load_battle_data(data_dir: &Path) -> Option<BattleData> {
    BattleData::parse(&std::fs::read(data_dir.join("DATA.MKF")).ok()?)
}

pub(super) fn load_magics(data_dir: &Path) -> Option<Magics> {
    let data = std::fs::read(data_dir.join("DATA.MKF")).ok()?;
    let archive = MkfArchive::new(&data)?;
    Magics::parse(archive.read_chunk(4)?)
}

pub(super) fn load_stores(data_dir: &Path) -> Option<Stores> {
    let data = std::fs::read(data_dir.join("DATA.MKF")).ok()?;
    let archive = MkfArchive::new(&data)?;
    Stores::parse(archive.read_chunk(0)?)
}

pub(super) fn load_global_objects(data_dir: &Path) -> Option<GlobalObjects> {
    let data = std::fs::read(data_dir.join("SSS.MKF")).ok()?;
    let archive = MkfArchive::new(&data)?;
    GlobalObjects::parse(archive.read_chunk(2)?, ObjectLayout::Dos)
}

pub(super) fn validate_music(data: &[u8]) -> Option<usize> {
    let archive = MkfArchive::new(data)?;
    let mut count = 0;
    for index in 0..archive.chunk_count() {
        let chunk = archive.read_chunk(index)?;
        if chunk.is_empty() {
            continue;
        }
        MidiSong::parse(chunk)?;
        count += 1;
    }
    Some(count)
}

pub(super) fn validate_sound_effects(data: &[u8]) -> Option<usize> {
    let archive = MkfArchive::new(data)?;
    let mut count = 0;
    for index in 0..archive.chunk_count() {
        let chunk = archive.read_chunk(index)?;
        if chunk.is_empty() {
            continue;
        }
        VocClip::parse(chunk)?;
        count += 1;
    }
    Some(count)
}

pub(super) fn create_scene_objects(
    scene: &pal_assets::scene::SceneView<'_>,
    sprites: &RoleSprites,
) -> Option<Vec<SceneObject>> {
    let first_id = scene.scene.event_object_index.checked_add(1)?;
    scene
        .event_objects
        .iter()
        .enumerate()
        .map(|(index, event)| {
            let id = first_id.checked_add(u16::try_from(index).ok()?)?;
            let frame_count = if event.sprite_num == 0 {
                0
            } else {
                sprites.character_frame_count(event.sprite_num as usize)?
            };
            SceneObject::from_asset(id, event, frame_count)
        })
        .collect()
}

pub(super) fn create_global_scene_objects(
    scene_data: &SceneData,
    sprites: &RoleSprites,
) -> Option<Vec<SceneObject>> {
    scene_data
        .event_objects()
        .iter()
        .enumerate()
        .map(|(index, event)| {
            let id = u16::try_from(index).ok()?.checked_add(1)?;
            let frame_count = if event.sprite_num == 0 {
                0
            } else {
                sprites.character_frame_count(event.sprite_num as usize)?
            };
            SceneObject::from_asset(id, event, frame_count)
        })
        .collect()
}
