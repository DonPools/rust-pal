use pal_assets::battle::{BattleEffects, BattleSpriteArchive};
use pal_assets::bitmap::Bitmap;
use pal_assets::player_roles::PlayerRole;
use pal_assets::rle::RleBitmap;
use pal_assets::text::{BitmapFont, TextLibrary};
use pal_core::battle::{
    BattleEvent, BattleMagic, BattleMagicVisual, BattlePhase, BattleResult, BattleState,
    BattleTarget,
};

use super::battle_timing::{
    battle_magic_for_event, event_has_full_magic_visual, kept_effect_frame, magic_event_timeline,
    offensive_effect_frame_at, original_frames_to_ticks, timed_frame_at, timed_frames_to_ticks,
    MagicEventTimeline,
};
use super::battle_update::{ACTION_EVENT_TICKS, PLAYER_MAGIC_ANIMATION_EVENT_TICKS};
use super::draw::{draw_number, fill_rect, stroke_rect};
use crate::renderer::Renderer;

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
        role_id: u16,
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
    pub(super) result: BattleResult,
    pub(super) pages: Vec<BattleSettlementPage>,
    pub(super) page: usize,
}

impl PostBattlePresentation {
    pub(super) fn current_page(&self) -> Option<&BattleSettlementPage> {
        self.pages.get(self.page)
    }
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
        let frame = match event {
            Some(BattleEvent::EnemyMagic {
                enemy: caster,
                visual: true,
                ..
            }) if caster == index && enemy.magic_frames > 0 => {
                let elapsed =
                    ACTION_EVENT_TICKS.saturating_sub(event_ticks.min(ACTION_EVENT_TICKS));
                idle_frames
                    + usize::from(elapsed).min(usize::from(enemy.magic_frames).saturating_sub(1))
            }
            Some(
                BattleEvent::EnemyAttack { enemy: caster, .. }
                | BattleEvent::EnemyConfusedAttack { enemy: caster, .. },
            ) if caster == index && enemy.attack_frames > 0 => {
                let elapsed =
                    ACTION_EVENT_TICKS.saturating_sub(event_ticks.min(ACTION_EVENT_TICKS));
                let wait = enemy.action_wait_frames.max(1);
                idle_frames
                    + usize::from(enemy.magic_frames)
                    + usize::from(elapsed / wait).min(usize::from(enemy.attack_frames) - 1)
            }
            _ => usize::try_from(ticks / speed).unwrap_or(0) % idle_frames,
        };
        let Some(bitmap) = resources
            .enemy_sprites
            .decode_frame(usize::from(enemy.enemy_id), frame)
        else {
            continue;
        };
        let enemy_action_offset = match event {
            Some(BattleEvent::EnemyAttack { enemy, .. }) if enemy == index => {
                action_offset(event_ticks)
            }
            Some(BattleEvent::EnemyMagic {
                enemy,
                visual: true,
                ..
            }) if enemy == index => action_offset(event_ticks) / 2,
            Some(BattleEvent::EnemyConfusedAttack { enemy, .. }) if enemy == index => {
                action_offset(event_ticks)
            }
            _ => 0,
        };
        let enemy_blow_offset = match event {
            Some(
                BattleEvent::PlayerMagic { blow, .. } | BattleEvent::SimulatedMagic { blow, .. },
            ) => magic_blow_offset(blow, event_ticks),
            _ => 0,
        };
        let enemy_offset = enemy_action_offset + enemy_blow_offset;
        let x = i32::from(enemy.position.x) - i32::from(bitmap.width) / 2 + enemy_offset;
        let y = i32::from(enemy.position.y) + i32::from(enemy.y_offset) - i32::from(bitmap.height)
            + enemy_offset / 2;
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
        if event.is_none()
            && targeting_enemy
            && index == selected_enemy
            && battle.phase() == BattlePhase::AwaitingCommand
            && ticks & 1 != 0
        {
            renderer.blit_rle_color_shift(&bitmap, x, y, 7);
        } else if is_hit && feedback_active && event_ticks.is_multiple_of(2) {
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
        if let Some(BattleEvent::EnemyMagic { blow, .. }) = event {
            let offset = magic_blow_offset(blow, event_ticks);
            x += offset;
            y += offset / 2;
        }
        match event {
            Some(BattleEvent::PlayerAttack { player, .. }) if player == index => {
                let offset = action_offset(event_ticks);
                x -= offset;
                y -= offset / 2;
            }
            Some(BattleEvent::PlayerConfusedAttack { player, .. }) if player == index => {
                let offset = action_offset(event_ticks);
                x -= offset;
                y -= offset / 2;
            }
            Some(BattleEvent::PlayerMagic {
                player,
                visual: true,
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
                player: target,
                protected_by: Some(cover),
                ..
            }) if cover == index => {
                let (target_x, target_y) = player_position(battle.players.len(), target);
                let offset = action_offset(event_ticks).min(12);
                x += (target_x - 24 - x) * offset / 12;
                y += (target_y - 12 - y) * offset / 12;
            }
            Some(BattleEvent::PlayerFlee {
                player: fleeing,
                succeeded,
            }) if succeeded || fleeing == index => {
                let elapsed =
                    ACTION_EVENT_TICKS.saturating_sub(event_ticks.min(ACTION_EVENT_TICKS));
                x += i32::from(elapsed) * if index == 2 { 6 } else { 4 };
                y += i32::from(elapsed)
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
            Some(BattleEvent::PlayerAttack { player, .. }) if player == index => {
                Some(if event_ticks > ACTION_EVENT_TICKS / 2 {
                    8
                } else {
                    9
                })
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
                auto_defended: true,
                ..
            }) if player == index => Some(3),
            Some(BattleEvent::EnemyMagic { player, .. }) if player == index => Some(4),
            Some(BattleEvent::PlayerMagic {
                player,
                visual: true,
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
                Some(BattleEvent::EnemyMagic { player, .. }) => player == index,
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
            renderer.apply_wave(visual.wave, i16::try_from(ticks).unwrap_or(i16::MAX));
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
        BattleEvent::PlayerAttack { player, enemy, .. } => {
            let actor = battle.players.get(player)?;
            let bitmap = effects.player_attack_frame(actor.battle_sprite_num, action_phase)?;
            let (x, y) = enemy_position(battle, enemy)?;
            (bitmap, x, y - 10)
        }
        BattleEvent::PlayerConfusedAttack { player, target, .. } => {
            let actor = battle.players.get(player)?;
            let bitmap = effects.player_attack_frame(actor.battle_sprite_num, action_phase)?;
            let (x, y) = player_position(battle.players.len(), target);
            (bitmap, x, y - 20)
        }
        BattleEvent::EnemyAttack { player, .. } => {
            let bitmap = effects.enemy_attack_frame(action_phase)?;
            let (x, y) = player_position(battle.players.len(), player);
            (bitmap, x - 12, y - 20)
        }
        BattleEvent::EnemyConfusedAttack { target, .. } => {
            let bitmap = effects.enemy_attack_frame(action_phase)?;
            let (x, y) = enemy_position(battle, target)?;
            (bitmap, x, y - 10)
        }
        BattleEvent::PlayerMagic {
            player,
            visual: true,
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
    if magic.magic_type == 9
        && elapsed >= timeline.body_start()
        && elapsed < timeline.effect_start()
    {
        if behind_fighters {
            return;
        }
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
        let frame = timed_frame_at(
            elapsed.saturating_sub(timeline.body_start()),
            frame_count,
            magic.speed,
        );
        if let Some(bitmap) = player_sprites.decode_frame(sprite, frame) {
            renderer.blit_rle(
                &bitmap,
                240 + i32::from(magic.x_offset) - i32::from(bitmap.width) / 2,
                165 + i32::from(magic.y_offset) - i32::from(bitmap.height),
            );
        }
        return;
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
    let timeline = magic_event_timeline(event, magic, effect_frame_count, summon_frame_count);
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
            let total = original_frames_to_ticks(25);
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
            let total = original_frames_to_ticks(24);
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
        BattleEvent::PlayerUseItem { item_object, .. } => (item_object, 25, 4..17),
        BattleEvent::PlayerThrowItem { item_object, .. } => (item_object, 24, 4..16),
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
        | BattleEvent::PlayerMagic { enemy, damage, .. }
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
        BattleEvent::EnemyAttack {
            auto_defended: true,
            ..
        } => return,
        BattleEvent::EnemyAttack { player, damage, .. }
        | BattleEvent::EnemyMagic { player, damage, .. }
        | BattleEvent::PlayerConfusedAttack {
            target: player,
            damage,
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
        | BattleEvent::PlayerFlee { .. }
        | BattleEvent::PlayerDefend { .. }
        | BattleEvent::PlayerDefensiveMagic { .. }
        | BattleEvent::PlayerMagicAnimation { .. }
        | BattleEvent::PlayerFriendDeath { .. }
        | BattleEvent::PlayerDying { .. }
        | BattleEvent::RoundCompleted
        | BattleEvent::Finished(_) => return,
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
    fill_rect(renderer, 76, 58, 168, 74, [8, 16, 32, 245]);
    stroke_rect(renderer, 76, 58, 168, 74, [224, 216, 168, 255]);
    if let Some(label) = text.word(30) {
        renderer.draw_big5_text_shadowed(font, label, 94, 70, 0x4f);
    }
    draw_ui_number(
        renderer,
        ui_sprites,
        rewards.experience,
        5,
        182,
        74,
        19,
        [240, 224, 96, 255],
    );
    if let Some(label) = text.word(9) {
        renderer.draw_big5_text_shadowed(font, label, 78, 108, 0x4f);
    }
    draw_ui_number(
        renderer,
        ui_sprites,
        rewards.cash,
        5,
        144,
        112,
        19,
        [240, 224, 96, 255],
    );
    if let Some(label) = text.word(10) {
        renderer.draw_big5_text_shadowed(font, label, 198, 108, 0x4f);
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
        BattleSettlementPage::LevelUp {
            role_id,
            before,
            after,
        } => {
            fill_rect(renderer, 70, 4, 180, 188, [8, 16, 32, 250]);
            stroke_rect(renderer, 70, 4, 180, 188, [224, 216, 168, 255]);
            if let Some(name) = text.word(usize::from(after.name_word_id)) {
                renderer.draw_big5_text_shadowed(font, name, 86, 12, 0x4f);
            }
            if let Some(level) = text.word(48) {
                renderer.draw_big5_text_shadowed(font, level, 142, 12, 0xbb);
            }
            if let Some(label) = text.word(32) {
                renderer.draw_big5_text_shadowed(font, label, 192, 12, 0x2d);
            }
            let rows = [
                (48usize, before.level, after.level),
                (49, before.max_hp, after.max_hp),
                (50, before.max_mp, after.max_mp),
                (51, before.attack_strength, after.attack_strength),
                (52, before.magic_strength, after.magic_strength),
                (53, before.defense, after.defense),
                (54, before.dexterity, after.dexterity),
                (55, before.flee_rate, after.flee_rate),
            ];
            for (row, (label, old, new)) in rows.into_iter().enumerate() {
                let y = 40 + i32::try_from(row).unwrap_or(0) * 18;
                if let Some(label) = text.word(label) {
                    renderer.draw_big5_text_shadowed(font, label, 86, y, 0xbb);
                }
                draw_ui_number(
                    renderer,
                    ui_sprites,
                    u32::from(old),
                    4,
                    140,
                    y + 3,
                    19,
                    [240, 224, 96, 255],
                );
                if let Some(arrow) = ui_sprites.get(47) {
                    renderer.blit_rle(arrow, 174, y + 1);
                }
                draw_ui_number(
                    renderer,
                    ui_sprites,
                    u32::from(new),
                    4,
                    196,
                    y + 3,
                    19,
                    [240, 224, 96, 255],
                );
            }
            let _ = role_id;
        }
        BattleSettlementPage::AttributeGrowth {
            role_id,
            label,
            amount,
        } => {
            fill_rect(renderer, 72, 60, 176, 52, [8, 16, 32, 250]);
            stroke_rect(renderer, 72, 60, 176, 52, [224, 216, 168, 255]);
            let name_word = presentation
                .battle
                .players
                .iter()
                .find(|player| player.role_id == *role_id)
                .map(|player| player.name_word_id);
            if let Some(name) = name_word.and_then(|word| text.word(usize::from(word))) {
                renderer.draw_big5_text_shadowed(font, name, 84, 72, 0x4f);
            }
            if let Some(label) = text.word(*label) {
                renderer.draw_big5_text_shadowed(font, label, 132, 72, 0xbb);
            }
            if let Some(up) = text.word(32) {
                renderer.draw_big5_text_shadowed(font, up, 184, 72, 0x2d);
            }
            draw_ui_number(
                renderer,
                ui_sprites,
                u32::from(*amount),
                3,
                190,
                94,
                19,
                [240, 224, 96, 255],
            );
        }
        BattleSettlementPage::LearnedMagic {
            role_id,
            magic_object,
        } => {
            fill_rect(renderer, 54, 94, 212, 42, [8, 16, 32, 250]);
            stroke_rect(renderer, 54, 94, 212, 42, [224, 216, 168, 255]);
            let name_word = presentation
                .battle
                .players
                .iter()
                .find(|player| player.role_id == *role_id)
                .map(|player| player.name_word_id);
            if let Some(name) = name_word.and_then(|word| text.word(usize::from(word))) {
                renderer.draw_big5_text_shadowed(font, name, 66, 108, 0x4f);
            }
            if let Some(label) = text.word(33) {
                renderer.draw_big5_text_shadowed(font, label, 118, 108, 0x4f);
            }
            if let Some(magic) = text.word(usize::from(*magic_object)) {
                renderer.draw_big5_text_shadowed(font, magic, 174, 108, 0x1b);
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
    ticks: u64,
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
            use pal_core::battle::BattleStatus;
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
        let arrow = if ticks & 1 == 0 { 66 } else { 67 };
        if let Some(sprite) = ui_sprites.get(arrow) {
            renderer.blit_rle(sprite, x - 8, y - 67);
        }
    } else if !matches!(menu, BattleMenuState::TargetEnemy { .. }) {
        let Some(active) = battle.active_player() else {
            return;
        };
        let (x, y) = player_position(battle.players.len(), active);
        let arrow = if ticks & 1 == 0 { 68 } else { 69 };
        if let Some(sprite) = ui_sprites.get(arrow) {
            renderer.blit_rle(sprite, x - 8, y - 74);
        }
    }

    let cooperative_magic_enabled = battle.can_use_cooperative_magic();
    for (index, (sprite_index, x, y)) in BATTLE_COMMAND_ICONS.into_iter().enumerate() {
        let Some(sprite) = ui_sprites.get(sprite_index) else {
            continue;
        };
        let enabled = index != 2 || cooperative_magic_enabled;
        if matches!(menu, BattleMenuState::Main) && index == selected_command && enabled {
            renderer.blit_rle(sprite, x, y);
        } else {
            renderer.blit_rle_mono(sprite, x, y, 0, -4);
        }
    }

    match menu {
        BattleMenuState::Main | BattleMenuState::TargetEnemy { .. } => {}
        BattleMenuState::Magic { selected } => {
            fill_rect(renderer, 5, 42, 310, 96, [8, 16, 32, 235]);
            stroke_rect(renderer, 5, 42, 310, 96, [184, 196, 224, 255]);
            let Some(player) = battle
                .active_player()
                .and_then(|active| battle.players.get(active))
            else {
                return;
            };
            for (index, magic) in player.magics.iter().enumerate() {
                let column = index % 3;
                let row = (index / 3) % 5;
                let page = selected / 15;
                if index / 15 != page {
                    continue;
                }
                let x = 18 + i32::try_from(column).unwrap_or(0) * 100;
                let y = 51 + i32::try_from(row).unwrap_or(0) * 17;
                if let Some(name) = text.word(usize::from(magic.object_id)) {
                    let enabled = player.mp >= magic.mp_cost;
                    let color = match (index == selected, enabled) {
                        (true, true) => {
                            if ticks & 1 == 0 {
                                0x2d
                            } else {
                                0x4f
                            }
                        }
                        (true, false) => 0x1c,
                        (false, true) => 0x4f,
                        (false, false) => 0x18,
                    };
                    renderer.draw_big5_text_shadowed(font, name, x, y, color);
                }
                if index == selected {
                    draw_ui_number(
                        renderer,
                        ui_sprites,
                        u32::from(magic.mp_cost),
                        3,
                        276,
                        127,
                        19,
                        [240, 224, 96, 255],
                    );
                }
            }
        }
        BattleMenuState::Misc { selected } => {
            render_word_menu(
                renderer,
                text,
                font,
                &[56, 57, 58, 59, 60],
                selected,
                2,
                20,
                ticks,
            );
        }
        BattleMenuState::ItemSubmenu { selected } => {
            render_word_menu(renderer, text, font, &[23, 24], selected, 30, 50, ticks);
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
        BattleMenuState::Status { selected } => {
            render_battle_player_status(renderer, battle, text, font, ui_sprites, selected);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_word_menu(
    renderer: &mut Renderer,
    text: &TextLibrary,
    font: &BitmapFont,
    words: &[usize],
    selected: usize,
    x: i32,
    y: i32,
    ticks: u64,
) {
    let height = i32::try_from(words.len()).unwrap_or(0) * 18 + 12;
    fill_rect(renderer, x, y, 96, height, [8, 16, 32, 240]);
    stroke_rect(renderer, x, y, 96, height, [184, 196, 224, 255]);
    for (index, &word) in words.iter().enumerate() {
        let Some(label) = text.word(word) else {
            continue;
        };
        let color = if index == selected {
            if ticks & 1 == 0 {
                0x2d
            } else {
                0x4f
            }
        } else {
            0x4f
        };
        renderer.draw_big5_text_shadowed(
            font,
            label,
            x + 14,
            y + 7 + i32::try_from(index).unwrap_or(0) * 18,
            color,
        );
    }
}

fn render_battle_player_status(
    renderer: &mut Renderer,
    battle: &BattleState,
    text: &TextLibrary,
    font: &BitmapFont,
    ui_sprites: &[RleBitmap],
    selected: usize,
) {
    let Some(player) = battle.players.get(selected) else {
        return;
    };
    fill_rect(renderer, 62, 12, 196, 142, [8, 16, 32, 248]);
    stroke_rect(renderer, 62, 12, 196, 142, [240, 224, 96, 255]);
    if let Some(name) = text.word(usize::from(player.name_word_id)) {
        renderer.draw_big5_text_shadowed(font, name, 82, 23, 0x4f);
    }
    for (row, (label, value)) in [
        (48usize, player.level),
        (49, player.hp),
        (50, player.mp),
        (51, player.attack_strength),
        (52, player.magic_strength),
        (53, player.defense),
        (54, player.dexterity),
        (55, player.flee_rate),
    ]
    .into_iter()
    .enumerate()
    {
        let y = 43 + i32::try_from(row).unwrap_or(0) * 13;
        if let Some(label) = text.word(label) {
            renderer.draw_big5_text_shadowed(font, label, 84, y, 0xbb);
        }
        draw_ui_number(
            renderer,
            ui_sprites,
            u32::from(value),
            4,
            220,
            y + 2,
            19,
            [240, 224, 96, 255],
        );
    }
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
            player_magic_animation_state(event, 1, 22),
            (-4, -2, Some(0), 0)
        );
        assert_eq!(
            player_magic_animation_state(event, 1, 16),
            (-10, -4, Some(5), 0)
        );
        assert_eq!(
            player_magic_animation_state(event, 1, 5),
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
        let use_total = original_frames_to_ticks(25);
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
        let throw_total = original_frames_to_ticks(24);
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
    fn battle_command_icons_match_the_original_cross_layout() {
        assert_eq!(
            BATTLE_COMMAND_ICONS,
            [(40, 27, 140), (41, 0, 155), (42, 54, 155), (43, 27, 170)]
        );
    }
}
