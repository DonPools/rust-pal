//! Battle rule regression tests.

use super::helpers::{battle_magic, physical_damage};
use super::*;
use pal_assets::battle::{BattleData, BattlePosition};
use pal_assets::magic::Magics;
use pal_assets::objects::{GlobalObjects, ObjectLayout};
use pal_assets::player_roles::PlayerRole;

const ENEMY_BYTES: usize = 70;

fn words(values: &[u16]) -> Vec<u8> {
    values.iter().flat_map(|word| word.to_le_bytes()).collect()
}

fn make_mkf(chunks: &[Vec<u8>]) -> Vec<u8> {
    let table_size = (chunks.len() + 1) * 4;
    let mut offset = table_size as u32;
    let mut data = Vec::new();
    data.extend_from_slice(&offset.to_le_bytes());
    for chunk in chunks {
        offset += chunk.len() as u32;
        data.extend_from_slice(&offset.to_le_bytes());
    }
    for chunk in chunks {
        data.extend_from_slice(chunk);
    }
    data
}

fn fixture(
    enemy_hp: u16,
    enemy_attack: u16,
    player_hp: u16,
) -> (BattleData, GlobalObjects, Magics, PlayerRole) {
    fixture_with_enemy_magic(enemy_hp, enemy_attack, player_hp, 0, 0, 0)
}

fn fixture_with_enemy_magic(
    enemy_hp: u16,
    enemy_attack: u16,
    player_hp: u16,
    enemy_magic: u16,
    magic_rate: u16,
    magic_type: u16,
) -> (BattleData, GlobalObjects, Magics, PlayerRole) {
    let mut chunks = vec![Vec::new(); 15];
    let mut enemy = vec![0; ENEMY_BYTES];
    for (word, value) in [
        (11, enemy_hp),
        (12, 26),
        (13, 48),
        (14, 2),
        (15, enemy_magic),
        (16, magic_rate),
        (17, 9),
        (18, 0),
        (19, 8),
        (20, 2),
        (21, enemy_attack),
        (22, 30),
        (23, 0),
        (32, 1),
        (33, 1),
        (34, 5),
    ] {
        enemy[word * 2..word * 2 + 2].copy_from_slice(&value.to_le_bytes());
    }
    let mut transformed_enemy = enemy.clone();
    for (word, value) in [
        (5, 7u16),
        (11, 240),
        (12, 77),
        (13, 88),
        (14, 9),
        (21, 99),
        (22, 91),
        (23, 83),
        (24, 75),
    ] {
        transformed_enemy[word * 2..word * 2 + 2].copy_from_slice(&value.to_le_bytes());
    }
    chunks[1] = [enemy, transformed_enemy].concat();
    chunks[2] = words(&[1, 1, u16::MAX, u16::MAX, u16::MAX]);
    chunks[5] = vec![0; 12];
    chunks[6] = vec![0; 20];
    chunks[13] = (0..25u16)
        .flat_map(|index| [index + 10, index + 20])
        .flat_map(u16::to_le_bytes)
        .collect();
    chunks[14] = vec![0; 200];
    let battle_data = BattleData::parse(&make_mkf(&chunks)).unwrap();

    let mut object_words = vec![0; 10 * 6];
    object_words[6..12].copy_from_slice(&[0, 0, 11, 12, 13, 0]);
    object_words[12..18].copy_from_slice(&[
        0,
        0,
        32,
        31,
        0,
        MAGIC_FLAG_USABLE_IN_BATTLE
            | MAGIC_FLAG_USABLE_TO_ENEMY
            | if magic_type != 0 {
                MAGIC_FLAG_APPLY_TO_ALL
            } else {
                0
            },
    ]);
    object_words[18..24].copy_from_slice(&[1, 4, 21, 22, 23, 0]);
    object_words[54..60].copy_from_slice(&[0, 0, 33, 0, 0, 0]);
    let objects = GlobalObjects::parse(&words(&object_words), ObjectLayout::Dos).unwrap();

    let role = PlayerRole {
        avatar: 0,
        battle_sprite_num: 0,
        scene_sprite_num: 0,
        name_word_id: 0,
        attack_all: false,
        level: 1,
        max_hp: player_hp,
        max_mp: 0,
        hp: player_hp,
        mp: 0,
        equipment: [0; 6],
        attack_strength: 80,
        magic_strength: 0,
        defense: 20,
        dexterity: 20,
        flee_rate: 20,
        poison_resistance: 0,
        elemental_resistance: [0; 5],
        covered_by: 0,
        magic: [0; 32],
        walk_frames: 3,
        cooperative_magic: 0,
        unknown_5: 0,
        unknown_6: 0,
        death_sound: 21,
        attack_sound: 22,
        weapon_sound: 23,
        critical_sound: 0,
        magic_sound: 24,
        cover_sound: 0,
        dying_sound: 0,
    };
    let mut magic_data = [0; 32];
    magic_data[2..4].copy_from_slice(&magic_type.to_le_bytes());
    magic_data[24..26].copy_from_slice(&5u16.to_le_bytes());
    magic_data[26..28].copy_from_slice(&50u16.to_le_bytes());
    magic_data[30..32].copy_from_slice(&9i16.to_le_bytes());
    let magics = Magics::parse(&magic_data).unwrap();
    (battle_data, objects, magics, role)
}

fn request(is_boss: bool) -> BattleRequest {
    BattleRequest {
        enemy_team: 0,
        lost_entry: 40,
        flee_entry: 50,
        is_boss,
    }
}

fn complete_pending_scripts(battle: &mut BattleState) {
    while let Some(request) = battle.take_script_request() {
        assert!(battle.complete_script(request.entry));
    }
}

fn resolve_until_input_or_finish(battle: &mut BattleState) -> Vec<BattleEvent> {
    let mut events = Vec::new();
    for _ in 0..128 {
        complete_pending_scripts(battle);
        events.extend(battle.advance_resolution());
        if battle.phase() != BattlePhase::AwaitingCommand
            || (battle.flow == BattleFlow::Command && !battle.has_script_work())
        {
            return events;
        }
    }
    panic!("battle resolution did not settle");
}

#[test]
fn creates_enemy_instances_from_team_objects_and_positions() {
    let (data, objects, magics, role) = fixture_with_enemy_magic(100, 10, 100, 2, 7, 0);
    let battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    assert_eq!(battle.players.len(), 1);
    assert_eq!(battle.enemies.len(), 2);
    assert_eq!(battle.enemies[0].position, BattlePosition { x: 11, y: 21 });
    assert_eq!(battle.enemies[1].position, BattlePosition { x: 16, y: 26 });
    assert_eq!(battle.enemies[0].turn_start_script, 11);
    assert_eq!(battle.enemies[0].magic_object, 2);
    assert_eq!(battle.enemies[0].magic_rate, 7);
    assert_eq!(battle.enemies[0].magic_strength, 30);
    assert!(battle.enemies[0].dual_move);
    assert_eq!(battle.enemies[0].collect_value, 5);
    assert_eq!(battle.players[0].death_sound, 21);
    assert_eq!(battle.players[0].attack_sound, 22);
    assert_eq!(battle.players[0].weapon_sound, 23);
    assert_eq!(battle.players[0].magic_sound, 24);
    assert_eq!(battle.rewards(), BattleRewards::default());
}

#[test]
fn command_phase_excludes_round_execution_while_public_phase_is_awaiting_command() {
    let (data, objects, magics, role) = fixture(100, 10, 100);
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut battle);
    assert!(battle.is_command_phase());

    assert!(battle.attack(0).is_some());

    assert_eq!(battle.phase(), BattlePhase::AwaitingCommand);
    assert!(!battle.is_command_phase());
}

#[test]
fn division_uses_free_slots_and_requested_health_divisor() {
    let (data, objects, magics, role) = fixture(100, 10, 100);
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    assert!(battle.divide_enemy(0, 2, &data.enemy_positions).is_none());

    battle.enemies[0]
        .statuses
        .set_for_enemy(BattleStatus::Protect, 4);
    assert!(battle.kill_enemy(1));
    battle.queue_post_action_check(false);
    let added = battle
        .divide_enemy(0, 2, &data.enemy_positions)
        .expect("sole living enemy should divide");
    assert_eq!(added, vec![2, 3]);
    assert_eq!(battle.enemies[0].hp, 34);
    assert_eq!(battle.enemies[2].hp, 34);
    assert_eq!(battle.enemies[3].hp, 34);
    assert_eq!(battle.enemies[2].slot, 1);
    assert_eq!(battle.enemies[3].slot, 2);
    assert_eq!(battle.enemies[1].object_id, 0);
    assert_eq!(battle.enemies[1].battle_end_script, 0);
    assert_eq!(battle.enemy_index_for_slot(1), Some(2));
    assert_eq!(battle.enemies[0].position, BattlePosition { x: 12, y: 22 });
    assert_eq!(battle.enemies[2].position, BattlePosition { x: 17, y: 27 });
    assert_eq!(battle.enemies[3].position, BattlePosition { x: 22, y: 32 });
    assert_eq!(battle.enemies[2].turn_start_script, 11);
    assert!(!battle.enemies[2].statuses.is_active(BattleStatus::Protect));
    let origin = battle.enemies[0].position;
    assert_eq!(
        battle.advance_resolution(),
        vec![BattleEvent::EnemyDivide { origin }]
    );
}

#[test]
fn summon_fills_only_current_layout_holes_and_loads_static_enemy_data() {
    let (data, objects, magics, role) = fixture(100, 10, 100);
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    assert!(battle
        .summon_enemy(0, 3, 1, &data, &objects, &magics)
        .is_none());
    assert_eq!(
        battle.advance_resolution(),
        vec![BattleEvent::EnemySummon {
            caster: 0,
            summoned_mask: 0,
        }]
    );
    assert!(battle.kill_enemy(1));
    battle.queue_post_action_check(false);
    assert!(battle
        .summon_enemy(0, 3, 2, &data, &objects, &magics)
        .is_none());
    assert_eq!(
        battle.advance_resolution(),
        vec![BattleEvent::EnemySummon {
            caster: 0,
            summoned_mask: 0,
        }]
    );

    let added = battle
        .summon_enemy(0, 3, 0, &data, &objects, &magics)
        .expect("zero count should summon one enemy");
    assert_eq!(added, vec![2]);
    let summoned = &battle.enemies[2];
    assert_eq!(summoned.slot, 1);
    assert_eq!(summoned.object_id, 3);
    assert_eq!(summoned.enemy_id, 1);
    assert_eq!(summoned.hp, 240);
    assert_eq!(summoned.attack_strength, 99);
    assert_eq!(summoned.turn_start_script, 21);
    assert_eq!(summoned.battle_end_script, 22);
    assert_eq!(summoned.ready_script, 23);
    assert_eq!(summoned.position, BattlePosition { x: 16, y: 26 });
    assert_eq!(
        battle.advance_resolution(),
        vec![BattleEvent::EnemySummon {
            caster: 0,
            summoned_mask: 0b00010,
        }]
    );
}

#[test]
fn transform_replaces_static_data_but_preserves_runtime_state_and_scripts() {
    let (data, objects, magics, role) = fixture(100, 10, 100);
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    battle.enemies[0].hp = 37;
    battle.enemies[0].turn_start_script = 41;
    battle.enemies[0].battle_end_script = 42;
    battle.enemies[0].ready_script = 43;
    battle.enemies[0]
        .statuses
        .set_for_enemy(BattleStatus::Protect, 4);
    battle.enemies[0].poisons[0] = BattlePoison {
        object_id: 6,
        script_entry: 90,
    };
    let previous_enemy_id = battle.enemies[0].enemy_id;
    let previous_y_offset = battle.enemies[0].y_offset;

    assert_eq!(
        battle.transform_enemy(0, 3, &data, &objects, &magics),
        Some(true)
    );
    let enemy = &battle.enemies[0];
    assert_eq!(enemy.object_id, 3);
    assert_eq!(enemy.enemy_id, 1);
    assert_eq!(enemy.hp, 37);
    assert_eq!(enemy.max_hp, 240);
    assert_eq!(enemy.attack_strength, 99);
    assert_eq!(enemy.magic_strength, 91);
    assert_eq!(enemy.defense, 83);
    assert_eq!(enemy.dexterity, 75);
    assert_eq!(enemy.y_offset, 7);
    assert_eq!(enemy.turn_start_script, 41);
    assert_eq!(enemy.battle_end_script, 42);
    assert_eq!(enemy.ready_script, 43);
    assert!(enemy.statuses.is_active(BattleStatus::Protect));
    assert_eq!(enemy.poisons[0].object_id, 6);
    assert_eq!(
        battle.advance_resolution(),
        vec![BattleEvent::EnemyTransform {
            enemy: 0,
            previous_enemy_id,
            previous_y_offset,
        }]
    );

    battle.enemies[0]
        .statuses
        .set_for_enemy(BattleStatus::Confused, 1);
    assert_eq!(
        battle.transform_enemy(0, 1, &data, &objects, &magics),
        Some(false)
    );
    assert_eq!(battle.enemies[0].object_id, 3);
}

#[test]
fn collect_steal_and_hiding_follow_battle_instance_state() {
    let (data, objects, magics, mut role) = fixture(1_000, 100, 500);
    role.attack_strength = 10;
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut battle);
    assert_eq!(battle.collect_enemy(0), Some(5));
    assert_eq!(battle.steal_enemy(0, 0), Some(BattleSteal::Item(8)));
    assert_eq!(battle.enemies[0].steal_item_count, 1);

    battle.enemies[0].steal_item = 0;
    battle.enemies[0].steal_item_count = 6;
    assert!(matches!(
        battle.steal_enemy(0, 0),
        Some(BattleSteal::Cash(2 | 3))
    ));

    assert!(battle.hide_players(1));
    assert!(battle.attack(0).unwrap().is_empty());
    let events = resolve_until_input_or_finish(&mut battle);
    assert!(events
        .iter()
        .all(|event| !matches!(event, BattleEvent::EnemyAttack { .. })));
    assert_eq!(battle.hiding_time(), 0);
}

#[test]
fn attacks_kill_enemies_and_finish_with_victory() {
    let (data, objects, magics, mut role) = fixture(1, 0, 100);
    role.dexterity = 100;
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut battle);
    assert!(battle.attack(0).unwrap().is_empty());
    let first = resolve_until_input_or_finish(&mut battle);
    assert!(matches!(
        first
            .iter()
            .find(|event| matches!(event, BattleEvent::PlayerAttack { .. })),
        Some(BattleEvent::PlayerAttack { defeated: true, .. })
    ));
    assert_eq!(battle.phase(), BattlePhase::AwaitingCommand);
    assert!(battle.attack(1).unwrap().is_empty());
    let finished = resolve_until_input_or_finish(&mut battle);
    assert_eq!(
        finished.last(),
        Some(&BattleEvent::Finished(BattleResult::Won))
    );
    assert_eq!(battle.phase(), BattlePhase::Finished(BattleResult::Won));
    assert_eq!(
        battle.settled_rewards(),
        Some(BattleRewards {
            experience: 52,
            cash: 96,
        })
    );
    assert!(battle.attack(1).is_none());
}

#[test]
fn enemy_round_can_defeat_the_party() {
    let (data, objects, magics, mut role) = fixture(500, 500, 1);
    role.attack_strength = 1;
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut battle);
    let mut events = battle.attack(0).unwrap();
    events.extend(resolve_until_input_or_finish(&mut battle));
    assert!(events
        .iter()
        .any(|event| matches!(event, BattleEvent::EnemyAttack { defeated: true, .. })));
    assert_eq!(
        events.last(),
        Some(&BattleEvent::Finished(BattleResult::Lost))
    );
    assert_eq!(battle.phase(), BattlePhase::Finished(BattleResult::Lost));
    assert_eq!(battle.settled_rewards(), Some(BattleRewards::default()));
}

#[test]
fn only_non_boss_battles_can_flee() {
    let (data, objects, magics, role) = fixture(100, 10, 100);
    let mut boss =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut boss);
    assert!(boss.flee().is_none());
    let mut normal =
        BattleState::new(request(false), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut normal);
    assert_eq!(
        normal.flee(),
        Some(BattleEvent::Finished(BattleResult::Fled))
    );
    assert_eq!(normal.settled_rewards(), Some(BattleRewards::default()));
}

#[test]
fn classic_flee_attempt_uses_the_action_queue_and_player_flee_rate() {
    let (data, objects, magics, mut role) = fixture(500, 0, 500);
    role.dexterity = 100;
    role.flee_rate = u16::MAX;
    let mut successful =
        BattleState::new(request(false), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut successful);
    assert!(successful.attempt_flee().unwrap().is_empty());
    assert!(matches!(
        resolve_until_input_or_finish(&mut successful).as_slice(),
        [BattleEvent::PlayerFlee {
            player: 0,
            succeeded: true,
        }]
    ));
    assert_eq!(
        successful.phase(),
        BattlePhase::Finished(BattleResult::Fled)
    );

    role.flee_rate = 0;
    let mut failed =
        BattleState::new(request(false), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut failed);
    failed.random_state = 1;
    assert!(failed.attempt_flee().unwrap().is_empty());
    let events = resolve_until_input_or_finish(&mut failed);
    assert!(events.iter().any(|event| matches!(
        event,
        BattleEvent::PlayerFlee {
            player: 0,
            succeeded: false,
        }
    )));
    assert_eq!(failed.phase(), BattlePhase::AwaitingCommand);

    let mut boss =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut boss);
    assert!(boss.attempt_flee().is_none());
}

#[test]
fn scripted_termination_continues_without_victory_rewards() {
    let (data, objects, magics, role) = fixture(100, 10, 100);
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut battle);
    assert!(!battle.enemy_not_first_kind(0).unwrap());
    assert!(battle.enemy_not_first_kind(1).unwrap());
    assert!(battle.set_enemy_magic(0, 2, 0, &objects, &magics));
    assert_eq!(battle.enemies[0].magic_object, 2);
    assert_eq!(battle.enemies[0].magic_rate, 10);

    assert!(battle.set_script_result(0));
    assert_eq!(
        battle.advance_resolution(),
        vec![BattleEvent::Finished(BattleResult::Terminated)]
    );
    assert_eq!(battle.settled_rewards(), Some(BattleRewards::default()));
}

#[test]
fn defeated_players_are_not_revived_when_battle_is_created() {
    let (data, objects, magics, mut role) = fixture(100, 10, 100);
    role.hp = 0;
    let battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();

    assert_eq!(battle.players[0].hp, 0);
    assert_eq!(battle.active_player(), None);
    assert_eq!(battle.phase(), BattlePhase::Finished(BattleResult::Lost));
}

#[test]
fn offensive_magic_consumes_mp_and_uses_magic_damage() {
    let (data, objects, magics, mut role) = fixture(40, 0, 100);
    role.mp = 10;
    role.max_mp = 10;
    role.magic_strength = 80;
    role.dexterity = 100;
    role.magic[0] = 2;
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut battle);
    assert_eq!(battle.players[0].magics.len(), 1);
    assert!(battle.cast_magic(0, 0).unwrap().is_empty());
    let events = resolve_until_input_or_finish(&mut battle);
    assert_eq!(battle.players[0].mp, 5);
    assert!(matches!(
        events.iter().find(|event| matches!(
            event,
            BattleEvent::PlayerMagic {
                phase: MagicEventPhase::Feedback,
                ..
            }
        )),
        Some(BattleEvent::PlayerMagic {
            magic_object: 2,
            defeated: true,
            ..
        })
    ));
}

#[test]
fn automatic_battle_magic_keeps_mp() {
    let (data, objects, magics, mut role) = fixture(1_000, 0, 100);
    role.mp = 10;
    role.max_mp = 10;
    role.magic_strength = 80;
    role.magic[0] = 2;
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut battle);
    battle.set_auto_battle(true);
    assert!(battle.cast_magic(0, 0).is_some());
    let _ = resolve_until_input_or_finish(&mut battle);
    assert_eq!(battle.players[0].mp, 10);
}

#[test]
fn player_magic_runs_use_and_success_scripts_with_mp_scaled_damage() {
    let (data, objects, magics, mut role) = fixture(1000, 0, 500);
    role.mp = 10;
    role.max_mp = 10;
    role.magic_strength = 80;
    role.magic[0] = 2;
    role.dexterity = 100;
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut battle);
    assert!(battle.cast_magic(0, 0).unwrap().is_empty());

    assert!(battle.advance_resolution().is_empty());
    let use_script = battle.take_script_request().unwrap();
    assert_eq!(
        use_script,
        BattleScriptRequest {
            source: BattleScriptSource::PlayerMagicUse {
                player: 0,
                magic_object: 2,
            },
            entry: 31,
            object_id: 0,
        }
    );
    assert_eq!(battle.players[0].mp, 5);
    assert_eq!(battle.scale_active_magic_by_mp(0, 2, 8), Some(40));
    assert!(battle.set_magic_blow(-3));
    assert_eq!(battle.players[0].mp, 0);
    assert!(battle.complete_script(41));

    assert!(matches!(
        battle.advance_resolution().as_slice(),
        [BattleEvent::PlayerMagic {
            phase: MagicEventPhase::Visual,
            damage: 0,
            ..
        }]
    ));
    assert!(battle.advance_resolution().is_empty());
    let success_script = battle.take_script_request().unwrap();
    assert_eq!(
        success_script,
        BattleScriptRequest {
            source: BattleScriptSource::PlayerMagicSuccess {
                player: 0,
                magic_object: 2,
            },
            entry: 32,
            object_id: 0,
        }
    );
    assert!(battle.complete_script(42));
    assert!(matches!(
        battle.advance_resolution().as_slice(),
        [BattleEvent::PlayerMagic {
            player: 0,
            enemy: 0,
            magic_object: 2,
            blow: -3,
            damage,
            defeated: _,
            ..
        }] if *damage >= 40
    ));
    assert_eq!(battle.players[0].magics[0].use_script, 41);
    assert_eq!(battle.players[0].magics[0].success_script, 42);
    assert_eq!(battle.players[0].magics[0].base_damage, 40);
}

#[test]
fn simulated_magic_retargets_and_queues_damage_feedback() {
    let (data, objects, magics, role) = fixture(500, 0, 500);
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut battle);
    battle.enemies[0].hp = 0;
    let target_hp = battle.enemies[1].hp;

    assert!(battle.set_magic_blow(-2));
    assert!(battle.simulate_player_magic(0, 2, 80, &objects, &magics));
    assert!(battle.enemies[1].hp < target_hp);
    assert!(matches!(
        battle.advance_resolution().as_slice(),
        [BattleEvent::SimulatedMagic {
            enemy: 1,
            magic,
            blow: -2,
            damage,
            defeated: _,
            ..
        }] if magic.object_id == 2 && *damage > 0
    ));
}

#[test]
fn consuming_and_thrown_items_reserve_inventory_during_command_selection() {
    let (data, objects, magics, mut role) = fixture(500, 0, 500);
    role.dexterity = 100;
    let other = role.clone();
    let mut battle = BattleState::new(
        request(true),
        0,
        7,
        [(0, &role), (1, &other)],
        &data,
        &objects,
        &magics,
    )
    .unwrap();
    complete_pending_scripts(&mut battle);

    assert!(battle.use_item(20, Some(0), 30, true).is_some());
    assert_eq!(battle.reserved_item_count(20), 1);
    assert!(battle.throw_item(21, Some(0), 40).is_some());
    assert_eq!(battle.reserved_item_count(21), 1);

    let mut non_consuming = BattleState::new(
        request(true),
        0,
        7,
        [(0, &role), (1, &other)],
        &data,
        &objects,
        &magics,
    )
    .unwrap();
    complete_pending_scripts(&mut non_consuming);
    assert!(non_consuming.use_item(22, Some(0), 50, false).is_some());
    assert_eq!(non_consuming.reserved_item_count(22), 0);
}

#[test]
fn unavailable_items_fall_back_when_the_queued_action_executes() {
    let (data, objects, magics, role) = fixture(1_000, 0, 500);
    let mut used =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut used);
    for enemy in &mut used.enemies {
        enemy.statuses.set_for_enemy(BattleStatus::Paralyzed, 2);
    }
    assert!(used.use_item(20, Some(0), 30, true).is_some());
    used.set_inventory_amounts(&[]);
    let used_events = resolve_until_input_or_finish(&mut used);
    assert!(!used_events
        .iter()
        .any(|event| matches!(event, BattleEvent::PlayerDefend { player: 0 })));
    assert_eq!(
        used.hidden_experience_counts(0).unwrap()[HIDDEN_EXP_DEFENSE],
        2
    );
    assert!(!used_events
        .iter()
        .any(|event| matches!(event, BattleEvent::PlayerUseItem { .. })));

    let mut thrown =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut thrown);
    for enemy in &mut thrown.enemies {
        enemy.statuses.set_for_enemy(BattleStatus::Paralyzed, 2);
    }
    assert!(thrown.throw_item(20, Some(0), 30).is_some());
    thrown.set_inventory_amounts(&[]);
    let thrown_events = resolve_until_input_or_finish(&mut thrown);
    assert!(thrown_events
        .iter()
        .any(|event| matches!(event, BattleEvent::PlayerAttack { player: 0, .. })));
    assert!(!thrown_events
        .iter()
        .any(|event| matches!(event, BattleEvent::PlayerThrowItem { .. })));

    let mut replenished = BattleState::new(
        request(true),
        0,
        7,
        [(0, &role), (1, &role)],
        &data,
        &objects,
        &magics,
    )
    .unwrap();
    complete_pending_scripts(&mut replenished);
    for enemy in &mut replenished.enemies {
        enemy.statuses.set_for_enemy(BattleStatus::Paralyzed, 2);
    }
    assert!(replenished.defend().is_some());
    assert!(replenished.use_item(20, Some(0), 0, true).is_some());
    replenished.set_inventory_amounts(&[]);
    assert!(replenished.advance_resolution().is_empty());
    replenished.set_inventory_amounts(&[(20, 1)]);
    let events = resolve_until_input_or_finish(&mut replenished);
    assert!(!events.iter().any(|event| matches!(
        event,
        BattleEvent::PlayerUseItem {
            player: 1,
            item_object: 20,
            ..
        }
    )));
    assert_eq!(
        replenished.hidden_experience_counts(1).unwrap()[HIDDEN_EXP_DEFENSE],
        2
    );
}

#[test]
fn disabled_players_keep_a_zero_dexterity_recovery_action() {
    let (data, objects, magics, role) = fixture(1_000, 0, 500);
    let mut disabled = role.clone();
    disabled.hp = 0;
    let mut battle = BattleState::new(
        request(true),
        0,
        7,
        [(0, &role), (1, &disabled)],
        &data,
        &objects,
        &magics,
    )
    .unwrap();
    complete_pending_scripts(&mut battle);
    for enemy in &mut battle.enemies {
        enemy.statuses.set_for_enemy(BattleStatus::Paralyzed, 2);
    }

    assert_eq!(battle.active_player(), Some(0));
    assert!(battle.attack(0).is_some());
    assert_eq!(battle.active_player(), None);
    assert!(battle.action_queue.iter().any(|queued| {
        matches!(
            queued,
            QueuedBattleAction {
                action: BattleActorAction::Player {
                    player: 1,
                    action: PlayerAction::Attack { .. }
                },
                dexterity: 0,
            }
        )
    }));

    battle.players[1].hp = 500;
    let events = resolve_until_input_or_finish(&mut battle);
    assert!(events
        .iter()
        .any(|event| matches!(event, BattleEvent::PlayerAttack { player: 1, .. })));

    let mut puppet = BattleState::new(
        request(true),
        0,
        7,
        [(0, &role), (1, &disabled)],
        &data,
        &objects,
        &magics,
    )
    .unwrap();
    complete_pending_scripts(&mut puppet);
    assert!(puppet.players[1]
        .statuses
        .set_for_player(BattleStatus::Puppet, 2, false,));
    for enemy in &mut puppet.enemies {
        enemy.statuses.set_for_enemy(BattleStatus::Paralyzed, 2);
    }
    assert!(puppet.attack(0).is_some());
    assert_eq!(puppet.active_player(), None);
    let events = resolve_until_input_or_finish(&mut puppet);
    assert!(events
        .iter()
        .any(|event| matches!(event, BattleEvent::PlayerAttack { player: 1, .. })));
}

#[test]
fn post_action_checks_queue_friend_death_and_dying_scripts() {
    let (data, mut objects, magics, mut victim) = fixture(1_000, 0, 100);
    objects.get_mut(0).unwrap().data[2] = 70;
    objects.get_mut(0).unwrap().data[3] = 80;
    victim.covered_by = 1;
    victim.dying_sound = 77;
    let cover = victim.clone();

    let mut death = BattleState::new(
        request(true),
        0,
        7,
        [(0, &victim), (1, &cover)],
        &data,
        &objects,
        &magics,
    )
    .unwrap();
    complete_pending_scripts(&mut death);
    death.backup_player_hp();
    death.players[0].hp = 0;
    death.queue_post_action_check(true);
    assert_eq!(
        death.advance_resolution(),
        vec![BattleEvent::PlayerFriendDeath { player: 1 }]
    );
    let script_request = death.take_script_request().unwrap();
    assert_eq!(
        script_request.source,
        BattleScriptSource::PlayerFriendDeath {
            player: 1,
            name_object: 0,
        }
    );
    assert_eq!(script_request.object_id, 1);
    assert!(death.complete_script(71));
    assert_eq!(death.players[1].friend_death_script, 71);

    let mut dying = BattleState::new(
        request(true),
        0,
        7,
        [(0, &victim), (1, &cover)],
        &data,
        &objects,
        &magics,
    )
    .unwrap();
    complete_pending_scripts(&mut dying);
    dying.backup_player_hp();
    dying.players[0].hp = 10;
    dying.queue_post_action_check(true);
    assert_eq!(
        dying.advance_resolution(),
        vec![BattleEvent::PlayerDying { player: 0 }]
    );
    let script_request = dying.take_script_request().unwrap();
    assert_eq!(
        script_request.source,
        BattleScriptSource::PlayerDying {
            player: 0,
            name_object: 0,
        }
    );
    assert_eq!(script_request.object_id, 0);
    assert!(dying.complete_script(81));
    assert_eq!(dying.players[0].dying_script, 81);
    assert_eq!(dying.players[0].dying_sound, 77);
}

#[test]
fn scripted_victory_rewards_only_enemies_already_defeated() {
    let (data, objects, magics, role) = fixture(100, 0, 500);
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut battle);
    battle.enemies[0].hp = 0;
    battle.queue_post_action_check(false);
    assert_eq!(
        battle.rewards(),
        BattleRewards {
            experience: 26,
            cash: 48,
        }
    );

    assert!(battle.set_script_result(3));
    assert_eq!(
        battle.advance_resolution(),
        vec![BattleEvent::Finished(BattleResult::Won)]
    );
    assert!(battle.mark_victory_rewards_applied());
    assert!(battle.begin_battle_end_scripts());
    complete_pending_scripts(&mut battle);
    assert_eq!(
        battle.advance_resolution(),
        vec![BattleEvent::Finished(BattleResult::Won)]
    );
    assert_eq!(
        battle.settled_rewards(),
        Some(BattleRewards {
            experience: 26,
            cash: 48,
        })
    );
}

#[test]
fn battle_item_scripts_use_original_owners_and_finish_even_when_failed() {
    let (data, objects, magics, mut role) = fixture(500, 0, 500);
    role.dexterity = 100;
    let mut battle =
        BattleState::new(request(true), 0, 7, [(4, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut battle);
    assert!(battle.use_item(20, Some(0), 30, true).is_some());

    assert!(matches!(
        battle.advance_resolution().as_slice(),
        [BattleEvent::PlayerUseItem {
            item_object: 20,
            ..
        }]
    ));
    assert!(battle.advance_resolution().is_empty());
    assert_eq!(
        battle.take_script_request(),
        Some(BattleScriptRequest {
            source: BattleScriptSource::PlayerItemUse {
                player: 0,
                item_object: 20,
            },
            entry: 30,
            object_id: 4,
        })
    );
    assert!(battle.complete_script_with_result(31, false));
    assert!(matches!(
        battle.advance_resolution().as_slice(),
        [BattleEvent::PlayerItemFeedback {
            player: 0,
            item_object: 20,
            consume: true,
            ..
        }]
    ));
}

#[test]
fn all_target_and_empty_item_scripts_keep_original_completion_semantics() {
    let (data, objects, magics, mut role) = fixture(500, 0, 500);
    role.dexterity = 100;
    let mut use_all =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut use_all);
    assert!(use_all.use_item(20, None, 30, true).is_some());
    assert!(matches!(
        use_all.advance_resolution().as_slice(),
        [BattleEvent::PlayerUseItem {
            item_object: 20,
            ..
        }]
    ));
    assert!(use_all.advance_resolution().is_empty());
    assert_eq!(use_all.take_script_request().unwrap().object_id, u16::MAX);

    let mut throw_all =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut throw_all);
    assert!(throw_all.throw_item(21, None, 40).is_some());
    assert!(matches!(
        throw_all.advance_resolution().as_slice(),
        [BattleEvent::PlayerThrowItem {
            item_object: 21,
            ..
        }]
    ));
    assert!(throw_all.advance_resolution().is_empty());
    assert_eq!(throw_all.take_script_request().unwrap().object_id, u16::MAX);

    let mut empty_script =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut empty_script);
    assert!(empty_script.use_item(22, Some(0), 0, false).is_some());
    assert!(matches!(
        empty_script.advance_resolution().as_slice(),
        [BattleEvent::PlayerUseItem {
            item_object: 22,
            consuming: false,
            ..
        }]
    ));
    assert!(matches!(
        empty_script.advance_resolution().as_slice(),
        [BattleEvent::PlayerItemFeedback {
            item_object: 22,
            consume: false,
            ..
        }]
    ));
}

#[test]
fn thrown_weapon_script_uses_enemy_owner_and_acting_player_attack() {
    let (data, objects, magics, mut role) = fixture(500, 0, 500);
    role.dexterity = 100;
    role.attack_strength = 80;
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut battle);
    assert!(battle.throw_item(21, Some(0), 40).is_some());

    assert!(matches!(
        battle.advance_resolution().as_slice(),
        [BattleEvent::PlayerThrowItem {
            item_object: 21,
            ..
        }]
    ));
    assert!(battle.advance_resolution().is_empty());
    assert_eq!(
        battle.take_script_request(),
        Some(BattleScriptRequest {
            source: BattleScriptSource::PlayerItemThrow {
                player: 0,
                item_object: 21,
            },
            entry: 40,
            object_id: 0,
        })
    );
    battle.random_state = 1;
    assert!(battle.throw_weapon(0, 2, 2, &objects, &magics));
    assert!(battle.complete_script(41));
    let simulated = battle.advance_resolution();
    assert!(simulated.iter().any(|event| matches!(
        event,
        BattleEvent::SimulatedMagic {
            enemy: 0,
            magic,
            damage,
            ..
        } if magic.object_id == 2 && *damage > 0
    )));
    let feedback = battle.advance_resolution();
    assert!(feedback.iter().any(|event| matches!(
        event,
        BattleEvent::PlayerItemFeedback {
            player: 0,
            item_object: 21,
            consume: true,
            ..
        }
    )));
}

#[test]
fn item_feedback_reports_player_hp_and_mp_changes_from_the_script() {
    let (data, objects, magics, mut role) = fixture(500, 0, 500);
    role.hp = 300;
    role.max_mp = 200;
    role.mp = 20;
    role.dexterity = 100;
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut battle);
    assert!(battle.use_item(20, Some(0), 30, true).is_some());
    assert!(matches!(
        battle.advance_resolution().as_slice(),
        [BattleEvent::PlayerUseItem { .. }]
    ));
    assert!(battle.advance_resolution().is_empty());
    assert!(battle.take_script_request().is_some());
    battle.players[0].hp = 425;
    battle.players[0].mp = 70;
    assert!(battle.complete_script(31));

    let feedback = battle.advance_resolution();
    let [BattleEvent::PlayerItemFeedback { player_changes, .. }] = feedback.as_slice() else {
        panic!("expected item feedback");
    };
    assert_eq!(
        player_changes[0],
        BattlePlayerStatChange { hp: 125, mp: 50 }
    );
}

#[test]
fn item_is_not_completed_when_player_is_defeated_before_acting() {
    let (data, objects, magics, mut role) = fixture(500, 500, 1);
    role.dexterity = 0;
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut battle);
    assert!(battle.use_item(20, Some(0), 30, true).is_some());

    let events = resolve_until_input_or_finish(&mut battle);
    assert!(events
        .iter()
        .any(|event| matches!(event, BattleEvent::EnemyAttack { defeated: true, .. })));
    assert!(!events.iter().any(|event| matches!(
        event,
        BattleEvent::PlayerUseItem { .. } | BattleEvent::PlayerThrowItem { .. }
    )));
}

#[test]
fn enemy_magic_runs_use_and_success_scripts_before_damage() {
    let (data, objects, magics, role) = fixture_with_enemy_magic(1000, 0, 200, 2, 10, 0);
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut battle);
    battle.enemies[1]
        .statuses
        .set_for_enemy(BattleStatus::Paralyzed, 2);
    assert!(battle.attack(0).is_some());

    assert!(battle.advance_resolution().is_empty());
    let ready = battle.take_script_request().unwrap();
    assert_eq!(ready.source, BattleScriptSource::EnemyReady { enemy: 0 });
    assert!(!battle.is_enemy_turn());
    assert!(battle.complete_script(ready.entry));
    assert!(battle.advance_resolution().is_empty());
    assert!(battle.is_enemy_turn());
    let use_script = battle.take_script_request().unwrap();
    assert_eq!(
        use_script,
        BattleScriptRequest {
            source: BattleScriptSource::EnemyMagicUse {
                enemy: 0,
                magic_object: 2,
            },
            entry: 31,
            object_id: 0,
        }
    );
    assert!(battle.complete_script_with_result(41, true));
    assert!(matches!(
        battle.advance_resolution().as_slice(),
        [BattleEvent::EnemyMagic {
            phase: MagicEventPhase::Visual,
            damage: 0,
            ..
        }]
    ));
    assert!(battle.advance_resolution().is_empty());
    let success_script = battle.take_script_request().unwrap();
    assert_eq!(
        success_script,
        BattleScriptRequest {
            source: BattleScriptSource::EnemyMagicSuccess {
                enemy: 0,
                magic_object: 2,
            },
            entry: 32,
            object_id: 0,
        }
    );
    assert!(battle.complete_script(42));
    assert!(battle.is_enemy_turn());
    assert!(matches!(
        battle.advance_resolution().as_slice(),
        [BattleEvent::EnemyMagic {
            enemy: 0,
            player: 0,
            magic_object: 2,
            damage,
            ..
        }] if *damage > 0
    ));
    assert!(!battle.is_enemy_turn());
    assert!(battle.players[0].hp < 200);
    assert_eq!(battle.enemies[0].magic.unwrap().use_script, 41);
    assert_eq!(battle.enemies[0].magic.unwrap().success_script, 42);
}

#[test]
fn failed_enemy_magic_use_skips_success_but_keeps_base_damage() {
    let (data, objects, magics, role) = fixture_with_enemy_magic(1000, 0, 200, 2, 10, 0);
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut battle);
    battle.enemies[1]
        .statuses
        .set_for_enemy(BattleStatus::Paralyzed, 2);
    assert!(battle.attack(0).is_some());
    assert!(battle.advance_resolution().is_empty());
    let ready = battle.take_script_request().unwrap();
    assert!(battle.complete_script(ready.entry));
    assert!(battle.advance_resolution().is_empty());
    let use_script = battle.take_script_request().unwrap();
    assert_eq!(
        use_script.source,
        BattleScriptSource::EnemyMagicUse {
            enemy: 0,
            magic_object: 2,
        }
    );
    assert!(battle.complete_script_with_result(41, false));

    assert!(matches!(
        battle.advance_resolution().as_slice(),
        [BattleEvent::EnemyMagic { enemy: 0, damage, .. }] if *damage > 0
    ));
    assert!(!battle.has_script_work());
    assert!(battle.players[0].hp < 200);
}

#[test]
fn enemy_magic_targets_all_living_players_and_protect_reduces_damage() {
    let (data, objects, magics, role) = fixture_with_enemy_magic(1000, 0, 500, 2, 10, 1);
    let other = role.clone();
    let mut battle = BattleState::new(
        request(true),
        0,
        7,
        [(0, &role), (1, &other)],
        &data,
        &objects,
        &magics,
    )
    .unwrap();
    let magic = battle.enemies[0].magic.unwrap();
    assert!(battle.set_magic_blow(2));
    let events = battle.perform_enemy_magic(0, 0, magic);
    assert!(matches!(
        events.as_slice(),
        [
            BattleEvent::EnemyMagic {
                player: 0,
                blow: 2,
                visual: true,
                ..
            },
            BattleEvent::EnemyMagic {
                player: 1,
                blow: 0,
                visual: false,
                ..
            }
        ]
    ));

    battle.players[0].hp = 500;
    battle.random_state = 1234;
    let normal = battle.enemy_magic_damage(0, 0, magic, false);
    battle.random_state = 1234;
    let auto_defended = battle.enemy_magic_damage(0, 0, magic, true);
    assert_eq!(auto_defended, (normal / 2).max(1));
    battle.players[0]
        .statuses
        .set_for_player(BattleStatus::Protect, 1, true);
    battle.random_state = 1234;
    let protected = battle.enemy_magic_damage(0, 0, magic, false);
    assert_eq!(protected, (normal / 2).max(1));
}

#[test]
fn all_target_magic_plays_the_full_visual_only_for_the_first_target() {
    let (data, objects, magics, role) = fixture_with_enemy_magic(1000, 0, 500, 0, 0, 1);
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut battle);
    let magic = battle_magic(2, &objects, &magics).unwrap();

    let player_events = battle.perform_player_magic(0, BattleTarget::AllEnemies, magic);
    assert!(matches!(
        player_events.as_slice(),
        [
            BattleEvent::PlayerMagic { visual: true, .. },
            BattleEvent::PlayerMagic { visual: false, .. }
        ]
    ));

    battle.enemies.iter_mut().for_each(|enemy| enemy.hp = 1000);
    assert!(battle.simulate_player_magic(0, 2, 20, &objects, &magics));
    assert!(matches!(
        battle.pending_events.make_contiguous(),
        [
            BattleEvent::SimulatedMagic { visual: true, .. },
            BattleEvent::SimulatedMagic { visual: false, .. }
        ]
    ));
}

#[test]
fn summon_magic_resolves_the_referenced_offensive_visual() {
    let mut object_words = vec![0u16; 4 * 6];
    object_words[2 * 6] = 0;
    object_words[2 * 6 + 5] = MAGIC_FLAG_USABLE_IN_BATTLE | MAGIC_FLAG_USABLE_TO_ENEMY;
    object_words[3 * 6] = 1;
    object_words[3 * 6 + 5] =
        MAGIC_FLAG_USABLE_IN_BATTLE | MAGIC_FLAG_USABLE_TO_ENEMY | MAGIC_FLAG_APPLY_TO_ALL;
    let objects = GlobalObjects::parse(&words(&object_words), ObjectLayout::Dos).unwrap();

    let mut magic_data = vec![0u8; 64];
    magic_data[0..2].copy_from_slice(&1u16.to_le_bytes());
    magic_data[2..4].copy_from_slice(&9u16.to_le_bytes());
    magic_data[8..10].copy_from_slice(&4i16.to_le_bytes());
    magic_data[32..34].copy_from_slice(&7u16.to_le_bytes());
    magic_data[34..36].copy_from_slice(&2u16.to_le_bytes());
    magic_data[38..40].copy_from_slice(&(-3i16).to_le_bytes());
    magic_data[46..48].copy_from_slice(&2u16.to_le_bytes());
    magic_data[48..50].copy_from_slice(&3u16.to_le_bytes());
    let magics = Magics::parse(&magic_data).unwrap();

    let summon = battle_magic(2, &objects, &magics).unwrap();
    let effect = summon.summon_effect.unwrap();
    assert_eq!(summon.magic_type, 9);
    assert_eq!(summon.specific, 4);
    assert_eq!(effect.object_id, 3);
    assert_eq!(effect.effect, 7);
    assert_eq!(effect.magic_type, 2);
    assert_eq!(effect.y_offset, -3);
    assert_eq!(effect.fire_delay, 2);
    assert_eq!(effect.effect_times, 3);
    assert!(effect.usable_to_enemy());

    magic_data[0..2].copy_from_slice(&9u16.to_le_bytes());
    let invalid_magics = Magics::parse(&magic_data).unwrap();
    assert!(battle_magic(2, &objects, &invalid_magics).is_none());
}

#[test]
fn silence_forces_an_enemy_with_magic_to_attack_normally() {
    let (data, objects, magics, role) = fixture_with_enemy_magic(1000, 20, 500, 2, 10, 0);
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut battle);
    battle.enemies[0]
        .statuses
        .set_for_enemy(BattleStatus::Silence, 1);
    battle.enemies[1]
        .statuses
        .set_for_enemy(BattleStatus::Paralyzed, 1);
    assert!(battle.attack(0).is_some());
    let events = resolve_until_input_or_finish(&mut battle);
    assert!(events
        .iter()
        .any(|event| matches!(event, BattleEvent::EnemyAttack { enemy: 0, .. })));
    assert!(!events
        .iter()
        .any(|event| matches!(event, BattleEvent::EnemyMagic { enemy: 0, .. })));
}

#[test]
fn confused_enemy_attacks_a_random_living_enemy_instead_of_the_party() {
    let (data, objects, magics, role) = fixture(1000, 200, 500);
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut battle);
    battle.enemies[0]
        .statuses
        .set_for_enemy(BattleStatus::Confused, 1);
    let seed = (1..100)
        .find(|&seed| {
            let mut candidate = battle.clone();
            candidate.random_state = seed;
            candidate.perform_confused_enemy_action(0).is_some()
        })
        .unwrap();
    battle.random_state = seed;
    battle.flow = BattleFlow::EnemyAction {
        enemy: 0,
        ready_complete: true,
    };
    let player_hp = battle.players[0].hp;
    let target_hp = battle.enemies[1].hp;

    assert!(matches!(
        battle.advance_resolution().as_slice(),
        [BattleEvent::EnemyConfusedAttack {
            enemy: 0,
            target: 1,
            damage,
            ..
        }] if *damage > 0
    ));
    assert_eq!(battle.players[0].hp, player_hp);
    assert!(battle.enemies[1].hp < target_hp);
}

#[test]
fn confused_player_skips_command_selection_and_attacks_a_living_teammate() {
    let (data, objects, magics, mut role) = fixture(1000, 0, 500);
    role.dexterity = 100;
    let mut teammate = role.clone();
    teammate.dexterity = 1;
    teammate.attack_strength = 1;
    let mut battle = BattleState::new(
        request(true),
        0,
        7,
        [(0, &role), (1, &teammate)],
        &data,
        &objects,
        &magics,
    )
    .unwrap();
    complete_pending_scripts(&mut battle);
    battle.enemies[0].dual_move = false;
    battle.enemies[0]
        .statuses
        .set_for_enemy(BattleStatus::Paralyzed, 1);
    battle.enemies[1].hp = 0;
    battle.players[0]
        .statuses
        .set_for_player(BattleStatus::Confused, 2, true);
    battle.refresh_player_effects();
    assert_eq!(battle.active_player(), Some(1));
    let teammate_hp = battle.players[1].hp;

    assert!(battle.attack(0).unwrap().is_empty());
    let events = resolve_until_input_or_finish(&mut battle);
    assert!(matches!(
        events.iter().find(|event| matches!(event, BattleEvent::PlayerConfusedAttack { .. })),
        Some(BattleEvent::PlayerConfusedAttack {
            player: 0,
            target: 1,
            damage,
            ..
        }) if *damage > 0
    ));
    assert!(battle.players[1].hp < teammate_hp);
}

#[test]
fn haste_uses_classic_triple_dexterity_in_the_round_action_queue() {
    let (data, objects, magics, mut role) = fixture(1000, 1, 500);
    role.attack_strength = 1;
    role.dexterity = 10;
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut battle);
    battle.enemies[0].dual_move = false;
    battle.enemies[0].ready_script = 0;
    battle.enemies[1].hp = 0;
    let mut normal = battle.clone();
    assert!(normal.attack(0).unwrap().is_empty());
    assert!(matches!(
        normal.advance_resolution().as_slice(),
        [BattleEvent::EnemyAttack { enemy: 0, .. }]
    ));

    battle.players[0]
        .statuses
        .set_for_player(BattleStatus::Haste, 2, true);
    assert!(battle.attack(0).unwrap().is_empty());

    assert!(matches!(
        battle.advance_resolution().as_slice(),
        [BattleEvent::PlayerAttack { player: 0, .. }]
    ));
    assert!(matches!(
        battle.advance_resolution().as_slice(),
        [BattleEvent::EnemyAttack { enemy: 0, .. }]
    ));
}

#[test]
fn enemy_attack_item_runs_during_enemy_turn_and_obeys_poison_resistance() {
    let (data, objects, magics, role) = fixture(1000, 20, 500);
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut battle);
    battle.enemies[0].attack_equivalent_item_rate = 10;
    battle.enemies[1]
        .statuses
        .set_for_enemy(BattleStatus::Paralyzed, 2);
    assert!(battle.attack(0).is_some());
    assert!(battle.advance_resolution().is_empty());
    let ready = battle.take_script_request().unwrap();
    assert_eq!(ready.source, BattleScriptSource::EnemyReady { enemy: 0 });
    assert!(!battle.is_enemy_turn());
    assert!(battle.complete_script(ready.entry));

    let seed = (1..100_000)
        .find(|&seed| {
            let mut candidate = battle.clone();
            candidate.random_state = seed;
            let _ = candidate.advance_resolution();
            candidate.has_script_work()
        })
        .unwrap();
    battle.random_state = seed;

    assert!(matches!(
        battle.advance_resolution().as_slice(),
        [BattleEvent::EnemyAttack { enemy: 0, .. }]
    ));
    assert!(battle.is_enemy_turn());
    let item = battle.take_script_request().unwrap();
    assert_eq!(
        item,
        BattleScriptRequest {
            source: BattleScriptSource::EnemyAttackItem {
                enemy: 0,
                item_object: 9,
            },
            entry: 33,
            object_id: 0,
        }
    );
    assert!(battle.complete_script(34));
    assert_eq!(battle.enemies[0].attack_equivalent_item_script, 34);
    assert!(battle.is_enemy_turn());
    assert!(battle.advance_resolution().is_empty());
    assert!(!battle.is_enemy_turn());

    let mut resisted =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut resisted);
    resisted.enemies[0].attack_equivalent_item_rate = 10;
    resisted.players[0].poison_resistance = 100;
    resisted.enemies[1]
        .statuses
        .set_for_enemy(BattleStatus::Paralyzed, 2);
    assert!(resisted.attack(0).is_some());
    assert!(resisted.advance_resolution().is_empty());
    let ready = resisted.take_script_request().unwrap();
    assert!(resisted.complete_script(ready.entry));
    assert!(matches!(
        resisted.advance_resolution().as_slice(),
        [BattleEvent::EnemyAttack { enemy: 0, .. }]
    ));
    assert!(!resisted.has_script_work());
    assert!(!resisted.is_enemy_turn());
}

#[test]
fn physical_damage_matches_pal_piecewise_boundaries() {
    assert_eq!(physical_damage(100, 50, 2), 60);
    assert_eq!(physical_damage(80, 100, 1), 20);
    assert_eq!(physical_damage(50, 100, 1), 0);
    assert_eq!(physical_damage(100, 50, 0), 120);
}

#[test]
fn negative_enemy_defense_wraps_before_player_attack_damage() {
    let (data, objects, magics, role) = fixture(40, 0, 500);
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut battle);
    battle.players[0].attack_strength = 35;
    battle.enemies[0].level = 0;
    battle.enemies[0].defense = (-6i16) as u16;
    battle.enemies[0].physical_resistance = 2;

    assert_eq!(battle.enemies[0].effective_defense(), 18);
    assert_eq!(battle.enemies[0].simulated_magic_defense(), 18);
    assert!(battle.player_single_attack_damage(0, 0).0 >= 21);
}

#[test]
fn negative_magic_base_damage_does_not_wrap_into_healing_the_enemy() {
    let (data, objects, _, role) = fixture(40, 0, 500);
    let mut magic_data = [0; 32];
    magic_data[26..28].copy_from_slice(&(-999i16).to_le_bytes());
    let magics = Magics::parse(&magic_data).unwrap();
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut battle);
    battle.enemies[0].level = 0;
    battle.enemies[0].defense = (-6i16) as u16;
    let hp_before = battle.enemies[0].hp;

    assert!(battle.simulate_player_magic(0, 2, 0, &objects, &magics));
    assert_eq!(battle.enemies[0].hp, hp_before);
    assert!(matches!(
        battle.advance_resolution().as_slice(),
        [BattleEvent::SimulatedMagic { damage: 0, .. }]
    ));

    assert!(battle.damage_enemy(0, 90, false));
    assert!(!battle.enemies[0].is_alive());
}

#[test]
fn random_float_is_applied_before_classic_integer_truncation() {
    let (data, objects, magics, role) = fixture(1000, 0, 1000);
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut battle);

    battle.random_state = 1;
    let mut expected_state = 1;
    let expected = (1000.0 * crate::random::random_float(&mut expected_state, 0.9, 1.1)) as i32;
    assert_eq!(battle.jitter_dexterity(1000), expected);

    battle.random_state = 1;
    let mut expected_state = 1;
    let expected =
        (1000.0 * crate::random::random_float(&mut expected_state, 10.0, 11.0)) as u32 / 10;
    assert_eq!(battle.randomized_magic_strength(1000), expected);

    battle.random_state = 1;
    let mut expected_state = 1;
    let attack_roll = crate::random::random_long(&mut expected_state, 0, 1);
    let critical = crate::random::random_long(&mut expected_state, 0, 5) == 0;
    let bonus = crate::random::random_long(&mut expected_state, 0, 11) == 0;
    let defender = &battle.enemies[0];
    let defense = u32::from(defender.effective_defense());
    let base = physical_damage(
        u32::from(battle.players[0].attack_strength),
        defense,
        u32::from(defender.physical_resistance),
    )
    .saturating_add(1 + attack_roll)
    .saturating_mul(if critical { 3 } else { 1 })
    .saturating_mul(if bonus { 2 } else { 1 });
    let expected =
        (base as f32 * crate::random::random_float(&mut expected_state, 1.0, 1.125)) as u16;
    assert_eq!(
        battle.player_single_attack_damage(0, 0),
        (expected.max(1), critical || bonus)
    );
}

#[test]
fn player_attack_critical_bonus_and_attack_all_follow_classic_rules() {
    let (data, objects, magics, mut role) = fixture(1000, 0, 500);
    role.attack_all = true;
    role.dexterity = 100;
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut battle);
    battle.players[0]
        .statuses
        .set_for_player(BattleStatus::Bravery, 2, true);
    for enemy in &mut battle.enemies {
        enemy.statuses.set_for_enemy(BattleStatus::Paralyzed, 2);
    }
    assert!(battle.attack(0).is_some());
    let attacks = resolve_until_input_or_finish(&mut battle)
        .into_iter()
        .filter_map(|event| match event {
            BattleEvent::PlayerAttack {
                enemy,
                damage,
                critical,
                ..
            } => Some((enemy, damage, critical)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(attacks.len(), 2);
    assert_eq!([attacks[0].0, attacks[1].0], [1, 0]);
    assert!(attacks.iter().all(|attack| attack.2));
    assert!(attacks[0].1 >= attacks[1].1.saturating_mul(2));
    let counts = battle.hidden_experience_counts(0).unwrap();
    assert_eq!(counts[HIDDEN_EXP_ATTACK], 1);
    assert!((2..=3).contains(&counts[HIDDEN_EXP_HEALTH]));

    let (data, objects, magics, role) = fixture(1000, 0, 500);
    let base =
        BattleState::new(request(true), 0, 7, [(1, &role)], &data, &objects, &magics).unwrap();
    let mut normal = base.clone();
    let mut critical = base.clone();
    let seed = (1..10_000)
        .find(|&seed| {
            let mut state = seed;
            let _ = crate::random::random_long(&mut state, 0, 1);
            crate::random::random_long(&mut state, 0, 5) != 0
        })
        .unwrap();
    normal.random_state = seed;
    critical.random_state = seed;
    critical.players[0]
        .statuses
        .set_for_player(BattleStatus::Bravery, 2, true);
    let normal_damage = normal.player_single_attack_damage(0, 0).0;
    let critical_damage = critical.player_single_attack_damage(0, 0).0;
    assert!(critical_damage >= normal_damage.saturating_mul(3).saturating_sub(2));
}

#[test]
fn enemy_physical_attack_can_be_covered_or_auto_defended() {
    let (data, objects, magics, mut role) = fixture(1000, 80, 500);
    role.hp = 20;
    role.covered_by = 1;
    let mut cover = role.clone();
    cover.hp = 500;
    cover.covered_by = 0;
    let mut battle = BattleState::new(
        request(true),
        0,
        7,
        [(0, &role), (1, &cover)],
        &data,
        &objects,
        &magics,
    )
    .unwrap();
    complete_pending_scripts(&mut battle);

    let mut covered = false;
    let mut defended = false;
    for _ in 0..256 {
        battle.players[0].hp = 20;
        battle.players[1].hp = 500;
        match battle.perform_enemy_action(0, if covered { 1 } else { 0 }) {
            Some(BattleEvent::EnemyAttack {
                player: 0,
                damage: 0,
                protected_by: Some(1),
                auto_defended: true,
                ..
            }) => covered = true,
            Some(BattleEvent::EnemyAttack {
                damage: 0,
                protected_by: None,
                auto_defended: true,
                ..
            }) => defended = true,
            _ => {}
        }
        if covered && defended {
            break;
        }
    }
    assert!(covered);
    assert!(defended);
}

#[test]
fn lethal_player_damage_is_capped_to_remaining_hp_in_the_event() {
    let (data, objects, magics, mut role) = fixture(1_000, 500, 5);
    role.hp = 5;
    let base =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    let (_, event) = (1..100_000)
        .find_map(|seed| {
            let mut battle = base.clone();
            battle.random_state = seed;
            let event = battle.perform_enemy_action(0, 0)?;
            matches!(event, BattleEvent::EnemyAttack { damage, .. } if damage > 0)
                .then_some((battle, event))
        })
        .unwrap();
    assert!(matches!(
        event,
        BattleEvent::EnemyAttack {
            damage: 5,
            defeated: true,
            ..
        }
    ));
}

#[test]
fn forced_action_uses_randomized_offensive_magic_and_skips_ultimate_moves() {
    let (data, objects, magics, mut role) = fixture(1000, 0, 500);
    role.magic[0] = 2;
    role.max_mp = 10;
    role.mp = 10;
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut battle);
    assert!(battle.commit_auto_action(60).is_some());
    assert!(matches!(
        battle.player_actions[0],
        Some(PlayerAction::Magic { magic: 0, .. })
    ));

    let mut ultimate =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut ultimate);
    ultimate.players[0].magics[0].mp_cost = 1;
    assert!(ultimate.commit_auto_action(9999).is_some());
    assert!(matches!(
        ultimate.player_actions[0],
        Some(PlayerAction::Attack { .. })
    ));
}

#[test]
fn repeat_round_fallback_does_not_replace_the_previous_action_cache() {
    let (data, objects, magics, mut role) = fixture(10_000, 0, 500);
    role.magic[0] = 2;
    role.mp = 5;
    role.max_mp = 5;
    role.dexterity = 100;
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut battle);
    for enemy in &mut battle.enemies {
        enemy.statuses.set_for_enemy(BattleStatus::Paralyzed, 8);
    }

    assert!(battle.cast_magic(0, 0).is_some());
    let events = resolve_until_input_or_finish(&mut battle);
    assert!(!matches!(events.last(), Some(BattleEvent::RoundCompleted)));
    assert_eq!(battle.players[0].mp, 0);
    assert!(matches!(
        battle.previous_player_actions[0],
        Some(PlayerAction::Magic { magic: 0, .. })
    ));

    assert!(battle.repeat_last_action().is_some());
    assert!(matches!(
        battle.player_actions[0],
        Some(PlayerAction::Attack { .. })
    ));
    let events = resolve_until_input_or_finish(&mut battle);
    assert!(!matches!(events.last(), Some(BattleEvent::RoundCompleted)));
    assert!(matches!(
        battle.previous_player_actions[0],
        Some(PlayerAction::Magic { magic: 0, .. })
    ));

    battle.players[0].mp = 5;
    assert!(battle.repeat_last_action().is_some());
    assert!(matches!(
        battle.player_actions[0],
        Some(PlayerAction::Magic { magic: 0, .. })
    ));

    let mut item_battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut item_battle);
    item_battle.previous_player_actions[0] = Some(PlayerAction::UseItem {
        item_object: 20,
        target: Some(0),
        script_entry: 30,
        consuming: true,
    });
    assert!(item_battle.repeat_unavailable_item(false).is_some());
    assert!(matches!(
        item_battle.previous_player_actions[0],
        Some(PlayerAction::UseItem {
            item_object: 20,
            ..
        })
    ));
}

#[test]
fn automatic_attack_propagates_during_execution_without_reordering_actions() {
    let (data, objects, magics, mut leader) = fixture(10_000, 0, 500);
    leader.dexterity = 1000;
    let mut follower = leader.clone();
    follower.dexterity = 1;
    let mut battle = BattleState::new(
        request(true),
        0,
        7,
        [(0, &leader), (1, &follower)],
        &data,
        &objects,
        &magics,
    )
    .unwrap();
    complete_pending_scripts(&mut battle);
    for enemy in &mut battle.enemies {
        enemy.statuses.set_for_enemy(BattleStatus::Paralyzed, 2);
    }

    assert!(battle.attack_automatically(0).is_some());
    assert!(battle.defend().is_some());
    assert!(battle.previous_round_used_auto_attack());
    let follower_dexterity = battle
        .action_queue
        .iter()
        .find_map(|queued| match queued.action {
            BattleActorAction::Player {
                player: 1,
                action: PlayerAction::Defend,
            } => Some(queued.dexterity),
            _ => None,
        })
        .expect("the follower should enter the queue with defend dexterity");
    assert!(follower_dexterity < 10);

    assert!(matches!(
        battle.advance_resolution().as_slice(),
        [BattleEvent::PlayerAttack { player: 0, .. }]
    ));
    assert!(matches!(
        battle.advance_resolution().as_slice(),
        [BattleEvent::PlayerAttack { player: 1, .. }]
    ));
    assert!(matches!(
        battle.player_actions[1],
        Some(PlayerAction::Attack { .. })
    ));

    let mut stopped = BattleState::new(
        request(true),
        0,
        7,
        [(0, &leader), (1, &follower)],
        &data,
        &objects,
        &magics,
    )
    .unwrap();
    complete_pending_scripts(&mut stopped);
    assert!(stopped.attack_automatically(0).is_some());
    stopped.set_auto_attack_mode(false);
    assert!(stopped.defend().is_some());
    assert!(!stopped.previous_round_used_auto_attack());

    let mut defeated_follower = follower.clone();
    defeated_follower.hp = 0;
    let mut recovering = BattleState::new(
        request(true),
        0,
        7,
        [(0, &leader), (1, &defeated_follower)],
        &data,
        &objects,
        &magics,
    )
    .unwrap();
    complete_pending_scripts(&mut recovering);
    for enemy in &mut recovering.enemies {
        enemy.statuses.set_for_enemy(BattleStatus::Paralyzed, 2);
    }
    assert!(recovering.attack_automatically(0).is_some());
    assert!(matches!(
        recovering.advance_resolution().as_slice(),
        [BattleEvent::PlayerAttack { player: 0, .. }]
    ));
    recovering.players[1].hp = 500;
    assert!(matches!(
        recovering.advance_resolution().as_slice(),
        [BattleEvent::PlayerAttack { player: 1, .. }]
    ));
    assert!(recovering.automatic_player_attacks[1]);
}

#[test]
fn defensive_magic_uses_player_targets_and_original_script_owners() {
    let (data, mut objects, magics, mut role) = fixture_with_enemy_magic(500, 20, 500, 0, 0, 4);
    role.max_mp = 20;
    role.mp = 20;
    role.magic[0] = 2;
    objects.get_mut(2).unwrap().data[6] = MAGIC_FLAG_USABLE_IN_BATTLE;
    let mut battle = BattleState::new(
        request(true),
        0,
        7,
        [(0, &role), (1, &role)],
        &data,
        &objects,
        &magics,
    )
    .unwrap();
    complete_pending_scripts(&mut battle);

    assert!(battle.cast_magic_at(0, BattleTarget::Player(1)).is_some());
    assert!(battle.attack(0).is_some());
    assert!(battle.advance_resolution().is_empty());
    let use_script = battle.take_script_request().unwrap();
    assert_eq!(use_script.object_id, 0);
    assert!(battle.complete_script(use_script.entry + 1));
    assert_eq!(
        battle.advance_resolution(),
        vec![BattleEvent::PlayerDefensiveMagic {
            player: 0,
            target: BattleTarget::Player(1),
            magic_object: 2,
        }]
    );
    assert!(battle.advance_resolution().is_empty());
    let success_script = battle.take_script_request().unwrap();
    assert_eq!(success_script.object_id, 1);
    assert!(battle.complete_script(success_script.entry + 1));
    assert!(battle.advance_resolution().is_empty());
    assert_eq!(battle.players[0].mp, 15);
}

#[test]
fn defend_reduces_damage_and_expires_after_the_round() {
    let (data, objects, magics, role) = fixture_with_enemy_magic(500, 20, 500, 0, 0, 0);
    let mut plain =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    let mut guarded = plain.clone();
    complete_pending_scripts(&mut plain);
    complete_pending_scripts(&mut guarded);
    let plain_damage = plain.enemy_damage(0, 0);
    guarded.players[0].defending = true;
    let guarded_damage = guarded.enemy_damage(0, 0);
    assert!(guarded_damage < plain_damage);

    guarded.players[0].defending = false;
    assert!(guarded.defend().is_some());
    let events = resolve_until_input_or_finish(&mut guarded);
    assert!(!events
        .iter()
        .any(|event| matches!(event, BattleEvent::PlayerDefend { player: 0 })));
    assert!(!guarded.players[0].defending);
    assert_eq!(
        guarded.hidden_experience_counts(0).unwrap()[HIDDEN_EXP_DEFENSE],
        2
    );
}

#[test]
fn cooperative_magic_ends_selection_but_keeps_already_committed_actions() {
    let (data, objects, magics, mut role) = fixture_with_enemy_magic(500, 0, 500, 0, 0, 0);
    role.cooperative_magic = 2;
    role.magic_strength = 80;
    role.dexterity = 1000;
    let mut other = role.clone();
    other.dexterity = 1;
    let third = other.clone();
    let mut battle = BattleState::new(
        request(true),
        0,
        7,
        [(0, &role), (1, &other), (2, &third)],
        &data,
        &objects,
        &magics,
    )
    .unwrap();
    complete_pending_scripts(&mut battle);
    for enemy in &mut battle.enemies {
        enemy.statuses.set_for_enemy(BattleStatus::Paralyzed, 2);
    }

    assert!(battle.attack(0).is_some());
    assert!(battle.can_use_cooperative_magic());
    assert!(battle
        .cast_cooperative_magic(BattleTarget::Enemy(0))
        .is_some());
    assert!(battle.attack(0).is_none());
    let events = resolve_until_input_or_finish(&mut battle);

    assert!(events.iter().any(|event| matches!(
        event,
        BattleEvent::PlayerCooperativeMagic {
            player: 1,
            enemy: 0,
            magic_object: 2,
            damage,
            ..
        } if *damage > 0
    )));
    assert!(events
        .iter()
        .any(|event| matches!(event, BattleEvent::PlayerAttack { player: 0, .. })));
    assert!(!events
        .iter()
        .any(|event| matches!(event, BattleEvent::PlayerAttack { player: 2, .. })));
    assert_eq!(battle.players[0].hp, 495);
    assert_eq!(battle.players[1].hp, 495);
    assert_eq!(battle.players[2].hp, 495);
}

#[test]
fn command_undo_releases_item_reservations_and_flee_can_cover_the_party() {
    let (data, objects, magics, role) = fixture_with_enemy_magic(500, 20, 500, 0, 0, 0);
    let mut battle = BattleState::new(
        request(false),
        0,
        7,
        [(0, &role), (1, &role)],
        &data,
        &objects,
        &magics,
    )
    .unwrap();
    complete_pending_scripts(&mut battle);
    assert!(battle.use_item(7, Some(0), 0, true).is_some());
    assert_eq!(battle.reserved_item_count(7), 1);
    assert_eq!(battle.undo_last_command(), Some(0));
    assert_eq!(battle.reserved_item_count(7), 0);
    assert_eq!(battle.active_player(), Some(0));

    assert!(battle.attempt_flee_all().is_some());
    assert!(battle
        .player_actions
        .iter()
        .all(|action| { matches!(action, Some(PlayerAction::Flee)) }));
    assert!(matches!(battle.flow, BattleFlow::PerformActions));
}

#[test]
fn status_rules_preserve_original_duration_and_alive_restrictions() {
    let mut statuses = BattleStatuses::default();
    assert!(statuses.set_for_player(BattleStatus::Sleep, 3, true));
    assert!(statuses.set_for_player(BattleStatus::Sleep, 8, true));
    assert_eq!(statuses.duration(BattleStatus::Sleep), 3);

    assert!(statuses.set_for_player(BattleStatus::Protect, 2, true));
    assert!(statuses.set_for_player(BattleStatus::Protect, 5, true));
    assert_eq!(statuses.duration(BattleStatus::Protect), 5);
    assert!(!statuses.set_for_player(BattleStatus::Puppet, 4, true));
    assert!(statuses.set_for_player(BattleStatus::Puppet, 4, false));
    assert_eq!(statuses.duration(BattleStatus::Puppet), 4);

    statuses.set_for_enemy(BattleStatus::Haste, 1000);
    statuses.remove_from_player(BattleStatus::Haste);
    assert_eq!(statuses.duration(BattleStatus::Haste), 1000);
    statuses.decrement_round();
    assert_eq!(statuses.duration(BattleStatus::Haste), 999);
}

#[test]
fn temporary_player_stats_replace_the_extra_effect_and_sprite_restores() {
    let (data, objects, magics, role) = fixture(500, 0, 500);
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    assert_eq!(battle.players[0].attack_strength, 80);
    assert_eq!(battle.players[0].battle_sprite_num, 0);

    assert!(battle.set_temporary_player_stat(0, 17, 40));
    assert_eq!(battle.players[0].attack_strength, 120);
    assert!(battle.set_temporary_player_stat(0, 17, 20));
    assert_eq!(battle.players[0].attack_strength, 100);
    assert!(battle.set_temporary_player_stat(0, 21, 15));
    assert_eq!(battle.players[0].flee_rate, 35);
    assert!(!battle.set_temporary_player_stat(0, 16, 10));
    assert!(!battle.set_temporary_player_stat(9, 17, 10));

    assert!(battle.set_temporary_player_sprite(0, 5));
    assert_eq!(battle.players[0].battle_sprite_num, 5);
    assert!(battle.set_temporary_player_sprite(0, 0));
    assert_eq!(battle.players[0].battle_sprite_num, 0);
}

#[test]
fn poison_slots_reject_duplicates_and_cure_by_object_level() {
    let objects = GlobalObjects::parse(
        &words(&[
            0, 0, 0, 0, 0, 0, // empty object 0
            1, 2, 10, 0, 11, 0, // level-one poison
            4, 3, 20, 0, 21, 0, // level-four poison
        ]),
        ObjectLayout::Dos,
    )
    .unwrap();
    let mut poisons = [BattlePoison::default(); MAX_BATTLE_POISONS];
    assert!(add_poison(&mut poisons, 1, 10));
    assert!(add_poison(&mut poisons, 1, 99));
    assert_eq!(poisons[0].script_entry, 10);
    assert!(add_poison(&mut poisons, 2, 20));
    assert!(cure_poison_by_level(&mut poisons, 1, &objects));
    assert_eq!(poisons[0].object_id, 2);
    assert_eq!(poisons[1], BattlePoison::default());
    assert!(cure_poison(&mut poisons, 2));
    assert!(!cure_poison(&mut poisons, 2));
}

#[test]
fn statuses_gate_magic_double_attacks_and_expire_after_enemy_round() {
    let (data, objects, magics, mut role) = fixture(1000, 20, 200);
    role.mp = 10;
    role.max_mp = 10;
    role.magic_strength = 80;
    role.magic[0] = 2;
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut battle);
    assert!(battle.poison_enemy(0, 40, 77, false));
    assert_eq!(battle.enemies[0].poisons[0].object_id, 40);
    assert_eq!(battle.enemies[0].poisons[0].script_entry, 77);
    let poison_script = battle.take_script_request().unwrap();
    assert_eq!(
        poison_script.source,
        BattleScriptSource::EnemyPoison {
            enemy: 0,
            poison_id: 40,
        }
    );
    assert!(battle.complete_script(78));
    assert_eq!(battle.enemies[0].poisons[0].script_entry, 78);
    assert!(battle.cure_enemy_poison(0, 40, false));
    assert_eq!(battle.enemies[0].poisons[0], BattlePoison::default());
    assert!((0..20).any(|_| { battle.set_enemy_status(0, BattleStatus::Sleep, 2) == Some(true) }));
    battle.enemies[0]
        .statuses
        .remove_from_player(BattleStatus::Sleep);
    battle.players[0]
        .statuses
        .set_for_player(BattleStatus::Silence, 1, true);
    assert!(battle.cast_magic(0, 0).is_none());
    assert_eq!(battle.players[0].mp, 10);
    battle.players[0]
        .statuses
        .remove_from_player(BattleStatus::Silence);
    battle.players[0]
        .statuses
        .set_for_player(BattleStatus::DualAttack, 2, true);
    battle.enemies[0]
        .statuses
        .set_for_enemy(BattleStatus::Paralyzed, 1);
    battle.enemies[1]
        .statuses
        .set_for_enemy(BattleStatus::Paralyzed, 1);

    let mut events = battle.attack(0).unwrap();
    events.extend(resolve_until_input_or_finish(&mut battle));
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, BattleEvent::PlayerAttack { .. }))
            .count(),
        2
    );
    assert!(!events
        .iter()
        .any(|event| matches!(event, BattleEvent::EnemyAttack { .. })));
    assert_eq!(
        battle.players[0]
            .statuses
            .duration(BattleStatus::DualAttack),
        1
    );
    assert_eq!(
        battle.enemies[0].statuses.duration(BattleStatus::Paralyzed),
        0
    );
}

#[test]
fn lifecycle_and_round_poison_scripts_run_in_original_order() {
    let (data, objects, magics, role) = fixture(1000, 0, 1000);
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();

    for (enemy, next_entry) in [(0, 21), (1, 22)] {
        let request = battle.take_script_request().unwrap();
        assert_eq!(
            request,
            BattleScriptRequest {
                source: BattleScriptSource::EnemyTurnStart { enemy },
                entry: 11,
                object_id: enemy as u16,
            }
        );
        assert!(battle.complete_script(next_entry));
    }
    assert_eq!(battle.active_player(), Some(0));

    battle.players[0].poisons[0] = BattlePoison {
        object_id: 40,
        script_entry: 31,
    };
    battle.enemies[0].poisons[0] = BattlePoison {
        object_id: 41,
        script_entry: 32,
    };
    battle.players[0]
        .statuses
        .set_for_player(BattleStatus::Protect, 2, true);
    battle.enemies[0]
        .statuses
        .set_for_enemy(BattleStatus::Protect, 2);
    battle.players[0].defending = true;
    assert!(battle.attack(0).is_some());

    let mut ready_entries = [13, 13];
    let mut ready_counts = [0; 2];
    for _ in 0..4 {
        while !battle.has_script_work() {
            battle.advance_resolution();
        }
        let request = battle.take_script_request().unwrap();
        let BattleScriptSource::EnemyReady { enemy } = request.source else {
            panic!("expected enemy ready script, got {:?}", request.source);
        };
        assert_eq!(request.entry, ready_entries[enemy]);
        ready_entries[enemy] += 1;
        ready_counts[enemy] += 1;
        assert!(battle.complete_script(ready_entries[enemy]));
        assert!(matches!(
            battle.advance_resolution().as_slice(),
            [BattleEvent::EnemyAttack { enemy: actor, .. }] if *actor == enemy
        ));
    }
    assert_eq!(ready_counts, [2, 2]);

    while !battle.has_script_work() {
        battle.advance_resolution();
    }
    let player_poison = battle.take_script_request().unwrap();
    assert!(!battle.players[0].defending);
    assert_eq!(
        player_poison.source,
        BattleScriptSource::PlayerPoison {
            role_id: 0,
            poison_id: 40,
        }
    );
    assert!(battle.complete_script(33));
    assert_eq!(
        battle.players[0].statuses.duration(BattleStatus::Protect),
        2
    );
    assert!(battle.advance_resolution().is_empty());
    assert_eq!(
        battle.players[0].statuses.duration(BattleStatus::Protect),
        1
    );
    assert_eq!(
        battle.enemies[0].statuses.duration(BattleStatus::Protect),
        2
    );
    let enemy_poison = battle.take_script_request().unwrap();
    assert_eq!(
        enemy_poison.source,
        BattleScriptSource::EnemyPoison {
            enemy: 0,
            poison_id: 41,
        }
    );
    assert!(battle.complete_script(34));

    assert!(battle.advance_resolution().is_empty());
    assert_eq!(
        battle.enemies[0].statuses.duration(BattleStatus::Protect),
        1
    );
    for (enemy, entry, next_entry) in [(0, 21, 23), (1, 22, 24)] {
        let request = battle.take_script_request().unwrap();
        assert_eq!(
            request,
            BattleScriptRequest {
                source: BattleScriptSource::EnemyTurnStart { enemy },
                entry,
                object_id: enemy as u16,
            }
        );
        assert!(battle.complete_script(next_entry));
    }
    assert!(battle.advance_resolution().is_empty());
    assert_eq!(battle.round(), 2);
    assert_eq!(battle.players[0].poisons[0].script_entry, 33);
    assert_eq!(battle.enemies[0].poisons[0].script_entry, 34);

    assert!(battle.set_script_result(3));
    assert_eq!(
        battle.advance_resolution(),
        vec![BattleEvent::Finished(BattleResult::Won)]
    );
    assert!(battle.mark_victory_rewards_applied());
    assert!(battle.begin_battle_end_scripts());
    for (enemy, next_entry) in [(0, 25), (1, 26)] {
        let request = battle.take_script_request().unwrap();
        assert_eq!(request.source, BattleScriptSource::EnemyBattleEnd { enemy });
        assert_eq!(request.entry, 12);
        assert!(battle.complete_script(next_entry));
    }
    assert_eq!(
        battle.advance_resolution(),
        vec![BattleEvent::Finished(BattleResult::Won)]
    );
}

#[test]
fn lifecycle_scripts_read_each_enemy_slot_after_the_previous_script() {
    let (data, objects, magics, role) = fixture(1000, 0, 1000);
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();

    let first = battle.take_script_request().unwrap();
    assert_eq!(
        first.source,
        BattleScriptSource::EnemyTurnStart { enemy: 0 }
    );
    battle.enemies[1].turn_start_script = 71;
    assert!(battle.complete_script(21));
    let second = battle.take_script_request().unwrap();
    assert_eq!(
        second,
        BattleScriptRequest {
            source: BattleScriptSource::EnemyTurnStart { enemy: 1 },
            entry: 71,
            object_id: 1,
        }
    );
    assert!(battle.complete_script(72));

    assert!(battle.set_script_result(3));
    assert_eq!(
        battle.advance_resolution(),
        vec![BattleEvent::Finished(BattleResult::Won)]
    );
    assert!(battle.mark_victory_rewards_applied());
    assert!(battle.begin_battle_end_scripts());
    let first = battle.take_script_request().unwrap();
    assert_eq!(
        first.source,
        BattleScriptSource::EnemyBattleEnd { enemy: 0 }
    );
    battle.enemies[1].battle_end_script = 81;
    assert!(battle.complete_script(22));
    let second = battle.take_script_request().unwrap();
    assert_eq!(
        second,
        BattleScriptRequest {
            source: BattleScriptSource::EnemyBattleEnd { enemy: 1 },
            entry: 81,
            object_id: 1,
        }
    );
    assert!(battle.complete_script(82));
    assert_eq!(
        battle.advance_resolution(),
        vec![BattleEvent::Finished(BattleResult::Won)]
    );
}

#[test]
fn pre_battle_result_stops_remaining_turn_start_scripts() {
    let (data, objects, magics, role) = fixture(1000, 0, 1000);
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();

    let first = battle.take_script_request().unwrap();
    assert_eq!(
        first.source,
        BattleScriptSource::EnemyTurnStart { enemy: 0 }
    );
    assert!(battle.set_script_result(0));
    assert!(battle.complete_script(21));
    assert!(battle.take_script_request().is_none());
    assert_eq!(
        battle.advance_resolution(),
        vec![BattleEvent::Finished(BattleResult::Terminated)]
    );
}

#[test]
fn round_scripts_defer_the_last_scripted_result_until_lifecycle_completion() {
    let (data, objects, magics, role) = fixture(1000, 0, 1000);
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut battle);
    for enemy in &mut battle.enemies {
        enemy.turn_start_script = 0;
    }
    battle.players[0].poisons[0] = BattlePoison {
        object_id: 40,
        script_entry: 31,
    };
    battle.players[0].poisons[1] = BattlePoison {
        object_id: 41,
        script_entry: 32,
    };
    battle.flow = BattleFlow::RoundScripts {
        actor: 0,
        poison_slot: 0,
    };

    assert!(battle.advance_resolution().is_empty());
    assert_eq!(
        battle.take_script_request().unwrap().source,
        BattleScriptSource::PlayerPoison {
            role_id: 0,
            poison_id: 40,
        }
    );
    assert!(battle.set_script_result(1));
    assert!(battle.complete_script(33));

    assert!(battle.advance_resolution().is_empty());
    assert_eq!(
        battle.take_script_request().unwrap().source,
        BattleScriptSource::PlayerPoison {
            role_id: 0,
            poison_id: 41,
        }
    );
    assert!(battle.set_script_result(0));
    assert!(battle.complete_script(34));

    assert_eq!(
        battle.advance_resolution(),
        vec![BattleEvent::Finished(BattleResult::Terminated)]
    );
    assert_eq!(battle.players[0].poisons[0].script_entry, 33);
    assert_eq!(battle.players[0].poisons[1].script_entry, 34);
}

#[test]
fn battle_end_scripts_continue_after_result_changes_and_last_write_wins() {
    let (data, objects, magics, role) = fixture(1000, 0, 1000);
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut battle);

    assert!(battle.set_script_result(3));
    assert_eq!(
        battle.advance_resolution(),
        vec![BattleEvent::Finished(BattleResult::Won)]
    );
    assert!(battle.mark_victory_rewards_applied());
    assert!(battle.begin_battle_end_scripts());
    assert_eq!(
        battle.take_script_request().unwrap().source,
        BattleScriptSource::EnemyBattleEnd { enemy: 0 }
    );
    assert!(battle.set_script_result(1));
    assert!(battle.complete_script(21));

    assert_eq!(
        battle.take_script_request().unwrap().source,
        BattleScriptSource::EnemyBattleEnd { enemy: 1 }
    );
    assert!(battle.set_script_result(0));
    assert!(battle.complete_script(22));
    assert_eq!(
        battle.advance_resolution(),
        vec![BattleEvent::Finished(BattleResult::Terminated)]
    );
}

#[test]
fn round_poison_scripts_read_one_live_slot_at_a_time() {
    let (data, objects, magics, role) = fixture(1000, 0, 1000);
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut battle);
    for enemy in &mut battle.enemies {
        enemy.turn_start_script = 0;
    }
    battle.players[0].poisons[0] = BattlePoison {
        object_id: 40,
        script_entry: 31,
    };
    battle.players[0].poisons[1] = BattlePoison {
        object_id: 41,
        script_entry: 32,
    };
    battle.players[0]
        .statuses
        .set_for_player(BattleStatus::Protect, 2, true);
    battle.flow = BattleFlow::RoundScripts {
        actor: 0,
        poison_slot: 0,
    };

    assert!(battle.advance_resolution().is_empty());
    let first = battle.take_script_request().unwrap();
    assert_eq!(
        first.source,
        BattleScriptSource::PlayerPoison {
            role_id: 0,
            poison_id: 40,
        }
    );
    assert!(cure_poison(&mut battle.players[0].poisons, 40));
    assert!(battle.complete_script(33));

    assert!(battle.advance_resolution().is_empty());
    assert!(!battle.has_script_work());
    assert_eq!(battle.players[0].poisons[0].object_id, 41);
    assert_eq!(battle.players[0].poisons[0].script_entry, 32);
    assert_eq!(
        battle.players[0].statuses.duration(BattleStatus::Protect),
        1
    );

    battle.players[0].poisons[0] = BattlePoison {
        object_id: 40,
        script_entry: 41,
    };
    battle.players[0].poisons[1] = BattlePoison {
        object_id: 41,
        script_entry: 42,
    };
    battle.flow = BattleFlow::RoundScripts {
        actor: 0,
        poison_slot: 0,
    };
    assert!(battle.advance_resolution().is_empty());
    let first = battle.take_script_request().unwrap();
    assert_eq!(first.entry, 41);
    battle.players[0].poisons[1] = BattlePoison {
        object_id: 42,
        script_entry: 99,
    };
    assert!(battle.complete_script(43));
    assert!(battle.advance_resolution().is_empty());
    let second = battle.take_script_request().unwrap();
    assert_eq!(
        second,
        BattleScriptRequest {
            source: BattleScriptSource::PlayerPoison {
                role_id: 0,
                poison_id: 42,
            },
            entry: 99,
            object_id: 0,
        }
    );
}

#[test]
fn fully_disabled_party_advances_rounds_without_being_declared_defeated() {
    let (data, objects, magics, role) = fixture(1000, 0, 1000);
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut battle);
    battle.players[0]
        .statuses
        .set_for_player(BattleStatus::Sleep, 2, true);
    battle.refresh_player_effects();
    assert_eq!(battle.phase(), BattlePhase::AwaitingCommand);
    assert_eq!(battle.active_player(), None);

    let mut first = battle.advance_automatic_turns();
    first.extend(resolve_until_input_or_finish(&mut battle));
    assert!(!matches!(first.last(), Some(BattleEvent::RoundCompleted)));
    assert_eq!(battle.phase(), BattlePhase::AwaitingCommand);
    assert_eq!(battle.active_player(), None);
    let mut second = battle.advance_automatic_turns();
    second.extend(resolve_until_input_or_finish(&mut battle));
    assert!(!matches!(second.last(), Some(BattleEvent::RoundCompleted)));
    assert_eq!(battle.active_player(), Some(0));
    assert_eq!(battle.players[0].statuses.duration(BattleStatus::Sleep), 0);
}

#[test]
fn scripted_hp_mutations_use_classic_word_arithmetic_and_thresholds() {
    let (data, objects, magics, role) = fixture(100, 0, 50);
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    battle.players[0].hp = 20;
    assert!(battle.drain_enemy_hp(0, 20));
    assert_eq!(battle.enemies[0].hp, 80);
    assert_eq!(battle.players[0].hp, 40);
    assert_eq!(battle.enemy_hp_above(0, 79), Some(true));
    assert_eq!(battle.enemy_hp_above(0, 80), Some(false));
    assert!(battle.halve_enemy_hp(0, 10));
    assert_eq!(battle.enemies[0].hp, 70);
    assert!(battle.kill_enemy(0));
    assert_eq!(battle.enemies[0].hp, 0);
    assert!(battle.halve_enemy_hp(0, 10));
    assert_eq!(battle.enemies[0].hp, u16::MAX);
    assert!(!battle.damage_enemy(99, 1, false));
}

#[test]
fn dead_enemy_slots_keep_residual_fields_and_all_target_ops_skip_them() {
    let (data, objects, magics, role) = fixture(1000, 0, 500);
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut battle);

    battle.enemies[0].hp = 1;
    battle.enemies[0].poisons[0] = BattlePoison {
        object_id: 40,
        script_entry: 70,
    };
    assert!(battle.damage_enemy(0, 2, false));
    assert_eq!(battle.enemies[0].hp, u16::MAX);
    battle.queue_post_action_check(false);
    assert_eq!(battle.enemies[0].object_id, 0);
    assert_eq!(battle.collect_enemy(0), Some(5));

    assert!(battle.damage_enemy(0, 7, true));
    assert_eq!(battle.enemies[0].hp, u16::MAX);
    assert_eq!(battle.enemies[1].hp, 993);
    assert!(battle.cure_enemy_poison(0, 40, true));
    assert_eq!(battle.enemies[0].poisons[0].object_id, 40);

    assert!(battle.damage_enemy(0, 1, false));
    assert_eq!(battle.enemies[0].hp, u16::MAX - 1);
    assert!(battle.kill_enemy(1));
    battle.queue_post_action_check(false);
    assert_eq!(battle.enemy_not_first_kind(1), Some(true));
}

#[test]
fn queued_and_repeated_magic_retargets_without_falling_back_to_attack() {
    let (data, objects, magics, mut role) = fixture(10_000, 0, 500);
    role.magic[0] = 2;
    role.mp = 20;
    role.max_mp = 20;
    role.dexterity = 100;
    let mut battle =
        BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
    complete_pending_scripts(&mut battle);
    for enemy in &mut battle.enemies {
        enemy.statuses.set_for_enemy(BattleStatus::Paralyzed, 8);
    }

    assert!(battle.cast_magic(0, 0).is_some());
    assert!(battle.kill_enemy(0));
    battle.queue_post_action_check(false);
    let events = resolve_until_input_or_finish(&mut battle);
    assert!(events.iter().any(|event| matches!(
        event,
        BattleEvent::PlayerMagic {
            enemy: 1,
            magic_object: 2,
            ..
        }
    )));
    assert!(!events
        .iter()
        .any(|event| matches!(event, BattleEvent::PlayerAttack { .. })));

    assert!(battle.repeat_last_action().is_some());
    assert!(matches!(
        battle.player_actions[0],
        Some(PlayerAction::Magic {
            target: BattleTarget::Enemy(1),
            ..
        })
    ));
}
