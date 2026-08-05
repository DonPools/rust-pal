use pal_core::battle::{BattleEvent, BattleMagic, BattleMagicVisual, BattleState};
use pal_core::game::UPDATE_INTERVAL_MS;

const ORIGINAL_BATTLE_FRAME_MS: u64 = 40;
const OFFENSIVE_FEEDBACK_FRAMES: usize = 4;
const DEFENSIVE_COLOR_SHIFT_FRAMES: usize = 13;
const NORMAL_PRE_MAGIC_FRAMES: usize = 17;
const SUMMON_PRE_MAGIC_FRAMES: usize = 7;
const SUMMON_BRIGHTEN_FRAMES: usize = 10;
const COOPERATIVE_PRE_MAGIC_FRAMES: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct MagicEventTimeline {
    pub(super) pre_ticks: u16,
    pub(super) brighten_ticks: u16,
    pub(super) body_ticks: u16,
    pub(super) effect_ticks: u16,
    pub(super) tail_ticks: u16,
    pub(super) total_ticks: u16,
}

impl MagicEventTimeline {
    pub(super) fn effect_start(self) -> u16 {
        self.pre_ticks
            .saturating_add(self.brighten_ticks)
            .saturating_add(self.body_ticks)
    }

    pub(super) fn body_start(self) -> u16 {
        self.pre_ticks.saturating_add(self.brighten_ticks)
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
        BattleEvent::SimulatedMagic { magic_object, .. } => battle
            .players
            .iter()
            .flat_map(|player| {
                player
                    .magics
                    .iter()
                    .copied()
                    .chain(player.cooperative_magic)
            })
            .chain(battle.enemies.iter().filter_map(|enemy| enemy.magic))
            .find(|magic| magic.object_id == magic_object),
        _ => None,
    }
}

pub(super) fn event_has_full_magic_visual(event: BattleEvent) -> bool {
    match event {
        BattleEvent::PlayerMagic { visual, .. }
        | BattleEvent::PlayerCooperativeMagic { visual, .. }
        | BattleEvent::EnemyMagic { visual, .. }
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
) -> MagicEventTimeline {
    if !event_has_full_magic_visual(event) {
        let tail_ticks = original_frames_to_ticks(OFFENSIVE_FEEDBACK_FRAMES);
        return MagicEventTimeline {
            pre_ticks: 0,
            brighten_ticks: 0,
            body_ticks: 0,
            effect_ticks: 0,
            tail_ticks,
            total_ticks: tail_ticks,
        };
    }

    let is_defensive = matches!(event, BattleEvent::PlayerDefensiveMagic { .. });
    let is_summon = magic.magic_type == 9 && magic.summon_effect.is_some();
    let pre_frames = match event {
        BattleEvent::PlayerMagic { .. } if is_summon => SUMMON_PRE_MAGIC_FRAMES,
        BattleEvent::PlayerMagic { .. } | BattleEvent::PlayerDefensiveMagic { .. } => {
            NORMAL_PRE_MAGIC_FRAMES
        }
        BattleEvent::PlayerCooperativeMagic { .. } => COOPERATIVE_PRE_MAGIC_FRAMES,
        _ => 0,
    };
    let pre_ticks = original_frames_to_ticks(pre_frames);
    let brighten_ticks = if is_summon {
        original_frames_to_ticks(SUMMON_BRIGHTEN_FRAMES)
    } else {
        0
    };
    let body_ticks = if is_summon {
        timed_frames_to_ticks(summon_frame_count.unwrap_or(0), magic.speed)
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
    let tail_frames = if is_defensive {
        DEFENSIVE_COLOR_SHIFT_FRAMES
    } else {
        OFFENSIVE_FEEDBACK_FRAMES
    };
    let tail_ticks = original_frames_to_ticks(tail_frames);
    let total_ticks = pre_ticks
        .saturating_add(brighten_ticks)
        .saturating_add(body_ticks)
        .saturating_add(effect_ticks)
        .saturating_add(tail_ticks)
        .max(1);
    MagicEventTimeline {
        pre_ticks,
        brighten_ticks,
        body_ticks,
        effect_ticks,
        tail_ticks,
        total_ticks,
    }
}

pub(super) fn original_frames_to_ticks(frames: usize) -> u16 {
    milliseconds_to_ticks((frames as u64).saturating_mul(ORIGINAL_BATTLE_FRAME_MS))
}

pub(super) fn timed_frames_to_ticks(frames: usize, speed: i16) -> u16 {
    milliseconds_to_ticks((frames as u64).saturating_mul(frame_time_ms(speed)))
}

pub(super) fn timed_frame_at(local_tick: u16, frame_count: usize, speed: i16) -> usize {
    if frame_count == 0 {
        return 0;
    }
    let elapsed_ms = u64::from(local_tick).saturating_mul(UPDATE_INTERVAL_MS);
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

fn milliseconds_to_ticks(milliseconds: u64) -> u16 {
    u16::try_from(milliseconds.div_ceil(UPDATE_INTERVAL_MS)).unwrap_or(u16::MAX)
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
        assert_eq!(timed_frames_to_ticks(38, magic.speed), 38);
        assert_eq!(
            effect_sound_elapsed_tick(
                MagicEventTimeline {
                    pre_ticks: 14,
                    brighten_ticks: 0,
                    body_ticks: 0,
                    effect_ticks: 38,
                    tail_ticks: 4,
                    total_ticks: 56,
                },
                magic,
                0,
            ),
            16
        );
        assert_eq!(offensive_effect_frame_at(10, 10, magic), 2);
        assert_eq!(kept_effect_frame(10, magic), 9);
    }

    #[test]
    fn sub_frame_speed_is_scaled_to_the_fixed_update_step() {
        let mut magic = visual();
        magic.speed = -3;
        assert_eq!(timed_frames_to_ticks(10, magic.speed), 4);
        assert_eq!(timed_frame_at(1, 10, magic.speed), 2);
    }
}
