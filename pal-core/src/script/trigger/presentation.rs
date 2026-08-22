//! Presentation trigger-opcode handling.

use pal_assets::script::ScriptEntry;

use super::decode::delay_60ms_ticks;
use super::{Execution, InstructionFlow, ScriptRuntime};
use crate::script::{DialogPosition, ScriptAction, ScriptEvent, ScriptOpcode, ScriptVisual};

impl ScriptRuntime {
    pub(super) fn dispatch_presentation(
        &mut self,
        mut execution: Execution,
        entry: ScriptEntry,
        opcode: ScriptOpcode,
    ) -> InstructionFlow {
        use ScriptOpcode::*;
        match opcode {
            Redraw => {
                execution.advance();
                execution.wait_frames =
                    u32::from(delay_60ms_ticks(entry.operands[1]).saturating_sub(1));
                execution.wait_updates_auto_scripts = false;
                execution.wait_processes_triggers = false;
                execution.wait_updates_party_gestures = false;
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Redraw {
                        update_party_gestures: entry.operands[2] != 0,
                    },
                );
            }
            ShakeScreen => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Visual(ScriptVisual::Shake {
                        frames: entry.operands[0],
                        level: if entry.operands[1] == 0 {
                            4
                        } else {
                            entry.operands[1]
                        },
                    }),
                );
            }
            SelectRngAnimation => {
                self.current_rng = entry.operands[0];
                execution.advance();
            }
            PlayRngAnimation => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Visual(ScriptVisual::PlayRng {
                        animation: self.current_rng,
                        start_frame: entry.operands[0],
                        end_frame: (entry.operands[1] != 0).then_some(entry.operands[1]),
                        speed: if entry.operands[2] == 0 {
                            16
                        } else {
                            entry.operands[2]
                        },
                    }),
                );
            }
            WaitForKey => {
                execution.advance();
                return InstructionFlow::Yield(execution, ScriptEvent::WaitForKey);
            }
            LoadLastSave => {
                self.call_stack.clear();
                return InstructionFlow::Halt(ScriptEvent::LoadLastSave);
            }
            FadeToRed => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Visual(ScriptVisual::FadeToRed),
                );
            }
            FadeOut => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Visual(ScriptVisual::FadeOut {
                        speed: entry.operands[0].max(1),
                    }),
                );
            }
            FadeIn => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Visual(ScriptVisual::FadeIn {
                        speed: (entry.operands[0] as i16).max(1) as u16,
                    }),
                );
            }
            UseDayPalette | UseNightPalette => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Visual(ScriptVisual::SetNightPalette {
                        night: opcode == UseNightPalette,
                    }),
                );
            }
            RestoreScreen => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Visual(ScriptVisual::RestoreScreen),
                );
            }
            Confirm => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Confirm {
                        no_entry: entry.operands[0],
                    },
                );
            }
            OpenBuyMenu => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::OpenBuyMenu {
                        store_number: entry.operands[0],
                    },
                );
            }
            OpenSellMenu => {
                execution.advance();
                return InstructionFlow::Yield(execution, ScriptEvent::OpenSellMenu);
            }
            PlayMusic => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::PlayMusic {
                        music_id: entry.operands[0],
                        looped: entry.operands[1] != 1,
                        fade_seconds: u8::from(entry.operands[1] == 3 && entry.operands[0] != 9)
                            * 3,
                    }),
                );
            }
            PlaySound => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::PlaySound {
                        sound_id: entry.operands[0],
                    }),
                );
            }
            SetScreenWave => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Visual(ScriptVisual::SetScreenWave {
                        level: entry.operands[0],
                        progression: entry.operands[1] as i16,
                    }),
                );
            }
            FadeScene => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::FadeScene {
                        speed: entry.operands[0],
                    },
                );
            }
            ShowFbp => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Visual(ScriptVisual::ShowFbp {
                        index: entry.operands[0],
                        fade: entry.operands[1],
                    }),
                );
            }
            StopMusic => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::PlayMusic {
                        music_id: 0,
                        looped: false,
                        fade_seconds: if entry.operands[0] == 0 {
                            2
                        } else {
                            u8::try_from(entry.operands[0].saturating_mul(3)).unwrap_or(u8::MAX)
                        },
                    }),
                );
            }
            ToggleDayNightPalette => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Visual(ScriptVisual::ToggleDayNightPalette {
                        update_scene: entry.operands[0] == 0,
                    }),
                );
            }
            SetPalette => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Visual(ScriptVisual::SetPalette {
                        index: entry.operands[0],
                    }),
                );
            }
            FadeColor => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Visual(ScriptVisual::FadeColor {
                        color: entry.operands[0] as u8,
                        from_color: entry.operands[2] != 0,
                        delay: entry.operands[1],
                    }),
                );
            }
            FadeSceneWithUpdate => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Visual(ScriptVisual::FadeSceneWithUpdate {
                        step: entry.operands[0] as i16,
                    }),
                );
            }
            FadeToCurrentScene => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Visual(ScriptVisual::FadeToCurrentScene { speed: 2 }),
                );
            }
            QuitGame => {
                self.call_stack.clear();
                return InstructionFlow::Halt(ScriptEvent::QuitGame);
            }
            ScrollFbp => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Visual(ScriptVisual::ScrollFbp {
                        index: entry.operands[0],
                        speed: entry.operands[2],
                    }),
                );
            }
            ShowFbpWithSprite => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Visual(ScriptVisual::ShowFbpWithSprite {
                        index: entry.operands[0],
                        sprite: (entry.operands[1] != 0xffff).then_some(entry.operands[1]),
                        fade: entry.operands[2],
                    }),
                );
            }
            PlayEndingAnimation => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Visual(ScriptVisual::PlayEndingAnimation),
                );
            }
            BackupScreen => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Visual(ScriptVisual::BackupScreen),
                );
            }
            PlayCdMusic => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::PlayMusic {
                        music_id: entry.operands[1],
                        looped: true,
                        fade_seconds: 0,
                    }),
                );
            }
            DialogCenter => {
                execution.dialog_position = DialogPosition::Center;
                if entry.operands[0] != 0 {
                    execution.dialog_color = entry.operands[0] as u8;
                }
                execution.dialog_face = None;
                execution.advance();
            }
            DialogUpper => {
                execution.dialog_position = DialogPosition::Upper;
                if entry.operands[1] != 0 {
                    execution.dialog_color = entry.operands[1] as u8;
                }
                execution.dialog_face = (entry.operands[0] != 0).then_some(entry.operands[0]);
                execution.dialog_playing_rng |=
                    entry.operands[2] != 0 && execution.dialog_face.is_some();
                execution.advance();
            }
            DialogLower => {
                execution.dialog_position = DialogPosition::Lower;
                if entry.operands[1] != 0 {
                    execution.dialog_color = entry.operands[1] as u8;
                }
                execution.dialog_face = (entry.operands[0] != 0).then_some(entry.operands[0]);
                execution.dialog_playing_rng |=
                    entry.operands[2] != 0 && execution.dialog_face.is_some();
                execution.advance();
            }
            DialogCenterWindow => {
                execution.dialog_position = DialogPosition::CenterWindow;
                if entry.operands[0] != 0 {
                    execution.dialog_color = entry.operands[0] as u8;
                }
                execution.dialog_face = None;
                execution.advance();
            }
            PrintMessage => {
                let position = execution.dialog_position;
                let font_color = execution.dialog_color;
                let face_index = execution.dialog_face;
                let playing_rng = execution.dialog_playing_rng;
                execution.advance();
                // A center-window message calls PAL_EndDialog immediately,
                // which restores the original upper/default dialog state.
                if position == DialogPosition::CenterWindow {
                    execution.dialog_position = DialogPosition::Upper;
                    execution.dialog_color = 0x4f;
                    execution.dialog_face = None;
                    execution.dialog_playing_rng = false;
                }
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Message {
                        message_id: entry.operands[0],
                        position,
                        font_color,
                        face_index,
                        playing_rng,
                    },
                );
            }
            // Equipment opcodes reject malformed slot operands explicitly.
            _ => unreachable!("opcode {opcode} is not a presentation instruction"),
        }
        InstructionFlow::Continue(execution)
    }
}
