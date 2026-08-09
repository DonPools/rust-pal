use super::*;

#[test]
fn nested_script_call_returns_to_the_caller_entry() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [ScriptOpcode::Stop.raw(), 0, 0, 0],
        [ScriptOpcode::Call.raw(), 3, 0, 0],
        [ScriptOpcode::Stop.raw(), 0, 0, 0],
    ]));
    runtime.start(trigger(2));
    assert!(runtime.call(3, 0xffff));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Completed {
            trigger: trigger(2),
            next_entry: 2,
            succeeded: true,
        })
    );
}

#[test]
fn system_trigger_reuses_the_last_concrete_event_object() {
    let mut runtime = ScriptRuntime::new(table(&[
        [ScriptOpcode::Stop.raw(), 0, 0, 0],
        [ScriptOpcode::Stop.raw(), 0, 0, 0],
        [ScriptOpcode::RideObject.raw(), 26, 93, 1],
        [ScriptOpcode::Stop.raw(), 0, 0, 0],
    ]));
    assert!(runtime.start(trigger(1)));
    assert!(matches!(
        runtime.advance(),
        Some(ScriptEvent::Completed { .. })
    ));

    let system = TriggerRequest {
        object_id: u16::MAX,
        script_entry: 2,
        kind: TriggerKind::Touch,
    };
    assert!(runtime.start(system));
    let debug = runtime.debug_snapshot();
    assert_eq!(debug.trigger, Some(system));
    assert_eq!(debug.next_instruction.unwrap().object_id, 7);
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::RideObjectTo {
            object_id: 7,
            tile_x: 26,
            tile_y: 93,
            half: 1,
            speed: 4,
            repeat_entry: 2,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Completed {
            trigger: system,
            next_entry: 2,
            succeeded: true,
        })
    );
}

#[test]
fn explicit_call_owner_becomes_the_next_system_owner() {
    let mut runtime = ScriptRuntime::new(table(&[
        [ScriptOpcode::Stop.raw(), 0, 0, 0],
        [ScriptOpcode::Call.raw(), 3, 9, 0],
        [ScriptOpcode::Stop.raw(), 0, 0, 0],
        [ScriptOpcode::Stop.raw(), 0, 0, 0],
        [ScriptOpcode::RideObject.raw(), 1, 2, 0],
    ]));
    assert!(runtime.start(trigger(1)));
    assert!(matches!(
        runtime.advance(),
        Some(ScriptEvent::Completed { .. })
    ));

    assert!(runtime.start(TriggerRequest {
        object_id: u16::MAX,
        script_entry: 4,
        kind: TriggerKind::Touch,
    }));
    assert!(matches!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::RideObjectTo {
            object_id: 9,
            ..
        }))
    ));
}

#[test]
fn debug_snapshot_tracks_trigger_and_instructions_after_completion() {
    let mut runtime = ScriptRuntime::new(table(&[[0, 0, 0, 0], [0x0005, 2, 0, 0], [0, 0, 0, 0]]));
    let request = trigger(1);

    assert!(runtime.start(request));
    assert_eq!(
        runtime.debug_snapshot(),
        ScriptDebugSnapshot {
            active: true,
            trigger: Some(request),
            last_instruction: None,
            next_instruction: Some(ScriptInstructionDebug {
                object_id: request.object_id,
                entry: 1,
                opcode: 0x0005,
                operands: [2, 0, 0],
            }),
            call_depth: 0,
            wait_frames: 0,
        }
    );

    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Redraw {
            update_party_gestures: false,
        })
    );
    let waiting = runtime.debug_snapshot();
    assert_eq!(waiting.last_instruction.unwrap().entry, 1);
    assert_eq!(waiting.next_instruction.unwrap().entry, 2);
    assert_eq!(waiting.wait_frames, 1);

    assert_eq!(runtime.advance(), Some(ScriptEvent::Delay));
    assert!(matches!(
        runtime.advance(),
        Some(ScriptEvent::Completed { .. })
    ));
    let completed = runtime.debug_snapshot();
    assert!(!completed.active);
    assert_eq!(completed.trigger, Some(request));
    assert_eq!(completed.last_instruction.unwrap().entry, 2);
    assert_eq!(completed.next_instruction, None);
}

#[test]
fn trace_retains_every_instruction_executed_in_one_advance() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [ScriptOpcode::Jump.raw(), 3, 0, 0],
        [ScriptOpcode::Stop.raw(), 0, 0, 0],
        [ScriptOpcode::AdvanceEntry.raw(), 0, 0, 0],
        [ScriptOpcode::Stop.raw(), 0, 0, 0],
    ]));
    let request = trigger(1);

    assert!(runtime.start(request));
    assert!(matches!(
        runtime.advance(),
        Some(ScriptEvent::Completed { .. })
    ));

    let records = runtime.trace_records().copied().collect::<Vec<_>>();
    assert_eq!(records.len(), 3);
    assert_eq!(
        records
            .iter()
            .map(|record| record.instruction.entry)
            .collect::<Vec<_>>(),
        vec![1, 3, 4]
    );
    assert_eq!(records[0].next_entry, Some(3));
    assert_eq!(records[1].next_entry, Some(4));
    assert_eq!(
        records[2].outcome,
        ScriptTraceOutcome::Completed {
            next_entry: 4,
            succeeded: true,
        }
    );
    assert!(records.iter().all(|record| record.trigger == request));
}

#[test]
fn debugger_can_break_before_a_trigger_and_step_one_instruction() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [ScriptOpcode::Jump.raw(), 3, 0, 0],
        [ScriptOpcode::Stop.raw(), 0, 0, 0],
        [ScriptOpcode::AdvanceEntry.raw(), 0, 0, 0],
        [ScriptOpcode::Stop.raw(), 0, 0, 0],
    ]));

    runtime.arm_debug_break();
    assert!(runtime.debug_break_armed());
    assert!(runtime.start(trigger(1)));
    assert!(!runtime.debug_paused());
    assert!(runtime.debug_break_armed());
    assert_eq!(runtime.advance(), None);
    assert!(runtime.debug_paused());
    assert!(!runtime.debug_break_armed());
    assert_eq!(runtime.trace_records().len(), 0);

    assert!(runtime.request_debug_step());
    assert_eq!(runtime.advance(), None);
    assert!(runtime.debug_paused());
    assert_eq!(runtime.debug_snapshot().next_instruction.unwrap().entry, 3);
    assert_eq!(runtime.trace_records().len(), 1);

    runtime.resume_debug();
    assert!(!runtime.debug_paused());
    assert!(matches!(
        runtime.advance(),
        Some(ScriptEvent::Completed { .. })
    ));
}

#[test]
fn debugger_lets_a_yield_finish_before_breaking_at_the_next_instruction() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [u16::MAX, 0x002a, 0, 0],
        [ScriptOpcode::Stop.raw(), 0, 0, 0],
    ]));

    runtime.arm_debug_break();
    assert!(runtime.start(trigger(1)));
    assert_eq!(runtime.advance(), None);
    assert!(runtime.request_debug_step());
    assert!(matches!(
        runtime.advance(),
        Some(ScriptEvent::Message {
            message_id: 0x002a,
            ..
        })
    ));
    assert!(!runtime.debug_paused());
    assert!(runtime.debug_break_armed());

    assert_eq!(runtime.advance(), None);
    assert!(runtime.debug_paused());
    assert!(!runtime.debug_break_armed());
    assert!(runtime.request_debug_step());
    assert!(matches!(
        runtime.advance(),
        Some(ScriptEvent::Completed { .. })
    ));
    assert!(!runtime.debug_paused());
    assert!(!runtime.debug_break_armed());
}

#[test]
fn trace_ring_discards_the_oldest_records_at_capacity() {
    let mut entries = vec![[ScriptOpcode::NoOp.raw(), 0, 0, 0]; 514];
    entries[0] = [ScriptOpcode::Stop.raw(), 0, 0, 0];
    entries[513] = [ScriptOpcode::Stop.raw(), 0, 0, 0];
    let mut runtime = ScriptRuntime::new(table(&entries));

    assert!(runtime.start(trigger(1)));
    assert!(matches!(
        runtime.advance(),
        Some(ScriptEvent::Completed { .. })
    ));

    let records = runtime.trace_records().copied().collect::<Vec<_>>();
    assert_eq!(records.len(), 512);
    assert_eq!(records.first().unwrap().instruction.entry, 2);
    assert_eq!(records.last().unwrap().instruction.entry, 513);
}

#[test]
fn follows_jumps_and_updates_persistent_entry() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [3, 3, 0, 0],
        [0, 0, 0, 0],
        [8, 0, 0, 0],
        [0, 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Completed {
            trigger: trigger(1),
            next_entry: 4,
            succeeded: true,
        })
    );
}

#[test]
fn idle_limited_control_flow_persists_across_trigger_runs() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [0x0002, 4, 2, 0],
        [0xffff, 9, 0, 0],
        [0, 0, 0, 0],
        [0xffff, 10, 0, 0],
        [0, 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Completed {
            trigger: trigger(1),
            next_entry: 4,
            succeeded: true,
        })
    );

    runtime.start(trigger(1));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Message {
            message_id: 9,
            position: DialogPosition::Upper,
            font_color: 0x4f,
            face_index: None,
            playing_rng: false,
        })
    );

    let mut jump = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [0x0003, 1, 2, 0],
        [0xffff, 20, 0, 0],
        [0, 0, 0, 0],
    ]));
    jump.start(trigger(1));
    assert_eq!(
        jump.advance(),
        Some(ScriptEvent::Message {
            message_id: 20,
            position: DialogPosition::Upper,
            font_color: 0x4f,
            face_index: None,
            playing_rng: false,
        })
    );
}

#[test]
fn calls_subscripts_and_returns_to_the_caller() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [0x0004, 4, 9, 0],
        [0xffff, 42, 0, 0],
        [0, 0, 0, 0],
        [0x0014, 2, 0, 0],
        [0, 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::SetObjectPose {
            object_id: 9,
            direction: Some(Direction::South),
            frame: Some(2),
        }))
    );
    let records = runtime.trace_records().copied().collect::<Vec<_>>();
    assert_eq!(records[0].instruction.opcode, ScriptOpcode::Call.raw());
    assert_eq!(
        (records[0].call_depth_before, records[0].call_depth_after),
        (0, 1)
    );
    assert_eq!(
        (records[1].call_depth_before, records[1].call_depth_after),
        (1, 1)
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Message {
            message_id: 42,
            position: DialogPosition::Upper,
            font_color: 0x4f,
            face_index: None,
            playing_rng: false,
        })
    );
    assert!(matches!(
        runtime.advance(),
        Some(ScriptEvent::Completed { .. })
    ));
}

#[test]
fn subscript_persistent_exit_does_not_mutate_the_called_object() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [0x0004, 4, 9, 0],
        [0xffff, 42, 0, 0],
        [0, 0, 0, 0],
        [0x0001, 0, 0, 0],
        [0, 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Message {
            message_id: 42,
            position: DialogPosition::Upper,
            font_color: 0x4f,
            face_index: None,
            playing_rng: false,
        })
    );
}

#[test]
fn probability_branch_uses_a_deterministic_percent_roll() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [0x0006, 1, 3, 0],
        [0xffff, 10, 0, 0],
        [0xffff, 20, 0, 0],
        [0, 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Message {
            message_id: 20,
            position: DialogPosition::Upper,
            font_color: 0x4f,
            face_index: None,
            playing_rng: false,
        })
    );

    let mut impossible = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [0x0006, 101, 3, 0],
        [0xffff, 30, 0, 0],
        [0xffff, 40, 0, 0],
    ]));
    impossible.start(trigger(1));
    assert_eq!(
        impossible.advance(),
        Some(ScriptEvent::Message {
            message_id: 30,
            position: DialogPosition::Upper,
            font_color: 0x4f,
            face_index: None,
            playing_rng: false,
        })
    );
}

#[test]
fn reports_unsupported_and_invalid_entries() {
    let mut runtime = ScriptRuntime::new(table(&[[0, 0, 0, 0], [0x1234, 0, 0, 0]]));
    runtime.start(trigger(1));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Unsupported {
            trigger: trigger(1),
            entry: 1,
            opcode: 0x1234,
        })
    );

    runtime.start(trigger(99));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::InvalidEntry {
            trigger: trigger(99),
            entry: 99,
        })
    );
}

#[test]
fn cash_action_can_redirect_active_execution() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [0x001e, 0xfff6, 4, 0],
        [0xffff, 10, 0, 0],
        [0, 0, 0, 0],
        [0xffff, 20, 0, 0],
        [0, 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::AdjustCash {
            amount: -10,
            insufficient_entry: 4,
        }))
    );
    assert!(runtime.branch_to(4));
    assert_eq!(
        runtime.trace_records().next_back().unwrap().next_entry,
        Some(4)
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Message {
            message_id: 20,
            position: DialogPosition::Upper,
            font_color: 0x4f,
            face_index: None,
            playing_rng: false,
        })
    );
}
