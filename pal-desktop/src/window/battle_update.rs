use pal_core::battle::{BattleEvent, BattlePhase, BattleResult, BattleStatus, BattleTarget};
use pal_core::game::{GameInput, GameState};
use pal_core::role::Direction;
use pal_core::script::ScriptRuntime;

use super::battle_render::{
    BattleMenuState, BattlePendingCommand, BattleSettlementPage, PostBattlePresentation,
};
use super::battle_timing::{
    battle_magic_for_event, effect_sound_elapsed_tick, event_has_full_magic_visual,
    magic_event_timeline, offensive_effect_frame_count, original_frames_to_ticks,
    MagicEventTimeline,
};
use super::menu_state::{update_wrapping_selection, InventoryMenu, InventoryMode};
use super::session::SessionState;

pub(super) const ACTION_EVENT_TICKS: u16 = 8;
pub(super) const PLAYER_MAGIC_ANIMATION_EVENT_TICKS: u16 = 22;
const ROUND_EVENT_TICKS: u16 = 2;
const FINISHED_EVENT_TICKS: u16 = 4;
const SETTLEMENT_TICKS: u16 = 60;
const BOSS_SETTLEMENT_TICKS: u16 = 110;
const POST_BATTLE_PAGE_TICKS: u16 = 60;

pub(super) struct FinishedBattle {
    pub(super) result: BattleResult,
}

pub(super) fn update_battle(
    input: GameInput,
    any_pressed: bool,
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
        queue_battle_events(game, services, automatic_events);
        return None;
    }
    if let Some(request) = game.take_battle_script() {
        if !battle_scripts.start(request) {
            game.finish_battle_script(request.script_entry, false);
        }
        return None;
    }
    if let Some(BattlePhase::Finished(result)) = game.battle().map(|battle| battle.phase()) {
        let wait_ticks = game.battle().map_or(0, |battle| {
            if result == BattleResult::Won && battle.rewards().experience > 0 {
                if battle.is_boss {
                    BOSS_SETTLEMENT_TICKS
                } else {
                    SETTLEMENT_TICKS
                }
            } else {
                0
            }
        });
        if !wait_elapsed(
            &mut services.battle_settlement_ticks,
            wait_ticks,
            any_pressed,
        ) {
            return None;
        }
        services.battle_settlement_ticks = None;
        services.inventory_menu = None;
        let battle = game.battle()?.clone();
        let before = battle
            .players
            .iter()
            .filter_map(|player| {
                let mut role = game.effective_player_role(player.role_id)?;
                role.level = player.level;
                role.hp = player.hp;
                role.max_hp = player.max_hp;
                role.mp = player.mp;
                role.max_mp = player.max_mp;
                role.attack_strength = player.attack_strength;
                role.magic_strength = player.magic_strength;
                role.defense = player.defense;
                role.dexterity = player.dexterity;
                role.flee_rate = player.flee_rate;
                Some((player.role_id, role))
            })
            .collect::<Vec<_>>();
        let (result, _) = game.settle_battle()?;
        let pages = settlement_pages(game, &before);
        if !pages.is_empty() {
            services.post_battle = Some(PostBattlePresentation {
                battle,
                result,
                pages,
                page: 0,
                ticks_remaining: POST_BATTLE_PAGE_TICKS,
            });
            return None;
        }
        return Some(FinishedBattle { result });
    }

    if game.auto_battle() {
        commit_forced_magic_or_attack(game, services, 9999);
        return None;
    }

    if update_battle_item_menu(input, game, services) {
        return None;
    }

    let living = game
        .battle()?
        .enemies
        .iter()
        .enumerate()
        .filter_map(|(index, enemy)| enemy.is_alive().then_some(index))
        .collect::<Vec<_>>();
    if input.battle_auto {
        services.battle_auto_attack = !services.battle_auto_attack;
        if let Some(battle) = game.battle_mut() {
            battle.set_auto_attack_mode(services.battle_auto_attack);
        }
        services.battle_menu = BattleMenuState::Main;
    }
    if services.battle_auto_attack && input.cancel {
        services.battle_auto_attack = false;
        if let Some(battle) = game.battle_mut() {
            battle.set_auto_attack_mode(false);
        }
        return None;
    }
    if input.battle_status {
        services.battle_menu = BattleMenuState::Status { selected: 0 };
    }
    if input.battle_flee {
        let committed = game
            .battle_mut()
            .and_then(|battle| battle.attempt_flee_all());
        commit_battle_action(game, services, committed);
        return None;
    }
    if input.battle_defend {
        let committed = game.battle_mut().and_then(|battle| battle.defend());
        commit_battle_action(game, services, committed);
        return None;
    }
    if input.battle_use_item {
        open_battle_inventory(game, services, InventoryMode::BattleUseItems);
        return None;
    }
    if input.battle_throw_item {
        open_battle_inventory(game, services, InventoryMode::BattleThrowItems);
        return None;
    }
    if input.battle_repeat {
        services.battle_repeat_all = true;
        services.battle_auto_attack = game
            .battle()
            .is_some_and(|battle| battle.previous_round_used_auto_attack());
    }
    if services.battle_repeat_all {
        let committed = game.repeat_battle_action();
        commit_battle_action(game, services, committed);
        if game
            .battle()
            .and_then(|battle| battle.active_player())
            .is_none()
        {
            services.battle_repeat_all = false;
        }
        return None;
    }
    if input.battle_force || services.battle_force_all {
        services.battle_force_all = true;
        commit_forced_magic_or_attack(game, services, 60);
        return None;
    }
    if services.battle_auto_attack {
        commit_automatic_attack(game, services);
        return None;
    }

    update_battle_menu(input, game, services, &living);

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

pub(super) fn advance_post_battle(
    any_pressed: bool,
    services: &mut SessionState,
) -> Option<FinishedBattle> {
    let presentation = services.post_battle.as_mut()?;
    if !advance_countdown(&mut presentation.ticks_remaining, any_pressed) {
        return None;
    }
    presentation.page += 1;
    if presentation.page < presentation.pages.len() {
        presentation.ticks_remaining = POST_BATTLE_PAGE_TICKS;
        return None;
    }
    let presentation = services.post_battle.take()?;
    Some(FinishedBattle {
        result: presentation.result,
    })
}

fn settlement_pages(
    game: &GameState,
    before: &[(u16, pal_assets::player_roles::PlayerRole)],
) -> Vec<BattleSettlementPage> {
    let mut pages = Vec::new();
    for (role_id, previous) in before {
        let Some(current) = game.effective_player_role(*role_id) else {
            continue;
        };
        if current.level > previous.level {
            pages.push(BattleSettlementPage::LevelUp {
                before: Box::new(previous.clone()),
                after: Box::new(current.clone()),
            });
        } else {
            for (label, old, new) in [
                (49usize, previous.max_hp, current.max_hp),
                (50, previous.max_mp, current.max_mp),
                (51, previous.attack_strength, current.attack_strength),
                (52, previous.magic_strength, current.magic_strength),
                (53, previous.defense, current.defense),
                (54, previous.dexterity, current.dexterity),
                (55, previous.flee_rate, current.flee_rate),
            ] {
                if new > old {
                    pages.push(BattleSettlementPage::AttributeGrowth {
                        role_id: *role_id,
                        label,
                        amount: new - old,
                    });
                }
            }
        }
        for magic_object in current
            .magic
            .iter()
            .copied()
            .filter(|magic| *magic != 0 && !previous.magic.contains(magic))
        {
            pages.push(BattleSettlementPage::LearnedMagic {
                role_id: *role_id,
                magic_object,
            });
        }
    }
    pages
}

fn wait_elapsed(ticks: &mut Option<u16>, duration: u16, skip: bool) -> bool {
    if duration == 0 || skip {
        return true;
    }
    let ticks = ticks.get_or_insert(duration);
    advance_countdown(ticks, false)
}

fn advance_countdown(ticks: &mut u16, skip: bool) -> bool {
    if skip {
        *ticks = 0;
        return true;
    }
    *ticks = ticks.saturating_sub(1);
    *ticks == 0
}

fn commit_battle_action(
    game: &GameState,
    services: &mut SessionState,
    committed: Option<Vec<BattleEvent>>,
) {
    if let Some(events) = committed {
        services.battle_menu = BattleMenuState::Main;
        services.battle_targeting_enemy = false;
        if !events.is_empty() {
            queue_battle_events(game, services, events);
        }
    }
}

fn commit_normal_attack(game: &mut GameState, services: &mut SessionState) {
    let Some(target) = game.battle().and_then(|battle| battle.first_living_enemy()) else {
        return;
    };
    let committed = game.battle_mut().and_then(|battle| battle.attack(target));
    commit_battle_action(game, services, committed);
}

fn commit_automatic_attack(game: &mut GameState, services: &mut SessionState) {
    let Some(target) = game.battle().and_then(|battle| battle.first_living_enemy()) else {
        return;
    };
    let committed = game
        .battle_mut()
        .and_then(|battle| battle.attack_automatically(target));
    commit_battle_action(game, services, committed);
}

fn commit_forced_magic_or_attack(
    game: &mut GameState,
    services: &mut SessionState,
    random_range: u16,
) {
    let committed = game
        .battle_mut()
        .and_then(|battle| battle.commit_auto_action(random_range));
    commit_battle_action(game, services, committed);
    if game
        .battle()
        .and_then(|battle| battle.active_player())
        .is_none()
    {
        services.battle_force_all = false;
    }
}

fn open_battle_inventory(game: &GameState, services: &mut SessionState, mode: InventoryMode) {
    let count = match mode {
        InventoryMode::BattleUseItems => game.battle_usable_inventory().len(),
        InventoryMode::BattleThrowItems => game.throwable_inventory().len(),
        _ => return,
    };
    services.inventory_selected = services.inventory_selected.min(count.saturating_sub(1));
    services.inventory_menu = Some(InventoryMenu {
        selected: services.inventory_selected,
        mode,
    });
}

fn update_battle_menu(
    input: GameInput,
    game: &mut GameState,
    services: &mut SessionState,
    living: &[usize],
) {
    match services.battle_menu {
        BattleMenuState::Main => update_battle_main_menu(input, game, services, living),
        BattleMenuState::Magic { mut selected } => {
            let magic_count = game
                .battle()
                .and_then(|battle| battle.players.get(battle.active_player()?))
                .map_or(0, |player| player.magics.len());
            update_grid_selection(&mut selected, input.direction_pressed, magic_count);
            services.battle_menu = BattleMenuState::Magic { selected };
            if input.cancel {
                services.battle_menu = BattleMenuState::Main;
            } else if input.confirm {
                begin_magic_selection(game, services, selected, living);
            }
        }
        BattleMenuState::Misc { mut selected } => {
            update_wrapping_selection(&mut selected, input.direction_pressed, 5);
            services.battle_menu = BattleMenuState::Misc { selected };
            if input.cancel {
                services.battle_menu = BattleMenuState::Main;
            } else if input.confirm {
                match selected {
                    0 => {
                        services.battle_auto_attack = true;
                        services.battle_menu = BattleMenuState::Main;
                    }
                    1 => services.battle_menu = BattleMenuState::ItemSubmenu { selected: 0 },
                    2 => {
                        let committed = game.battle_mut().and_then(|battle| battle.defend());
                        commit_battle_action(game, services, committed);
                    }
                    3 => {
                        let committed = game
                            .battle_mut()
                            .and_then(|battle| battle.attempt_flee_all());
                        commit_battle_action(game, services, committed);
                    }
                    _ => services.battle_menu = BattleMenuState::Status { selected: 0 },
                }
            }
        }
        BattleMenuState::ItemSubmenu { mut selected } => {
            match input.direction_pressed {
                Some(Direction::North | Direction::West) => selected = 0,
                Some(Direction::South | Direction::East) => selected = 1,
                None => selected = selected.min(1),
            }
            services.battle_menu = BattleMenuState::ItemSubmenu { selected };
            if input.cancel {
                services.battle_menu = BattleMenuState::Misc { selected: 1 };
            } else if input.confirm {
                open_battle_inventory(
                    game,
                    services,
                    if selected == 0 {
                        InventoryMode::BattleUseItems
                    } else {
                        InventoryMode::BattleThrowItems
                    },
                );
            }
        }
        BattleMenuState::TargetEnemy { command } => {
            if !living.is_empty() {
                services.battle_selected_enemy = select_enemy(
                    living,
                    services.battle_selected_enemy,
                    input.direction_pressed,
                );
            }
            services.battle_targeting_enemy = true;
            if input.cancel {
                services.battle_targeting_enemy = false;
                services.battle_menu = match command {
                    BattlePendingCommand::Magic(selected) => BattleMenuState::Magic { selected },
                    BattlePendingCommand::Attack
                    | BattlePendingCommand::CooperativeMagic
                    | BattlePendingCommand::UseItem(_)
                    | BattlePendingCommand::ThrowItem(_) => BattleMenuState::Main,
                };
            } else if input.confirm {
                let committed = match command {
                    BattlePendingCommand::Attack => game
                        .battle_mut()
                        .and_then(|battle| battle.attack(services.battle_selected_enemy)),
                    BattlePendingCommand::Magic(magic) => game.battle_mut().and_then(|battle| {
                        battle.cast_magic_at(
                            magic,
                            BattleTarget::Enemy(services.battle_selected_enemy),
                        )
                    }),
                    BattlePendingCommand::CooperativeMagic => {
                        game.battle_mut().and_then(|battle| {
                            battle.cast_cooperative_magic(BattleTarget::Enemy(
                                services.battle_selected_enemy,
                            ))
                        })
                    }
                    BattlePendingCommand::ThrowItem(item) => {
                        game.battle_throw_item(item, Some(services.battle_selected_enemy))
                    }
                    BattlePendingCommand::UseItem(_) => None,
                };
                commit_battle_action(game, services, committed);
            }
        }
        BattleMenuState::TargetPlayer {
            command,
            mut selected,
        } => {
            let player_count = game.battle().map_or(0, |battle| battle.players.len());
            selected = select_player(player_count, selected, input.direction_pressed);
            services.battle_menu = BattleMenuState::TargetPlayer { command, selected };
            if input.cancel {
                services.battle_menu = match command {
                    BattlePendingCommand::Magic(magic) => {
                        BattleMenuState::Magic { selected: magic }
                    }
                    _ => BattleMenuState::Main,
                };
            } else if input.confirm {
                let committed = match command {
                    BattlePendingCommand::Magic(magic) => game.battle_mut().and_then(|battle| {
                        battle.cast_magic_at(magic, BattleTarget::Player(selected))
                    }),
                    BattlePendingCommand::UseItem(item) => {
                        game.battle_use_item(item, Some(selected))
                    }
                    BattlePendingCommand::Attack
                    | BattlePendingCommand::CooperativeMagic
                    | BattlePendingCommand::ThrowItem(_) => None,
                };
                commit_battle_action(game, services, committed);
            }
        }
        BattleMenuState::Status { mut selected } => {
            let player_count = game.battle().map_or(0, |battle| battle.players.len());
            selected = selected.min(player_count.saturating_sub(1));
            let leave = if input.cancel {
                true
            } else {
                match input.direction_pressed {
                    Some(Direction::North | Direction::West) => {
                        if selected == 0 {
                            true
                        } else {
                            selected -= 1;
                            false
                        }
                    }
                    Some(Direction::South | Direction::East) => {
                        if selected + 1 >= player_count {
                            true
                        } else {
                            selected += 1;
                            false
                        }
                    }
                    None if input.confirm => {
                        if selected + 1 >= player_count {
                            true
                        } else {
                            selected += 1;
                            false
                        }
                    }
                    None => false,
                }
            };
            if leave || player_count == 0 {
                services.battle_menu = BattleMenuState::Main;
            } else {
                services.battle_menu = BattleMenuState::Status { selected };
            }
        }
    }
}

pub(super) fn queue_battle_events(
    game: &GameState,
    services: &mut SessionState,
    events: impl IntoIterator<Item = BattleEvent>,
) {
    let was_empty = services.battle_events.is_empty();
    services.battle_events.extend(events);
    if !was_empty {
        return;
    }
    let Some(&event) = services.battle_events.front() else {
        return;
    };
    services.battle_event_ticks = dynamic_battle_event_duration(game, services, event);
    services.battle_effect_sound_count = 0;
    services.battle_feedback_sound_played = false;
    play_battle_event_sounds(game, services, event);
    play_due_magic_sounds(game, services);
}

fn update_battle_item_menu(
    input: GameInput,
    game: &mut GameState,
    services: &mut SessionState,
) -> bool {
    let Some(mut menu) = services.inventory_menu.take() else {
        return false;
    };
    match menu.mode {
        InventoryMode::BattleUseItems => {
            let inventory = game.battle_usable_inventory();
            menu.selected = menu.selected.min(inventory.len().saturating_sub(1));
            menu.update(input.direction_pressed, inventory.len());
            services.inventory_selected = menu.selected;
            if input.cancel {
                services.battle_menu = BattleMenuState::Main;
                return true;
            }
            if input.confirm {
                if let Some(item) = inventory.get(menu.selected).copied() {
                    if item.apply_to_all {
                        if game.battle_use_item(item.item_id, None).is_some() {
                            services.battle_menu = BattleMenuState::Main;
                            return true;
                        }
                    } else {
                        let player_count = game.battle().map_or(0, |battle| battle.players.len());
                        if player_count == 1 {
                            if game.battle_use_item(item.item_id, Some(0)).is_some() {
                                services.battle_menu = BattleMenuState::Main;
                                return true;
                            }
                            services.inventory_menu = Some(menu);
                            return true;
                        }
                        services.battle_menu = BattleMenuState::TargetPlayer {
                            command: BattlePendingCommand::UseItem(item.item_id),
                            selected: 0,
                        };
                        return true;
                    }
                }
            }
        }
        InventoryMode::BattleThrowItems => {
            let inventory = game.throwable_inventory();
            menu.selected = menu.selected.min(inventory.len().saturating_sub(1));
            menu.update(input.direction_pressed, inventory.len());
            services.inventory_selected = menu.selected;
            if input.cancel {
                services.battle_menu = BattleMenuState::Main;
                return true;
            }
            if input.confirm {
                if let Some(item) = inventory.get(menu.selected).copied() {
                    if item.apply_to_all {
                        if game.battle_throw_item(item.item_id, None).is_some() {
                            services.battle_menu = BattleMenuState::Main;
                            return true;
                        }
                    } else {
                        services.battle_menu = BattleMenuState::TargetEnemy {
                            command: BattlePendingCommand::ThrowItem(item.item_id),
                        };
                        services.battle_targeting_enemy = true;
                        return true;
                    }
                }
            }
        }
        InventoryMode::Items
        | InventoryMode::EquipItems
        | InventoryMode::EquipTarget { .. }
        | InventoryMode::Target { .. } => {}
    }
    services.inventory_menu = Some(menu);
    true
}

fn advance_battle_events(game: &GameState, services: &mut SessionState) -> bool {
    let completed = services.battle_events.front().copied();
    match tick_battle_event_queue(
        &mut services.battle_events,
        &mut services.battle_event_ticks,
    ) {
        BattleEventTick::Idle => false,
        BattleEventTick::Started(event) => {
            retain_completed_magic_effect(game, services, completed);
            services.battle_event_ticks = dynamic_battle_event_duration(game, services, event);
            services.battle_effect_sound_count = 0;
            services.battle_feedback_sound_played = false;
            play_battle_event_sounds(game, services, event);
            play_due_magic_sounds(game, services);
            true
        }
        BattleEventTick::Active => {
            play_due_magic_sounds(game, services);
            true
        }
        BattleEventTick::Drained => {
            retain_completed_magic_effect(game, services, completed);
            services.battle_effect_sound_count = 0;
            services.battle_feedback_sound_played = false;
            true
        }
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
        | BattleEvent::EnemyMagic { .. }
        | BattleEvent::EnemyConfusedAttack { .. }
        | BattleEvent::PlayerConfusedAttack { .. }
        | BattleEvent::SimulatedMagic { .. }
        | BattleEvent::PlayerFlee { .. }
        | BattleEvent::PlayerDefend { .. }
        | BattleEvent::PlayerDefensiveMagic { .. }
        | BattleEvent::PlayerCooperativeMagic { .. } => ACTION_EVENT_TICKS,
        BattleEvent::PlayerUseItem { .. } => original_frames_to_ticks(25),
        BattleEvent::PlayerThrowItem { .. } => original_frames_to_ticks(24),
        BattleEvent::PlayerMagicAnimation { .. } => PLAYER_MAGIC_ANIMATION_EVENT_TICKS,
        BattleEvent::PlayerFriendDeath { .. } | BattleEvent::PlayerDying { .. } => {
            original_frames_to_ticks(10)
        }
        BattleEvent::RoundCompleted => ROUND_EVENT_TICKS,
        BattleEvent::Finished(_) => FINISHED_EVENT_TICKS,
    }
}

fn dynamic_battle_event_duration(
    game: &GameState,
    services: &SessionState,
    event: BattleEvent,
) -> u16 {
    session_magic_timing(game, services, event).map_or_else(
        || battle_event_duration(event),
        |(_, timing)| timing.total_ticks,
    )
}

fn session_magic_timing(
    game: &GameState,
    services: &SessionState,
    event: BattleEvent,
) -> Option<(pal_core::battle::BattleMagic, MagicEventTimeline)> {
    let battle = game.battle()?;
    let magic = battle_magic_for_event(battle, event)?;
    let visual = magic.effect_visual();
    let effect_frame_count = services
        .magic_effect_frame_counts
        .get(usize::from(visual.effect))
        .copied()
        .flatten();
    let summon_frame_count = (magic.magic_type == 9)
        .then(|| {
            usize::try_from(magic.specific)
                .ok()?
                .checked_add(10)
                .and_then(|sprite| {
                    services
                        .player_battle_frame_counts
                        .get(sprite)
                        .copied()
                        .flatten()
                })
        })
        .flatten();
    Some((
        magic,
        magic_event_timeline(event, magic, effect_frame_count, summon_frame_count),
    ))
}

fn retain_completed_magic_effect(
    game: &GameState,
    services: &mut SessionState,
    completed: Option<BattleEvent>,
) {
    if completed.is_some_and(|event| matches!(event, BattleEvent::Finished(_))) {
        services.battle_kept_effects.clear();
        return;
    }
    let Some(event) = completed.filter(|event| event_has_full_magic_visual(*event)) else {
        return;
    };
    let Some(magic) = game
        .battle()
        .and_then(|battle| battle_magic_for_event(battle, event))
    else {
        return;
    };
    if magic.effect_visual().keep_effect == u16::MAX {
        services.battle_kept_effects.push(event);
    }
}

fn play_due_magic_sounds(game: &GameState, services: &mut SessionState) {
    let Some(event) = services.battle_events.front().copied() else {
        return;
    };
    let Some((magic, timeline)) = session_magic_timing(game, services, event) else {
        return;
    };
    let elapsed = timeline
        .total_ticks
        .saturating_sub(services.battle_event_ticks.min(timeline.total_ticks));
    if !event_has_full_magic_visual(event) {
        services.battle_effect_sound_count = u16::MAX;
    } else {
        let visual = magic.effect_visual();
        let frame_count = services
            .magic_effect_frame_counts
            .get(usize::from(visual.effect))
            .copied()
            .flatten()
            .unwrap_or(0);
        let is_defensive = matches!(event, BattleEvent::PlayerDefensiveMagic { .. });
        let non_shake_frames = if is_defensive {
            frame_count
        } else {
            offensive_effect_frame_count(frame_count, visual)
                .saturating_sub(usize::from(visual.shake))
        };
        let repeats_sound = matches!(
            event,
            BattleEvent::PlayerMagic { .. }
                | BattleEvent::PlayerCooperativeMagic { .. }
                | BattleEvent::SimulatedMagic { .. }
        );
        if frame_count != 0 {
            loop {
                let cycle = usize::from(services.battle_effect_sound_count);
                if !repeats_sound && cycle > 0 {
                    break;
                }
                let cycle_frames = if repeats_sound {
                    cycle.saturating_mul(frame_count)
                } else {
                    0
                };
                let sound_frame = usize::from(visual.fire_delay).saturating_add(cycle_frames);
                if sound_frame >= non_shake_frames
                    || elapsed < effect_sound_elapsed_tick(timeline, visual, cycle_frames)
                {
                    break;
                }
                if let Ok(sound) = u16::try_from(visual.sound) {
                    if sound != 0 {
                        services.sound_effects.play(sound);
                    }
                }
                services.battle_effect_sound_count =
                    services.battle_effect_sound_count.saturating_add(1);
            }
        }
    }
    if !services.battle_feedback_sound_played && elapsed >= timeline.tail_start() {
        if let Some(sound) = magic_feedback_sound(game, event).filter(|sound| *sound != 0) {
            services.sound_effects.play(sound);
        }
        services.battle_feedback_sound_played = true;
    }
}

fn magic_feedback_sound(game: &GameState, event: BattleEvent) -> Option<u16> {
    let battle = game.battle()?;
    match event {
        BattleEvent::PlayerMagic {
            enemy, defeated, ..
        }
        | BattleEvent::PlayerCooperativeMagic {
            enemy, defeated, ..
        }
        | BattleEvent::SimulatedMagic {
            enemy, defeated, ..
        } => {
            let enemy = battle.enemies.get(enemy)?;
            u16::try_from(if defeated {
                enemy.death_sound
            } else {
                enemy.action_sound
            })
            .ok()
        }
        BattleEvent::EnemyMagic {
            player, defeated, ..
        } => defeated.then(|| battle.players.get(player).map(|player| player.death_sound))?,
        BattleEvent::PlayerDefensiveMagic { .. } => None,
        _ => None,
    }
}

fn play_battle_event_sounds(game: &GameState, services: &mut SessionState, event: BattleEvent) {
    if event == BattleEvent::Finished(BattleResult::Won) {
        if let Some(battle) = game.battle() {
            let _ = services
                .music
                .play(if battle.is_boss { 2 } else { 3 }, false, 0);
        }
    }
    let sounds = match event {
        BattleEvent::PlayerAttack {
            player,
            enemy,
            critical,
            defeated,
            ..
        } => game.battle().and_then(|battle| {
            let enemy = battle.enemies.get(enemy)?;
            let player = battle.players.get(player)?;
            Some(vec![
                Some(if critical {
                    player.critical_sound
                } else {
                    player.attack_sound
                }),
                Some(player.weapon_sound),
                u16::try_from(if defeated {
                    enemy.death_sound
                } else {
                    enemy.action_sound
                })
                .ok(),
            ])
        }),
        BattleEvent::PlayerMagic { player, visual, .. } => game.battle().and_then(|battle| {
            let player = battle.players.get(player)?;
            Some(vec![visual.then_some(player.magic_sound), None, None])
        }),
        BattleEvent::EnemyAttack {
            enemy,
            player,
            protected_by,
            auto_defended,
            defeated,
            ..
        } => game.battle().and_then(|battle| {
            let enemy = battle.enemies.get(enemy)?;
            let player = battle.players.get(player)?;
            let defense_sound = protected_by
                .and_then(|cover| battle.players.get(cover))
                .map(|cover| cover.cover_sound)
                .or_else(|| auto_defended.then_some(player.cover_sound));
            Some(vec![
                u16::try_from(enemy.attack_sound).ok(),
                defense_sound.or_else(|| defeated.then_some(player.death_sound)),
                None,
            ])
        }),
        BattleEvent::EnemyMagic { enemy, visual, .. } => game.battle().and_then(|battle| {
            let enemy = battle.enemies.get(enemy)?;
            Some(vec![
                visual
                    .then(|| u16::try_from(enemy.magic_sound).ok())
                    .flatten(),
                None,
                None,
            ])
        }),
        BattleEvent::EnemyConfusedAttack {
            enemy,
            target,
            defeated,
            ..
        } => game.battle().and_then(|battle| {
            let enemy = battle.enemies.get(enemy)?;
            let target = battle.enemies.get(target)?;
            Some(vec![
                u16::try_from(enemy.attack_sound).ok(),
                u16::try_from(if defeated {
                    target.death_sound
                } else {
                    target.action_sound
                })
                .ok(),
                None,
            ])
        }),
        BattleEvent::PlayerConfusedAttack {
            player,
            target,
            defeated,
            ..
        } => game.battle().and_then(|battle| {
            let player = battle.players.get(player)?;
            let target = battle.players.get(target)?;
            Some(vec![
                Some(player.attack_sound),
                Some(player.weapon_sound),
                defeated.then_some(target.death_sound),
            ])
        }),
        BattleEvent::SimulatedMagic { .. } => None,
        BattleEvent::PlayerUseItem { .. } => Some(vec![Some(28), None, None]),
        BattleEvent::PlayerThrowItem { player, .. } => game.battle().and_then(|battle| {
            let player = battle.players.get(player)?;
            Some(vec![Some(player.magic_sound), None, None])
        }),
        BattleEvent::PlayerDefensiveMagic { player, .. } => game.battle().and_then(|battle| {
            let player = battle.players.get(player)?;
            Some(vec![Some(player.magic_sound), None, None])
        }),
        BattleEvent::PlayerDefend { .. } => None,
        BattleEvent::PlayerCooperativeMagic { player, visual, .. } => {
            game.battle().and_then(|battle| {
                let player = battle.players.get(player)?;
                Some(vec![visual.then_some(player.magic_sound), None, None])
            })
        }
        BattleEvent::PlayerMagicAnimation {
            player: Some(player),
        } => game.battle().and_then(|battle| {
            let player = battle.players.get(player)?;
            Some(vec![Some(player.magic_sound), None, None])
        }),
        BattleEvent::PlayerMagicAnimation { player: None } => None,
        BattleEvent::PlayerDying { player } => game.battle().and_then(|battle| {
            let player = battle.players.get(player)?;
            Some(vec![Some(player.dying_sound), None, None])
        }),
        BattleEvent::PlayerFriendDeath { .. } => None,
        BattleEvent::PlayerFlee {
            succeeded: true, ..
        } => Some(vec![Some(45), None, None]),
        BattleEvent::PlayerFlee {
            succeeded: false, ..
        } => None,
        BattleEvent::RoundCompleted | BattleEvent::Finished(_) => None,
    }
    .unwrap_or_default();
    for sound in sounds.into_iter().flatten().filter(|&sound| sound != 0) {
        services.sound_effects.play(sound);
    }
}

fn select_battle_command(
    current: usize,
    direction: Option<Direction>,
    magic_enabled: bool,
    cooperative_magic_enabled: bool,
) -> usize {
    let current = if battle_command_enabled(current, magic_enabled, cooperative_magic_enabled) {
        current
    } else {
        0
    };
    let proposed = match direction {
        Some(Direction::North) => 0,
        Some(Direction::West) => 1,
        Some(Direction::East) => 2,
        Some(Direction::South) => 3,
        None => current.min(3),
    };
    if battle_command_enabled(proposed, magic_enabled, cooperative_magic_enabled) {
        proposed
    } else {
        current
    }
}

fn battle_command_enabled(
    command: usize,
    magic_enabled: bool,
    cooperative_magic_enabled: bool,
) -> bool {
    match command {
        1 => magic_enabled,
        2 => cooperative_magic_enabled,
        _ => true,
    }
}

fn update_battle_main_menu(
    input: GameInput,
    game: &mut GameState,
    services: &mut SessionState,
    living: &[usize],
) {
    let (magic_enabled, cooperative_magic_enabled) = game
        .battle()
        .and_then(|battle| {
            let active = battle.active_player()?;
            Some((
                !battle.players[active]
                    .statuses
                    .is_active(BattleStatus::Silence),
                battle.can_use_cooperative_magic(),
            ))
        })
        .unwrap_or((false, false));
    services.battle_command_selected = select_battle_command(
        services.battle_command_selected,
        input.direction_pressed,
        magic_enabled,
        cooperative_magic_enabled,
    );
    if input.cancel {
        let _ = game
            .battle_mut()
            .and_then(|battle| battle.undo_last_command());
        return;
    }
    if !input.confirm {
        return;
    }
    match services.battle_command_selected {
        0 => {
            let attacks_all = game
                .battle()
                .and_then(|battle| battle.players.get(battle.active_player()?))
                .is_some_and(|player| player.attacks_all);
            if attacks_all || living.len() <= 1 {
                commit_normal_attack(game, services);
            } else {
                services.battle_menu = BattleMenuState::TargetEnemy {
                    command: BattlePendingCommand::Attack,
                };
                services.battle_targeting_enemy = true;
            }
        }
        1 => {
            services.battle_menu = BattleMenuState::Magic {
                selected: services.magic_selected,
            };
        }
        2 => {
            let Some((magic, enabled)) = game.battle().and_then(|battle| {
                Some((
                    battle.active_cooperative_magic()?,
                    battle.can_use_cooperative_magic(),
                ))
            }) else {
                return;
            };
            if !enabled {
                return;
            }
            if magic.apply_to_all() {
                let committed = game
                    .battle_mut()
                    .and_then(|battle| battle.cast_cooperative_magic(BattleTarget::AllEnemies));
                commit_battle_action(game, services, committed);
            } else if let Some(&target) = living.first().filter(|_| living.len() <= 1) {
                let committed = game
                    .battle_mut()
                    .and_then(|battle| battle.cast_cooperative_magic(BattleTarget::Enemy(target)));
                commit_battle_action(game, services, committed);
            } else if !living.is_empty() {
                services.battle_menu = BattleMenuState::TargetEnemy {
                    command: BattlePendingCommand::CooperativeMagic,
                };
                services.battle_targeting_enemy = true;
            }
        }
        _ => services.battle_menu = BattleMenuState::Misc { selected: 0 },
    }
}

fn begin_magic_selection(
    game: &mut GameState,
    services: &mut SessionState,
    selected: usize,
    living: &[usize],
) {
    let Some((magic, active_player, player_count)) = game.battle().and_then(|battle| {
        let active = battle.active_player()?;
        Some((
            *battle.players.get(active)?.magics.get(selected)?,
            active,
            battle.players.len(),
        ))
    }) else {
        return;
    };
    let can_cast = game
        .battle()
        .and_then(|battle| battle.players.get(active_player))
        .is_some_and(|player| {
            player.mp >= magic.mp_cost && !player.statuses.is_active(BattleStatus::Silence)
        });
    if !can_cast {
        return;
    }
    services.magic_selected = selected;
    let committed = match (magic.usable_to_enemy(), magic.apply_to_all()) {
        (true, true) => game
            .battle_mut()
            .and_then(|battle| battle.cast_magic_at(selected, BattleTarget::AllEnemies)),
        (true, false) if living.len() <= 1 => {
            let Some(target) = living.first().copied() else {
                return;
            };
            game.battle_mut()
                .and_then(|battle| battle.cast_magic_at(selected, BattleTarget::Enemy(target)))
        }
        (true, false) => {
            services.battle_menu = BattleMenuState::TargetEnemy {
                command: BattlePendingCommand::Magic(selected),
            };
            services.battle_targeting_enemy = true;
            return;
        }
        (false, true) => game
            .battle_mut()
            .and_then(|battle| battle.cast_magic_at(selected, BattleTarget::AllPlayers)),
        (false, false) if player_count <= 1 => game
            .battle_mut()
            .and_then(|battle| battle.cast_magic_at(selected, BattleTarget::Player(0))),
        (false, false) => {
            services.battle_menu = BattleMenuState::TargetPlayer {
                command: BattlePendingCommand::Magic(selected),
                selected: 0,
            };
            return;
        }
    };
    commit_battle_action(game, services, committed);
}

fn update_grid_selection(current: &mut usize, direction: Option<Direction>, count: usize) {
    if count == 0 {
        *current = 0;
        return;
    }
    *current = (*current).min(count - 1);
    match direction {
        Some(Direction::North) => *current = current.saturating_sub(3),
        Some(Direction::South) => *current = (*current + 3).min(count - 1),
        Some(Direction::West) => *current = current.saturating_sub(1),
        Some(Direction::East) => *current = (*current + 1).min(count - 1),
        None => {}
    }
}

fn select_player(count: usize, current: usize, direction: Option<Direction>) -> usize {
    if count == 0 {
        return 0;
    }
    let current = current.min(count - 1);
    match direction {
        Some(Direction::West | Direction::South) => current.checked_sub(1).unwrap_or(count - 1),
        Some(Direction::East | Direction::North) => (current + 1) % count,
        None => current,
    }
}

fn select_enemy(living: &[usize], current: usize, direction: Option<Direction>) -> usize {
    let position = living
        .iter()
        .position(|&index| index == current)
        .unwrap_or(0);
    let next = match direction {
        Some(Direction::West | Direction::South) => {
            position.checked_sub(1).unwrap_or(living.len() - 1)
        }
        Some(Direction::East | Direction::North) => (position + 1) % living.len(),
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
        assert_eq!(select_enemy(&living, 1, Some(Direction::South)), 4);
        assert_eq!(select_enemy(&living, 4, Some(Direction::North)), 1);
        assert_eq!(select_enemy(&living, 2, None), 1);
    }

    #[test]
    fn battle_commands_follow_the_original_cross_directions() {
        assert_eq!(
            select_battle_command(3, Some(Direction::North), true, true),
            0
        );
        assert_eq!(
            select_battle_command(0, Some(Direction::West), true, true),
            1
        );
        assert_eq!(
            select_battle_command(0, Some(Direction::East), true, true),
            2
        );
        assert_eq!(
            select_battle_command(0, Some(Direction::South), true, true),
            3
        );
        assert_eq!(select_battle_command(9, None, true, true), 3);
    }

    #[test]
    fn battle_commands_do_not_select_disabled_magic_actions() {
        assert_eq!(
            select_battle_command(0, Some(Direction::West), false, true),
            0
        );
        assert_eq!(
            select_battle_command(0, Some(Direction::East), true, false),
            0
        );
        assert_eq!(select_battle_command(1, None, false, true), 0);
        assert_eq!(select_battle_command(2, None, true, false), 0);
    }

    #[test]
    fn magic_grid_clamps_at_both_ends() {
        let mut selected = 0;
        update_grid_selection(&mut selected, Some(Direction::West), 7);
        assert_eq!(selected, 0);
        update_grid_selection(&mut selected, Some(Direction::North), 7);
        assert_eq!(selected, 0);
        selected = 6;
        update_grid_selection(&mut selected, Some(Direction::East), 7);
        assert_eq!(selected, 6);
        update_grid_selection(&mut selected, Some(Direction::South), 7);
        assert_eq!(selected, 6);
    }

    #[test]
    fn player_target_directions_match_the_classic_battle_ui() {
        assert_eq!(select_player(3, 0, Some(Direction::West)), 2);
        assert_eq!(select_player(3, 0, Some(Direction::South)), 2);
        assert_eq!(select_player(3, 2, Some(Direction::East)), 0);
        assert_eq!(select_player(3, 2, Some(Direction::North)), 0);
    }

    #[test]
    fn settlement_waits_for_the_classic_timeout_or_any_key() {
        let mut ticks = None;
        for _ in 0..SETTLEMENT_TICKS - 1 {
            assert!(!wait_elapsed(&mut ticks, SETTLEMENT_TICKS, false));
        }
        assert!(wait_elapsed(&mut ticks, SETTLEMENT_TICKS, false));

        let mut skipped = None;
        assert!(wait_elapsed(&mut skipped, SETTLEMENT_TICKS, true));
        assert_eq!(skipped, None);
    }

    #[test]
    fn battle_event_durations_leave_time_for_action_and_settlement_feedback() {
        let action = battle_event_duration(BattleEvent::PlayerAttack {
            player: 0,
            enemy: 0,
            damage: 1,
            critical: false,
            defeated: false,
        });
        let finished = battle_event_duration(BattleEvent::Finished(BattleResult::Won));
        let round = battle_event_duration(BattleEvent::RoundCompleted);
        let magic_animation =
            battle_event_duration(BattleEvent::PlayerMagicAnimation { player: Some(0) });
        let use_item = battle_event_duration(BattleEvent::PlayerUseItem {
            player: 0,
            item_object: 1,
            target: None,
            consuming: true,
        });
        let throw_item = battle_event_duration(BattleEvent::PlayerThrowItem {
            player: 0,
            item_object: 1,
            target: Some(0),
        });
        assert_eq!((action, magic_animation, finished, round), (8, 22, 4, 2));
        assert_eq!((use_item, throw_item), (20, 20));
    }

    #[test]
    fn battle_event_queue_blocks_until_every_feedback_event_is_drained() {
        let attack = BattleEvent::PlayerAttack {
            player: 0,
            enemy: 0,
            damage: 1,
            critical: false,
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
