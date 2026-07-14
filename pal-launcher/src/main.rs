//! Rust-PAL desktop launcher.

use std::path::{Path, PathBuf};

use pal_assets::mkf::MkfArchive;
use pal_assets::palette::Palette;
use pal_core::map::Map;
use pal_desktop::renderer::Renderer;
use pal_desktop::window::{render_tile_map, run_game_window};

const SCREEN_WIDTH: u32 = 320;
const SCREEN_HEIGHT: u32 = 200;
const DEFAULT_MAP: usize = 1;
// SDLPAL initializes gameplay with palette 0. Palette 1 is mostly grayscale.
const DEFAULT_PALETTE: usize = 0;

fn main() {
    let data_dir = workspace_data_dir();
    let check_only = std::env::args().any(|argument| argument == "--check-assets");

    println!("Rust-PAL Phase 2");
    println!("data: {}", data_dir.display());

    let palette = load_palette(&data_dir, DEFAULT_PALETTE).expect("failed to load palette");
    let map = load_map(&data_dir, DEFAULT_MAP).expect("failed to load map");
    let (scroll_x, scroll_y) = initial_viewport(&map);
    println!(
        "map {} loaded: {} tile frames, palette {}, viewport ({scroll_x}, {scroll_y})",
        map.map_num,
        map.tile_sprite.frame_count(),
        DEFAULT_PALETTE
    );

    let mut renderer = Renderer::new(palette, SCREEN_WIDTH as usize, SCREEN_HEIGHT as usize);
    if check_only {
        render_tile_map(
            &mut renderer,
            &map,
            scroll_x,
            scroll_y,
            SCREEN_WIDTH,
            SCREEN_HEIGHT,
        );
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
        println!(
            "asset check passed: {visible_pixels} visible pixels, \
             {chromatic_pixels} chromatic pixels"
        );
        return;
    }

    run_game_window(
        renderer,
        map,
        SCREEN_WIDTH,
        SCREEN_HEIGHT,
        scroll_x,
        scroll_y,
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

fn initial_viewport(map: &Map) -> (i32, i32) {
    let Some((x, y, h)) = map.first_occupied_tile() else {
        return (0, 0);
    };
    let center_x = x as i32 * 32 + h as i32 * 16;
    let center_y = y as i32 * 16 + h as i32 * 8;
    (
        (center_x - SCREEN_WIDTH as i32 / 2).max(0),
        (center_y - SCREEN_HEIGHT as i32 / 2).max(0),
    )
}
