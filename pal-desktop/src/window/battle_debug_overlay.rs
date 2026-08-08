//! Read-only battle assistance rendered beside the scaled game framebuffer.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use encoding_rs::BIG5;
use fontdue::{Font, FontSettings};
use pal_assets::text::TextLibrary;
use pal_core::battle::{BattleState, BattleStatus};
use pixels::wgpu;

use super::game_viewport::SurfaceRect;

const SURFACE_MARGIN: f32 = 12.0;
const MAX_PANEL_WIDTH: f32 = 300.0;
const MAX_RASTER_SCALE: f32 = 1.5;

const PANEL: [u8; 4] = [8, 15, 24, 205];
const PANEL_BORDER: [u8; 4] = [86, 111, 137, 220];
const CARD: [u8; 4] = [18, 29, 43, 184];
const CARD_BORDER: [u8; 4] = [50, 70, 91, 200];
const TEXT: [u8; 4] = [232, 239, 246, 255];
const MUTED: [u8; 4] = [148, 165, 183, 255];
const ACCENT: [u8; 4] = [74, 214, 232, 255];
const GOOD: [u8; 4] = [92, 219, 137, 255];
const WARNING: [u8; 4] = [247, 190, 78, 255];
const DANGER: [u8; 4] = [244, 105, 116, 255];
const BAR_TRACK: [u8; 4] = [43, 56, 70, 255];

const SHADER: &str = r#"
@group(0) @binding(0)
var panel: texture_2d<f32>;

@group(0) @binding(1)
var panel_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let uv = vec2<f32>(f32(index & 1u), f32(index >> 1u));
    var output: VertexOutput;
    output.position = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    output.uv = uv;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(panel, panel_sampler, input.uv);
}
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
struct StatusView {
    label: &'static str,
    rounds: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EnemyView {
    index: usize,
    name: String,
    hp: u16,
    max_hp: u16,
    alive: bool,
    level: u16,
    attack: u16,
    defense: u16,
    physical_resistance: u16,
    elemental_resistance: [u16; pal_assets::battle::MAGIC_ELEMENT_COUNT],
    statuses: Vec<StatusView>,
    poisoned: bool,
}

impl EnemyView {
    fn display_hp(&self) -> u16 {
        u16::try_from((self.hp as i16).max(0))
            .unwrap_or(0)
            .min(self.max_hp)
    }

    fn status_summary(&self) -> String {
        let mut labels = self
            .statuses
            .iter()
            .map(|status| format!("{}{}", status.label, status.rounds))
            .collect::<Vec<_>>();
        if self.poisoned {
            labels.push("毒".to_owned());
        }
        labels.join(" · ")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct BattleDebugSnapshot {
    scale_factor: f64,
    surface_width: u32,
    surface_height: u32,
    round: u32,
    selected_enemy: Option<usize>,
    enemies: Vec<EnemyView>,
}

impl BattleDebugSnapshot {
    pub(super) fn capture(
        battle: &BattleState,
        text: &TextLibrary,
        selected_enemy: Option<usize>,
        scale_factor: f64,
        surface_width: u32,
        surface_height: u32,
    ) -> Self {
        let enemies = battle
            .enemies
            .iter()
            .enumerate()
            .filter(|(_, enemy)| enemy.object_id != 0)
            .map(|(index, enemy)| EnemyView {
                index,
                name: text
                    .word(usize::from(enemy.object_id))
                    .and_then(decode_big5)
                    .unwrap_or_else(|| format!("敌人 {}", index + 1)),
                hp: enemy.hp,
                max_hp: enemy.max_hp,
                alive: enemy.is_alive(),
                level: enemy.level,
                attack: enemy.effective_attack_strength(),
                defense: enemy.effective_defense(),
                physical_resistance: enemy.physical_resistance,
                elemental_resistance: enemy.elemental_resistance,
                statuses: BattleStatus::ALL
                    .into_iter()
                    .filter_map(|status| {
                        let rounds = enemy.statuses.duration(status);
                        (rounds != 0).then_some(StatusView {
                            label: status_label(status),
                            rounds,
                        })
                    })
                    .collect(),
                poisoned: enemy.poisons.iter().any(|poison| poison.object_id != 0),
            })
            .collect::<Vec<_>>();
        Self {
            scale_factor,
            surface_width,
            surface_height,
            round: battle.round(),
            selected_enemy: selected_enemy
                .filter(|index| enemies.iter().any(|enemy| enemy.index == *index)),
            enemies,
        }
    }

    fn selected_enemy(&self) -> Option<&EnemyView> {
        let selected = self.selected_enemy?;
        self.enemies.iter().find(|enemy| enemy.index == selected)
    }
}

fn decode_big5(text: &[u8]) -> Option<String> {
    let (decoded, _, had_errors) = BIG5.decode(text);
    (!had_errors && !decoded.is_empty()).then(|| decoded.into_owned())
}

fn status_label(status: BattleStatus) -> &'static str {
    match status {
        BattleStatus::Confused => "乱",
        BattleStatus::Paralyzed => "定",
        BattleStatus::Sleep => "眠",
        BattleStatus::Silence => "封",
        BattleStatus::Puppet => "傀",
        BattleStatus::Bravery => "勇",
        BattleStatus::Protect => "护",
        BattleStatus::Haste => "速",
        BattleStatus::DualAttack => "连",
    }
}

pub(super) struct BattleDebugOverlay {
    bind_group_layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    texture: wgpu::Texture,
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    display_width: u32,
    display_height: u32,
    scale_factor: f64,
    font: UiFont,
    cached_snapshot: Option<BattleDebugSnapshot>,
}

impl BattleDebugOverlay {
    pub(super) fn new(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        scale_factor: f64,
    ) -> Self {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("battle_debug_overlay_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("battle_debug_overlay_shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("battle_debug_overlay_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("battle_debug_overlay_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
        });
        let (texture, bind_group) = create_texture(device, &bind_group_layout, 1, 1);
        let font = UiFont::discover();
        eprintln!("battle debug overlay font: {}", font.name());
        Self {
            bind_group_layout,
            bind_group,
            pipeline,
            texture,
            pixels: Vec::new(),
            width: 1,
            height: 1,
            display_width: 1,
            display_height: 1,
            scale_factor,
            font,
            cached_snapshot: None,
        }
    }

    pub(super) fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        snapshot: BattleDebugSnapshot,
    ) {
        if !replace_changed_snapshot(&mut self.cached_snapshot, snapshot) {
            return;
        }
        let snapshot = self
            .cached_snapshot
            .as_ref()
            .expect("changed battle debug snapshot was cached");
        let frame = rasterize_snapshot(&mut self.font, snapshot, &mut self.pixels);
        if self.width != frame.width || self.height != frame.height {
            let (texture, bind_group) =
                create_texture(device, &self.bind_group_layout, frame.width, frame.height);
            self.texture = texture;
            self.bind_group = bind_group;
            self.width = frame.width;
            self.height = frame.height;
        }
        self.display_width = frame.display_width;
        self.display_height = frame.display_height;
        self.scale_factor = snapshot.scale_factor;
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &self.pixels,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(frame.width * 4),
                rows_per_image: Some(frame.height),
            },
            wgpu::Extent3d {
                width: frame.width,
                height: frame.height,
                depth_or_array_layers: 1,
            },
        );
    }

    pub(super) fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        render_target: &wgpu::TextureView,
        panel: SurfaceRect,
    ) {
        let margin = physical(SURFACE_MARGIN, self.scale_factor);
        let width = self
            .display_width
            .min(panel.width.saturating_sub(margin.saturating_mul(2)));
        let height = self
            .display_height
            .min(panel.height.saturating_sub(margin.saturating_mul(2)));
        if width == 0 || height == 0 {
            return;
        }
        let x = panel.x + panel.width.saturating_sub(width) / 2;
        let y = panel.y + panel.height.saturating_sub(height) / 2;
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("battle_debug_overlay_render_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: render_target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: true,
                },
            })],
            depth_stencil_attachment: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_viewport(x as f32, y as f32, width as f32, height as f32, 0.0, 1.0);
        pass.set_scissor_rect(x, y, width, height);
        pass.draw(0..4, 0..1);
    }
}

fn replace_changed_snapshot(
    cached: &mut Option<BattleDebugSnapshot>,
    snapshot: BattleDebugSnapshot,
) -> bool {
    if cached.as_ref() == Some(&snapshot) {
        return false;
    }
    *cached = Some(snapshot);
    true
}

struct RasterFrame {
    width: u32,
    height: u32,
    display_width: u32,
    display_height: u32,
}

fn rasterize_snapshot(
    font: &mut UiFont,
    snapshot: &BattleDebugSnapshot,
    pixels: &mut Vec<u8>,
) -> RasterFrame {
    const HEADER_HEIGHT: f32 = 38.0;
    const TARGET_HEIGHT: f32 = 92.0;
    const SECTION_LABEL_HEIGHT: f32 = 18.0;
    const ENEMY_STEP: f32 = 44.0;

    let display_scale = normalized_scale(snapshot.scale_factor);
    let raster_scale = display_scale.min(MAX_RASTER_SCALE);
    let logical_surface_width = snapshot.surface_width as f32 / display_scale;
    let logical_surface_height = snapshot.surface_height as f32 / display_scale;
    let panel_width =
        MAX_PANEL_WIDTH.min((logical_surface_width - SURFACE_MARGIN * 2.0).max(280.0));
    let target_height = snapshot.selected_enemy().map_or(0.0, |_| TARGET_HEIGHT);
    let enemy_height = SECTION_LABEL_HEIGHT + snapshot.enemies.len() as f32 * ENEMY_STEP;
    let panel_height = (HEADER_HEIGHT + target_height + enemy_height + 8.0)
        .min((logical_surface_height - SURFACE_MARGIN * 2.0).max(140.0));
    let width = physical(panel_width, f64::from(raster_scale));
    let height = physical(panel_height, f64::from(raster_scale));
    let display_width = physical(panel_width, f64::from(display_scale));
    let display_height = physical(panel_height, f64::from(display_scale));
    pixels.resize(width as usize * height as usize * 4, 0);
    pixels.fill(0);
    let mut painter = Painter {
        pixels,
        width,
        height,
        scale: raster_scale,
        font,
    };
    painter.rounded_rect(0.0, 0.0, panel_width, panel_height, 10.0, PANEL_BORDER);
    painter.rounded_rect(1.0, 1.0, panel_width - 2.0, panel_height - 2.0, 9.0, PANEL);

    painter.text(14.0, 8.0, 15.0, "战斗助手", ACCENT, None);
    painter.text(
        panel_width - 112.0,
        11.0,
        10.0,
        &format!("第 {} 回合 · F7 隐藏", snapshot.round),
        MUTED,
        Some(RectF::new(panel_width - 116.0, 6.0, 104.0, 24.0)),
    );

    let content_x = 12.0;
    let content_width = panel_width - 24.0;
    let enemies_y = snapshot.selected_enemy().map_or(HEADER_HEIGHT, |enemy| {
        draw_target_card(&mut painter, enemy, content_x, HEADER_HEIGHT, content_width);
        HEADER_HEIGHT + TARGET_HEIGHT
    });
    draw_enemy_overview(
        &mut painter,
        &snapshot.enemies,
        snapshot.selected_enemy,
        content_x,
        enemies_y,
        content_width,
        panel_height,
    );

    RasterFrame {
        width,
        height,
        display_width,
        display_height,
    }
}

fn draw_target_card(painter: &mut Painter<'_>, enemy: &EnemyView, x: f32, y: f32, width: f32) {
    painter.rounded_rect(x, y, width, 86.0, 5.0, ACCENT);
    painter.rounded_rect(x + 1.0, y + 1.0, width - 2.0, 84.0, 4.0, CARD);
    painter.text(x + 8.0, y + 5.0, 9.0, "当前目标", MUTED, None);
    painter.text(
        x + 66.0,
        y + 4.0,
        11.0,
        &format!("{}  Lv.{}", enemy.name, enemy.level),
        ACCENT,
        Some(RectF::new(x + 66.0, y + 1.0, width - 74.0, 18.0)),
    );
    painter.text(
        x + 8.0,
        y + 23.0,
        10.0,
        &format!("HP  {} / {}", enemy.display_hp(), enemy.max_hp),
        TEXT,
        None,
    );
    painter.bar(
        x + 104.0,
        y + 28.0,
        width - 116.0,
        5.0,
        enemy.display_hp(),
        enemy.max_hp,
        hp_color(enemy),
    );
    painter.text(
        x + 8.0,
        y + 42.0,
        9.5,
        &format!(
            "攻击 {}   防御 {}   物抗 {}",
            enemy.attack, enemy.defense, enemy.physical_resistance
        ),
        TEXT,
        Some(RectF::new(x + 8.0, y + 38.0, width - 16.0, 17.0)),
    );
    let [wind, thunder, water, fire, earth] = enemy.elemental_resistance;
    painter.text(
        x + 8.0,
        y + 58.0,
        9.0,
        &format!("风 {wind}  雷 {thunder}  水 {water}  火 {fire}  土 {earth}"),
        MUTED,
        Some(RectF::new(x + 8.0, y + 55.0, width - 16.0, 16.0)),
    );
    let statuses = enemy.status_summary();
    if !statuses.is_empty() {
        painter.text(
            x + 8.0,
            y + 73.0,
            8.5,
            &format!("状态  {statuses}"),
            WARNING,
            Some(RectF::new(x + 8.0, y + 70.0, width - 16.0, 14.0)),
        );
    }
}

fn hp_color(enemy: &EnemyView) -> [u8; 4] {
    if !enemy.alive {
        DANGER
    } else if enemy.max_hp != 0 && u32::from(enemy.display_hp()) * 4 <= u32::from(enemy.max_hp) {
        WARNING
    } else {
        GOOD
    }
}

fn draw_enemy_overview(
    painter: &mut Painter<'_>,
    enemies: &[EnemyView],
    selected_enemy: Option<usize>,
    x: f32,
    y: f32,
    width: f32,
    panel_height: f32,
) {
    painter.text(x + 2.0, y + 1.0, 10.0, "敌方概览", MUTED, None);
    let mut row_y = y + 18.0;
    for enemy in enemies {
        if row_y + 40.0 > panel_height - 6.0 {
            break;
        }
        let selected = selected_enemy == Some(enemy.index);
        painter.rounded_rect(
            x,
            row_y,
            width,
            40.0,
            4.0,
            if selected { ACCENT } else { CARD_BORDER },
        );
        painter.rounded_rect(x + 1.0, row_y + 1.0, width - 2.0, 38.0, 3.0, CARD);
        painter.text(
            x + 8.0,
            row_y + 4.0,
            10.0,
            &format!("{}  Lv.{}", enemy.name, enemy.level),
            if selected { ACCENT } else { TEXT },
            Some(RectF::new(x + 8.0, row_y + 1.0, width - 58.0, 17.0)),
        );
        if !enemy.alive {
            painter.text(x + width - 42.0, row_y + 4.0, 9.0, "倒下", DANGER, None);
        }
        painter.text(
            x + 8.0,
            row_y + 22.0,
            9.0,
            &format!("HP {}/{}", enemy.display_hp(), enemy.max_hp),
            if enemy.alive { MUTED } else { DANGER },
            None,
        );
        painter.bar(
            x + width - 96.0,
            row_y + 28.0,
            82.0,
            4.0,
            enemy.display_hp(),
            enemy.max_hp,
            hp_color(enemy),
        );
        row_y += 44.0;
    }
}

#[derive(Clone)]
struct CachedGlyph {
    width: usize,
    height: usize,
    xmin: i32,
    ymin: i32,
    advance: f32,
    bitmap: Arc<[u8]>,
}

struct UiFont {
    font: Option<Font>,
    name: String,
    glyphs: HashMap<(char, u16), CachedGlyph>,
}

impl UiFont {
    fn discover() -> Self {
        for (path, name, collection_index) in font_candidates() {
            if let Some(font) = load_font(&path, collection_index) {
                return Self {
                    font: Some(font),
                    name,
                    glyphs: HashMap::new(),
                };
            }
        }
        Self {
            font: None,
            name: "内置 ASCII 回退".to_owned(),
            glyphs: HashMap::new(),
        }
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn glyph(&mut self, character: char, size: u16) -> CachedGlyph {
        if let Some(glyph) = self.glyphs.get(&(character, size)) {
            return glyph.clone();
        }
        let glyph = if let Some(font) = &self.font {
            let (metrics, bitmap) = font.rasterize(character, f32::from(size));
            CachedGlyph {
                width: metrics.width,
                height: metrics.height,
                xmin: metrics.xmin,
                ymin: metrics.ymin,
                advance: metrics.advance_width,
                bitmap: bitmap.into(),
            }
        } else {
            fallback_glyph(character, size)
        };
        self.glyphs.insert((character, size), glyph.clone());
        glyph
    }
}

fn load_font(path: &Path, collection_index: u32) -> Option<Font> {
    let bytes = fs::read(path).ok()?;
    Font::from_bytes(
        bytes,
        FontSettings {
            collection_index,
            ..FontSettings::default()
        },
    )
    .ok()
}

fn font_candidates() -> Vec<(PathBuf, String, u32)> {
    let mut candidates = Vec::new();
    if let Some(path) = env::var_os("RUST_PAL_DEBUG_FONT") {
        candidates.push((PathBuf::from(path), "自定义调试字体".to_owned(), 0));
    }
    #[cfg(target_os = "macos")]
    candidates.extend([
        (
            PathBuf::from("/System/Library/Fonts/Hiragino Sans GB.ttc"),
            "冬青黑体简体".to_owned(),
            0,
        ),
        (
            PathBuf::from("/System/Library/Fonts/STHeiti Medium.ttc"),
            "华文黑体".to_owned(),
            0,
        ),
    ]);
    #[cfg(target_os = "windows")]
    if let Some(windows) = env::var_os("WINDIR") {
        let fonts = PathBuf::from(windows).join("Fonts");
        candidates.extend([
            (fonts.join("msyh.ttc"), "微软雅黑".to_owned(), 0),
            (fonts.join("msyhbd.ttc"), "微软雅黑粗体".to_owned(), 0),
            (fonts.join("simhei.ttf"), "黑体".to_owned(), 0),
        ]);
    }
    #[cfg(target_os = "linux")]
    candidates.extend([
        (
            PathBuf::from("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"),
            "Noto Sans CJK SC".to_owned(),
            2,
        ),
        (
            PathBuf::from("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc"),
            "Noto Sans CJK SC".to_owned(),
            2,
        ),
        (
            PathBuf::from("/usr/share/fonts/truetype/wqy/wqy-microhei.ttc"),
            "文泉驿微米黑".to_owned(),
            0,
        ),
    ]);
    if let Some((path, collection_index)) = fontconfig_match() {
        candidates.push((path, "系统中文字体".to_owned(), collection_index));
    }
    candidates
}

fn fontconfig_match() -> Option<(PathBuf, u32)> {
    let output = Command::new("fc-match")
        .args(["-f", "%{file}\t%{index}", "Noto Sans CJK SC:lang=zh-cn"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let output = String::from_utf8_lossy(&output.stdout);
    let (path, index) = output.trim().rsplit_once('\t')?;
    Some((PathBuf::from(path), index.parse().ok()?))
}

fn fallback_glyph(character: char, size: u16) -> CachedGlyph {
    let bits = crate::debug_overlay::glyph(character);
    let scale = (u32::from(size) / 7).max(1) as usize;
    let width = 5 * scale;
    let height = 7 * scale;
    let mut bitmap = vec![0; width * height];
    for (row, row_bits) in bits.into_iter().enumerate() {
        for column in 0..5 {
            if row_bits & (0b1_0000 >> column) == 0 {
                continue;
            }
            for dy in 0..scale {
                for dx in 0..scale {
                    bitmap[(row * scale + dy) * width + column * scale + dx] = 255;
                }
            }
        }
    }
    CachedGlyph {
        width,
        height,
        xmin: 0,
        ymin: 0,
        advance: (6 * scale) as f32,
        bitmap: bitmap.into(),
    }
}

#[derive(Clone, Copy)]
struct RectF {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl RectF {
    fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

struct Painter<'a> {
    pixels: &'a mut [u8],
    width: u32,
    height: u32,
    scale: f32,
    font: &'a mut UiFont,
}

impl Painter<'_> {
    fn rounded_rect(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radius: f32,
        color: [u8; 4],
    ) {
        let x = self.px(x) as i32;
        let y = self.px(y) as i32;
        let width = self.px(width) as i32;
        let height = self.px(height) as i32;
        let radius = self.px(radius) as i32;
        for row in 0..height {
            for column in 0..width {
                let dx = if column < radius {
                    radius - column
                } else if column >= width - radius {
                    column - (width - radius - 1)
                } else {
                    0
                };
                let dy = if row < radius {
                    radius - row
                } else if row >= height - radius {
                    row - (height - radius - 1)
                } else {
                    0
                };
                if dx != 0 && dy != 0 && dx * dx + dy * dy > radius * radius {
                    continue;
                }
                blend_pixel(
                    self.pixels,
                    self.width,
                    self.height,
                    x + column,
                    y + row,
                    color,
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn bar(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        current: u16,
        maximum: u16,
        color: [u8; 4],
    ) {
        self.rounded_rect(x, y, width, height, height / 2.0, BAR_TRACK);
        let ratio = if maximum == 0 {
            0.0
        } else {
            f32::from(current.min(maximum)) / f32::from(maximum)
        };
        if ratio > 0.0 {
            self.rounded_rect(x, y, width * ratio, height, height / 2.0, color);
        }
    }

    fn text(&mut self, x: f32, y: f32, size: f32, text: &str, color: [u8; 4], clip: Option<RectF>) {
        let physical_size = self.px(size).clamp(7, u32::from(u16::MAX)) as u16;
        let mut cursor = self.px(x) as f32;
        let baseline = self.px(y + size * 0.86) as i32;
        let clip = clip.map(|clip| {
            (
                self.px(clip.x) as i32,
                self.px(clip.y) as i32,
                self.px(clip.x + clip.width) as i32,
                self.px(clip.y + clip.height) as i32,
            )
        });
        for character in text.chars() {
            let glyph = self.font.glyph(character, physical_size);
            let glyph_x = cursor.round() as i32 + glyph.xmin;
            let glyph_y = baseline - glyph.height as i32 - glyph.ymin;
            for row in 0..glyph.height {
                for column in 0..glyph.width {
                    let alpha = glyph.bitmap[row * glyph.width + column];
                    if alpha == 0 {
                        continue;
                    }
                    let pixel_x = glyph_x + column as i32;
                    let pixel_y = glyph_y + row as i32;
                    if clip.is_some_and(|(left, top, right, bottom)| {
                        pixel_x < left || pixel_x >= right || pixel_y < top || pixel_y >= bottom
                    }) {
                        continue;
                    }
                    let mut glyph_color = color;
                    glyph_color[3] = ((u16::from(color[3]) * u16::from(alpha)) / 255) as u8;
                    blend_pixel(
                        self.pixels,
                        self.width,
                        self.height,
                        pixel_x,
                        pixel_y,
                        glyph_color,
                    );
                }
            }
            cursor += glyph.advance;
        }
    }

    fn px(&self, logical: f32) -> u32 {
        (logical * self.scale).round().max(0.0) as u32
    }
}

fn blend_pixel(pixels: &mut [u8], width: u32, height: u32, x: i32, y: i32, source: [u8; 4]) {
    let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) else {
        return;
    };
    if x >= width || y >= height || source[3] == 0 {
        return;
    }
    let index = (y as usize * width as usize + x as usize) * 4;
    let destination = &mut pixels[index..index + 4];
    let source_alpha = u32::from(source[3]);
    let destination_alpha = u32::from(destination[3]);
    let output_alpha = source_alpha + destination_alpha * (255 - source_alpha) / 255;
    if output_alpha == 0 {
        return;
    }
    for channel in 0..3 {
        let source_value = u32::from(source[channel]);
        let destination_value = u32::from(destination[channel]);
        let premultiplied = source_value * source_alpha
            + destination_value * destination_alpha * (255 - source_alpha) / 255;
        destination[channel] = (premultiplied / output_alpha).min(255) as u8;
    }
    destination[3] = output_alpha.min(255) as u8;
}

fn create_texture(
    device: &wgpu::Device,
    bind_group_layout: &wgpu::BindGroupLayout,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::BindGroup) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("battle_debug_overlay_texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("battle_debug_overlay_sampler"),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("battle_debug_overlay_bind_group"),
        layout: bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });
    (texture, bind_group)
}

fn normalized_scale(scale_factor: f64) -> f32 {
    scale_factor.clamp(1.0, 3.0) as f32
}

fn physical(logical: f32, scale_factor: f64) -> u32 {
    (f64::from(logical) * scale_factor.clamp(1.0, 3.0))
        .round()
        .max(1.0) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enemy(index: usize) -> EnemyView {
        EnemyView {
            index,
            name: format!("敌人 {}", index + 1),
            hp: 80,
            max_hp: 120,
            alive: true,
            level: 12,
            attack: 82,
            defense: 64,
            physical_resistance: 2,
            elemental_resistance: [1, 2, 3, 4, 5],
            statuses: Vec::new(),
            poisoned: false,
        }
    }

    fn snapshot(selected_enemy: Option<usize>) -> BattleDebugSnapshot {
        BattleDebugSnapshot {
            scale_factor: 2.0,
            surface_width: 600,
            surface_height: 800,
            round: 3,
            selected_enemy,
            enemies: vec![enemy(0), enemy(1)],
        }
    }

    fn test_font() -> UiFont {
        UiFont {
            font: None,
            name: "测试点阵".to_owned(),
            glyphs: HashMap::new(),
        }
    }

    #[test]
    fn decodes_original_big5_enemy_names() {
        assert_eq!(
            decode_big5(&[0xa4, 0xa4, 0xa4, 0xe5]).as_deref(),
            Some("中文")
        );
        assert_eq!(decode_big5(&[]), None);
    }

    #[test]
    fn enemy_summary_only_contains_player_facing_statuses() {
        let mut enemy = enemy(0);
        enemy.statuses = vec![StatusView {
            label: "眠",
            rounds: 2,
        }];
        enemy.poisoned = true;

        assert_eq!(enemy.status_summary(), "眠2 · 毒");
    }

    #[test]
    fn target_details_only_take_space_during_enemy_selection() {
        let mut font = test_font();
        let mut pixels = Vec::new();
        let overview = rasterize_snapshot(&mut font, &snapshot(None), &mut pixels);
        let overview_height = overview.display_height;
        let selected = rasterize_snapshot(&mut font, &snapshot(Some(0)), &mut pixels);

        assert_eq!((overview.width, overview.display_width), (420, 560));
        assert_eq!(selected.display_height - overview_height, 184);
        assert_eq!(
            pixels.len(),
            selected.width as usize * selected.height as usize * 4
        );
    }

    #[test]
    fn unchanged_snapshot_skips_panel_rasterization() {
        let snapshot = snapshot(None);
        let mut cached = None;

        assert!(replace_changed_snapshot(&mut cached, snapshot.clone()));
        assert!(!replace_changed_snapshot(&mut cached, snapshot.clone()));
        let mut selected = snapshot;
        selected.selected_enemy = Some(1);
        assert!(replace_changed_snapshot(&mut cached, selected));
    }
}
