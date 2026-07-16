//! Deterministic trigger-script execution and dialog yields.

use pal_assets::script::ScriptTable;

use crate::role::Direction;
use crate::scene::TriggerRequest;

const MAX_INSTRUCTIONS_PER_ADVANCE: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DialogPosition {
    Center,
    Upper,
    #[default]
    Lower,
    CenterWindow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptEvent {
    Message {
        message_id: u16,
        position: DialogPosition,
    },
    Waiting,
    Action(ScriptAction),
    Completed {
        trigger: TriggerRequest,
        next_entry: u16,
    },
    Unsupported {
        trigger: TriggerRequest,
        entry: u16,
        opcode: u16,
    },
    InvalidEntry {
        trigger: TriggerRequest,
        entry: u16,
    },
    InstructionLimit {
        trigger: TriggerRequest,
        entry: u16,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptAction {
    AddItem {
        item_id: u16,
        amount: i16,
    },
    PlayMusic {
        music_id: u16,
        looped: bool,
        fade_seconds: u8,
    },
    PlaySound {
        sound_id: u16,
    },
    MoveObject {
        object_id: u16,
        direction: Direction,
    },
    SetObjectPose {
        object_id: u16,
        direction: Option<Direction>,
        frame: Option<u16>,
    },
    SetObjectPosition {
        object_id: u16,
        x: i32,
        y: i32,
    },
    OffsetObject {
        object_id: u16,
        dx: i32,
        dy: i32,
    },
    SetObjectState {
        object_id: u16,
        state: i16,
    },
    SetPlayerPose {
        direction: Direction,
        frame: u8,
    },
    SetPlayerSprite {
        sprite_index: usize,
    },
    OffsetPlayer {
        dx: i32,
        dy: i32,
    },
    SetPlayerPosition {
        tile_x: u16,
        tile_y: u16,
        half: u16,
    },
    ChangeScene {
        scene_number: u16,
    },
    SetParty {
        /// Zero-based role IDs. Empty script slots are omitted.
        members: [Option<u16>; 3],
    },
}

#[derive(Debug, Clone, Copy)]
struct Execution {
    trigger: TriggerRequest,
    entry: u16,
    next_entry: u16,
    dialog_position: DialogPosition,
    wait_frames: u16,
}

pub struct ScriptRuntime {
    table: ScriptTable,
    execution: Option<Execution>,
}

impl ScriptRuntime {
    pub fn new(table: ScriptTable) -> Self {
        Self {
            table,
            execution: None,
        }
    }

    pub fn start(&mut self, trigger: TriggerRequest) -> bool {
        if self.execution.is_some() || trigger.script_entry == 0 {
            return false;
        }
        self.execution = Some(Execution {
            trigger,
            entry: trigger.script_entry,
            next_entry: trigger.script_entry,
            dialog_position: DialogPosition::Lower,
            wait_frames: 0,
        });
        true
    }

    pub fn is_active(&self) -> bool {
        self.execution.is_some()
    }

    /// Execute until a message, completion, or unsupported instruction yields control.
    pub fn advance(&mut self) -> Option<ScriptEvent> {
        let mut execution = self.execution?;
        if execution.wait_frames > 0 {
            execution.wait_frames -= 1;
            self.execution = Some(execution);
            return Some(ScriptEvent::Waiting);
        }
        for _ in 0..MAX_INSTRUCTIONS_PER_ADVANCE {
            let Some(entry) = self.table.entry(execution.entry).copied() else {
                self.execution = None;
                return Some(ScriptEvent::InvalidEntry {
                    trigger: execution.trigger,
                    entry: execution.entry,
                });
            };

            match entry.opcode {
                0x0000 => {
                    self.execution = None;
                    return Some(ScriptEvent::Completed {
                        trigger: execution.trigger,
                        next_entry: execution.next_entry,
                    });
                }
                0x0001 => {
                    execution.next_entry = execution.entry.wrapping_add(1);
                    self.execution = None;
                    return Some(ScriptEvent::Completed {
                        trigger: execution.trigger,
                        next_entry: execution.next_entry,
                    });
                }
                0x0003 => execution.entry = entry.operands[0],
                0x0005 | 0x0045 | 0x0050 | 0x008e => {
                    execution.entry = execution.entry.wrapping_add(1)
                }
                0x0008 => {
                    execution.entry = execution.entry.wrapping_add(1);
                    execution.next_entry = execution.entry;
                }
                0x0009 => {
                    execution.entry = execution.entry.wrapping_add(1);
                    execution.wait_frames = entry.operands[0].max(1) - 1;
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Waiting);
                }
                0x000b..=0x000e => {
                    let direction = Direction::from_pal(entry.opcode - 0x000b)
                        .expect("walk opcodes always encode a valid direction");
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::MoveObject {
                        object_id: execution.trigger.object_id,
                        direction,
                    }));
                }
                0x000f => {
                    let direction = optional_direction(entry.operands[0]);
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetObjectPose {
                        object_id: execution.trigger.object_id,
                        direction,
                        frame: (entry.operands[1] != 0xffff).then_some(entry.operands[1]),
                    }));
                }
                0x0013 => {
                    let object_id = selected_object(entry.operands[0], execution.trigger.object_id);
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetObjectPosition {
                        object_id,
                        x: i32::from(entry.operands[1]),
                        y: i32::from(entry.operands[2]),
                    }));
                }
                0x0014 => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetObjectPose {
                        object_id: execution.trigger.object_id,
                        direction: Some(Direction::South),
                        frame: Some(entry.operands[0]),
                    }));
                }
                0x0015 => {
                    let Some(direction) = Direction::from_pal(entry.operands[0]) else {
                        self.execution = None;
                        return Some(ScriptEvent::Unsupported {
                            trigger: execution.trigger,
                            entry: execution.entry,
                            opcode: entry.opcode,
                        });
                    };
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetPlayerPose {
                        direction,
                        frame: u8::try_from(entry.operands[1]).unwrap_or(u8::MAX),
                    }));
                }
                0x0016 if entry.operands[0] != 0 => {
                    let Some(direction) = Direction::from_pal(entry.operands[1]) else {
                        self.execution = None;
                        return Some(ScriptEvent::Unsupported {
                            trigger: execution.trigger,
                            entry: execution.entry,
                            opcode: entry.opcode,
                        });
                    };
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetObjectPose {
                        object_id: entry.operands[0],
                        direction: Some(direction),
                        frame: Some(entry.operands[2]),
                    }));
                }
                0x0016 => execution.entry = execution.entry.wrapping_add(1),
                0x001f => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::AddItem {
                        item_id: entry.operands[0],
                        amount: entry.operands[1] as i16,
                    }));
                }
                0x0043 => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::PlayMusic {
                        music_id: entry.operands[0],
                        looped: entry.operands[1] != 1,
                        fade_seconds: u8::from(entry.operands[1] == 3 && entry.operands[0] != 9)
                            * 3,
                    }));
                }
                0x0047 => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::PlaySound {
                        sound_id: entry.operands[0],
                    }));
                }
                0x0049 if entry.operands[0] != 0 => {
                    let object_id = selected_object(entry.operands[0], execution.trigger.object_id);
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetObjectState {
                        object_id,
                        state: entry.operands[1] as i16,
                    }));
                }
                0x0049 => execution.entry = execution.entry.wrapping_add(1),
                0x0046 => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetPlayerPosition {
                        tile_x: entry.operands[0],
                        tile_y: entry.operands[1],
                        half: entry.operands[2],
                    }));
                }
                0x0059 => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::ChangeScene {
                        scene_number: entry.operands[0],
                    }));
                }
                0x0065 if entry.operands[0] == 0 => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetPlayerSprite {
                        sprite_index: usize::from(entry.operands[1]),
                    }));
                }
                0x0065 => execution.entry = execution.entry.wrapping_add(1),
                0x006c => {
                    let object_id = selected_object(entry.operands[0], execution.trigger.object_id);
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::OffsetObject {
                        object_id,
                        dx: i32::from(entry.operands[1] as i16),
                        dy: i32::from(entry.operands[2] as i16),
                    }));
                }
                0x006e => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::OffsetPlayer {
                        dx: i32::from(entry.operands[0] as i16),
                        dy: i32::from(entry.operands[1] as i16),
                    }));
                }
                0x0070 => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetPlayerPosition {
                        tile_x: entry.operands[0],
                        tile_y: entry.operands[1],
                        half: entry.operands[2],
                    }));
                }
                0x0075 => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    let mut members = entry.operands.map(|role| role.checked_sub(1));
                    if members.iter().all(Option::is_none) {
                        members[0] = Some(0);
                    }
                    return Some(ScriptEvent::Action(ScriptAction::SetParty { members }));
                }
                0x003b => {
                    execution.dialog_position = DialogPosition::Center;
                    execution.entry = execution.entry.wrapping_add(1);
                }
                0x003c => {
                    execution.dialog_position = DialogPosition::Upper;
                    execution.entry = execution.entry.wrapping_add(1);
                }
                0x003d => {
                    execution.dialog_position = DialogPosition::Lower;
                    execution.entry = execution.entry.wrapping_add(1);
                }
                0x003e => {
                    execution.dialog_position = DialogPosition::CenterWindow;
                    execution.entry = execution.entry.wrapping_add(1);
                }
                0xffff => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Message {
                        message_id: entry.operands[0],
                        position: execution.dialog_position,
                    });
                }
                opcode => {
                    self.execution = None;
                    return Some(ScriptEvent::Unsupported {
                        trigger: execution.trigger,
                        entry: execution.entry,
                        opcode,
                    });
                }
            }
        }

        self.execution = None;
        Some(ScriptEvent::InstructionLimit {
            trigger: execution.trigger,
            entry: execution.entry,
        })
    }
}

fn selected_object(selector: u16, current: u16) -> u16 {
    if selector == 0 || selector == 0xffff {
        current
    } else {
        selector
    }
}

fn optional_direction(value: u16) -> Option<Direction> {
    (value != 0xffff)
        .then(|| Direction::from_pal(value))
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::TriggerKind;

    fn table(entries: &[[u16; 4]]) -> ScriptTable {
        let data = entries
            .iter()
            .flat_map(|entry| entry.iter().flat_map(|value| value.to_le_bytes()))
            .collect::<Vec<_>>();
        ScriptTable::parse(&data).unwrap()
    }

    fn trigger(entry: u16) -> TriggerRequest {
        TriggerRequest {
            object_id: 7,
            script_entry: entry,
            kind: TriggerKind::Search,
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
            })
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Message {
                message_id: 43,
                position: DialogPosition::Upper,
            })
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Completed {
                trigger: trigger(1),
                next_entry: 1,
            })
        );
        assert!(!runtime.is_active());
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
            })
        );
    }

    #[test]
    fn reports_unsupported_and_invalid_entries() {
        let mut runtime = ScriptRuntime::new(table(&[[0, 0, 0, 0], [0x42, 0, 0, 0]]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Unsupported {
                trigger: trigger(1),
                entry: 1,
                opcode: 0x42,
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
    fn yields_music_and_sound_actions() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x0043, 7, 3, 0],
            [0x0043, 9, 1, 0],
            [0x0047, 12, 0, 0],
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
    }
}
