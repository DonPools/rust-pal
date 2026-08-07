use super::*;

#[test]
fn follower_and_scene_map_opcodes_yield_persistent_world_actions() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [ScriptOpcode::SetPartyFollower.raw(), 3, 4, 0],
        [ScriptOpcode::SetSceneMap.raw(), 0xffff, 17, 0],
        [ScriptOpcode::SetSceneMap.raw(), 5, 18, 0],
        [ScriptOpcode::Stop.raw(), 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::SetPartyFollowers {
            followers: [Some(3), Some(4)]
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::SetSceneMap {
            scene_number: None,
            map_number: 17,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::SetSceneMap {
            scene_number: Some(5),
            map_number: 18,
        }))
    );
    assert!(matches!(
        runtime.advance(),
        Some(ScriptEvent::Completed { .. })
    ));
}

#[test]
fn teleport_yields_scene_transfer_request_and_preserves_failure_entry() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [ScriptOpcode::TeleportParty.raw(), 47, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Teleport { failure_entry: 47 })
    );
}

#[test]
fn yields_item_recovery_actions_and_reports_script_success() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [0x001b, 0, 50, 0],
        [0x001c, 1, 20, 0],
        [0x001d, 0, 10, 0],
        [0x0022, 0, 3, 0],
        [0, 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::AdjustPlayerHealth {
            role_id: 7,
            hp: 50,
            mp: 0,
            apply_to_all: false,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::AdjustPlayerHealth {
            role_id: 7,
            hp: 0,
            mp: 20,
            apply_to_all: true,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::AdjustPlayerHealth {
            role_id: 7,
            hp: 10,
            mp: 10,
            apply_to_all: false,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::RevivePlayer {
            role_id: 7,
            hp_tenths: 3,
            apply_to_all: false,
        }))
    );
    assert!(runtime.set_success(false));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Completed {
            trigger: trigger(1),
            next_entry: 1,
            succeeded: false,
        })
    );
}

#[test]
fn yields_chase_growth_and_cash_actions() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [ScriptOpcode::PauseEnemyChase.raw(), 30, 0, 0],
        [ScriptOpcode::SpeedUpEnemyChase.raw(), 40, 0, 0],
        [ScriptOpcode::LevelUpPlayer.raw(), 2, 0, 0],
        [ScriptOpcode::HalveCash.raw(), 0, 0, 0],
        [ScriptOpcode::Stop.raw(), 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    for expected in [
        ScriptAction::SetEnemyChase {
            range: 0,
            cycles: 30,
        },
        ScriptAction::SetEnemyChase {
            range: 3,
            cycles: 40,
        },
        ScriptAction::LevelUpPlayer {
            role_id: 7,
            levels: 2,
        },
        ScriptAction::HalveCash,
    ] {
        assert_eq!(runtime.advance(), Some(ScriptEvent::Action(expected)));
    }
    assert!(matches!(
        runtime.advance(),
        Some(ScriptEvent::Completed { .. })
    ));
}

#[test]
fn yields_object_script_mutations_and_rejects_invalid_fields() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [ScriptOpcode::SetObjectScript.raw(), 435, 0, 0],
        [ScriptOpcode::SetObjectScript.raw(), 454, 123, 2],
        [ScriptOpcode::Stop.raw(), 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::SetObjectScript {
            object_id: 435,
            script_entry: 0,
            field: 0,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::SetObjectScript {
            object_id: 454,
            script_entry: 123,
            field: 2,
        }))
    );

    let mut invalid = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [ScriptOpcode::SetObjectScript.raw(), 1, 2, 3],
    ]));
    invalid.start(trigger(1));
    assert_eq!(
        invalid.advance(),
        Some(ScriptEvent::Unsupported {
            trigger: trigger(1),
            entry: 1,
            opcode: ScriptOpcode::SetObjectScript.raw(),
        })
    );
}

#[test]
fn yields_role_sprite_zone_check_and_cd_fallback_actions() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [ScriptOpcode::SetPlayerSprite.raw(), 3, 42, 1],
        [ScriptOpcode::JumpIfObjectOutsideZone.raw(), 9, 2, 77],
        [ScriptOpcode::PlayCdMusic.raw(), 5, 31, 0],
        [ScriptOpcode::Stop.raw(), 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::SetPlayerSprite {
            role_id: 3,
            sprite_index: 42,
            reload: true,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::CheckObjectZone {
            object_id: 7,
            target_id: 9,
            range: 2,
            failure_entry: 77,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::PlayMusic {
            music_id: 31,
            looped: true,
            fade_seconds: 0,
        }))
    );
    assert!(matches!(
        runtime.advance(),
        Some(ScriptEvent::Completed { .. })
    ));
}

#[test]
fn yields_repeating_walk_and_relative_position_actions() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [0x0010, 4, 6, 1],
        [0x0012, 9, 0xfff0, 8],
        [0, 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::WalkObjectTo {
            object_id: 7,
            tile_x: 4,
            tile_y: 6,
            half: 1,
            speed: 3,
            repeat_entry: 1,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(
            ScriptAction::SetObjectPositionRelativeToPlayer {
                object_id: 9,
                dx: -16,
                dy: 8,
            }
        ))
    );
}

#[test]
fn moves_viewport_over_multiple_script_ticks() {
    let mut runtime =
        ScriptRuntime::new(table(&[[0, 0, 0, 0], [0x007f, 2, 0xffff, 3], [0, 0, 0, 0]]));
    runtime.start(trigger(1));
    for _ in 0..3 {
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::MoveViewport {
                x: 2,
                y: -1,
                frames: 1,
            }))
        );
    }
    assert!(matches!(
        runtime.advance(),
        Some(ScriptEvent::Completed { .. })
    ));
}

#[test]
fn sets_and_restores_viewport_without_repeating() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [0x007f, 20, 30, 0xffff],
        [0x007f, 0, 0, 0],
        [0, 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::MoveViewport {
            x: 20,
            y: 30,
            frames: -1,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::MoveViewport {
            x: 0,
            y: 0,
            frames: 1,
        }))
    );
    assert!(matches!(
        runtime.advance(),
        Some(ScriptEvent::Completed { .. })
    ));

    let mut single_frame =
        ScriptRuntime::new(table(&[[0, 0, 0, 0], [0x007f, 1, 0, 0], [0, 0, 0, 0]]));
    single_frame.start(trigger(1));
    assert_eq!(
        single_frame.advance(),
        Some(ScriptEvent::Action(ScriptAction::MoveViewport {
            x: 1,
            y: 0,
            frames: 1,
        }))
    );
    assert!(matches!(
        single_frame.advance(),
        Some(ScriptEvent::Completed { .. })
    ));
}

#[test]
fn yields_party_ride_speeds_and_collapse_action() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [0x003f, 10, 20, 0],
        [0x0044, 11, 21, 1],
        [0x0097, 12, 22, 0],
        [0x0078, 0, 0, 0],
        [0x00a1, 0, 0, 0],
        [0, 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    for (entry, speed) in [(1, 2), (2, 4), (3, 8)] {
        assert!(matches!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::RideObjectTo {
                object_id: 7,
                speed: actual_speed,
                repeat_entry,
                ..
            })) if actual_speed == speed && repeat_entry == entry
        ));
    }
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::CollapseParty))
    );
    assert!(matches!(
        runtime.advance(),
        Some(ScriptEvent::Completed { .. })
    ));
}

#[test]
fn yields_confirmation_and_item_removal() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [0x000a, 4, 0, 0],
        [0x0020, 99, 0, 6],
        [0, 0, 0, 0],
        [0xffff, 10, 0, 0],
        [0, 0, 0, 0],
        [0xffff, 11, 0, 0],
        [0, 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Confirm { no_entry: 4 })
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::RemoveItem {
            item_id: 99,
            amount: 1,
            insufficient_entry: 6,
        }))
    );
    assert!(runtime.branch_to(6));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Message {
            message_id: 11,
            position: DialogPosition::Upper,
            font_color: 0x4f,
            face_index: None,
            playing_rng: false,
        })
    );
}

#[test]
fn marks_failure_and_yields_party_and_equipment_conditions() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [0x0084, 798, 2, 7],
        [0x0041, 0, 0, 0],
        [0x0074, 8, 0, 0],
        [0x0086, 274, 2, 9],
        [0, 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::PlaceObjectInFront {
            object_id: 798,
            state: 2,
            blocked_entry: 7,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Condition(ScriptCondition::PartyNotFullHp {
            target_entry: 8,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Condition(ScriptCondition::ItemNotEquipped {
            item_id: 274,
            amount: 2,
            target_entry: 9,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Completed {
            trigger: trigger(1),
            next_entry: 1,
            succeeded: false,
        })
    );
}

#[test]
fn yields_equipment_attribute_and_magic_actions() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [0x0018, 0x0e, 163, 0],
        [0x0017, 0x0e, 17, 20],
        [0x001a, 4, 1, 0],
        [0x0019, 17, 3, 2],
        [0x0023, 1, 4, 0],
        [0x0055, 88, 0, 0],
        [0x0056, 89, 2, 0],
        [0, 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::EquipItem {
            role_id: 7,
            slot: 3,
            item_id: 163,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::SetEquipmentEffect {
            role_id: 7,
            attribute: 17,
            slot: 3,
            value: 20,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::ChangePlayerAttribute {
            role_id: 7,
            attribute: 4,
            value: 1,
            absolute: true,
        }))
    );
    assert!(matches!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::ChangePlayerAttribute {
            role_id: 1,
            absolute: false,
            ..
        }))
    ));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::RemoveEquipment {
            role_id: 1,
            slot: Some(3),
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::ChangeMagic {
            role_id: 7,
            magic_id: 88,
            add: true,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::ChangeMagic {
            role_id: 1,
            magic_id: 89,
            add: false,
        }))
    );
}

#[test]
fn yields_player_facing_condition() {
    let mut runtime = ScriptRuntime::new(table(&[[0, 0, 0, 0], [0x0081, 12, 2, 7], [0, 0, 0, 0]]));
    runtime.start(trigger(1));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Condition(ScriptCondition::PlayerFacesObject {
            object_id: 12,
            range: 2,
            target_entry: 7,
        }))
    );
}

#[test]
fn yields_scene_script_updates() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [0x006d, 2, 100, 200],
        [0x006d, 3, 0, 0],
        [0, 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::SetSceneScripts {
            scene_number: 2,
            enter_script: Some(100),
            teleport_script: Some(200),
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::SetSceneScripts {
            scene_number: 3,
            enter_script: Some(0),
            teleport_script: Some(0),
        }))
    );
}

#[test]
fn yields_wait_ticks_and_world_actions() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [0x0009, 2, 0, 0],
        [0x000b, 0, 0, 0],
        [0x0049, 0xffff, 0xffff, 0],
        [0x006e, 0xfff0, 8, 0],
        [0, 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert_eq!(runtime.advance(), Some(ScriptEvent::Waiting));
    assert_eq!(runtime.advance(), Some(ScriptEvent::Waiting));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::MoveObject {
            object_id: 7,
            direction: Direction::South,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::SetObjectState {
            object_id: 7,
            state: -1,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::OffsetPlayer {
            dx: -16,
            dy: 8,
        }))
    );
    assert!(matches!(
        runtime.advance(),
        Some(ScriptEvent::Completed { .. })
    ));
}

#[test]
fn updates_object_trigger_fields_and_delays_in_80ms_periods() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [0x0024, 10, 0x135d, 0],
        [0x0040, 10, 2, 0],
        [0x0025, 10, 0x119e, 0],
        [0x0085, 2, 0, 0],
        [0, 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::SetObjectAutoScript {
            object_id: 10,
            script_entry: 0x135d,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::SetObjectTriggerMode {
            object_id: 10,
            trigger_mode: 2,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::SetObjectTriggerScript {
            object_id: 10,
            script_entry: 0x119e,
        }))
    );
    assert_eq!(runtime.advance(), Some(ScriptEvent::Delay));
    assert_eq!(runtime.advance(), Some(ScriptEvent::Delay));
    assert_eq!(runtime.advance(), Some(ScriptEvent::Delay));
    assert_eq!(runtime.advance(), Some(ScriptEvent::Delay));
    assert!(matches!(
        runtime.advance(),
        Some(ScriptEvent::Completed { .. })
    ));
}

#[test]
fn yields_inventory_position_and_scene_actions() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [0x001f, 99, 0, 0],
        [0x0046, 45, 96, 0],
        [0x0059, 3, 0, 0],
        [0, 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::AddItem {
            item_id: 99,
            amount: 0,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::SetPlayerPosition {
            tile_x: 45,
            tile_y: 96,
            half: 0,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::ChangeScene {
            scene_number: 3,
        }))
    );
}

#[test]
fn yields_zero_based_party_members_and_default_leader() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [0x0075, 3, 1, 0],
        [0x0075, 0, 0, 0],
        [0, 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::SetParty {
            members: [Some(2), Some(0), None],
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::SetParty {
            members: [Some(0), None, None],
        }))
    );
}

#[test]
fn yields_temporary_hide_fast_walk_and_object_transform_actions() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [0x0052, 0, 0, 0],
        [0x0079, 37, 9, 0],
        [0x007a, 4, 6, 1],
        [0x007c, 8, 9, 0],
        [0x007d, 0xffff, 0xfffc, 2],
        [0x007e, 12, 0xfff6, 0],
        [0x0082, 10, 11, 1],
        [0, 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::HideObjectTemporarily {
            object_id: 7,
            vanish_time: 800,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Condition(ScriptCondition::PartyContainsName {
            name_word_id: 37,
            target_entry: 9,
        }))
    );
    assert!(matches!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::WalkPlayerTo {
            speed: 4,
            repeat_entry: 3,
            ..
        }))
    ));
    assert!(matches!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::WalkObjectTo {
            object_id: 7,
            speed: 2,
            repeat_entry: 4,
            ..
        }))
    ));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::MoveObjectBy {
            object_id: 7,
            dx: -4,
            dy: 2,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::SetObjectLayer {
            object_id: 12,
            layer: -10,
        }))
    );
    assert!(matches!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::WalkObjectTo {
            speed: 8,
            repeat_entry: 7,
            ..
        }))
    ));
}

#[test]
fn yields_persistent_state_conditions_and_batch_mutation() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [0x0058, 9, 2, 7],
        [0x0094, 0xffff, 0xffff, 8],
        [0x0095, 3, 9, 0],
        [0x009a, 4, 6, 0xfffe],
        [0, 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Condition(ScriptCondition::ItemCountLess {
            item_id: 9,
            amount: 2,
            target_entry: 7,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Condition(ScriptCondition::ObjectStateEquals {
            object_id: 7,
            state: -1,
            target_entry: 8,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Condition(ScriptCondition::SceneEquals {
            scene_number: 3,
            target_entry: 9,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::SetObjectStates {
            first_object_id: 4,
            last_object_id: 6,
            state: -2,
        }))
    );
}
