use super::*;

#[test]
fn battle_suspends_execution_and_resumes_on_the_result_branch() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [ScriptOpcode::StartBattle.raw(), 18, 4, 5],
        [ScriptOpcode::Stop.raw(), 0, 0, 0],
        [0, 0, 0, 0],
        [ScriptOpcode::Stop.raw(), 0, 0, 0],
        [ScriptOpcode::Stop.raw(), 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    let request = BattleRequest {
        enemy_team: 18,
        lost_entry: 4,
        flee_entry: 5,
        is_boss: false,
    };
    assert_eq!(runtime.advance(), Some(ScriptEvent::StartBattle(request)));
    assert!(runtime.is_active());
    assert!(runtime.is_waiting_for_battle());
    assert_eq!(runtime.advance(), None);
    assert!(runtime.resolve_battle(BattleResult::Lost));
    assert!(!runtime.is_waiting_for_battle());
    assert!(matches!(
        runtime.advance(),
        Some(ScriptEvent::Completed { .. })
    ));
}

#[test]
fn battle_win_continues_and_zero_flee_operand_marks_a_boss() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [ScriptOpcode::StartBattle.raw(), 18, 40, 0],
        [ScriptOpcode::Stop.raw(), 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::StartBattle(BattleRequest {
            enemy_team: 18,
            lost_entry: 40,
            flee_entry: 0,
            is_boss: true,
        }))
    );
    assert!(runtime.resolve_battle(BattleResult::Won));
    assert!(matches!(
        runtime.advance(),
        Some(ScriptEvent::Completed { .. })
    ));
    assert!(runtime.start(trigger(1)));
    assert!(matches!(
        runtime.advance(),
        Some(ScriptEvent::StartBattle(_))
    ));
    assert!(runtime.resolve_battle(BattleResult::Terminated));
    assert!(matches!(
        runtime.advance(),
        Some(ScriptEvent::Completed { .. })
    ));
}

#[test]
fn battle_configuration_opcodes_yield_world_actions() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [ScriptOpcode::SetBattleMusic.raw(), 7, 0, 0],
        [ScriptOpcode::SetBattlefield.raw(), 21, 0, 0],
        [ScriptOpcode::Stop.raw(), 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::SetBattleMusic {
            music_id: 7,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::SetBattlefield {
            battlefield_id: 21,
        }))
    );
}

#[test]
fn yields_battle_status_and_poison_actions() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [ScriptOpcode::DamageEnemy.raw(), 1, 25, 0],
        [ScriptOpcode::PoisonEnemy.raw(), 0, 40, 0],
        [ScriptOpcode::PoisonPlayer.raw(), 1, 41, 0],
        [ScriptOpcode::CureEnemyPoison.raw(), 0, 40, 0],
        [ScriptOpcode::CurePlayerPoison.raw(), 1, 41, 0],
        [ScriptOpcode::CurePoisonByLevel.raw(), 0, 3, 0],
        [ScriptOpcode::SetPlayerStatus.raw(), 6, 4, 0],
        [ScriptOpcode::SetEnemyStatus.raw(), 2, 5, 99],
        [ScriptOpcode::RemovePlayerStatus.raw(), 6, 0, 0],
        [ScriptOpcode::AdjustTemporaryPlayerStat.raw(), 17, 50, 2],
        [ScriptOpcode::SetTemporaryBattleSprite.raw(), 5, 0, 0],
        [ScriptOpcode::CollectEnemy.raw(), 88, 0, 0],
        [ScriptOpcode::TransmuteCollectedEnemies.raw(), 0, 0, 0],
        [ScriptOpcode::HideBattleActor.raw(), 3, 0, 0],
        [ScriptOpcode::StealEnemy.raw(), 5, 0, 0],
        [ScriptOpcode::EnableAutoBattle.raw(), 0, 0, 0],
        [ScriptOpcode::DrainEnemyHp.raw(), 12, 0, 0],
        [ScriptOpcode::FleeBattle.raw(), 94, 0, 0],
        [ScriptOpcode::HalvePlayerHp.raw(), 0, 0, 0],
        [ScriptOpcode::HalveEnemyHp.raw(), 50, 0, 0],
        [ScriptOpcode::KillPlayer.raw(), 0, 0, 0],
        [ScriptOpcode::KillEnemy.raw(), 0, 0, 0],
        [ScriptOpcode::EnemyCastMagic.raw(), 88, 0, 0],
        [ScriptOpcode::EnemyEscape.raw(), 0, 0, 0],
        [ScriptOpcode::SetBattleResult.raw(), 0, 0, 0],
        [ScriptOpcode::SimulatePlayerMagic.raw(), 88, 123, 0],
        [ScriptOpcode::ThrowWeapon.raw(), 91, 7, 0],
        [ScriptOpcode::ScaleMagicByMp.raw(), 89, 0, 0],
        [ScriptOpcode::ScaleMagicByCash.raw(), 90, 0, 0],
        [ScriptOpcode::DivideEnemy.raw(), 2, 98, 0],
        [ScriptOpcode::SummonEnemy.raw(), 501, 3, 99],
        [ScriptOpcode::TransformEnemy.raw(), 502, 0, 0],
        [ScriptOpcode::JumpIfPlayerLacksPoison.raw(), 40, 91, 0],
        [ScriptOpcode::JumpIfEnemyLacksPoison.raw(), 41, 92, 0],
        [ScriptOpcode::JumpIfPlayerNotPoisoned.raw(), 93, 0, 0],
        [ScriptOpcode::JumpIfEnemyHpAbove.raw(), 60, 95, 0],
        [ScriptOpcode::JumpIfEnemyNotFirstKind.raw(), 96, 0, 0],
        [ScriptOpcode::JumpIfEnemyTurn.raw(), 97, 0, 0],
        [ScriptOpcode::Stop.raw(), 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    let expected = [
        ScriptAction::DamageEnemy {
            enemy_index: 7,
            amount: 25,
            apply_to_all: true,
        },
        ScriptAction::PoisonEnemy {
            enemy_index: 7,
            poison_id: 40,
            apply_to_all: false,
        },
        ScriptAction::PoisonPlayer {
            role_id: 7,
            poison_id: 41,
            apply_to_all: true,
        },
        ScriptAction::CureEnemyPoison {
            enemy_index: 7,
            poison_id: 40,
            apply_to_all: false,
        },
        ScriptAction::CurePlayerPoison {
            role_id: 7,
            poison_id: 41,
            apply_to_all: true,
        },
        ScriptAction::CurePlayerPoisonByLevel {
            role_id: 7,
            maximum_level: 3,
            apply_to_all: false,
        },
        ScriptAction::SetPlayerStatus {
            role_id: 7,
            status: 6,
            rounds: 4,
        },
        ScriptAction::SetEnemyStatus {
            enemy_index: 7,
            status: 2,
            rounds: 5,
            resisted_entry: 99,
        },
        ScriptAction::RemovePlayerStatus {
            role_id: 7,
            status: 6,
        },
        ScriptAction::AdjustTemporaryPlayerStat {
            role_id: 1,
            attribute: 17,
            percent: 50,
        },
        ScriptAction::SetTemporaryBattleSprite {
            role_id: 7,
            sprite: 5,
        },
        ScriptAction::CollectEnemy {
            enemy_index: 7,
            failure_entry: 88,
        },
        ScriptAction::TransmuteCollectedEnemies,
        ScriptAction::HideBattleActor { rounds: 3 },
        ScriptAction::StealEnemy {
            enemy_index: 7,
            rate: 5,
        },
        ScriptAction::EnableAutoBattle,
        ScriptAction::DrainEnemyHp {
            enemy_index: 7,
            amount: 12,
        },
        ScriptAction::FleeBattle { failure_entry: 94 },
        ScriptAction::HalvePlayerHp { role_id: 7 },
        ScriptAction::HalveEnemyHp {
            enemy_index: 7,
            maximum_damage: 50,
        },
        ScriptAction::KillPlayer { role_id: 7 },
        ScriptAction::KillEnemy { enemy_index: 7 },
        ScriptAction::SetEnemyMagic {
            enemy_index: 7,
            magic_object: 88,
            rate: 0,
        },
        ScriptAction::EnemyEscape,
        ScriptAction::SetBattleResult { result: 0 },
        ScriptAction::SimulatePlayerMagic {
            enemy_index: 7,
            magic_object: 88,
            base_strength: 123,
        },
        ScriptAction::ThrowWeapon {
            enemy_index: 7,
            magic_object: 91,
            multiplier: 7,
        },
        ScriptAction::ScaleMagicByMp {
            role_id: 7,
            magic_object: 89,
            multiplier: 8,
        },
        ScriptAction::ScaleMagicByCash { magic_object: 90 },
        ScriptAction::DivideEnemy {
            enemy_index: 7,
            copies: 2,
            failure_entry: 98,
        },
        ScriptAction::SummonEnemy {
            enemy_index: 7,
            object_id: 501,
            count: 3,
            failure_entry: 99,
        },
        ScriptAction::TransformEnemy {
            enemy_index: 7,
            object_id: 502,
        },
    ];
    for action in expected {
        assert_eq!(runtime.advance(), Some(ScriptEvent::Action(action)));
    }
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Condition(ScriptCondition::PlayerLacksPoison {
            role_id: 7,
            poison_id: 40,
            target_entry: 91,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Condition(ScriptCondition::EnemyLacksPoison {
            enemy_index: 7,
            poison_id: 41,
            target_entry: 92,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Condition(ScriptCondition::PlayerNotPoisoned {
            role_id: 7,
            target_entry: 93,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Condition(ScriptCondition::EnemyHpAbove {
            enemy_index: 7,
            percentage: 60,
            target_entry: 95,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Condition(ScriptCondition::EnemyNotFirstKind {
            enemy_index: 7,
            target_entry: 96,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Condition(ScriptCondition::EnemyTurn {
            target_entry: 97,
        }))
    );
    assert!(matches!(
        runtime.advance(),
        Some(ScriptEvent::Completed { .. })
    ));
}

#[test]
fn yields_signed_battle_blow_amounts() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [ScriptOpcode::BlowEnemiesAway.raw(), 2, 0, 0],
        [ScriptOpcode::BlowEnemiesAway.raw(), 0xfffd, 0, 0],
        [ScriptOpcode::Stop.raw(), 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::SetBattleBlow {
            amount: 2
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::SetBattleBlow {
            amount: -3
        }))
    );
}

#[test]
fn yields_optional_player_magic_animation_target() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [ScriptOpcode::PlayerMagicAnimation.raw(), 2, 0, 0],
        [ScriptOpcode::PlayerMagicAnimation.raw(), 0, 0, 0],
        [ScriptOpcode::Stop.raw(), 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::PlayerMagicAnimation {
            player: Some(1),
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::PlayerMagicAnimation {
            player: None,
        }))
    );
}
