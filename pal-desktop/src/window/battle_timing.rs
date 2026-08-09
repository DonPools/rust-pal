use std::collections::VecDeque;

use pal_assets::battle::MAX_ENEMIES_IN_TEAM;
use pal_core::battle::{BattleEvent, BattleMagic, BattleMagicVisual, BattleState, MagicEventPhase};
use pal_core::game::BATTLE_FRAME_MS;

const OFFENSIVE_FEEDBACK_FRAMES: usize = 9;
const DEFENSIVE_COLOR_SHIFT_FRAMES: usize = 22;
const COOPERATIVE_FEEDBACK_FRAMES: usize = 15;
const ENEMY_MAGIC_FEEDBACK_FRAMES: usize = 14;
const NORMAL_PRE_MAGIC_FRAMES: usize = 17;
const SUMMON_PRE_MAGIC_FRAMES: usize = 7;
const SUMMON_BRIGHTEN_FRAMES: usize = 10;
const COOPERATIVE_PRE_MAGIC_FRAMES: usize = 20;
pub(super) const BATTLE_FADE_TICKS: u16 = 29;

fn follows_enemy_feedback_event(first: BattleEvent, next: BattleEvent) -> bool {
    match (first, next) {
        (
            BattleEvent::PlayerAttack {
                player,
                visual: true,
                ..
            },
            BattleEvent::PlayerAttack {
                player: next_player,
                visual: false,
                ..
            },
        ) => player == next_player,
        (
            BattleEvent::PlayerMagic {
                player,
                magic_object,
                phase: MagicEventPhase::Feedback,
                visual: true,
                ..
            },
            BattleEvent::PlayerMagic {
                player: next_player,
                magic_object: next_magic,
                phase: MagicEventPhase::Feedback,
                visual: false,
                ..
            },
        ) => player == next_player && magic_object == next_magic,
        (
            BattleEvent::EnemyMagic {
                enemy,
                magic_object,
                phase: MagicEventPhase::Feedback,
                visual: true,
                ..
            },
            BattleEvent::EnemyMagic {
                enemy: next_enemy,
                magic_object: next_magic,
                phase: MagicEventPhase::Feedback,
                visual: false,
                ..
            },
        ) => enemy == next_enemy && magic_object == next_magic,
        (
            BattleEvent::PlayerCooperativeMagic {
                player,
                magic_object,
                visual: true,
                ..
            },
            BattleEvent::PlayerCooperativeMagic {
                player: next_player,
                magic_object: next_magic,
                visual: false,
                ..
            },
        ) => player == next_player && magic_object == next_magic,
        (
            BattleEvent::SimulatedMagic {
                magic,
                visual: true,
                ..
            },
            BattleEvent::SimulatedMagic {
                magic: next_magic,
                visual: false,
                ..
            },
        ) => magic.object_id == next_magic.object_id,
        _ => false,
    }
}

pub(super) fn battle_event_group(events: &VecDeque<BattleEvent>) -> Vec<BattleEvent> {
    let Some(&first) = events.front() else {
        return Vec::new();
    };
    let mut group = Vec::with_capacity(MAX_ENEMIES_IN_TEAM);
    group.push(first);
    for &event in events.iter().skip(1).take(MAX_ENEMIES_IN_TEAM - 1) {
        if !follows_enemy_feedback_event(first, event) {
            break;
        }
        group.push(event);
    }
    group
}

pub(super) fn battle_event_group_timing_event(group: &[BattleEvent]) -> Option<BattleEvent> {
    let mut event = *group.first()?;
    let defeated = group.iter().copied().any(enemy_defeated);
    match &mut event {
        BattleEvent::PlayerAttack {
            defeated: value, ..
        }
        | BattleEvent::PlayerMagic {
            defeated: value, ..
        }
        | BattleEvent::PlayerCooperativeMagic {
            defeated: value, ..
        }
        | BattleEvent::SimulatedMagic {
            defeated: value, ..
        } => *value = defeated,
        _ => {}
    }
    Some(event)
}

pub(super) fn enemy_feedback_target(event: BattleEvent) -> Option<usize> {
    match event {
        BattleEvent::PlayerAttack { enemy, .. }
        | BattleEvent::PlayerMagic {
            enemy,
            phase: MagicEventPhase::Feedback,
            ..
        }
        | BattleEvent::PlayerCooperativeMagic { enemy, .. }
        | BattleEvent::SimulatedMagic { enemy, .. } => Some(enemy),
        BattleEvent::EnemyConfusedAttack { target, .. } => Some(target),
        _ => None,
    }
}

pub(super) fn player_feedback_target(event: BattleEvent) -> Option<usize> {
    match event {
        BattleEvent::EnemyMagic {
            player,
            phase: MagicEventPhase::Feedback,
            ..
        } => Some(player),
        _ => None,
    }
}

pub(super) fn pending_enemy_feedback_mask(events: &VecDeque<BattleEvent>) -> u8 {
    events.iter().fold(0, |mask, &event| {
        enemy_feedback_target(event).map_or(mask, |enemy| {
            u32::try_from(enemy)
                .ok()
                .and_then(|shift| 1u8.checked_shl(shift))
                .map_or(mask, |bit| mask | bit)
        })
    })
}

pub(super) fn enemy_feedback_timing_event(
    timing_event: BattleEvent,
    target_event: BattleEvent,
) -> BattleEvent {
    match (timing_event, target_event) {
        (
            BattleEvent::PlayerAttack {
                visual, defeated, ..
            },
            BattleEvent::PlayerAttack {
                player,
                enemy,
                damage,
                critical,
                ..
            },
        ) => BattleEvent::PlayerAttack {
            player,
            enemy,
            damage,
            critical,
            visual,
            defeated,
        },
        _ => target_event,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct EnemyEscapeTimeline {
    movement_steps: u32,
    pub(super) total_ticks: u16,
}

/// Classic moves every enemy five pixels left every 10 ms, then waits 500 ms.
pub(super) fn enemy_escape_timeline(rightmost_edge: i32) -> EnemyEscapeTimeline {
    let movement_steps = u32::try_from(rightmost_edge.max(0))
        .unwrap_or(0)
        .div_ceil(5)
        .max(1);
    let duration_ms = u64::from(movement_steps)
        .saturating_mul(10)
        .saturating_add(500);
    EnemyEscapeTimeline {
        movement_steps,
        total_ticks: battle_milliseconds_to_ticks(duration_ms),
    }
}

pub(super) fn enemy_escape_offset(timeline: EnemyEscapeTimeline, ticks_remaining: u16) -> i32 {
    let elapsed_ticks = timeline
        .total_ticks
        .saturating_sub(ticks_remaining.min(timeline.total_ticks));
    let elapsed_steps = u32::from(elapsed_ticks)
        .saturating_mul(u32::try_from(BATTLE_FRAME_MS / 10).unwrap_or(0))
        .min(timeline.movement_steps);
    -i32::try_from(elapsed_steps.saturating_mul(5)).unwrap_or(i32::MAX)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct MagicEventTimeline {
    pub(super) pre_ticks: u16,
    pub(super) brighten_ticks: u16,
    pub(super) summon_fade_in_ticks: u16,
    pub(super) body_ticks: u16,
    pub(super) effect_ticks: u16,
    pub(super) tail_ticks: u16,
    pub(super) death_fade_ticks: u16,
    pub(super) summon_fade_out_ticks: u16,
    pub(super) total_ticks: u16,
}

impl MagicEventTimeline {
    pub(super) fn effect_start(self) -> u16 {
        self.pre_ticks
            .saturating_add(self.brighten_ticks)
            .saturating_add(self.summon_fade_in_ticks)
            .saturating_add(self.body_ticks)
    }

    pub(super) fn body_start(self) -> u16 {
        self.pre_ticks
            .saturating_add(self.brighten_ticks)
            .saturating_add(self.summon_fade_in_ticks)
    }

    pub(super) fn tail_start(self) -> u16 {
        self.effect_start().saturating_add(self.effect_ticks)
    }
}

pub(super) fn battle_magic_for_event(
    battle: &BattleState,
    event: BattleEvent,
) -> Option<BattleMagic> {
    match event {
        BattleEvent::PlayerMagic {
            player,
            magic_object,
            ..
        } => battle
            .players
            .get(player)?
            .magics
            .iter()
            .copied()
            .find(|magic| magic.object_id == magic_object),
        BattleEvent::PlayerCooperativeMagic {
            player,
            magic_object,
            ..
        } => battle
            .players
            .get(player)?
            .cooperative_magic
            .filter(|magic| magic.object_id == magic_object),
        BattleEvent::EnemyMagic {
            enemy,
            magic_object,
            ..
        } => battle
            .enemies
            .get(enemy)?
            .magic
            .filter(|magic| magic.object_id == magic_object),
        BattleEvent::PlayerDefensiveMagic {
            player,
            magic_object,
            ..
        } => battle
            .players
            .get(player)?
            .magics
            .iter()
            .copied()
            .find(|magic| magic.object_id == magic_object),
        BattleEvent::SimulatedMagic { magic, .. } => Some(magic),
        _ => None,
    }
}

pub(super) fn event_has_full_magic_visual(event: BattleEvent) -> bool {
    match event {
        BattleEvent::PlayerMagic { phase, .. } | BattleEvent::EnemyMagic { phase, .. } => {
            phase == MagicEventPhase::Visual
        }
        BattleEvent::PlayerCooperativeMagic { visual, .. }
        | BattleEvent::SimulatedMagic { visual, .. } => visual,
        BattleEvent::PlayerDefensiveMagic { .. } => true,
        _ => false,
    }
}

pub(super) fn magic_event_timeline(
    event: BattleEvent,
    magic: BattleMagic,
    effect_frame_count: Option<usize>,
    summon_frame_count: Option<usize>,
    enemy_pre_frames: usize,
) -> MagicEventTimeline {
    let death_fade_ticks = if enemy_defeated(event) {
        BATTLE_FADE_TICKS
    } else {
        0
    };
    if !event_has_full_magic_visual(event) {
        let feedback_frames = match event {
            BattleEvent::PlayerMagic {
                phase: MagicEventPhase::Feedback,
                visual: true,
                ..
            } => OFFENSIVE_FEEDBACK_FRAMES,
            BattleEvent::EnemyMagic {
                phase: MagicEventPhase::Feedback,
                visual: true,
                ..
            } => ENEMY_MAGIC_FEEDBACK_FRAMES,
            _ => 1,
        };
        let tail_ticks = original_frames_to_ticks(feedback_frames);
        let total_ticks = tail_ticks.saturating_add(death_fade_ticks).max(1);
        return MagicEventTimeline {
            pre_ticks: 0,
            brighten_ticks: 0,
            summon_fade_in_ticks: 0,
            body_ticks: 0,
            effect_ticks: 0,
            tail_ticks,
            death_fade_ticks,
            summon_fade_out_ticks: 0,
            total_ticks,
        };
    }

    let is_defensive = matches!(event, BattleEvent::PlayerDefensiveMagic { .. });
    let is_summon = magic.magic_type == 9 && magic.summon_effect.is_some();
    let pre_frames = match event {
        BattleEvent::PlayerMagic {
            phase: MagicEventPhase::Visual,
            ..
        } if is_summon => SUMMON_PRE_MAGIC_FRAMES,
        BattleEvent::PlayerMagic {
            phase: MagicEventPhase::Visual,
            ..
        }
        | BattleEvent::PlayerDefensiveMagic { .. } => NORMAL_PRE_MAGIC_FRAMES,
        BattleEvent::PlayerCooperativeMagic { .. } => COOPERATIVE_PRE_MAGIC_FRAMES,
        BattleEvent::EnemyMagic {
            phase: MagicEventPhase::Visual,
            ..
        } => enemy_pre_frames,
        _ => 0,
    };
    let pre_ticks = original_frames_to_ticks(pre_frames);
    let brighten_ticks = if is_summon {
        original_frames_to_ticks(SUMMON_BRIGHTEN_FRAMES)
    } else {
        0
    };
    let summon_fade_in_ticks = if is_summon { BATTLE_FADE_TICKS } else { 0 };
    let body_ticks = if is_summon {
        timed_frames_to_ticks(
            summon_frame_count.unwrap_or(0).saturating_sub(1),
            magic.speed,
        )
    } else {
        0
    };
    let effect_visual = magic.effect_visual();
    let effect_frames = if is_defensive {
        effect_frame_count.unwrap_or(0)
    } else {
        offensive_effect_frame_count(effect_frame_count.unwrap_or(0), effect_visual)
    };
    let effect_ticks = timed_frames_to_ticks(effect_frames, effect_visual.speed);
    let tail_frames = match event {
        BattleEvent::PlayerMagic {
            phase: MagicEventPhase::Visual,
            ..
        }
        | BattleEvent::EnemyMagic {
            phase: MagicEventPhase::Visual,
            ..
        } => 0,
        BattleEvent::PlayerDefensiveMagic { .. } => DEFENSIVE_COLOR_SHIFT_FRAMES,
        BattleEvent::PlayerCooperativeMagic { .. } => COOPERATIVE_FEEDBACK_FRAMES,
        BattleEvent::EnemyMagic { .. } => ENEMY_MAGIC_FEEDBACK_FRAMES,
        _ => OFFENSIVE_FEEDBACK_FRAMES,
    };
    let tail_ticks = original_frames_to_ticks(tail_frames);
    let summon_fade_out_ticks = if is_summon { BATTLE_FADE_TICKS } else { 0 };
    let total_ticks = pre_ticks
        .saturating_add(brighten_ticks)
        .saturating_add(summon_fade_in_ticks)
        .saturating_add(body_ticks)
        .saturating_add(effect_ticks)
        .saturating_add(tail_ticks)
        .saturating_add(death_fade_ticks)
        .saturating_add(summon_fade_out_ticks)
        .max(1);
    MagicEventTimeline {
        pre_ticks,
        brighten_ticks,
        summon_fade_in_ticks,
        body_ticks,
        effect_ticks,
        tail_ticks,
        death_fade_ticks,
        summon_fade_out_ticks,
        total_ticks,
    }
}

pub(super) fn enemy_magic_pre_frames(
    battle: &BattleState,
    event: BattleEvent,
    magic: BattleMagic,
) -> usize {
    let BattleEvent::EnemyMagic {
        enemy,
        visual: true,
        ..
    } = event
    else {
        return 0;
    };
    let Some(enemy) = battle.enemies.get(enemy) else {
        return 0;
    };
    let wait = usize::from(enemy.action_wait_frames.max(1));
    let casting = usize::from(enemy.magic_frames).saturating_mul(wait).max(1);
    let attack = if magic.effect_visual().fire_delay == 0 {
        usize::from(enemy.attack_frames)
            .saturating_add(1)
            .saturating_mul(wait)
    } else {
        0
    };
    2usize.saturating_add(casting).saturating_add(attack)
}

pub(super) fn enemy_attack_frames(battle: &BattleState, event: BattleEvent) -> usize {
    let (BattleEvent::EnemyAttack { enemy, .. } | BattleEvent::EnemyConfusedAttack { enemy, .. }) =
        event
    else {
        return 0;
    };
    let Some(enemy) = battle.enemies.get(enemy) else {
        return 0;
    };
    if matches!(event, BattleEvent::EnemyConfusedAttack { .. }) {
        return 17;
    }
    let magic_frames = usize::from(enemy.magic_frames);
    let startup = magic_frames
        .saturating_mul(2)
        .saturating_add(3usize.saturating_sub(magic_frames))
        .saturating_add(1);
    let attack = if enemy.attack_frames == 0 {
        2
    } else {
        usize::from(enemy.attack_frames)
            .saturating_add(1)
            .saturating_mul(usize::from(enemy.action_wait_frames.max(1)))
    };
    startup.saturating_add(attack).saturating_add(11)
}

pub(super) fn player_attack_ticks(event: BattleEvent) -> u16 {
    let BattleEvent::PlayerAttack {
        visual, defeated, ..
    } = event
    else {
        return 0;
    };
    let action = original_frames_to_ticks(if visual { 16 } else { 1 });
    action.saturating_add(if defeated { BATTLE_FADE_TICKS } else { 0 })
}

pub(super) fn enemy_defeated(event: BattleEvent) -> bool {
    matches!(
        event,
        BattleEvent::PlayerAttack { defeated: true, .. }
            | BattleEvent::PlayerMagic { defeated: true, .. }
            | BattleEvent::PlayerCooperativeMagic { defeated: true, .. }
            | BattleEvent::SimulatedMagic { defeated: true, .. }
            | BattleEvent::EnemyConfusedAttack { defeated: true, .. }
    )
}

pub(super) fn original_frames_to_ticks(frames: usize) -> u16 {
    battle_milliseconds_to_ticks((frames as u64).saturating_mul(BATTLE_FRAME_MS))
}

pub(super) fn timed_frames_to_ticks(frames: usize, speed: i16) -> u16 {
    battle_milliseconds_to_ticks((frames as u64).saturating_mul(frame_time_ms(speed)))
}

pub(super) fn timed_frame_at(local_tick: u16, frame_count: usize, speed: i16) -> usize {
    if frame_count == 0 {
        return 0;
    }
    let elapsed_ms = u64::from(local_tick).saturating_mul(BATTLE_FRAME_MS);
    usize::try_from(elapsed_ms / frame_time_ms(speed))
        .unwrap_or(usize::MAX)
        .min(frame_count - 1)
}

pub(super) fn offensive_effect_frame_count(frame_count: usize, visual: BattleMagicVisual) -> usize {
    if frame_count == 0 {
        return 0;
    }
    let fire_delay = usize::from(visual.fire_delay).min(frame_count);
    frame_count
        .saturating_add(
            frame_count
                .saturating_sub(fire_delay)
                .saturating_mul(usize::from(visual.effect_times)),
        )
        .saturating_add(usize::from(visual.shake))
}

pub(super) fn offensive_effect_frame_at(
    local_tick: u16,
    frame_count: usize,
    visual: BattleMagicVisual,
) -> usize {
    if frame_count == 0 {
        return 0;
    }
    let sequence_frames = offensive_effect_frame_count(frame_count, visual).max(1);
    let sequence_index = timed_frame_at(local_tick, sequence_frames, visual.speed);
    let non_shake_frames = sequence_frames.saturating_sub(usize::from(visual.shake));
    let sequence_index = sequence_index.min(non_shake_frames.saturating_sub(1));
    if sequence_index < frame_count {
        return sequence_index;
    }
    let fire_delay = usize::from(visual.fire_delay).min(frame_count.saturating_sub(1));
    let repeating = frame_count.saturating_sub(fire_delay).max(1);
    fire_delay + (sequence_index - fire_delay) % repeating
}

pub(super) fn kept_effect_frame(frame_count: usize, visual: BattleMagicVisual) -> usize {
    if frame_count == 0 {
        return 0;
    }
    let sequence_frames = offensive_effect_frame_count(frame_count, visual);
    let non_shake_last = sequence_frames
        .saturating_sub(usize::from(visual.shake))
        .saturating_sub(1);
    if non_shake_last < frame_count {
        return non_shake_last;
    }
    let fire_delay = usize::from(visual.fire_delay).min(frame_count.saturating_sub(1));
    let repeating = frame_count.saturating_sub(fire_delay).max(1);
    fire_delay + (non_shake_last - fire_delay) % repeating
}

pub(super) fn effect_sound_elapsed_tick(
    timeline: MagicEventTimeline,
    visual: BattleMagicVisual,
    additional_frames: usize,
) -> u16 {
    timeline
        .effect_start()
        .saturating_add(timed_frames_to_ticks(
            usize::from(visual.fire_delay).saturating_add(additional_frames),
            visual.speed,
        ))
}

fn frame_time_ms(speed: i16) -> u64 {
    u64::try_from((i32::from(speed) + 5).max(1)).unwrap_or(1) * 10
}

pub(super) fn battle_milliseconds_to_ticks(milliseconds: u64) -> u16 {
    u16::try_from(milliseconds.div_ceil(BATTLE_FRAME_MS)).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visual() -> BattleMagicVisual {
        BattleMagicVisual {
            object_id: 1,
            flags: 0,
            effect: 2,
            magic_type: 0,
            x_offset: 0,
            y_offset: 0,
            specific: 0,
            speed: 0,
            keep_effect: 0,
            fire_delay: 2,
            effect_times: 3,
            shake: 4,
            wave: 0,
            sound: 0,
        }
    }

    #[test]
    fn offensive_sequence_uses_fire_delay_repetitions_speed_and_shake() {
        let magic = visual();
        assert_eq!(offensive_effect_frame_count(10, magic), 38);
        assert_eq!(timed_frames_to_ticks(38, magic.speed), 48);
        assert_eq!(
            effect_sound_elapsed_tick(
                MagicEventTimeline {
                    pre_ticks: 17,
                    brighten_ticks: 0,
                    summon_fade_in_ticks: 0,
                    body_ticks: 0,
                    effect_ticks: 48,
                    tail_ticks: 9,
                    death_fade_ticks: 0,
                    summon_fade_out_ticks: 0,
                    total_ticks: 74,
                },
                magic,
                0,
            ),
            20
        );
        assert_eq!(offensive_effect_frame_at(10, 10, magic), 8);
        assert_eq!(offensive_effect_frame_at(13, 10, magic), 2);
        assert_eq!(kept_effect_frame(10, magic), 9);
    }

    #[test]
    fn enemy_escape_moves_at_ten_millisecond_steps_then_waits_half_a_second() {
        let timeline = enemy_escape_timeline(305);
        assert_eq!(timeline.movement_steps, 61);
        assert_eq!(timeline.total_ticks, 28);
        assert_eq!(enemy_escape_offset(timeline, 28), 0);
        assert_eq!(enemy_escape_offset(timeline, 27), -20);
        assert_eq!(enemy_escape_offset(timeline, 12), -305);
        assert_eq!(enemy_escape_offset(timeline, 1), -305);
    }

    #[test]
    fn sub_frame_speed_is_scaled_to_the_fixed_update_step() {
        let mut magic = visual();
        magic.speed = -3;
        assert_eq!(timed_frames_to_ticks(10, magic.speed), 5);
        assert_eq!(timed_frame_at(1, 10, magic.speed), 2);
    }

    #[test]
    fn all_target_feedback_is_one_group_with_one_shared_death_fade() {
        let feedback = |enemy, visual, defeated| BattleEvent::PlayerMagic {
            player: 0,
            enemy,
            magic_object: 7,
            blow: 0,
            damage: 10,
            phase: MagicEventPhase::Feedback,
            visual,
            defeated,
        };
        let events = VecDeque::from([
            feedback(0, true, false),
            feedback(1, false, true),
            BattleEvent::Finished(pal_core::battle::BattleResult::Won),
        ]);

        let group = battle_event_group(&events);
        assert_eq!(group.len(), 2);
        assert!(matches!(
            battle_event_group_timing_event(&group),
            Some(BattleEvent::PlayerMagic {
                enemy: 0,
                defeated: true,
                ..
            })
        ));
        assert_eq!(pending_enemy_feedback_mask(&events), 0b11);
    }

    #[test]
    fn all_target_attack_shares_feedback_but_dual_attack_starts_a_new_group() {
        let attack = |enemy, visual, defeated| BattleEvent::PlayerAttack {
            player: 0,
            enemy,
            damage: 10,
            critical: false,
            visual,
            defeated,
        };
        let events = VecDeque::from([
            attack(0, true, false),
            attack(1, false, true),
            attack(0, true, true),
        ]);

        let group = battle_event_group(&events);
        assert_eq!(group.len(), 2);
        let timing = battle_event_group_timing_event(&group).unwrap();
        assert!(matches!(
            enemy_feedback_timing_event(timing, group[1]),
            BattleEvent::PlayerAttack {
                enemy: 1,
                visual: true,
                defeated: true,
                ..
            }
        ));
    }
}
