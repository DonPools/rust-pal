use std::path::PathBuf;

use pal_assets::objects::GlobalObjects;
use pal_assets::player_roles::PlayerRoles;
use pal_assets::rle::RleBitmap;
use pal_assets::scene::SceneData;
use pal_assets::script::ScriptTable;
use pal_assets::text::{BitmapFont, TextLibrary};
use pal_core::game::GameState;
use pal_core::map::tile_to_world;
use pal_core::party::Party;
use pal_core::role::{Direction, Role, RoleSprites};
use pal_desktop::renderer::Renderer;

use crate::assets::{
    create_global_scene_objects, create_scene_objects, load_dialog_faces, load_global_objects,
    load_magics, load_map, load_music, load_palette, load_player_roles, load_role_sprites,
    load_scene_data, load_script_table, load_sound_effects, load_sound_font, load_stores,
    load_text_resources,
};
use crate::{DEFAULT_PALETTE, DEFAULT_SCENE, SCREEN_HEIGHT, SCREEN_WIDTH};

pub(super) struct BootstrappedGame {
    pub(super) data_dir: PathBuf,
    pub(super) scene_data: SceneData,
    pub(super) initial_scene_number: u16,
    pub(super) initial_map_number: u16,
    pub(super) initial_enter_script: u16,
    pub(super) initial_event_sprite_numbers: Vec<u16>,
    pub(super) text: TextLibrary,
    pub(super) font: BitmapFont,
    pub(super) script_table: ScriptTable,
    pub(super) voc_mkf: Vec<u8>,
    pub(super) midi_mkf: Vec<u8>,
    pub(super) sound_font: Vec<u8>,
    pub(super) player_roles: PlayerRoles,
    pub(super) global_objects: GlobalObjects,
    pub(super) role_sprites: RoleSprites,
    pub(super) dialog_faces: Vec<Option<RleBitmap>>,
    pub(super) game: GameState,
    pub(super) renderer: Renderer,
}

pub(super) fn bootstrap(data_dir: PathBuf) -> BootstrappedGame {
    let palette = load_palette(&data_dir, DEFAULT_PALETTE).expect("failed to load palette");
    let scene_data = load_scene_data(&data_dir).expect("failed to load scene data");
    let (text, font) = load_text_resources(&data_dir).expect("failed to load text resources");
    let script_table = load_script_table(&data_dir).expect("failed to load script table");
    let voc_mkf = load_sound_effects(&data_dir).expect("failed to load sound effects");
    let midi_mkf = load_music(&data_dir).expect("failed to load MIDI music");
    let sound_font = load_sound_font(&data_dir).expect("failed to load data/TimGM6mb.sf2");
    let player_roles = load_player_roles(&data_dir).expect("failed to load player role data");
    let magics = load_magics(&data_dir).expect("failed to load magic data");
    let global_objects =
        load_global_objects(&data_dir).expect("failed to load global object definitions");
    let stores = load_stores(&data_dir).expect("failed to load store definitions");
    let party = Party::single(0, &player_roles).expect("failed to create initial party");
    let scene = scene_data
        .scene(DEFAULT_SCENE)
        .expect("default scene is missing");
    let map = load_map(&data_dir, usize::from(scene.scene.map_num)).expect("failed to load map");
    let role_sprites = load_role_sprites(&data_dir).expect("failed to load role sprites");
    let dialog_faces = load_dialog_faces(&data_dir).expect("failed to load RGM.MKF dialog faces");
    let leader = party.leader().expect("initial party has no leader");
    let player = create_player(
        &role_sprites,
        usize::from(leader.attributes.scene_sprite_num),
        leader.attributes.frames_per_direction(),
        scene_entry_position(&script_table, scene.scene.script_on_enter)
            .expect("scene enter script has no initial party position"),
    )
    .expect("failed to create player at the scene entry position");
    let scene_objects = create_scene_objects(&scene, &role_sprites)
        .expect("failed to create current scene event objects");
    let global_scene_objects = create_global_scene_objects(&scene_data, &role_sprites)
        .expect("failed to create global event objects");
    let game = GameState::new(map, player, SCREEN_WIDTH, SCREEN_HEIGHT)
        .with_scene_number(scene.number as u16)
        .with_scene_objects(scene_objects)
        .with_global_objects(global_scene_objects)
        .with_party(party)
        .with_player_roles(player_roles.clone())
        .with_economy_data(stores, global_objects.clone())
        .with_magic_data(magics);
    let initial_scene_number = scene.number as u16;
    let initial_map_number = scene.scene.map_num;
    let initial_enter_script = scene.scene.script_on_enter;
    let initial_event_sprite_numbers = scene
        .event_objects
        .iter()
        .map(|event| event.sprite_num)
        .collect();

    BootstrappedGame {
        data_dir,
        scene_data,
        initial_scene_number,
        initial_map_number,
        initial_enter_script,
        initial_event_sprite_numbers,
        text,
        font,
        script_table,
        voc_mkf,
        midi_mkf,
        sound_font,
        player_roles,
        global_objects,
        role_sprites,
        dialog_faces,
        game,
        renderer: Renderer::new(palette, SCREEN_WIDTH as usize, SCREEN_HEIGHT as usize),
    }
}

fn create_player(
    sprites: &RoleSprites,
    sprite_index: usize,
    frames_per_direction: u8,
    world_position: (i32, i32),
) -> Option<Role> {
    if !sprites.has_directional_animation(sprite_index, frames_per_direction) {
        return None;
    }
    Some(Role {
        sprite_index,
        world_x: world_position.0,
        world_y: world_position.1,
        direction: Direction::South,
        anim_frame: 0,
        frames_per_direction,
    })
}

/// Find the first party-position opcode executed by a scene's entry script.
fn scene_entry_position(scripts: &ScriptTable, start: u16) -> Option<(i32, i32)> {
    let mut entry = start;
    for _ in 0..1024 {
        let script = scripts.entry(entry)?;
        match script.opcode {
            0x0046 => {
                return tile_to_world(
                    usize::from(script.operands[0]),
                    usize::from(script.operands[1]),
                    usize::from(script.operands[2]),
                );
            }
            0x0000..=0x0002 => return None,
            0x0003 => entry = script.operands[0],
            _ => entry = entry.checked_add(1)?,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scripts(entries: &[[u16; 4]]) -> ScriptTable {
        let data = entries
            .iter()
            .flat_map(|entry| entry.iter().flat_map(|value| value.to_le_bytes()))
            .collect::<Vec<_>>();
        ScriptTable::parse(&data).unwrap()
    }

    #[test]
    fn scene_entry_position_follows_jumps_to_party_position() {
        let scripts = scripts(&[
            [0, 0, 0, 0],
            [3, 3, 0, 0],
            [0, 0, 0, 0],
            [0x0046, 41, 18, 0],
        ]);
        assert_eq!(scene_entry_position(&scripts, 1), Some((1312, 288)));
    }

    #[test]
    fn scene_entry_position_rejects_missing_or_invalid_position() {
        assert_eq!(scene_entry_position(&scripts(&[[0, 0, 0, 0]]), 0), None);
        assert_eq!(
            scene_entry_position(&scripts(&[[0x0046, 128, 0, 0]]), 0),
            None
        );
    }
}
