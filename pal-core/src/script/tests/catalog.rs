use super::*;
use crate::script::opcode::TriggerHandler;

#[test]
fn opcode_catalog_covers_every_original_instruction_once() {
    assert_eq!(ScriptOpcode::ALL.len(), 165);
    for pair in ScriptOpcode::ALL.windows(2) {
        assert!(pair[0].raw() < pair[1].raw());
    }
    for &opcode in ScriptOpcode::ALL {
        assert_eq!(ScriptOpcode::from_raw(opcode.raw()), Some(opcode));
        assert!(!opcode.name().is_empty());
    }

    let handler_counts = ScriptOpcode::ALL
        .iter()
        .fold([0usize; 6], |mut counts, opcode| {
            let index = match opcode.trigger_handler() {
                TriggerHandler::Control => 0,
                TriggerHandler::Presentation => 1,
                TriggerHandler::Scene => 2,
                TriggerHandler::Role => 3,
                TriggerHandler::Battle => 4,
                TriggerHandler::Condition => 5,
            };
            counts[index] += 1;
            counts
        });
    assert_eq!(handler_counts, [13, 37, 44, 20, 37, 14]);

    for hole in [0x0032, 0x0048, 0x0072, 0x009d] {
        assert_eq!(ScriptOpcode::from_raw(hole), None);
    }
    assert_eq!(ScriptOpcode::FadeScene.name(), "FadeScene");
    assert_eq!(ScriptOpcode::FadeScene.to_string(), "FadeScene");
}

#[test]
fn trigger_dispatches_chase_player_to_the_world_host() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [ScriptOpcode::ChasePlayer.raw(), 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::ChaseObject {
            object_id: 7,
            speed: 4,
            range: 8,
            floating: false,
        }))
    );
}
