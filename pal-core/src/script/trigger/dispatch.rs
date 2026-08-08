use super::{InstructionFlow, ScriptInstructionDebug, ScriptRuntime};
use crate::script::executor::{decode_instruction, DecodeError};
use crate::script::opcode::TriggerHandler;
use crate::script::ScriptEvent;

const MAX_INSTRUCTIONS_PER_ADVANCE: usize = 1024;

impl ScriptRuntime {
    /// Execute until a message, completion, or unsupported instruction yields control.
    pub fn advance(&mut self) -> Option<ScriptEvent> {
        if self.pending_battle.is_some() {
            return None;
        }
        let mut execution = self.execution?;
        if execution.wait_frames > 0 {
            execution.wait_frames -= 1;
            self.execution = Some(execution);
            return Some(if execution.wait_updates_auto_scripts {
                ScriptEvent::Waiting
            } else {
                ScriptEvent::Delay
            });
        }
        for _ in 0..MAX_INSTRUCTIONS_PER_ADVANCE {
            let decoded = match decode_instruction(&self.table, execution.entry) {
                Ok(decoded) => decoded,
                Err(DecodeError::InvalidEntry { entry }) => {
                    self.execution = None;
                    return Some(ScriptEvent::InvalidEntry {
                        trigger: execution.trigger,
                        entry,
                    });
                }
                Err(DecodeError::Unsupported { entry, opcode }) => {
                    self.execution = None;
                    return Some(ScriptEvent::Unsupported {
                        trigger: execution.trigger,
                        entry,
                        opcode,
                    });
                }
            };
            let entry = decoded.instruction;
            self.last_instruction = Some(ScriptInstructionDebug {
                object_id: execution.object_id,
                entry: decoded.entry,
                opcode: entry.opcode,
                operands: entry.operands,
            });
            let opcode = decoded.opcode;
            let flow = match opcode.trigger_handler() {
                TriggerHandler::Control => self.dispatch_control(execution, entry, opcode),
                TriggerHandler::Presentation => {
                    self.dispatch_presentation(execution, entry, opcode)
                }
                TriggerHandler::Scene => self.dispatch_scene(execution, entry, opcode),
                TriggerHandler::Role => self.dispatch_role(execution, entry, opcode),
                TriggerHandler::Battle => self.dispatch_battle(execution, entry, opcode),
                TriggerHandler::Condition => self.dispatch_condition(execution, entry, opcode),
            };
            match flow {
                InstructionFlow::Continue(next) => execution = next,
                InstructionFlow::Yield(next, event) => {
                    self.execution = Some(next);
                    return Some(event);
                }
                InstructionFlow::Halt(event) => {
                    self.execution = None;
                    return Some(event);
                }
            }
        }

        self.execution = None;
        self.call_stack.clear();
        Some(ScriptEvent::InstructionLimit {
            trigger: execution.trigger,
            entry: execution.entry,
        })
    }
}
