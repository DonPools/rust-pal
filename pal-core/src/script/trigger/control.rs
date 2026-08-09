//! Control trigger-opcode handling.

use pal_assets::script::ScriptEntry;

use super::decode::delay_80ms_ticks;
use super::{Execution, InstructionFlow, ScriptRuntime};
use crate::script::executor::ScriptCallFrame;
use crate::script::{ScriptEvent, ScriptOpcode};

impl ScriptRuntime {
    pub(super) fn dispatch_control(
        &mut self,
        mut execution: Execution,
        entry: ScriptEntry,
        opcode: ScriptOpcode,
    ) -> InstructionFlow {
        use ScriptOpcode::*;
        match opcode {
            Stop => {
                if let Some(frame) = self.call_stack.pop() {
                    execution.resume_call(frame);
                    return InstructionFlow::Continue(execution);
                }
                return InstructionFlow::Halt(ScriptEvent::Completed {
                    trigger: execution.trigger,
                    next_entry: execution.next_entry,
                    succeeded: execution.succeeded,
                });
            }
            StopAndAdvance => {
                let next_entry = execution.entry.wrapping_add(1);
                if let Some(frame) = self.call_stack.pop() {
                    execution.resume_call(frame);
                    return InstructionFlow::Continue(execution);
                }
                execution.next_entry = next_entry;
                return InstructionFlow::Halt(ScriptEvent::Completed {
                    trigger: execution.trigger,
                    next_entry: execution.next_entry,
                    succeeded: execution.succeeded,
                });
            }
            StopAndReplace => {
                if self.idle_branch(execution.object_id, entry.operands[1]) {
                    let next_entry = entry.operands[0];
                    if let Some(frame) = self.call_stack.pop() {
                        execution.resume_call(frame);
                        return InstructionFlow::Continue(execution);
                    }
                    execution.next_entry = next_entry;
                    return InstructionFlow::Halt(ScriptEvent::Completed {
                        trigger: execution.trigger,
                        next_entry,
                        succeeded: execution.succeeded,
                    });
                }
                execution.advance();
            }
            Jump => {
                execution.entry = if self.idle_branch(execution.object_id, entry.operands[1]) {
                    entry.operands[0]
                } else {
                    execution.entry.wrapping_add(1)
                };
            }
            Call => {
                self.call_stack.push(ScriptCallFrame {
                    object_id: execution.object_id,
                    return_entry: execution.entry.wrapping_add(1),
                    wait_frames: execution.wait_frames,
                    wait_updates_auto_scripts: execution.wait_updates_auto_scripts,
                    wait_processes_triggers: execution.wait_processes_triggers,
                    wait_updates_party_gestures: execution.wait_updates_party_gestures,
                    viewport_frames_remaining: execution.viewport_frames_remaining,
                });
                let requested_object = if entry.operands[1] == 0 {
                    execution.object_id
                } else {
                    entry.operands[1]
                };
                execution.object_id = self.resolve_trigger_object(requested_object);
                execution.entry = entry.operands[0];
            }
            AdvanceEntry => {
                execution.advance();
                execution.next_entry = execution.entry;
            }
            WaitFrames => {
                execution.advance();
                execution.wait_frames = u32::from(entry.operands[0].max(1)) * 2;
                execution.wait_updates_auto_scripts = true;
                execution.wait_processes_triggers = entry.operands[1] != 0;
                execution.wait_updates_party_gestures = entry.operands[2] != 0;
                // The first 50 ms half-frame only holds the current image. The
                // following half completes Classic's 100 ms scene frame.
                return InstructionFlow::Yield(execution, ScriptEvent::Delay);
            }
            JumpByChance => {
                let roll = self.next_random_percent();
                execution.entry = if roll >= entry.operands[0] {
                    entry.operands[1]
                } else {
                    execution.entry.wrapping_add(1)
                };
            }
            NoOp => execution.entry = execution.entry.wrapping_add(1),
            RandomSelect if entry.operands[0] != 0 => {
                let choices = entry.operands[0];
                let choice = self.next_random_percent().wrapping_sub(1) % choices;
                execution.entry = execution.entry.wrapping_add(choice).wrapping_add(1);
            }
            AutoScriptNoOp => execution.entry = execution.entry.wrapping_add(1),
            MarkScriptFailed => {
                execution.succeeded = false;
                execution.advance();
            }
            Delay => {
                execution.advance();
                execution.wait_frames =
                    u32::from(delay_80ms_ticks(entry.operands[0]).saturating_sub(1));
                execution.wait_updates_auto_scripts = false;
                execution.wait_processes_triggers = false;
                execution.wait_updates_party_gestures = false;
                return InstructionFlow::Yield(execution, ScriptEvent::Delay);
            }
            RandomSelect => {
                return InstructionFlow::Halt(ScriptEvent::Unsupported {
                    trigger: execution.trigger,
                    entry: execution.entry,
                    opcode: opcode.raw(),
                });
            }
            _ => unreachable!("opcode {opcode} is not a control instruction"),
        }
        InstructionFlow::Continue(execution)
    }
}
