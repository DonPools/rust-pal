use pal_assets::battle::{BattleEffects, BattlePosition, BattleSpriteArchive};
use pal_assets::bitmap::Bitmap;
use pal_assets::player_roles::PlayerRole;
use pal_assets::rle::RleBitmap;
use pal_assets::text::{BitmapFont, TextLibrary};
use pal_core::battle::{
    BattleEvent, BattleMagic, BattleMagicVisual, BattlePhase, BattleResult, BattleState,
    BattleStatus, BattleTarget, MagicEventPhase,
};
use pal_core::game::BATTLE_FRAME_MS;

use super::battle_timing::{
    battle_magic_for_event, enemy_attack_frames, enemy_escape_offset, enemy_escape_timeline,
    enemy_magic_pre_frames, event_has_full_magic_visual, kept_effect_frame, magic_event_timeline,
    offensive_effect_frame_at, original_frames_to_ticks, player_attack_ticks, timed_frame_at,
    timed_frames_to_ticks, EnemyEscapeTimeline, MagicEventTimeline, BATTLE_FADE_TICKS,
};
use super::battle_update::{
    ACTION_EVENT_TICKS, ENEMY_DIVIDE_EVENT_TICKS, ENEMY_TRANSFORM_EVENT_TICKS,
    FLEE_FAILURE_EVENT_TICKS, FLEE_SUCCESS_EVENT_TICKS, PLAYER_MAGIC_ANIMATION_EVENT_TICKS,
};
use super::draw::draw_number;
use super::menu_render::{
    draw_cursor, draw_single_line_box, draw_slash, draw_ui_box_with_shadow, selected_color,
};
use crate::renderer::Renderer;

use super::UI_TIME_QUANTUM_MS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattlePendingCommand {
    Attack,
    Magic(usize),
    CooperativeMagic,
    UseItem(u16),
    ThrowItem(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattleMenuState {
    Main,
    Magic {
        selected: usize,
    },
    Misc {
        selected: usize,
    },
    ItemSubmenu {
        selected: usize,
    },
    TargetEnemy {
        command: BattlePendingCommand,
    },
    TargetPlayer {
        command: BattlePendingCommand,
        selected: usize,
    },
    Status {
        selected: usize,
    },
}

const BATTLE_COMMAND_ICONS: [(usize, i32, i32); 4] =
    [(40, 27, 140), (41, 0, 155), (42, 54, 155), (43, 27, 170)];

pub struct BattleRenderResources<'a> {
    pub enemy_sprites: &'a BattleSpriteArchive,
    pub player_sprites: &'a BattleSpriteArchive,
    pub magic_effect_sprites: &'a BattleSpriteArchive,
    pub battle_effects: &'a BattleEffects,
    pub backgrounds: &'a [Option<Bitmap>],
    pub text: &'a TextLibrary,
    pub font: &'a BitmapFont,
    pub ui_sprites: &'a [RleBitmap],
    pub cash: u32,
}

#[derive(Clone, Copy)]
pub struct BattleRenderState<'a> {
    pub selected_enemy: usize,
    pub selected_command: usize,
    pub targeting_enemy: bool,
    pub menu: BattleMenuState,
    pub auto_attack: bool,
    pub ticks: u64,
    pub event: Option<BattleEvent>,
    pub event_ticks: u16,
    pub kept_effects: &'a [BattleEvent],
}

#[derive(Clone)]
pub(super) enum BattleSettlementPage {
    LevelUp {
        before: Box<PlayerRole>,
        after: Box<PlayerRole>,
    },
    AttributeGrowth {
        role_id: u16,
        label: usize,
        amount: u16,
    },
    LearnedMagic {
        role_id: u16,
        magic_object: u16,
    },
}

pub(super) struct PostBattlePresentation {
    pub(super) battle: BattleState,
    pub(super) pages: Vec<BattleSettlementPage>,
    pub(super) page: usize,
    pub(super) ticks_remaining: u16,
}

impl PostBattlePresentation {
    pub(super) fn current_page(&self) -> Option<&BattleSettlementPage> {
        self.pages.get(self.page)
    }
}

fn battle_animation_ticks(ui_ticks: u64) -> u64 {
    ui_ticks.saturating_mul(UI_TIME_QUANTUM_MS) / BATTLE_FRAME_MS
}

pub fn render_battle(
    renderer: &mut Renderer,
    battle: &BattleState,
    resources: BattleRenderResources<'_>,
    state: BattleRenderState<'_>,
) {
    let BattleRenderState {
        selected_enemy,
        selected_command,
        targeting_enemy,
        menu,
        auto_attack,
        ticks,
        event,
        event_ticks,
        kept_effects,
    } = state;
    let battle_ticks = battle_animation_ticks(ticks);
    if let Some(background) = resources
        .backgrounds
        .get(usize::from(battle.battlefield))
        .and_then(Option::as_ref)
    {
        renderer.blit_bitmap(background, 0, 0);
    } else {
        renderer.clear_black();
    }

    for &kept_effect in kept_effects {
        render_kept_magic_effect(
            renderer,
            battle,
            resources.magic_effect_sprites,
            kept_effect,
        );
    }
    let magic_timing = event.and_then(|event| {
        magic_render_timeline(
            battle,
            resources.magic_effect_sprites,
            resources.player_sprites,
            event,
            event_ticks,
        )
    });
    let feedback_active =
        magic_timing.is_none_or(|(_, timeline, elapsed)| elapsed >= timeline.tail_start());
    let enemy_escape_timing = matches!(event, Some(BattleEvent::EnemyEscape)).then(|| {
        let rightmost_edge = battle
            .enemies
            .iter()
            .filter(|enemy| enemy.is_alive())
            .filter_map(|enemy| {
                resources
                    .enemy_sprites
                    .decode_frame(usize::from(enemy.enemy_id), 0)
                    .map(|frame| i32::from(enemy.position.x) + i32::from(frame.width))
            })
            .max()
            .unwrap_or(0);
        enemy_escape_timeline(rightmost_edge)
    });

    if let Some(event) = event {
        render_magic_effect(
            renderer,
            battle,
            resources.magic_effect_sprites,
            resources.player_sprites,
            event,
            event_ticks,
            true,
        );
    }

    for (index, enemy) in battle.enemies.iter().enumerate() {
        let is_defeat_event = matches!(
            event,
            Some(
                BattleEvent::PlayerAttack {
                    enemy: target,
                    defeated: true,
                    ..
                } | BattleEvent::PlayerMagic {
                    enemy: target,
                    defeated: true,
                    ..
                } | BattleEvent::SimulatedMagic {
                    enemy: target,
                    defeated: true,
                    ..
                } | BattleEvent::PlayerCooperativeMagic {
                    enemy: target,
                    defeated: true,
                    ..
                } | BattleEvent::EnemyConfusedAttack {
                    target,
                    defeated: true,
                    ..
                }
            ) if target == index
        );
        if !enemy.is_alive() && !is_defeat_event {
            continue;
        }
        let idle_frames = usize::from(enemy.idle_frames.max(1));
        let speed = u64::from(enemy.idle_animation_speed.max(1));
        let actor_state = event.and_then(|event| {
            enemy_actor_animation_state(battle, event, index, event_ticks, magic_timing)
        });
        let transform_sprite =
            event.and_then(|event| enemy_transform_sprite_state(event, index, event_ticks));
        let frame = transform_sprite.map_or_else(
            || {
                actor_state.map_or_else(
                    || usize::try_from(battle_ticks / speed).unwrap_or(0) % idle_frames,
                    |state| state.2,
                )
            },
            |state| state.2,
        );
        let enemy_id = transform_sprite.map_or(enemy.enemy_id, |state| state.0);
        let y_offset = transform_sprite.map_or(enemy.y_offset, |state| state.1);
        let Some(bitmap) = resources
            .enemy_sprites
            .decode_frame(usize::from(enemy_id), frame)
        else {
            continue;
        };
        let (enemy_action_x, enemy_action_y) = actor_state
            .map(|state| (state.0, state.1))
            .unwrap_or((0, 0));
        let enemy_blow_offset = match event {
            Some(
                BattleEvent::PlayerMagic { blow, .. } | BattleEvent::SimulatedMagic { blow, .. },
            ) => magic_blow_offset(blow, event_ticks),
            _ => 0,
        };
        let (feedback_x, feedback_y, exact_flash) = event
            .map(|event| enemy_feedback_state(event, index, event_ticks, magic_timing))
            .unwrap_or((0, 0, false));
        let (script_x, script_y, script_visibility, script_color_shift) = event
            .map(|event| {
                enemy_script_animation_state(battle, event, index, event_ticks, enemy_escape_timing)
            })
            .unwrap_or((0, 0, 64, 0));
        let enemy_offset = enemy_blow_offset;
        let x = i32::from(enemy.position.x) - i32::from(bitmap.width) / 2
            + enemy_offset
            + enemy_action_x
            + feedback_x
            + script_x;
        let y = i32::from(enemy.position.y) + i32::from(y_offset) - i32::from(bitmap.height)
            + enemy_offset / 2
            + enemy_action_y
            + feedback_y
            + script_y;
        let is_hit = matches!(
            event,
            Some(
                BattleEvent::PlayerAttack { enemy, .. }
                    | BattleEvent::PlayerMagic { enemy, .. }
                    | BattleEvent::SimulatedMagic { enemy, .. }
                    | BattleEvent::PlayerCooperativeMagic { enemy, .. }
                    | BattleEvent::EnemyConfusedAttack { target: enemy, .. }
            ) if enemy == index
        );
        let exact_attack_feedback = matches!(
            event,
            Some(BattleEvent::PlayerAttack {
                enemy: target,
                visual: true,
                ..
            }) if target == index
        );
        let fade_visibility = event
            .filter(|_| is_defeat_event)
            .map(|_| {
                magic_timing.map_or_else(
                    || death_fade_visibility(event_ticks),
                    |(_, timeline, elapsed)| {
                        let fade_end = timeline
                            .total_ticks
                            .saturating_sub(timeline.summon_fade_out_ticks);
                        let fade_start = fade_end.saturating_sub(timeline.death_fade_ticks);
                        if elapsed < fade_start {
                            64
                        } else if elapsed >= fade_end {
                            0
                        } else {
                            u8::try_from(
                                u32::from(fade_end - elapsed)
                                    .saturating_mul(64)
                                    .checked_div(u32::from(timeline.death_fade_ticks.max(1)))
                                    .unwrap_or(0),
                            )
                            .unwrap_or(64)
                            .min(64)
                        }
                    },
                )
            })
            .unwrap_or(64)
            .min(script_visibility);
        if fade_visibility < 64 {
            renderer.blit_rle_dithered(&bitmap, x, y, fade_visibility);
        } else if script_color_shift != 0 {
            renderer.blit_rle_color_shift(&bitmap, x, y, script_color_shift);
        } else if event.is_none()
            && targeting_enemy
            && index == selected_enemy
            && battle.phase() == BattlePhase::AwaitingCommand
            && battle_ticks & 1 != 0
        {
            renderer.blit_rle_color_shift(&bitmap, x, y, 7);
        } else if is_hit
            && feedback_active
            && (if exact_attack_feedback {
                exact_flash
            } else {
                exact_flash || event_ticks.is_multiple_of(2)
            })
        {
            renderer.blit_rle_color_shift(&bitmap, x, y, 6);
        } else {
            renderer.blit_rle(&bitmap, x, y);
        }
    }

    for (index, player) in battle.players.iter().enumerate() {
        if battle.hiding_time() != 0 {
            continue;
        }
        let (mut x, mut y) = player_position(battle.players.len(), index);
        let attack_state = event
            .and_then(|event| player_attack_animation_state(battle, event, index, event_ticks));
        let (magic_x, magic_y, magic_frame, mut color_shift) =
            player_magic_animation_state(event, index, event_ticks);
        let (item_x, item_y, item_frame, item_color_shift) =
            player_item_animation_state(event, index, event_ticks);
        color_shift = color_shift.max(item_color_shift);
        if let (Some(event), Some((magic, timeline, elapsed))) = (event, magic_timing) {
            if magic.magic_type == 9
                && elapsed >= timeline.pre_ticks
                && elapsed < timeline.effect_start()
            {
                let brighten_elapsed = elapsed.saturating_sub(timeline.pre_ticks);
                color_shift = color_shift.max(if brighten_elapsed < timeline.brighten_ticks {
                    i16::try_from(
                        u32::from(brighten_elapsed)
                            .saturating_mul(10)
                            .checked_div(u32::from(timeline.brighten_ticks.max(1)))
                            .unwrap_or(0)
                            .saturating_add(1),
                    )
                    .unwrap_or(10)
                    .min(10)
                } else {
                    10
                });
            }
            if let BattleEvent::PlayerDefensiveMagic { target, .. } = event {
                if elapsed >= timeline.tail_start() && defensive_target_includes(target, index) {
                    let phase = usize::from(elapsed.saturating_sub(timeline.tail_start()))
                        .saturating_mul(13)
                        / usize::from(timeline.tail_ticks.max(1));
                    let shift = if phase < 6 {
                        phase
                    } else {
                        12usize.saturating_sub(phase)
                    };
                    color_shift = color_shift.max(i16::try_from(shift).unwrap_or(0));
                }
            }
        }
        x += magic_x + item_x;
        y += magic_y + item_y;
        if let Some((attack_x, attack_y, _)) = attack_state {
            x = attack_x;
            y = attack_y;
        }
        if let Some(BattleEvent::EnemyMagic { blow, .. }) = event {
            let offset = magic_blow_offset(blow, event_ticks);
            x += offset;
            y += offset / 2;
        }
        if let Some(event) = event {
            let (feedback_x, feedback_y) =
                player_feedback_offset(battle, event, index, event_ticks, magic_timing);
            x += feedback_x;
            y += feedback_y;
        }
        match event {
            Some(BattleEvent::PlayerAttack { .. }) if attack_state.is_some() => {}
            Some(BattleEvent::PlayerConfusedAttack { player, .. }) if player == index => {
                let offset = action_offset(event_ticks);
                x -= offset;
                y -= offset / 2;
            }
            Some(BattleEvent::PlayerMagic {
                player,
                phase: MagicEventPhase::Visual,
                ..
            }) if player == index => {
                if let Some((_, _, elapsed)) = magic_timing {
                    let (offset_x, offset_y) = pre_magic_offset(elapsed);
                    x += offset_x;
                    y += offset_y;
                }
            }
            Some(BattleEvent::PlayerDefensiveMagic { player, .. }) if player == index => {
                if let Some((_, _, elapsed)) = magic_timing {
                    let (offset_x, offset_y) = pre_magic_offset(elapsed);
                    x += offset_x;
                    y += offset_y;
                }
            }
            Some(BattleEvent::EnemyAttack {
                protected_by: Some(cover),
                ..
            }) if cover == index => {}
            Some(BattleEvent::PlayerFlee {
                player: fleeing,
                succeeded,
            }) if succeeded || fleeing == index => {
                let duration = if succeeded {
                    FLEE_SUCCESS_EVENT_TICKS
                } else {
                    FLEE_FAILURE_EVENT_TICKS
                };
                let elapsed = duration.saturating_sub(event_ticks.min(duration));
                let movement = if succeeded { elapsed } else { elapsed.min(3) };
                x += i32::from(movement) * if index == 2 { 6 } else { 4 };
                y += i32::from(movement)
                    * if index == 0 && battle.players.len() > 1 {
                        6
                    } else {
                        4
                    };
            }
            Some(BattleEvent::PlayerCooperativeMagic { visual: true, .. })
                if cooperative_contributor(player) =>
            {
                const POSITIONS: [(i32, i32); 3] = [(208, 157), (234, 170), (260, 183)];
                let target = POSITIONS[index.min(POSITIONS.len() - 1)];
                let elapsed = magic_timing.map_or(0, |(_, _, elapsed)| elapsed);
                let progress = original_frame_for_tick(elapsed).saturating_add(1).min(6);
                x += (target.0 - x) * i32::from(progress) / 6;
                y += (target.1 - y) * i32::from(progress) / 6;
            }
            _ => {}
        }
        let sprite = usize::from(player.battle_sprite_num);
        let available = resources.player_sprites.frame_count(sprite).unwrap_or(0);
        let event_frame = match event {
            Some(BattleEvent::PlayerAttack { .. }) if attack_state.is_some() => {
                attack_state.map(|(_, _, frame)| frame)
            }
            Some(BattleEvent::EnemyAttack {
                player,
                protected_by,
                auto_defended,
                ..
            }) if protected_by == Some(index) || (player == index && auto_defended) => Some(3),
            Some(BattleEvent::EnemyAttack {
                player,
                auto_defended: false,
                ..
            }) if player == index => Some(4),
            Some(BattleEvent::EnemyMagic {
                player,
                phase: MagicEventPhase::Feedback,
                auto_defended: true,
                ..
            }) if player == index => Some(3),
            Some(BattleEvent::EnemyMagic {
                player,
                phase: MagicEventPhase::Feedback,
                ..
            }) if player == index => Some(4),
            Some(BattleEvent::PlayerMagic {
                player,
                phase: MagicEventPhase::Visual,
                ..
            }) if player == index => Some(
                magic_timing
                    .filter(|(_, timeline, elapsed)| *elapsed < timeline.effect_start())
                    .map_or(6, |_| 5),
            ),
            Some(BattleEvent::PlayerDefensiveMagic { player, .. }) if player == index => Some(
                magic_timing
                    .filter(|(_, timeline, elapsed)| *elapsed < timeline.effect_start())
                    .map_or(6, |_| 5),
            ),
            Some(BattleEvent::PlayerCooperativeMagic { visual: true, .. })
                if cooperative_contributor(player) =>
            {
                Some(
                    magic_timing
                        .filter(|(_, timeline, elapsed)| *elapsed < timeline.effect_start())
                        .map_or(6, |_| 5),
                )
            }
            Some(BattleEvent::PlayerUseItem { player, .. })
            | Some(BattleEvent::PlayerThrowItem { player, .. })
                if player == index =>
            {
                item_frame
            }
            Some(BattleEvent::PlayerFlee {
                player,
                succeeded: false,
            }) if player == index => Some(1),
            _ => None,
        };
        let frame = event_frame.or(magic_frame).unwrap_or_else(|| {
            if !player.is_alive() {
                2.min(available.saturating_sub(1))
            } else if player.defending {
                3.min(available.saturating_sub(1))
            } else if player.is_dying()
                || player
                    .statuses
                    .is_active(pal_core::battle::BattleStatus::Sleep)
            {
                1.min(available.saturating_sub(1))
            } else {
                0
            }
        });
        let frame = frame.min(available.saturating_sub(1));
        if let Some(bitmap) = resources.player_sprites.decode_frame(sprite, frame) {
            let left = x - i32::from(bitmap.width) / 2;
            let top = y - i32::from(bitmap.height);
            if color_shift == 0 {
                renderer.blit_rle(&bitmap, left, top);
            } else {
                renderer.blit_rle_color_shift(&bitmap, left, top, color_shift);
            }
            let is_hit = match event {
                Some(BattleEvent::EnemyAttack {
                    player,
                    auto_defended,
                    ..
                }) => player == index && !auto_defended,
                Some(BattleEvent::EnemyMagic {
                    player,
                    phase: MagicEventPhase::Feedback,
                    ..
                }) => player == index,
                Some(BattleEvent::PlayerConfusedAttack { target, .. }) => target == index,
                _ => false,
            };
            if is_hit && feedback_active && event_ticks.is_multiple_of(2) {
                renderer.blit_rle_color_shift(&bitmap, left, top, 6);
            }
        }
    }

    if let Some(event) = event {
        let _ = render_shared_battle_effect(
            renderer,
            battle,
            resources.battle_effects,
            resources.magic_effect_sprites,
            resources.player_sprites,
            event,
            event_ticks,
        );
    }

    if let Some(event) = event {
        render_magic_effect(
            renderer,
            battle,
            resources.magic_effect_sprites,
            resources.player_sprites,
            event,
            event_ticks,
            false,
        );
    }

    if let Some(event) = event {
        render_battle_action_label(renderer, event, event_ticks, resources.text, resources.font);
        if feedback_active {
            render_battle_event(
                renderer,
                battle,
                event,
                resources.ui_sprites,
                resources.text,
                resources.font,
            );
        }
    }

    if event.is_none() && battle.phase() == BattlePhase::AwaitingCommand {
        render_status(
            renderer,
            battle,
            resources.ui_sprites,
            selected_command,
            menu,
            auto_attack,
            resources.text,
            resources.font,
            ticks,
            battle_ticks,
            resources.cash,
        );
    }
    if event.is_none() {
        if let BattlePhase::Finished(result) = battle.phase() {
            render_settlement(
                renderer,
                battle,
                result,
                resources.ui_sprites,
                resources.text,
                resources.font,
            );
        }
    }
    if let Some((magic, timeline, elapsed)) =
        magic_timing.filter(|_| event.is_some_and(event_has_full_magic_visual))
    {
        let visual = magic.effect_visual();
        if elapsed >= timeline.effect_start() && elapsed < timeline.tail_start() && visual.wave != 0
        {
            renderer.apply_wave(visual.wave, i16::try_from(battle_ticks).unwrap_or(i16::MAX));
        }
        let shake_ticks = timed_frames_to_ticks(usize::from(visual.shake), visual.speed);
        if shake_ticks != 0
            && elapsed >= timeline.tail_start().saturating_sub(shake_ticks)
            && elapsed < timeline.tail_start()
        {
            let level = 3;
            let (x, y) = match elapsed % 4 {
                0 => (-level, 0),
                1 => (0, level),
                2 => (level, 0),
                _ => (0, -level),
            };
            renderer.apply_shake(x, y);
        }
    }
}

fn render_shared_battle_effect(
    renderer: &mut Renderer,
    battle: &BattleState,
    effects: &BattleEffects,
    magic_effects: &BattleSpriteArchive,
    player_sprites: &BattleSpriteArchive,
    event: BattleEvent,
    ticks_remaining: u16,
) -> Option<()> {
    let action_elapsed =
        usize::from(ACTION_EVENT_TICKS.saturating_sub(ticks_remaining.min(ACTION_EVENT_TICKS)));
    let action_phase = action_elapsed.saturating_mul(3) / usize::from(ACTION_EVENT_TICKS);
    let (bitmap, x, y) = match event {
        BattleEvent::PlayerAttack {
            player,
            enemy,
            visual: true,
            ..
        } => {
            let actor = battle.players.get(player)?;
            let total = player_attack_ticks(event);
            let elapsed = total.saturating_sub(ticks_remaining.min(total));
            if !(7..10).contains(&elapsed) {
                return None;
            }
            let bitmap = effects.player_attack_frame(
                actor.battle_sprite_num,
                usize::from(elapsed.saturating_sub(7)),
            )?;
            let (x, y) = enemy_position(battle, enemy)?;
            (bitmap, x, y - 10)
        }
        BattleEvent::PlayerConfusedAttack { player, target, .. } => {
            let actor = battle.players.get(player)?;
            let bitmap = effects.player_attack_frame(actor.battle_sprite_num, action_phase)?;
            let (x, y) = player_position(battle.players.len(), target);
            (bitmap, x, y - 20)
        }
        BattleEvent::EnemyConfusedAttack { target, .. } => {
            let bitmap = effects.enemy_attack_frame(action_phase)?;
            let (x, y) = enemy_position(battle, target)?;
            (bitmap, x, y - 10)
        }
        BattleEvent::PlayerMagic {
            player,
            phase: MagicEventPhase::Visual,
            ..
        }
        | BattleEvent::PlayerDefensiveMagic { player, .. } => {
            let actor = battle.players.get(player)?;
            let (magic, timeline, elapsed) = magic_render_timeline(
                battle,
                magic_effects,
                player_sprites,
                event,
                ticks_remaining,
            )?;
            if magic.magic_type == 9 || elapsed >= timeline.pre_ticks {
                return None;
            }
            let effect_ticks = timeline.pre_ticks.clamp(1, 8);
            let effect_start = timeline.pre_ticks.saturating_sub(effect_ticks);
            if elapsed < effect_start {
                return None;
            }
            let phase =
                usize::from(elapsed - effect_start).saturating_mul(10) / usize::from(effect_ticks);
            let bitmap = effects.player_pre_magic_frame(actor.battle_sprite_num, phase)?;
            let (x, y) = player_position(battle.players.len(), player);
            (bitmap, x, y - 24)
        }
        BattleEvent::PlayerMagicAnimation {
            player: Some(player),
        } => {
            let actor = battle.players.get(player)?;
            let elapsed = usize::from(
                PLAYER_MAGIC_ANIMATION_EVENT_TICKS
                    .saturating_sub(ticks_remaining.min(PLAYER_MAGIC_ANIMATION_EVENT_TICKS)),
            );
            let bitmap = effects.player_pre_magic_frame(actor.battle_sprite_num, elapsed)?;
            let (x, y) = player_position(battle.players.len(), player);
            (bitmap, x - 10, y - 28)
        }
        _ => return None,
    };
    renderer.blit_rle(
        &bitmap,
        x - i32::from(bitmap.width) / 2,
        y - i32::from(bitmap.height),
    );
    Some(())
}

fn render_magic_effect(
    renderer: &mut Renderer,
    battle: &BattleState,
    effects: &BattleSpriteArchive,
    player_sprites: &BattleSpriteArchive,
    event: BattleEvent,
    ticks_remaining: u16,
    behind_fighters: bool,
) {
    let Some((magic, timeline, elapsed)) =
        magic_render_timeline(battle, effects, player_sprites, event, ticks_remaining)
    else {
        return;
    };
    if magic.magic_type == 9 {
        let summon_start = timeline.pre_ticks.saturating_add(timeline.brighten_ticks);
        let summon_end = timeline.total_ticks;
        if elapsed >= summon_start && elapsed < summon_end && !behind_fighters {
            let Some(sprite) = usize::try_from(magic.specific)
                .ok()
                .and_then(|sprite| sprite.checked_add(10))
            else {
                return;
            };
            let Some(frame_count) = player_sprites
                .frame_count(sprite)
                .filter(|count| *count > 0)
            else {
                return;
            };
            let body_end = timeline.body_start().saturating_add(timeline.body_ticks);
            let frame = if elapsed < timeline.body_start() {
                0
            } else if elapsed < body_end {
                timed_frame_at(
                    elapsed.saturating_sub(timeline.body_start()),
                    frame_count.saturating_sub(1).max(1),
                    magic.speed,
                )
            } else {
                frame_count.saturating_sub(1)
            };
            let visibility = if elapsed < timeline.body_start() {
                u8::try_from(
                    u32::from(elapsed.saturating_sub(summon_start))
                        .saturating_mul(64)
                        .checked_div(u32::from(timeline.summon_fade_in_ticks.max(1)))
                        .unwrap_or(0),
                )
                .unwrap_or(64)
                .min(64)
            } else {
                let fade_out_start = summon_end.saturating_sub(timeline.summon_fade_out_ticks);
                if elapsed >= fade_out_start {
                    u8::try_from(
                        u32::from(summon_end.saturating_sub(elapsed))
                            .saturating_mul(64)
                            .checked_div(u32::from(timeline.summon_fade_out_ticks.max(1)))
                            .unwrap_or(0),
                    )
                    .unwrap_or(64)
                    .min(64)
                } else {
                    64
                }
            };
            if let Some(bitmap) = player_sprites.decode_frame(sprite, frame) {
                let x = 240 + i32::from(magic.x_offset) - i32::from(bitmap.width) / 2;
                let y = 165 + i32::from(magic.y_offset) - i32::from(bitmap.height);
                if visibility < 64 {
                    renderer.blit_rle_dithered(&bitmap, x, y, visibility);
                } else {
                    renderer.blit_rle(&bitmap, x, y);
                }
            }
        }
        if elapsed < timeline.effect_start() {
            return;
        }
    }
    if elapsed < timeline.effect_start() || elapsed >= timeline.tail_start() {
        if elapsed >= timeline.tail_start() && magic.effect_visual().keep_effect == u16::MAX {
            render_kept_magic_effect(renderer, battle, effects, event);
        }
        return;
    }
    let visual = magic.effect_visual();
    if (visual.specific < 0) != behind_fighters {
        return;
    }
    let effect = usize::from(visual.effect);
    let Some(frame_count) = effects.frame_count(effect).filter(|count| *count > 0) else {
        return;
    };
    let effect_elapsed = elapsed.saturating_sub(timeline.effect_start());
    let frame = if matches!(event, BattleEvent::PlayerDefensiveMagic { .. }) {
        timed_frame_at(effect_elapsed, frame_count, visual.speed)
    } else {
        offensive_effect_frame_at(effect_elapsed, frame_count, visual)
    };
    let Some(bitmap) = effects.decode_frame(effect, frame) else {
        return;
    };
    let Some(positions) = magic_positions(battle, event, visual) else {
        return;
    };
    for (x, y) in positions {
        renderer.blit_rle(
            &bitmap,
            x + i32::from(visual.x_offset) - i32::from(bitmap.width) / 2,
            y + i32::from(visual.y_offset) - i32::from(bitmap.height),
        );
    }
}

fn magic_render_timeline(
    battle: &BattleState,
    effects: &BattleSpriteArchive,
    player_sprites: &BattleSpriteArchive,
    event: BattleEvent,
    ticks_remaining: u16,
) -> Option<(BattleMagic, MagicEventTimeline, u16)> {
    let magic = battle_magic_for_event(battle, event)?;
    let visual = magic.effect_visual();
    let effect_frame_count = effects.frame_count(usize::from(visual.effect));
    let summon_frame_count = (magic.magic_type == 9)
        .then(|| {
            usize::try_from(magic.specific)
                .ok()?
                .checked_add(10)
                .and_then(|sprite| player_sprites.frame_count(sprite))
        })
        .flatten();
    let timeline = magic_event_timeline(
        event,
        magic,
        effect_frame_count,
        summon_frame_count,
        enemy_magic_pre_frames(battle, event, magic),
    );
    let elapsed = timeline
        .total_ticks
        .saturating_sub(ticks_remaining.min(timeline.total_ticks));
    Some((magic, timeline, elapsed))
}

fn render_kept_magic_effect(
    renderer: &mut Renderer,
    battle: &BattleState,
    effects: &BattleSpriteArchive,
    event: BattleEvent,
) {
    let Some(magic) = battle_magic_for_event(battle, event) else {
        return;
    };
    let visual = magic.effect_visual();
    if visual.keep_effect != u16::MAX {
        return;
    }
    let effect = usize::from(visual.effect);
    let Some(frame_count) = effects.frame_count(effect).filter(|count| *count > 0) else {
        return;
    };
    let Some(bitmap) = effects.decode_frame(effect, kept_effect_frame(frame_count, visual)) else {
        return;
    };
    let Some(positions) = magic_positions(battle, event, visual) else {
        return;
    };
    for (x, y) in positions {
        renderer.blit_rle(
            &bitmap,
            x + i32::from(visual.x_offset) - i32::from(bitmap.width) / 2,
            y + i32::from(visual.y_offset) - i32::from(bitmap.height),
        );
    }
}

fn magic_positions(
    battle: &BattleState,
    event: BattleEvent,
    visual: BattleMagicVisual,
) -> Option<Vec<(i32, i32)>> {
    let (enemy, player, target) = match event {
        BattleEvent::PlayerMagic { enemy, .. }
        | BattleEvent::PlayerCooperativeMagic { enemy, .. }
        | BattleEvent::SimulatedMagic { enemy, .. } => (Some(enemy), None, None),
        BattleEvent::EnemyMagic { player, .. } => (None, Some(player), None),
        BattleEvent::PlayerDefensiveMagic { target, .. } => (None, None, Some(target)),
        _ => return None,
    };
    let to_enemy = visual.usable_to_enemy();
    Some(match visual.magic_type {
        1 if to_enemy => vec![(70, 140), (100, 110), (160, 100)],
        1 => vec![(180, 180), (234, 170), (270, 146)],
        2 if to_enemy => vec![(120, 100)],
        2 => vec![(240, 150)],
        3 => vec![(160, 200)],
        9 => vec![(160, 100)],
        _ => match target {
            Some(BattleTarget::Enemy(target)) => {
                enemy_position(battle, target).into_iter().collect()
            }
            Some(BattleTarget::Player(target)) => {
                vec![player_position(battle.players.len(), target)]
            }
            Some(BattleTarget::AllEnemies) => battle
                .enemies
                .iter()
                .enumerate()
                .filter_map(|(index, actor)| {
                    actor
                        .is_alive()
                        .then(|| enemy_position(battle, index))
                        .flatten()
                })
                .collect(),
            Some(BattleTarget::AllPlayers) => (0..battle.players.len())
                .map(|index| player_position(battle.players.len(), index))
                .collect(),
            None if enemy.is_some() => enemy_position(battle, enemy?).into_iter().collect(),
            None if player.is_some() => vec![player_position(battle.players.len(), player?)],
            None => Vec::new(),
        },
    })
}

fn enemy_position(battle: &BattleState, enemy: usize) -> Option<(i32, i32)> {
    let enemy = battle.enemies.get(enemy)?;
    Some((
        i32::from(enemy.position.x),
        i32::from(enemy.position.y) + i32::from(enemy.y_offset),
    ))
}

fn cooperative_contributor(player: &pal_core::battle::BattlePlayer) -> bool {
    use pal_core::battle::BattleStatus;

    player.is_alive()
        && !player.is_dying()
        && !player.statuses.is_active(BattleStatus::Sleep)
        && !player.statuses.is_active(BattleStatus::Confused)
        && !player.statuses.is_active(BattleStatus::Silence)
        && !player.statuses.is_active(BattleStatus::Paralyzed)
        && !player.statuses.is_active(BattleStatus::Puppet)
}

fn defensive_target_includes(target: BattleTarget, player: usize) -> bool {
    matches!(target, BattleTarget::AllPlayers)
        || matches!(target, BattleTarget::Player(target) if target == player)
}

fn action_offset(ticks_remaining: u16) -> i32 {
    let elapsed = ACTION_EVENT_TICKS.saturating_sub(ticks_remaining.min(ACTION_EVENT_TICKS));
    let distance = elapsed.min(ACTION_EVENT_TICKS.saturating_sub(elapsed));
    i32::from(distance) * 3
}

fn player_attack_animation_state(
    battle: &BattleState,
    event: BattleEvent,
    player_index: usize,
    ticks_remaining: u16,
) -> Option<(i32, i32, usize)> {
    let BattleEvent::PlayerAttack {
        player,
        enemy,
        visual: true,
        ..
    } = event
    else {
        return None;
    };
    if player != player_index {
        return None;
    }
    let total = player_attack_ticks(event);
    let elapsed = total.saturating_sub(ticks_remaining.min(total));
    let original = player_position(battle.players.len(), player);
    if elapsed < 4 {
        return Some((original.0, original.1, 7));
    }
    if elapsed >= 13 {
        return Some((original.0, original.1, 0));
    }
    let actor = battle.players.get(player)?;
    let target = battle.enemies.get(enemy)?;
    let (enemy_x, enemy_y) = if actor.attacks_all {
        (150, 100)
    } else {
        (
            i32::from(target.position.x),
            i32::from(target.position.y) + i32::from(target.y_offset),
        )
    };
    let distance = if !actor.attacks_all && target.slot >= 3 {
        (i32::try_from(target.slot).ok()? - i32::try_from(player).ok()?) * 8
    } else {
        0
    };
    let mut x = enemy_x - distance + 64;
    let mut y = enemy_y + distance + 20;
    let frame = if elapsed < 6 {
        8
    } else if elapsed < 7 {
        x -= 10;
        y -= 2;
        8
    } else {
        x -= 26;
        y -= 6;
        if elapsed >= 9 {
            x += 2;
            y += 1;
        }
        9
    };
    Some((x, y, frame))
}

fn enemy_actor_animation_state(
    battle: &BattleState,
    event: BattleEvent,
    enemy_index: usize,
    ticks_remaining: u16,
    magic_timing: Option<(BattleMagic, MagicEventTimeline, u16)>,
) -> Option<(i32, i32, usize)> {
    let enemy = battle.enemies.get(enemy_index)?;
    let idle = usize::from(enemy.idle_frames.max(1));
    let wait = usize::from(enemy.action_wait_frames.max(1));
    match event {
        BattleEvent::EnemyMagic {
            enemy: caster,
            phase: MagicEventPhase::Visual,
            ..
        } if caster == enemy_index => {
            let (magic, timeline, elapsed) = magic_timing?;
            if elapsed >= timeline.tail_start() {
                return Some((0, 0, 0));
            }
            let (x, y) = if elapsed == 0 { (12, 6) } else { (16, 8) };
            let casting_elapsed = elapsed.saturating_sub(2);
            let casting_frames = usize::from(enemy.magic_frames);
            let casting_ticks =
                u16::try_from(casting_frames.saturating_mul(wait)).unwrap_or(u16::MAX);
            if casting_elapsed < casting_ticks && casting_frames != 0 {
                let frame = idle
                    + usize::from(casting_elapsed)
                        .checked_div(wait)
                        .unwrap_or(0)
                        .min(casting_frames - 1);
                return Some((x, y, frame));
            }
            if magic.effect_visual().fire_delay == 0 && enemy.attack_frames != 0 {
                let local = usize::from(casting_elapsed.saturating_sub(casting_ticks));
                let frame = idle
                    + casting_frames
                    + local
                        .checked_div(wait)
                        .unwrap_or(0)
                        .min(usize::from(enemy.attack_frames).saturating_sub(1));
                return Some((x, y, frame));
            }
            Some((x, y, idle.saturating_sub(1)))
        }
        BattleEvent::EnemySummon {
            caster,
            summoned_mask,
        } if caster == enemy_index => {
            let casting_frames = usize::from(enemy.magic_frames);
            if casting_frames == 0 {
                return Some((0, 0, idle.saturating_sub(1)));
            }
            let pre_ticks = original_frames_to_ticks(casting_frames.saturating_mul(wait));
            let total = if summoned_mask == 0 {
                pre_ticks.max(1)
            } else {
                pre_ticks
                    .saturating_add(BATTLE_FADE_TICKS.saturating_mul(2))
                    .saturating_add(2)
            };
            let elapsed = total.saturating_sub(ticks_remaining.min(total));
            let frame = idle
                + usize::from(elapsed)
                    .checked_div(wait)
                    .unwrap_or(0)
                    .min(casting_frames - 1);
            Some((0, 0, frame))
        }
        BattleEvent::EnemyAttack {
            enemy: caster,
            player,
            ..
        } if caster == enemy_index => {
            let frames = enemy_attack_frames(battle, event);
            let total = original_frames_to_ticks(frames);
            let elapsed = usize::from(total.saturating_sub(ticks_remaining.min(total)));
            let magic_frames = usize::from(enemy.magic_frames);
            let startup = magic_frames
                .saturating_mul(2)
                .saturating_add(3usize.saturating_sub(magic_frames))
                .saturating_add(1);
            let attack_frames = frames.saturating_sub(startup).saturating_sub(11);
            if elapsed < magic_frames.saturating_mul(2) && magic_frames != 0 {
                return Some((0, 0, idle + (elapsed / 2).min(magic_frames - 1)));
            }
            if elapsed < startup {
                let steps = elapsed.saturating_sub(magic_frames.saturating_mul(2));
                return Some((-(steps as i32) * 2, -(steps as i32), idle.saturating_sub(1)));
            }
            if elapsed < startup.saturating_add(attack_frames) {
                let (target_x, target_y) = player_position(battle.players.len(), player);
                let target_x = target_x - 44 - i32::from(enemy.position.x);
                let target_y = target_y - 16 - i32::from(enemy.position.y);
                let local = elapsed - startup;
                let frame = if enemy.attack_frames == 0 {
                    idle.saturating_sub(1)
                } else {
                    idle + magic_frames
                        + (local / wait).min(usize::from(enemy.attack_frames).saturating_sub(1))
                };
                return Some((target_x, target_y, frame));
            }
            Some((0, 0, 0))
        }
        BattleEvent::EnemyConfusedAttack {
            enemy: caster,
            target,
            ..
        } if caster == enemy_index => {
            let total = original_frames_to_ticks(enemy_attack_frames(battle, event));
            let elapsed = usize::from(total.saturating_sub(ticks_remaining.min(total)));
            if elapsed >= 10 {
                return Some((0, 0, 0));
            }
            let target = battle.enemies.get(target)?;
            let divisor = 1i32 << elapsed.min(3);
            let x = (i32::from(target.position.x) - i32::from(enemy.position.x)) / divisor;
            let y = (i32::from(target.position.y) - i32::from(enemy.position.y)) / divisor;
            Some((x, y, idle.saturating_sub(1)))
        }
        _ => None,
    }
}

fn death_fade_visibility(ticks_remaining: u16) -> u8 {
    if ticks_remaining > BATTLE_FADE_TICKS {
        return 64;
    }
    u8::try_from(
        u32::from(ticks_remaining)
            .saturating_mul(64)
            .checked_div(u32::from(BATTLE_FADE_TICKS.max(1)))
            .unwrap_or(0),
    )
    .unwrap_or(64)
    .min(64)
}

fn enemy_feedback_state(
    event: BattleEvent,
    enemy_index: usize,
    ticks_remaining: u16,
    magic_timing: Option<(BattleMagic, MagicEventTimeline, u16)>,
) -> (i32, i32, bool) {
    if let BattleEvent::PlayerAttack {
        enemy,
        visual: true,
        ..
    } = event
    {
        if enemy != enemy_index {
            return (0, 0, false);
        }
        let total = player_attack_ticks(event);
        let elapsed = total.saturating_sub(ticks_remaining.min(total));
        return match elapsed {
            7 => (0, 0, true),
            10 => (-8, -4, false),
            11 => (-4, -2, false),
            12 => (-6, -3, false),
            _ => (0, 0, false),
        };
    }
    let targets_enemy = matches!(
        event,
        BattleEvent::PlayerMagic {
            enemy,
            phase: MagicEventPhase::Feedback,
            ..
        }
            | BattleEvent::PlayerCooperativeMagic { enemy, .. }
            | BattleEvent::SimulatedMagic { enemy, .. }
            if enemy == enemy_index
    );
    if targets_enemy {
        if let Some((_, timeline, elapsed)) = magic_timing {
            let tail = elapsed.saturating_sub(timeline.tail_start());
            return match tail {
                0 => (-8, 0, false),
                1 => (-4, 0, true),
                2 => (-6, 0, false),
                _ => (0, 0, false),
            };
        }
    }
    (0, 0, false)
}

fn player_feedback_offset(
    battle: &BattleState,
    event: BattleEvent,
    player_index: usize,
    ticks_remaining: u16,
    magic_timing: Option<(BattleMagic, MagicEventTimeline, u16)>,
) -> (i32, i32) {
    if let BattleEvent::EnemyAttack {
        player,
        protected_by,
        ..
    } = event
    {
        let affected = protected_by.unwrap_or(player);
        if affected != player_index {
            return (0, 0);
        }
        let frames = enemy_attack_frames(battle, event);
        let total = original_frames_to_ticks(frames);
        let elapsed = usize::from(total.saturating_sub(ticks_remaining.min(total)));
        let hit = frames.saturating_sub(11);
        if let Some(cover) = protected_by {
            let enemy = match event {
                BattleEvent::EnemyAttack { enemy, .. } => battle.enemies.get(enemy),
                _ => None,
            };
            let attack_frames = enemy.map_or(2, |enemy| {
                if enemy.attack_frames == 0 {
                    2
                } else {
                    usize::from(enemy.attack_frames)
                        .saturating_add(1)
                        .saturating_mul(usize::from(enemy.action_wait_frames.max(1)))
                }
            });
            if player_index == cover
                && elapsed >= hit.saturating_sub(attack_frames)
                && elapsed <= hit.saturating_add(4)
            {
                let original = player_position(battle.players.len(), cover);
                let target = player_position(battle.players.len(), player);
                let knock = if elapsed > hit { (4, 2) } else { (0, 0) };
                return (
                    target.0 - 24 - original.0 + knock.0,
                    target.1 - 12 - original.1 + knock.1,
                );
            }
        }
        if elapsed <= hit {
            return (0, 0);
        }
        return match elapsed - hit {
            1 => (8, 4),
            2..=4 => (10, 5),
            _ => (0, 0),
        };
    }
    if let BattleEvent::EnemyMagic {
        player,
        phase: MagicEventPhase::Feedback,
        ..
    } = event
    {
        if player != player_index {
            return (0, 0);
        }
        if let Some((_, timeline, elapsed)) = magic_timing {
            return match elapsed.saturating_sub(timeline.tail_start()) {
                0 => (0, 0),
                1 => (4, 2),
                2 => (6, 3),
                3..=4 => (7, 3),
                _ => (0, 0),
            };
        }
    }
    (0, 0)
}

fn magic_blow_offset(amount: i16, ticks_remaining: u16) -> i32 {
    if amount == 0 {
        return 0;
    }
    let frame_count = ACTION_EVENT_TICKS
        .saturating_sub(ticks_remaining.min(ACTION_EVENT_TICKS))
        .saturating_add(1);
    let lower = i32::from(amount.min(0));
    let upper = i32::from(amount.max(0));
    let span = u32::try_from(upper - lower + 1).unwrap_or(1);
    let mut state = 0x6d2b_79f5 ^ u32::from(amount as u16);
    let mut offset = 0i32;
    for _ in 0..frame_count {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        offset += lower + i32::try_from(state % span).unwrap_or(0);
    }
    offset
}

fn player_magic_animation_state(
    event: Option<BattleEvent>,
    player_index: usize,
    ticks_remaining: u16,
) -> (i32, i32, Option<usize>, i16) {
    let Some(BattleEvent::PlayerMagicAnimation { player }) = event else {
        return (0, 0, None, 0);
    };
    let elapsed = PLAYER_MAGIC_ANIMATION_EVENT_TICKS
        .saturating_sub(ticks_remaining.min(PLAYER_MAGIC_ANIMATION_EVENT_TICKS));
    let color_shift = if elapsed >= 17 {
        i16::try_from((elapsed - 17).min(4) * 2).unwrap_or(8)
    } else {
        0
    };
    if player != Some(player_index) {
        return (0, 0, None, color_shift);
    }
    let movement_steps = usize::from(elapsed.saturating_add(1).min(4));
    let x = -[4, 3, 2, 1][..movement_steps].iter().sum::<i32>();
    let y = -[2, 1, 1, 0][..movement_steps].iter().sum::<i32>();
    let frame = if elapsed >= 17 {
        6
    } else if elapsed >= 6 {
        5
    } else {
        0
    };
    (x, y, Some(frame), color_shift)
}

fn pre_magic_offset(elapsed: u16) -> (i32, i32) {
    let movement_steps = usize::from(original_frame_for_tick(elapsed).saturating_add(1).min(4));
    (
        -[4, 3, 2, 1][..movement_steps].iter().sum::<i32>(),
        -[2, 1, 1, 0][..movement_steps].iter().sum::<i32>(),
    )
}

fn player_item_animation_state(
    event: Option<BattleEvent>,
    player_index: usize,
    ticks_remaining: u16,
) -> (i32, i32, Option<usize>, i16) {
    match event {
        Some(BattleEvent::PlayerUseItem { player, target, .. }) => {
            let total = original_frames_to_ticks(17);
            let elapsed = total.saturating_sub(ticks_remaining.min(total));
            let frame = original_frame_for_tick(elapsed);
            let affected = target.is_none_or(|target| target == player_index);
            let color_shift = if affected && (4..17).contains(&frame) {
                let phase = usize::from(frame - 4);
                i16::try_from(if phase <= 6 { phase } else { 12 - phase }).unwrap_or(0)
            } else {
                0
            };
            if player == player_index && (4..17).contains(&frame) {
                (-15, -7, Some(5), color_shift)
            } else {
                (0, 0, None, color_shift)
            }
        }
        Some(BattleEvent::PlayerThrowItem { player, .. }) if player == player_index => {
            let total = original_frames_to_ticks(16);
            let elapsed = total.saturating_sub(ticks_remaining.min(total));
            let frame = original_frame_for_tick(elapsed);
            let movement_steps = usize::from(frame.saturating_add(1).min(4));
            let x = -[4, 3, 2, 1][..movement_steps].iter().sum::<i32>();
            let y = -[2, 1, 1, 0][..movement_steps].iter().sum::<i32>();
            let pose = match frame {
                6..14 => Some(5),
                14..16 => Some(6),
                _ => None,
            };
            (x, y, pose, 0)
        }
        _ => (0, 0, None, 0),
    }
}

fn original_frame_for_tick(elapsed: u16) -> u16 {
    elapsed.saturating_mul(5) / 4
}

fn render_battle_action_label(
    renderer: &mut Renderer,
    event: BattleEvent,
    ticks_remaining: u16,
    text: &TextLibrary,
    font: &BitmapFont,
) {
    let (object_id, total_frames, visible_range) = match event {
        BattleEvent::PlayerUseItem { item_object, .. } => (item_object, 17, 4..17),
        BattleEvent::PlayerThrowItem { item_object, .. } => (item_object, 16, 4..16),
        _ => return,
    };
    let total = original_frames_to_ticks(total_frames);
    let elapsed = total.saturating_sub(ticks_remaining.min(total));
    if !visible_range.contains(&original_frame_for_tick(elapsed)) {
        return;
    }
    if let Some(label) = text.word(usize::from(object_id)) {
        renderer.draw_big5_text_shadowed(font, label, 210, 50, 0x2d);
    }
}

fn render_battle_event(
    renderer: &mut Renderer,
    battle: &BattleState,
    event: BattleEvent,
    ui_sprites: &[RleBitmap],
    text: &TextLibrary,
    font: &BitmapFont,
) {
    if let BattleEvent::PlayerFlee {
        succeeded: false, ..
    } = event
    {
        if let Some(label) = text.word(31) {
            let width = i32::try_from(label.len() / 2).unwrap_or(0) * 16;
            renderer.draw_big5_text_shadowed(font, label, 160 - width / 2, 90, 0x2d);
        }
        return;
    }
    let (x, y, damage) = match event {
        BattleEvent::PlayerAttack { enemy, damage, .. }
        | BattleEvent::PlayerCooperativeMagic { enemy, damage, .. }
        | BattleEvent::SimulatedMagic { enemy, damage, .. } => {
            let Some(enemy) = battle.enemies.get(enemy) else {
                return;
            };
            (
                i32::from(enemy.position.x) + 12,
                i32::from(enemy.position.y) - 24,
                damage,
            )
        }
        BattleEvent::PlayerMagic {
            enemy,
            damage,
            phase: MagicEventPhase::Feedback,
            ..
        } => {
            let Some(enemy) = battle.enemies.get(enemy) else {
                return;
            };
            (
                i32::from(enemy.position.x) + 12,
                i32::from(enemy.position.y) - 24,
                damage,
            )
        }
        BattleEvent::EnemyAttack {
            auto_defended: true,
            ..
        } => return,
        BattleEvent::EnemyAttack { player, damage, .. }
        | BattleEvent::PlayerConfusedAttack {
            target: player,
            damage,
            ..
        } => {
            let (x, y) = player_position(battle.players.len(), player);
            (x + 12, y - 34, damage)
        }
        BattleEvent::EnemyMagic {
            player,
            damage,
            phase: MagicEventPhase::Feedback,
            ..
        } => {
            let (x, y) = player_position(battle.players.len(), player);
            (x + 12, y - 34, damage)
        }
        BattleEvent::EnemyConfusedAttack { target, damage, .. } => {
            let Some(enemy) = battle.enemies.get(target) else {
                return;
            };
            (
                i32::from(enemy.position.x) + 12,
                i32::from(enemy.position.y) - 24,
                damage,
            )
        }
        BattleEvent::PlayerUseItem { .. }
        | BattleEvent::PlayerThrowItem { .. }
        | BattleEvent::PlayerItemFeedback { .. }
        | BattleEvent::PlayerFlee { .. }
        | BattleEvent::PlayerDefend { .. }
        | BattleEvent::PlayerDefensiveMagic { .. }
        | BattleEvent::PlayerMagicAnimation { .. }
        | BattleEvent::PlayerFriendDeath { .. }
        | BattleEvent::PlayerDying { .. }
        | BattleEvent::EnemyDivide { .. }
        | BattleEvent::EnemySummon { .. }
        | BattleEvent::EnemyTransform { .. }
        | BattleEvent::EnemyEscape
        | BattleEvent::RoundCompleted
        | BattleEvent::Finished(_) => return,
        BattleEvent::PlayerMagic {
            phase: MagicEventPhase::Visual,
            ..
        }
        | BattleEvent::EnemyMagic {
            phase: MagicEventPhase::Visual,
            ..
        } => return,
    };
    draw_ui_number(
        renderer,
        ui_sprites,
        u32::from(damage),
        5,
        x - 20,
        y,
        19,
        [255, 255, 255, 255],
    );
}

fn enemy_script_animation_state(
    battle: &BattleState,
    event: BattleEvent,
    enemy_index: usize,
    ticks_remaining: u16,
    enemy_escape_timing: Option<EnemyEscapeTimeline>,
) -> (i32, i32, u8, i16) {
    let Some(enemy) = battle.enemies.get(enemy_index) else {
        return (0, 0, 64, 0);
    };
    match event {
        BattleEvent::EnemyDivide { origin } => {
            let elapsed = ENEMY_DIVIDE_EVENT_TICKS
                .saturating_sub(ticks_remaining.min(ENEMY_DIVIDE_EVENT_TICKS));
            let rendered = enemy_divide_position(origin, enemy.position, elapsed);
            (
                i32::from(rendered.x) - i32::from(enemy.position.x),
                i32::from(rendered.y) - i32::from(enemy.position.y),
                64,
                0,
            )
        }
        BattleEvent::EnemySummon {
            caster,
            summoned_mask,
        } if summoned_mask & (1u8.checked_shl(enemy.slot as u32).unwrap_or(0)) != 0 => {
            let pre_ticks = battle.enemies.get(caster).map_or(0, |caster| {
                original_frames_to_ticks(
                    usize::from(caster.magic_frames)
                        .saturating_mul(usize::from(caster.action_wait_frames.max(1))),
                )
            });
            let total = pre_ticks
                .saturating_add(BATTLE_FADE_TICKS.saturating_mul(2))
                .saturating_add(2);
            let elapsed = total.saturating_sub(ticks_remaining.min(total));
            let (visibility, color_shift) = enemy_summon_visual_state(pre_ticks, elapsed);
            (0, 0, visibility, color_shift)
        }
        BattleEvent::EnemyTransform { enemy: target, .. } if target == enemy_index => {
            let elapsed = ENEMY_TRANSFORM_EVENT_TICKS
                .saturating_sub(ticks_remaining.min(ENEMY_TRANSFORM_EVENT_TICKS));
            let (visibility, color_shift) = enemy_transform_visual_state(elapsed);
            (0, 0, visibility, color_shift)
        }
        BattleEvent::EnemyEscape => enemy_escape_timing.map_or((0, 0, 64, 0), |timeline| {
            (enemy_escape_offset(timeline, ticks_remaining), 0, 64, 0)
        }),
        _ => (0, 0, 64, 0),
    }
}

fn enemy_summon_visual_state(pre_ticks: u16, elapsed: u16) -> (u8, i16) {
    let first_fade_elapsed = elapsed.saturating_sub(pre_ticks);
    if first_fade_elapsed < BATTLE_FADE_TICKS {
        return (fade_progress(first_fade_elapsed, 64), 8);
    }
    let second_fade_elapsed = first_fade_elapsed
        .saturating_sub(BATTLE_FADE_TICKS)
        .saturating_sub(2);
    let color_shift = if second_fade_elapsed < BATTLE_FADE_TICKS {
        let remaining = BATTLE_FADE_TICKS.saturating_sub(second_fade_elapsed);
        i16::try_from(
            u32::from(remaining)
                .saturating_mul(8)
                .div_ceil(u32::from(BATTLE_FADE_TICKS.max(1))),
        )
        .unwrap_or(8)
    } else {
        0
    };
    (64, color_shift)
}

fn enemy_transform_visual_state(elapsed: u16) -> (u8, i16) {
    if elapsed < 6 {
        (64, i16::try_from(elapsed).unwrap_or(5))
    } else {
        (fade_progress(elapsed.saturating_sub(6), 64), 0)
    }
}

fn enemy_divide_position(
    origin: BattlePosition,
    destination: BattlePosition,
    elapsed: u16,
) -> BattlePosition {
    if elapsed >= 10 {
        return destination;
    }
    let mut x = i32::from(origin.x);
    let mut y = i32::from(origin.y);
    for _ in 0..=elapsed {
        x = (x + i32::from(destination.x)) / 2;
        y = (y + i32::from(destination.y)) / 2;
    }
    BattlePosition {
        x: u16::try_from(x).unwrap_or(destination.x),
        y: u16::try_from(y).unwrap_or(destination.y),
    }
}

fn enemy_transform_sprite_state(
    event: BattleEvent,
    enemy_index: usize,
    ticks_remaining: u16,
) -> Option<(u16, u16, usize)> {
    let BattleEvent::EnemyTransform {
        enemy,
        previous_enemy_id,
        previous_y_offset,
    } = event
    else {
        return None;
    };
    let elapsed = ENEMY_TRANSFORM_EVENT_TICKS
        .saturating_sub(ticks_remaining.min(ENEMY_TRANSFORM_EVENT_TICKS));
    (enemy == enemy_index && elapsed < 6).then_some((previous_enemy_id, previous_y_offset, 0))
}

fn fade_progress(elapsed: u16, maximum: u8) -> u8 {
    u8::try_from(
        u32::from(elapsed.min(BATTLE_FADE_TICKS))
            .saturating_mul(u32::from(maximum))
            .checked_div(u32::from(BATTLE_FADE_TICKS.max(1)))
            .unwrap_or(0),
    )
    .unwrap_or(maximum)
    .min(maximum)
}

fn render_settlement(
    renderer: &mut Renderer,
    battle: &BattleState,
    result: BattleResult,
    ui_sprites: &[RleBitmap],
    text: &TextLibrary,
    font: &BitmapFont,
) {
    if result != BattleResult::Won {
        return;
    }
    let rewards = battle.rewards();
    if rewards.experience == 0 {
        return;
    }
    let experience_label = text.word(30);
    let experience_box_length = experience_label.map_or(7, |label| text_columns(label) + 3);
    let experience_offset = (i32::try_from(experience_box_length).unwrap_or(8) - 8) * 8;
    draw_single_line_box(
        renderer,
        ui_sprites,
        83 - experience_offset,
        60,
        experience_box_length,
    );
    draw_single_line_box(renderer, ui_sprites, 65, 105, 10);
    if let Some(label) = experience_label {
        renderer.draw_big5_text(font, label, 95 - experience_offset, 70, 0);
    }
    draw_ui_number(
        renderer,
        ui_sprites,
        rewards.experience,
        5,
        182 + experience_offset,
        74,
        19,
        [240, 224, 96, 255],
    );
    if let Some(label) = text.word(9) {
        renderer.draw_big5_text(font, label, 77, 115, 0);
    }
    draw_ui_number_mid(
        renderer,
        ui_sprites,
        rewards.cash,
        5,
        162,
        119,
        19,
        [240, 224, 96, 255],
    );
    if let Some(label) = text.word(10) {
        renderer.draw_big5_text(font, label, 197, 115, 0);
    }
}

pub(super) fn render_post_battle_page(
    renderer: &mut Renderer,
    presentation: &PostBattlePresentation,
    ui_sprites: &[RleBitmap],
    text: &TextLibrary,
    font: &BitmapFont,
) {
    let Some(page) = presentation.current_page() else {
        return;
    };
    match page {
        BattleSettlementPage::LevelUp { before, after } => {
            let property_length = (48usize..=55)
                .filter_map(|word| text.word(word))
                .map(text_columns)
                .max()
                .unwrap_or(2)
                .saturating_sub(2);
            let offset_x = -8 * i32::try_from(property_length).unwrap_or(0);
            draw_single_line_box(renderer, ui_sprites, offset_x + 80, 0, property_length + 10);
            draw_ui_box_with_shadow(
                renderer,
                ui_sprites,
                offset_x + 82,
                32,
                7,
                property_length + 8,
                1,
                6,
            );
            let mut title_x = 110;
            for word in [usize::from(after.name_word_id), 48, 32] {
                if let Some(label) = text.word(word) {
                    renderer.draw_big5_text(font, label, title_x, 10, 0);
                    title_x += i32::try_from(big5_text_width(label)).unwrap_or(0);
                }
            }
            for (row, label) in (48usize..=55).enumerate() {
                let y = 44 + i32::try_from(row).unwrap_or(0) * 18;
                if let Some(label) = text.word(label) {
                    renderer.draw_big5_text_shadowed(font, label, offset_x + 100, y, 0xbb);
                }
                if let Some(arrow) = ui_sprites.get(47) {
                    renderer.blit_rle(arrow, 180 - offset_x, 48 + row as i32 * 18);
                }
            }

            draw_ui_number(
                renderer,
                ui_sprites,
                u32::from(before.level),
                4,
                133 - offset_x,
                47,
                19,
                [240, 224, 96, 255],
            );
            draw_ui_number(
                renderer,
                ui_sprites,
                u32::from(after.level),
                4,
                195 - offset_x,
                47,
                19,
                [240, 224, 96, 255],
            );
            for (row, (old_current, old_max, new_current, new_max)) in [
                (before.hp, before.max_hp, after.hp, after.max_hp),
                (before.mp, before.max_mp, after.mp, after.max_mp),
            ]
            .into_iter()
            .enumerate()
            {
                let current_y = 64 + row as i32 * 18;
                let max_y = 68 + row as i32 * 18;
                draw_ui_number(
                    renderer,
                    ui_sprites,
                    u32::from(old_current),
                    4,
                    133 - offset_x,
                    current_y,
                    19,
                    [240, 224, 96, 255],
                );
                draw_ui_number(
                    renderer,
                    ui_sprites,
                    u32::from(old_max),
                    4,
                    154 - offset_x,
                    max_y,
                    29,
                    [144, 184, 240, 255],
                );
                draw_slash(renderer, ui_sprites, 156 - offset_x, 66 + row as i32 * 18);
                draw_ui_number(
                    renderer,
                    ui_sprites,
                    u32::from(new_current),
                    4,
                    195 - offset_x,
                    current_y,
                    19,
                    [240, 224, 96, 255],
                );
                draw_ui_number(
                    renderer,
                    ui_sprites,
                    u32::from(new_max),
                    4,
                    216 - offset_x,
                    max_y,
                    29,
                    [144, 184, 240, 255],
                );
                draw_slash(renderer, ui_sprites, 218 - offset_x, 66 + row as i32 * 18);
            }
            for (row, (old, new)) in [
                (before.attack_strength, after.attack_strength),
                (before.magic_strength, after.magic_strength),
                (before.defense, after.defense),
                (before.dexterity, after.dexterity),
                (before.flee_rate, after.flee_rate),
            ]
            .into_iter()
            .enumerate()
            {
                let y = 101 + row as i32 * 18;
                draw_ui_number(
                    renderer,
                    ui_sprites,
                    u32::from(old),
                    4,
                    133 - offset_x,
                    y,
                    19,
                    [240, 224, 96, 255],
                );
                draw_ui_number(
                    renderer,
                    ui_sprites,
                    u32::from(new),
                    4,
                    195 - offset_x,
                    y,
                    19,
                    [240, 224, 96, 255],
                );
            }
        }
        BattleSettlementPage::AttributeGrowth {
            role_id,
            label,
            amount,
        } => {
            let name_word = presentation
                .battle
                .players
                .iter()
                .find(|player| player.role_id == *role_id)
                .map(|player| player.name_word_id);
            let max_name_width = presentation
                .battle
                .players
                .iter()
                .filter_map(|player| text.word(usize::from(player.name_word_id)))
                .map(text_columns)
                .max()
                .unwrap_or(3);
            let max_property_width = (48usize..=55)
                .filter_map(|word| text.word(word))
                .map(text_columns)
                .max()
                .unwrap_or(2)
                .saturating_sub(1);
            let property_length = max_property_width.saturating_sub(1);
            let offset_x = -8 * i32::try_from(property_length).unwrap_or(0);
            let up_width = text.word(32).map_or(1, |up| big5_text_width(up) / 32);
            draw_single_line_box(
                renderer,
                ui_sprites,
                offset_x + 78,
                60,
                max_name_width + max_property_width + up_width + 4,
            );
            let mut message_x = offset_x + 90;
            for word in [name_word.map(usize::from), Some(*label), Some(32)]
                .into_iter()
                .flatten()
            {
                if let Some(part) = text.word(word) {
                    renderer.draw_big5_text(font, part, message_x, 70, 0);
                    message_x += i32::try_from(big5_text_width(part)).unwrap_or(0);
                }
            }
            draw_ui_number(
                renderer,
                ui_sprites,
                u32::from(*amount),
                3,
                183 + i32::try_from(max_name_width + max_property_width)
                    .unwrap_or(3)
                    .saturating_sub(3)
                    * 8,
                74,
                19,
                [240, 224, 96, 255],
            );
        }
        BattleSettlementPage::LearnedMagic {
            role_id,
            magic_object,
        } => {
            let name_word = presentation
                .battle
                .players
                .iter()
                .find(|player| player.role_id == *role_id)
                .map(|player| player.name_word_id);
            let name = name_word.and_then(|word| text.word(usize::from(word)));
            let learned = text.word(33);
            let magic = text.word(usize::from(*magic_object));
            let name_width = name.map_or(3, text_columns).max(3);
            let learned_width = learned.map_or(2, text_columns).max(2);
            let magic_width = magic.map_or(5, text_columns).max(5);
            let total_width = name_width + learned_width + magic_width;
            let offset = (i32::try_from(total_width).unwrap_or(10) - 10) * 8;
            draw_single_line_box(renderer, ui_sprites, 65 - offset, 105, total_width);
            if let Some(name) = name {
                renderer.draw_big5_text(font, name, 75 - offset, 115, 0);
            }
            if let Some(learned) = learned {
                renderer.draw_big5_text(
                    font,
                    learned,
                    75 + i32::try_from(name_width).unwrap_or(3) * 16 - offset,
                    115,
                    0,
                );
            }
            if let Some(magic) = magic {
                renderer.draw_big5_text(
                    font,
                    magic,
                    75 + i32::try_from(name_width + learned_width).unwrap_or(5) * 16 - offset,
                    115,
                    0x1b,
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_status(
    renderer: &mut Renderer,
    battle: &BattleState,
    ui_sprites: &[RleBitmap],
    selected_command: usize,
    menu: BattleMenuState,
    auto_attack: bool,
    text: &TextLibrary,
    font: &BitmapFont,
    ui_ticks: u64,
    battle_ticks: u64,
    cash: u32,
) {
    for (index, player) in battle.players.iter().enumerate() {
        let x = 91 + i32::try_from(index).unwrap_or(0) * 77;
        let y = 165;
        if let Some(info_box) = ui_sprites.get(18) {
            renderer.blit_rle(info_box, x, y);
        }
        if let Some(face) = ui_sprites.get(48 + usize::from(player.role_id)) {
            let color = if player.is_alive() {
                player.poison_face_color
            } else {
                Some(0)
            };
            if let Some(color) = color {
                renderer.blit_rle_mono(face, x - 2, y - 4, color, 0);
            } else {
                renderer.blit_rle(face, x - 2, y - 4);
            }
        }
        if let Some(slash) = ui_sprites.get(39) {
            renderer.blit_rle(slash, x + 49, y + 6);
            renderer.blit_rle(slash, x + 49, y + 22);
        }
        draw_ui_number(
            renderer,
            ui_sprites,
            u32::from(player.hp),
            4,
            x + 26,
            y + 5,
            19,
            [240, 224, 96, 255],
        );
        draw_ui_number(
            renderer,
            ui_sprites,
            u32::from(player.max_hp),
            4,
            x + 47,
            y + 8,
            19,
            [240, 224, 96, 255],
        );
        draw_ui_number(
            renderer,
            ui_sprites,
            u32::from(player.mp),
            4,
            x + 26,
            y + 21,
            56,
            [96, 224, 240, 255],
        );
        draw_ui_number(
            renderer,
            ui_sprites,
            u32::from(player.max_mp),
            4,
            x + 47,
            y + 24,
            56,
            [96, 224, 240, 255],
        );
        if player.is_alive() {
            for (status, word, dx, dy, color) in [
                (BattleStatus::Confused, 0x1d, 35, 19, 0x5f),
                (BattleStatus::Paralyzed, 0x1b, 44, 12, 0xbf),
                (BattleStatus::Sleep, 0x1c, 54, 1, 0x0e),
                (BattleStatus::Silence, 0x1a, 55, 20, 0x3c),
            ] {
                if player.statuses.is_active(status) {
                    if let Some(label) = text.word(word) {
                        renderer.draw_big5_text_shadowed(font, label, x + dx, y + dy, color);
                    }
                }
            }
        }
    }

    if auto_attack {
        if let Some(label) = text.word(56) {
            let width = i32::try_from(label.len() / 2).unwrap_or(0) * 16;
            renderer.draw_big5_text_shadowed(font, label, 312 - width, 10, 0x2d);
        }
    }

    if let BattleMenuState::TargetPlayer { selected, .. } = menu {
        let (x, y) = player_position(battle.players.len(), selected);
        let arrow = if battle_ticks & 1 == 0 { 66 } else { 67 };
        if let Some(sprite) = ui_sprites.get(arrow) {
            renderer.blit_rle(sprite, x - 8, y - 67);
        }
    } else if !matches!(menu, BattleMenuState::TargetEnemy { .. }) {
        let Some(active) = battle.active_player() else {
            return;
        };
        let (x, y) = player_position(battle.players.len(), active);
        let arrow = if battle_ticks & 1 == 0 { 68 } else { 69 };
        if let Some(sprite) = ui_sprites.get(arrow) {
            renderer.blit_rle(sprite, x - 8, y - 74);
        }
    }

    let magic_enabled = battle.active_player().is_some_and(|active| {
        !battle.players[active]
            .statuses
            .is_active(BattleStatus::Silence)
    });
    let cooperative_magic_enabled = battle.can_use_cooperative_magic();
    for (index, (sprite_index, x, y)) in BATTLE_COMMAND_ICONS.into_iter().enumerate() {
        let Some(sprite) = ui_sprites.get(sprite_index) else {
            continue;
        };
        if matches!(menu, BattleMenuState::TargetEnemy { .. }) {
            continue;
        }
        let enabled = match index {
            1 => magic_enabled,
            2 => cooperative_magic_enabled,
            _ => true,
        };
        if !matches!(menu, BattleMenuState::TargetPlayer { .. })
            && index == selected_command
            && enabled
        {
            renderer.blit_rle(sprite, x, y);
        } else if enabled {
            renderer.blit_rle_mono(sprite, x, y, 0, -4);
        } else {
            renderer.blit_rle_mono(sprite, x, y, 0x10, -4);
        }
    }

    match menu {
        BattleMenuState::Main | BattleMenuState::TargetEnemy { .. } => {}
        BattleMenuState::Magic { selected } => {
            draw_ui_box_with_shadow(renderer, ui_sprites, 10, 42, 4, 16, 1, 0);
            draw_single_line_box(renderer, ui_sprites, 0, 0, 5);
            if let Some(label) = text.word(21) {
                renderer.draw_big5_text(font, label, 10, 10, 0);
            }
            draw_ui_number(
                renderer,
                ui_sprites,
                cash,
                6,
                49,
                14,
                19,
                [240, 224, 96, 255],
            );
            draw_single_line_box(renderer, ui_sprites, 215, 0, 5);
            let Some(player) = battle
                .active_player()
                .and_then(|active| battle.players.get(active))
            else {
                return;
            };
            if let Some(magic) = player
                .magics
                .get(selected.min(player.magics.len().saturating_sub(1)))
            {
                draw_ui_number(
                    renderer,
                    ui_sprites,
                    u32::from(magic.mp_cost),
                    4,
                    230,
                    14,
                    19,
                    [240, 224, 96, 255],
                );
            }
            draw_slash(renderer, ui_sprites, 260, 14);
            draw_ui_number(
                renderer,
                ui_sprites,
                u32::from(player.mp),
                4,
                265,
                14,
                56,
                [96, 224, 240, 255],
            );

            let first = selected.saturating_div(3).saturating_sub(2) * 3;
            for (visible_index, magic) in player.magics.iter().skip(first).take(15).enumerate() {
                let index = first + visible_index;
                let column = visible_index % 3;
                let row = visible_index / 3;
                let x = 35 + i32::try_from(column).unwrap_or(0) * 87;
                let y = 54 + i32::try_from(row).unwrap_or(0) * 18;
                if let Some(name) = text.word(usize::from(magic.object_id)) {
                    let enabled = magic_enabled && player.mp >= magic.mp_cost;
                    let color = match (index == selected, enabled) {
                        (true, true) => selected_color(ui_ticks),
                        (true, false) => 0x1c,
                        (false, true) => 0x4f,
                        (false, false) => 0x18,
                    };
                    renderer.draw_big5_text_shadowed(font, name, x, y, color);
                }
                if index == selected {
                    draw_cursor(renderer, ui_sprites, x + 25, y + 10);
                }
            }
        }
        BattleMenuState::Misc { selected } => {
            render_word_menu(
                renderer,
                ui_sprites,
                text,
                font,
                &[56, 57, 58, 59, 60],
                selected,
                2,
                20,
                ui_ticks,
            );
        }
        BattleMenuState::ItemSubmenu { selected } => {
            render_word_menu(
                renderer,
                ui_sprites,
                text,
                font,
                &[23, 24],
                selected,
                30,
                50,
                ui_ticks,
            );
        }
        BattleMenuState::TargetPlayer { command, selected } => {
            if let BattlePendingCommand::Magic(magic) = command {
                if let Some(name) = battle
                    .active_player()
                    .and_then(|active| battle.players.get(active))
                    .and_then(|player| player.magics.get(magic))
                    .and_then(|magic| text.word(usize::from(magic.object_id)))
                {
                    renderer.draw_big5_text_shadowed(font, name, 12, 118, 0x4f);
                }
            }
            let _ = selected;
        }
        BattleMenuState::Status { .. } => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn render_word_menu(
    renderer: &mut Renderer,
    ui_sprites: &[RleBitmap],
    text: &TextLibrary,
    font: &BitmapFont,
    words: &[usize],
    selected: usize,
    x: i32,
    y: i32,
    ticks: u64,
) {
    let columns = words
        .iter()
        .filter_map(|&word| text.word(word))
        .map(big5_text_width)
        .max()
        .unwrap_or(16)
        .saturating_add(8)
        / 16;
    draw_ui_box_with_shadow(
        renderer,
        ui_sprites,
        x,
        y,
        words.len().saturating_sub(1),
        columns.saturating_sub(1),
        0,
        6,
    );
    for (index, &word) in words.iter().enumerate() {
        let Some(label) = text.word(word) else {
            continue;
        };
        let color = if index == selected {
            selected_color(ticks)
        } else {
            0x4f
        };
        renderer.draw_big5_text_shadowed(
            font,
            label,
            x + 14,
            y + 12 + i32::try_from(index).unwrap_or(0) * 18,
            color,
        );
    }
}

fn big5_text_width(text: &[u8]) -> usize {
    let mut width = 0usize;
    let mut index = 0usize;
    while index < text.len() {
        if text[index] < 0x80 {
            width = width.saturating_add(8);
            index += 1;
        } else if index + 1 < text.len() {
            width = width.saturating_add(16);
            index += 2;
        } else {
            break;
        }
    }
    width
}

fn text_columns(text: &[u8]) -> usize {
    big5_text_width(text).saturating_add(8) >> 4
}

#[allow(clippy::too_many_arguments)]
fn draw_ui_number(
    renderer: &mut Renderer,
    ui_sprites: &[RleBitmap],
    value: u32,
    length: usize,
    x: i32,
    y: i32,
    sprite_base: usize,
    fallback: [u8; 4],
) {
    if ui_sprites.get(sprite_base + 9).is_none() {
        draw_number(renderer, value, x + length as i32 * 6, y, fallback);
        return;
    }
    let digits = value.to_string();
    let visible = &digits[digits.len().saturating_sub(length)..];
    let mut draw_x = x + (length.saturating_sub(visible.len())) as i32 * 6;
    for digit in visible.bytes() {
        renderer.blit_rle(
            &ui_sprites[sprite_base + usize::from(digit - b'0')],
            draw_x,
            y,
        );
        draw_x += 6;
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_ui_number_mid(
    renderer: &mut Renderer,
    ui_sprites: &[RleBitmap],
    value: u32,
    length: usize,
    x: i32,
    y: i32,
    sprite_base: usize,
    fallback: [u8; 4],
) {
    let digits = value.to_string();
    let visible = &digits[digits.len().saturating_sub(length)..];
    let draw_x = x + i32::try_from(length.saturating_sub(visible.len())).unwrap_or(0) * 3;
    if ui_sprites.get(sprite_base + 9).is_none() {
        draw_number(
            renderer,
            value,
            draw_x + i32::try_from(visible.len()).unwrap_or(0) * 6,
            y,
            fallback,
        );
        return;
    }
    for (index, digit) in visible.bytes().enumerate() {
        renderer.blit_rle(
            &ui_sprites[sprite_base + usize::from(digit - b'0')],
            draw_x + i32::try_from(index).unwrap_or(0) * 6,
            y,
        );
    }
}

fn player_position(count: usize, index: usize) -> (i32, i32) {
    const POSITIONS: [[(i32, i32); 3]; 3] = [
        [(240, 170), (240, 170), (240, 170)],
        [(200, 176), (256, 152), (256, 152)],
        [(180, 180), (234, 170), (270, 146)],
    ];
    if (1..=3).contains(&count) && index < count {
        return POSITIONS[count - 1][index];
    }
    (
        180 + i32::try_from(index).unwrap_or(0) * 28,
        180 - i32::try_from(index).unwrap_or(0) * 9,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battle_animation_clock_uses_original_forty_millisecond_frames() {
        assert_eq!(battle_animation_ticks(3), 0);
        assert_eq!(battle_animation_ticks(4), 1);
        assert_eq!(battle_animation_ticks(8), 2);
    }

    #[test]
    fn player_positions_match_original_one_to_three_member_layouts() {
        assert_eq!(player_position(1, 0), (240, 170));
        assert_eq!(player_position(2, 0), (200, 176));
        assert_eq!(player_position(2, 1), (256, 152));
        assert_eq!(player_position(3, 2), (270, 146));
        assert_eq!(player_position(4, 3), (264, 153));
    }

    #[test]
    fn action_offset_moves_out_and_returns_to_idle_position() {
        let offsets = (1..=ACTION_EVENT_TICKS)
            .rev()
            .map(action_offset)
            .collect::<Vec<_>>();
        assert_eq!(offsets, [0, 3, 6, 9, 12, 9, 6, 3]);
    }

    #[test]
    fn magic_blow_accumulates_with_the_requested_sign_and_resets_after_feedback() {
        let negative = (1..=ACTION_EVENT_TICKS)
            .rev()
            .map(|ticks| magic_blow_offset(-3, ticks))
            .collect::<Vec<_>>();
        let positive = (1..=ACTION_EVENT_TICKS)
            .rev()
            .map(|ticks| magic_blow_offset(2, ticks))
            .collect::<Vec<_>>();
        assert!(negative.windows(2).all(|pair| pair[1] <= pair[0]));
        assert!(positive.windows(2).all(|pair| pair[1] >= pair[0]));
        assert!(negative.last().is_some_and(|&offset| offset < 0));
        assert!(positive.last().is_some_and(|&offset| offset > 0));
        assert_eq!(magic_blow_offset(0, 1), 0);
    }

    #[test]
    fn scripted_magic_animation_moves_selected_player_then_shifts_entire_party() {
        let event = Some(BattleEvent::PlayerMagicAnimation { player: Some(1) });
        assert_eq!(
            player_magic_animation_state(event, 1, PLAYER_MAGIC_ANIMATION_EVENT_TICKS),
            (-4, -2, Some(0), 0)
        );
        assert_eq!(
            player_magic_animation_state(event, 1, PLAYER_MAGIC_ANIMATION_EVENT_TICKS - 6),
            (-10, -4, Some(5), 0)
        );
        assert_eq!(
            player_magic_animation_state(event, 1, PLAYER_MAGIC_ANIMATION_EVENT_TICKS - 17),
            (-10, -4, Some(6), 0)
        );
        assert_eq!(player_magic_animation_state(event, 0, 1), (0, 0, None, 8));
        assert_eq!(
            player_magic_animation_state(
                Some(BattleEvent::PlayerMagicAnimation { player: None }),
                0,
                1,
            ),
            (0, 0, None, 8)
        );
    }

    #[test]
    fn magic_and_item_actions_use_their_original_forward_poses() {
        assert_eq!(pre_magic_offset(0), (-4, -2));
        assert_eq!(pre_magic_offset(3), (-10, -4));
        assert_eq!(pre_magic_offset(30), (-10, -4));

        let use_item = Some(BattleEvent::PlayerUseItem {
            player: 0,
            item_object: 1,
            target: None,
            consuming: true,
        });
        let use_total = original_frames_to_ticks(17);
        assert_eq!(
            player_item_animation_state(use_item, 0, use_total),
            (0, 0, None, 0)
        );
        assert_eq!(
            player_item_animation_state(use_item, 1, use_total - 4),
            (0, 0, None, 1)
        );
        assert_eq!(
            player_item_animation_state(use_item, 0, use_total - 4),
            (-15, -7, Some(5), 1)
        );

        let throw_item = Some(BattleEvent::PlayerThrowItem {
            player: 0,
            item_object: 1,
            target: Some(0),
        });
        let throw_total = original_frames_to_ticks(16);
        assert_eq!(
            player_item_animation_state(throw_item, 0, throw_total),
            (-4, -2, None, 0)
        );
        assert_eq!(
            player_item_animation_state(throw_item, 0, throw_total - 6),
            (-10, -4, Some(5), 0)
        );
        assert_eq!(
            player_item_animation_state(throw_item, 0, throw_total - 12),
            (-10, -4, Some(6), 0)
        );
    }

    #[test]
    fn enemy_script_animations_follow_classic_frame_sequences() {
        let origin = BattlePosition { x: 0, y: 0 };
        let destination = BattlePosition { x: 100, y: 60 };
        assert_eq!(
            enemy_divide_position(origin, destination, 0),
            BattlePosition { x: 50, y: 30 }
        );
        assert_eq!(
            enemy_divide_position(origin, destination, 1),
            BattlePosition { x: 75, y: 45 }
        );
        assert_eq!(
            enemy_divide_position(origin, destination, 9),
            BattlePosition { x: 99, y: 59 }
        );
        assert_eq!(enemy_divide_position(origin, destination, 10), destination);

        let transform = BattleEvent::EnemyTransform {
            enemy: 1,
            previous_enemy_id: 7,
            previous_y_offset: 9,
        };
        assert_eq!(
            enemy_transform_sprite_state(transform, 1, ENEMY_TRANSFORM_EVENT_TICKS),
            Some((7, 9, 0))
        );
        assert_eq!(
            enemy_transform_sprite_state(transform, 1, ENEMY_TRANSFORM_EVENT_TICKS - 5),
            Some((7, 9, 0))
        );
        assert_eq!(
            enemy_transform_sprite_state(transform, 1, ENEMY_TRANSFORM_EVENT_TICKS - 6),
            None
        );
        assert_eq!(enemy_transform_visual_state(0), (64, 0));
        assert_eq!(enemy_transform_visual_state(5), (64, 5));
        assert_eq!(enemy_transform_visual_state(6), (0, 0));
        assert_eq!(enemy_transform_visual_state(35), (64, 0));

        let pre_ticks = 4;
        assert_eq!(enemy_summon_visual_state(pre_ticks, 0), (0, 8));
        assert_eq!(enemy_summon_visual_state(pre_ticks, 3), (0, 8));
        assert_eq!(enemy_summon_visual_state(pre_ticks, 4), (0, 8));
        assert_eq!(
            enemy_summon_visual_state(pre_ticks, pre_ticks + BATTLE_FADE_TICKS),
            (64, 8)
        );
        assert_eq!(enemy_summon_visual_state(pre_ticks, 63), (64, 1));
        assert_eq!(enemy_summon_visual_state(pre_ticks, 64), (64, 0));
    }

    #[test]
    fn battle_command_icons_match_the_original_cross_layout() {
        assert_eq!(
            BATTLE_COMMAND_ICONS,
            [(40, 27, 140), (41, 0, 155), (42, 54, 155), (43, 27, 170)]
        );
    }
}
