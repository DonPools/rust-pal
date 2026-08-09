//! Stateful trigger-script execution.

mod battle;
mod condition;
mod control;
mod decode;
mod dispatch;
mod presentation;
mod role;
mod scene;

use std::collections::BTreeMap;

use pal_assets::script::{ScriptEntry, ScriptTable};

use crate::battle::{BattleRequest, BattleResult};
use crate::random;
use crate::scene::TriggerRequest;

use super::{DialogPosition, ScriptOpcode};
use crate::script::executor::ScriptCallFrame;

#[derive(Debug, Clone, Copy)]
struct Execution {
    trigger: TriggerRequest,
    object_id: u16,
    entry: u16,
    next_entry: u16,
    dialog_position: DialogPosition,
    dialog_color: u8,
    dialog_face: Option<u16>,
    dialog_playing_rng: bool,
    wait_frames: u32,
    wait_updates_auto_scripts: bool,
    wait_processes_triggers: bool,
    wait_updates_party_gestures: bool,
    viewport_frames_remaining: u16,
    succeeded: bool,
}

impl Execution {
    fn advance(&mut self) {
        self.entry = self.entry.wrapping_add(1);
    }

    fn resume_call(&mut self, frame: ScriptCallFrame) {
        self.object_id = frame.object_id;
        self.entry = frame.return_entry;
        self.wait_frames = frame.wait_frames;
        self.wait_updates_auto_scripts = frame.wait_updates_auto_scripts;
        self.wait_processes_triggers = frame.wait_processes_triggers;
        self.wait_updates_party_gestures = frame.wait_updates_party_gestures;
        self.viewport_frames_remaining = frame.viewport_frames_remaining;
    }
}

/// Internal result of executing one decoded trigger-script instruction.
enum InstructionFlow {
    Continue(Execution),
    Yield(Execution, super::ScriptEvent),
    Halt(super::ScriptEvent),
}

/// Result of decoding one general instruction without running through the
/// following record. Automatic scripts use this to retain their one-opcode
/// per-frame scheduling while sharing the trigger handlers.
pub(crate) struct ScriptInstructionStep {
    pub(crate) next_entry: u16,
    pub(crate) event: Option<super::ScriptEvent>,
    pub(crate) succeeded: bool,
}

/// One script instruction captured for development diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptInstructionDebug {
    pub object_id: u16,
    pub entry: u16,
    pub opcode: u16,
    pub operands: [u16; 3],
}

impl ScriptInstructionDebug {
    pub fn decoded_opcode(self) -> Option<ScriptOpcode> {
        ScriptOpcode::from_raw(self.opcode)
    }
}

/// Read-only execution details used by platform debug UIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptDebugSnapshot {
    pub active: bool,
    pub trigger: Option<TriggerRequest>,
    pub last_instruction: Option<ScriptInstructionDebug>,
    pub next_instruction: Option<ScriptInstructionDebug>,
    pub call_depth: usize,
    pub wait_frames: u32,
}

pub struct ScriptRuntime {
    table: ScriptTable,
    execution: Option<Execution>,
    call_stack: Vec<ScriptCallFrame>,
    random_state: u32,
    trigger_idle_frames: BTreeMap<u16, u16>,
    last_trigger: Option<TriggerRequest>,
    last_instruction: Option<ScriptInstructionDebug>,
    pending_battle: Option<BattleRequest>,
    current_rng: u16,
}

impl ScriptRuntime {
    pub fn new(table: ScriptTable) -> Self {
        Self {
            table,
            execution: None,
            call_stack: Vec::new(),
            random_state: random::DEFAULT_RANDOM_SEED,
            trigger_idle_frames: BTreeMap::new(),
            last_trigger: None,
            last_instruction: None,
            pending_battle: None,
            current_rng: 0,
        }
    }

    pub fn start(&mut self, trigger: TriggerRequest) -> bool {
        if self.execution.is_some() || self.pending_battle.is_some() || trigger.script_entry == 0 {
            return false;
        }
        self.execution = Some(Execution {
            trigger,
            object_id: trigger.object_id,
            entry: trigger.script_entry,
            next_entry: trigger.script_entry,
            dialog_position: DialogPosition::Upper,
            dialog_color: 0x4f,
            dialog_face: None,
            dialog_playing_rng: false,
            wait_frames: 0,
            wait_updates_auto_scripts: false,
            wait_processes_triggers: false,
            wait_updates_party_gestures: false,
            viewport_frames_remaining: 0,
            succeeded: true,
        });
        self.call_stack.clear();
        self.last_trigger = Some(trigger);
        self.last_instruction = None;
        self.pending_battle = None;
        true
    }

    /// Enter a nested script while preserving the active caller.
    ///
    /// Scene teleport scripts and autoscript `CALL`s raised during a waiting
    /// trigger use this path, then return when their `STOP` is reached.
    pub fn call(&mut self, entry: u16, object_id: u16) -> bool {
        let Some(mut execution) = self.execution else {
            return false;
        };
        if entry == 0 || self.table.entry(entry).is_none() {
            return false;
        }
        self.call_stack.push(ScriptCallFrame {
            object_id: execution.object_id,
            return_entry: execution.entry,
            wait_frames: execution.wait_frames,
            wait_updates_auto_scripts: execution.wait_updates_auto_scripts,
            wait_processes_triggers: execution.wait_processes_triggers,
            wait_updates_party_gestures: execution.wait_updates_party_gestures,
            viewport_frames_remaining: execution.viewport_frames_remaining,
        });
        execution.object_id = object_id;
        execution.entry = entry;
        execution.wait_frames = 0;
        execution.wait_updates_auto_scripts = false;
        execution.wait_processes_triggers = false;
        execution.wait_updates_party_gestures = false;
        execution.viewport_frames_remaining = 0;
        self.execution = Some(execution);
        true
    }

    pub fn is_active(&self) -> bool {
        self.execution.is_some()
    }

    pub fn is_waiting_for_battle(&self) -> bool {
        self.pending_battle.is_some()
    }

    pub fn debug_snapshot(&self) -> ScriptDebugSnapshot {
        let next_instruction = self.execution.and_then(|execution| {
            self.table
                .entry(execution.entry)
                .map(|entry| ScriptInstructionDebug {
                    object_id: execution.object_id,
                    entry: execution.entry,
                    opcode: entry.opcode,
                    operands: entry.operands,
                })
        });
        ScriptDebugSnapshot {
            active: self.execution.is_some(),
            trigger: self.last_trigger,
            last_instruction: self.last_instruction,
            next_instruction,
            call_depth: self.call_stack.len(),
            wait_frames: self.execution.map_or(0, |execution| execution.wait_frames),
        }
    }

    /// Return the shared Classic random state used by the next script roll.
    pub fn random_state(&self) -> u32 {
        self.random_state
    }

    /// Synchronize the script runner with the process-wide Classic random sequence.
    pub fn set_random_state(&mut self, state: u32) {
        self.random_state = state;
    }

    /// Redirect an active script after a world-state condition fails.
    pub fn branch_to(&mut self, entry: u16) -> bool {
        let Some(execution) = self.execution.as_mut() else {
            return false;
        };
        execution.entry = entry;
        execution.viewport_frames_remaining = 0;
        true
    }

    /// Record the success state of a world-dependent item effect.
    pub fn set_success(&mut self, succeeded: bool) -> bool {
        let Some(execution) = self.execution.as_mut() else {
            return false;
        };
        execution.succeeded = succeeded;
        true
    }

    /// Insert one 50 ms host delay before the active instruction resumes.
    ///
    /// Blocking movement helpers use this between their 100 ms scene steps;
    /// synchronous script actions remain free to continue on the next host
    /// update.
    pub fn delay_next_advance(&mut self) -> bool {
        let Some(execution) = self.execution.as_mut() else {
            return false;
        };
        execution.wait_frames = execution.wait_frames.max(1);
        execution.wait_updates_auto_scripts = false;
        execution.wait_processes_triggers = false;
        execution.wait_updates_party_gestures = false;
        true
    }

    /// Resume a script suspended by `BATTLE`, applying the original result branches.
    pub fn resolve_battle(&mut self, result: BattleResult) -> bool {
        let Some(request) = self.pending_battle.take() else {
            return false;
        };
        let Some(execution) = self.execution.as_mut() else {
            return false;
        };
        execution.entry = match result {
            BattleResult::Won | BattleResult::Terminated => execution.entry,
            BattleResult::Lost if request.lost_entry != 0 => request.lost_entry,
            BattleResult::Fled if request.flee_entry != 0 => request.flee_entry,
            BattleResult::Lost | BattleResult::Fled => execution.entry,
        };
        true
    }
    fn next_random_percent(&mut self) -> u16 {
        random::random_long(&mut self.random_state, 1, 100) as u16
    }

    fn idle_branch(&mut self, object_id: u16, limit: u16) -> bool {
        if limit == 0 {
            return true;
        }
        let idle = self
            .trigger_idle_frames
            .entry(object_id)
            .or_default()
            .wrapping_add(1);
        if idle < limit {
            self.trigger_idle_frames.insert(object_id, idle);
            true
        } else {
            self.trigger_idle_frames.remove(&object_id);
            false
        }
    }

    pub(crate) fn dispatch_single_instruction(
        &mut self,
        trigger: TriggerRequest,
        entry: ScriptEntry,
        opcode: ScriptOpcode,
    ) -> ScriptInstructionStep {
        self.call_stack.clear();
        self.pending_battle = None;
        let execution = Execution {
            trigger,
            object_id: trigger.object_id,
            entry: trigger.script_entry,
            next_entry: trigger.script_entry,
            dialog_position: DialogPosition::Upper,
            dialog_color: 0x4f,
            dialog_face: None,
            dialog_playing_rng: false,
            wait_frames: 0,
            wait_updates_auto_scripts: false,
            wait_processes_triggers: false,
            wait_updates_party_gestures: false,
            viewport_frames_remaining: 0,
            succeeded: true,
        };
        let flow = match opcode.trigger_handler() {
            super::opcode::TriggerHandler::Control => {
                self.dispatch_control(execution, entry, opcode)
            }
            super::opcode::TriggerHandler::Presentation => {
                self.dispatch_presentation(execution, entry, opcode)
            }
            super::opcode::TriggerHandler::Scene => self.dispatch_scene(execution, entry, opcode),
            super::opcode::TriggerHandler::Role => self.dispatch_role(execution, entry, opcode),
            super::opcode::TriggerHandler::Battle => self.dispatch_battle(execution, entry, opcode),
            super::opcode::TriggerHandler::Condition => {
                self.dispatch_condition(execution, entry, opcode)
            }
        };
        match flow {
            InstructionFlow::Continue(next) => ScriptInstructionStep {
                next_entry: next.entry,
                event: None,
                succeeded: next.succeeded,
            },
            InstructionFlow::Yield(next, event) => ScriptInstructionStep {
                next_entry: next.entry,
                event: Some(event),
                succeeded: next.succeeded,
            },
            InstructionFlow::Halt(event) => ScriptInstructionStep {
                next_entry: trigger.script_entry,
                event: Some(event),
                succeeded: false,
            },
        }
    }
}
