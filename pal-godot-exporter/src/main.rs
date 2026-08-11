use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use encoding_rs::BIG5;
use image::{Rgba, RgbaImage};
use nuked_opl3::Opl3Chip;
use pal_assets::mkf::MkfArchive;
use pal_assets::palette::Palette;
use pal_assets::rix::{RixSequencer, RixTrack};
use pal_assets::rle::RleBitmap;
use pal_assets::sprite::{sprite_from_yj1_chunk, Sprite};
use pal_assets::voc::VocClip;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const PROFILE: &str = "framework-lab";
const PALETTE_CHUNK: usize = 0;
const TILE_CHUNKS: [usize; 2] = [10, 12];
const CHARACTER_CHUNKS: [usize; 5] = [2, 21, 29, 30, 207];
const PORTRAIT_CHUNKS: [usize; 5] = [1, 3, 6, 55, 59];
const SOUND_CHUNKS: [usize; 2] = [78, 98];
const MUSIC_CHUNK: usize = 31;
const MUSIC_SAMPLE_RATE: u32 = 44_100;
const RIX_TICKS_PER_SECOND: usize = 70;
const MAX_RIX_TICKS: usize = 15 * 60 * RIX_TICKS_PER_SECOND;
const FONT_DATA_OFFSET: usize = 0x682;
const FONT_GLYPH_BYTES: usize = 30;
const FONT_CELL_WIDTH: u32 = 16;
const FONT_CELL_HEIGHT: u32 = 16;

#[derive(Debug)]
struct Command {
    data: PathBuf,
    output: PathBuf,
    json: bool,
}

#[derive(Serialize)]
struct Manifest {
    schema_version: u32,
    source_variant: &'static str,
    export_profile: &'static str,
    exporter_version: &'static str,
    source_hashes: BTreeMap<String, String>,
    assets: Vec<ManifestAsset>,
}

#[derive(Serialize)]
struct ManifestAsset {
    kind: String,
    source: Source,
    path: String,
    sha256: String,
    metadata: Value,
}

#[derive(Serialize)]
struct Source {
    file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    chunk: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    palette: Option<usize>,
}

#[derive(Debug, Serialize)]
struct AtlasFrame {
    index: usize,
    atlas: [u32; 2],
    size: [u16; 2],
    pivot: [u32; 2],
}

struct PackedAtlas {
    image: RgbaImage,
    cell_size: [u32; 2],
    columns: u32,
    rows: u32,
    frames: Vec<AtlasFrame>,
}

fn main() {
    let command = Command::parse(std::env::args().skip(1)).unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(2);
    });
    match export(&command) {
        Ok(summary) => {
            if command.json {
                println!("{}", serde_json::to_string(&summary).unwrap());
            } else {
                println!(
                    "exported {} assets for {} to {}",
                    summary["asset_count"],
                    PROFILE,
                    command.output.display()
                );
            }
        }
        Err(error) => {
            if command.json {
                println!(
                    "{}",
                    serde_json::to_string(&json!({"ok": false, "error": error})).unwrap()
                );
            } else {
                eprintln!("{error}");
            }
            std::process::exit(1);
        }
    }
}

impl Command {
    fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut data = None;
        let mut output = None;
        let mut profile = None;
        let mut json = false;
        let mut arguments = arguments.into_iter();
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--data" => data = arguments.next().map(PathBuf::from),
                "--output" => output = arguments.next().map(PathBuf::from),
                "--profile" => profile = arguments.next(),
                "--json" => json = true,
                _ => return Err(format!("unknown argument: {argument}")),
            }
        }
        if profile.as_deref() != Some(PROFILE) {
            return Err(format!("--profile must be {PROFILE}"));
        }
        Ok(Self {
            data: data.ok_or_else(|| "--data requires a directory".to_owned())?,
            output: output.ok_or_else(|| "--output requires a directory".to_owned())?,
            json,
        })
    }
}

fn export(command: &Command) -> Result<Value, String> {
    let required = [
        "PAT.MKF",
        "GOP.MKF",
        "MGO.MKF",
        "RGM.MKF",
        "DATA.MKF",
        "VOC.MKF",
        "MUS.MKF",
        "WOR16.ASC",
        "WOR16.FON",
    ];
    let mut inputs = BTreeMap::new();
    let mut source_hashes = BTreeMap::new();
    for name in required {
        let bytes = fs::read(command.data.join(name))
            .map_err(|error| format!("failed to read {name}: {error}"))?;
        source_hashes.insert(name.to_owned(), hash_bytes(&bytes));
        inputs.insert(name, bytes);
    }

    let palette_archive = archive(&inputs, "PAT.MKF")?;
    let palette = Palette::from_bytes(
        palette_archive
            .read_chunk(PALETTE_CHUNK)
            .ok_or_else(|| "PAT.MKF palette 0 is missing".to_owned())?,
    )
    .ok_or_else(|| "PAT.MKF palette 0 is invalid".to_owned())?;

    let temporary = command.output.with_extension("tmp");
    if temporary.exists() {
        fs::remove_dir_all(&temporary)
            .map_err(|error| format!("failed to clear {}: {error}", temporary.display()))?;
    }
    fs::create_dir_all(&temporary)
        .map_err(|error| format!("failed to create {}: {error}", temporary.display()))?;

    let mut assets = Vec::new();
    export_palette(&temporary, &palette, &mut assets)?;
    export_tiles(
        &temporary,
        archive(&inputs, "GOP.MKF")?,
        &palette,
        &mut assets,
    )?;
    export_characters(
        &temporary,
        archive(&inputs, "MGO.MKF")?,
        &palette,
        &mut assets,
    )?;
    export_portraits(
        &temporary,
        archive(&inputs, "RGM.MKF")?,
        &palette,
        &mut assets,
    )?;
    export_ui(
        &temporary,
        archive(&inputs, "DATA.MKF")?,
        &palette,
        &mut assets,
    )?;
    export_font(
        &temporary,
        inputs
            .get("WOR16.ASC")
            .ok_or_else(|| "WOR16.ASC was not loaded".to_owned())?,
        inputs
            .get("WOR16.FON")
            .ok_or_else(|| "WOR16.FON was not loaded".to_owned())?,
        &mut assets,
    )?;
    export_sounds(&temporary, archive(&inputs, "VOC.MKF")?, &mut assets)?;
    export_music(&temporary, archive(&inputs, "MUS.MKF")?, &mut assets)?;

    assets.sort_by(|left, right| left.path.cmp(&right.path));
    let asset_count = assets.len();
    let manifest = Manifest {
        schema_version: 1,
        source_variant: "dos_zh",
        export_profile: PROFILE,
        exporter_version: env!("CARGO_PKG_VERSION"),
        source_hashes,
        assets,
    };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| format!("failed to encode manifest: {error}"))?;
    fs::write(temporary.join("manifest.json"), manifest_bytes)
        .map_err(|error| format!("failed to write manifest: {error}"))?;

    if command.output.exists() {
        fs::remove_dir_all(&command.output)
            .map_err(|error| format!("failed to replace {}: {error}", command.output.display()))?;
    }
    fs::rename(&temporary, &command.output)
        .map_err(|error| format!("failed to install {}: {error}", command.output.display()))?;
    Ok(json!({
        "ok": true,
        "profile": PROFILE,
        "asset_count": asset_count,
        "manifest": "manifest.json"
    }))
}

fn archive(inputs: &BTreeMap<&str, Vec<u8>>, name: &str) -> Result<MkfArchive, String> {
    MkfArchive::new(
        inputs
            .get(name)
            .ok_or_else(|| format!("{name} was not loaded"))?,
    )
    .ok_or_else(|| format!("{name} is not a valid MKF archive"))
}

fn export_palette(
    root: &Path,
    palette: &Palette,
    assets: &mut Vec<ManifestAsset>,
) -> Result<(), String> {
    let mut image = RgbaImage::new(256, 1);
    for index in 0..=u8::MAX {
        let (r, g, b) = palette.get_rgb(index);
        image.put_pixel(u32::from(index), 0, Rgba([r, g, b, 255]));
    }
    let path = "textures/palettes/pat_000_day.png";
    save_png(root, path, &image)?;
    assets.push(asset(
        root,
        "palette",
        "PAT.MKF",
        Some(0),
        None,
        path,
        json!({"colors": 256, "size": [256, 1], "variant": "day"}),
    )?);
    Ok(())
}

fn export_tiles(
    root: &Path,
    archive: MkfArchive,
    palette: &Palette,
    assets: &mut Vec<ManifestAsset>,
) -> Result<(), String> {
    for chunk in TILE_CHUNKS {
        let sprite = Sprite::from_gop_chunk(
            archive
                .read_chunk(chunk)
                .ok_or_else(|| format!("GOP.MKF chunk {chunk} is missing"))?,
        )
        .ok_or_else(|| format!("GOP.MKF chunk {chunk} is invalid"))?;
        let frames = sprite
            .decode_frames()
            .ok_or_else(|| format!("GOP.MKF chunk {chunk} contains an invalid frame"))?;
        let packed = pack_atlas(&frames, palette, 16)?;
        let path = format!("textures/tiles/gop_{chunk:04}/atlas.png");
        save_png(root, &path, &packed.image)?;
        assets.push(asset(
            root,
            "tile_atlas",
            "GOP.MKF",
            Some(chunk),
            Some(PALETTE_CHUNK),
            &path,
            atlas_metadata(&packed, true),
        )?);
    }
    Ok(())
}

fn export_characters(
    root: &Path,
    archive: MkfArchive,
    palette: &Palette,
    assets: &mut Vec<ManifestAsset>,
) -> Result<(), String> {
    for chunk in CHARACTER_CHUNKS {
        let sprite = sprite_from_yj1_chunk(
            archive
                .read_chunk(chunk)
                .ok_or_else(|| format!("MGO.MKF chunk {chunk} is missing"))?,
        )
        .ok_or_else(|| format!("MGO.MKF chunk {chunk} is invalid"))?;
        let frames = sprite
            .decode_frames()
            .ok_or_else(|| format!("MGO.MKF chunk {chunk} contains an invalid frame"))?;
        let packed = pack_atlas(&frames, palette, 16)?;
        let path = format!("textures/characters/mgo_{chunk:04}/atlas.png");
        save_png(root, &path, &packed.image)?;
        let mut metadata = atlas_metadata(&packed, false);
        metadata["animation_hint"] = if packed.frames.len() >= 12 {
            json!({
                "directions": ["south", "west", "north", "east"],
                "frames_per_direction": 3,
                "walk_cycle": [0, 1, 0, 2]
            })
        } else {
            Value::Null
        };
        assets.push(asset(
            root,
            "field_character_atlas",
            "MGO.MKF",
            Some(chunk),
            Some(PALETTE_CHUNK),
            &path,
            metadata,
        )?);
    }
    Ok(())
}

fn export_portraits(
    root: &Path,
    archive: MkfArchive,
    palette: &Palette,
    assets: &mut Vec<ManifestAsset>,
) -> Result<(), String> {
    for chunk in PORTRAIT_CHUNKS {
        let bitmap = RleBitmap::decode(
            archive
                .read_chunk(chunk)
                .ok_or_else(|| format!("RGM.MKF chunk {chunk} is missing"))?,
        )
        .ok_or_else(|| format!("RGM.MKF chunk {chunk} is invalid"))?;
        let image = rgba_image(&bitmap, palette)?;
        let path = format!("textures/portraits/rgm_{chunk:04}.png");
        save_png(root, &path, &image)?;
        assets.push(asset(
            root,
            "portrait",
            "RGM.MKF",
            Some(chunk),
            Some(PALETTE_CHUNK),
            &path,
            json!({"size": [bitmap.width, bitmap.height], "pivot": [0, 0]}),
        )?);
    }
    Ok(())
}

fn export_ui(
    root: &Path,
    archive: MkfArchive,
    palette: &Palette,
    assets: &mut Vec<ManifestAsset>,
) -> Result<(), String> {
    for (chunk, name, selected) in [
        (9usize, "window", vec![44usize, 45, 46]),
        (12usize, "wait_icon", vec![0usize, 1, 2]),
    ] {
        let raw = archive
            .read_chunk(chunk)
            .ok_or_else(|| format!("DATA.MKF chunk {chunk} is missing"))?;
        let sprite = Sprite::from_gop_chunk(raw)
            .or_else(|| sprite_from_yj1_chunk(raw))
            .ok_or_else(|| format!("DATA.MKF chunk {chunk} is not a sprite"))?;
        let frames = selected
            .iter()
            .map(|index| {
                sprite
                    .decode_frame(*index)
                    .ok_or_else(|| format!("DATA.MKF chunk {chunk} frame {index} is invalid"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut packed = pack_atlas(&frames, palette, 8)?;
        for (frame, source_index) in packed.frames.iter_mut().zip(&selected) {
            frame.index = *source_index;
        }
        let path = format!("textures/ui/data_{chunk:04}_{name}/atlas.png");
        save_png(root, &path, &packed.image)?;
        assets.push(asset(
            root,
            "ui_atlas",
            "DATA.MKF",
            Some(chunk),
            Some(PALETTE_CHUNK),
            &path,
            atlas_metadata(&packed, false),
        )?);
    }
    Ok(())
}

fn export_font(
    root: &Path,
    code_table: &[u8],
    font_data: &[u8],
    assets: &mut Vec<ManifestAsset>,
) -> Result<(), String> {
    if code_table.is_empty() || !code_table.len().is_multiple_of(2) {
        return Err("WOR16.ASC has an invalid length".to_owned());
    }
    let glyph_count = code_table.len() / 2;
    let end = FONT_DATA_OFFSET
        .checked_add(glyph_count * FONT_GLYPH_BYTES)
        .ok_or_else(|| "font size overflow".to_owned())?;
    let glyph_data = font_data
        .get(FONT_DATA_OFFSET..end)
        .ok_or_else(|| "WOR16.FON is truncated".to_owned())?;
    let mut glyphs = BTreeMap::<u32, &[u8]>::new();
    for (code, bitmap) in code_table
        .chunks_exact(2)
        .zip(glyph_data.chunks_exact(FONT_GLYPH_BYTES))
    {
        let (decoded, _, had_errors) = BIG5.decode(code);
        if had_errors {
            continue;
        }
        let mut chars = decoded.chars();
        let Some(character) = chars.next() else {
            continue;
        };
        if chars.next().is_some() || character.is_control() {
            continue;
        }
        glyphs.entry(character as u32).or_insert(bitmap);
    }
    if glyphs.is_empty() {
        return Err("font contains no Unicode-mappable glyphs".to_owned());
    }

    let columns = 64u32;
    let rows = (glyphs.len() as u32).div_ceil(columns);
    let mut image = RgbaImage::new(columns * FONT_CELL_WIDTH, rows * FONT_CELL_HEIGHT);
    let mut descriptors = Vec::with_capacity(glyphs.len());
    for (index, (&codepoint, bitmap)) in glyphs.iter().enumerate() {
        let column = index as u32 % columns;
        let row = index as u32 / columns;
        let x = column * FONT_CELL_WIDTH;
        let y = row * FONT_CELL_HEIGHT;
        for glyph_y in 0..15u32 {
            let row_bytes = &bitmap[glyph_y as usize * 2..glyph_y as usize * 2 + 2];
            for glyph_x in 0..16u32 {
                let byte = row_bytes[glyph_x as usize / 8];
                if byte & (0x80 >> (glyph_x % 8)) != 0 {
                    image.put_pixel(x + glyph_x, y + glyph_y, Rgba([255, 255, 255, 255]));
                }
            }
        }
        descriptors.push((codepoint, x, y));
    }

    let png_path = "fonts/pal_bitmap_16.png";
    save_png(root, png_path, &image)?;
    assets.push(asset(
        root,
        "bitmap_font_texture",
        "WOR16.FON",
        None,
        None,
        png_path,
        json!({
            "glyph_count": descriptors.len(),
            "glyph_size": [16, 15],
            "atlas_size": [image.width(), image.height()]
        }),
    )?);

    let mut descriptor = format!(
        "info face=\"PAL Bitmap 16\" size=16 bold=0 italic=0 charset=\"\" unicode=1 stretchH=100 smooth=0 aa=0 padding=0,0,0,0 spacing=0,0\n\
         common lineHeight=18 base=15 scaleW={} scaleH={} pages=1 packed=0\n\
         page id=0 file=\"pal_bitmap_16.png\"\n\
         chars count={}\n",
        image.width(),
        image.height(),
        descriptors.len()
    );
    for (codepoint, x, y) in &descriptors {
        descriptor.push_str(&format!(
            "char id={codepoint} x={x} y={y} width=16 height=15 xoffset=0 yoffset=0 xadvance=16 page=0 chnl=15\n"
        ));
    }
    let fnt_path = "fonts/pal_bitmap_16.fnt";
    write_bytes(root, fnt_path, descriptor.as_bytes())?;
    assets.push(asset(
        root,
        "bitmap_font",
        "WOR16.ASC",
        None,
        None,
        fnt_path,
        json!({"texture": png_path, "line_height": 18, "unicode": true}),
    )?);
    Ok(())
}

fn export_sounds(
    root: &Path,
    archive: MkfArchive,
    assets: &mut Vec<ManifestAsset>,
) -> Result<(), String> {
    for chunk in SOUND_CHUNKS {
        let clip = VocClip::parse(
            archive
                .read_chunk(chunk)
                .ok_or_else(|| format!("VOC.MKF chunk {chunk} is missing"))?,
        )
        .ok_or_else(|| format!("VOC.MKF chunk {chunk} is invalid"))?;
        let samples = clip
            .samples
            .iter()
            .map(|sample| (i16::from(*sample) - 128) << 8)
            .collect::<Vec<_>>();
        let path = format!("audio/sfx/voc_{chunk:04}.wav");
        write_wav(root, &path, 1, clip.sample_rate, &samples)?;
        assets.push(asset(
            root,
            "sound_effect",
            "VOC.MKF",
            Some(chunk),
            None,
            &path,
            json!({
                "sample_rate": clip.sample_rate,
                "channels": 1,
                "sample_frames": samples.len(),
                "loop": false
            }),
        )?);
    }
    Ok(())
}

fn export_music(
    root: &Path,
    archive: MkfArchive,
    assets: &mut Vec<ManifestAsset>,
) -> Result<(), String> {
    let track = archive
        .read_chunk(MUSIC_CHUNK)
        .ok_or_else(|| format!("MUS.MKF chunk {MUSIC_CHUNK} is missing"))?;
    let samples = render_rix(track)?;
    let path = format!("audio/music/mus_{MUSIC_CHUNK:04}.wav");
    write_wav(root, &path, 2, MUSIC_SAMPLE_RATE, &samples)?;
    let frames = samples.len() / 2;
    assets.push(asset(
        root,
        "music",
        "MUS.MKF",
        Some(MUSIC_CHUNK),
        None,
        &path,
        json!({
            "sample_rate": MUSIC_SAMPLE_RATE,
            "channels": 2,
            "sample_frames": frames,
            "loop_begin": 0,
            "loop_end": frames
        }),
    )?);
    Ok(())
}

fn render_rix(data: &[u8]) -> Result<Vec<i16>, String> {
    let track = RixTrack::parse(data).ok_or_else(|| "RIX track is invalid".to_owned())?;
    let mut sequencer = RixSequencer::new(track);
    let mut chip = Opl3Chip::new(MUSIC_SAMPLE_RATE);
    let samples_per_tick = MUSIC_SAMPLE_RATE as usize / RIX_TICKS_PER_SECOND;
    let mut output = Vec::new();
    for _ in 0..MAX_RIX_TICKS {
        let Some(writes) = sequencer.advance() else {
            if sequencer.ended_cleanly() && !output.is_empty() {
                return Ok(output);
            }
            return Err("RIX track ended without a clean marker".to_owned());
        };
        for write in writes {
            chip.write_register(write.register, write.value);
        }
        let start = output.len();
        output.resize(start + samples_per_tick * 2, 0);
        chip.generate_stream(&mut output[start..])
            .map_err(|error| format!("OPL2 synthesis failed: {error:?}"))?;
    }
    Err("RIX track exceeds the 15 minute export limit".to_owned())
}

fn pack_atlas(
    frames: &[RleBitmap],
    palette: &Palette,
    max_columns: u32,
) -> Result<PackedAtlas, String> {
    if frames.is_empty() {
        return Err("cannot pack an empty atlas".to_owned());
    }
    let cell_width = frames
        .iter()
        .map(|frame| u32::from(frame.width))
        .max()
        .unwrap()
        .next_multiple_of(2);
    let cell_height = frames
        .iter()
        .map(|frame| u32::from(frame.height))
        .max()
        .unwrap()
        .next_multiple_of(2);
    let columns = (frames.len() as u32).min(max_columns.max(1));
    let rows = (frames.len() as u32).div_ceil(columns);
    let mut image = RgbaImage::new(cell_width * columns, cell_height * rows);
    let mut metadata = Vec::with_capacity(frames.len());
    for (index, frame) in frames.iter().enumerate() {
        let column = index as u32 % columns;
        let row = index as u32 / columns;
        let left = column * cell_width + (cell_width - u32::from(frame.width)) / 2;
        let top = row * cell_height + cell_height - u32::from(frame.height);
        let frame_image = rgba_image(frame, palette)?;
        image::imageops::overlay(&mut image, &frame_image, i64::from(left), i64::from(top));
        metadata.push(AtlasFrame {
            index,
            atlas: [column, row],
            size: [frame.width, frame.height],
            pivot: [cell_width / 2, cell_height],
        });
    }
    Ok(PackedAtlas {
        image,
        cell_size: [cell_width, cell_height],
        columns,
        rows,
        frames: metadata,
    })
}

fn atlas_metadata(atlas: &PackedAtlas, tile: bool) -> Value {
    let mut metadata = json!({
        "cell_size": atlas.cell_size,
        "columns": atlas.columns,
        "rows": atlas.rows,
        "frames": atlas.frames,
        "alignment": "bottom_center"
    });
    if tile {
        metadata["logical_tile_size"] = json!([32, 16]);
        metadata["texture_origin"] = json!([0, 8 - atlas.cell_size[1] as i32 / 2]);
    }
    metadata
}

fn rgba_image(bitmap: &RleBitmap, palette: &Palette) -> Result<RgbaImage, String> {
    RgbaImage::from_raw(
        u32::from(bitmap.width),
        u32::from(bitmap.height),
        bitmap.to_rgba(palette),
    )
    .ok_or_else(|| "decoded bitmap has an invalid RGBA buffer".to_owned())
}

fn save_png(root: &Path, path: &str, image: &RgbaImage) -> Result<(), String> {
    let destination = root.join(path);
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }
    image
        .save(&destination)
        .map_err(|error| format!("failed to save {}: {error}", destination.display()))
}

fn write_wav(
    root: &Path,
    path: &str,
    channels: u16,
    sample_rate: u32,
    samples: &[i16],
) -> Result<(), String> {
    let destination = root.join(path);
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }
    let specification = hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&destination, specification)
        .map_err(|error| format!("failed to create {}: {error}", destination.display()))?;
    for sample in samples {
        writer
            .write_sample(*sample)
            .map_err(|error| format!("failed to write {}: {error}", destination.display()))?;
    }
    writer
        .finalize()
        .map_err(|error| format!("failed to finalize {}: {error}", destination.display()))
}

fn write_bytes(root: &Path, path: &str, bytes: &[u8]) -> Result<(), String> {
    let destination = root.join(path);
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }
    fs::write(&destination, bytes)
        .map_err(|error| format!("failed to write {}: {error}", destination.display()))
}

fn asset(
    root: &Path,
    kind: &str,
    file: &str,
    chunk: Option<usize>,
    palette: Option<usize>,
    path: &str,
    metadata: Value,
) -> Result<ManifestAsset, String> {
    let bytes =
        fs::read(root.join(path)).map_err(|error| format!("failed to hash {path}: {error}"))?;
    Ok(ManifestAsset {
        kind: kind.to_owned(),
        source: Source {
            file: file.to_owned(),
            chunk,
            palette,
        },
        path: path.to_owned(),
        sha256: hash_bytes(&bytes),
        metadata,
    })
}

fn hash_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlas_places_different_frames_on_the_same_bottom_edge() {
        let palette = Palette::default();
        let frames = [
            RleBitmap {
                width: 2,
                height: 2,
                pixels: vec![1; 4],
                opaque: vec![true; 4],
            },
            RleBitmap {
                width: 4,
                height: 4,
                pixels: vec![1; 16],
                opaque: vec![true; 16],
            },
        ];
        let atlas = pack_atlas(&frames, &palette, 2).unwrap();
        assert_eq!(atlas.cell_size, [4, 4]);
        assert_eq!(atlas.frames[0].pivot, [2, 4]);
        assert_eq!(atlas.frames[1].pivot, [2, 4]);
        assert_eq!(atlas.image.dimensions(), (8, 4));
    }

    #[test]
    fn command_requires_the_explicit_profile() {
        assert!(Command::parse([]).is_err());
        let parsed = Command::parse([
            "--data".to_owned(),
            "data".to_owned(),
            "--output".to_owned(),
            "generated".to_owned(),
            "--profile".to_owned(),
            PROFILE.to_owned(),
        ])
        .unwrap();
        assert_eq!(parsed.data, PathBuf::from("data"));
        assert_eq!(parsed.output, PathBuf::from("generated"));
    }
}
