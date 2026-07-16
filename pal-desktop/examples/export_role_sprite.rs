use std::path::PathBuf;

use image::{Rgba, RgbaImage};
use pal_assets::bitmap::Bitmap;
use pal_assets::mkf::MkfArchive;
use pal_assets::palette::Palette;
use pal_assets::player_roles::PlayerRoleGraphics;
use pal_core::role::{Direction, Role, RoleSprites};

const SCALE: u32 = 6;

fn main() {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("pal-desktop must be in the workspace")
        .to_path_buf();
    let data_dir = workspace.join("data");
    let output_dir = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace.join("target/role-export"));
    std::fs::create_dir_all(&output_dir).expect("failed to create output directory");

    let palette_data = std::fs::read(data_dir.join("PAT.MKF")).expect("failed to read PAT.MKF");
    let palette_archive = MkfArchive::new(&palette_data).expect("invalid PAT.MKF");
    let palette = Palette::from_bytes(palette_archive.read_chunk(0).expect("palette 0 is missing"))
        .expect("invalid palette 0");

    let data = std::fs::read(data_dir.join("DATA.MKF")).expect("failed to read DATA.MKF");
    let data_archive = MkfArchive::new(&data).expect("invalid DATA.MKF");
    let graphics = PlayerRoleGraphics::parse(
        data_archive.read_chunk(3).expect("PLAYERROLES is missing"),
        0,
    )
    .expect("invalid PLAYERROLES");
    let frames_per_direction = if graphics.walk_frames == 4 { 4 } else { 3 };

    let mgo = std::fs::read(data_dir.join("MGO.MKF")).expect("failed to read MGO.MKF");
    let sprites = RoleSprites::load(&mgo).expect("invalid MGO.MKF");
    let mut frames = Vec::new();
    for direction in Direction::ALL {
        for anim_frame in 0..frames_per_direction {
            let role = Role {
                sprite_index: graphics.sprite_num as usize,
                world_x: 0,
                world_y: 0,
                direction,
                anim_frame,
                frames_per_direction,
            };
            let rle = sprites
                .decode_role_frame(&role)
                .expect("failed to decode role frame");
            let bitmap = Bitmap::from_decoded_rle(&rle, &palette);
            frames.push((direction, anim_frame, bitmap));
        }
    }

    let current = &frames[0].2;
    save_scaled(current, &output_dir.join("player-south-frame-0.png"), SCALE);

    let max_width = frames
        .iter()
        .map(|(_, _, frame)| frame.width)
        .max()
        .unwrap() as u32;
    let max_height = frames
        .iter()
        .map(|(_, _, frame)| frame.height)
        .max()
        .unwrap() as u32;
    let cell_width = max_width + 8;
    let cell_height = max_height + 8;
    let mut sheet = RgbaImage::from_pixel(
        cell_width * frames_per_direction as u32,
        cell_height * 4,
        Rgba([255, 0, 255, 255]),
    );
    for (direction, anim_frame, frame) in &frames {
        let column = u32::from(*anim_frame);
        let row = *direction as u32;
        let x = column * cell_width + (cell_width - frame.width as u32) / 2;
        let y = row * cell_height + (cell_height - frame.height as u32) / 2;
        image::imageops::overlay(
            &mut sheet,
            &RgbaImage::from_raw(frame.width as u32, frame.height as u32, frame.rgba.clone())
                .unwrap(),
            i64::from(x),
            i64::from(y),
        );
    }
    image::imageops::resize(
        &sheet,
        sheet.width() * 4,
        sheet.height() * 4,
        image::imageops::FilterType::Nearest,
    )
    .save(output_dir.join("player-all-walk-frames.png"))
    .expect("failed to save frame sheet");

    println!(
        "sprite={} frames_per_direction={} current={}x{} output={}",
        graphics.sprite_num,
        frames_per_direction,
        current.width,
        current.height,
        output_dir.display()
    );
}

fn save_scaled(bitmap: &Bitmap, path: &std::path::Path, scale: u32) {
    let image = RgbaImage::from_raw(
        bitmap.width as u32,
        bitmap.height as u32,
        bitmap.rgba.clone(),
    )
    .expect("invalid RGBA bitmap");
    image::imageops::resize(
        &image,
        image.width() * scale,
        image.height() * scale,
        image::imageops::FilterType::Nearest,
    )
    .save(path)
    .expect("failed to save role frame");
}
