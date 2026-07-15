//! Rust-PAL desktop launcher.

use std::path::{Path, PathBuf};

use pal_assets::mkf::MkfArchive;
use pal_assets::palette::Palette;
use pal_assets::player_roles::PlayerRoleGraphics;
use pal_core::game::GameState;
use pal_core::map::{tile_to_world, Map, MAP_COLUMNS, MAP_HALVES, MAP_ROWS};
use pal_core::role::{Direction, Role, RoleSprites};
use pal_desktop::renderer::Renderer;
use pal_desktop::window::{render_tile_map, run_game_window, Viewport};

const SCREEN_WIDTH: u32 = 320;
const SCREEN_HEIGHT: u32 = 200;
const DEFAULT_MAP: usize = 1;
// SDLPAL initializes gameplay with palette 0. Palette 1 is mostly grayscale.
const DEFAULT_PALETTE: usize = 0;

fn main() {
    let data_dir = workspace_data_dir();
    let check_only = std::env::args().any(|argument| argument == "--check-assets");

    println!("Rust-PAL M1 exploration");
    println!("data: {}", data_dir.display());

    let palette = load_palette(&data_dir, DEFAULT_PALETTE).expect("failed to load palette");
    let map = load_map(&data_dir, DEFAULT_MAP).expect("failed to load map");
    let role_sprites = load_role_sprites(&data_dir).expect("failed to load role sprites");
    let (role_sprite_index, walk_frames) =
        load_default_role_settings(&data_dir).expect("failed to load player role settings");
    let player = create_player(&map, &role_sprites, role_sprite_index, walk_frames)
        .expect("failed to create player on a walkable tile");
    let game = GameState::new(map, player, SCREEN_WIDTH, SCREEN_HEIGHT);
    let viewport = Viewport::from(game.camera);
    println!(
        "map {} loaded: {} tile frames, palette {}, viewport ({}, {})",
        game.map.map_num,
        game.map.tile_sprite.frame_count(),
        DEFAULT_PALETTE,
        viewport.x,
        viewport.y,
    );

    let mut renderer = Renderer::new(palette, SCREEN_WIDTH as usize, SCREEN_HEIGHT as usize);
    if check_only {
        render_tile_map(&mut renderer, &game.map, Some(&role_sprites), &[], viewport);
        let map_only = renderer.screen().to_vec();
        render_tile_map(
            &mut renderer,
            &game.map,
            Some(&role_sprites),
            std::slice::from_ref(&game.player),
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

        assert!(visible_pixels > 0, "rendered map is blank");
        assert!(
            chromatic_pixels > 0,
            "rendered map contains no chromatic pixels; check palette selection"
        );
        assert!(
            sprite_pixels > 0,
            "scene sprite did not change the framebuffer"
        );
        println!(
            "scene sprite rendered: {sprite_pixels} pixels, {} slots loaded",
            role_sprites.character_count()
        );
        println!(
            "asset check passed: {visible_pixels} visible pixels, \
             {chromatic_pixels} chromatic pixels"
        );
        return;
    }

    run_game_window(renderer, game, role_sprites);
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

fn load_role_sprites(data_dir: &Path) -> Option<RoleSprites> {
    let data = std::fs::read(data_dir.join("MGO.MKF")).ok()?;
    RoleSprites::load(&data)
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
    map: &Map,
    sprites: &RoleSprites,
    sprite_index: usize,
    frames_per_direction: u8,
) -> Option<Role> {
    if !sprites.has_directional_animation(sprite_index, frames_per_direction) {
        return None;
    }
    let (map_x, map_y, map_h) = display_tile(map)?;
    let (world_x, world_y) = tile_to_world(map_x, map_y, map_h)?;
    Some(Role {
        sprite_index,
        world_x,
        world_y,
        direction: Direction::South,
        anim_frame: 0,
        frames_per_direction,
    })
}

fn display_tile(map: &Map) -> Option<(usize, usize, usize)> {
    // Keep enough map above the role for its bitmap to be fully visible.
    for y in 8..MAP_ROWS {
        for x in 0..MAP_COLUMNS {
            for h in 0..MAP_HALVES {
                if map.get_bottom_tile_index(x, y, h)? != 0 && !map.is_tile_blocked(x, y, h) {
                    return Some((x, y, h));
                }
            }
        }
    }
    map.first_occupied_tile()
}
