use super::{
    InstructionFlow, ScriptInstructionDebug, ScriptRuntime, ScriptTraceEvent, ScriptTraceOutcome,
};
use crate::script::executor::{decode_instruction, DecodeError};
use crate::script::opcode::TriggerHandler;
use crate::script::ScriptEvent;

const MAX_INSTRUCTIONS_PER_ADVANCE: usize = 1024;
pub(super) const SCRIPT_TRACE_CAPACITY: usize = 512;

impl ScriptRuntime {
    /// Execute until a message, completion, or unsupported instruction yields control.
    pub fn advance(&mut self) -> Option<ScriptEvent> {
        if self.debug_break_armed && self.execution.is_some() {
            self.debug_break_armed = false;
            self.debug_paused = true;
            return None;
        }
        let single_step = if self.debug_paused {
            if !std::mem::take(&mut self.debug_step_requested) {
                return None;
            }
            true
        } else {
            false
        };
        let result = self.advance_with_limit(if single_step {
            1
        } else {
            MAX_INSTRUCTIONS_PER_ADVANCE
        });
        if single_step {
            if self.execution.is_none() {
                self.debug_paused = false;
            } else if result.is_some() {
                // Let the host finish a yielded dialog, visual, menu, action, or
                // wait, then break again immediately before the next runtime unit.
                self.debug_paused = false;
                self.debug_break_armed = true;
            } else {
                self.debug_paused = true;
            }
        }
        result
    }

    fn advance_with_limit(&mut self, instruction_limit: usize) -> Option<ScriptEvent> {
        if self.pending_battle.is_some() {
            return None;
        }
        let mut execution = self.execution?;
        if execution.wait_frames > 0 {
            execution.wait_frames -= 1;
            self.execution = Some(execution);
            return Some(
                if execution.wait_updates_auto_scripts && execution.wait_frames.is_multiple_of(2) {
                    ScriptEvent::Waiting {
                        process_triggers: execution.wait_processes_triggers,
                        update_party_gestures: execution.wait_updates_party_gestures,
                    }
                } else {
                    ScriptEvent::Delay
                },
            );
        }
        for _ in 0..instruction_limit {
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
            let instruction = ScriptInstructionDebug {
                object_id: execution.object_id,
                entry: decoded.entry,
                opcode: entry.opcode,
                operands: entry.operands,
            };
            self.last_instruction = Some(instruction);
            let opcode = decoded.opcode;
            let trigger = execution.trigger;
            let call_depth_before = self.call_stack.len();
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
            let (next_entry, call_depth_after, outcome) = trace_flow(&flow, self.call_stack.len());
            self.push_trace(
                trigger,
                instruction,
                next_entry,
                call_depth_before,
                call_depth_after,
                outcome,
            );
            match flow {
                InstructionFlow::Continue(next) => {
                    execution = next;
                    if execution.wait_frames > 0 {
                        execution.wait_frames -= 1;
                        self.execution = Some(execution);
                        return Some(
                            if execution.wait_updates_auto_scripts
                                && execution.wait_frames.is_multiple_of(2)
                            {
                                ScriptEvent::Waiting {
                                    process_triggers: execution.wait_processes_triggers,
                                    update_party_gestures: execution.wait_updates_party_gestures,
                                }
                            } else {
                                ScriptEvent::Delay
                            },
                        );
                    }
                }
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

        if instruction_limit < MAX_INSTRUCTIONS_PER_ADVANCE {
            self.execution = Some(execution);
            return None;
        }

        self.execution = None;
        self.call_stack.clear();
        Some(ScriptEvent::InstructionLimit {
            trigger: execution.trigger,
            entry: execution.entry,
        })
    }
}

fn trace_flow(
    flow: &InstructionFlow,
    call_depth: usize,
) -> (Option<u16>, usize, ScriptTraceOutcome) {
    match flow {
        InstructionFlow::Continue(next) => {
            (Some(next.entry), call_depth, ScriptTraceOutcome::Continue)
        }
        InstructionFlow::Yield(next, event) => (
            Some(next.entry),
            call_depth,
            ScriptTraceOutcome::Yield(trace_event(*event)),
        ),
        InstructionFlow::Halt(ScriptEvent::Completed {
            next_entry,
            succeeded,
            ..
        }) => (
            None,
            call_depth,
            ScriptTraceOutcome::Completed {
                next_entry: *next_entry,
                succeeded: *succeeded,
            },
        ),
        InstructionFlow::Halt(_) => (None, call_depth, ScriptTraceOutcome::Error),
    }
}

fn trace_event(event: ScriptEvent) -> ScriptTraceEvent {
    match event {
        ScriptEvent::Message { .. } => ScriptTraceEvent::Message,
        ScriptEvent::Waiting { .. } => ScriptTraceEvent::Waiting,
        ScriptEvent::Redraw { .. } => ScriptTraceEvent::Redraw,
        ScriptEvent::Delay => ScriptTraceEvent::Delay,
        ScriptEvent::Confirm { .. } => ScriptTraceEvent::Confirm,
        ScriptEvent::OpenBuyMenu { .. } => ScriptTraceEvent::BuyMenu,
        ScriptEvent::OpenSellMenu => ScriptTraceEvent::SellMenu,
        ScriptEvent::StartBattle(_) => ScriptTraceEvent::Battle,
        ScriptEvent::Teleport { .. } => ScriptTraceEvent::Teleport,
        ScriptEvent::FadeScene { .. } => ScriptTraceEvent::Fade,
        ScriptEvent::Visual(_) => ScriptTraceEvent::Visual,
        ScriptEvent::WaitForKey => ScriptTraceEvent::WaitForKey,
        ScriptEvent::LoadLastSave => ScriptTraceEvent::LoadSave,
        ScriptEvent::QuitGame => ScriptTraceEvent::Quit,
        ScriptEvent::Action(_) => ScriptTraceEvent::Action,
        ScriptEvent::Condition(_) => ScriptTraceEvent::Condition,
        ScriptEvent::Completed { .. }
        | ScriptEvent::Unsupported { .. }
        | ScriptEvent::InvalidEntry { .. }
        | ScriptEvent::InstructionLimit { .. } => unreachable!("halt events are traced separately"),
    }
}
