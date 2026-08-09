//! Scene trigger-opcode handling.

use pal_assets::script::ScriptEntry;

use super::decode::{optional_direction, selected_object};
use super::{Execution, InstructionFlow, ScriptRuntime};
use crate::role::Direction;
use crate::script::{ScriptAction, ScriptEvent, ScriptOpcode};

impl ScriptRuntime {
    pub(super) fn dispatch_scene(
        &mut self,
        mut execution: Execution,
        entry: ScriptEntry,
        opcode: ScriptOpcode,
    ) -> InstructionFlow {
        use ScriptOpcode::*;
        match opcode {
            RideObjectSlow | RideObject | RideObjectFast => {
                let repeat_entry = execution.entry;
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::RideObjectTo {
                        object_id: execution.object_id,
                        tile_x: entry.operands[0],
                        tile_y: entry.operands[1],
                        half: entry.operands[2],
                        speed: match opcode {
                            RideObjectSlow => 2,
                            RideObject => 4,
                            _ => 8,
                        },
                        repeat_entry,
                    }),
                );
            }
            WalkObjectSouth | WalkObjectWest | WalkObjectNorth | WalkObjectEast => {
                let direction = Direction::from_pal(opcode.raw() - WalkObjectSouth.raw())
                    .expect("walk opcodes always encode a valid direction");
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::MoveObject {
                        object_id: execution.object_id,
                        direction,
                    }),
                );
            }
            SetObjectPose => {
                let direction = optional_direction(entry.operands[0]);
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::SetObjectPose {
                        object_id: execution.object_id,
                        direction,
                        frame: (entry.operands[1] != 0xffff).then_some(entry.operands[1]),
                    }),
                );
            }
            SetObjectPosition => {
                let object_id = selected_object(entry.operands[0], execution.object_id);
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::SetObjectPosition {
                        object_id,
                        x: i32::from(entry.operands[1]),
                        y: i32::from(entry.operands[2]),
                    }),
                );
            }
            WalkObjectTo | WalkObjectToSlow => {
                let repeat_entry = execution.entry;
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::WalkObjectTo {
                        object_id: execution.object_id,
                        tile_x: entry.operands[0],
                        tile_y: entry.operands[1],
                        half: entry.operands[2],
                        speed: if opcode == WalkObjectTo { 3 } else { 2 },
                        repeat_entry,
                    }),
                );
            }
            SetObjectPositionRelative => {
                let object_id = selected_object(entry.operands[0], execution.object_id);
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::SetObjectPositionRelativeToPlayer {
                        object_id,
                        dx: i32::from(entry.operands[1] as i16),
                        dy: i32::from(entry.operands[2] as i16),
                    }),
                );
            }
            SetObjectGesture => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::SetObjectPose {
                        object_id: execution.object_id,
                        direction: Some(Direction::South),
                        frame: Some(entry.operands[0]),
                    }),
                );
            }
            SetPartyMemberPose => {
                let Some(direction) = Direction::from_pal(entry.operands[0]) else {
                    return InstructionFlow::Halt(ScriptEvent::Unsupported {
                        trigger: execution.trigger,
                        entry: execution.entry,
                        opcode: entry.opcode,
                    });
                };
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::SetPlayerPose {
                        direction,
                        frame: u8::try_from(entry.operands[1]).unwrap_or(u8::MAX),
                        party_index: entry.operands[2],
                    }),
                );
            }
            SetSelectedObjectPose if entry.operands[0] != 0 => {
                let object_id = selected_object(entry.operands[0], execution.object_id);
                let Some(direction) = Direction::from_pal(entry.operands[1]) else {
                    return InstructionFlow::Halt(ScriptEvent::Unsupported {
                        trigger: execution.trigger,
                        entry: execution.entry,
                        opcode: entry.opcode,
                    });
                };
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::SetObjectPose {
                        object_id,
                        direction: Some(direction),
                        frame: Some(entry.operands[2]),
                    }),
                );
            }
            SetSelectedObjectPose => execution.entry = execution.entry.wrapping_add(1),
            PauseEnemyChase | SpeedUpEnemyChase => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::SetEnemyChase {
                        range: if opcode == PauseEnemyChase { 0 } else { 3 },
                        cycles: entry.operands[0],
                    }),
                );
            }
            SetObjectScript if entry.operands[2] <= 2 => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::SetObjectScript {
                        object_id: entry.operands[0],
                        script_entry: entry.operands[1],
                        field: entry.operands[2],
                    }),
                );
            }
            TeleportParty => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Teleport {
                        failure_entry: entry.operands[0],
                    },
                );
            }
            SetObjectAutoScript if entry.operands[0] != 0 => {
                let object_id = selected_object(entry.operands[0], execution.object_id);
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::SetObjectAutoScript {
                        object_id,
                        script_entry: entry.operands[1],
                    }),
                );
            }
            SetObjectAutoScript => execution.entry = execution.entry.wrapping_add(1),
            SetObjectTriggerScript if entry.operands[0] != 0 => {
                let object_id = selected_object(entry.operands[0], execution.object_id);
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::SetObjectTriggerScript {
                        object_id,
                        script_entry: entry.operands[1],
                    }),
                );
            }
            SetObjectTriggerScript => execution.entry = execution.entry.wrapping_add(1),
            SetObjectState if entry.operands[0] != 0 => {
                let object_id = selected_object(entry.operands[0], execution.object_id);
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::SetObjectState {
                        object_id,
                        state: entry.operands[1] as i16,
                    }),
                );
            }
            SetObjectState => execution.entry = execution.entry.wrapping_add(1),
            HideObjectShort => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::SetObjectVanishTime {
                        object_id: execution.object_id,
                        vanish_time: -15,
                    }),
                );
            }
            HideObject => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::HideObjectTemporarily {
                        object_id: execution.object_id,
                        vanish_time: if entry.operands[0] == 0 {
                            800
                        } else {
                            entry.operands[0] as i16
                        },
                    }),
                );
            }
            SetPartyPosition => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::SetPlayerPosition {
                        tile_x: entry.operands[0],
                        tile_y: entry.operands[1],
                        half: entry.operands[2],
                    }),
                );
            }
            ChangeScene => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::ChangeScene {
                        scene_number: entry.operands[0],
                    }),
                );
            }
            OffsetObjectAndAnimate => {
                let object_id = selected_object(entry.operands[0], execution.object_id);
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::OffsetObject {
                        object_id,
                        dx: i32::from(entry.operands[1] as i16),
                        dy: i32::from(entry.operands[2] as i16),
                    }),
                );
            }
            SetSceneScripts if entry.operands[0] != 0 => {
                execution.advance();
                let clear = entry.operands[1] == 0 && entry.operands[2] == 0;
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::SetSceneScripts {
                        scene_number: entry.operands[0],
                        enter_script: (clear || entry.operands[1] != 0)
                            .then_some(entry.operands[1]),
                        teleport_script: (clear || entry.operands[2] != 0)
                            .then_some(entry.operands[2]),
                    }),
                );
            }
            SetSceneScripts => execution.entry = execution.entry.wrapping_add(1),
            OffsetParty => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::OffsetPlayer {
                        dx: i32::from(entry.operands[0] as i16),
                        dy: i32::from(entry.operands[1] as i16),
                        layer: entry.operands[2].wrapping_mul(8),
                    }),
                );
            }
            WalkParty => {
                let repeat_entry = execution.entry;
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::WalkPlayerTo {
                        tile_x: entry.operands[0],
                        tile_y: entry.operands[1],
                        half: entry.operands[2],
                        speed: 2,
                        repeat_entry,
                    }),
                );
            }
            WalkPartyFast | WalkPartyFastest => {
                let repeat_entry = execution.entry;
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::WalkPlayerTo {
                        tile_x: entry.operands[0],
                        tile_y: entry.operands[1],
                        half: entry.operands[2],
                        speed: if opcode == WalkPartyFast { 4 } else { 8 },
                        repeat_entry,
                    }),
                );
            }
            WalkObjectHalfSpeed | WalkObjectFast => {
                let repeat_entry = execution.entry;
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::WalkObjectTo {
                        object_id: execution.object_id,
                        tile_x: entry.operands[0],
                        tile_y: entry.operands[1],
                        half: entry.operands[2],
                        // 0x007c moves four pixels every other original frame.
                        speed: if opcode == WalkObjectHalfSpeed { 2 } else { 8 },
                        repeat_entry,
                    }),
                );
            }
            OffsetObject => {
                let object_id = selected_object(entry.operands[0], execution.object_id);
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::MoveObjectBy {
                        object_id,
                        dx: i32::from(entry.operands[1] as i16),
                        dy: i32::from(entry.operands[2] as i16),
                    }),
                );
            }
            SetObjectLayer => {
                let object_id = selected_object(entry.operands[0], execution.object_id);
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::SetObjectLayer {
                        object_id,
                        layer: entry.operands[1] as i16,
                    }),
                );
            }
            MoveViewport => {
                let frames = entry.operands[2] as i16;
                if (entry.operands[0] == 0 && entry.operands[1] == 0) || frames == -1 {
                    execution.advance();
                } else {
                    if execution.viewport_frames_remaining == 0 {
                        execution.viewport_frames_remaining =
                            u16::try_from(frames).unwrap_or(1).max(1);
                    }
                    execution.viewport_frames_remaining -= 1;
                    if execution.viewport_frames_remaining == 0 {
                        execution.advance();
                    }
                }
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::MoveViewport {
                        x: entry.operands[0] as i16,
                        y: entry.operands[1] as i16,
                        frames: if frames == -1 { -1 } else { 1 },
                    }),
                );
            }
            PlaceUsedItemObject => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::PlaceObjectInFront {
                        object_id: entry.operands[0],
                        state: entry.operands[1] as i16,
                        blocked_entry: entry.operands[2],
                    }),
                );
            }
            SyncObjectState => {
                let source_object_id = selected_object(entry.operands[0], execution.object_id);
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::SyncObjectState {
                        object_id: execution.object_id,
                        source_object_id,
                        state: entry.operands[1] as i16,
                    }),
                );
            }
            AnimateObject => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::AnimateObject {
                        object_id: execution.object_id,
                    }),
                );
            }
            SetSceneMap => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::SetSceneMap {
                        scene_number: (entry.operands[0] != 0xffff).then_some(entry.operands[0]),
                        map_number: entry.operands[1],
                    }),
                );
            }
            SetObjectStates if entry.operands[0] != 0 && entry.operands[0] <= entry.operands[1] => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::SetObjectStates {
                        first_object_id: entry.operands[0],
                        last_object_id: entry.operands[1],
                        state: entry.operands[2] as i16,
                    }),
                );
            }
            SetObjectStates if entry.operands[0] > entry.operands[1] => execution.advance(),
            SetObjectTriggerMode if entry.operands[0] != 0 => {
                let object_id = selected_object(entry.operands[0], execution.object_id);
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::SetObjectTriggerMode {
                        object_id,
                        trigger_mode: entry.operands[1],
                    }),
                );
            }
            SetObjectTriggerMode => execution.entry = execution.entry.wrapping_add(1),
            ChasePlayer => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::ChaseObject {
                        object_id: execution.object_id,
                        speed: if entry.operands[1] == 0 {
                            4
                        } else {
                            entry.operands[1]
                        },
                        range: if entry.operands[0] == 0 {
                            8
                        } else {
                            entry.operands[0]
                        },
                        floating: entry.operands[2] != 0,
                    }),
                );
            }
            // Implemented instructions with malformed operands are rejected explicitly.
            SetObjectScript => {
                return InstructionFlow::Halt(ScriptEvent::Unsupported {
                    trigger: execution.trigger,
                    entry: execution.entry,
                    opcode: opcode.raw(),
                });
            }
            SetObjectStates => {
                return InstructionFlow::Halt(ScriptEvent::Unsupported {
                    trigger: execution.trigger,
                    entry: execution.entry,
                    opcode: opcode.raw(),
                });
            }
            _ => unreachable!("opcode {opcode} is not a scene instruction"),
        }
        InstructionFlow::Continue(execution)
    }
}
