use pal_core::battle::{
    BattleEvent, BattlePhase, BattleResult, BattleStatus, BattleTarget, MagicEventPhase,
    HIDDEN_EXP_ATTACK, HIDDEN_EXP_DEFENSE, HIDDEN_EXP_DEXTERITY, HIDDEN_EXP_FLEE,
    HIDDEN_EXP_HEALTH, HIDDEN_EXP_MAGIC, HIDDEN_EXP_MAGIC_POWER,
};
use pal_core::game::{BattlePlayerSettlement, GameInput, GameState};
use pal_core::role::Direction;
use pal_core::script::ScriptRuntime;

use super::battle_render::{
    BattleMenuState, BattlePendingCommand, BattleSettlementPage, PostBattlePresentation,
};
use super::battle_timing::{
    battle_magic_for_event, battle_milliseconds_to_ticks, effect_sound_elapsed_tick,
    enemy_attack_frames, enemy_escape_timeline, enemy_magic_pre_frames,
    event_has_full_magic_visual, magic_event_timeline, offensive_effect_frame_count,
    original_frames_to_ticks, player_attack_ticks, MagicEventTimeline, BATTLE_FADE_TICKS,
};
use super::menu_state::{update_wrapping_selection, InventoryMenu, InventoryMode};
use super::session::{BattleDebugHit, BattleDebugItemStart, BattleDebugTarget, DesktopSession};

pub(super) const ACTION_EVENT_TICKS: u16 = 8;
pub(super) const PLAYER_MAGIC_ANIMATION_EVENT_TICKS: u16 = 51;
pub(super) const FLEE_SUCCESS_EVENT_TICKS: u16 = 17;
pub(super) const FLEE_FAILURE_EVENT_TICKS: u16 = 11;
pub(super) const ENEMY_DIVIDE_EVENT_TICKS: u16 = 11;
pub(super) const ENEMY_TRANSFORM_EVENT_TICKS: u16 = 35;
const ROUND_EVENT_TICKS: u16 = 8;
const FINISHED_EVENT_TICKS: u16 = 4;
const SETTLEMENT_MS: u64 = 3_000;
const BOSS_SETTLEMENT_MS: u64 = 5_500;
const POST_BATTLE_PAGE_MS: u64 = 3_000;

pub(super) struct FinishedBattle {
    pub(super) result: BattleResult,
}

pub(super) fn update_battle(
    input: GameInput,
    any_pressed: bool,
    game: &mut GameState,
    services: &mut DesktopSession,
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
        if !game
            .battle()
            .is_some_and(|battle| battle.victory_rewards_pending())
        {
            let (result, _) = game.settle_battle()?;
            return Some(FinishedBattle { result });
        }
        let wait_ticks = game.battle().map_or(0, |battle| {
            if result == BattleResult::Won && battle.rewards().experience > 0 {
                if battle.is_boss {
                    battle_milliseconds_to_ticks(BOSS_SETTLEMENT_MS)
                } else {
                    battle_milliseconds_to_ticks(SETTLEMENT_MS)
                }
            } else {
                0
            }
        });
        if !wait_elapsed(
            &mut services.battle.battle_settlement_ticks,
            wait_ticks,
            any_pressed,
        ) {
            return None;
        }
        services.battle.battle_settlement_ticks = None;
        services.clear_active_menu();
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
        let settlement = game.prepare_battle_victory_settlement()?;
        let pages = settlement_pages(game, &before, &settlement.players);
        if !pages.is_empty() {
            services.battle.post_battle = Some(PostBattlePresentation {
                battle,
                pages,
                page: 0,
                ticks_remaining: battle_milliseconds_to_ticks(POST_BATTLE_PAGE_MS),
                presented_page: None,
            });
            return None;
        }
        game.begin_battle_end_scripts();
        return None;
    }

    if game.auto_battle() {
        commit_forced_magic_or_attack(game, services, 9999);
        return None;
    }

    let living = game
        .battle()?
        .enemies
        .iter()
        .enumerate()
        .filter_map(|(index, enemy)| enemy.is_alive().then_some(index))
        .collect::<Vec<_>>();
    if update_battle_item_menu(input, game, services, &living) {
        return None;
    }

    let input = prioritize_battle_shortcut_direction(input, &services.battle.battle_menu);
    if input.battle_auto {
        services.battle.battle_auto_attack = !services.battle.battle_auto_attack;
        if let Some(battle) = game.battle_mut() {
            battle.set_auto_attack_mode(services.battle.battle_auto_attack);
        }
        services.battle.battle_menu = BattleMenuState::Main;
    }
    if services.battle.battle_auto_attack && input.cancel {
        services.battle.battle_auto_attack = false;
        if let Some(battle) = game.battle_mut() {
            battle.set_auto_attack_mode(false);
        }
        return None;
    }
    if input.battle_status {
        services.battle.battle_menu = BattleMenuState::Status { selected: 0 };
        return None;
    }

    if matches!(services.battle.battle_menu, BattleMenuState::Main) {
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
            services.battle.battle_repeat_all = true;
            services.battle.battle_auto_attack = game
                .battle()
                .is_some_and(|battle| battle.previous_round_used_auto_attack());
        }
        if services.battle.battle_repeat_all {
            let committed = game.repeat_battle_action();
            commit_battle_action(game, services, committed);
            if game
                .battle()
                .and_then(|battle| battle.active_player())
                .is_none()
            {
                services.battle.battle_repeat_all = false;
            }
            return None;
        }
        if input.battle_force || services.battle.battle_force_all {
            services.battle.battle_force_all = true;
            commit_forced_magic_or_attack(game, services, 60);
            return None;
        }
    }
    if services.battle.battle_auto_attack {
        commit_automatic_attack(game, services);
        return None;
    }

    update_battle_menu(input, game, services, &living);

    if let Some(target) = game
        .battle()
        .and_then(|battle| battle.first_living_enemy())
        .filter(|_| {
            game.battle()
                .and_then(|battle| battle.enemies.get(services.battle.battle_selected_enemy))
                .is_none_or(|enemy| !enemy.is_alive())
        })
    {
        services.battle.battle_selected_enemy = target;
    }

    None
}

fn prioritize_battle_shortcut_direction(mut input: GameInput, menu: &BattleMenuState) -> GameInput {
    let global_shortcut = input.battle_auto || input.battle_status;
    let main_wasd_shortcut =
        matches!(menu, BattleMenuState::Main) && (input.battle_defend || input.battle_throw_item);
    if global_shortcut || main_wasd_shortcut {
        input.direction_pressed = None;
    }
    input
}

pub(super) fn advance_post_battle(
    any_pressed: bool,
    game: &mut GameState,
    services: &mut DesktopSession,
) -> bool {
    let Some(presentation) = services.battle.post_battle.as_mut() else {
        return false;
    };
    if !advance_post_battle_countdown(
        presentation.page,
        presentation.presented_page,
        &mut presentation.ticks_remaining,
        any_pressed,
    ) {
        return false;
    }
    presentation.page += 1;
    if presentation.page < presentation.pages.len() {
        presentation.ticks_remaining = battle_milliseconds_to_ticks(POST_BATTLE_PAGE_MS);
        return false;
    }
    services.battle.post_battle.take();
    game.begin_battle_end_scripts()
}

fn settlement_pages<M: pal_core::game::CollisionMap>(
    game: &GameState<M>,
    before: &[(u16, pal_assets::player_roles::PlayerRole)],
    settlements: &[BattlePlayerSettlement],
) -> Vec<BattleSettlementPage> {
    let mut pages = Vec::new();
    for (role_id, previous) in before {
        let Some(settlement) = settlements
            .iter()
            .find(|settlement| settlement.role_id == *role_id)
        else {
            continue;
        };
        let Some(current) = game.effective_player_role(*role_id) else {
            continue;
        };
        if settlement.levels_gained != 0 {
            let mut after_primary = current.clone();
            after_primary.max_hp = after_primary
                .max_hp
                .wrapping_sub(settlement.hidden_growth[HIDDEN_EXP_HEALTH]);
            after_primary.max_mp = after_primary
                .max_mp
                .wrapping_sub(settlement.hidden_growth[HIDDEN_EXP_MAGIC]);
            after_primary.attack_strength = after_primary
                .attack_strength
                .wrapping_sub(settlement.hidden_growth[HIDDEN_EXP_ATTACK]);
            after_primary.magic_strength = after_primary
                .magic_strength
                .wrapping_sub(settlement.hidden_growth[HIDDEN_EXP_MAGIC_POWER]);
            after_primary.defense = after_primary
                .defense
                .wrapping_sub(settlement.hidden_growth[HIDDEN_EXP_DEFENSE]);
            after_primary.dexterity = after_primary
                .dexterity
                .wrapping_sub(settlement.hidden_growth[HIDDEN_EXP_DEXTERITY]);
            after_primary.flee_rate = after_primary
                .flee_rate
                .wrapping_sub(settlement.hidden_growth[HIDDEN_EXP_FLEE]);
            after_primary.hp = after_primary.max_hp;
            after_primary.mp = after_primary.max_mp;
            pages.push(BattleSettlementPage::LevelUp {
                before: Box::new(previous.clone()),
                after: Box::new(after_primary),
            });
        }
        for (label, amount) in [
            (49usize, settlement.hidden_growth[HIDDEN_EXP_HEALTH]),
            (50, settlement.hidden_growth[HIDDEN_EXP_MAGIC]),
            (51, settlement.hidden_growth[HIDDEN_EXP_ATTACK]),
            (52, settlement.hidden_growth[HIDDEN_EXP_MAGIC_POWER]),
            (53, settlement.hidden_growth[HIDDEN_EXP_DEFENSE]),
            (54, settlement.hidden_growth[HIDDEN_EXP_DEXTERITY]),
            (55, settlement.hidden_growth[HIDDEN_EXP_FLEE]),
        ] {
            if amount != 0 {
                pages.push(BattleSettlementPage::AttributeGrowth {
                    role_id: *role_id,
                    label,
                    amount,
                });
            }
        }
        for &magic_object in &settlement.learned_magics {
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

fn advance_post_battle_countdown(
    current_page: usize,
    presented_page: Option<usize>,
    ticks: &mut u16,
    skip: bool,
) -> bool {
    // PAL_WaitForAnyKey clears stale input only after the new notice has reached the screen.
    if presented_page != Some(current_page) {
        return false;
    }
    advance_countdown(ticks, skip)
}

fn commit_battle_action(
    game: &GameState,
    services: &mut DesktopSession,
    committed: Option<Vec<BattleEvent>>,
) {
    if let Some(events) = committed {
        let battle = &mut services.battle;
        reset_battle_command_ui(
            &mut battle.battle_menu,
            &mut battle.battle_command_selected,
            &mut battle.battle_targeting_enemy,
        );
        if !events.is_empty() {
            queue_battle_events(game, services, events);
        }
    }
}

fn reset_battle_command_ui(
    menu: &mut BattleMenuState,
    selected_command: &mut usize,
    targeting_enemy: &mut bool,
) {
    *menu = BattleMenuState::Main;
    *selected_command = 0;
    *targeting_enemy = false;
}

fn commit_normal_attack(game: &mut GameState, services: &mut DesktopSession) {
    let Some(target) = game.battle().and_then(|battle| battle.first_living_enemy()) else {
        return;
    };
    let committed = game.battle_mut().and_then(|battle| battle.attack(target));
    commit_battle_action(game, services, committed);
}

fn commit_automatic_attack(game: &mut GameState, services: &mut DesktopSession) {
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
    services: &mut DesktopSession,
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
        services.battle.battle_force_all = false;
    }
}

fn open_battle_inventory(game: &GameState, services: &mut DesktopSession, mode: InventoryMode) {
    let count = match mode {
        InventoryMode::BattleUseItems => game.battle_usable_inventory().len(),
        InventoryMode::BattleThrowItems => game.throwable_inventory().len(),
        _ => return,
    };
    services.menus.inventory_selected = services
        .menus
        .inventory_selected
        .min(count.saturating_sub(1));
    services.set_inventory_menu(InventoryMenu {
        selected: services.menus.inventory_selected,
        mode,
    });
}

fn update_battle_menu(
    input: GameInput,
    game: &mut GameState,
    services: &mut DesktopSession,
    living: &[usize],
) {
    match services.battle.battle_menu {
        BattleMenuState::Main => update_battle_main_menu(input, game, services, living),
        BattleMenuState::Magic { mut selected } => {
            let magic_count = game
                .battle()
                .and_then(|battle| battle.players.get(battle.active_player()?))
                .map_or(0, |player| player.magics.len());
            update_grid_selection(&mut selected, input.direction_pressed, magic_count);
            services.battle.battle_menu = BattleMenuState::Magic { selected };
            if input.cancel {
                services.battle.battle_menu = BattleMenuState::Main;
            } else if input.confirm {
                begin_magic_selection(game, services, selected, living);
            }
        }
        BattleMenuState::Misc { mut selected } => {
            update_wrapping_selection(&mut selected, input.direction_pressed, 5);
            services.battle.battle_menu = BattleMenuState::Misc { selected };
            if input.cancel {
                services.battle.battle_menu = BattleMenuState::Main;
            } else if input.confirm {
                match selected {
                    0 => {
                        services.battle.battle_auto_attack = true;
                        services.battle.battle_menu = BattleMenuState::Main;
                    }
                    1 => services.battle.battle_menu = BattleMenuState::ItemSubmenu { selected: 0 },
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
                    _ => services.battle.battle_menu = BattleMenuState::Status { selected: 0 },
                }
            }
        }
        BattleMenuState::ItemSubmenu { mut selected } => {
            match input.direction_pressed {
                Some(Direction::North | Direction::West) => selected = 0,
                Some(Direction::South | Direction::East) => selected = 1,
                None => selected = selected.min(1),
            }
            services.battle.battle_menu = BattleMenuState::ItemSubmenu { selected };
            if input.cancel {
                services.battle.battle_menu = BattleMenuState::Misc { selected: 1 };
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
                services.battle.battle_selected_enemy = select_enemy(
                    living,
                    services.battle.battle_selected_enemy,
                    input.direction_pressed,
                );
            }
            services.battle.battle_targeting_enemy = true;
            if input.cancel {
                services.battle.battle_targeting_enemy = false;
                services.battle.battle_menu = match command {
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
                        .and_then(|battle| battle.attack(services.battle.battle_selected_enemy)),
                    BattlePendingCommand::Magic(magic) => game.battle_mut().and_then(|battle| {
                        battle.cast_magic_at(
                            magic,
                            BattleTarget::Enemy(services.battle.battle_selected_enemy),
                        )
                    }),
                    BattlePendingCommand::CooperativeMagic => {
                        game.battle_mut().and_then(|battle| {
                            battle.cast_cooperative_magic(BattleTarget::Enemy(
                                services.battle.battle_selected_enemy,
                            ))
                        })
                    }
                    BattlePendingCommand::ThrowItem(item) => {
                        game.battle_throw_item(item, Some(services.battle.battle_selected_enemy))
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
            services.battle.battle_menu = BattleMenuState::TargetPlayer { command, selected };
            if input.cancel {
                services.battle.battle_menu = match command {
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
                services.battle.battle_menu = BattleMenuState::Main;
            } else {
                services.battle.battle_menu = BattleMenuState::Status { selected };
            }
        }
    }
}

pub(super) fn queue_battle_events(
    game: &GameState,
    services: &mut DesktopSession,
    events: impl IntoIterator<Item = BattleEvent>,
) {
    let events = events.into_iter().collect::<Vec<_>>();
    if let Some(hit) = latest_battle_debug_hit(game, &events) {
        services.battle.battle_debug_hit = Some(hit);
    }
    update_battle_item_debug(game, services, &events);
    let was_empty = services.battle.battle_events.is_empty();
    services.battle.battle_events.extend(events);
    if !was_empty {
        return;
    }
    let Some(&event) = services.battle.battle_events.front() else {
        return;
    };
    services.battle.battle_event_ticks = dynamic_battle_event_duration(game, services, event);
    services.battle.battle_effect_sound_count = 0;
    services.battle.battle_feedback_sound_played = false;
    play_battle_event_sounds(game, services, event);
    play_due_magic_sounds(game, services);
}

fn update_battle_item_debug(
    game: &GameState,
    services: &mut DesktopSession,
    events: &[BattleEvent],
) {
    let Some(battle) = game.battle() else {
        return;
    };
    for &event in events {
        match event {
            BattleEvent::PlayerThrowItem {
                player,
                item_object,
                target,
            } => {
                services.battle.battle_debug_item_start = Some(BattleDebugItemStart {
                    player,
                    item_object,
                    target,
                    enemy_hp: battle.enemies.iter().map(|enemy| enemy.hp).collect(),
                });
            }
            BattleEvent::PlayerItemFeedback {
                player,
                item_object,
                ..
            } => {
                let Some(start) = services.battle.battle_debug_item_start.take() else {
                    continue;
                };
                if start.player != player || start.item_object != item_object {
                    services.battle.battle_debug_item_start = Some(start);
                    continue;
                }
                let target = start.target.or_else(|| {
                    battle
                        .enemies
                        .iter()
                        .zip(&start.enemy_hp)
                        .position(|(enemy, &before)| enemy.hp != before)
                });
                let Some(target_index) = target else {
                    continue;
                };
                let Some((&hp_before, enemy)) = start
                    .enemy_hp
                    .get(target_index)
                    .zip(battle.enemies.get(target_index))
                else {
                    continue;
                };
                services.battle.battle_debug_hit = Some(BattleDebugHit {
                    action: "ITEM.TOTAL",
                    source: player,
                    object_id: Some(start.item_object),
                    target: BattleDebugTarget::Enemy,
                    target_index,
                    damage: item_total_word_damage(hp_before, enemy.hp),
                    hp_before: Some(hp_before),
                    hp_after: enemy.hp,
                    defeated: !enemy.is_alive(),
                });
            }
            _ => {}
        }
    }
}

fn latest_battle_debug_hit(game: &GameState, events: &[BattleEvent]) -> Option<BattleDebugHit> {
    let battle = game.battle()?;
    let mut enemy_hp = battle
        .enemies
        .iter()
        .map(|enemy| enemy.hp)
        .collect::<Vec<_>>();
    let mut player_hp = battle
        .players
        .iter()
        .map(|player| player.hp)
        .collect::<Vec<_>>();
    let mut latest = None;

    // Core resolves a whole action before desktop feedback starts. Walk the resulting events
    // backwards so multi-target and double-hit actions retain their event-local HP delta. A
    // later direct script mutation can still make this inferred value differ from live raw HP.
    for &event in events.iter().rev() {
        let (action, source, object_id, target, target_index, damage, defeated) = match event {
            BattleEvent::PlayerAttack {
                player,
                enemy,
                damage,
                defeated,
                ..
            } => (
                "P.ATTACK",
                player,
                None,
                BattleDebugTarget::Enemy,
                enemy,
                damage,
                defeated,
            ),
            BattleEvent::PlayerMagic {
                player,
                enemy,
                magic_object,
                damage,
                phase: MagicEventPhase::Feedback,
                defeated,
                ..
            } => (
                "P.MAGIC",
                player,
                Some(magic_object),
                BattleDebugTarget::Enemy,
                enemy,
                damage,
                defeated,
            ),
            BattleEvent::PlayerCooperativeMagic {
                player,
                enemy,
                magic_object,
                damage,
                defeated,
                ..
            } => (
                "COOP",
                player,
                Some(magic_object),
                BattleDebugTarget::Enemy,
                enemy,
                damage,
                defeated,
            ),
            BattleEvent::SimulatedMagic {
                enemy,
                magic,
                damage,
                defeated,
                ..
            } => (
                "SIM.MAGIC",
                0,
                Some(magic.object_id),
                BattleDebugTarget::Enemy,
                enemy,
                damage,
                defeated,
            ),
            BattleEvent::EnemyConfusedAttack {
                enemy,
                target,
                damage,
                defeated,
            } => (
                "E.CONF",
                enemy,
                None,
                BattleDebugTarget::Enemy,
                target,
                damage,
                defeated,
            ),
            BattleEvent::EnemyAttack {
                enemy,
                player,
                damage,
                defeated,
                ..
            } => (
                "E.ATTACK",
                enemy,
                None,
                BattleDebugTarget::Player,
                player,
                damage,
                defeated,
            ),
            BattleEvent::EnemyMagic {
                enemy,
                player,
                magic_object,
                damage,
                phase: MagicEventPhase::Feedback,
                defeated,
                ..
            } => (
                "E.MAGIC",
                enemy,
                Some(magic_object),
                BattleDebugTarget::Player,
                player,
                damage,
                defeated,
            ),
            BattleEvent::PlayerConfusedAttack {
                player,
                target,
                damage,
                defeated,
            } => (
                "P.CONF",
                player,
                None,
                BattleDebugTarget::Player,
                target,
                damage,
                defeated,
            ),
            BattleEvent::PlayerMagic {
                phase: MagicEventPhase::Visual,
                ..
            }
            | BattleEvent::EnemyMagic {
                phase: MagicEventPhase::Visual,
                ..
            }
            | BattleEvent::PlayerUseItem { .. }
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
            | BattleEvent::Finished(_) => continue,
        };

        let (hp_before, hp_after) = match target {
            BattleDebugTarget::Enemy => {
                let hp_after = *enemy_hp.get(target_index)?;
                let hp_before = enemy_hp_before_damage(hp_after, damage);
                enemy_hp[target_index] = hp_before;
                (Some(hp_before), hp_after)
            }
            BattleDebugTarget::Player => {
                let hp_after = *player_hp.get(target_index)?;
                let hp_before = hp_after.checked_add(damage);
                if let Some(hp_before) = hp_before {
                    player_hp[target_index] = hp_before;
                }
                (hp_before, hp_after)
            }
        };
        if latest.is_none() {
            latest = Some(BattleDebugHit {
                action,
                source,
                object_id,
                target,
                target_index,
                damage,
                hp_before,
                hp_after,
                defeated,
            });
        }
    }
    latest
}

fn enemy_hp_before_damage(hp_after: u16, damage: u16) -> u16 {
    hp_after.wrapping_add(damage)
}

fn item_total_word_damage(hp_before: u16, hp_after: u16) -> u16 {
    hp_before.wrapping_sub(hp_after)
}

fn update_battle_item_menu(
    input: GameInput,
    game: &mut GameState,
    services: &mut DesktopSession,
    living: &[usize],
) -> bool {
    let Some(mut menu) = services.take_inventory_menu() else {
        return false;
    };
    match menu.mode {
        InventoryMode::BattleUseItems => {
            let inventory = game.battle_usable_inventory();
            menu.selected = menu.selected.min(inventory.len().saturating_sub(1));
            menu.update(input.direction_pressed, inventory.len());
            services.menus.inventory_selected = menu.selected;
            if input.cancel {
                services.battle.battle_menu = BattleMenuState::Main;
                return true;
            }
            if input.confirm {
                if let Some(item) = inventory.get(menu.selected).copied() {
                    if item.apply_to_all {
                        let committed = game.battle_use_item(item.item_id, None);
                        if committed.is_some() {
                            commit_battle_action(game, services, committed);
                            return true;
                        }
                    } else {
                        let player_count = game.battle().map_or(0, |battle| battle.players.len());
                        if player_count == 1 {
                            let committed = game.battle_use_item(item.item_id, Some(0));
                            if committed.is_some() {
                                commit_battle_action(game, services, committed);
                                return true;
                            }
                            services.set_inventory_menu(menu);
                            return true;
                        }
                        services.battle.battle_menu = BattleMenuState::TargetPlayer {
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
            services.menus.inventory_selected = menu.selected;
            if input.cancel {
                services.battle.battle_menu = BattleMenuState::Main;
                return true;
            }
            if input.confirm {
                if let Some(item) = inventory.get(menu.selected).copied() {
                    if let Some(target) = immediate_throw_target(item.apply_to_all, living) {
                        let committed = game.battle_throw_item(item.item_id, target);
                        if committed.is_some() {
                            commit_battle_action(game, services, committed);
                            return true;
                        }
                    } else if !living.is_empty() {
                        services.battle.battle_menu = BattleMenuState::TargetEnemy {
                            command: BattlePendingCommand::ThrowItem(item.item_id),
                        };
                        services.battle.battle_targeting_enemy = true;
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
    services.set_inventory_menu(menu);
    true
}

/// All-target throws and single living enemies do not need a target-selection screen.
fn immediate_throw_target(apply_to_all: bool, living: &[usize]) -> Option<Option<usize>> {
    if apply_to_all {
        Some(None)
    } else if let [target] = living {
        Some(Some(*target))
    } else {
        None
    }
}

fn advance_battle_events(game: &GameState, services: &mut DesktopSession) -> bool {
    let completed = services.battle.battle_events.front().copied();
    match tick_battle_event_queue(
        &mut services.battle.battle_events,
        &mut services.battle.battle_event_ticks,
    ) {
        BattleEventTick::Idle => false,
        BattleEventTick::Started(event) => {
            retain_completed_magic_effect(game, services, completed);
            services.battle.battle_event_ticks =
                dynamic_battle_event_duration(game, services, event);
            services.battle.battle_effect_sound_count = 0;
            services.battle.battle_feedback_sound_played = false;
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
            services.battle.battle_effect_sound_count = 0;
            services.battle.battle_feedback_sound_played = false;
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
        BattleEvent::PlayerAttack { .. } => player_attack_ticks(event),
        BattleEvent::PlayerMagic { .. }
        | BattleEvent::EnemyAttack { .. }
        | BattleEvent::EnemyMagic { .. }
        | BattleEvent::EnemyConfusedAttack { .. }
        | BattleEvent::PlayerConfusedAttack { .. }
        | BattleEvent::SimulatedMagic { .. }
        | BattleEvent::PlayerDefensiveMagic { .. }
        | BattleEvent::PlayerCooperativeMagic { .. } => ACTION_EVENT_TICKS,
        BattleEvent::PlayerFlee {
            succeeded: true, ..
        } => FLEE_SUCCESS_EVENT_TICKS,
        BattleEvent::PlayerFlee {
            succeeded: false, ..
        } => FLEE_FAILURE_EVENT_TICKS,
        BattleEvent::PlayerDefend { .. } => 1,
        BattleEvent::PlayerUseItem { .. } => original_frames_to_ticks(17),
        BattleEvent::PlayerThrowItem { .. } => original_frames_to_ticks(16),
        BattleEvent::PlayerItemFeedback { .. } => original_frames_to_ticks(8),
        BattleEvent::PlayerMagicAnimation { .. } => PLAYER_MAGIC_ANIMATION_EVENT_TICKS,
        BattleEvent::PlayerFriendDeath { .. } | BattleEvent::PlayerDying { .. } => {
            original_frames_to_ticks(10)
        }
        BattleEvent::EnemyDivide { .. } => ENEMY_DIVIDE_EVENT_TICKS,
        BattleEvent::EnemySummon { .. } => BATTLE_FADE_TICKS.saturating_mul(2).saturating_add(2),
        BattleEvent::EnemyTransform { .. } => ENEMY_TRANSFORM_EVENT_TICKS,
        BattleEvent::EnemyEscape => enemy_escape_timeline(0).total_ticks,
        BattleEvent::RoundCompleted => ROUND_EVENT_TICKS,
        BattleEvent::Finished(_) => FINISHED_EVENT_TICKS,
    }
}

fn dynamic_battle_event_duration(
    game: &GameState,
    services: &DesktopSession,
    event: BattleEvent,
) -> u16 {
    if let Some((_, timing)) = session_magic_timing(game, services, event) {
        return timing.total_ticks;
    }
    let Some(battle) = game.battle() else {
        return battle_event_duration(event);
    };
    match event {
        BattleEvent::PlayerAttack { .. } => player_attack_ticks(event),
        BattleEvent::EnemyAttack { .. } => {
            original_frames_to_ticks(enemy_attack_frames(battle, event))
        }
        BattleEvent::EnemyConfusedAttack { defeated, .. } => {
            original_frames_to_ticks(enemy_attack_frames(battle, event))
                .saturating_add(if defeated { BATTLE_FADE_TICKS } else { 0 })
        }
        BattleEvent::EnemySummon {
            caster,
            summoned_mask,
        } => battle
            .enemies
            .get(caster)
            .map(|enemy| {
                let pre_ticks = original_frames_to_ticks(
                    usize::from(enemy.magic_frames)
                        .saturating_mul(usize::from(enemy.action_wait_frames.max(1))),
                );
                if summoned_mask == 0 {
                    pre_ticks.max(1)
                } else {
                    pre_ticks
                        .saturating_add(BATTLE_FADE_TICKS.saturating_mul(2))
                        .saturating_add(2)
                }
            })
            .unwrap_or_else(|| battle_event_duration(event)),
        BattleEvent::EnemyEscape => {
            let rightmost_edge = battle
                .enemies
                .iter()
                .filter(|enemy| enemy.is_alive())
                .filter_map(|enemy| {
                    services
                        .battle
                        .enemy_battle_frame_widths
                        .get(usize::from(enemy.enemy_id))
                        .copied()
                        .flatten()
                        .map(|width| i32::from(enemy.position.x) + i32::from(width))
                })
                .max()
                .unwrap_or(0);
            enemy_escape_timeline(rightmost_edge).total_ticks
        }
        BattleEvent::PlayerConfusedAttack { .. } => original_frames_to_ticks(21),
        _ => battle_event_duration(event),
    }
}

fn session_magic_timing(
    game: &GameState,
    services: &DesktopSession,
    event: BattleEvent,
) -> Option<(pal_core::battle::BattleMagic, MagicEventTimeline)> {
    let battle = game.battle()?;
    let magic = battle_magic_for_event(battle, event)?;
    let visual = magic.effect_visual();
    let effect_frame_count = services
        .battle
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
                        .battle
                        .player_battle_frame_counts
                        .get(sprite)
                        .copied()
                        .flatten()
                })
        })
        .flatten();
    Some((
        magic,
        magic_event_timeline(
            event,
            magic,
            effect_frame_count,
            summon_frame_count,
            enemy_magic_pre_frames(battle, event, magic),
        ),
    ))
}

fn retain_completed_magic_effect(
    game: &GameState,
    services: &mut DesktopSession,
    completed: Option<BattleEvent>,
) {
    if completed.is_some_and(|event| matches!(event, BattleEvent::Finished(_))) {
        services.battle.battle_kept_effects.clear();
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
        services.battle.battle_kept_effects.push(event);
    }
}

fn play_due_magic_sounds(game: &GameState, services: &mut DesktopSession) {
    let Some(event) = services.battle.battle_events.front().copied() else {
        return;
    };
    match event {
        BattleEvent::PlayerUseItem { .. } => {
            let total = original_frames_to_ticks(17);
            let elapsed = total.saturating_sub(services.battle.battle_event_ticks.min(total));
            if elapsed >= original_frames_to_ticks(4)
                && !services.battle.battle_feedback_sound_played
            {
                services.audio.sound_effects.play(28);
                services.battle.battle_feedback_sound_played = true;
            }
            return;
        }
        BattleEvent::PlayerThrowItem { player, .. } => {
            let total = original_frames_to_ticks(16);
            let elapsed = total.saturating_sub(services.battle.battle_event_ticks.min(total));
            if elapsed >= original_frames_to_ticks(6)
                && !services.battle.battle_feedback_sound_played
            {
                if let Some(sound) = game
                    .battle()
                    .and_then(|battle| battle.players.get(player))
                    .map(|player| player.magic_sound)
                    .filter(|&sound| sound != 0)
                {
                    services.audio.sound_effects.play(sound);
                }
                services.battle.battle_feedback_sound_played = true;
            }
            return;
        }
        BattleEvent::EnemySummon {
            caster,
            summoned_mask,
        } => {
            let pre_ticks = game
                .battle()
                .and_then(|battle| battle.enemies.get(caster))
                .map_or(0, |enemy| {
                    original_frames_to_ticks(
                        usize::from(enemy.magic_frames)
                            .saturating_mul(usize::from(enemy.action_wait_frames.max(1))),
                    )
                });
            let total_ticks = if summoned_mask == 0 {
                pre_ticks.max(1)
            } else {
                pre_ticks
                    .saturating_add(BATTLE_FADE_TICKS.saturating_mul(2))
                    .saturating_add(2)
            };
            let elapsed =
                total_ticks.saturating_sub(services.battle.battle_event_ticks.min(total_ticks));
            if summoned_mask != 0
                && elapsed >= pre_ticks
                && !services.battle.battle_feedback_sound_played
            {
                services.audio.sound_effects.play(212);
                services.battle.battle_feedback_sound_played = true;
            }
            return;
        }
        BattleEvent::EnemyTransform { .. } => {
            let elapsed = ENEMY_TRANSFORM_EVENT_TICKS.saturating_sub(
                services
                    .battle
                    .battle_event_ticks
                    .min(ENEMY_TRANSFORM_EVENT_TICKS),
            );
            if elapsed >= 6 && !services.battle.battle_feedback_sound_played {
                services.audio.sound_effects.play(47);
                services.battle.battle_feedback_sound_played = true;
            }
            return;
        }
        _ => {}
    }
    let Some((magic, timeline)) = session_magic_timing(game, services, event) else {
        return;
    };
    let elapsed = timeline
        .total_ticks
        .saturating_sub(services.battle.battle_event_ticks.min(timeline.total_ticks));
    if !event_has_full_magic_visual(event) {
        services.battle.battle_effect_sound_count = u16::MAX;
    } else {
        let visual = magic.effect_visual();
        let frame_count = services
            .battle
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
                let cycle = usize::from(services.battle.battle_effect_sound_count);
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
                        services.audio.sound_effects.play(sound);
                    }
                }
                services.battle.battle_effect_sound_count =
                    services.battle.battle_effect_sound_count.saturating_add(1);
            }
        }
    }
    if !services.battle.battle_feedback_sound_played && elapsed >= timeline.tail_start() {
        if let Some(sound) = magic_feedback_sound(game, event).filter(|sound| *sound != 0) {
            services.audio.sound_effects.play(sound);
        }
        services.battle.battle_feedback_sound_played = true;
    }
}

fn magic_feedback_sound(game: &GameState, event: BattleEvent) -> Option<u16> {
    let battle = game.battle()?;
    match event {
        BattleEvent::PlayerMagic {
            enemy,
            defeated,
            phase: MagicEventPhase::Feedback,
            ..
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
            player,
            defeated,
            phase: MagicEventPhase::Feedback,
            ..
        } => defeated.then(|| battle.players.get(player).map(|player| player.death_sound))?,
        BattleEvent::PlayerDefensiveMagic { .. } => None,
        BattleEvent::EnemyDivide { .. }
        | BattleEvent::EnemySummon { .. }
        | BattleEvent::EnemyTransform { .. }
        | BattleEvent::EnemyEscape => None,
        _ => None,
    }
}

fn victory_music_track(event: BattleEvent, experience: u32, is_boss: bool) -> Option<u16> {
    (event == BattleEvent::Finished(BattleResult::Won) && experience > 0).then_some(if is_boss {
        2
    } else {
        3
    })
}

fn play_battle_event_sounds(game: &GameState, services: &mut DesktopSession, event: BattleEvent) {
    if let Some(track) = game
        .battle()
        .and_then(|battle| victory_music_track(event, battle.rewards().experience, battle.is_boss))
    {
        let _ = services.audio.music.play(track, false, 0);
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
        BattleEvent::PlayerMagic {
            player,
            phase: MagicEventPhase::Visual,
            ..
        } => game.battle().and_then(|battle| {
            let player = battle.players.get(player)?;
            Some(vec![Some(player.magic_sound), None, None])
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
        BattleEvent::EnemyMagic {
            enemy,
            phase: MagicEventPhase::Visual,
            ..
        } => game.battle().and_then(|battle| {
            let enemy = battle.enemies.get(enemy)?;
            Some(vec![u16::try_from(enemy.magic_sound).ok(), None, None])
        }),
        BattleEvent::PlayerMagic {
            phase: MagicEventPhase::Feedback,
            ..
        }
        | BattleEvent::EnemyMagic {
            phase: MagicEventPhase::Feedback,
            ..
        } => None,
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
        BattleEvent::PlayerUseItem { .. } | BattleEvent::PlayerThrowItem { .. } => None,
        BattleEvent::PlayerDefensiveMagic { player, .. } => game.battle().and_then(|battle| {
            let player = battle.players.get(player)?;
            Some(vec![Some(player.magic_sound), None, None])
        }),
        BattleEvent::PlayerDefend { .. } => None,
        BattleEvent::PlayerItemFeedback { .. } => None,
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
        BattleEvent::EnemyDivide { .. }
        | BattleEvent::EnemySummon { .. }
        | BattleEvent::EnemyTransform { .. } => None,
        BattleEvent::EnemyEscape => Some(vec![Some(45), None, None]),
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
        services.audio.sound_effects.play(sound);
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
    services: &mut DesktopSession,
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
    services.battle.battle_command_selected = select_battle_command(
        services.battle.battle_command_selected,
        input.direction_pressed,
        magic_enabled,
        cooperative_magic_enabled,
    );
    if input.cancel {
        if game
            .battle_mut()
            .and_then(|battle| battle.undo_last_command())
            .is_some()
        {
            let battle = &mut services.battle;
            reset_battle_command_ui(
                &mut battle.battle_menu,
                &mut battle.battle_command_selected,
                &mut battle.battle_targeting_enemy,
            );
        }
        return;
    }
    if !input.confirm {
        return;
    }
    match services.battle.battle_command_selected {
        0 => {
            let attacks_all = game
                .battle()
                .and_then(|battle| battle.players.get(battle.active_player()?))
                .is_some_and(|player| player.attacks_all);
            if attacks_all || living.len() <= 1 {
                commit_normal_attack(game, services);
            } else {
                services.battle.battle_menu = BattleMenuState::TargetEnemy {
                    command: BattlePendingCommand::Attack,
                };
                services.battle.battle_targeting_enemy = true;
            }
        }
        1 => {
            services.battle.battle_menu = classic_battle_magic_menu();
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
                services.battle.battle_menu = BattleMenuState::TargetEnemy {
                    command: BattlePendingCommand::CooperativeMagic,
                };
                services.battle.battle_targeting_enemy = true;
            }
        }
        _ => services.battle.battle_menu = BattleMenuState::Misc { selected: 0 },
    }
}

fn begin_magic_selection(
    game: &mut GameState,
    services: &mut DesktopSession,
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
            services.battle.battle_menu = BattleMenuState::TargetEnemy {
                command: BattlePendingCommand::Magic(selected),
            };
            services.battle.battle_targeting_enemy = true;
            return;
        }
        (false, true) => game
            .battle_mut()
            .and_then(|battle| battle.cast_magic_at(selected, BattleTarget::AllPlayers)),
        (false, false) if player_count <= 1 => game
            .battle_mut()
            .and_then(|battle| battle.cast_magic_at(selected, BattleTarget::Player(0))),
        (false, false) => {
            services.battle.battle_menu = BattleMenuState::TargetPlayer {
                command: BattlePendingCommand::Magic(selected),
                selected: 0,
            };
            return;
        }
    };
    commit_battle_action(game, services, committed);
}

fn classic_battle_magic_menu() -> BattleMenuState {
    // Classic passes magic object 0 as the default every time it opens this menu. Since object 0
    // is not a learned magic, the cursor always remains on the first entry.
    BattleMenuState::Magic { selected: 0 }
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

    #[derive(Clone, Copy)]
    struct TestMap;

    impl pal_core::game::CollisionMap for TestMap {
        fn is_world_blocked(&self, _world_x: i32, _world_y: i32) -> bool {
            false
        }

        fn world_size(&self) -> (i32, i32) {
            (640, 400)
        }
    }

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
    fn throw_items_skip_target_selection_for_all_or_one_enemy() {
        assert_eq!(immediate_throw_target(true, &[1, 3]), Some(None));
        assert_eq!(immediate_throw_target(false, &[3]), Some(Some(3)));
        assert_eq!(immediate_throw_target(false, &[1, 3]), None);
        assert_eq!(immediate_throw_target(false, &[]), None);
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
    fn each_classic_battle_magic_menu_starts_from_the_first_spell() {
        assert_eq!(
            classic_battle_magic_menu(),
            BattleMenuState::Magic { selected: 0 }
        );
    }

    #[test]
    fn committed_or_reopened_player_commands_reset_to_attack() {
        let mut menu = BattleMenuState::TargetEnemy {
            command: BattlePendingCommand::Magic(4),
        };
        let mut selected_command = 1;
        let mut targeting_enemy = true;

        reset_battle_command_ui(&mut menu, &mut selected_command, &mut targeting_enemy);

        assert_eq!(menu, BattleMenuState::Main);
        assert_eq!(selected_command, 0);
        assert!(!targeting_enemy);
    }

    #[test]
    fn classic_shortcuts_do_not_also_move_the_main_menu() {
        for input in [
            GameInput {
                direction_pressed: Some(Direction::West),
                battle_auto: true,
                ..GameInput::default()
            },
            GameInput {
                direction_pressed: Some(Direction::East),
                battle_defend: true,
                ..GameInput::default()
            },
            GameInput {
                direction_pressed: Some(Direction::North),
                battle_throw_item: true,
                ..GameInput::default()
            },
            GameInput {
                direction_pressed: Some(Direction::South),
                battle_status: true,
                ..GameInput::default()
            },
        ] {
            assert_eq!(
                prioritize_battle_shortcut_direction(input, &BattleMenuState::Main)
                    .direction_pressed,
                None
            );
        }
    }

    #[test]
    fn battle_submenus_keep_wasd_navigation_for_main_only_shortcuts() {
        let magic_menu = BattleMenuState::Magic { selected: 0 };
        let throw_key = GameInput {
            direction_pressed: Some(Direction::North),
            battle_throw_item: true,
            ..GameInput::default()
        };
        let defend_key = GameInput {
            direction_pressed: Some(Direction::East),
            battle_defend: true,
            ..GameInput::default()
        };

        assert_eq!(
            prioritize_battle_shortcut_direction(throw_key, &magic_menu).direction_pressed,
            Some(Direction::North)
        );
        assert_eq!(
            prioritize_battle_shortcut_direction(defend_key, &magic_menu).direction_pressed,
            Some(Direction::East)
        );
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
        let settlement_ticks = battle_milliseconds_to_ticks(SETTLEMENT_MS);
        assert_eq!(settlement_ticks, 75);
        assert_eq!(battle_milliseconds_to_ticks(BOSS_SETTLEMENT_MS), 138);
        let mut ticks = None;
        for _ in 0..settlement_ticks - 1 {
            assert!(!wait_elapsed(&mut ticks, settlement_ticks, false));
        }
        assert!(wait_elapsed(&mut ticks, settlement_ticks, false));

        let mut skipped = None;
        assert!(wait_elapsed(&mut skipped, settlement_ticks, true));
        assert_eq!(skipped, None);
    }

    #[test]
    fn post_battle_notice_discards_input_until_the_page_has_been_presented() {
        let mut ticks = battle_milliseconds_to_ticks(POST_BATTLE_PAGE_MS);
        assert!(!advance_post_battle_countdown(2, Some(1), &mut ticks, true));
        assert_eq!(ticks, battle_milliseconds_to_ticks(POST_BATTLE_PAGE_MS));

        assert!(advance_post_battle_countdown(2, Some(2), &mut ticks, true));
        assert_eq!(ticks, 0);
    }

    #[test]
    fn victory_music_requires_positive_experience() {
        let won = BattleEvent::Finished(BattleResult::Won);
        assert_eq!(victory_music_track(won, 1, false), Some(3));
        assert_eq!(victory_music_track(won, 1, true), Some(2));
        assert_eq!(victory_music_track(won, 0, false), None);
        assert_eq!(
            victory_music_track(BattleEvent::Finished(BattleResult::Lost), 1, false),
            None
        );
    }

    #[test]
    fn level_up_pages_are_followed_by_hidden_growth_and_learned_magic() {
        use pal_assets::player_roles::{PlayerRoles, PLAYER_ROLE_COUNT};
        use pal_core::party::Party;
        use pal_core::role::Role;

        fn roles(level: u16, max_hp: u16, attack: u16, magic: u16) -> PlayerRoles {
            let mut data = vec![0; 900];
            for (array, value) in [
                (6, level),
                (7, max_hp),
                (9, max_hp),
                (17, attack),
                (32, magic),
            ] {
                let offset = array * PLAYER_ROLE_COUNT * 2;
                data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
            }
            PlayerRoles::parse(&data).unwrap()
        }

        let before_roles = roles(1, 100, 30, 0);
        let previous = before_roles.role(0).unwrap().clone();
        let current_roles = roles(2, 112, 35, 9);
        let party = Party::single(0, &current_roles).unwrap();
        let game = GameState::new(
            TestMap,
            Role {
                sprite_index: 0,
                world_x: 0,
                world_y: 0,
                direction: Direction::South,
                anim_frame: 0,
                frames_per_direction: 4,
            },
            320,
            200,
        )
        .with_party(party)
        .with_player_roles(current_roles);
        let mut hidden_growth = [0; pal_core::battle::HIDDEN_EXPERIENCE_CATEGORY_COUNT];
        hidden_growth[HIDDEN_EXP_HEALTH] = 2;
        hidden_growth[HIDDEN_EXP_ATTACK] = 1;
        let settlements = [BattlePlayerSettlement {
            role_id: 0,
            levels_gained: 1,
            hidden_growth,
            learned_magics: vec![9],
        }];

        let pages = settlement_pages(&game, &[(0, previous)], &settlements);
        assert_eq!(pages.len(), 4);
        assert!(matches!(
            &pages[0],
            BattleSettlementPage::LevelUp { after, .. }
                if after.max_hp == 110 && after.attack_strength == 34
        ));
        assert!(matches!(
            pages[1],
            BattleSettlementPage::AttributeGrowth {
                label: 49,
                amount: 2,
                ..
            }
        ));
        assert!(matches!(
            pages[2],
            BattleSettlementPage::AttributeGrowth {
                label: 51,
                amount: 1,
                ..
            }
        ));
        assert!(matches!(
            pages[3],
            BattleSettlementPage::LearnedMagic {
                magic_object: 9,
                ..
            }
        ));
    }

    #[test]
    fn battle_event_durations_leave_time_for_action_and_settlement_feedback() {
        let action = battle_event_duration(BattleEvent::PlayerAttack {
            player: 0,
            enemy: 0,
            damage: 1,
            critical: false,
            visual: true,
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
        let divide = battle_event_duration(BattleEvent::EnemyDivide {
            origin: pal_assets::battle::BattlePosition { x: 0, y: 0 },
        });
        let summon = battle_event_duration(BattleEvent::EnemySummon {
            caster: 0,
            summoned_mask: 1,
        });
        let transform = battle_event_duration(BattleEvent::EnemyTransform {
            enemy: 0,
            previous_enemy_id: 0,
            previous_y_offset: 0,
        });
        let escape = battle_event_duration(BattleEvent::EnemyEscape);
        assert_eq!((action, magic_animation, finished, round), (16, 51, 4, 8));
        assert_eq!((use_item, throw_item), (17, 16));
        assert_eq!((divide, summon, transform, escape), (11, 60, 35, 13));
    }

    #[test]
    fn battle_event_queue_blocks_until_every_feedback_event_is_drained() {
        let attack = BattleEvent::PlayerAttack {
            player: 0,
            enemy: 0,
            damage: 1,
            critical: false,
            visual: true,
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

    #[test]
    fn battle_debug_reconstructs_enemy_hp_across_word_underflow() {
        assert_eq!(enemy_hp_before_damage(59_402, 6_234), 100);
        assert_eq!(item_total_word_damage(40, 40u16.wrapping_sub(90)), 90);
    }
}
