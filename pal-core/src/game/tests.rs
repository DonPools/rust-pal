//! GameState integration and regression tests.

use crate::script::{ScriptAction, ScriptEvent, ScriptOpcode, ScriptRuntime, ScriptVisual};

use std::collections::HashSet;

use super::*;

fn blocking_object(world_x: i32, world_y: i32) -> SceneObject {
    SceneObject {
        id: 1,
        world_x,
        world_y,
        layer: 0,
        trigger_script: 0,
        auto_script: 0,
        state: 2,
        trigger_mode: 0,
        sprite_index: Some(1),
        frames_per_direction: 3,
        sprite_frame_count: 12,
        direction: Direction::South,
        current_frame: 0,
        vanish_time: 0,
        auto_script_idle_frame: 0,
    }
}

struct TestMap {
    blocked: HashSet<(i32, i32)>,
    size: (i32, i32),
}

impl CollisionMap for TestMap {
    fn is_world_blocked(&self, world_x: i32, world_y: i32) -> bool {
        self.blocked.contains(&(world_x, world_y))
    }

    fn world_size(&self) -> (i32, i32) {
        self.size
    }
}

fn test_map() -> TestMap {
    TestMap {
        blocked: HashSet::new(),
        size: (1000, 800),
    }
}

fn state(blocked: &[(i32, i32)]) -> GameState<TestMap> {
    GameState::new(
        TestMap {
            blocked: blocked.iter().copied().collect(),
            size: (1000, 800),
        },
        Role {
            sprite_index: 0,
            world_x: 320,
            world_y: 240,
            direction: Direction::South,
            anim_frame: 0,
            frames_per_direction: 4,
        },
        320,
        200,
    )
}

fn run_action_only_script(
    scripts: ScriptTable,
    request: TriggerRequest,
    state: &mut GameState<TestMap>,
) {
    let mut runtime = ScriptRuntime::new(scripts);
    assert!(runtime.start(request));
    for _ in 0..64 {
        match runtime.advance() {
            Some(ScriptEvent::Action(action)) => {
                assert!(state.apply_script_action(action));
            }
            Some(ScriptEvent::Completed { .. }) => return,
            event => panic!("action-only test script yielded {event:?}"),
        }
    }
    panic!("action-only test script did not complete");
}

fn battle_data_for_growth() -> BattleData {
    let mut chunks = vec![Vec::new(); 15];
    chunks[1] = vec![0; 70];
    chunks[1][22..24].copy_from_slice(&1000u16.to_le_bytes());
    chunks[2] = [1, u16::MAX, u16::MAX, u16::MAX, u16::MAX]
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect();
    chunks[5] = vec![0; 12];
    chunks[6] = vec![0; 20];
    chunks[6][0..2].copy_from_slice(&2u16.to_le_bytes());
    chunks[6][2..4].copy_from_slice(&9u16.to_le_bytes());
    chunks[13] = vec![0; 100];
    chunks[14] = vec![0; 200];
    chunks[14][2..4].copy_from_slice(&10u16.to_le_bytes());
    chunks[14][198..200].copy_from_slice(&10u16.to_le_bytes());

    let table_size = (chunks.len() + 1) * 4;
    let mut offset = table_size as u32;
    let mut archive = Vec::new();
    archive.extend_from_slice(&offset.to_le_bytes());
    for chunk in &chunks {
        offset += chunk.len() as u32;
        archive.extend_from_slice(&offset.to_le_bytes());
    }
    for chunk in chunks {
        archive.extend_from_slice(&chunk);
    }
    BattleData::parse(&archive).unwrap()
}

fn battle_item_state(party_size: usize) -> GameState<TestMap> {
    let mut role_data = vec![0; 900];
    for role in 0..party_size {
        for (array, value) in [
            (7, 500u16),
            (9, 500),
            (17, 80),
            (19, 20),
            (20, 100),
            (22, 20),
        ] {
            let offset = (array * PLAYER_ROLE_COUNT + role) * 2;
            role_data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
    }
    let roles = PlayerRoles::parse(&role_data).unwrap();
    let mut party = Party::single(0, &roles).unwrap();
    for role in 1..party_size {
        assert!(party.add(u16::try_from(role).unwrap(), &roles));
    }
    let object_words = [
        [0u16; 6],
        [0, 0, 0, 0, 0, 0],
        [0, 0, 31, 0, 0, ITEM_FLAG_USABLE | ITEM_FLAG_CONSUMING],
        [0, 0, 32, 0, 0, ITEM_FLAG_USABLE],
        [0, 0, 0, 0, 41, ITEM_FLAG_THROWABLE],
        [0, 0, 0, 0, 0, MAGIC_FLAG_APPLY_TO_ALL],
    ];
    let objects = GlobalObjects::parse(
        &object_words
            .into_iter()
            .flatten()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
        pal_assets::objects::ObjectLayout::Dos,
    )
    .unwrap();
    let stores = Stores::parse(&[0; 18]).unwrap();
    let magics = Magics::parse(&[0; 32]).unwrap();
    let scripts = ScriptTable::parse(&[0; 8]).unwrap();
    let mut state = state(&[])
        .with_party(party)
        .with_player_roles(roles)
        .with_economy_data(stores, objects)
        .with_magic_data(magics)
        .with_battle_data(battle_data_for_growth());
    for item_id in 2..=4 {
        assert!(state.apply_script_action(ScriptAction::AddItem { item_id, amount: 1 }));
    }
    assert!(state.start_battle(
        BattleRequest {
            enemy_team: 0,
            lost_entry: 0,
            flee_entry: 0,
            is_boss: true,
        },
        &scripts,
    ));
    state
}

#[test]
fn battle_experience_levels_living_roles_and_teaches_magic() {
    let mut role_data = vec![0; 900];
    for (array, value) in [
        (6, 1u16),
        (7, 100),
        (8, 50),
        (9, 20),
        (10, 10),
        (17, 30),
        (18, 20),
        (19, 25),
        (20, 15),
        (21, 12),
    ] {
        let offset = array * 12;
        role_data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    let roles = PlayerRoles::parse(&role_data).unwrap();
    let party = Party::single(0, &roles).unwrap();
    let mut state = state(&[])
        .with_party(party)
        .with_player_roles(roles)
        .with_battle_data(battle_data_for_growth());

    assert_eq!(state.award_battle_experience_for_role(0, 25), 1);
    assert_eq!(state.learn_eligible_magics_for_role(0), vec![9]);

    let role = state.player_role(0).unwrap();
    assert_eq!(role.level, 2);
    assert_eq!(state.player_experience(0), Some(15));
    assert!(role.max_hp >= 110);
    assert_eq!(role.hp, role.max_hp);
    assert_eq!(role.mp, role.max_mp);
    assert_eq!(role.magic[0], 9);
    assert_eq!(state.party.leader().unwrap().attributes.level, 2);

    let roles = state.player_roles.as_mut().unwrap();
    roles.role_mut(0).unwrap().magic[0] = 0;
    state.party.sync_from_roles(roles);
    assert_eq!(state.award_battle_experience_for_role(0, 0), 0);
    assert_eq!(state.learn_eligible_magics_for_role(0), vec![9]);
    assert_eq!(state.player_role(0).unwrap().magic[0], 9);
}

#[test]
fn max_level_primary_experience_keeps_only_the_threshold_remainder() {
    let mut role_data = vec![0; 900];
    for (array, value) in [(6, 99u16), (7, 100), (9, 100)] {
        let offset = array * PLAYER_ROLE_COUNT * 2;
        role_data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    let roles = PlayerRoles::parse(&role_data).unwrap();
    let party = Party::single(0, &roles).unwrap();
    let mut state = state(&[])
        .with_party(party)
        .with_player_roles(roles)
        .with_battle_data(battle_data_for_growth());
    state.role_experience[0] = 9;

    assert_eq!(state.award_battle_experience_for_role(0, 21), 0);
    assert_eq!(state.player_role(0).unwrap().level, 99);
    assert_eq!(state.player_experience(0), Some(0));
}

#[test]
fn scripted_level_up_grows_stats_without_restoring_health_and_halves_cash() {
    let mut role_data = vec![0; 900];
    for (array, value) in [
        (6, 1u16),
        (7, 100),
        (8, 60),
        (9, 50),
        (10, 20),
        (17, 30),
        (18, 25),
        (19, 20),
        (20, 15),
        (21, 10),
    ] {
        let offset = array * PLAYER_ROLE_COUNT * 2;
        role_data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    let roles = PlayerRoles::parse(&role_data).unwrap();
    let party = Party::single(0, &roles).unwrap();
    let mut state = state(&[]).with_party(party).with_player_roles(roles);
    state.role_experience[0] = 123;
    state.cash = 101;

    assert!(state.apply_script_action(ScriptAction::LevelUpPlayer {
        role_id: 0,
        levels: 2,
    }));
    let role = state.player_role(0).unwrap();
    assert_eq!(role.level, 3);
    assert!((120..=134).contains(&role.max_hp));
    assert!((76..=86).contains(&role.max_mp));
    assert!((38..=40).contains(&role.attack_strength));
    assert!((33..=35).contains(&role.magic_strength));
    assert!((24..=26).contains(&role.defense));
    assert!((19..=21).contains(&role.dexterity));
    assert_eq!(role.flee_rate, 14);
    assert_eq!(role.hp, 50);
    assert_eq!(role.mp, 20);
    assert_eq!(state.player_experience(0), Some(0));
    assert_eq!(state.party.leader().unwrap().attributes.level, 3);

    assert!(state.apply_script_action(ScriptAction::HalveCash));
    assert_eq!(state.cash, 50);
}

#[test]
fn object_script_overrides_update_union_views_and_round_trip() {
    let mut role_data = vec![0; 900];
    let hp_offset = 9 * PLAYER_ROLE_COUNT * 2;
    role_data[hp_offset..hp_offset + 2].copy_from_slice(&100u16.to_le_bytes());
    let roles = PlayerRoles::parse(&role_data).unwrap();
    let party = Party::single(0, &roles).unwrap();
    let objects = GlobalObjects::parse(
        &[[0u16; 6], [2, 0, 11, 12, 13, 0]]
            .into_iter()
            .flatten()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
        pal_assets::objects::ObjectLayout::Dos,
    )
    .unwrap();
    let stores = Stores::parse(&[0; 18]).unwrap();
    let mut state = state(&[])
        .with_party(party)
        .with_player_roles(roles)
        .with_economy_data(stores, objects);

    for (field, script_entry) in [(0, 21), (1, 22), (2, 23)] {
        assert!(state.apply_script_action(ScriptAction::SetObjectScript {
            object_id: 1,
            script_entry,
            field,
        }));
    }
    assert_eq!(state.item_use_scripts.get(&1), Some(&21));
    assert_eq!(state.magic_success_scripts.get(&1), Some(&21));
    assert_eq!(state.item_equip_scripts.get(&1), Some(&22));
    assert_eq!(state.magic_use_scripts.get(&1), Some(&22));
    assert_eq!(state.item_throw_scripts.get(&1), Some(&23));
    assert!(!state.apply_script_action(ScriptAction::SetObjectScript {
        object_id: 1,
        script_entry: 99,
        field: 3,
    }));
    assert!(!state.apply_script_action(ScriptAction::SetObjectScript {
        object_id: 2,
        script_entry: 99,
        field: 0,
    }));

    assert!(state.apply_script_action(ScriptAction::PoisonPlayer {
        role_id: 0,
        poison_id: 1,
        apply_to_all: false,
    }));
    assert_eq!(state.player_poisons(0).unwrap()[0].script_entry, 21);

    let snapshot = state
        .decode_snapshot(&state.encode_snapshot().unwrap())
        .unwrap();
    assert!(state.apply_script_action(ScriptAction::SetObjectScript {
        object_id: 1,
        script_entry: 99,
        field: 0,
    }));
    state.restore_snapshot(snapshot, test_map());
    assert_eq!(state.object_script_overrides.get(&(1, 0)), Some(&21));
    assert_eq!(state.item_use_scripts.get(&1), Some(&21));
    assert_eq!(state.magic_success_scripts.get(&1), Some(&21));
}

#[test]
fn object_script_overrides_seed_enemy_lifecycle_without_rewriting_transform_state() {
    let mut role_data = vec![0; 900];
    for (array, value) in [(7, 100u16), (9, 100), (17, 20), (19, 20), (20, 20)] {
        let offset = array * PLAYER_ROLE_COUNT * 2;
        role_data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    let roles = PlayerRoles::parse(&role_data).unwrap();
    let party = Party::single(0, &roles).unwrap();
    let objects = GlobalObjects::parse(
        &[[0u16; 6], [0, 0, 11, 12, 13, 0], [0, 0, 101, 102, 103, 0]]
            .into_iter()
            .flatten()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
        pal_assets::objects::ObjectLayout::Dos,
    )
    .unwrap();
    let stores = Stores::parse(&[0; 18]).unwrap();
    let magics = Magics::parse(&[0; 32]).unwrap();
    let scripts = ScriptTable::parse(&[0; 8]).unwrap();
    let mut state = state(&[])
        .with_party(party)
        .with_player_roles(roles)
        .with_economy_data(stores, objects)
        .with_magic_data(magics)
        .with_battle_data(battle_data_for_growth());
    for (object_id, entries) in [(1, [0, 22, 23]), (2, [201, 202, 203])] {
        for (field, script_entry) in entries.into_iter().enumerate() {
            assert!(state.apply_script_action(ScriptAction::SetObjectScript {
                object_id,
                script_entry,
                field: u16::try_from(field).unwrap(),
            }));
        }
    }
    assert!(state.start_battle(
        BattleRequest {
            enemy_team: 0,
            lost_entry: 0,
            flee_entry: 0,
            is_boss: true,
        },
        &scripts,
    ));
    let enemy = &state.battle().unwrap().enemies[0];
    assert_eq!(
        (
            enemy.turn_start_script,
            enemy.battle_end_script,
            enemy.ready_script,
        ),
        (0, 22, 23)
    );
    assert!(state.take_battle_script().is_none());
    assert!(state.apply_script_action(ScriptAction::PoisonEnemy {
        enemy_index: 0,
        poison_id: 2,
        apply_to_all: false,
    }));
    assert_eq!(
        state.battle().unwrap().enemies[0].poisons[0].script_entry,
        203
    );

    assert_eq!(state.transform_enemy(0, 2), Some(true));
    let transformed = &state.battle().unwrap().enemies[0];
    assert_eq!(transformed.object_id, 2);
    assert_eq!(
        (
            transformed.turn_start_script,
            transformed.battle_end_script,
            transformed.ready_script,
        ),
        (0, 22, 23)
    );
    assert_eq!(transformed.poisons[0].script_entry, 203);
}

#[test]
fn battle_settlement_clears_temporary_statuses_and_low_level_poisons() {
    let mut role_data = vec![0; 900];
    for (array, value) in [(7, 100u16), (9, 100)] {
        let offset = array * PLAYER_ROLE_COUNT * 2;
        role_data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    let roles = PlayerRoles::parse(&role_data).unwrap();
    let party = Party::single(0, &roles).unwrap();
    let objects = GlobalObjects::parse(
        &[
            [0u16; 6],
            [0, 0, 0, 0, 0, 0],
            [2, 0, 0, 0, 0, 0],
            [4, 0, 0, 0, 0, 0],
        ]
        .into_iter()
        .flatten()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>(),
        pal_assets::objects::ObjectLayout::Dos,
    )
    .unwrap();
    let stores = Stores::parse(&[0; 18]).unwrap();
    let magics = Magics::parse(&[0; 32]).unwrap();
    let scripts = ScriptTable::parse(&[0; 8]).unwrap();
    let mut state = state(&[])
        .with_party(party)
        .with_player_roles(roles)
        .with_economy_data(stores, objects)
        .with_magic_data(magics)
        .with_battle_data(battle_data_for_growth());
    assert!(state.player_statuses[0].set_for_player(BattleStatus::Protect, 5, true));
    assert!(state.player_statuses[0].set_for_player(BattleStatus::Haste, 1000, true));
    state.player_poisons[0][0] = BattlePoison {
        object_id: 2,
        script_entry: 20,
    };
    state.player_poisons[0][1] = BattlePoison {
        object_id: 3,
        script_entry: 30,
    };

    assert!(state.start_battle(
        BattleRequest {
            enemy_team: 0,
            lost_entry: 0,
            flee_entry: 0,
            is_boss: true,
        },
        &scripts,
    ));
    assert!(state.battle_mut().unwrap().set_script_result(0));
    assert_eq!(
        state.advance_battle_resolution(),
        vec![BattleEvent::Finished(BattleResult::Terminated)]
    );
    assert_eq!(
        state.settle_battle(),
        Some((BattleResult::Terminated, BattleRewards::default()))
    );
    assert_eq!(
        state.player_status_duration(0, BattleStatus::Protect),
        Some(0)
    );
    assert_eq!(
        state.player_status_duration(0, BattleStatus::Haste),
        Some(1000)
    );
    assert_eq!(state.player_poisons[0][0].object_id, 3);
    assert_eq!(state.player_poisons[0][1], BattlePoison::default());
}

#[test]
fn victory_applies_hidden_experience_and_recovers_half_missing_hp_and_mp() {
    let mut state = battle_item_state(1);
    state
        .player_roles
        .as_mut()
        .unwrap()
        .role_mut(0)
        .unwrap()
        .max_mp = 500;
    for category in 1..SAVE_EXPERIENCE_KINDS {
        state.save_experience[category][0].level = 1;
    }
    {
        let battle = state.battle_mut().unwrap();
        battle.players[0].hp = 100;
        battle.players[0].mp = 100;
        battle.players[0].max_mp = 500;
        battle.enemies[0].hp = 1;
        battle.enemies[0].experience = 20;
        battle.enemies[0]
            .statuses
            .set_for_enemy(BattleStatus::Paralyzed, 2);
        assert!(battle.attack(0).is_some());
    }
    for _ in 0..32 {
        let _ = state.advance_battle_resolution();
        if matches!(
            state.battle().map(BattleState::phase),
            Some(BattlePhase::Finished(BattleResult::Won))
        ) {
            break;
        }
    }
    assert!(state.prepare_battle_victory().is_some());
    assert!(state.begin_battle_end_scripts());
    for _ in 0..8 {
        if let Some(request) = state.take_battle_script() {
            assert!(state.finish_battle_script(request.script_entry, true));
        }
        let _ = state.advance_battle_resolution();
        if state.battle().is_some_and(|battle| battle.ready_to_leave()) {
            break;
        }
    }
    assert_eq!(
        state.settle_battle().map(|settled| settled.0),
        Some(BattleResult::Won)
    );

    let role = state.player_role(0).unwrap();
    assert!(role.max_hp > 500);
    assert!(role.attack_strength > 80);
    assert_eq!(role.mp, 300);
    assert_eq!(role.hp, 100 + (role.max_hp - 100) / 2);
    assert_eq!(state.save_experience[HIDDEN_EXP_ATTACK + 1][0].count, 0);
}

#[test]
fn victory_refills_health_after_primary_and_hidden_growth_in_the_same_battle() {
    let mut state = battle_item_state(1);
    state.role_experience[0] = 0;
    for category in 1..SAVE_EXPERIENCE_KINDS {
        state.save_experience[category][0].level = 1;
    }
    {
        let roles = state.player_roles.as_mut().unwrap();
        roles.role_mut(0).unwrap().level = 1;
        state.party.sync_from_roles(roles);
        let battle = state.battle_mut().unwrap();
        battle.players[0].level = 1;
        battle.enemies[0].hp = 1;
        battle.enemies[0].experience = 20;
        battle.enemies[0]
            .statuses
            .set_for_enemy(BattleStatus::Paralyzed, 2);
        assert!(battle.attack(0).is_some());
    }
    for _ in 0..32 {
        let _ = state.advance_battle_resolution();
        if matches!(
            state.battle().map(BattleState::phase),
            Some(BattlePhase::Finished(BattleResult::Won))
        ) {
            break;
        }
    }

    let settlement = state.prepare_battle_victory_settlement().unwrap();
    let player = &settlement.players[0];
    assert_eq!(player.levels_gained, 1);
    assert!(player.hidden_growth[HIDDEN_EXP_HEALTH] > 0);
    let role = state.player_role(0).unwrap();
    assert_eq!(role.hp, role.max_hp);
    assert_eq!(role.mp, role.max_mp);
    let battle_player = &state.battle().unwrap().players[0];
    assert_eq!(battle_player.hp, battle_player.max_hp);
    assert_eq!(battle_player.mp, battle_player.max_mp);
}

#[test]
fn hidden_experience_keeps_growing_at_level_99_without_a_999_cap() {
    let mut state = battle_item_state(1);
    state.save_experience[HIDDEN_EXP_DEFENSE + 1][0].level = 99;
    state
        .player_roles
        .as_mut()
        .unwrap()
        .role_mut(0)
        .unwrap()
        .defense = 999;
    assert!(state.battle_mut().unwrap().defend().is_some());
    for _ in 0..32 {
        let _ = state.advance_battle_resolution();
        if state
            .battle()
            .is_some_and(|battle| battle.active_player().is_some())
        {
            break;
        }
    }
    let battle = state.battle().unwrap().clone();
    state.clear_hidden_experience_counts(0);
    state.award_hidden_battle_experience_for_player(&battle, 0, 5);
    assert!(state.player_role(0).unwrap().defense > 999);
    assert_eq!(state.save_experience[HIDDEN_EXP_DEFENSE + 1][0].level, 99);
}

#[test]
fn battle_magic_scripts_scale_damage_from_remaining_mp_and_cash() {
    let mut role_data = vec![0; 900];
    for (array, value) in [
        (7, 500u16),
        (8, 20),
        (9, 500),
        (10, 20),
        (17, 20),
        (18, 80),
        (19, 20),
        (20, 100),
        (32, 2),
    ] {
        let offset = array * PLAYER_ROLE_COUNT * 2;
        role_data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    let roles = PlayerRoles::parse(&role_data).unwrap();
    let party = Party::single(0, &roles).unwrap();
    let objects = GlobalObjects::parse(
        &[
            [0u16; 6],
            [0, 0, 0, 0, 0, 0],
            [
                0,
                0,
                32,
                31,
                0,
                crate::battle::MAGIC_FLAG_USABLE_IN_BATTLE
                    | crate::battle::MAGIC_FLAG_USABLE_TO_ENEMY,
            ],
        ]
        .into_iter()
        .flatten()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>(),
        pal_assets::objects::ObjectLayout::Dos,
    )
    .unwrap();
    let stores = Stores::parse(&[0; 18]).unwrap();
    let mut magic_data = [0; 32];
    magic_data[24..26].copy_from_slice(&5u16.to_le_bytes());
    magic_data[26..28].copy_from_slice(&50u16.to_le_bytes());
    let magics = Magics::parse(&magic_data).unwrap();
    let scripts = ScriptTable::parse(&[0; 8]).unwrap();
    let mut state = state(&[])
        .with_party(party)
        .with_player_roles(roles)
        .with_economy_data(stores, objects)
        .with_magic_data(magics)
        .with_battle_data(battle_data_for_growth());
    state.cash = 100;
    assert!(state.start_battle(
        BattleRequest {
            enemy_team: 0,
            lost_entry: 0,
            flee_entry: 0,
            is_boss: true,
        },
        &scripts,
    ));
    assert!(state.battle_mut().unwrap().cast_magic(0, 0).is_some());
    assert!(state.advance_battle_resolution().is_empty());
    let use_script = state.take_battle_script().unwrap();
    assert_eq!(use_script.script_entry, 31);
    assert!(state.apply_script_action(ScriptAction::ScaleMagicByMp {
        role_id: 0,
        magic_object: 2,
        multiplier: 8,
    }));
    assert_eq!(state.player_role(0).unwrap().mp, 0);
    assert!(state.apply_script_action(ScriptAction::ScaleMagicByCash { magic_object: 2 }));
    assert_eq!(state.cash, 0);
    assert!(state.apply_script_action(ScriptAction::SetBattleBlow { amount: -3 }));
    assert!(state.finish_battle_script(41, true));
    assert!(matches!(
        state.advance_battle_resolution().as_slice(),
        [BattleEvent::PlayerMagic {
            phase: crate::battle::MagicEventPhase::Visual,
            damage: 0,
            ..
        }]
    ));
    assert!(state.advance_battle_resolution().is_empty());
    let success_script = state.take_battle_script().unwrap();
    assert_eq!(success_script.script_entry, 32);
    assert!(state.finish_battle_script(42, true));
    assert!(matches!(
        state.advance_battle_resolution().as_slice(),
        [BattleEvent::PlayerMagic {
            blow: -3,
            damage,
            ..
        }] if *damage >= 40
    ));
}

#[test]
fn battle_item_selection_reserves_the_last_inventory_copy() {
    let mut state = battle_item_state(2);
    assert!(state.battle_use_item(2, Some(0)).is_some());
    assert!(state.battle_usable_item(2).is_none());
    assert!(state.battle_use_item(2, Some(1)).is_none());

    assert!(state.battle_throw_item(4, Some(0)).is_some());
    assert!(state.throwable_item(4).is_none());

    let mut reusable = battle_item_state(2);
    assert!(reusable.battle_use_item(3, Some(0)).is_some());
    assert_eq!(reusable.battle_usable_item(3).unwrap().amount, 1);
    assert!(reusable.battle_use_item(3, Some(1)).is_some());
}

#[test]
fn all_target_simulated_magic_accepts_classic_sentinel_and_carries_visual_data() {
    let mut state = battle_item_state(1);
    let mut second = state.battle().unwrap().enemies[0].clone();
    second.slot = 1;
    state.battle_mut().unwrap().enemies.push(second);
    let hp_before = state
        .battle()
        .unwrap()
        .enemies
        .iter()
        .map(|enemy| enemy.hp)
        .collect::<Vec<_>>();

    assert!(
        state.apply_script_action(ScriptAction::SimulatePlayerMagic {
            enemy_index: u16::MAX,
            magic_object: 5,
            base_strength: 100,
        })
    );

    assert!(state
        .battle()
        .unwrap()
        .enemies
        .iter()
        .zip(&hp_before)
        .all(|(enemy, before)| enemy.hp < *before));
    assert!(matches!(
        state.advance_battle_resolution().as_slice(),
        [
            BattleEvent::SimulatedMagic {
                magic,
                visual: true,
                ..
            },
            BattleEvent::SimulatedMagic { visual: false, .. }
        ] if magic.object_id == 5 && magic.attacks_all
    ));
}

#[test]
fn starting_battle_revives_party_members_and_clears_puppet() {
    let mut state = battle_item_state(1);
    assert!(state.battle_mut().unwrap().set_script_result(0));
    assert_eq!(
        state.advance_battle_resolution(),
        vec![BattleEvent::Finished(BattleResult::Terminated)]
    );
    assert!(state.settle_battle().is_some());

    let roles = state.player_roles.as_mut().unwrap();
    roles.role_mut(0).unwrap().hp = 0;
    state.party.sync_from_roles(roles);
    assert!(state.player_statuses[0].set_for_player(BattleStatus::Puppet, 5, false));
    let scripts = ScriptTable::parse(&[0; 8]).unwrap();
    assert!(state.start_battle(
        BattleRequest {
            enemy_team: 0,
            lost_entry: 0,
            flee_entry: 0,
            is_boss: true,
        },
        &scripts,
    ));
    assert_eq!(state.player_role(0).unwrap().hp, 1);
    assert_eq!(state.battle().unwrap().players[0].hp, 1);
    assert!(!state.battle().unwrap().players[0]
        .statuses
        .is_active(BattleStatus::Puppet));
}

#[test]
fn battle_items_consume_after_completion_and_persist_script_entries() {
    let mut consuming = battle_item_state(1);
    assert!(consuming.battle_use_item(2, Some(0)).is_some());
    assert!(matches!(
        consuming.advance_battle_resolution().as_slice(),
        [BattleEvent::PlayerUseItem { item_object: 2, .. }]
    ));
    assert!(consuming.advance_battle_resolution().is_empty());
    let request = consuming.take_battle_script().unwrap();
    assert_eq!(request.script_entry, 31);
    assert!(consuming.finish_battle_script(51, false));
    assert!(matches!(
        consuming.advance_battle_resolution().as_slice(),
        [BattleEvent::PlayerItemFeedback {
            item_object: 2,
            consume: true,
            ..
        }]
    ));
    assert_eq!(consuming.inventory_count(2), 0);
    assert_eq!(consuming.item_use_scripts.get(&2), Some(&51));

    let mut reusable = battle_item_state(1);
    assert!(reusable.battle_use_item(3, Some(0)).is_some());
    assert!(matches!(
        reusable.advance_battle_resolution().as_slice(),
        [BattleEvent::PlayerUseItem { item_object: 3, .. }]
    ));
    assert!(reusable.advance_battle_resolution().is_empty());
    assert!(reusable.take_battle_script().is_some());
    assert!(reusable.finish_battle_script(52, true));
    assert!(matches!(
        reusable.advance_battle_resolution().as_slice(),
        [BattleEvent::PlayerItemFeedback {
            item_object: 3,
            consume: false,
            ..
        }]
    ));
    assert_eq!(reusable.inventory_count(3), 1);
    assert_eq!(reusable.item_use_scripts.get(&3), Some(&52));

    let mut thrown = battle_item_state(1);
    assert!(thrown.battle_throw_item(4, Some(0)).is_some());
    assert!(matches!(
        thrown.advance_battle_resolution().as_slice(),
        [BattleEvent::PlayerThrowItem { item_object: 4, .. }]
    ));
    assert!(thrown.advance_battle_resolution().is_empty());
    let request = thrown.take_battle_script().unwrap();
    assert_eq!(request.script_entry, 41);
    assert!(thrown.finish_battle_script(53, false));
    assert!(matches!(
        thrown.advance_battle_resolution().as_slice(),
        [BattleEvent::PlayerItemFeedback {
            item_object: 4,
            consume: true,
            ..
        }]
    ));
    assert_eq!(thrown.inventory_count(4), 0);
    assert_eq!(thrown.item_throw_scripts.get(&4), Some(&53));
}

#[test]
fn temporary_battle_effects_use_base_percentages_and_do_not_persist() {
    let mut state = battle_item_state(1);
    assert_eq!(state.battle().unwrap().players[0].attack_strength, 80);

    assert!(
        state.apply_script_action(ScriptAction::AdjustTemporaryPlayerStat {
            role_id: 0,
            attribute: 17,
            percent: 50,
        })
    );
    assert_eq!(state.battle().unwrap().players[0].attack_strength, 120);
    assert!(
        state.apply_script_action(ScriptAction::AdjustTemporaryPlayerStat {
            role_id: 0,
            attribute: 17,
            percent: -50,
        })
    );
    assert_eq!(state.battle().unwrap().players[0].attack_strength, 40);

    assert!(
        state.apply_script_action(ScriptAction::SetTemporaryBattleSprite {
            role_id: 0,
            sprite: 5,
        })
    );
    assert_eq!(state.battle().unwrap().players[0].battle_sprite_num, 5);
    assert!(
        state.apply_script_action(ScriptAction::SetTemporaryBattleSprite {
            role_id: 0,
            sprite: 0,
        })
    );
    assert_eq!(state.battle().unwrap().players[0].battle_sprite_num, 0);

    assert!(
        state.apply_script_action(ScriptAction::AdjustTemporaryPlayerStat {
            role_id: 0,
            attribute: 22,
            percent: 400,
        })
    );
    assert_eq!(state.battle().unwrap().players[0].poison_resistance, 100);
    assert!(state.apply_script_action(ScriptAction::PoisonPlayer {
        role_id: 0,
        poison_id: 4,
        apply_to_all: false,
    }));
    assert!(!state.player_has_poison(0, 4));

    assert!(state.battle_mut().unwrap().set_script_result(0));
    assert_eq!(
        state.advance_battle_resolution(),
        vec![BattleEvent::Finished(BattleResult::Terminated)]
    );
    assert!(state.settle_battle().is_some());
    assert_eq!(state.player_role(0).unwrap().attack_strength, 80);
}

#[test]
fn dynamic_enemy_actions_resolve_original_slots_after_reuse() {
    let mut state = battle_item_state(1);
    assert!(state.apply_script_action(ScriptAction::DivideEnemy {
        enemy_index: 0,
        copies: 1,
        failure_entry: 80,
    }));
    assert_eq!(state.battle().unwrap().enemies.len(), 2);
    assert_eq!(state.battle().unwrap().enemies[0].hp, 500);
    assert_eq!(state.battle().unwrap().enemies[1].slot, 1);

    assert!(state.apply_script_action(ScriptAction::KillEnemy { enemy_index: 1 }));
    state.battle_mut().unwrap().queue_post_action_check(false);
    assert!(state.apply_script_action(ScriptAction::SummonEnemy {
        enemy_index: 0,
        object_id: 0,
        count: 1,
        failure_entry: 81,
    }));
    let battle = state.battle().unwrap();
    assert_eq!(battle.enemies.len(), 3);
    assert_eq!(battle.enemy_index_for_slot(1), Some(2));
    assert_eq!(battle.enemies[2].hp, 1000);

    assert!(state.apply_script_action(ScriptAction::DamageEnemy {
        enemy_index: 1,
        amount: 7,
        apply_to_all: false,
    }));
    assert_eq!(state.battle().unwrap().enemies[1].hp, 0);
    assert_eq!(state.battle().unwrap().enemies[2].hp, 993);
    assert_eq!(state.transform_enemy(1, 1), Some(true));
    assert_eq!(state.battle().unwrap().enemies[2].hp, 993);
}

#[test]
fn scripted_player_magic_animation_uses_zero_based_battle_party_indices() {
    let mut state = battle_item_state(1);
    assert!(!state.apply_script_action(ScriptAction::PlayerMagicAnimation { player: Some(1) }));
    assert!(state.apply_script_action(ScriptAction::PlayerMagicAnimation { player: Some(0) }));
    assert!(state.apply_script_action(ScriptAction::PlayerMagicAnimation { player: None }));
    assert_eq!(
        state.advance_battle_resolution(),
        vec![
            BattleEvent::PlayerMagicAnimation { player: Some(0) },
            BattleEvent::PlayerMagicAnimation { player: None },
        ]
    );
}

#[test]
fn collect_transmute_steal_hide_and_auto_battle_update_game_state() {
    let mut state = battle_item_state(1);
    state.stores =
        Some(Stores::parse(&(10u16..=18).flat_map(u16::to_le_bytes).collect::<Vec<_>>()).unwrap());
    state.battle_mut().unwrap().enemies[0].collect_value = 9;
    state.battle_mut().unwrap().enemies[0].steal_item = 12;
    state.battle_mut().unwrap().enemies[0].steal_item_count = 1;

    assert!(state.apply_script_action(ScriptAction::CollectEnemy {
        enemy_index: 0,
        failure_entry: 80,
    }));
    assert_eq!(state.collect_value(), 9);
    assert!(state.apply_script_action(ScriptAction::TransmuteCollectedEnemies));
    assert!(state.collect_value() < 9);
    assert_eq!(state.inventory().filter(|(item, _)| *item >= 10).count(), 1);

    let stolen_before = state.inventory_count(12);
    assert!(state.apply_script_action(ScriptAction::StealEnemy {
        enemy_index: 0,
        rate: 0,
    }));
    assert_eq!(state.inventory_count(12), stolen_before + 1);
    assert!(state.apply_script_action(ScriptAction::HideBattleActor { rounds: 2 }));
    assert_eq!(state.battle().unwrap().hiding_time(), 2);

    assert!(state.apply_script_action(ScriptAction::EnableAutoBattle));
    assert!(state.auto_battle());
    assert!(state.battle_mut().unwrap().set_script_result(0));
    assert_eq!(
        state.advance_battle_resolution(),
        vec![BattleEvent::Finished(BattleResult::Terminated)]
    );
    assert!(state.settle_battle().is_some());
    assert!(!state.auto_battle());
}

#[test]
fn walking_updates_position_animation_and_camera() {
    let mut state = state(&[]);
    assert!(state.update(GameInput {
        direction: Some(Direction::East),
        ..GameInput::default()
    }));
    assert_eq!((state.player.world_x, state.player.world_y), (336, 248));
    assert_eq!(state.player.anim_frame, 1);
    assert_eq!((state.camera.x, state.camera.y), (176, 148));
}

#[test]
fn blocked_walking_only_changes_facing() {
    let mut state = state(&[(336, 232)]);
    assert!(state.update(GameInput {
        direction: Some(Direction::North),
        ..GameInput::default()
    }));
    assert_eq!((state.player.world_x, state.player.world_y), (320, 240));
    assert_eq!(state.player.direction, Direction::North);
    assert_eq!(state.player.anim_frame, 0);
}

#[test]
fn blocked_walking_resets_an_active_animation() {
    let mut state = state(&[(336, 248)]);
    state.player.anim_frame = 2;
    assert!(state.update(GameInput {
        direction: Some(Direction::East),
        ..GameInput::default()
    }));
    assert_eq!((state.player.world_x, state.player.world_y), (320, 240));
    assert_eq!(state.player.anim_frame, 0);
}

#[test]
fn event_object_blockers_prevent_walking() {
    let mut state = state(&[]).with_scene_objects(vec![blocking_object(336, 248)]);
    assert!(state.update(GameInput {
        direction: Some(Direction::East),
        ..GameInput::default()
    }));
    assert_eq!((state.player.world_x, state.player.world_y), (320, 240));
    assert_eq!(state.player.direction, Direction::East);
}

#[test]
fn script_actions_update_object_trigger_fields() {
    let mut state = state(&[]).with_scene_objects(vec![blocking_object(320, 240)]);
    assert!(
        state.apply_script_action(ScriptAction::SetObjectAutoScript {
            object_id: 1,
            script_entry: 0x135d,
        })
    );
    assert!(
        state.apply_script_action(ScriptAction::SetObjectTriggerScript {
            object_id: 1,
            script_entry: 0x119e,
        })
    );
    assert!(
        state.apply_script_action(ScriptAction::SetObjectTriggerMode {
            object_id: 1,
            trigger_mode: 2,
        })
    );
    assert_eq!(state.scene_objects[0].auto_script, 0x135d);
    assert_eq!(state.scene_objects[0].trigger_script, 0x119e);
    assert_eq!(state.scene_objects[0].trigger_mode, 2);
}

#[test]
fn touch_trigger_is_queued_and_pauses_exploration_until_consumed() {
    let mut object = blocking_object(330, 240);
    object.state = 1;
    object.trigger_mode = 4;
    object.trigger_script = 99;
    let mut state = state(&[]).with_scene_objects(vec![object]);

    assert!(state.update(GameInput {
        direction: Some(Direction::East),
        ..GameInput::default()
    }));
    assert_eq!((state.player.world_x, state.player.world_y), (320, 240));
    assert_eq!(state.pending_trigger.unwrap().script_entry, 99);
    assert!(!state.update(GameInput {
        direction: Some(Direction::East),
        ..GameInput::default()
    }));
    assert_eq!(state.take_trigger().unwrap().object_id, 1);
    assert!(state.pending_trigger.is_none());
}

#[test]
fn confirm_queues_search_trigger_in_front_of_player() {
    let mut object = blocking_object(336, 248);
    object.state = 1;
    object.trigger_mode = 1;
    object.trigger_script = 77;
    let mut state = state(&[]).with_scene_objects(vec![object]);
    state.player.direction = Direction::East;

    assert!(state.update(GameInput {
        confirm: true,
        ..GameInput::default()
    }));
    let trigger = state.take_trigger().unwrap();
    assert_eq!(trigger.object_id, 1);
    assert_eq!(trigger.script_entry, 77);
    assert_eq!(state.scene_objects[0].direction, Direction::West);
}

#[test]
fn script_actions_mutate_world_state_and_follow_player() {
    let mut state = state(&[]).with_scene_objects(vec![blocking_object(320, 200)]);
    assert!(state.apply_script_action(ScriptAction::MoveObject {
        object_id: 1,
        direction: Direction::East,
    }));
    assert_eq!(
        (
            state.scene_objects[0].world_x,
            state.scene_objects[0].world_y
        ),
        (324, 202)
    );
    assert_eq!(state.scene_objects[0].current_frame, 1);

    assert!(state.apply_script_action(ScriptAction::SetObjectState {
        object_id: 1,
        state: -1,
    }));
    assert_eq!(state.scene_objects[0].state, -1);
    assert!(state.apply_script_action(ScriptAction::OffsetPlayer {
        dx: 32,
        dy: 16,
        layer: 0,
    }));
    assert_eq!((state.player.world_x, state.player.world_y), (352, 256));
    assert_eq!((state.camera.x, state.camera.y), (192, 156));
    assert!(!state.apply_script_action(ScriptAction::SetObjectState {
        object_id: 99,
        state: 0,
    }));

    assert!(state.apply_script_action(ScriptAction::AddItem {
        item_id: 99,
        amount: 0,
    }));
    assert_eq!(state.item_count(99), 1);
    assert!(state.apply_script_action(ScriptAction::AddItem {
        item_id: 99,
        amount: -1,
    }));
    assert_eq!(state.item_count(99), 0);
    assert!(state.apply_script_action(ScriptAction::AddItem {
        item_id: 42,
        amount: 3,
    }));
    assert!(state.apply_script_action(ScriptAction::AddItem {
        item_id: 7,
        amount: 2,
    }));
    assert_eq!(state.inventory().collect::<Vec<_>>(), vec![(42, 3), (7, 2)]);
    assert!(state.apply_script_action(ScriptAction::PlayMusic {
        music_id: 6,
        looped: true,
        fade_seconds: 0,
    }));
    assert_eq!(state.current_music, Some(6));
    assert!(state.apply_script_action(ScriptAction::PlayMusic {
        music_id: 0,
        looped: false,
        fade_seconds: 0,
    }));
    assert_eq!(state.current_music, None);
    assert!(state.apply_script_action(ScriptAction::PlaySound { sound_id: 1 }));
    assert!(state.apply_script_action(ScriptAction::AdjustCash {
        amount: 20,
        insufficient_entry: 0,
    }));
    assert!(state.apply_script_action(ScriptAction::AdjustCash {
        amount: -7,
        insufficient_entry: 0,
    }));
    assert_eq!(state.cash, 13);
    assert!(!state.apply_script_action(ScriptAction::AdjustCash {
        amount: -14,
        insufficient_entry: 0,
    }));
    assert_eq!(state.cash, 13);
    assert!(state.apply_script_action(ScriptAction::SetPlayerPosition {
        tile_x: 10,
        tile_y: 12,
        half: 1,
    }));
    assert_eq!((state.player.world_x, state.player.world_y), (336, 200));
}

#[test]
fn party_script_action_replaces_members_and_updates_leader_sprite() {
    let mut role_data = vec![0; 900];
    role_data[28..30].copy_from_slice(&42u16.to_le_bytes());
    role_data[772..774].copy_from_slice(&4u16.to_le_bytes());
    let player_roles = PlayerRoles::parse(&role_data).unwrap();
    let party = Party::single(0, &player_roles).unwrap();
    let mut state = state(&[]).with_party(party).with_player_roles(player_roles);
    state.player_poisons[0][0] = BattlePoison {
        object_id: 7,
        script_entry: 99,
    };

    assert!(state.apply_script_action(ScriptAction::SetParty {
        members: [Some(2), Some(1), None],
    }));
    assert_eq!(
        state
            .party
            .members()
            .iter()
            .map(|member| member.role_id)
            .collect::<Vec<_>>(),
        vec![2, 1]
    );
    assert_eq!(state.player.sprite_index, 42);
    assert_eq!(state.player.frames_per_direction, 4);
    assert_eq!(state.player_poisons(0).unwrap()[0], BattlePoison::default());
    assert_eq!(state.party_followers().len(), 1);
    assert_eq!(
        (
            state.party_followers()[0].world_x,
            state.party_followers()[0].world_y
        ),
        (320, 239)
    );
    assert!(state.apply_script_action(ScriptAction::SetPartyFollowers {
        followers: [Some(3), None],
    }));
    assert_eq!(state.party_followers().len(), 2);
    assert!(!state.apply_script_action(ScriptAction::SetPartyFollowers {
        followers: [Some(2), None],
    }));
    assert!(state.apply_script_action(ScriptAction::SetSceneMap {
        scene_number: None,
        map_number: 17,
    }));
    assert_eq!(state.scene_map_override(state.scene_number), Some(17));
    let snapshot = state
        .decode_snapshot(&state.encode_snapshot().unwrap())
        .unwrap();
    assert_eq!(snapshot.scene_map_override(), Some(17));
    state.restore_snapshot(snapshot, test_map());
    assert_eq!(state.party_followers().len(), 2);

    assert!(state.update(GameInput {
        direction: Some(Direction::East),
        ..GameInput::default()
    }));
    assert_ne!(
        (
            state.party_followers()[0].world_x,
            state.party_followers()[0].world_y
        ),
        (state.player.world_x, state.player.world_y - 1)
    );
    assert!(state.apply_script_action(ScriptAction::CollapseParty));
    assert_eq!(
        (
            state.party_followers()[0].world_x,
            state.party_followers()[0].world_y
        ),
        (state.player.world_x, state.player.world_y - 1)
    );
    assert!(state.apply_script_action(ScriptAction::SetPlayerPosition {
        tile_x: 10,
        tile_y: 12,
        half: 1,
    }));
    assert_eq!(
        (
            state.party_followers()[0].world_x,
            state.party_followers()[0].world_y
        ),
        (320, 192)
    );

    assert!(!state.apply_script_action(ScriptAction::SetParty {
        members: [Some(2), Some(2), None],
    }));
    assert_eq!(state.party.members().len(), 2);
}

#[test]
fn party_setup_preserves_position_and_pose_of_an_inactive_slot() {
    let player_roles = PlayerRoles::parse(&vec![0; 900]).unwrap();
    let party = Party::single(0, &player_roles).unwrap();
    let mut state = state(&[]).with_party(party).with_player_roles(player_roles);
    state.player.direction = Direction::West;

    // Scene 22 uses this same ordering: position every fixed party slot,
    // pose slots 0 and 1, then assign the second role to slot 1.
    assert!(state.apply_script_action(ScriptAction::SetPlayerPosition {
        tile_x: 34,
        tile_y: 86,
        half: 1,
    }));
    assert!(state.apply_script_action(ScriptAction::SetPlayerPose {
        direction: Direction::East,
        frame: 0,
        party_index: 0,
    }));
    assert!(state.apply_script_action(ScriptAction::SetPlayerPose {
        direction: Direction::East,
        frame: 1,
        party_index: 1,
    }));
    assert!(state.apply_script_action(ScriptAction::SetParty {
        members: [Some(0), Some(1), None],
    }));

    assert_eq!((state.player.world_x, state.player.world_y), (1104, 1384));
    let follower = &state.party_followers()[0];
    assert_eq!((follower.world_x, follower.world_y), (1120, 1392));
    assert_eq!(follower.direction, Direction::East);
    assert_eq!(follower.anim_frame, 1);

    let snapshot = state
        .decode_snapshot(&state.encode_snapshot().unwrap())
        .unwrap();
    state.restore_snapshot(snapshot, test_map());
    let follower = &state.party_followers()[0];
    assert_eq!((follower.world_x, follower.world_y), (1120, 1392));
    assert_eq!(follower.direction, Direction::East);
    assert_eq!(follower.anim_frame, 1);
}

#[test]
fn batch_object_state_requires_a_complete_global_range() {
    let mut first = blocking_object(1, 1);
    first.id = 4;
    let mut second = blocking_object(2, 2);
    second.id = 5;
    let mut state = state(&[]).with_scene_objects(vec![first, second]);
    assert!(state.apply_script_action(ScriptAction::SetObjectStates {
        first_object_id: 4,
        last_object_id: 5,
        state: -2,
    }));
    assert_eq!(state.object_state(4), Some(-2));
    assert_eq!(state.object_state(5), Some(-2));
    assert!(!state.apply_script_action(ScriptAction::SetObjectStates {
        first_object_id: 4,
        last_object_id: 6,
        state: 1,
    }));
    assert_eq!(state.object_state(4), Some(-2));
}

#[test]
fn item_removal_counts_and_unequips_active_party_items() {
    let mut role_data = vec![0; 900];
    let role_zero_weapon = 11 * PLAYER_ROLE_COUNT * 2;
    role_data[role_zero_weapon..role_zero_weapon + 2].copy_from_slice(&99u16.to_le_bytes());
    let roles = PlayerRoles::parse(&role_data).unwrap();
    let party = Party::single(0, &roles).unwrap();
    let mut state = state(&[]).with_party(party).with_player_roles(roles);
    assert!(state.apply_script_action(ScriptAction::AddItem {
        item_id: 99,
        amount: 1,
    }));
    assert_eq!(state.inventory_count(99), 1);
    assert_eq!(state.item_count(99), 2);

    assert!(!state.remove_item(99, 3, 77));
    assert_eq!(state.inventory_count(99), 1);
    assert_eq!(state.player_role(0).unwrap().equipment[0], 99);

    assert!(state.remove_item(99, 2, 77));
    assert_eq!(state.inventory_count(99), 0);
    assert_eq!(state.item_count(99), 0);
    assert_eq!(state.player_role(0).unwrap().equipment[0], 0);
    assert_eq!(state.party.members()[0].attributes.equipment[0], 0);
}

#[test]
fn party_health_and_equipped_item_conditions_use_active_mutable_roles() {
    let mut role_data = vec![0; 900];
    let mut set_role_value = |array: usize, role: usize, value: u16| {
        let offset = array * PLAYER_ROLE_COUNT * 2 + role * 2;
        role_data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    };
    for role in 0..3 {
        set_role_value(7, role, 100);
        set_role_value(9, role, if role == 1 { 75 } else { 100 });
        set_role_value(11, role, 274);
    }
    let roles = PlayerRoles::parse(&role_data).unwrap();
    let mut party = Party::single(0, &roles).unwrap();
    assert!(party.add(1, &roles));
    let mut state = state(&[]).with_party(party).with_player_roles(roles);

    assert!(state.party_not_full_hp());
    assert_eq!(state.equipped_item_count(274), 2);
    state.player_roles.as_mut().unwrap().role_mut(1).unwrap().hp = 100;
    assert!(!state.party_not_full_hp());
    state
        .player_roles
        .as_mut()
        .unwrap()
        .role_mut(1)
        .unwrap()
        .equipment[0] = 0;
    assert_eq!(state.equipped_item_count(274), 1);
}

#[test]
fn equipment_scripts_preserve_inventory_slots_and_refresh_effective_stats() {
    let mut roles = PlayerRoles::parse(&vec![0; 900]).unwrap();
    let role = roles.role_mut(0).unwrap();
    role.attack_strength = 10;
    role.hp = 20;
    let party = Party::single(0, &roles).unwrap();

    let mut object_data = vec![0; 24];
    for (index, word) in [
        0u16,
        0,
        0,
        42,
        0,
        ITEM_FLAG_EQUIPPABLE | ITEM_FLAG_ROLE_FIRST,
    ]
    .into_iter()
    .enumerate()
    {
        object_data[12 + index * 2..14 + index * 2].copy_from_slice(&word.to_le_bytes());
    }
    let objects =
        GlobalObjects::parse(&object_data, pal_assets::objects::ObjectLayout::Dos).unwrap();
    let stores = Stores::parse(&[0; 18]).unwrap();
    let mut state = state(&[])
        .with_party(party)
        .with_player_roles(roles)
        .with_economy_data(stores, objects);
    assert!(state.apply_script_action(ScriptAction::AddItem {
        item_id: 1,
        amount: 1,
    }));
    assert_eq!(state.equippable_item(1, 0).unwrap().script_entry, 42);
    assert_eq!(
        state.item_equip_request(1, 0).unwrap().kind,
        crate::scene::TriggerKind::Equip
    );

    assert!(state.apply_script_action(ScriptAction::EquipItem {
        role_id: 0,
        slot: 0,
        item_id: 1,
    }));
    assert!(state.apply_script_action(ScriptAction::SetEquipmentEffect {
        role_id: 0,
        attribute: 17,
        slot: 0,
        value: 5,
    }));
    assert!(
        state.apply_script_action(ScriptAction::ChangePlayerAttribute {
            role_id: 0,
            attribute: 4,
            value: 1,
            absolute: true,
        })
    );
    assert_eq!(state.inventory_count(1), 0);
    assert_eq!(state.player_role(0).unwrap().equipment[0], 1);
    let effective = state.effective_player_role(0).unwrap();
    assert_eq!(effective.attack_strength, 15);
    assert!(effective.attack_all);

    state.finish_item_equip(1, 43);
    assert!(state.apply_script_action(ScriptAction::RemoveEquipment {
        role_id: 0,
        slot: Some(0),
    }));
    assert_eq!(state.inventory().collect::<Vec<_>>(), vec![(1, 1)]);
    assert_eq!(state.effective_player_role(0).unwrap().attack_strength, 10);
}

#[test]
fn battle_equipment_refresh_replays_existing_equipment_without_stacking() {
    let mut roles = PlayerRoles::parse(&vec![0; 900]).unwrap();
    let role = roles.role_mut(0).unwrap();
    role.equipment[0] = 1;
    role.attack_strength = 10;
    let party = Party::single(0, &roles).unwrap();

    let mut object_data = vec![0; 24];
    for (index, word) in [0u16, 0, 0, 1, 0, 0].into_iter().enumerate() {
        object_data[12 + index * 2..14 + index * 2].copy_from_slice(&word.to_le_bytes());
    }
    let objects =
        GlobalObjects::parse(&object_data, pal_assets::objects::ObjectLayout::Dos).unwrap();
    let stores = Stores::parse(&[0; 18]).unwrap();
    let script_data = [
        [ScriptOpcode::Stop.raw(), 0, 0, 0],
        [ScriptOpcode::EquipItem.raw(), 0x0b, 1, 0],
        [ScriptOpcode::SetEquipmentEffect.raw(), 0x0b, 17, 5],
        [ScriptOpcode::Stop.raw(), 0, 0, 0],
        [ScriptOpcode::SetParty.raw(), 1, 0, 0],
        [ScriptOpcode::Stop.raw(), 0, 0, 0],
    ]
    .into_iter()
    .flatten()
    .flat_map(u16::to_le_bytes)
    .collect::<Vec<_>>();
    let scripts = ScriptTable::parse(&script_data).unwrap();
    let mut state = state(&[])
        .with_party(party)
        .with_player_roles(roles)
        .with_economy_data(stores, objects);

    assert!(state.refresh_equipment_effects(&scripts));
    assert_eq!(state.effective_player_role(0).unwrap().attack_strength, 15);
    assert!(state.refresh_equipment_effects(&scripts));
    assert_eq!(state.effective_player_role(0).unwrap().attack_strength, 15);
    assert_eq!(state.player_role(0).unwrap().equipment[0], 1);
    assert_eq!(state.inventory_count(1), 0);

    state.equipment_effects.insert((0, 0, 17), 99);
    state.player_poisons[0][0] = BattlePoison {
        object_id: 7,
        script_entry: 99,
    };
    let mut object = blocking_object(80, 80);
    object.auto_script = 4;
    state.scene_objects.push(object);
    assert!(state.update_auto_scripts(&scripts).unwrap());
    assert_eq!(state.effective_player_role(0).unwrap().attack_strength, 15);
    assert_eq!(state.player_poisons(0).unwrap()[0], BattlePoison::default());

    let invalid_script_data = [
        [ScriptOpcode::Stop.raw(), 0, 0, 0],
        [ScriptOpcode::EquipItem.raw(), 0x0b, 1, 0],
        [ScriptOpcode::AddItem.raw(), 2, 1, 0],
        [ScriptOpcode::Stop.raw(), 0, 0, 0],
    ]
    .into_iter()
    .flatten()
    .flat_map(u16::to_le_bytes)
    .collect::<Vec<_>>();
    let invalid_scripts = ScriptTable::parse(&invalid_script_data).unwrap();
    assert!(!state.refresh_equipment_effects(&invalid_scripts));
    assert_eq!(state.effective_player_role(0).unwrap().attack_strength, 15);
    assert_eq!(state.player_role(0).unwrap().equipment[0], 1);
    assert_eq!(state.inventory_count(1), 0);
}

#[test]
fn field_magic_uses_object_scripts_and_consumes_mp_after_success() {
    let mut roles = PlayerRoles::parse(&vec![0; 900]).unwrap();
    let role = roles.role_mut(0).unwrap();
    role.hp = 20;
    role.mp = 10;
    role.magic[0] = 1;
    let party = Party::single(0, &roles).unwrap();

    let mut object_data = vec![0; 24];
    for (index, word) in [0u16, 0, 44, 43, 0, MAGIC_FLAG_USABLE_OUTSIDE_BATTLE]
        .into_iter()
        .enumerate()
    {
        object_data[12 + index * 2..14 + index * 2].copy_from_slice(&word.to_le_bytes());
    }
    let objects =
        GlobalObjects::parse(&object_data, pal_assets::objects::ObjectLayout::Dos).unwrap();
    let stores = Stores::parse(&[0; 18]).unwrap();
    let mut magic_data = vec![0; 32];
    magic_data[24..26].copy_from_slice(&3u16.to_le_bytes());
    let magics = Magics::parse(&magic_data).unwrap();
    let mut state = state(&[])
        .with_party(party)
        .with_player_roles(roles)
        .with_economy_data(stores, objects)
        .with_magic_data(magics);

    let magic = state.field_magics(0)[0];
    assert_eq!((magic.magic_id, magic.mp_cost), (1, 3));
    assert!(magic.enabled);
    assert_eq!(
        state
            .magic_request(0, 1, Some(0), false)
            .unwrap()
            .script_entry,
        43
    );
    assert_eq!(
        state
            .magic_request(0, 1, Some(0), true)
            .unwrap()
            .script_entry,
        44
    );
    let (request, success_phase) = state.initial_magic_request(0, 1, Some(0)).unwrap();
    assert_eq!(request.script_entry, 43);
    assert!(!success_phase);
    state.finish_magic_script(1, 0, false);
    let (request, success_phase) = state.initial_magic_request(0, 1, Some(0)).unwrap();
    assert_eq!(request.script_entry, 44);
    assert!(success_phase);
    state.finish_magic_script(1, 45, false);
    assert!(state.consume_magic_mp(0, 1));
    assert_eq!(state.player_role(0).unwrap().mp, 7);
    assert!(state.apply_script_action(ScriptAction::ChangeMagic {
        role_id: 0,
        magic_id: 1,
        add: false,
    }));
    assert!(state.field_magics(0).is_empty());
}

#[test]
fn item_use_validates_targets_applies_recovery_and_consumes_on_success() {
    let mut role_data = vec![0; 900];
    let mut set_role_value = |array: usize, role: usize, value: u16| {
        let offset = array * PLAYER_ROLE_COUNT * 2 + role * 2;
        role_data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    };
    for role in 0..2 {
        set_role_value(7, role, 100);
        set_role_value(8, role, 80);
        set_role_value(9, role, if role == 0 { 40 } else { 0 });
        set_role_value(10, role, 20);
    }
    let roles = PlayerRoles::parse(&role_data).unwrap();
    let mut party = Party::single(0, &roles).unwrap();
    assert!(party.add(1, &roles));
    let stores = Stores::parse(&[0; 18]).unwrap();
    let objects = GlobalObjects::parse(
        &[
            [0u16; 6],
            [0, 0, 123, 0, 0, ITEM_FLAG_USABLE | ITEM_FLAG_CONSUMING],
            [
                0,
                0,
                200,
                0,
                0,
                ITEM_FLAG_USABLE | ITEM_FLAG_CONSUMING | ITEM_FLAG_APPLY_TO_ALL,
            ],
        ]
        .into_iter()
        .flatten()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>(),
        pal_assets::objects::ObjectLayout::Dos,
    )
    .unwrap();
    let mut state = state(&[])
        .with_party(party)
        .with_player_roles(roles)
        .with_economy_data(stores, objects);
    for item_id in [1, 2] {
        assert!(state.apply_script_action(ScriptAction::AddItem { item_id, amount: 1 }));
    }

    let request = state.item_use_request(1, Some(0)).unwrap();
    assert_eq!(request.object_id, 0);
    assert_eq!(request.script_entry, 123);
    assert_eq!(request.kind, crate::scene::TriggerKind::Item);
    assert!(state.item_use_request(1, Some(5)).is_none());
    assert!(state.item_use_request(1, None).is_none());
    assert!(state.item_use_request(2, Some(0)).is_none());
    assert_eq!(state.item_use_request(2, None).unwrap().object_id, 0xffff);

    assert!(state.apply_script_action(ScriptAction::AdjustPlayerHealth {
        role_id: 0,
        hp: 50,
        mp: 30,
        apply_to_all: false,
    }));
    assert_eq!(state.player_role(0).unwrap().hp, 90);
    assert_eq!(state.player_role(0).unwrap().mp, 50);
    assert!(state.finish_item_use(1, 321, false));
    assert_eq!(state.inventory_count(1), 1);
    assert_eq!(
        state.item_use_request(1, Some(0)).unwrap().script_entry,
        321
    );
    let saved = state
        .decode_snapshot(&state.encode_snapshot().unwrap())
        .unwrap();
    state.restore_snapshot(saved, test_map());
    assert_eq!(
        state.item_use_request(1, Some(0)).unwrap().script_entry,
        321
    );

    assert!(state.apply_script_action(ScriptAction::RevivePlayer {
        role_id: 0xffff,
        hp_tenths: 3,
        apply_to_all: true,
    }));
    assert_eq!(state.player_role(1).unwrap().hp, 30);
    assert_eq!(state.party.members()[1].attributes.hp, 30);

    assert!(state.finish_item_use(1, 321, true));
    assert_eq!(state.inventory_count(1), 0);
    assert!(state.item_use_request(1, Some(0)).is_none());
}

#[test]
fn recovery_rejects_dead_or_full_targets_without_mutating_them() {
    let mut role_data = vec![0; 900];
    for (array, value) in [(7, 100u16), (8, 80), (9, 100), (10, 80)] {
        let offset = array * PLAYER_ROLE_COUNT * 2;
        role_data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    let roles = PlayerRoles::parse(&role_data).unwrap();
    let party = Party::single(0, &roles).unwrap();
    let mut state = state(&[]).with_party(party).with_player_roles(roles);
    assert!(
        !state.apply_script_action(ScriptAction::AdjustPlayerHealth {
            role_id: 0,
            hp: 50,
            mp: 50,
            apply_to_all: false,
        })
    );
    assert!(!state.apply_script_action(ScriptAction::RevivePlayer {
        role_id: 0,
        hp_tenths: 5,
        apply_to_all: false,
    }));
    state.player_roles.as_mut().unwrap().role_mut(0).unwrap().hp = 0;
    assert!(
        !state.apply_script_action(ScriptAction::AdjustPlayerHealth {
            role_id: 0,
            hp: 50,
            mp: 0,
            apply_to_all: false,
        })
    );
    assert_eq!(state.player_role(0).unwrap().hp, 0);
}

#[test]
fn store_transactions_use_prices_flags_and_inventory() {
    let stores = Stores::parse(
        &[2u16, 0, 0, 0, 0, 0, 0, 0, 0]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let objects = GlobalObjects::parse(
        &[[0u16; 6], [0u16; 6], [0, 100, 0, 0, 0, ITEM_FLAG_SELLABLE]]
            .into_iter()
            .flatten()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
        pal_assets::objects::ObjectLayout::Dos,
    )
    .unwrap();
    let mut state = state(&[]).with_economy_data(stores, objects);
    state.cash = 120;
    assert_eq!(
        state.store_items(0).unwrap(),
        vec![StoreItem {
            item_id: 2,
            price: 100,
        }]
    );
    assert!(state.buy_item(2));
    assert_eq!(state.cash, 20);
    assert_eq!(state.inventory_count(2), 1);
    assert!(!state.buy_item(2));
    assert!(state.sell_item(2));
    assert_eq!(state.cash, 70);
    assert_eq!(state.inventory_count(2), 0);
}

#[test]
fn player_facing_condition_checks_current_scene_range_and_state() {
    let mut object = blocking_object(304, 248);
    object.state = 1;
    let mut state = state(&[]).with_scene_objects(vec![object]);
    state.player.direction = Direction::South;
    assert!(state.player_faces_object(1, 0));
    state.player.direction = Direction::East;
    assert!(!state.player_faces_object(1, 0));

    state.scene_objects[0].world_x = 320;
    state.scene_objects[0].world_y = 240;
    assert!(state.player_faces_object(1, 1));
    assert_eq!(state.scene_objects[0].trigger_mode, 6);
    state.scene_objects[0].state = 0;
    assert!(!state.player_faces_object(1, 1));
    assert!(!state.player_faces_object(99, 1));
}

#[test]
fn item_object_placement_requires_current_scene_and_clear_space() {
    let mut item_object = blocking_object(0, 0);
    item_object.id = 7;
    item_object.state = 0;
    let mut game = state(&[]).with_scene_objects(vec![item_object.clone()]);

    assert!(game.place_object_in_front(7, 2));
    assert_eq!(
        (game.scene_objects[0].world_x, game.scene_objects[0].world_y),
        (304, 248)
    );
    assert_eq!(game.object_state(7), Some(2));
    assert!(!game.place_object_in_front(99, 2));

    let mut blocked_by_map = state(&[(304, 248)]).with_scene_objects(vec![item_object.clone()]);
    assert!(!blocked_by_map.place_object_in_front(7, 2));
    assert_eq!(blocked_by_map.object_state(7), Some(0));

    let blocker = blocking_object(304, 248);
    let mut blocked_by_object = state(&[]).with_scene_objects(vec![item_object, blocker]);
    assert!(!blocked_by_object.place_object_in_front(7, 2));
    assert_eq!(blocked_by_object.object_state(7), Some(0));
}

#[test]
fn inactive_global_objects_can_be_modified_before_scene_load() {
    let mut current = blocking_object(1, 1);
    current.id = 1;
    let mut second = blocking_object(2, 2);
    second.id = 2;
    let mut third = blocking_object(3, 3);
    third.id = 3;
    let mut state = state(&[])
        .with_scene_objects(vec![current.clone()])
        .with_global_objects(vec![current, second.clone(), third]);

    assert!(state.apply_script_action(ScriptAction::SetObjectState {
        object_id: 2,
        state: -1,
    }));
    assert!(
        state.apply_script_action(ScriptAction::SetObjectTriggerScript {
            object_id: 3,
            script_entry: 77,
        })
    );
    assert!(state.apply_script_action(ScriptAction::SetObjectStates {
        first_object_id: 1,
        last_object_id: 3,
        state: -2,
    }));
    assert_eq!(state.object_state(1), Some(-2));
    assert_eq!(state.object_state(2), Some(-2));
    assert_eq!(state.object_state(3), Some(-2));

    state.replace_scene(2, test_map(), vec![second]);
    assert_eq!(state.scene_objects[0].state, -2);
    let mut fresh_third = blocking_object(3, 3);
    fresh_third.id = 3;
    state.replace_scene(3, test_map(), vec![fresh_third]);
    assert_eq!(state.scene_objects[0].trigger_script, 77);
}

#[test]
fn scene_object_state_survives_leaving_and_reentering() {
    let mut first = blocking_object(320, 200);
    first.trigger_script = 10;
    let mut state = state(&[])
        .with_scene_number(1)
        .with_scene_objects(vec![first]);
    assert!(state.apply_script_action(ScriptAction::SetObjectPosition {
        object_id: 1,
        x: 444,
        y: 222,
    }));
    assert!(state.apply_script_action(ScriptAction::SetObjectState {
        object_id: 1,
        state: -1,
    }));
    state.scene_objects[0].trigger_script = 11;
    state.scene_objects[0].current_frame = 3;

    let mut second = blocking_object(50, 60);
    second.id = 2;
    state.replace_scene(2, test_map(), vec![second]);
    state.scene_objects[0].world_x = 77;
    assert_eq!(state.inactive_objects.len(), 1);
    assert!(
        state.apply_script_action(ScriptAction::SetObjectTriggerScript {
            object_id: 1,
            script_entry: 12,
        })
    );
    assert_eq!(state.object_state(1), Some(-1));

    let mut fresh_first = blocking_object(1, 2);
    fresh_first.trigger_script = 10;
    state.replace_scene(1, test_map(), vec![fresh_first]);
    let restored = &state.scene_objects[0];
    assert_eq!((restored.world_x, restored.world_y), (444, 222));
    assert_eq!(restored.state, -1);
    assert_eq!(restored.trigger_script, 12);
    assert_eq!(restored.current_frame, 3);

    let mut fresh_second = blocking_object(5, 6);
    fresh_second.id = 2;
    state.replace_scene(2, test_map(), vec![fresh_second]);
    assert_eq!(state.scene_objects[0].world_x, 77);
}

#[test]
fn snapshot_restores_global_and_scene_state() {
    let mut state = state(&[])
        .with_scene_number(4)
        .with_scene_objects(vec![blocking_object(320, 200)]);
    assert!(state.apply_script_action(ScriptAction::AddItem {
        item_id: 8,
        amount: 2,
    }));
    state.player.world_x = 500;
    state.scene_objects[0].state = -1;
    let snapshot = state.snapshot();

    state.player.world_x = 12;
    state.scene_objects[0].state = 2;
    assert!(state.apply_script_action(ScriptAction::AddItem {
        item_id: 8,
        amount: -2,
    }));
    state.restore_snapshot(snapshot, test_map());

    assert_eq!(state.scene_number, 4);
    assert_eq!(state.player.world_x, 500);
    assert_eq!(state.scene_objects[0].state, -1);
    assert_eq!(state.item_count(8), 2);
    assert_eq!((state.camera.x, state.camera.y), (340, 140));
}

#[test]
fn scene_enter_script_entries_persist_per_scene_and_in_snapshots() {
    let mut state = state(&[]).with_scene_number(4);
    assert_eq!(state.scene_enter_script(100), 100);
    state.update_scene_enter_script(101);
    assert_eq!(state.scene_enter_script(999), 101);

    state.update_scene_enter_script_for(5, 201);

    state.replace_scene(5, test_map(), Vec::new());
    assert_eq!(state.scene_enter_script(200), 201);
    let snapshot = state.snapshot();

    state.update_scene_enter_script(202);
    state.replace_scene(4, test_map(), Vec::new());
    assert_eq!(state.scene_enter_script(100), 101);

    state.restore_snapshot(snapshot, test_map());
    assert_eq!(state.scene_number, 5);
    assert_eq!(state.scene_enter_script(200), 201);
}

#[test]
fn player_statuses_and_poisons_round_trip_and_follow_cure_rules() {
    let mut role_data = vec![0; 900];
    let hp_offset = 9 * PLAYER_ROLE_COUNT * 2;
    role_data[hp_offset..hp_offset + 2].copy_from_slice(&100u16.to_le_bytes());
    let roles = PlayerRoles::parse(&role_data).unwrap();
    let party = Party::single(0, &roles).unwrap();
    let objects = GlobalObjects::parse(
        &[[0u16; 6], [2, 4, 77, 0, 88, 0]]
            .into_iter()
            .flatten()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
        pal_assets::objects::ObjectLayout::Dos,
    )
    .unwrap();
    let stores = Stores::parse(&[0; 18]).unwrap();
    let mut state = state(&[])
        .with_party(party)
        .with_player_roles(roles)
        .with_economy_data(stores, objects);

    assert!(state.apply_script_action(ScriptAction::SetPlayerStatus {
        role_id: 0,
        status: BattleStatus::Protect as u16,
        rounds: 5,
    }));
    assert!(!state.apply_script_action(ScriptAction::SetPlayerStatus {
        role_id: 0,
        status: BattleStatus::Puppet as u16,
        rounds: 3,
    }));
    assert!(state.apply_script_action(ScriptAction::PoisonPlayer {
        role_id: 0,
        poison_id: 1,
        apply_to_all: false,
    }));
    assert_eq!(
        state.player_status_duration(0, BattleStatus::Protect),
        Some(5)
    );
    assert_eq!(state.player_poisons(0).unwrap()[0].object_id, 1);
    assert_eq!(state.player_poisons(0).unwrap()[0].script_entry, 77);

    let snapshot = state
        .decode_snapshot(&state.encode_snapshot().unwrap())
        .unwrap();
    state.remove_player_status(0, BattleStatus::Protect as u16);
    state.cure_player_poison(0, 1, false);
    state.restore_snapshot(snapshot, test_map());
    assert_eq!(
        state.player_status_duration(0, BattleStatus::Protect),
        Some(5)
    );
    assert_eq!(state.player_poisons(0).unwrap()[0].object_id, 1);
    assert!(
        state.apply_script_action(ScriptAction::CurePlayerPoisonByLevel {
            role_id: 0,
            maximum_level: 2,
            apply_to_all: false,
        })
    );
    assert_eq!(state.player_poisons(0).unwrap()[0], BattlePoison::default());
}

#[test]
fn disk_snapshot_round_trips_and_rejects_invalid_data() {
    let player_roles = PlayerRoles::parse(&vec![0; 900]).unwrap();
    let mut party = Party::single(0, &player_roles).unwrap();
    assert!(party.add(1, &player_roles));
    let mut state = state(&[])
        .with_scene_number(3)
        .with_scene_objects(vec![blocking_object(40, 50)])
        .with_party(party)
        .with_player_roles(player_roles);
    assert!(state.apply_script_action(ScriptAction::AddItem {
        item_id: 9,
        amount: 4,
    }));
    assert!(state.apply_script_action(ScriptAction::AddItem {
        item_id: 7,
        amount: 2,
    }));
    assert!(state.adjust_cash(123));
    state.update_scene_enter_script(321);
    state.player_roles.as_mut().unwrap().role_mut(0).unwrap().hp = 321;
    assert!(state.apply_script_action(ScriptAction::SetEquipmentEffect {
        role_id: 0,
        attribute: 17,
        slot: 0,
        value: 7,
    }));
    state.finish_item_equip(9, 44);
    state.item_throw_scripts.insert(9, 77);
    state.finish_magic_script(88, 55, false);
    state.finish_magic_script(88, 66, true);
    assert!(state.apply_script_action(ScriptAction::SetBattleMusic { music_id: 7 }));
    assert!(state.apply_script_action(ScriptAction::SetBattlefield { battlefield_id: 21 }));
    assert!(state.apply_script_action(ScriptAction::SetEnemyChase {
        range: 3,
        cycles: 12,
    }));
    state.role_experience[0] = 42;

    let encoded = state.encode_snapshot().unwrap();
    let decoded = state.decode_snapshot(&encoded).unwrap();
    assert_eq!(decoded.scene_number(), 3);
    state.restore_snapshot(decoded, test_map());
    assert_eq!(state.party_followers().len(), 1);
    assert_eq!(state.item_count(9), 4);
    assert_eq!(state.inventory().collect::<Vec<_>>(), vec![(9, 4), (7, 2)]);
    assert_eq!(state.cash, 123);
    assert_eq!(state.current_battle_music, 7);
    assert_eq!(state.current_battlefield, 21);
    assert_eq!(state.player_experience(0), Some(42));
    assert_eq!(state.player_role(0).unwrap().hp, 321);
    assert_eq!(state.effective_player_role(0).unwrap().attack_strength, 7);
    assert_eq!(state.scene_enter_script(0), 321);
    assert_eq!(state.item_throw_scripts.get(&9), Some(&77));
    assert_eq!(state.chase_range, 3);
    assert_eq!(state.chase_speed_change_cycles, 12);
    assert_eq!(
        (
            state.scene_objects[0].world_x,
            state.scene_objects[0].world_y
        ),
        (40, 50)
    );

    assert!(state.decode_snapshot(b"not json").is_none());
    assert!(state
        .decode_snapshot(&vec![b' '; 4 * 1024 * 1024 + 1])
        .is_none());
    let wrong_version = String::from_utf8(encoded)
        .unwrap()
        .replace(&format!("\"version\":{SNAPSHOT_VERSION}"), "\"version\":0");
    assert!(state.decode_snapshot(wrong_version.as_bytes()).is_none());
}

#[test]
fn auto_script_moves_object_and_enables_scene_exit() {
    let script_data = [
        [0x0000, 0, 0, 0],
        [0x0010, 4, 6, 0],
        [0x0049, 4, 1, 0],
        [0x0049, 0xffff, 0, 0],
        [0x0000, 0, 0, 0],
    ]
    .into_iter()
    .flat_map(|entry| entry.into_iter().flat_map(u16::to_le_bytes))
    .collect::<Vec<_>>();
    let scripts = ScriptTable::parse(&script_data).unwrap();
    let mut door = blocking_object(128, 96);
    door.id = 4;
    door.state = 0;
    let mut mover = blocking_object(100, 100);
    mover.id = 11;
    mover.auto_script = 1;
    let mut state = state(&[]).with_scene_objects(vec![door, mover]);

    for _ in 0..3 {
        assert!(state.update_auto_scripts(&scripts).unwrap());
    }
    assert_eq!(state.object_state(4), Some(1));
    assert_eq!(state.object_state(11), Some(0));
    assert_eq!(
        (
            state.scene_objects[1].world_x,
            state.scene_objects[1].world_y
        ),
        (128, 96)
    );
}

#[test]
fn chase_overrides_pause_and_expand_monster_detection_until_expiry() {
    let scripts = ScriptTable::parse(
        &[
            [0u16, 0, 0, 0],
            [ScriptOpcode::ChasePlayer.raw(), 1, 4, 1],
            [0, 0, 0, 0],
        ]
        .into_iter()
        .flatten()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>(),
    )
    .unwrap();
    let mut monster = blocking_object(256, 240);
    monster.auto_script = 1;
    let mut state = state(&[]).with_scene_objects(vec![monster]);

    assert!(state.update_auto_scripts(&scripts).unwrap());
    assert_eq!(state.scene_objects[0].world_x, 256);

    state.scene_objects[0].auto_script = 1;
    assert!(state.apply_script_action(ScriptAction::SetEnemyChase {
        range: 3,
        cycles: 1,
    }));
    assert!(state.update_auto_scripts(&scripts).unwrap());
    assert!(state.scene_objects[0].world_x > 256);
    assert_eq!(state.chase_range, 1);
    assert_eq!(state.chase_speed_change_cycles, 0);

    let paused_x = state.scene_objects[0].world_x;
    state.scene_objects[0].auto_script = 1;
    assert!(state.apply_script_action(ScriptAction::SetEnemyChase {
        range: 0,
        cycles: 2,
    }));
    assert!(state.update_auto_scripts(&scripts).unwrap());
    assert_eq!(state.scene_objects[0].world_x, paused_x);
    assert_eq!(state.chase_speed_change_cycles, 1);
    state.scene_objects[0].auto_script = 1;
    assert!(state.update_auto_scripts(&scripts).unwrap());
    assert_eq!(state.scene_objects[0].world_x, paused_x);
    assert_eq!(state.chase_range, 1);
}

#[test]
fn trigger_walk_moves_one_step_until_reaching_the_target() {
    let mut object = blocking_object(80, 80);
    object.id = 7;
    let mut state = state(&[]).with_scene_objects(vec![object]);

    assert_eq!(state.walk_object_to(7, 4, 6, 0, 3), Some(false));
    assert_eq!(
        (
            state.scene_objects[0].world_x,
            state.scene_objects[0].world_y
        ),
        (86, 83)
    );
    while state.walk_object_to(7, 4, 6, 0, 3) == Some(false) {}
    assert_eq!(
        (
            state.scene_objects[0].world_x,
            state.scene_objects[0].world_y
        ),
        (128, 96)
    );
    assert_eq!(state.scene_objects[0].current_frame, 0);
}

#[test]
fn party_walk_moves_over_multiple_ticks_and_updates_camera() {
    let mut state = state(&[]);
    assert_eq!(state.walk_player_to(12, 16, 0, 2), Some(false));
    assert_eq!((state.player.world_x, state.player.world_y), (324, 242));
    assert_eq!(state.player.direction, Direction::East);
    assert_eq!((state.camera.x, state.camera.y), (164, 142));

    while state.walk_player_to(12, 16, 0, 2) == Some(false) {}
    assert_eq!((state.player.world_x, state.player.world_y), (384, 256));
    assert_eq!(state.player.anim_frame, 0);
}

#[test]
fn scripted_party_offset_advances_walking_animation_only_when_moving() {
    let mut state = state(&[]);

    assert!(state.apply_script_action(ScriptAction::OffsetPlayer {
        dx: 8,
        dy: 4,
        layer: 24,
    }));
    assert_eq!(state.player.anim_frame, 1);
    assert_eq!(state.party_layer(), 24);

    assert!(state.apply_script_action(ScriptAction::OffsetPlayer {
        dx: 0,
        dy: 0,
        layer: 8,
    }));
    assert_eq!(state.player.anim_frame, 1);
    assert_eq!(state.party_layer(), 8);

    assert!(state.apply_script_action(ScriptAction::OffsetPlayer {
        dx: 8,
        dy: 4,
        layer: 0,
    }));
    assert_eq!(state.player.anim_frame, 2);
}

#[test]
fn party_ride_moves_actors_without_advancing_animation() {
    let mut state = state(&[]).with_scene_objects(vec![blocking_object(300, 220)]);
    state.player.anim_frame = 2;
    let mut follower = state.player.clone();
    follower.world_x = 288;
    follower.world_y = 224;
    follower.anim_frame = 3;
    state.party_followers.push(follower);
    state.scene_objects[0].current_frame = 1;

    assert_eq!(state.ride_object_to(1, 12, 16, 0, 2), Some(false));
    assert_eq!((state.player.world_x, state.player.world_y), (324, 242));
    assert_eq!(state.player.anim_frame, 2);
    assert_eq!(
        (
            state.party_followers[0].world_x,
            state.party_followers[0].world_y,
            state.party_followers[0].anim_frame,
        ),
        (292, 226, 3)
    );
    assert_eq!(
        (
            state.scene_objects[0].world_x,
            state.scene_objects[0].world_y,
            state.scene_objects[0].current_frame,
        ),
        (304, 222, 1)
    );

    while state.ride_object_to(1, 12, 16, 0, 2) == Some(false) {}
    assert_eq!(state.player.anim_frame, 2);
    assert_eq!(state.party_followers[0].anim_frame, 3);
    assert_eq!(state.scene_objects[0].current_frame, 1);
}

#[test]
fn scripted_viewport_stays_locked_until_restored() {
    let mut state = state(&[]);
    assert!(state.apply_script_action(ScriptAction::MoveViewport {
        x: 2,
        y: 3,
        frames: 1,
    }));
    let scripted_camera = (state.camera.x, state.camera.y);
    assert_eq!(scripted_camera, (162, 143));
    assert!(state.apply_script_action(ScriptAction::OffsetPlayer {
        dx: 32,
        dy: 16,
        layer: 0,
    }));
    assert_eq!((state.camera.x, state.camera.y), scripted_camera);

    assert!(state.apply_script_action(ScriptAction::MoveViewport {
        x: 0,
        y: 0,
        frames: 1,
    }));
    assert_eq!((state.camera.x, state.camera.y), (192, 156));
}

#[test]
fn direct_viewport_position_matches_pal_tile_coordinates() {
    let mut state = state(&[]);
    assert!(state.apply_script_action(ScriptAction::MoveViewport {
        x: 1,
        y: 1,
        frames: -1,
    }));
    assert_eq!((state.camera.x, state.camera.y), (-128, -96));
}

#[test]
fn object_transform_actions_preserve_animation_and_toggle_visibility() {
    let mut object = blocking_object(80, 80);
    object.id = 7;
    object.state = 2;
    object.current_frame = 3;
    let mut state = state(&[]).with_scene_objects(vec![object]);

    assert!(state.apply_script_action(ScriptAction::MoveObjectBy {
        object_id: 7,
        dx: -4,
        dy: 2,
    }));
    assert!(state.apply_script_action(ScriptAction::SetObjectLayer {
        object_id: 7,
        layer: -10,
    }));
    assert!(
        state.apply_script_action(ScriptAction::HideObjectTemporarily {
            object_id: 7,
            vanish_time: 150,
        })
    );
    let object = &state.scene_objects[0];
    assert_eq!((object.world_x, object.world_y), (76, 82));
    assert_eq!(object.current_frame, 3);
    assert_eq!(object.layer, -10);
    assert_eq!(object.state, -2);
    assert_eq!(object.vanish_time, 150);
}

#[test]
fn hidden_object_returns_after_its_timer_expires_offscreen() {
    let mut object = blocking_object(700, 700);
    object.state = -2;
    object.vanish_time = 1;
    object.current_frame = 3;
    let mut state = state(&[]).with_scene_objects(vec![object]);

    assert!(state.update(GameInput::default()));
    assert_eq!(state.scene_objects[0].vanish_time, 0);
    assert_eq!(state.scene_objects[0].state, -2);
    assert!(state.update(GameInput::default()));
    assert_eq!(state.scene_objects[0].state, 2);
    assert_eq!(state.scene_objects[0].current_frame, 0);
}

#[test]
fn non_leader_pose_only_changes_the_party_direction() {
    let mut state = state(&[]);
    state.player.anim_frame = 3;
    assert!(state.apply_script_action(ScriptAction::SetPlayerPose {
        direction: Direction::West,
        frame: 1,
        party_index: 2,
    }));
    assert_eq!(state.player.direction, Direction::West);
    assert_eq!(state.player.anim_frame, 3);

    assert!(state.apply_script_action(ScriptAction::SetPlayerPose {
        direction: Direction::South,
        frame: 1,
        party_index: 0,
    }));
    assert_eq!(state.player.anim_frame, 1);
}

#[test]
fn player_sprite_updates_roles_and_only_reloads_active_sprites_when_requested() {
    let roles = PlayerRoles::parse(&vec![0; 900]).unwrap();
    let mut party = Party::single(0, &roles).unwrap();
    assert!(party.add(1, &roles));
    let mut state = state(&[]).with_party(party).with_player_roles(roles);

    assert!(state.apply_script_action(ScriptAction::SetPlayerSprite {
        role_id: 1,
        sprite_index: 42,
        reload: true,
    }));
    assert_eq!(state.player_role(1).unwrap().scene_sprite_num, 42);
    assert_eq!(state.party_followers()[0].sprite_index, 42);

    assert!(state.apply_script_action(ScriptAction::SetPlayerSprite {
        role_id: 0,
        sprite_index: 33,
        reload: false,
    }));
    assert_eq!(state.player_role(0).unwrap().scene_sprite_num, 33);
    assert_eq!(state.player.sprite_index, 0);
    assert!(state.apply_script_action(ScriptAction::SetPlayerSprite {
        role_id: 0,
        sprite_index: 44,
        reload: true,
    }));
    assert_eq!(state.player.sprite_index, 44);
}

#[test]
fn object_zone_checks_require_both_objects_in_the_current_scene() {
    let mut owner = blocking_object(100, 100);
    owner.id = 1;
    let mut target = blocking_object(130, 100);
    target.id = 2;
    let mut state = state(&[]).with_scene_objects(vec![owner, target]);
    assert!(state.objects_within_zone(1, 2, 1));
    state.scene_objects[1].world_x = 148;
    assert!(!state.objects_within_zone(1, 2, 1));
    assert!(!state.objects_within_zone(1, 99, 1));
    assert!(state.apply_script_action(ScriptAction::CheckObjectZone {
        object_id: 1,
        target_id: 2,
        range: 2,
        failure_entry: 77,
    }));
}

#[test]
fn auto_scripts_animate_and_queue_sound_effects() {
    let script_data = [
        [0x0000, 0, 0, 0],
        [0x0087, 0, 0, 0],
        [0x0047, 48, 0, 0],
        [0x0000, 0, 0, 0],
    ]
    .into_iter()
    .flat_map(|entry| entry.into_iter().flat_map(u16::to_le_bytes))
    .collect::<Vec<_>>();
    let scripts = ScriptTable::parse(&script_data).unwrap();
    let mut object = blocking_object(100, 100);
    object.auto_script = 1;
    let mut state = state(&[]).with_scene_objects(vec![object]);

    assert!(state.update_auto_scripts(&scripts).unwrap());
    assert_eq!(state.scene_objects[0].current_frame, 1);
    assert!(state.update_auto_scripts(&scripts).unwrap());
    assert_eq!(state.take_auto_script_sounds(), vec![48]);
    assert!(state.take_auto_script_sounds().is_empty());
}

#[test]
fn auto_script_sets_selected_object_pose() {
    let script_data = [
        [0x0000, 0, 0, 0],
        [0x0016, 417, Direction::East as u16, 1],
        [0x0000, 0, 0, 0],
    ]
    .into_iter()
    .flat_map(|entry| entry.into_iter().flat_map(u16::to_le_bytes))
    .collect::<Vec<_>>();
    let scripts = ScriptTable::parse(&script_data).unwrap();
    let mut owner = blocking_object(80, 80);
    owner.id = 412;
    owner.auto_script = 1;
    let mut target = blocking_object(100, 100);
    target.id = 417;
    target.direction = Direction::North;
    let mut state = state(&[]).with_scene_objects(vec![owner, target]);

    assert!(state.update_auto_scripts(&scripts).unwrap());
    assert_eq!(state.scene_objects[0].auto_script, 2);
    assert_eq!(state.scene_objects[1].direction, Direction::East);
    assert_eq!(state.scene_objects[1].current_frame, 1);
}

#[test]
fn auto_script_replaces_another_objects_auto_script() {
    let script_data = [
        [0x0000, 0, 0, 0],
        [0x0009, 16, 0, 0],
        [0x0024, 422, 3, 0],
        [0x0000, 0, 0, 0],
    ]
    .into_iter()
    .flat_map(|entry| entry.into_iter().flat_map(u16::to_le_bytes))
    .collect::<Vec<_>>();
    let scripts = ScriptTable::parse(&script_data).unwrap();
    let mut ship = blocking_object(1216, 1472);
    ship.id = 422;
    ship.auto_script = 1;
    ship.auto_script_idle_frame = 15;
    let mut npc = blocking_object(1152, 1408);
    npc.id = 423;
    npc.auto_script = 2;
    let mut state = state(&[]).with_scene_objects(vec![ship, npc]);

    assert!(state.update_auto_scripts(&scripts).unwrap());
    assert_eq!(state.scene_objects[0].auto_script, 3);
    assert_eq!(state.scene_objects[0].auto_script_idle_frame, 0);
    assert_eq!(state.scene_objects[1].auto_script, 3);
}

#[test]
fn auto_script_zero_object_selector_is_a_no_op() {
    let script_data = [
        [0x0000, 0, 0, 0],
        [0x0024, 0, 7, 0],
        [0x0025, 0, 8, 0],
        [0x0040, 0, 7, 0],
        [0x0049, 0, 0, 0],
        [0x0000, 0, 0, 0],
    ]
    .into_iter()
    .flat_map(|entry| entry.into_iter().flat_map(u16::to_le_bytes))
    .collect::<Vec<_>>();
    let scripts = ScriptTable::parse(&script_data).unwrap();
    let mut object = blocking_object(80, 80);
    object.auto_script = 1;
    object.trigger_script = 6;
    object.trigger_mode = 5;
    object.state = 2;
    let mut state = state(&[]).with_scene_objects(vec![object]);

    for _ in 0..4 {
        assert!(state.update_auto_scripts(&scripts).unwrap());
    }
    assert_eq!(state.scene_objects[0].auto_script, 5);
    assert_eq!(state.scene_objects[0].trigger_script, 6);
    assert_eq!(state.scene_objects[0].trigger_mode, 5);
    assert_eq!(state.scene_objects[0].state, 2);
}

#[test]
fn auto_script_sets_party_member_pose_like_scene_173() {
    let script_data = [
        [0x0000, 0, 0, 0],
        [0x0015, Direction::West as u16, 1, 0],
        [0x0000, 0, 0, 0],
    ]
    .into_iter()
    .flat_map(|entry| entry.into_iter().flat_map(u16::to_le_bytes))
    .collect::<Vec<_>>();
    let scripts = ScriptTable::parse(&script_data).unwrap();
    let mut object = blocking_object(80, 80);
    object.auto_script = 1;
    let mut state = state(&[]).with_scene_objects(vec![object]);

    assert!(state.update_auto_scripts(&scripts).unwrap());
    assert_eq!(state.scene_objects[0].auto_script, 2);
    assert_eq!(state.player.direction, Direction::West);
    assert_eq!(state.player.anim_frame, 1);
}

#[test]
fn auto_script_chance_uses_the_shared_classic_random_state() {
    let script_data = [[0x0000, 0, 0, 0], [0x0006, 100, 2, 0], [0x0000, 0, 0, 0]]
        .into_iter()
        .flat_map(|entry| entry.into_iter().flat_map(u16::to_le_bytes))
        .collect::<Vec<_>>();
    let scripts = ScriptTable::parse(&script_data).unwrap();
    let mut object = blocking_object(80, 80);
    object.auto_script = 1;
    let mut state = state(&[]).with_scene_objects(vec![object]);
    state.set_random_state(1);
    let mut expected_state = 1;
    let _ = random::random_long(&mut expected_state, 1, 100);

    assert!(state.update_auto_scripts(&scripts).unwrap());
    assert_eq!(state.random_state(), expected_state);
}

#[test]
fn auto_script_dispatches_every_reference_general_instruction() {
    for &opcode in ScriptOpcode::ALL {
        if !(0x000b..=0x00a6).contains(&opcode.raw())
            || matches!(
                opcode,
                ScriptOpcode::DialogCenter
                    | ScriptOpcode::DialogUpper
                    | ScriptOpcode::DialogLower
                    | ScriptOpcode::DialogCenterWindow
                    | ScriptOpcode::RestoreScreen
            )
        {
            continue;
        }
        let mut operands = [1, 1, 1];
        if matches!(
            opcode,
            ScriptOpcode::SetEquipmentEffect | ScriptOpcode::EquipItem
        ) {
            operands[0] = 0x0b;
        }
        let script_data = [
            [0x0000, 0, 0, 0],
            [opcode.raw(), operands[0], operands[1], operands[2]],
            [0x0000, 0, 0, 0],
        ]
        .into_iter()
        .flat_map(|entry| entry.into_iter().flat_map(u16::to_le_bytes))
        .collect::<Vec<_>>();
        let scripts = ScriptTable::parse(&script_data).unwrap();
        let mut object = blocking_object(80, 80);
        object.auto_script = 1;
        let mut state = state(&[]).with_scene_objects(vec![object]);

        let result = state.update_auto_scripts(&scripts);
        assert!(result.is_ok(), "{opcode} failed with {result:?}");
    }
}

#[test]
fn auto_script_rejects_only_trigger_only_reference_instructions() {
    for opcode in [
        ScriptOpcode::Redraw,
        ScriptOpcode::StartBattle,
        ScriptOpcode::AdvanceEntry,
        ScriptOpcode::Confirm,
        ScriptOpcode::DialogCenter,
        ScriptOpcode::DialogUpper,
        ScriptOpcode::DialogLower,
        ScriptOpcode::DialogCenterWindow,
        ScriptOpcode::RestoreScreen,
    ] {
        let script_data = [
            [0x0000, 0, 0, 0],
            [opcode.raw(), 1, 1, 1],
            [0x0000, 0, 0, 0],
        ]
        .into_iter()
        .flat_map(|entry| entry.into_iter().flat_map(u16::to_le_bytes))
        .collect::<Vec<_>>();
        let scripts = ScriptTable::parse(&script_data).unwrap();
        let mut object = blocking_object(80, 80);
        object.auto_script = 1;
        let mut state = state(&[]).with_scene_objects(vec![object]);

        assert_eq!(
            state.update_auto_scripts(&scripts),
            Err(AutoScriptError::Unsupported {
                object_id: 1,
                entry: 1,
                opcode: opcode.raw(),
            })
        );
    }
}

#[test]
fn auto_script_viewport_move_finishes_after_the_requested_frames() {
    let script_data = [
        [0x0000, 0, 0, 0],
        [ScriptOpcode::MoveViewport.raw(), 2, (-1i16) as u16, 3],
        [0x0000, 0, 0, 0],
    ]
    .into_iter()
    .flat_map(|entry| entry.into_iter().flat_map(u16::to_le_bytes))
    .collect::<Vec<_>>();
    let scripts = ScriptTable::parse(&script_data).unwrap();
    let mut object = blocking_object(80, 80);
    object.auto_script = 1;
    let mut state = state(&[]).with_scene_objects(vec![object]);
    let start = (state.camera.x, state.camera.y);

    for frame in 1..=3 {
        assert!(state.update_auto_scripts(&scripts).unwrap());
        assert_eq!(
            (state.camera.x, state.camera.y),
            (start.0 + frame * 2, start.1 - frame)
        );
        assert_eq!(
            state.scene_objects[0].auto_script,
            if frame == 3 { 2 } else { 1 }
        );
    }
    assert_eq!(state.scene_objects[0].auto_script_idle_frame, 0);
}

#[test]
fn auto_script_delay_retains_the_reference_blocking_duration() {
    let script_data = [
        [0x0000, 0, 0, 0],
        [ScriptOpcode::Delay.raw(), 3, 0, 0],
        [ScriptOpcode::AnimateObject.raw(), 0, 0, 0],
        [0x0000, 0, 0, 0],
    ]
    .into_iter()
    .flat_map(|entry| entry.into_iter().flat_map(u16::to_le_bytes))
    .collect::<Vec<_>>();
    let scripts = ScriptTable::parse(&script_data).unwrap();
    let mut object = blocking_object(80, 80);
    object.auto_script = 1;
    let mut state = state(&[]).with_scene_objects(vec![object]);

    assert!(state.update_auto_scripts(&scripts).unwrap());
    assert_eq!(state.scene_objects[0].auto_script, 1);
    assert!(state.update_auto_scripts(&scripts).unwrap());
    assert_eq!(state.scene_objects[0].auto_script, 1);
    assert!(state.update_auto_scripts(&scripts).unwrap());
    assert_eq!(state.scene_objects[0].auto_script, 2);

    assert!(state.update_auto_scripts(&scripts).unwrap());
    assert_eq!(state.scene_objects[0].auto_script, 3);
    assert_eq!(state.scene_objects[0].current_frame, 1);
}

#[test]
fn auto_script_queues_platform_events_after_advancing_its_entry() {
    let script_data = [
        [0x0000, 0, 0, 0],
        [ScriptOpcode::PlayMusic.raw(), 6, 0, 0],
        [ScriptOpcode::ShakeScreen.raw(), 4, 2, 0],
        [0x0000, 0, 0, 0],
    ]
    .into_iter()
    .flat_map(|entry| entry.into_iter().flat_map(u16::to_le_bytes))
    .collect::<Vec<_>>();
    let scripts = ScriptTable::parse(&script_data).unwrap();
    let mut object = blocking_object(80, 80);
    object.auto_script = 1;
    let mut state = state(&[]).with_scene_objects(vec![object]);

    assert!(state.update_auto_scripts(&scripts).unwrap());
    assert_eq!(state.scene_objects[0].auto_script, 2);
    assert_eq!(state.current_music, Some(6));
    assert_eq!(
        state.take_auto_script_events(),
        vec![ScriptEvent::Action(ScriptAction::PlayMusic {
            music_id: 6,
            looped: true,
            fade_seconds: 0,
        })]
    );

    assert!(state.update_auto_scripts(&scripts).unwrap());
    assert_eq!(state.scene_objects[0].auto_script, 3);
    assert_eq!(
        state.take_auto_script_events(),
        vec![ScriptEvent::Visual(ScriptVisual::Shake {
            frames: 4,
            level: 2,
        })]
    );
}

#[test]
fn auto_script_exposes_script_failure_to_an_active_trigger_host() {
    let script_data = [
        [0x0000, 0, 0, 0],
        [ScriptOpcode::MarkScriptFailed.raw(), 0, 0, 0],
        [0x0000, 0, 0, 0],
    ]
    .into_iter()
    .flat_map(|entry| entry.into_iter().flat_map(u16::to_le_bytes))
    .collect::<Vec<_>>();
    let scripts = ScriptTable::parse(&script_data).unwrap();
    let mut object = blocking_object(80, 80);
    object.auto_script = 1;
    let mut state = state(&[]).with_scene_objects(vec![object]);

    assert!(state.update_auto_scripts(&scripts).unwrap());
    assert!(state.take_auto_script_failure());
    assert!(!state.take_auto_script_failure());
}

#[test]
fn auto_script_call_uses_the_full_trigger_runtime() {
    let script_data = [
        [0x0000, 0, 0, 0],
        [0x0004, 4, 2, 0],
        [0x0000, 0, 0, 0],
        [0x0000, 0, 0, 0],
        [0x0049, 0xffff, 1, 0],
        [0x0014, 2, 0, 0],
        [0x0016, 1, Direction::East as u16, 1],
        [0x0000, 0, 0, 0],
    ]
    .into_iter()
    .flat_map(|entry| entry.into_iter().flat_map(u16::to_le_bytes))
    .collect::<Vec<_>>();
    let scripts = ScriptTable::parse(&script_data).unwrap();
    let mut caller = blocking_object(80, 80);
    caller.auto_script = 1;
    let mut target = blocking_object(100, 100);
    target.id = 2;
    target.state = 2;
    let mut state = state(&[]).with_scene_objects(vec![caller, target]);

    let update = state.update_auto_scripts_report(&scripts);
    assert!(update.changed);
    assert_eq!(update.error, None);
    assert_eq!(state.scene_objects[0].auto_script, 2);
    let request = state.take_trigger().unwrap();
    assert_eq!(request.kind, TriggerKind::Auto);
    assert_eq!(request.object_id, 2);
    assert_eq!(request.script_entry, 4);
    run_action_only_script(scripts, request, &mut state);
    assert_eq!(state.scene_objects[0].direction, Direction::East);
    assert_eq!(state.scene_objects[0].current_frame, 1);
    assert_eq!(state.scene_objects[1].state, 1);
    assert_eq!(state.scene_objects[1].direction, Direction::South);
    assert_eq!(state.scene_objects[1].current_frame, 2);
}

#[test]
fn headless_auto_call_reports_that_battle_requires_a_host() {
    let script_data = [
        [0x0000, 0, 0, 0],
        [ScriptOpcode::Call.raw(), 3, 0, 0],
        [0x0000, 0, 0, 0],
        [ScriptOpcode::StartBattle.raw(), 1, 5, 6],
        [0x0000, 0, 0, 0],
        [0x0000, 0, 0, 0],
        [0x0000, 0, 0, 0],
    ]
    .into_iter()
    .flat_map(|entry| entry.into_iter().flat_map(u16::to_le_bytes))
    .collect::<Vec<_>>();
    let scripts = ScriptTable::parse(&script_data).unwrap();
    let mut object = blocking_object(80, 80);
    object.auto_script = 1;
    let mut state = state(&[]).with_scene_objects(vec![object]);

    assert_eq!(
        state.update_auto_scripts(&scripts),
        Err(AutoScriptError::HostRequired {
            object_id: 1,
            entry: 3,
            opcode: ScriptOpcode::StartBattle.raw(),
        })
    );
    assert_eq!(state.scene_objects[0].auto_script, 2);
}

#[test]
fn auto_script_call_applies_real_object_setup_and_offset_instructions() {
    let script_data = [
        [0x0000, 0, 0, 0],
        [0x0004, 4, 0, 0],
        [0x0000, 0, 0, 0],
        [0x0000, 0, 0, 0],
        [0x0024, 2, 0, 0],
        [0x0025, 2, 9, 0],
        [0x0040, 2, 1, 0],
        [0x007D, 0xffff, 10, (-6i16) as u16],
        [0x0000, 0, 0, 0],
        [0x0000, 0, 0, 0],
    ]
    .into_iter()
    .flat_map(|entry| entry.into_iter().flat_map(u16::to_le_bytes))
    .collect::<Vec<_>>();
    let scripts = ScriptTable::parse(&script_data).unwrap();
    let mut caller = blocking_object(80, 80);
    caller.auto_script = 1;
    let mut target = blocking_object(100, 100);
    target.id = 2;
    target.auto_script = 3;
    target.auto_script_idle_frame = 12;
    target.trigger_script = 4;
    target.trigger_mode = 5;
    let mut state = state(&[]).with_scene_objects(vec![caller, target]);

    assert!(state.update_auto_scripts(&scripts).unwrap());
    assert_eq!(state.scene_objects[0].auto_script, 2);
    assert!(state.take_trigger().is_none());
    assert_eq!(
        (
            state.scene_objects[0].world_x,
            state.scene_objects[0].world_y
        ),
        (90, 74)
    );
    assert_eq!(state.scene_objects[1].auto_script, 0);
    assert_eq!(state.scene_objects[1].auto_script_idle_frame, 0);
    assert_eq!(state.scene_objects[1].trigger_script, 9);
    assert_eq!(state.scene_objects[1].trigger_mode, 1);
}

#[test]
fn auto_script_report_preserves_later_animation_when_an_object_errors() {
    let script_data = [[0x0000, 0, 0, 0], [0x1234, 0, 0, 0], [0x0087, 0, 0, 0]]
        .into_iter()
        .flat_map(|entry| entry.into_iter().flat_map(u16::to_le_bytes))
        .collect::<Vec<_>>();
    let scripts = ScriptTable::parse(&script_data).unwrap();
    let mut broken = blocking_object(80, 80);
    broken.auto_script = 1;
    let mut animator = blocking_object(100, 100);
    animator.id = 2;
    animator.auto_script = 2;
    let mut state = state(&[]).with_scene_objects(vec![broken, animator]);

    let update = state.update_auto_scripts_report(&scripts);
    assert!(update.changed);
    assert_eq!(
        update.error,
        Some(AutoScriptError::Unsupported {
            object_id: 1,
            entry: 1,
            opcode: 0x1234,
        })
    );
    assert_eq!(state.scene_objects[1].current_frame, 1);
    assert_eq!(state.scene_objects[1].auto_script, 3);
}

#[test]
fn auto_scripts_support_extended_object_motion_and_zone_branches() {
    let script_data = [
        [0x0000, 0, 0, 0],
        [0x007d, 0xffff, 0xfffc, 2],
        [0x0000, 0, 0, 0],
        [0x007c, 4, 6, 0],
        [0x0083, 4, 1, 6],
        [0x0000, 0, 0, 0],
        [0x0000, 0, 0, 0],
        [0x007e, 0xffff, 0xfffe, 0],
        [0x0000, 0, 0, 0],
    ]
    .into_iter()
    .flat_map(|entry| entry.into_iter().flat_map(u16::to_le_bytes))
    .collect::<Vec<_>>();
    let scripts = ScriptTable::parse(&script_data).unwrap();
    let mut offset = blocking_object(100, 100);
    offset.auto_script = 1;
    let mut walker = blocking_object(100, 100);
    walker.id = 2;
    walker.auto_script = 3;
    let mut zone_owner = blocking_object(100, 100);
    zone_owner.id = 3;
    zone_owner.auto_script = 4;
    let mut zone_target = blocking_object(200, 100);
    zone_target.id = 4;
    let mut layered = blocking_object(80, 80);
    layered.id = 5;
    layered.auto_script = 7;
    let mut state =
        state(&[]).with_scene_objects(vec![offset, walker, zone_owner, zone_target, layered]);

    assert!(state.update_auto_scripts(&scripts).unwrap());
    assert_eq!(
        (
            state.scene_objects[0].world_x,
            state.scene_objects[0].world_y
        ),
        (96, 102)
    );
    assert_eq!(state.scene_objects[0].auto_script, 2);
    assert_eq!(
        (
            state.scene_objects[1].world_x,
            state.scene_objects[1].world_y
        ),
        (104, 98)
    );
    assert_eq!(state.scene_objects[1].auto_script, 3);
    assert_eq!(state.scene_objects[2].auto_script, 6);
    assert_eq!(state.scene_objects[4].layer, -2);
    assert_eq!(state.scene_objects[4].auto_script, 8);
}

#[test]
fn auto_random_and_slow_walk_do_not_consume_idle_wait_frames() {
    let script_data = [
        [0x0000, 0, 0, 0],
        [0x0006, 101, 0, 0],
        [0x0011, 4, 6, 0],
        [0x0009, 3, 0, 0],
    ]
    .into_iter()
    .flat_map(|entry| entry.into_iter().flat_map(u16::to_le_bytes))
    .collect::<Vec<_>>();
    let scripts = ScriptTable::parse(&script_data).unwrap();
    let mut object = blocking_object(80, 80);
    object.auto_script = 1;
    let mut state = state(&[]).with_scene_objects(vec![object]);

    assert!(state.update_auto_scripts(&scripts).unwrap());
    assert_eq!(state.scene_objects[0].auto_script_idle_frame, 0);
    assert!(state.update_auto_scripts(&scripts).unwrap());
    assert_eq!(state.scene_objects[0].auto_script_idle_frame, 0);
}

#[test]
fn idle_resets_animation_without_moving() {
    let mut state = state(&[]);
    state.player.anim_frame = 2;
    assert!(state.update(GameInput::default()));
    assert_eq!((state.player.world_x, state.player.world_y), (320, 240));
    assert_eq!(state.player.anim_frame, 0);
    assert!(!state.update(GameInput::default()));
}

#[test]
fn idle_resets_follower_animation_when_the_leader_is_already_standing() {
    let mut state = state(&[]);
    let mut follower = state.player.clone();
    follower.anim_frame = 2;
    state.party_followers.push(follower);

    assert!(state.update(GameInput::default()));
    assert_eq!(state.player.anim_frame, 0);
    assert_eq!(state.party_followers[0].anim_frame, 0);
}

#[test]
fn camera_clamps_to_world_edges() {
    let mut camera = Camera::new(320, 200);
    camera.follow((10, 10), (1000, 800));
    assert_eq!((camera.x, camera.y), (0, 0));
    camera.follow((990, 790), (1000, 800));
    assert_eq!((camera.x, camera.y), (680, 600));
}

#[test]
fn camera_handles_a_world_smaller_than_the_viewport() {
    let mut camera = Camera::new(320, 200);
    camera.follow((50, 40), (100, 80));
    assert_eq!((camera.x, camera.y), (0, 0));
}

#[test]
fn original_save_restores_party_economy_scripts_and_viewport() {
    use pal_assets::save::{OriginalSave, DOS_SAVE_FIXED_SIZE};

    fn write_u16(data: &mut [u8], offset: usize, value: u16) {
        data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn write_i16(data: &mut [u8], offset: usize, value: i16) {
        data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    let mut bytes = vec![0; DOS_SAVE_FIXED_SIZE];
    write_i16(&mut bytes, 2, 100);
    write_i16(&mut bytes, 4, 50);
    write_u16(&mut bytes, 6, 0);
    write_u16(&mut bytes, 8, 1);
    write_u16(&mut bytes, 12, Direction::South as u16);
    write_u16(&mut bytes, 14, 31);
    write_u16(&mut bytes, 16, 5);
    write_u16(&mut bytes, 18, 9);
    write_u16(&mut bytes, 24, 17);
    write_u16(&mut bytes, 28, 3);
    write_u16(&mut bytes, 30, 45);
    bytes[40..44].copy_from_slice(&1234u32.to_le_bytes());
    write_u16(&mut bytes, 44, 0);
    write_i16(&mut bytes, 46, 160);
    write_i16(&mut bytes, 48, 112);
    write_u16(&mut bytes, 124, 42);
    write_u16(&mut bytes, 1408, 1);
    write_u16(&mut bytes, 1410, 77);
    write_u16(&mut bytes, 1728, 99);
    write_u16(&mut bytes, 1730, 3);
    write_u16(&mut bytes, 3264, 12);
    write_u16(&mut bytes, 3266, 123);
    write_u16(&mut bytes, 3268, 456);

    let save = OriginalSave::parse(&bytes).unwrap();
    let mut state = state(&[]);
    state.set_random_state(0x1234_5678);
    state.object_script_overrides.insert((1, 0), 999);
    assert!(state.restore_original_save(save, test_map(), Vec::new()));
    assert_eq!(state.random_state(), 0x1234_5678);
    assert_eq!(state.scene_number, 1);
    assert_eq!((state.camera.x, state.camera.y), (100, 50));
    assert_eq!((state.player.world_x, state.player.world_y), (260, 162));
    assert_eq!(state.party.members()[0].role_id, 0);
    assert_eq!(state.current_music, Some(31));
    assert_eq!(state.current_battle_music, 5);
    assert_eq!(state.current_battlefield, 9);
    assert_eq!(state.cash, 1234);
    assert_eq!(state.collect_value(), 17);
    assert_eq!(state.chase_range, 3);
    assert_eq!(state.chase_speed_change_cycles, 45);
    assert_eq!(state.player_poisons(0).unwrap()[0].object_id, 1);
    assert_eq!(state.player_poisons(0).unwrap()[0].script_entry, 77);
    assert_eq!(state.inventory_count(99), 3);
    assert_eq!(state.player_experience(0), Some(42));
    assert_eq!(state.scene_enter_script(0), 123);
    assert_eq!(state.scene_teleport_script(0), 456);
    assert!(state.object_script_overrides.is_empty());

    let encoded = state.original_save(13, true, 8).unwrap().encode().unwrap();
    let saved = OriginalSave::parse(&encoded).unwrap();
    assert_eq!(saved.saved_times, 13);
    assert!(saved.night_palette);
    assert_eq!(saved.screen_wave, 8);
    assert_eq!((saved.viewport_x, saved.viewport_y), (100, 50));
    assert_eq!((saved.party[0].x, saved.party[0].y), (160, 112));
    assert_eq!(saved.music_number, 31);
    assert_eq!(saved.cash, 1234);
    assert_eq!(saved.inventory[0].item_id, 99);
    assert_eq!(saved.inventory[0].amount, 3);
    assert_eq!(saved.experience[0][0].experience, 42);
    assert_eq!(saved.poisons[0][0].poison_id, 1);
    assert_eq!(saved.scenes[0].script_on_enter, 123);
    assert_eq!(saved.scenes[0].script_on_teleport, 456);

    let mut reloaded = self::state(&[]);
    assert!(reloaded.restore_original_save(saved, test_map(), Vec::new()));
    assert_eq!(reloaded.cash, 1234);
    assert_eq!(reloaded.inventory_count(99), 3);
    assert_eq!((reloaded.camera.x, reloaded.camera.y), (100, 50));
}
