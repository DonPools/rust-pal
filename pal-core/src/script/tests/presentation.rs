use super::*;

#[test]
fn visual_opcodes_yield_typed_blocking_events() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [ScriptOpcode::ShakeScreen.raw(), 3, 0, 0],
        [ScriptOpcode::SelectRngAnimation.raw(), 7, 0, 0],
        [ScriptOpcode::PlayRngAnimation.raw(), 2, 5, 0],
        [ScriptOpcode::FadeOut.raw(), 0, 0, 0],
        [ScriptOpcode::FadeIn.raw(), 0xffff, 0, 0],
        [ScriptOpcode::UseNightPalette.raw(), 0, 0, 0],
        [ScriptOpcode::SetScreenWave.raw(), 4, 0xffff, 0],
        [ScriptOpcode::ShowFbp.raw(), 9, 2, 0],
        [ScriptOpcode::ToggleDayNightPalette.raw(), 0, 0, 0],
        [ScriptOpcode::SetPalette.raw(), 3, 0, 0],
        [ScriptOpcode::FadeColor.raw(), 0x4f, 1, 2],
        [ScriptOpcode::RestoreScreen.raw(), 0, 0, 0],
        [ScriptOpcode::FadeSceneWithUpdate.raw(), 0xfffe, 0, 0],
        [ScriptOpcode::FadeToCurrentScene.raw(), 0, 0, 0],
        [ScriptOpcode::ScrollFbp.raw(), 6, 0, 4],
        [ScriptOpcode::ShowFbpWithSprite.raw(), 8, 0xffff, 3],
        [ScriptOpcode::PlayEndingAnimation.raw(), 0, 0, 0],
        [ScriptOpcode::BackupScreen.raw(), 0, 0, 0],
        [ScriptOpcode::AutoScriptNoOp.raw(), 0, 0, 0],
        [ScriptOpcode::WaitForKey.raw(), 0, 0, 0],
        [ScriptOpcode::Stop.raw(), 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Visual(ScriptVisual::Shake {
            frames: 3,
            level: 4
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Visual(ScriptVisual::PlayRng {
            animation: 7,
            start_frame: 2,
            end_frame: Some(5),
            speed: 16,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Visual(ScriptVisual::FadeOut { speed: 1 }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Visual(ScriptVisual::FadeIn { speed: 1 }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Visual(ScriptVisual::SetNightPalette {
            night: true
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Visual(ScriptVisual::SetScreenWave {
            level: 4,
            progression: -1,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Visual(ScriptVisual::ShowFbp {
            index: 9,
            fade: 2,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Visual(ScriptVisual::ToggleDayNightPalette {
            update_scene: true
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Visual(ScriptVisual::SetPalette { index: 3 }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Visual(ScriptVisual::FadeColor {
            color: 0x4f,
            from_color: true,
            delay: 2,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Visual(ScriptVisual::RestoreScreen))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Visual(ScriptVisual::FadeSceneWithUpdate {
            step: -2
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Visual(ScriptVisual::FadeToCurrentScene {
            speed: 2
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Visual(ScriptVisual::ScrollFbp {
            index: 6,
            speed: 4,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Visual(ScriptVisual::ShowFbpWithSprite {
            index: 8,
            sprite: None,
            fade: 3,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Visual(ScriptVisual::PlayEndingAnimation))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Visual(ScriptVisual::BackupScreen))
    );
    assert_eq!(runtime.advance(), Some(ScriptEvent::WaitForKey));
    assert!(matches!(
        runtime.advance(),
        Some(ScriptEvent::Completed { .. })
    ));
}

#[test]
fn load_and_quit_events_stop_the_active_script() {
    for (opcode, expected) in [
        (ScriptOpcode::LoadLastSave, ScriptEvent::LoadLastSave),
        (ScriptOpcode::QuitGame, ScriptEvent::QuitGame),
    ] {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [opcode.raw(), 0, 0, 0],
            [ScriptOpcode::Stop.raw(), 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(runtime.advance(), Some(expected));
        assert!(!runtime.is_active());
    }
}

#[test]
fn yields_messages_and_resumes_until_completion() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [0x003c, 0, 0, 0],
        [0xffff, 42, 0, 0],
        [0xffff, 43, 0, 0],
        [0, 0, 0, 0],
    ]));
    assert!(runtime.start(trigger(1)));
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
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Message {
            message_id: 43,
            position: DialogPosition::Upper,
            font_color: 0x4f,
            face_index: None,
            playing_rng: false,
        })
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Completed {
            trigger: trigger(1),
            next_entry: 1,
            succeeded: true,
        })
    );
    assert!(!runtime.is_active());
}

#[test]
fn dialog_opcodes_preserve_face_and_font_color() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [0x003c, 5, 0x2d, 0xffff],
        [0xffff, 42, 0, 0],
        [0x003d, 6, 0x1a, 0],
        [0xffff, 43, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Message {
            message_id: 42,
            position: DialogPosition::Upper,
            font_color: 0x2d,
            face_index: Some(5),
            playing_rng: true,
        })
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Message {
            message_id: 43,
            position: DialogPosition::Lower,
            font_color: 0x1a,
            face_index: Some(6),
            playing_rng: true,
        })
    );
}

#[test]
fn center_window_is_single_message_and_restores_default_upper_dialog() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [0x003e, 0, 0, 0],
        [0xffff, 42, 0, 0],
        [0xffff, 43, 0, 0],
    ]));
    runtime.start(trigger(1));

    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Message {
            message_id: 42,
            position: DialogPosition::CenterWindow,
            font_color: 0x4f,
            face_index: None,
            playing_rng: false,
        })
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Message {
            message_id: 43,
            position: DialogPosition::Upper,
            font_color: 0x4f,
            face_index: None,
            playing_rng: false,
        })
    );
}

#[test]
fn redraw_delay_does_not_report_a_scene_updating_wait() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [0x0005, 0, 0, 0],
        [0xffff, 12, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert_eq!(runtime.advance(), Some(ScriptEvent::Delay));
    assert_eq!(runtime.advance(), Some(ScriptEvent::Delay));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Message {
            message_id: 12,
            position: DialogPosition::Upper,
            font_color: 0x4f,
            face_index: None,
            playing_rng: false,
        })
    );
}

#[test]
fn yields_buy_and_sell_menus() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [0x0026, 3, 0, 0],
        [0x0027, 0, 0, 0],
        [0, 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::OpenBuyMenu { store_number: 3 })
    );
    assert_eq!(runtime.advance(), Some(ScriptEvent::OpenSellMenu));
    assert!(matches!(
        runtime.advance(),
        Some(ScriptEvent::Completed { .. })
    ));
}

#[test]
fn yields_scene_fade_and_continues_at_the_next_instruction() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [0x0073, 4, 0x48, 0],
        [0xffff, 42, 0, 0],
        [0, 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert_eq!(runtime.advance(), Some(ScriptEvent::FadeScene { speed: 4 }));
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
fn yields_music_and_sound_actions() {
    let mut runtime = ScriptRuntime::new(table(&[
        [0, 0, 0, 0],
        [0x0043, 7, 3, 0],
        [0x0043, 9, 1, 0],
        [0x0047, 12, 0, 0],
        [0x0077, 0, 0, 0],
        [0, 0, 0, 0],
    ]));
    runtime.start(trigger(1));
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::PlayMusic {
            music_id: 7,
            looped: true,
            fade_seconds: 3,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::PlayMusic {
            music_id: 9,
            looped: false,
            fade_seconds: 0,
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::PlaySound {
            sound_id: 12
        }))
    );
    assert_eq!(
        runtime.advance(),
        Some(ScriptEvent::Action(ScriptAction::PlayMusic {
            music_id: 0,
            looped: false,
            fade_seconds: 2,
        }))
    );
}
