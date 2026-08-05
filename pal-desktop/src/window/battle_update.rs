use pal_core::battle::{BattleEvent, BattlePhase, BattleResult, BattleRewards};
use pal_core::game::{GameInput, GameState};
use pal_core::role::Direction;
use pal_core::script::ScriptRuntime;

use super::session::SessionState;

pub(super) const ACTION_EVENT_TICKS: u16 = 8;
const ROUND_EVENT_TICKS: u16 = 2;
const FINISHED_EVENT_TICKS: u16 = 4;

pub(super) struct FinishedBattle {
    pub(super) result: BattleResult,
    pub(super) rewards: BattleRewards,
}

pub(super) fn update_battle(
    input: GameInput,
    game: &mut GameState,
    services: &mut SessionState,
    battle_scripts: &mut ScriptRuntime,
) -> Option<FinishedBattle> {
    if advance_battle_events(game, services) {
        return None;
    }
    if battle_scripts.is_active() {
        return None;
    }
    let mut automatic_events = game.advance_battle_resolution();
    automatic_events.extend(
        game.battle_mut()
            .map(|battle| battle.advance_automatic_turns())
            .unwrap_or_default(),
    );
    if !automatic_events.is_empty() {
        services.battle_events.extend(automatic_events);
        let event = *services
            .battle_events
            .front()
            .expect("a newly queued automatic battle event is available");
        services.battle_event_ticks = battle_event_duration(event);
        play_battle_event_sounds(game, services, event);
        return None;
    }
    if let Some(request) = game.take_battle_script() {
        if !battle_scripts.start(request) {
            game.finish_battle_script(request.script_entry, false);
        }
        return None;
    }
    if let Some(BattlePhase::Finished(_)) = game.battle().map(|battle| battle.phase()) {
        if !input.confirm {
            return None;
        }
        let (result, rewards) = game.settle_battle()?;
        return Some(FinishedBattle { result, rewards });
    }

    let living = game
        .battle()?
        .enemies
        .iter()
        .enumerate()
        .filter_map(|(index, enemy)| enemy.is_alive().then_some(index))
        .collect::<Vec<_>>();
    if matches!(
        input.direction_pressed,
        Some(Direction::North | Direction::South)
    ) {
        services.battle_command_selected = (services.battle_command_selected + 1) % 2;
    }
    if !living.is_empty() {
        services.battle_selected_enemy = select_enemy(
            &living,
            services.battle_selected_enemy,
            input.direction_pressed,
        );
    }

    let events = if input.confirm && services.battle_command_selected == 0 {
        game.battle_mut()
            .and_then(|battle| battle.attack(services.battle_selected_enemy))
            .unwrap_or_default()
    } else if input.confirm {
        let magic = game.battle().and_then(|battle| {
            let player = battle.players.get(battle.active_player()?)?;
            player
                .magics
                .iter()
                .enumerate()
                .filter(|(_, magic)| player.mp >= magic.mp_cost)
                .max_by_key(|(_, magic)| magic.base_damage)
                .map(|(index, _)| index)
        });
        magic
            .and_then(|magic| {
                game.battle_mut()
                    .and_then(|battle| battle.cast_magic(magic, services.battle_selected_enemy))
            })
            .unwrap_or_default()
    } else if input.cancel {
        game.battle_mut()
            .and_then(|battle| battle.flee())
            .into_iter()
            .collect()
    } else {
        Vec::new()
    };
    if !events.is_empty() {
        services.battle_events.extend(events);
        let event = *services
            .battle_events
            .front()
            .expect("a newly queued battle event is available");
        services.battle_event_ticks = battle_event_duration(event);
        play_battle_event_sounds(game, services, event);
    }

    if let Some(target) = game
        .battle()
        .and_then(|battle| battle.first_living_enemy())
        .filter(|_| {
            game.battle()
                .and_then(|battle| battle.enemies.get(services.battle_selected_enemy))
                .is_none_or(|enemy| !enemy.is_alive())
        })
    {
        services.battle_selected_enemy = target;
    }

    None
}

fn advance_battle_events(game: &GameState, services: &mut SessionState) -> bool {
    match tick_battle_event_queue(
        &mut services.battle_events,
        &mut services.battle_event_ticks,
    ) {
        BattleEventTick::Idle => false,
        BattleEventTick::Started(event) => {
            play_battle_event_sounds(game, services, event);
            true
        }
        BattleEventTick::Active | BattleEventTick::Drained => true,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BattleEventTick {
    Idle,
    Active,
    Started(BattleEvent),
    Drained,
}

fn tick_battle_event_queue(
    events: &mut std::collections::VecDeque<BattleEvent>,
    ticks: &mut u16,
) -> BattleEventTick {
    if events.is_empty() {
        *ticks = 0;
        return BattleEventTick::Idle;
    }
    if *ticks > 1 {
        *ticks -= 1;
        return BattleEventTick::Active;
    }

    events.pop_front();
    let Some(event) = events.front().copied() else {
        *ticks = 0;
        return BattleEventTick::Drained;
    };
    *ticks = battle_event_duration(event);
    BattleEventTick::Started(event)
}

fn battle_event_duration(event: BattleEvent) -> u16 {
    match event {
        BattleEvent::PlayerAttack { .. }
        | BattleEvent::PlayerMagic { .. }
        | BattleEvent::EnemyAttack { .. }
        | BattleEvent::EnemyMagic { .. } => ACTION_EVENT_TICKS,
        BattleEvent::RoundCompleted => ROUND_EVENT_TICKS,
        BattleEvent::Finished(_) => FINISHED_EVENT_TICKS,
    }
}

fn play_battle_event_sounds(game: &GameState, services: &mut SessionState, event: BattleEvent) {
    let sounds = match event {
        BattleEvent::PlayerAttack {
            player,
            enemy,
            defeated,
            ..
        } => game.battle().and_then(|battle| {
            let enemy = battle.enemies.get(enemy)?;
            let player = battle.players.get(player)?;
            Some(vec![
                Some(player.attack_sound),
                Some(player.weapon_sound),
                u16::try_from(if defeated {
                    enemy.death_sound
                } else {
                    enemy.action_sound
                })
                .ok(),
            ])
        }),
        BattleEvent::PlayerMagic {
            player,
            enemy,
            magic_object,
            defeated,
            ..
        } => game.battle().and_then(|battle| {
            let player = battle.players.get(player)?;
            let magic = player
                .magics
                .iter()
                .find(|magic| magic.object_id == magic_object)
                .map(|magic| magic.sound);
            let enemy = battle.enemies.get(enemy)?;
            Some(vec![
                Some(player.magic_sound),
                magic.and_then(|sound| u16::try_from(sound).ok()),
                u16::try_from(if defeated {
                    enemy.death_sound
                } else {
                    enemy.action_sound
                })
                .ok(),
            ])
        }),
        BattleEvent::EnemyAttack {
            enemy,
            player,
            defeated,
            ..
        } => game.battle().and_then(|battle| {
            let enemy = battle.enemies.get(enemy)?;
            let player = battle.players.get(player)?;
            Some(vec![
                u16::try_from(enemy.attack_sound).ok(),
                defeated.then_some(player.death_sound),
                None,
            ])
        }),
        BattleEvent::EnemyMagic {
            enemy,
            player,
            magic_object,
            defeated,
            ..
        } => game.battle().and_then(|battle| {
            let enemy = battle.enemies.get(enemy)?;
            let magic = enemy
                .magic
                .filter(|magic| magic.object_id == magic_object)
                .map(|magic| magic.sound);
            let player = battle.players.get(player)?;
            Some(vec![
                u16::try_from(enemy.magic_sound).ok(),
                magic.and_then(|sound| u16::try_from(sound).ok()),
                defeated.then_some(player.death_sound),
            ])
        }),
        BattleEvent::RoundCompleted | BattleEvent::Finished(_) => None,
    }
    .unwrap_or_default();
    for sound in sounds.into_iter().flatten().filter(|&sound| sound != 0) {
        services.sound_effects.play(sound);
    }
}

fn select_enemy(living: &[usize], current: usize, direction: Option<Direction>) -> usize {
    let position = living
        .iter()
        .position(|&index| index == current)
        .unwrap_or(0);
    let next = match direction {
        Some(Direction::West) => position.checked_sub(1).unwrap_or(living.len() - 1),
        Some(Direction::East) => (position + 1) % living.len(),
        Some(Direction::North | Direction::South) => position,
        None => position,
    };
    living[next]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enemy_selection_wraps_and_skips_defeated_slots() {
        let living = [1, 3, 4];
        assert_eq!(select_enemy(&living, 1, Some(Direction::West)), 4);
        assert_eq!(select_enemy(&living, 4, Some(Direction::East)), 1);
        assert_eq!(select_enemy(&living, 2, None), 1);
    }

    #[test]
    fn battle_event_durations_leave_time_for_action_and_settlement_feedback() {
        let action = battle_event_duration(BattleEvent::PlayerAttack {
            player: 0,
            enemy: 0,
            damage: 1,
            defeated: false,
        });
        let finished = battle_event_duration(BattleEvent::Finished(BattleResult::Won));
        let round = battle_event_duration(BattleEvent::RoundCompleted);
        assert_eq!((action, finished, round), (8, 4, 2));
    }

    #[test]
    fn battle_event_queue_blocks_until_every_feedback_event_is_drained() {
        let attack = BattleEvent::PlayerAttack {
            player: 0,
            enemy: 0,
            damage: 1,
            defeated: true,
        };
        let finished = BattleEvent::Finished(BattleResult::Won);
        let mut events = std::collections::VecDeque::from([attack, finished]);
        let mut ticks = 1;

        assert_eq!(
            tick_battle_event_queue(&mut events, &mut ticks),
            BattleEventTick::Started(finished)
        );
        assert_eq!(ticks, FINISHED_EVENT_TICKS);
        for expected in (1..FINISHED_EVENT_TICKS).rev() {
            assert_eq!(
                tick_battle_event_queue(&mut events, &mut ticks),
                BattleEventTick::Active
            );
            assert_eq!(ticks, expected);
        }
        assert_eq!(
            tick_battle_event_queue(&mut events, &mut ticks),
            BattleEventTick::Drained
        );
        assert_eq!(
            tick_battle_event_queue(&mut events, &mut ticks),
            BattleEventTick::Idle
        );
    }
}
