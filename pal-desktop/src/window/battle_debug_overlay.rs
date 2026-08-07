//! Physical-resolution battle diagnostics rendered above the scaled game framebuffer.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use fontdue::{Font, FontSettings};
use pal_core::battle::{BattleEvent, BattlePhase, BattleResult, BattleState, MagicEventPhase};
use pixels::wgpu;

use super::session::{BattleDebugHit, BattleDebugTarget};

const SURFACE_MARGIN: f32 = 12.0;
const MAX_PANEL_WIDTH: f32 = 520.0;

const TRANSPARENT: [u8; 4] = [0, 0, 0, 0];
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tone {
    Neutral,
    Accent,
    Good,
    Warning,
    Danger,
}

impl Tone {
    fn color(self) -> [u8; 4] {
        match self {
            Self::Neutral => TEXT,
            Self::Accent => ACCENT,
            Self::Good => GOOD,
            Self::Warning => WARNING,
            Self::Danger => DANGER,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EventView {
    title: String,
    detail: String,
    tone: Tone,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EnemyView {
    index: usize,
    object_id: u16,
    enemy_id: u16,
    hp: u16,
    hp_signed: i16,
    max_hp: u16,
    alive: bool,
    level: u16,
    attack_signed: i16,
    effective_attack: u16,
    defense_raw: u16,
    defense_signed: i16,
    effective_defense: u16,
    simulated_magic_defense: u16,
    physical_resistance: u16,
}

impl EnemyView {
    fn display_hp(&self) -> u16 {
        u16::try_from(self.hp_signed.max(0))
            .unwrap_or(0)
            .min(self.max_hp)
    }

    fn hp_summary(&self) -> String {
        if self.hp > self.max_hp || self.hp_signed < 0 {
            format!(
                "HP {}/{} · raw {} ({})",
                self.display_hp(),
                self.max_hp,
                self.hp,
                self.hp_signed
            )
        } else {
            format!("HP {}/{}", self.display_hp(), self.max_hp)
        }
    }

    fn defense_summary(&self) -> String {
        if self.effective_defense == self.simulated_magic_defense {
            format!("防 {}→{}", self.defense_signed, self.effective_defense)
        } else {
            format!(
                "防 {}→{} / 模 {}",
                self.defense_signed, self.effective_defense, self.simulated_magic_defense
            )
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlayerView {
    index: usize,
    hp: u16,
    max_hp: u16,
    attack: u16,
    magic: u16,
    defense: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct BattleDebugSnapshot {
    scale_factor: f64,
    surface_width: u32,
    surface_height: u32,
    enemy_team: u16,
    round: u32,
    phase: &'static str,
    active_player: Option<usize>,
    current_event: EventView,
    last_hit: EventView,
    enemies: Vec<EnemyView>,
    players: Vec<PlayerView>,
}

impl BattleDebugSnapshot {
    pub(super) fn capture(
        battle: &BattleState,
        current_event: Option<BattleEvent>,
        last_hit: Option<BattleDebugHit>,
        scale_factor: f64,
        surface_width: u32,
        surface_height: u32,
    ) -> Self {
        let enemies = battle
            .enemies
            .iter()
            .enumerate()
            .map(|(index, enemy)| EnemyView {
                index,
                object_id: enemy.object_id,
                enemy_id: enemy.enemy_id,
                hp: enemy.hp,
                hp_signed: enemy.hp as i16,
                max_hp: enemy.max_hp,
                alive: enemy.is_alive(),
                level: enemy.level,
                attack_signed: enemy.attack_strength as i16,
                effective_attack: enemy.effective_attack_strength(),
                defense_raw: enemy.defense,
                defense_signed: enemy.defense as i16,
                effective_defense: enemy.effective_defense(),
                simulated_magic_defense: enemy.simulated_magic_defense(),
                physical_resistance: enemy.physical_resistance,
            })
            .collect::<Vec<_>>();
        let players = battle
            .players
            .iter()
            .enumerate()
            .map(|(index, player)| PlayerView {
                index,
                hp: player.hp,
                max_hp: player.max_hp,
                attack: player.attack_strength,
                magic: player.magic_strength,
                defense: player.defense,
            })
            .collect();
        let phase = match battle.phase() {
            BattlePhase::AwaitingCommand => "进行中",
            BattlePhase::Finished(BattleResult::Won) => "胜利",
            BattlePhase::Finished(BattleResult::Lost) => "失败",
            BattlePhase::Finished(BattleResult::Fled) => "已逃离",
            BattlePhase::Finished(BattleResult::Terminated) => "已终止",
        };
        Self {
            scale_factor,
            surface_width,
            surface_height,
            enemy_team: battle.enemy_team,
            round: battle.round(),
            phase,
            active_player: battle.active_player(),
            current_event: current_event.map_or_else(no_current_event, current_event_view),
            last_hit: last_hit.map_or_else(no_last_hit, last_hit_view),
            enemies,
            players,
        }
    }
}

pub(super) struct BattleDebugOverlay {
    bind_group_layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    texture: wgpu::Texture,
    width: u32,
    height: u32,
    scale_factor: f64,
    font: UiFont,
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
            width: 1,
            height: 1,
            scale_factor,
            font,
        }
    }

    pub(super) fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        snapshot: BattleDebugSnapshot,
    ) {
        let frame = rasterize_snapshot(&mut self.font, &snapshot);
        if self.width != frame.width || self.height != frame.height {
            let (texture, bind_group) =
                create_texture(device, &self.bind_group_layout, frame.width, frame.height);
            self.texture = texture;
            self.bind_group = bind_group;
            self.width = frame.width;
            self.height = frame.height;
        }
        self.scale_factor = snapshot.scale_factor;
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &frame.pixels,
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
        let margin = physical(SURFACE_MARGIN, self.scale_factor);
        let width = self
            .width
            .min(surface_width.saturating_sub(margin.saturating_mul(2)));
        let height = self
            .height
            .min(surface_height.saturating_sub(margin.saturating_mul(2)));
        if width == 0 || height == 0 {
            return;
        }
        let x = right_aligned_x(surface_width, margin, width);
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
        pass.set_viewport(
            x as f32,
            margin as f32,
            width as f32,
            height as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(x, margin, width, height);
        pass.draw(0..4, 0..1);
    }
}

fn right_aligned_x(surface_width: u32, margin: u32, panel_width: u32) -> u32 {
    surface_width
        .saturating_sub(margin)
        .saturating_sub(panel_width)
}

struct RasterFrame {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
}

fn rasterize_snapshot(font: &mut UiFont, snapshot: &BattleDebugSnapshot) -> RasterFrame {
    const HEADER_HEIGHT: f32 = 42.0;
    const EVENTS_HEIGHT: f32 = 76.0;
    const SECTION_LABEL_HEIGHT: f32 = 16.0;
    const ENEMY_STEP: f32 = 44.0;
    const PLAYER_STEP: f32 = 27.0;

    let scale = normalized_scale(snapshot.scale_factor);
    let logical_surface_width = snapshot.surface_width as f32 / scale;
    let logical_surface_height = snapshot.surface_height as f32 / scale;
    let panel_width =
        MAX_PANEL_WIDTH.min((logical_surface_width - SURFACE_MARGIN * 2.0).max(280.0));
    let enemy_height = if snapshot.enemies.is_empty() {
        0.0
    } else {
        SECTION_LABEL_HEIGHT + snapshot.enemies.len() as f32 * ENEMY_STEP
    };
    let player_height = if snapshot.players.is_empty() {
        0.0
    } else {
        SECTION_LABEL_HEIGHT + snapshot.players.len() as f32 * PLAYER_STEP
    };
    let panel_height = (HEADER_HEIGHT + EVENTS_HEIGHT + enemy_height + player_height + 8.0)
        .min((logical_surface_height - SURFACE_MARGIN * 2.0).max(140.0));
    let width = physical(panel_width, snapshot.scale_factor);
    let height = physical(panel_height, snapshot.scale_factor);
    let mut pixels = vec![0; width as usize * height as usize * 4];
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.copy_from_slice(&TRANSPARENT);
    }
    let mut painter = Painter {
        pixels: &mut pixels,
        width,
        height,
        scale,
        font,
    };
    painter.rounded_rect(0.0, 0.0, panel_width, panel_height, 10.0, PANEL_BORDER);
    painter.rounded_rect(1.0, 1.0, panel_width - 2.0, panel_height - 2.0, 9.0, PANEL);

    let action = snapshot
        .active_player
        .map_or_else(|| "敌方行动".to_owned(), |player| format!("P{player} 选令"));
    painter.text(14.0, 9.0, 16.0, "战斗调试", ACCENT, None);
    painter.text(
        102.0,
        12.0,
        10.5,
        &format!(
            "队 {} · 回合 {} · {}",
            snapshot.enemy_team, snapshot.round, snapshot.phase
        ),
        TEXT,
        Some(RectF::new(102.0, 7.0, panel_width - 202.0, 27.0)),
    );
    painter.badge(panel_width - 92.0, 9.0, 80.0, 20.0, &action, Tone::Accent);

    let content_x = 12.0;
    let content_width = panel_width - 24.0;
    draw_event_card(
        &mut painter,
        content_x,
        HEADER_HEIGHT,
        content_width,
        "当前",
        &snapshot.current_event,
    );
    draw_event_card(
        &mut painter,
        content_x,
        HEADER_HEIGHT + 38.0,
        content_width,
        "最近",
        &snapshot.last_hit,
    );
    let enemies_y = HEADER_HEIGHT + EVENTS_HEIGHT;
    let players_y = draw_enemies(
        &mut painter,
        &snapshot.enemies,
        content_x,
        enemies_y,
        content_width,
        panel_height,
    );
    draw_players(
        &mut painter,
        &snapshot.players,
        content_x,
        players_y,
        content_width,
        panel_height,
    );

    RasterFrame {
        pixels,
        width,
        height,
    }
}

fn draw_event_card(
    painter: &mut Painter<'_>,
    x: f32,
    y: f32,
    width: f32,
    label: &str,
    event: &EventView,
) {
    painter.rounded_rect(x, y, width, 34.0, 6.0, CARD_BORDER);
    painter.rounded_rect(x + 1.0, y + 1.0, width - 2.0, 32.0, 5.0, CARD);
    painter.text(x + 8.0, y + 5.0, 9.0, label, MUTED, None);
    painter.text(
        x + 48.0,
        y + 4.0,
        10.5,
        &event.title,
        event.tone.color(),
        Some(RectF::new(x + 48.0, y + 1.0, width - 56.0, 16.0)),
    );
    painter.text(
        x + 8.0,
        y + 19.0,
        9.2,
        &event.detail,
        TEXT,
        Some(RectF::new(x + 8.0, y + 16.0, width - 16.0, 16.0)),
    );
}

fn draw_enemies(
    painter: &mut Painter<'_>,
    enemies: &[EnemyView],
    x: f32,
    y: f32,
    width: f32,
    panel_height: f32,
) -> f32 {
    if enemies.is_empty() {
        return y;
    }
    painter.text(x + 2.0, y + 1.0, 10.5, "敌人", MUTED, None);
    let mut card_y = y + 16.0;
    for enemy in enemies {
        if card_y + 40.0 > panel_height - 7.0 {
            break;
        }
        painter.rounded_rect(x, card_y, width, 40.0, 6.0, CARD_BORDER);
        painter.rounded_rect(x + 1.0, card_y + 1.0, width - 2.0, 38.0, 5.0, CARD);
        let status = if enemy.alive { "存活" } else { "倒下" };
        let status_color = if enemy.alive { GOOD } else { DANGER };
        painter.text(
            x + 8.0,
            card_y + 4.0,
            10.5,
            &format!(
                "E{} · 对象 {} / 定义 {} · Lv{}",
                enemy.index, enemy.object_id, enemy.enemy_id, enemy.level
            ),
            ACCENT,
            Some(RectF::new(x + 8.0, card_y + 1.0, width - 58.0, 17.0)),
        );
        painter.text(
            x + width - 42.0,
            card_y + 4.0,
            9.5,
            status,
            status_color,
            None,
        );
        painter.text(
            x + 8.0,
            card_y + 21.0,
            9.4,
            &format!(
                "{} · 攻 {}→{} · {} · 物抗 {}",
                enemy.hp_summary(),
                enemy.attack_signed,
                enemy.effective_attack,
                enemy.defense_summary(),
                enemy.physical_resistance
            ),
            if enemy.alive { TEXT } else { DANGER },
            Some(RectF::new(x + 8.0, card_y + 18.0, width - 108.0, 18.0)),
        );
        painter.bar(
            x + width - 92.0,
            card_y + 27.0,
            78.0,
            4.0,
            enemy.display_hp(),
            enemy.max_hp,
            status_color,
        );
        card_y += 44.0;
    }
    card_y + 2.0
}

fn draw_players(
    painter: &mut Painter<'_>,
    players: &[PlayerView],
    x: f32,
    y: f32,
    width: f32,
    panel_height: f32,
) {
    if players.is_empty() || y + 16.0 > panel_height - 7.0 {
        return;
    }
    painter.text(x + 2.0, y + 1.0, 10.5, "我方", MUTED, None);
    let mut row_y = y + 16.0;
    for player in players {
        if row_y + 23.0 > panel_height - 7.0 {
            break;
        }
        painter.rounded_rect(x, row_y, width, 23.0, 5.0, CARD);
        painter.text(
            x + 8.0,
            row_y + 5.0,
            9.6,
            &format!(
                "P{} · HP {}/{} · 攻 {}  灵 {}  防 {}",
                player.index, player.hp, player.max_hp, player.attack, player.magic, player.defense
            ),
            TEXT,
            Some(RectF::new(x + 8.0, row_y + 2.0, width - 16.0, 18.0)),
        );
        row_y += 27.0;
    }
}

fn no_current_event() -> EventView {
    EventView {
        title: "等待下一项行动".to_owned(),
        detail: "当前没有正在播放的战斗反馈".to_owned(),
        tone: Tone::Neutral,
    }
}

fn no_last_hit() -> EventView {
    EventView {
        title: "尚无伤害记录".to_owned(),
        detail: "造成伤害后会保留最近一次结算".to_owned(),
        tone: Tone::Neutral,
    }
}

fn current_event_view(event: BattleEvent) -> EventView {
    match event {
        BattleEvent::PlayerAttack {
            player,
            enemy,
            damage,
            critical,
            defeated,
            ..
        } => EventView {
            title: format!("我方 P{player} 普通攻击 → 敌人 E{enemy}"),
            detail: format!(
                "伤害 {damage}{} · {}",
                if critical { " · 暴击" } else { "" },
                defeated_label(defeated)
            ),
            tone: defeated_tone(defeated),
        },
        BattleEvent::PlayerMagic {
            player,
            enemy,
            magic_object,
            damage,
            phase,
            defeated,
            ..
        } => EventView {
            title: format!("我方 P{player} 施放仙术 {magic_object} → 敌人 E{enemy}"),
            detail: format!(
                "{} · 伤害 {damage} · {}",
                magic_phase_label(phase),
                defeated_label(defeated)
            ),
            tone: defeated_tone(defeated),
        },
        BattleEvent::EnemyAttack {
            enemy,
            player,
            damage,
            protected_by,
            auto_defended,
            defeated,
        } => EventView {
            title: format!("敌人 E{enemy} 普通攻击 → 我方 P{player}"),
            detail: format!(
                "伤害 {damage}{}{} · {}",
                if auto_defended {
                    " · 自动防御"
                } else {
                    ""
                },
                protected_by.map_or_else(String::new, |cover| format!(" · P{cover} 援护")),
                defeated_label(defeated)
            ),
            tone: if auto_defended {
                Tone::Good
            } else {
                defeated_tone(defeated)
            },
        },
        BattleEvent::EnemyMagic {
            enemy,
            player,
            magic_object,
            damage,
            phase,
            auto_defended,
            defeated,
            ..
        } => EventView {
            title: format!("敌人 E{enemy} 施放仙术 {magic_object} → 我方 P{player}"),
            detail: format!(
                "{} · 伤害 {damage}{} · {}",
                magic_phase_label(phase),
                if auto_defended {
                    " · 自动防御"
                } else {
                    ""
                },
                defeated_label(defeated)
            ),
            tone: defeated_tone(defeated),
        },
        BattleEvent::EnemyConfusedAttack {
            enemy,
            target,
            damage,
            defeated,
        } => EventView {
            title: format!("混乱敌人 E{enemy} 攻击同伴 E{target}"),
            detail: format!("伤害 {damage} · {}", defeated_label(defeated)),
            tone: Tone::Warning,
        },
        BattleEvent::PlayerConfusedAttack {
            player,
            target,
            damage,
            defeated,
        } => EventView {
            title: format!("混乱队员 P{player} 攻击同伴 P{target}"),
            detail: format!("伤害 {damage} · {}", defeated_label(defeated)),
            tone: Tone::Warning,
        },
        BattleEvent::SimulatedMagic {
            enemy,
            magic,
            damage,
            defeated,
            ..
        } => EventView {
            title: format!("脚本模拟仙术 {} → 敌人 E{enemy}", magic.object_id),
            detail: format!(
                "伤害 raw {damage} / i16 {} · {}",
                damage as i16,
                defeated_label(defeated)
            ),
            tone: if damage as i16 <= 0 {
                Tone::Warning
            } else {
                defeated_tone(defeated)
            },
        },
        BattleEvent::PlayerUseItem {
            player,
            item_object,
            target,
            ..
        } => EventView {
            title: format!("我方 P{player} 使用物品 {item_object}"),
            detail: target.map_or_else(
                || "作用目标：全体队员".to_owned(),
                |target| format!("作用目标：我方 P{target}"),
            ),
            tone: Tone::Accent,
        },
        BattleEvent::PlayerThrowItem {
            player,
            item_object,
            target,
        } => EventView {
            title: format!("我方 P{player} 投掷物品 {item_object}"),
            detail: target.map_or_else(
                || "作用目标：全体敌人".to_owned(),
                |target| format!("作用目标：敌人 E{target}"),
            ),
            tone: Tone::Accent,
        },
        BattleEvent::PlayerItemFeedback {
            player,
            item_object,
            ..
        } => EventView {
            title: format!("物品 {item_object} 效果结算"),
            detail: format!("执行者：我方 P{player}"),
            tone: Tone::Accent,
        },
        BattleEvent::PlayerFlee { player, succeeded } => EventView {
            title: format!("我方 P{player} 尝试逃跑"),
            detail: if succeeded {
                "逃跑成功"
            } else {
                "逃跑失败"
            }
            .to_owned(),
            tone: if succeeded { Tone::Good } else { Tone::Warning },
        },
        BattleEvent::PlayerDefend { player } => EventView {
            title: format!("我方 P{player} 防御"),
            detail: "本回合物理防御提高".to_owned(),
            tone: Tone::Good,
        },
        BattleEvent::PlayerDefensiveMagic {
            player,
            magic_object,
            ..
        } => EventView {
            title: format!("我方 P{player} 施放辅助仙术 {magic_object}"),
            detail: "辅助效果正在结算".to_owned(),
            tone: Tone::Good,
        },
        BattleEvent::PlayerCooperativeMagic {
            player,
            enemy,
            magic_object,
            damage,
            defeated,
            ..
        } => EventView {
            title: format!("我方 P{player} 合体仙术 {magic_object} → 敌人 E{enemy}"),
            detail: format!("伤害 {damage} · {}", defeated_label(defeated)),
            tone: defeated_tone(defeated),
        },
        BattleEvent::PlayerMagicAnimation { player } => EventView {
            title: "脚本施法动画".to_owned(),
            detail: player.map_or_else(
                || "全队动画".to_owned(),
                |player| format!("动画角色：我方 P{player}"),
            ),
            tone: Tone::Accent,
        },
        BattleEvent::PlayerFriendDeath { player } => EventView {
            title: format!("我方 P{player} 响应队友倒下"),
            detail: "正在执行队友死亡脚本".to_owned(),
            tone: Tone::Danger,
        },
        BattleEvent::PlayerDying { player } => EventView {
            title: format!("我方 P{player} 进入濒死状态"),
            detail: "正在执行濒死脚本".to_owned(),
            tone: Tone::Danger,
        },
        BattleEvent::EnemyDivide { .. } => EventView {
            title: "敌人分裂".to_owned(),
            detail: "正在生成分裂后的敌人".to_owned(),
            tone: Tone::Warning,
        },
        BattleEvent::EnemySummon {
            caster,
            summoned_mask,
        } => EventView {
            title: format!("敌人 E{caster} 召唤同伴"),
            detail: format!("新敌人槽位掩码 0b{summoned_mask:05b}"),
            tone: Tone::Warning,
        },
        BattleEvent::EnemyTransform { enemy, .. } => EventView {
            title: format!("敌人 E{enemy} 变身"),
            detail: "敌人属性与外观已经更新".to_owned(),
            tone: Tone::Warning,
        },
        BattleEvent::EnemyEscape => EventView {
            title: "敌人逃跑".to_owned(),
            detail: "正在播放敌方退场反馈".to_owned(),
            tone: Tone::Neutral,
        },
        BattleEvent::RoundCompleted => EventView {
            title: "本回合结束".to_owned(),
            detail: "正在处理状态、毒与下一回合".to_owned(),
            tone: Tone::Neutral,
        },
        BattleEvent::Finished(result) => EventView {
            title: "战斗结束".to_owned(),
            detail: match result {
                BattleResult::Won => "胜利",
                BattleResult::Lost => "失败",
                BattleResult::Fled => "逃跑",
                BattleResult::Terminated => "脚本终止",
            }
            .to_owned(),
            tone: if result == BattleResult::Won {
                Tone::Good
            } else {
                Tone::Warning
            },
        },
    }
}

fn last_hit_view(hit: BattleDebugHit) -> EventView {
    let action = match hit.action {
        "P.ATTACK" => "我方普通攻击",
        "P.MAGIC" => "我方攻击仙术",
        "COOP" => "合体仙术",
        "SIM.MAGIC" => "脚本模拟仙术",
        "E.CONF" => "混乱敌人攻击",
        "E.ATTACK" => "敌方普通攻击",
        "E.MAGIC" => "敌方攻击仙术",
        "P.CONF" => "混乱队员攻击",
        "ITEM.TOTAL" => "整件投掷物品净变化",
        _ => "伤害结算",
    };
    let source = if hit.action.starts_with('P') || hit.action == "COOP" {
        format!("我方 P{}", hit.source)
    } else if hit.action.starts_with('E') {
        format!("敌人 E{}", hit.source)
    } else {
        "战斗脚本".to_owned()
    };
    let target = match hit.target {
        BattleDebugTarget::Enemy => format!("敌人 E{}", hit.target_index),
        BattleDebugTarget::Player => format!("我方 P{}", hit.target_index),
    };
    let before = hit
        .hp_before
        .map_or_else(|| "?".to_owned(), |hp| hp.to_string());
    let object = hit
        .object_id
        .map_or_else(String::new, |object| format!(" · 关联对象 {object}"));
    let signed = hit.damage as i16;
    EventView {
        title: format!("{action} · {source} → {target}"),
        detail: format!(
            "伤害 raw {} / i16 {} · 生命 {} → {}{} · {}",
            hit.damage,
            signed,
            before,
            hit.hp_after,
            object,
            defeated_label(hit.defeated)
        ),
        tone: if signed < 0 {
            Tone::Warning
        } else {
            defeated_tone(hit.defeated)
        },
    }
}

fn magic_phase_label(phase: MagicEventPhase) -> &'static str {
    match phase {
        MagicEventPhase::Visual => "动画阶段",
        MagicEventPhase::Feedback => "伤害阶段",
    }
}

fn defeated_label(defeated: bool) -> &'static str {
    if defeated {
        "目标倒下"
    } else {
        "目标存活"
    }
}

fn defeated_tone(defeated: bool) -> Tone {
    if defeated {
        Tone::Danger
    } else {
        Tone::Neutral
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

    fn badge(&mut self, x: f32, y: f32, width: f32, height: f32, text: &str, tone: Tone) {
        let mut background = tone.color();
        background[3] = 42;
        self.rounded_rect(x, y, width, height, height / 2.0, background);
        self.text(x + 9.0, y + 5.0, 10.5, text, tone.color(), None);
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

    #[test]
    fn automatic_defense_is_explained_in_simplified_chinese() {
        let event = current_event_view(BattleEvent::EnemyAttack {
            enemy: 0,
            player: 0,
            damage: 0,
            protected_by: None,
            auto_defended: true,
            defeated: false,
        });
        assert!(event.title.contains("敌人"));
        assert!(event.detail.contains("自动防御"));
        assert_eq!(event.tone, Tone::Good);
    }

    #[test]
    fn item_total_shows_classic_word_hp_after_lethal_damage() {
        let event = last_hit_view(BattleDebugHit {
            action: "ITEM.TOTAL",
            source: 0,
            object_id: Some(153),
            target: BattleDebugTarget::Enemy,
            target_index: 0,
            damage: 90,
            hp_before: Some(40),
            hp_after: 40u16.wrapping_sub(90),
            defeated: true,
        });
        assert!(event.title.contains("整件投掷物品"));
        assert!(event.detail.contains("raw 90 / i16 90"));
        assert!(event.detail.contains("40 → 65486"));
        assert_eq!(event.tone, Tone::Danger);
    }

    #[test]
    fn source_over_blending_preserves_opaque_text_on_a_translucent_panel() {
        let mut pixel = vec![0, 0, 0, 0];
        blend_pixel(&mut pixel, 1, 1, 0, 0, PANEL);
        blend_pixel(&mut pixel, 1, 1, 0, 0, TEXT);
        assert_eq!(pixel, TEXT);
    }

    #[test]
    fn default_window_keeps_the_fifth_enemy_card_visible() {
        let enemy = EnemyView {
            index: 0,
            object_id: 153,
            enemy_id: 42,
            hp: 40,
            hp_signed: 40,
            max_hp: 40,
            alive: true,
            level: 0,
            attack_signed: 8,
            effective_attack: 44,
            defense_raw: u16::MAX - 5,
            defense_signed: -6,
            effective_defense: 18,
            simulated_magic_defense: 18,
            physical_resistance: 0,
        };
        let mut defeated = enemy.clone();
        defeated.hp = 40u16.wrapping_sub(90);
        defeated.hp_signed = defeated.hp as i16;
        defeated.alive = false;
        assert_eq!(defeated.display_hp(), 0);
        let snapshot = BattleDebugSnapshot {
            scale_factor: 1.0,
            surface_width: 640,
            surface_height: 400,
            enemy_team: 18,
            round: 1,
            phase: "进行中",
            active_player: Some(0),
            current_event: no_current_event(),
            last_hit: no_last_hit(),
            enemies: vec![enemy; 5],
            players: Vec::new(),
        };
        let mut font = UiFont {
            font: None,
            name: "测试点阵".to_owned(),
            glyphs: HashMap::new(),
        };

        let frame = rasterize_snapshot(&mut font, &snapshot);

        assert_eq!((frame.width, frame.height), (520, 362));
        let panel_pixel = pixel(&frame, 5, 330);
        assert!((20..500).any(|x| pixel(&frame, x, 330) != panel_pixel));
        assert_eq!(right_aligned_x(640, 12, frame.width), 108);
    }

    fn pixel(frame: &RasterFrame, x: u32, y: u32) -> [u8; 4] {
        let index = (y as usize * frame.width as usize + x as usize) * 4;
        frame.pixels[index..index + 4].try_into().unwrap()
    }
}
