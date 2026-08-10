//! Read-only battle assistance rendered above the scaled game framebuffer.

use std::collections::HashMap;
use std::fs;
use std::sync::Arc;

use encoding_rs::BIG5;
use fontdue::{Font, FontSettings};
use pal_assets::text::TextLibrary;
use pal_core::battle::{BattleState, BattleStatus};
use pixels::wgpu;

use super::system_font::chinese_font_candidates;

const SURFACE_MARGIN: f32 = 12.0;
const MIN_PANEL_WIDTH: f32 = 260.0;
const MAX_PANEL_WIDTH: f32 = 360.0;
const MAX_RASTER_SCALE: f32 = 3.0;
const HEADER_HEIGHT: f32 = 34.0;
const DETAIL_HEIGHT: f32 = 164.0;
const OVERVIEW_LABEL_HEIGHT: f32 = 16.0;
const ENEMY_STEP: f32 = 30.0;
const PANEL_BOTTOM_PADDING: f32 = 8.0;

const PANEL: [u8; 4] = [27, 14, 8, 224];
const PANEL_BORDER: [u8; 4] = [213, 177, 109, 238];
const CARD: [u8; 4] = [53, 28, 16, 208];
const CARD_BORDER: [u8; 4] = [132, 93, 55, 220];
const TEXT: [u8; 4] = [244, 231, 194, 255];
const MUTED: [u8; 4] = [194, 158, 111, 255];
const ACCENT: [u8; 4] = [81, 218, 207, 255];
const GOOD: [u8; 4] = [83, 210, 125, 255];
const WARNING: [u8; 4] = [239, 180, 67, 255];
const DANGER: [u8; 4] = [232, 84, 71, 255];
const BAR_TRACK: [u8; 4] = [76, 47, 28, 255];

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
    magic_strength: u16,
    defense: u16,
    dexterity: u16,
    physical_resistance: u16,
    poison_resistance: u16,
    sorcery_resistance: u16,
    elemental_resistance: [u16; pal_assets::battle::MAGIC_ELEMENT_COUNT],
    magic_name: Option<String>,
    magic_rate: u16,
    dual_move: bool,
    steal: StealView,
    collect_value: u16,
    statuses: Vec<StatusView>,
    poison_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StealView {
    None,
    Cash(u16),
    Item { name: String, count: u16 },
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
        if self.poison_count != 0 {
            labels.insert(
                0,
                if self.poison_count == 1 {
                    "中毒".to_owned()
                } else {
                    format!("中毒×{}", self.poison_count)
                },
            );
        }
        if labels.is_empty() {
            "无".to_owned()
        } else {
            labels.join(" · ")
        }
    }

    fn magic_summary(&self) -> String {
        self.magic_name.as_ref().map_or_else(
            || "无".to_owned(),
            |name| format!("{name} · {}%", self.magic_rate.min(10) * 10),
        )
    }

    fn trait_summary(&self) -> String {
        let mut traits = Vec::new();
        if self.dual_move {
            traits.push("双行动".to_owned());
        }
        if self.collect_value != 0 {
            traits.push(format!("炼化 {}", self.collect_value));
        }
        if traits.is_empty() {
            "无".to_owned()
        } else {
            traits.join(" · ")
        }
    }

    fn steal_summary(&self) -> String {
        match &self.steal {
            StealView::None => "无".to_owned(),
            StealView::Cash(remaining) => format!("金钱 · 余量 {remaining}"),
            StealView::Item { name, count } => format!("{name} ×{count}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct BattleAssistSnapshot {
    scale_factor: f64,
    surface_width: u32,
    surface_height: u32,
    round: u32,
    selected_enemy: Option<usize>,
    targeting_enemy: bool,
    enemies: Vec<EnemyView>,
}

impl BattleAssistSnapshot {
    pub(super) fn capture(
        battle: &BattleState,
        text: &TextLibrary,
        selected_enemy: Option<usize>,
        targeting_enemy: bool,
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
                magic_strength: enemy.magic_strength,
                defense: enemy.effective_defense(),
                dexterity: enemy.dexterity,
                physical_resistance: enemy.physical_resistance,
                poison_resistance: enemy.poison_resistance,
                sorcery_resistance: enemy.sorcery_resistance.min(9),
                elemental_resistance: enemy.elemental_resistance,
                magic_name: enemy
                    .magic
                    .is_some()
                    .then(|| text.word(usize::from(enemy.magic_object)))
                    .flatten()
                    .and_then(decode_big5),
                magic_rate: enemy.magic_rate,
                dual_move: enemy.dual_move,
                steal: capture_steal(enemy.steal_item, enemy.steal_item_count, text),
                collect_value: enemy.collect_value,
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
                poison_count: enemy
                    .poisons
                    .iter()
                    .filter(|poison| poison.object_id != 0)
                    .count(),
            })
            .collect::<Vec<_>>();
        Self {
            scale_factor,
            surface_width,
            surface_height,
            round: battle.round(),
            selected_enemy: selected_enemy
                .filter(|index| enemies.iter().any(|enemy| enemy.index == *index))
                .or_else(|| {
                    enemies
                        .iter()
                        .find(|enemy| enemy.alive)
                        .map(|enemy| enemy.index)
                }),
            targeting_enemy,
            enemies,
        }
    }

    fn selected_enemy(&self) -> Option<&EnemyView> {
        let selected = self.selected_enemy?;
        self.enemies.iter().find(|enemy| enemy.index == selected)
    }
}

fn capture_steal(item: u16, count: u16, text: &TextLibrary) -> StealView {
    if count == 0 {
        StealView::None
    } else if item == 0 {
        StealView::Cash(count)
    } else {
        StealView::Item {
            name: text
                .word(usize::from(item))
                .and_then(decode_big5)
                .unwrap_or_else(|| "未知物品".to_owned()),
            count,
        }
    }
}

fn decode_big5(text: &[u8]) -> Option<String> {
    let (decoded, _, had_errors) = BIG5.decode(text);
    (!had_errors && !decoded.is_empty()).then(|| decoded.into_owned())
}

fn status_label(status: BattleStatus) -> &'static str {
    match status {
        BattleStatus::Confused => "混乱",
        BattleStatus::Paralyzed => "麻痹",
        BattleStatus::Sleep => "昏睡",
        BattleStatus::Silence => "封咒",
        BattleStatus::Puppet => "傀儡",
        BattleStatus::Bravery => "勇敢",
        BattleStatus::Protect => "保护",
        BattleStatus::Haste => "加速",
        BattleStatus::DualAttack => "连击",
    }
}

pub(super) struct BattleAssistOverlay {
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
    cached_snapshot: Option<BattleAssistSnapshot>,
}

impl BattleAssistOverlay {
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
        eprintln!("battle assist overlay font: {}", font.name());
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
        snapshot: BattleAssistSnapshot,
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
        surface_width: u32,
        surface_height: u32,
    ) {
        let game = game_surface_rect(surface_width, surface_height);
        let margin = physical(SURFACE_MARGIN, self.scale_factor);
        let width = self
            .display_width
            .min(game.width.saturating_sub(margin.saturating_mul(2)));
        let height = self
            .display_height
            .min(game.height.saturating_sub(margin.saturating_mul(2)));
        if width == 0 || height == 0 {
            return;
        }
        let x = game
            .x
            .saturating_add(game.width)
            .saturating_sub(margin)
            .saturating_sub(width);
        let y = game.y.saturating_add(margin);
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
    cached: &mut Option<BattleAssistSnapshot>,
    snapshot: BattleAssistSnapshot,
) -> bool {
    if cached.as_ref() == Some(&snapshot) {
        return false;
    }
    *cached = Some(snapshot);
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SurfaceRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

fn game_surface_rect(surface_width: u32, surface_height: u32) -> SurfaceRect {
    const GAME_WIDTH: u32 = 320;
    const GAME_HEIGHT: u32 = 200;
    let width_from_height = surface_height.saturating_mul(GAME_WIDTH) / GAME_HEIGHT;
    if width_from_height <= surface_width {
        SurfaceRect {
            x: surface_width.saturating_sub(width_from_height) / 2,
            y: 0,
            width: width_from_height,
            height: surface_height,
        }
    } else {
        let height_from_width = surface_width.saturating_mul(GAME_HEIGHT) / GAME_WIDTH;
        SurfaceRect {
            x: 0,
            y: surface_height.saturating_sub(height_from_width) / 2,
            width: surface_width,
            height: height_from_width,
        }
    }
}

struct RasterFrame {
    width: u32,
    height: u32,
    display_width: u32,
    display_height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct PanelLayout {
    width: f32,
    height: f32,
    overview_height: f32,
    detail_height: f32,
}

fn panel_layout(snapshot: &BattleAssistSnapshot) -> PanelLayout {
    let display_scale = normalized_scale(snapshot.scale_factor);
    let game = game_surface_rect(snapshot.surface_width, snapshot.surface_height);
    let logical_width = game.width as f32 / display_scale;
    let logical_height = game.height as f32 / display_scale;
    let available_width = (logical_width - SURFACE_MARGIN * 2.0).max(1.0);
    let available_height = (logical_height - SURFACE_MARGIN * 2.0).max(1.0);
    let desired_width = (logical_width * 0.44).clamp(MIN_PANEL_WIDTH, MAX_PANEL_WIDTH);
    let width = desired_width.min(available_width);
    let full_overview_height =
        OVERVIEW_LABEL_HEIGHT + snapshot.enemies.len() as f32 * ENEMY_STEP + 4.0;
    let show_overview = snapshot.enemies.len() > 1
        && HEADER_HEIGHT + full_overview_height + DETAIL_HEIGHT + PANEL_BOTTOM_PADDING
            <= available_height;
    let overview_height = if show_overview {
        full_overview_height
    } else {
        0.0
    };
    let detail_height = (available_height - HEADER_HEIGHT - overview_height - PANEL_BOTTOM_PADDING)
        .clamp(96.0, DETAIL_HEIGHT);
    PanelLayout {
        width,
        height: HEADER_HEIGHT + overview_height + detail_height + PANEL_BOTTOM_PADDING,
        overview_height,
        detail_height,
    }
}

fn rasterize_snapshot(
    font: &mut UiFont,
    snapshot: &BattleAssistSnapshot,
    pixels: &mut Vec<u8>,
) -> RasterFrame {
    let display_scale = normalized_scale(snapshot.scale_factor);
    let raster_scale = display_scale.min(MAX_RASTER_SCALE);
    let layout = panel_layout(snapshot);
    let width = physical(layout.width, f64::from(raster_scale));
    let height = physical(layout.height, f64::from(raster_scale));
    let display_width = physical(layout.width, f64::from(display_scale));
    let display_height = physical(layout.height, f64::from(display_scale));
    pixels.resize(width as usize * height as usize * 4, 0);
    pixels.fill(0);
    let mut painter = Painter {
        pixels,
        width,
        height,
        scale: raster_scale,
        font,
    };

    painter.rounded_rect(0.0, 0.0, layout.width, layout.height, 5.0, PANEL_BORDER);
    painter.rounded_rect(
        1.0,
        1.0,
        layout.width - 2.0,
        layout.height - 2.0,
        4.0,
        PANEL,
    );
    painter.text(12.0, 7.0, 14.0, "战斗助手", TEXT, None);
    painter.text(
        layout.width - 126.0,
        10.0,
        9.5,
        &format!("第 {} 回合  ·  F7 隐藏", snapshot.round),
        MUTED,
        Some(RectF::new(layout.width - 130.0, 5.0, 120.0, 22.0)),
    );

    let content_x = 10.0;
    let content_width = layout.width - 20.0;
    let mut detail_y = HEADER_HEIGHT;
    if layout.overview_height != 0.0 {
        draw_enemy_overview(
            &mut painter,
            &snapshot.enemies,
            snapshot.selected_enemy,
            content_x,
            HEADER_HEIGHT,
            content_width,
        );
        detail_y += layout.overview_height;
    }
    if let Some(enemy) = snapshot.selected_enemy() {
        draw_target_details(
            &mut painter,
            enemy,
            content_x,
            detail_y,
            content_width,
            layout.detail_height,
            snapshot.targeting_enemy,
        );
    } else {
        painter.rounded_rect(
            content_x,
            detail_y,
            content_width,
            layout.detail_height,
            3.0,
            CARD_BORDER,
        );
        painter.rounded_rect(
            content_x + 1.0,
            detail_y + 1.0,
            content_width - 2.0,
            layout.detail_height - 2.0,
            2.0,
            CARD,
        );
        painter.text(
            content_x + 10.0,
            detail_y + 12.0,
            11.0,
            "暂无敌人信息",
            MUTED,
            None,
        );
    }

    RasterFrame {
        width,
        height,
        display_width,
        display_height,
    }
}

fn draw_enemy_overview(
    painter: &mut Painter<'_>,
    enemies: &[EnemyView],
    selected_enemy: Option<usize>,
    x: f32,
    y: f32,
    width: f32,
) {
    painter.text(x + 2.0, y, 9.0, "敌方概览", MUTED, None);
    let mut row_y = y + OVERVIEW_LABEL_HEIGHT;
    for enemy in enemies {
        let selected = selected_enemy == Some(enemy.index);
        painter.rounded_rect(
            x,
            row_y,
            width,
            ENEMY_STEP - 2.0,
            3.0,
            if selected { ACCENT } else { CARD_BORDER },
        );
        painter.rounded_rect(
            x + 1.0,
            row_y + 1.0,
            width - 2.0,
            ENEMY_STEP - 4.0,
            2.0,
            CARD,
        );
        painter.text(
            x + 7.0,
            row_y + 3.0,
            9.5,
            &format!("{}  Lv.{}", enemy.name, enemy.level),
            if selected { ACCENT } else { TEXT },
            Some(RectF::new(x + 7.0, row_y + 1.0, width - 98.0, 14.0)),
        );
        let status = enemy.status_summary();
        if status != "无" {
            painter.text(
                x + width - 88.0,
                row_y + 4.0,
                8.0,
                &status,
                WARNING,
                Some(RectF::new(x + width - 91.0, row_y + 1.0, 82.0, 13.0)),
            );
        }
        painter.text(
            x + 7.0,
            row_y + 16.0,
            8.5,
            &format!("HP {}/{}", enemy.display_hp(), enemy.max_hp),
            if enemy.alive { MUTED } else { DANGER },
            None,
        );
        painter.bar(
            x + width - 86.0,
            row_y + 20.0,
            76.0,
            3.0,
            enemy.display_hp(),
            enemy.max_hp,
            hp_color(enemy),
        );
        row_y += ENEMY_STEP;
    }
}

fn draw_target_details(
    painter: &mut Painter<'_>,
    enemy: &EnemyView,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    targeting_enemy: bool,
) {
    painter.rounded_rect(
        x,
        y,
        width,
        height,
        3.0,
        if targeting_enemy {
            ACCENT
        } else {
            PANEL_BORDER
        },
    );
    painter.rounded_rect(x + 1.0, y + 1.0, width - 2.0, height - 2.0, 2.0, CARD);
    painter.text(
        x + 8.0,
        y + 4.0,
        11.5,
        &format!("{}  Lv.{}", enemy.name, enemy.level),
        ACCENT,
        Some(RectF::new(x + 8.0, y + 1.0, width - 72.0, 18.0)),
    );
    if targeting_enemy {
        painter.text(
            x + width - 56.0,
            y + 6.0,
            8.5,
            "选敌中",
            WARNING,
            Some(RectF::new(x + width - 60.0, y + 2.0, 50.0, 15.0)),
        );
    }
    painter.text(
        x + 8.0,
        y + 21.0,
        9.5,
        &format!("HP  {} / {}", enemy.display_hp(), enemy.max_hp),
        TEXT,
        None,
    );
    painter.bar(
        x + 94.0,
        y + 26.0,
        (width - 104.0).max(24.0),
        4.0,
        enemy.display_hp(),
        enemy.max_hp,
        hp_color(enemy),
    );

    draw_detail_line(
        painter,
        x,
        y,
        width,
        height,
        40.0,
        &format!("攻击 {:<5}   灵力 {}", enemy.attack, enemy.magic_strength),
        TEXT,
    );
    draw_detail_line(
        painter,
        x,
        y,
        width,
        height,
        55.0,
        &format!("防御 {:<5}   身法 {}", enemy.defense, enemy.dexterity),
        TEXT,
    );
    draw_detail_line(
        painter,
        x,
        y,
        width,
        height,
        70.0,
        &format!(
            "物抗 {}   毒抗 {}   异常抗 {}/9",
            enemy.physical_resistance, enemy.poison_resistance, enemy.sorcery_resistance
        ),
        TEXT,
    );
    let [wind, thunder, water, fire, earth] = enemy.elemental_resistance;
    draw_detail_line(
        painter,
        x,
        y,
        width,
        height,
        85.0,
        &format!("风 {wind}  雷 {thunder}  水 {water}  火 {fire}  土 {earth}"),
        MUTED,
    );
    draw_detail_line(
        painter,
        x,
        y,
        width,
        height,
        100.0,
        &format!("状态  {}", enemy.status_summary()),
        WARNING,
    );
    draw_detail_line(
        painter,
        x,
        y,
        width,
        height,
        115.0,
        &format!("法术  {}", enemy.magic_summary()),
        TEXT,
    );
    draw_detail_line(
        painter,
        x,
        y,
        width,
        height,
        130.0,
        &format!("特性  {}", enemy.trait_summary()),
        TEXT,
    );
    draw_detail_line(
        painter,
        x,
        y,
        width,
        height,
        145.0,
        &format!("可偷  {}", enemy.steal_summary()),
        TEXT,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_detail_line(
    painter: &mut Painter<'_>,
    card_x: f32,
    card_y: f32,
    card_width: f32,
    card_height: f32,
    relative_y: f32,
    text: &str,
    color: [u8; 4],
) {
    if relative_y + 13.0 > card_height - 3.0 {
        return;
    }
    painter.text(
        card_x + 8.0,
        card_y + relative_y,
        9.0,
        text,
        color,
        Some(RectF::new(
            card_x + 8.0,
            card_y + relative_y - 2.0,
            card_width - 16.0,
            14.0,
        )),
    );
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
        for source in chinese_font_candidates() {
            if let Some(font) = load_font(&source) {
                return Self {
                    font: Some(font),
                    name: source.name,
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

fn load_font(source: &super::system_font::ChineseFontSource) -> Option<Font> {
    let bytes = fs::read(&source.path).ok()?;
    Font::from_bytes(
        bytes,
        FontSettings {
            collection_index: source.collection_index,
            ..FontSettings::default()
        },
    )
    .ok()
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
            magic_strength: 70,
            defense: 64,
            dexterity: 55,
            physical_resistance: 2,
            poison_resistance: 3,
            sorcery_resistance: 4,
            elemental_resistance: [1, 2, 3, 4, 5],
            magic_name: Some("一阳指".to_owned()),
            magic_rate: 3,
            dual_move: true,
            steal: StealView::Item {
                name: "蜂王蜜".to_owned(),
                count: 2,
            },
            collect_value: 5,
            statuses: Vec::new(),
            poison_count: 0,
        }
    }

    fn snapshot(selected_enemy: Option<usize>) -> BattleAssistSnapshot {
        BattleAssistSnapshot {
            scale_factor: 2.0,
            surface_width: 1280,
            surface_height: 800,
            round: 3,
            selected_enemy,
            targeting_enemy: false,
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
            label: "昏睡",
            rounds: 2,
        }];
        enemy.poison_count = 1;

        assert_eq!(enemy.status_summary(), "中毒 · 昏睡2");
        assert_eq!(enemy.magic_summary(), "一阳指 · 30%");
        assert_eq!(enemy.trait_summary(), "双行动 · 炼化 5");
        assert_eq!(enemy.steal_summary(), "蜂王蜜 ×2");
    }

    #[test]
    fn responsive_layout_keeps_details_and_drops_overview_on_small_surfaces() {
        let regular = panel_layout(&snapshot(Some(0)));
        let mut small = snapshot(Some(0));
        small.surface_width = 640;
        small.surface_height = 400;
        let small = panel_layout(&small);

        assert!((regular.width - 281.6).abs() < f32::EPSILON);
        assert_eq!(regular.overview_height, 80.0);
        assert_eq!(regular.detail_height, DETAIL_HEIGHT);
        assert_eq!(regular.height, 286.0);
        assert_eq!(small.width, MIN_PANEL_WIDTH);
        assert_eq!(small.overview_height, 0.0);
        assert_eq!(small.detail_height, 134.0);
        assert_eq!(small.height, 176.0);
    }

    #[test]
    fn rasterized_panel_matches_its_physical_display_extent() {
        let mut font = test_font();
        let mut pixels = Vec::new();
        let frame = rasterize_snapshot(&mut font, &snapshot(Some(0)), &mut pixels);

        assert_eq!((frame.width, frame.display_width), (563, 563));
        assert_eq!((frame.height, frame.display_height), (572, 572));
        assert_eq!(
            pixels.len(),
            frame.width as usize * frame.height as usize * 4
        );
    }

    #[test]
    fn floating_panel_anchors_to_the_aspect_fitted_game_rect() {
        assert_eq!(
            game_surface_rect(1600, 800),
            SurfaceRect {
                x: 160,
                y: 0,
                width: 1280,
                height: 800,
            }
        );
        assert_eq!(
            game_surface_rect(640, 500),
            SurfaceRect {
                x: 0,
                y: 50,
                width: 640,
                height: 400,
            }
        );
    }

    #[test]
    fn unchanged_snapshot_skips_panel_rasterization() {
        let snapshot = snapshot(Some(0));
        let mut cached = None;

        assert!(replace_changed_snapshot(&mut cached, snapshot.clone()));
        assert!(!replace_changed_snapshot(&mut cached, snapshot.clone()));
        let mut targeting = snapshot;
        targeting.targeting_enemy = true;
        assert!(replace_changed_snapshot(&mut cached, targeting));
    }
}
