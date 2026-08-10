//! Rust-PAL desktop launcher.

mod asset_check;
mod assets;
mod bootstrap;
mod command;

use std::path::{Path, PathBuf};

use pal_desktop::window::{
    run_game_window, run_scene_editor_window, GameResources, SceneEditorResources, Viewport,
};

use asset_check::check_assets;
use assets::load_runtime_scene_with_map;
use bootstrap::{bootstrap, bootstrap_scene_editor, BootstrappedGame, BootstrappedSceneEditor};
use command::{Command, CommandMode};

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

    match command.mode {
        CommandMode::Run => {
            let boot = bootstrap(data_dir, command.sound_font_path.as_deref());
            print_bootstrap_summary(&boot);
            run_desktop(boot, command.music_backend);
        }
        CommandMode::CheckAssets => {
            let boot = bootstrap(data_dir, command.sound_font_path.as_deref());
            print_bootstrap_summary(&boot);
            check_assets(boot);
        }
        CommandMode::SceneEditor { scene } => {
            if let Err(error) = run_scene_editor(bootstrap_scene_editor(data_dir), scene) {
                eprintln!("{error}");
                std::process::exit(2);
            }
        }
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

fn run_desktop(boot: BootstrappedGame, music_backend: pal_desktop::audio::MusicBackend) {
    let BootstrappedGame {
        data_dir,
        scene_data,
        initial_enter_script,
        opening_background,
        text,
        font,
        item_descriptions,
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
            item_descriptions,
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
            music_backend,
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

fn run_scene_editor(
    boot: BootstrappedSceneEditor,
    initial_scene_number: u16,
) -> Result<(), String> {
    let BootstrappedSceneEditor {
        data_dir,
        scene_data,
        script_table,
        text,
        font,
        item_descriptions,
        player_roles,
        global_objects,
        battle_data,
        magics,
        stores,
        item_sprites,
        enemy_battle_sprites,
        player_battle_sprites,
        magic_effect_sprites,
        script_references,
        role_sprites,
        renderer,
    } = boot;
    let scene_count = u16::try_from(scene_data.scene_count()).expect("too many scenes");
    validate_scene_number(initial_scene_number, scene_count)?;
    let initial_scene = load_runtime_scene_with_map(
        &data_dir,
        &scene_data,
        initial_scene_number,
        None,
        &role_sprites,
    )
    .ok_or_else(|| format!("failed to load scene {initial_scene_number}"))?;
    let scene_data_dir = data_dir.clone();

    run_scene_editor_window(
        renderer,
        initial_scene,
        SceneEditorResources {
            role_sprites,
            script_table,
            text,
            font,
            item_descriptions,
            player_roles,
            global_objects,
            battle_data,
            magics,
            stores,
            item_sprites,
            enemy_battle_sprites,
            player_battle_sprites,
            magic_effect_sprites,
            script_references,
            scene_count,
        },
        move |number, sprites| {
            load_runtime_scene_with_map(&scene_data_dir, &scene_data, number, None, sprites)
        },
    );
    Ok(())
}

fn validate_scene_number(scene: u16, scene_count: u16) -> Result<(), String> {
    if (1..=scene_count).contains(&scene) {
        Ok(())
    } else {
        Err(format!("scene {scene} is outside 1..={scene_count}"))
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_scene_selection_requires_a_valid_one_based_scene() {
        assert_eq!(validate_scene_number(1, 293), Ok(()));
        assert_eq!(validate_scene_number(293, 293), Ok(()));
        assert_eq!(
            validate_scene_number(294, 293),
            Err("scene 294 is outside 1..=293".to_owned())
        );
        assert!(validate_scene_number(0, 293).is_err());
    }
}
