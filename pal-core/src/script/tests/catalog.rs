use super::*;

#[test]
fn opcode_catalog_covers_every_original_instruction_once() {
    assert_eq!(ScriptOpcode::ALL.len(), 165);
    for pair in ScriptOpcode::ALL.windows(2) {
        assert!(pair[0].raw() < pair[1].raw());
    }
    for &opcode in ScriptOpcode::ALL {
        assert_eq!(ScriptOpcode::from_raw(opcode.raw()), Some(opcode));
        assert!(!opcode.mnemonic().is_empty());
        assert!(!opcode.description().is_empty());
    }

    let support_counts = ScriptOpcode::ALL
        .iter()
        .fold([0usize; 3], |mut counts, opcode| {
            let index = match opcode.support() {
                OpcodeSupport::Implemented => 0,
                OpcodeSupport::Stub => 1,
                OpcodeSupport::Unsupported => 2,
            };
            counts[index] += 1;
            counts
        });
    assert_eq!(support_counts, [165, 0, 0]);

    for hole in [0x0032, 0x0048, 0x0072, 0x009d] {
        assert_eq!(ScriptOpcode::from_raw(hole), None);
    }
    assert_eq!(
        ScriptOpcode::AdjustPlayerHp.support(),
        OpcodeSupport::Implemented
    );
    assert_eq!(
        ScriptOpcode::FadeScene.support(),
        OpcodeSupport::Implemented
    );
    assert_eq!(
        ScriptOpcode::StartBattle.support(),
        OpcodeSupport::Implemented
    );
    assert!(ScriptOpcode::PrintMessage.is_implemented());
    assert_eq!(ScriptOpcode::FadeScene.to_string(), "FADE_SCENE");
}

#[test]
fn catalog_support_is_engine_wide_and_trigger_rejects_unsupported_opcodes() {
    for &opcode in ScriptOpcode::ALL {
        if opcode.support() != OpcodeSupport::Unsupported {
            continue;
        }
        let mut runtime = ScriptRuntime::new(table(&[[0, 0, 0, 0], [opcode.raw(), 0, 0, 0]]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Unsupported {
                trigger: trigger(1),
                entry: 1,
                opcode: opcode.raw(),
            }),
            "{} ({:04X}) was not rejected by its explicit match arm",
            opcode.mnemonic(),
            opcode.raw()
        );
    }

    // ChasePlayer is implemented by the auto-script scheduler, but not by
    // the trigger-script interpreter.
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [ScriptOpcode::ChasePlayer.raw(), 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert!(matches!(
        runtime.advance(),
        Some(ScriptEvent::Unsupported { opcode: 0x004c, .. })
    ));
}
