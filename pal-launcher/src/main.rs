//! Rust-PAL desktop launcher.

use std::path::{Path, PathBuf};

use pal_assets::mkf::MkfArchive;
use pal_assets::palette::Palette;
use pal_assets::player_roles::PlayerRoleGraphics;
use pal_assets::scene::SceneData;
use pal_assets::script::ScriptTable;
use pal_assets::text::{BitmapFont, TextLibrary};
use pal_core::game::GameState;
use pal_core::map::{tile_to_world, Map};
use pal_core::role::{Direction, Role, RoleSprites};
use pal_core::scene::SceneObject;
use pal_core::script::{ScriptEvent, ScriptRuntime};
use pal_desktop::renderer::Renderer;
use pal_desktop::window::{render_tile_map, run_game_window, GameResources, LoadedScene, Viewport};

const SCREEN_WIDTH: u32 = 320;
const SCREEN_HEIGHT: u32 = 200;
const DEFAULT_SCENE: usize = 1;
// SDLPAL initializes gameplay with palette 0. Palette 1 is mostly grayscale.
const DEFAULT_PALETTE: usize = 0;

fn main() {
    let data_dir = workspace_data_dir();
    let check_only = std::env::args().any(|argument| argument == "--check-assets");

    println!("Rust-PAL M2 interaction groundwork");
    println!("data: {}", data_dir.display());

    let palette = load_palette(&data_dir, DEFAULT_PALETTE).expect("failed to load palette");
    let scene_data = load_scene_data(&data_dir).expect("failed to load scene data");
    let (text, font) = load_text_resources(&data_dir).expect("failed to load text resources");
    let script_table = load_script_table(&data_dir).expect("failed to load script table");
    let scene = scene_data
        .scene(DEFAULT_SCENE)
        .expect("default scene is missing");
    let map = load_map(&data_dir, scene.scene.map_num as usize).expect("failed to load map");
    let role_sprites = load_role_sprites(&data_dir).expect("failed to load role sprites");
    let (role_sprite_index, walk_frames) =
        load_default_role_settings(&data_dir).expect("failed to load player role settings");
    let player = create_player(
        &role_sprites,
        role_sprite_index,
        walk_frames,
        scene_entry_position(&script_table, scene.scene.script_on_enter)
            .expect("scene enter script has no initial party position"),
    )
    .expect("failed to create player at the scene entry position");
    let scene_objects = create_scene_objects(&scene, &role_sprites)
        .expect("failed to create current scene event objects");
    let mut game = GameState::new(map, player, SCREEN_WIDTH, SCREEN_HEIGHT)
        .with_scene_number(scene.number as u16)
        .with_scene_objects(scene_objects);
    let viewport = Viewport::from(game.camera);
    println!(
        "scene {} loaded: map {}, {} event objects, {} tile frames, palette {}, viewport ({}, {})",
        scene.number,
        game.map.map_num,
        scene.event_objects.len(),
        game.map.tile_sprite.frame_count(),
        DEFAULT_PALETTE,
        viewport.x,
        viewport.y,
    );

    let mut renderer = Renderer::new(palette, SCREEN_WIDTH as usize, SCREEN_HEIGHT as usize);
    if check_only {
        assert!(
            scene.event_objects.iter().all(|event| event.sprite_num == 0
                || role_sprites
                    .character_frame_count(event.sprite_num as usize)
                    .is_some()),
            "scene event object references an unavailable MGO.MKF sprite"
        );
        render_tile_map(
            &mut renderer,
            &game.map,
            Some(&role_sprites),
            &[],
            &[],
            viewport,
        );
        let map_only = renderer.screen().to_vec();
        render_tile_map(
            &mut renderer,
            &game.map,
            Some(&role_sprites),
            std::slice::from_ref(&game.player),
            &game.scene_objects,
            viewport,
        );
        let sprite_pixels = renderer
            .screen()
            .chunks_exact(4)
            .zip(map_only.chunks_exact(4))
            .filter(|(with_sprite, map_pixel)| with_sprite != map_pixel)
            .count();
        let visible_pixels = renderer
            .screen()
            .chunks_exact(4)
            .filter(|pixel| pixel[..3] != [0, 0, 0])
            .count();
        let chromatic_pixels = renderer
            .screen()
            .chunks_exact(4)
            .filter(|pixel| {
                let [r, g, b, _] = pixel else {
                    return false;
                };
                r.abs_diff(*g).max(g.abs_diff(*b)).max(b.abs_diff(*r)) >= 8
            })
            .count();

        let visible_object = game
            .scene_objects
            .iter()
            .find(|object| object.is_visible())
            .expect("current scene has no visible event object");
        let visible_object_id = visible_object.id;
        let object_viewport = Viewport::new(
            (visible_object.world_x - SCREEN_WIDTH as i32 / 2).max(0),
            (visible_object.world_y - SCREEN_HEIGHT as i32 / 2).max(0),
            SCREEN_WIDTH,
            SCREEN_HEIGHT,
        );
        render_tile_map(
            &mut renderer,
            &game.map,
            Some(&role_sprites),
            &[],
            &[],
            object_viewport,
        );
        let object_map_only = renderer.screen().to_vec();
        render_tile_map(
            &mut renderer,
            &game.map,
            Some(&role_sprites),
            &[],
            std::slice::from_ref(visible_object),
            object_viewport,
        );
        let event_object_pixels = renderer
            .screen()
            .chunks_exact(4)
            .zip(object_map_only.chunks_exact(4))
            .filter(|(with_object, map_pixel)| with_object != map_pixel)
            .count();
        let text_before = renderer.screen().to_vec();
        let sample_word = (0..text.word_count())
            .filter_map(|index| text.word(index))
            .find(|word| !word.is_empty())
            .expect("WORD.DAT has no displayable sample");
        renderer.draw_big5_text(&font, sample_word, 8, 8, 0x4f);
        let text_pixels = renderer
            .screen()
            .chunks_exact(4)
            .zip(text_before.chunks_exact(4))
            .filter(|(with_text, before)| with_text != before)
            .count();
        let script_object = game
            .scene_objects
            .iter()
            .find(|object| {
                script_table
                    .entry(object.trigger_script)
                    .is_some_and(|entry| entry.opcode == 0xffff)
            })
            .expect("scene has no directly displayable message script");
        let trigger = pal_core::scene::TriggerRequest {
            object_id: script_object.id,
            script_entry: script_object.trigger_script,
            kind: pal_core::scene::TriggerKind::Search,
        };
        let script_count = script_table.len();
        let mut scripts = ScriptRuntime::new(script_table);
        assert!(scripts.start(trigger));
        let mut script_messages = 0;
        loop {
            match scripts
                .advance()
                .expect("script runtime stopped without an event")
            {
                ScriptEvent::Message { message_id, .. } => {
                    assert!(
                        text.message(usize::from(message_id)).is_some(),
                        "script references an unavailable message"
                    );
                    script_messages += 1;
                }
                ScriptEvent::Completed { .. } => break,
                event => panic!("message script did not complete: {event:?}"),
            }
        }
        assert!(script_messages > 0, "message script yielded no messages");
        let movement_trigger = pal_core::scene::TriggerRequest {
            object_id: 2,
            script_entry: 69,
            kind: pal_core::scene::TriggerKind::Touch,
        };
        assert!(scripts.start(movement_trigger));
        let mut script_ticks = 0;
        loop {
            match scripts
                .advance()
                .expect("movement script stopped without an event")
            {
                ScriptEvent::Action(_) | ScriptEvent::Waiting => script_ticks += 1,
                ScriptEvent::Completed { .. } => break,
                event => panic!("movement script did not complete: {event:?}"),
            }
        }
        assert!(script_ticks > 0, "movement script yielded no timed work");

        let intro_trigger = pal_core::scene::TriggerRequest {
            object_id: 0xffff,
            script_entry: scene.scene.script_on_enter,
            kind: pal_core::scene::TriggerKind::Touch,
        };
        assert!(scripts.start(intro_trigger));
        let mut intro_messages = 0;
        let mut intro_actions = 0;
        loop {
            match scripts
                .advance()
                .expect("scene enter script stopped without an event")
            {
                ScriptEvent::Message { message_id, .. } => {
                    assert!(text.message(usize::from(message_id)).is_some());
                    intro_messages += 1;
                }
                ScriptEvent::Action(action) => {
                    assert!(
                        game.apply_script_action(action),
                        "scene enter action could not be applied: {action:?}"
                    );
                    intro_actions += 1;
                }
                ScriptEvent::Waiting => {}
                ScriptEvent::Completed { .. } => break,
                event => panic!("scene enter script did not complete: {event:?}"),
            }
        }
        assert_eq!(intro_messages, 67);
        assert!(intro_actions > 0);

        let item_trigger = pal_core::scene::TriggerRequest {
            object_id: 5,
            script_entry: 6318,
            kind: pal_core::scene::TriggerKind::Search,
        };
        assert!(scripts.start(item_trigger));
        loop {
            match scripts
                .advance()
                .expect("item script stopped without an event")
            {
                ScriptEvent::Message { message_id, .. } => {
                    assert!(text.message(usize::from(message_id)).is_some());
                }
                ScriptEvent::Action(action) => assert!(game.apply_script_action(action)),
                ScriptEvent::Waiting => {}
                ScriptEvent::Completed { .. } => break,
                event => panic!("item script did not complete: {event:?}"),
            }
        }
        assert_eq!(game.item_count(99), 1);

        let exit_trigger = pal_core::scene::TriggerRequest {
            object_id: 1,
            script_entry: 4667,
            kind: pal_core::scene::TriggerKind::Touch,
        };
        assert!(scripts.start(exit_trigger));
        let mut switched_scene = None;
        loop {
            match scripts
                .advance()
                .expect("exit script stopped without an event")
            {
                ScriptEvent::Action(pal_core::script::ScriptAction::ChangeScene {
                    scene_number,
                }) => {
                    let loaded =
                        load_runtime_scene(&data_dir, &scene_data, scene_number, &role_sprites)
                            .expect("exit script target scene could not be loaded");
                    game.replace_scene(loaded.number, loaded.map, loaded.objects);
                    switched_scene = Some(scene_number);
                }
                ScriptEvent::Action(action) => assert!(game.apply_script_action(action)),
                ScriptEvent::Waiting => {}
                ScriptEvent::Completed { .. } => break,
                event => panic!("exit script did not complete: {event:?}"),
            }
        }
        assert_eq!(switched_scene, Some(3));
        assert_eq!(game.scene_number, 3);

        assert!(visible_pixels > 0, "rendered map is blank");
        assert!(
            chromatic_pixels > 0,
            "rendered map contains no chromatic pixels; check palette selection"
        );
        assert!(
            sprite_pixels > 0,
            "scene sprite did not change the framebuffer"
        );
        assert!(
            event_object_pixels > 0,
            "event object did not change the framebuffer"
        );
        assert!(
            text_pixels > 0,
            "bitmap text did not change the framebuffer"
        );
        println!(
            "scene sprite rendered: {sprite_pixels} pixels, {} slots loaded",
            role_sprites.character_count()
        );
        println!(
            "scene data passed: {} scenes, {} event objects total, {} in scene {}",
            scene_data.scene_count(),
            scene_data.event_object_count(),
            scene.event_objects.len(),
            scene.number,
        );
        println!(
            "event object {} rendered: {event_object_pixels} pixels",
            visible_object_id
        );
        println!(
            "text data passed: {} words, {} messages, {} glyphs, {text_pixels} sample pixels",
            text.word_count(),
            text.message_count(),
            font.glyph_count(),
        );
        println!(
            "script data passed: {} records, {script_messages} messages, {script_ticks} timed actions",
            script_count,
        );
        println!(
            "M2 flow passed: {intro_messages} intro messages, {intro_actions} intro actions, item 99 acquired, scene 3 loaded"
        );
        println!(
            "asset check passed: {visible_pixels} visible pixels, \
             {chromatic_pixels} chromatic pixels"
        );
        return;
    }

    let scene_data_dir = data_dir.clone();
    run_game_window(
        renderer,
        game,
        GameResources {
            role_sprites,
            script_table,
            initial_enter_script: scene.scene.script_on_enter,
            text,
            font,
        },
        move |number, sprites| load_runtime_scene(&scene_data_dir, &scene_data, number, sprites),
    );
}

fn workspace_data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("launcher must be inside the workspace")
        .join("data")
}

fn load_palette(data_dir: &Path, palette_num: usize) -> Option<Palette> {
    let data = std::fs::read(data_dir.join("PAT.MKF")).ok()?;
    let archive = MkfArchive::new(&data)?;
    Palette::from_bytes(archive.read_chunk(palette_num)?)
}

fn load_map(data_dir: &Path, map_num: usize) -> Option<Map> {
    let map_mkf = std::fs::read(data_dir.join("MAP.MKF")).ok()?;
    let gop_mkf = std::fs::read(data_dir.join("GOP.MKF")).ok()?;
    Map::load(map_num, &map_mkf, &gop_mkf)
}

fn load_scene_data(data_dir: &Path) -> Option<SceneData> {
    let data = std::fs::read(data_dir.join("SSS.MKF")).ok()?;
    SceneData::parse(&data)
}

fn load_text_resources(data_dir: &Path) -> Option<(TextLibrary, BitmapFont)> {
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

fn load_script_table(data_dir: &Path) -> Option<ScriptTable> {
    let data = std::fs::read(data_dir.join("SSS.MKF")).ok()?;
    let archive = MkfArchive::new(&data)?;
    ScriptTable::parse(archive.read_chunk(4)?)
}

fn load_role_sprites(data_dir: &Path) -> Option<RoleSprites> {
    let data = std::fs::read(data_dir.join("MGO.MKF")).ok()?;
    RoleSprites::load(&data)
}

fn create_scene_objects(
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

fn load_runtime_scene(
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

fn load_default_role_settings(data_dir: &Path) -> Option<(usize, u8)> {
    let data = std::fs::read(data_dir.join("DATA.MKF")).ok()?;
    let archive = MkfArchive::new(&data)?;
    let graphics = PlayerRoleGraphics::parse(archive.read_chunk(3)?, 0)?;
    Some((
        graphics.sprite_num as usize,
        if graphics.walk_frames == 4 { 4 } else { 3 },
    ))
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
