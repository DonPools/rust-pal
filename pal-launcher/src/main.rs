//! Rust-PAL desktop launcher.

mod asset_check;
mod assets;
mod bootstrap;
mod command;

use std::path::{Path, PathBuf};

use pal_desktop::window::{run_game_window, GameResources, Viewport};

use asset_check::check_assets;
use assets::load_runtime_scene_with_map;
use bootstrap::{bootstrap, BootstrappedGame};
use command::Command;

const SCREEN_WIDTH: u32 = 320;
const SCREEN_HEIGHT: u32 = 200;
const DEFAULT_SCENE: usize = 1;
// SDLPAL initializes gameplay with palette 0. Palette 1 is mostly grayscale.
const DEFAULT_PALETTE: usize = 0;

fn main() {
    let data_dir = workspace_data_dir();
    let command = Command::parse(std::env::args().skip(1)).unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(2);
    });

    println!("Rust-PAL M6");
    println!("data: {}", data_dir.display());

    let boot = bootstrap(data_dir);
    print_bootstrap_summary(&boot);
    match command {
        Command::Run => run_desktop(boot),
        Command::CheckAssets => check_assets(boot),
    }
}

fn print_bootstrap_summary(boot: &BootstrappedGame) {
    let viewport = Viewport::from(boot.game.camera);
    println!(
        "scene {} loaded: map {}, {} event objects, {} tile frames, palette {}, viewport ({}, {})",
        boot.initial_scene_number,
        boot.initial_map_number,
        boot.initial_event_sprite_numbers.len(),
        boot.game.map.tile_sprite.frame_count(),
        DEFAULT_PALETTE,
        viewport.x,
        viewport.y,
    );
}

fn run_desktop(boot: BootstrappedGame) {
    let BootstrappedGame {
        data_dir,
        scene_data,
        initial_enter_script,
        opening_background,
        text,
        font,
        script_table,
        voc_mkf,
        mus_mkf,
        midi_mkf,
        sound_font,
        palettes,
        fbp_archive,
        rng_archive,
        role_sprites,
        dialog_faces,
        dialog_icons,
        ui_sprites,
        item_sprites,
        status_background,
        equip_background,
        enemy_battle_sprites,
        player_battle_sprites,
        magic_effect_sprites,
        battle_effects,
        battle_backgrounds,
        game,
        renderer,
        ..
    } = boot;
    let scene_data_dir = data_dir.clone();

    run_game_window(
        renderer,
        game,
        GameResources {
            role_sprites,
            script_table,
            initial_enter_script,
            opening_background,
            text,
            font,
            dialog_faces,
            dialog_icons,
            ui_sprites,
            item_sprites,
            status_background,
            equip_background,
            enemy_battle_sprites,
            player_battle_sprites,
            magic_effect_sprites,
            battle_effects,
            battle_backgrounds,
            voc_mkf,
            mus_mkf,
            midi_mkf,
            sound_font,
            palettes,
            fbp_archive,
            rng_archive,
            original_save_dir: data_dir.clone(),
            snapshot_path: workspace_snapshot_path(),
        },
        move |number, map_override, sprites| {
            load_runtime_scene_with_map(&scene_data_dir, &scene_data, number, map_override, sprites)
        },
    );
}

fn workspace_data_dir() -> PathBuf {
    workspace_root().join("data")
}

fn workspace_snapshot_path() -> PathBuf {
    workspace_root().join("rust-pal.snapshot.json")
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("launcher must be inside the workspace")
        .to_owned()
}
