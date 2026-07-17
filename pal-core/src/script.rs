//! Deterministic trigger-script execution and dialog yields.

use std::collections::BTreeMap;

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
        font_color: u8,
        face_index: Option<u16>,
    },
    Waiting,
    Delay,
    Confirm {
        no_entry: u16,
    },
    OpenBuyMenu {
        store_number: u16,
    },
    OpenSellMenu,
    Action(ScriptAction),
    Condition(ScriptCondition),
    Completed {
        trigger: TriggerRequest,
        next_entry: u16,
        succeeded: bool,
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
    RemoveItem {
        item_id: u16,
        amount: u16,
        insufficient_entry: u16,
    },
    AdjustCash {
        amount: i16,
        insufficient_entry: u16,
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
    WalkObjectTo {
        object_id: u16,
        tile_x: u16,
        tile_y: u16,
        half: u16,
        speed: u8,
        repeat_entry: u16,
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
    SetObjectPositionRelativeToPlayer {
        object_id: u16,
        dx: i32,
        dy: i32,
    },
    OffsetObject {
        object_id: u16,
        dx: i32,
        dy: i32,
    },
    MoveObjectBy {
        object_id: u16,
        dx: i32,
        dy: i32,
    },
    SetObjectLayer {
        object_id: u16,
        layer: i16,
    },
    SetObjectState {
        object_id: u16,
        state: i16,
    },
    SetObjectVanishTime {
        object_id: u16,
        vanish_time: i16,
    },
    HideObjectTemporarily {
        object_id: u16,
        vanish_time: i16,
    },
    SyncObjectState {
        object_id: u16,
        source_object_id: u16,
        state: i16,
    },
    AnimateObject {
        object_id: u16,
    },
    SetObjectTriggerScript {
        object_id: u16,
        script_entry: u16,
    },
    SetObjectAutoScript {
        object_id: u16,
        script_entry: u16,
    },
    SetObjectTriggerMode {
        object_id: u16,
        trigger_mode: u16,
    },
    SetObjectStates {
        first_object_id: u16,
        last_object_id: u16,
        state: i16,
    },
    SetPlayerPose {
        direction: Direction,
        frame: u8,
        party_index: u16,
    },
    SetPlayerSprite {
        sprite_index: usize,
    },
    AdjustPlayerHealth {
        role_id: u16,
        hp: i16,
        mp: i16,
        apply_to_all: bool,
    },
    RevivePlayer {
        role_id: u16,
        hp_tenths: u16,
        apply_to_all: bool,
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
    WalkPlayerTo {
        tile_x: u16,
        tile_y: u16,
        half: u16,
        speed: u8,
        repeat_entry: u16,
    },
    RideObjectTo {
        object_id: u16,
        tile_x: u16,
        tile_y: u16,
        half: u16,
        speed: u8,
        repeat_entry: u16,
    },
    MoveViewport {
        x: i16,
        y: i16,
        frames: i16,
    },
    CollapseParty,
    ChangeScene {
        scene_number: u16,
    },
    SetSceneScripts {
        scene_number: u16,
        enter_script: Option<u16>,
        teleport_script: Option<u16>,
    },
    SetParty {
        /// Zero-based role IDs. Empty script slots are omitted.
        members: [Option<u16>; 3],
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptCondition {
    ItemCountLess {
        item_id: u16,
        amount: i16,
        target_entry: u16,
    },
    ObjectStateEquals {
        object_id: u16,
        state: i16,
        target_entry: u16,
    },
    SceneEquals {
        scene_number: u16,
        target_entry: u16,
    },
    PartyContainsName {
        name_word_id: u16,
        target_entry: u16,
    },
    PlayerFacesObject {
        object_id: u16,
        range: u16,
        target_entry: u16,
    },
}

#[derive(Debug, Clone, Copy)]
struct Execution {
    trigger: TriggerRequest,
    object_id: u16,
    entry: u16,
    next_entry: u16,
    dialog_position: DialogPosition,
    dialog_color: u8,
    dialog_face: Option<u16>,
    wait_frames: u16,
    wait_updates_auto_scripts: bool,
    viewport_frames_remaining: u16,
    succeeded: bool,
}

#[derive(Debug, Clone, Copy)]
struct CallFrame {
    object_id: u16,
    return_entry: u16,
}

/// One script instruction captured for development diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptInstructionDebug {
    pub object_id: u16,
    pub entry: u16,
    pub opcode: u16,
    pub operands: [u16; 3],
}

/// Read-only execution details used by platform debug UIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptDebugSnapshot {
    pub active: bool,
    pub trigger: Option<TriggerRequest>,
    pub last_instruction: Option<ScriptInstructionDebug>,
    pub next_instruction: Option<ScriptInstructionDebug>,
    pub call_depth: usize,
    pub wait_frames: u16,
}

pub struct ScriptRuntime {
    table: ScriptTable,
    execution: Option<Execution>,
    call_stack: Vec<CallFrame>,
    random_state: u32,
    trigger_idle_frames: BTreeMap<u16, u16>,
    last_trigger: Option<TriggerRequest>,
    last_instruction: Option<ScriptInstructionDebug>,
}

impl ScriptRuntime {
    pub fn new(table: ScriptTable) -> Self {
        Self {
            table,
            execution: None,
            call_stack: Vec::new(),
            random_state: 0x4d59_5df4,
            trigger_idle_frames: BTreeMap::new(),
            last_trigger: None,
            last_instruction: None,
        }
    }

    pub fn start(&mut self, trigger: TriggerRequest) -> bool {
        if self.execution.is_some() || trigger.script_entry == 0 {
            return false;
        }
        self.execution = Some(Execution {
            trigger,
            object_id: trigger.object_id,
            entry: trigger.script_entry,
            next_entry: trigger.script_entry,
            dialog_position: DialogPosition::Lower,
            dialog_color: 0x4f,
            dialog_face: None,
            wait_frames: 0,
            wait_updates_auto_scripts: false,
            viewport_frames_remaining: 0,
            succeeded: true,
        });
        self.call_stack.clear();
        self.last_trigger = Some(trigger);
        self.last_instruction = None;
        true
    }

    pub fn is_active(&self) -> bool {
        self.execution.is_some()
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

    /// Execute until a message, completion, or unsupported instruction yields control.
    pub fn advance(&mut self) -> Option<ScriptEvent> {
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
            let Some(entry) = self.table.entry(execution.entry).copied() else {
                self.execution = None;
                return Some(ScriptEvent::InvalidEntry {
                    trigger: execution.trigger,
                    entry: execution.entry,
                });
            };
            self.last_instruction = Some(ScriptInstructionDebug {
                object_id: execution.object_id,
                entry: execution.entry,
                opcode: entry.opcode,
                operands: entry.operands,
            });

            match entry.opcode {
                0x0000 => {
                    if let Some(frame) = self.call_stack.pop() {
                        execution.object_id = frame.object_id;
                        execution.entry = frame.return_entry;
                        continue;
                    }
                    self.execution = None;
                    return Some(ScriptEvent::Completed {
                        trigger: execution.trigger,
                        next_entry: execution.next_entry,
                        succeeded: execution.succeeded,
                    });
                }
                0x0001 => {
                    let next_entry = execution.entry.wrapping_add(1);
                    if let Some(frame) = self.call_stack.pop() {
                        execution.object_id = frame.object_id;
                        execution.entry = frame.return_entry;
                        continue;
                    }
                    execution.next_entry = next_entry;
                    self.execution = None;
                    return Some(ScriptEvent::Completed {
                        trigger: execution.trigger,
                        next_entry: execution.next_entry,
                        succeeded: execution.succeeded,
                    });
                }
                0x0002 => {
                    if self.idle_branch(execution.object_id, entry.operands[1]) {
                        let next_entry = entry.operands[0];
                        if let Some(frame) = self.call_stack.pop() {
                            execution.object_id = frame.object_id;
                            execution.entry = frame.return_entry;
                            continue;
                        }
                        execution.next_entry = next_entry;
                        self.execution = None;
                        return Some(ScriptEvent::Completed {
                            trigger: execution.trigger,
                            next_entry,
                            succeeded: execution.succeeded,
                        });
                    }
                    execution.entry = execution.entry.wrapping_add(1);
                }
                0x0003 => {
                    execution.entry = if self.idle_branch(execution.object_id, entry.operands[1]) {
                        entry.operands[0]
                    } else {
                        execution.entry.wrapping_add(1)
                    };
                }
                0x0004 => {
                    self.call_stack.push(CallFrame {
                        object_id: execution.object_id,
                        return_entry: execution.entry.wrapping_add(1),
                    });
                    execution.object_id = selected_object(entry.operands[1], execution.object_id);
                    execution.entry = entry.operands[0];
                }
                0x0005 => {
                    execution.entry = execution.entry.wrapping_add(1);
                    execution.wait_frames = delay_60ms_ticks(entry.operands[1]).saturating_sub(1);
                    execution.wait_updates_auto_scripts = false;
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Delay);
                }
                0x0045 | 0x004a | 0x0050 | 0x008e => {
                    execution.entry = execution.entry.wrapping_add(1)
                }
                0x003f | 0x0044 | 0x0097 => {
                    let repeat_entry = execution.entry;
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::RideObjectTo {
                        object_id: execution.object_id,
                        tile_x: entry.operands[0],
                        tile_y: entry.operands[1],
                        half: entry.operands[2],
                        speed: match entry.opcode {
                            0x003f => 2,
                            0x0044 => 4,
                            _ => 8,
                        },
                        repeat_entry,
                    }));
                }
                0x0008 => {
                    execution.entry = execution.entry.wrapping_add(1);
                    execution.next_entry = execution.entry;
                }
                0x0009 => {
                    execution.entry = execution.entry.wrapping_add(1);
                    execution.wait_frames = entry.operands[0].max(1) - 1;
                    execution.wait_updates_auto_scripts = true;
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Waiting);
                }
                0x000a => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Confirm {
                        no_entry: entry.operands[0],
                    });
                }
                0x0006 => {
                    let roll = self.next_random_percent();
                    execution.entry = if roll >= entry.operands[0] {
                        entry.operands[1]
                    } else {
                        execution.entry.wrapping_add(1)
                    };
                }
                0x000b..=0x000e => {
                    let direction = Direction::from_pal(entry.opcode - 0x000b)
                        .expect("walk opcodes always encode a valid direction");
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::MoveObject {
                        object_id: execution.object_id,
                        direction,
                    }));
                }
                0x000f => {
                    let direction = optional_direction(entry.operands[0]);
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetObjectPose {
                        object_id: execution.object_id,
                        direction,
                        frame: (entry.operands[1] != 0xffff).then_some(entry.operands[1]),
                    }));
                }
                0x0013 => {
                    let object_id = selected_object(entry.operands[0], execution.object_id);
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetObjectPosition {
                        object_id,
                        x: i32::from(entry.operands[1]),
                        y: i32::from(entry.operands[2]),
                    }));
                }
                0x0010 | 0x0011 => {
                    let repeat_entry = execution.entry;
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::WalkObjectTo {
                        object_id: execution.object_id,
                        tile_x: entry.operands[0],
                        tile_y: entry.operands[1],
                        half: entry.operands[2],
                        speed: if entry.opcode == 0x0010 { 3 } else { 2 },
                        repeat_entry,
                    }));
                }
                0x0012 => {
                    let object_id = selected_object(entry.operands[0], execution.object_id);
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(
                        ScriptAction::SetObjectPositionRelativeToPlayer {
                            object_id,
                            dx: i32::from(entry.operands[1] as i16),
                            dy: i32::from(entry.operands[2] as i16),
                        },
                    ));
                }
                0x0014 => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetObjectPose {
                        object_id: execution.object_id,
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
                        party_index: entry.operands[2],
                    }));
                }
                0x0016 if entry.operands[0] != 0 => {
                    let object_id = selected_object(entry.operands[0], execution.object_id);
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
                        object_id,
                        direction: Some(direction),
                        frame: Some(entry.operands[2]),
                    }));
                }
                0x0016 => execution.entry = execution.entry.wrapping_add(1),
                0x001b..=0x001d => {
                    let (hp, mp) = match entry.opcode {
                        0x001b => (entry.operands[1] as i16, 0),
                        0x001c => (0, entry.operands[1] as i16),
                        _ => (entry.operands[1] as i16, entry.operands[1] as i16),
                    };
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::AdjustPlayerHealth {
                        role_id: execution.object_id,
                        hp,
                        mp,
                        apply_to_all: entry.operands[0] != 0,
                    }));
                }
                0x001e => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::AdjustCash {
                        amount: entry.operands[0] as i16,
                        insufficient_entry: entry.operands[1],
                    }));
                }
                0x001f => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::AddItem {
                        item_id: entry.operands[0],
                        amount: entry.operands[1] as i16,
                    }));
                }
                0x0020 => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::RemoveItem {
                        item_id: entry.operands[0],
                        amount: entry.operands[1].max(1),
                        insufficient_entry: entry.operands[2],
                    }));
                }
                0x0022 => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::RevivePlayer {
                        role_id: execution.object_id,
                        hp_tenths: entry.operands[1],
                        apply_to_all: entry.operands[0] != 0,
                    }));
                }
                0x0026 => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::OpenBuyMenu {
                        store_number: entry.operands[0],
                    });
                }
                0x0027 => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::OpenSellMenu);
                }
                0x0024 if entry.operands[0] != 0 => {
                    let object_id = selected_object(entry.operands[0], execution.object_id);
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetObjectAutoScript {
                        object_id,
                        script_entry: entry.operands[1],
                    }));
                }
                0x0024 => execution.entry = execution.entry.wrapping_add(1),
                0x0025 if entry.operands[0] != 0 => {
                    let object_id = selected_object(entry.operands[0], execution.object_id);
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetObjectTriggerScript {
                        object_id,
                        script_entry: entry.operands[1],
                    }));
                }
                0x0025 => execution.entry = execution.entry.wrapping_add(1),
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
                    let object_id = selected_object(entry.operands[0], execution.object_id);
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetObjectState {
                        object_id,
                        state: entry.operands[1] as i16,
                    }));
                }
                0x0049 => execution.entry = execution.entry.wrapping_add(1),
                0x004b => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetObjectVanishTime {
                        object_id: execution.object_id,
                        vanish_time: -15,
                    }));
                }
                0x0052 => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::HideObjectTemporarily {
                        object_id: execution.object_id,
                        vanish_time: if entry.operands[0] == 0 {
                            800
                        } else {
                            entry.operands[0] as i16
                        },
                    }));
                }
                0x0046 => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetPlayerPosition {
                        tile_x: entry.operands[0],
                        tile_y: entry.operands[1],
                        half: entry.operands[2],
                    }));
                }
                0x0058 => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Condition(ScriptCondition::ItemCountLess {
                        item_id: entry.operands[0],
                        amount: entry.operands[1] as i16,
                        target_entry: entry.operands[2],
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
                    let object_id = selected_object(entry.operands[0], execution.object_id);
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::OffsetObject {
                        object_id,
                        dx: i32::from(entry.operands[1] as i16),
                        dy: i32::from(entry.operands[2] as i16),
                    }));
                }
                0x006d if entry.operands[0] != 0 => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    let clear = entry.operands[1] == 0 && entry.operands[2] == 0;
                    return Some(ScriptEvent::Action(ScriptAction::SetSceneScripts {
                        scene_number: entry.operands[0],
                        enter_script: (clear || entry.operands[1] != 0)
                            .then_some(entry.operands[1]),
                        teleport_script: (clear || entry.operands[2] != 0)
                            .then_some(entry.operands[2]),
                    }));
                }
                0x006d => execution.entry = execution.entry.wrapping_add(1),
                0x006e => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::OffsetPlayer {
                        dx: i32::from(entry.operands[0] as i16),
                        dy: i32::from(entry.operands[1] as i16),
                    }));
                }
                0x0070 => {
                    let repeat_entry = execution.entry;
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::WalkPlayerTo {
                        tile_x: entry.operands[0],
                        tile_y: entry.operands[1],
                        half: entry.operands[2],
                        speed: 2,
                        repeat_entry,
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
                0x0077 => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::PlayMusic {
                        music_id: 0,
                        looped: false,
                        fade_seconds: if entry.operands[0] == 0 {
                            2
                        } else {
                            u8::try_from(entry.operands[0].saturating_mul(3)).unwrap_or(u8::MAX)
                        },
                    }));
                }
                0x0079 => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Condition(ScriptCondition::PartyContainsName {
                        name_word_id: entry.operands[0],
                        target_entry: entry.operands[1],
                    }));
                }
                0x007a | 0x007b => {
                    let repeat_entry = execution.entry;
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::WalkPlayerTo {
                        tile_x: entry.operands[0],
                        tile_y: entry.operands[1],
                        half: entry.operands[2],
                        speed: if entry.opcode == 0x007a { 4 } else { 8 },
                        repeat_entry,
                    }));
                }
                0x007c | 0x0082 => {
                    let repeat_entry = execution.entry;
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::WalkObjectTo {
                        object_id: execution.object_id,
                        tile_x: entry.operands[0],
                        tile_y: entry.operands[1],
                        half: entry.operands[2],
                        // 0x007c moves four pixels every other original frame.
                        speed: if entry.opcode == 0x007c { 2 } else { 8 },
                        repeat_entry,
                    }));
                }
                0x007d => {
                    let object_id = selected_object(entry.operands[0], execution.object_id);
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::MoveObjectBy {
                        object_id,
                        dx: i32::from(entry.operands[1] as i16),
                        dy: i32::from(entry.operands[2] as i16),
                    }));
                }
                0x007e => {
                    let object_id = selected_object(entry.operands[0], execution.object_id);
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetObjectLayer {
                        object_id,
                        layer: entry.operands[1] as i16,
                    }));
                }
                0x007f => {
                    let frames = entry.operands[2] as i16;
                    if (entry.operands[0] == 0 && entry.operands[1] == 0) || frames == -1 {
                        execution.entry = execution.entry.wrapping_add(1);
                    } else {
                        if execution.viewport_frames_remaining == 0 {
                            execution.viewport_frames_remaining =
                                u16::try_from(frames).unwrap_or(1).max(1);
                        }
                        execution.viewport_frames_remaining -= 1;
                        if execution.viewport_frames_remaining == 0 {
                            execution.entry = execution.entry.wrapping_add(1);
                        }
                    }
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::MoveViewport {
                        x: entry.operands[0] as i16,
                        y: entry.operands[1] as i16,
                        frames: if frames == -1 { -1 } else { 1 },
                    }));
                }
                0x0078 => execution.entry = execution.entry.wrapping_add(1),
                0x0081 => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Condition(ScriptCondition::PlayerFacesObject {
                        object_id: entry.operands[0],
                        range: entry.operands[1],
                        target_entry: entry.operands[2],
                    }));
                }
                0x006f => {
                    let source_object_id = selected_object(entry.operands[0], execution.object_id);
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SyncObjectState {
                        object_id: execution.object_id,
                        source_object_id,
                        state: entry.operands[1] as i16,
                    }));
                }
                0x0087 => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::AnimateObject {
                        object_id: execution.object_id,
                    }));
                }
                0x0094 => {
                    let object_id = selected_object(entry.operands[0], execution.object_id);
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Condition(ScriptCondition::ObjectStateEquals {
                        object_id,
                        state: entry.operands[1] as i16,
                        target_entry: entry.operands[2],
                    }));
                }
                0x0095 => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Condition(ScriptCondition::SceneEquals {
                        scene_number: entry.operands[0],
                        target_entry: entry.operands[1],
                    }));
                }
                0x009a if entry.operands[0] != 0 && entry.operands[0] <= entry.operands[1] => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetObjectStates {
                        first_object_id: entry.operands[0],
                        last_object_id: entry.operands[1],
                        state: entry.operands[2] as i16,
                    }));
                }
                0x00a1 => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::CollapseParty));
                }
                0x00a2 if entry.operands[0] != 0 => {
                    let choices = entry.operands[0];
                    let choice = self.next_random_percent().wrapping_sub(1) % choices;
                    execution.entry = execution.entry.wrapping_add(choice).wrapping_add(1);
                }
                0x003b => {
                    execution.dialog_position = DialogPosition::Center;
                    if entry.operands[0] != 0 {
                        execution.dialog_color = entry.operands[0] as u8;
                    }
                    execution.dialog_face = None;
                    execution.entry = execution.entry.wrapping_add(1);
                }
                0x003c => {
                    execution.dialog_position = DialogPosition::Upper;
                    if entry.operands[1] != 0 {
                        execution.dialog_color = entry.operands[1] as u8;
                    }
                    execution.dialog_face = (entry.operands[0] != 0).then_some(entry.operands[0]);
                    execution.entry = execution.entry.wrapping_add(1);
                }
                0x003d => {
                    execution.dialog_position = DialogPosition::Lower;
                    if entry.operands[1] != 0 {
                        execution.dialog_color = entry.operands[1] as u8;
                    }
                    execution.dialog_face = (entry.operands[0] != 0).then_some(entry.operands[0]);
                    execution.entry = execution.entry.wrapping_add(1);
                }
                0x003e => {
                    execution.dialog_position = DialogPosition::CenterWindow;
                    if entry.operands[0] != 0 {
                        execution.dialog_color = entry.operands[0] as u8;
                    }
                    execution.dialog_face = None;
                    execution.entry = execution.entry.wrapping_add(1);
                }
                0x0040 if entry.operands[0] != 0 => {
                    let object_id = selected_object(entry.operands[0], execution.trigger.object_id);
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Action(ScriptAction::SetObjectTriggerMode {
                        object_id,
                        trigger_mode: entry.operands[1],
                    }));
                }
                0x0040 => execution.entry = execution.entry.wrapping_add(1),
                0x0085 => {
                    execution.entry = execution.entry.wrapping_add(1);
                    execution.wait_frames = delay_80ms_ticks(entry.operands[0]).saturating_sub(1);
                    execution.wait_updates_auto_scripts = false;
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Delay);
                }
                0xffff => {
                    execution.entry = execution.entry.wrapping_add(1);
                    self.execution = Some(execution);
                    return Some(ScriptEvent::Message {
                        message_id: entry.operands[0],
                        position: execution.dialog_position,
                        font_color: execution.dialog_color,
                        face_index: execution.dialog_face,
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
        self.call_stack.clear();
        Some(ScriptEvent::InstructionLimit {
            trigger: execution.trigger,
            entry: execution.entry,
        })
    }

    fn next_random_percent(&mut self) -> u16 {
        self.random_state = self
            .random_state
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        ((self.random_state >> 16) % 100 + 1) as u16
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

fn delay_80ms_ticks(periods: u16) -> u16 {
    const SCRIPT_TICK_MS: u32 = 50;
    let milliseconds = u32::from(periods) * 80;
    milliseconds
        .div_ceil(SCRIPT_TICK_MS)
        .max(1)
        .min(u32::from(u16::MAX)) as u16
}

fn delay_60ms_ticks(periods: u16) -> u16 {
    const SCRIPT_TICK_MS: u32 = 50;
    let periods = u32::from(periods.max(1));
    (periods * 60)
        .div_ceil(SCRIPT_TICK_MS)
        .min(u32::from(u16::MAX)) as u16
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

    #[test]
    fn debug_snapshot_tracks_trigger_and_instructions_after_completion() {
        let mut runtime =
            ScriptRuntime::new(table(&[[0, 0, 0, 0], [0x0005, 2, 0, 0], [0, 0, 0, 0]]));
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

        assert_eq!(runtime.advance(), Some(ScriptEvent::Delay));
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
                font_color: 0x4f,
                face_index: None,
            })
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Message {
                message_id: 43,
                position: DialogPosition::Upper,
                font_color: 0x4f,
                face_index: None,
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
    fn yields_item_recovery_actions_and_reports_script_success() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x001b, 0, 50, 0],
            [0x001c, 1, 20, 0],
            [0x001d, 0, 10, 0],
            [0x0022, 0, 3, 0],
            [0, 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::AdjustPlayerHealth {
                role_id: 7,
                hp: 50,
                mp: 0,
                apply_to_all: false,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::AdjustPlayerHealth {
                role_id: 7,
                hp: 0,
                mp: 20,
                apply_to_all: true,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::AdjustPlayerHealth {
                role_id: 7,
                hp: 10,
                mp: 10,
                apply_to_all: false,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::RevivePlayer {
                role_id: 7,
                hp_tenths: 3,
                apply_to_all: false,
            }))
        );
        assert!(runtime.set_success(false));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Completed {
                trigger: trigger(1),
                next_entry: 1,
                succeeded: false,
            })
        );
    }

    #[test]
    fn dialog_opcodes_preserve_face_and_font_color() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x003c, 5, 0x2d, 0],
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
            })
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Message {
                message_id: 43,
                position: DialogPosition::Lower,
                font_color: 0x1a,
                face_index: Some(6),
            })
        );
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
                position: DialogPosition::Lower,
                font_color: 0x4f,
                face_index: None,
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
                position: DialogPosition::Lower,
                font_color: 0x4f,
                face_index: None,
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
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Message {
                message_id: 42,
                position: DialogPosition::Lower,
                font_color: 0x4f,
                face_index: None,
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
                position: DialogPosition::Lower,
                font_color: 0x4f,
                face_index: None,
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
                position: DialogPosition::Lower,
                font_color: 0x4f,
                face_index: None,
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
                position: DialogPosition::Lower,
                font_color: 0x4f,
                face_index: None,
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
                position: DialogPosition::Lower,
                font_color: 0x4f,
                face_index: None,
            })
        );
    }

    #[test]
    fn yields_repeating_walk_and_relative_position_actions() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x0010, 4, 6, 1],
            [0x0012, 9, 0xfff0, 8],
            [0, 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::WalkObjectTo {
                object_id: 7,
                tile_x: 4,
                tile_y: 6,
                half: 1,
                speed: 3,
                repeat_entry: 1,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(
                ScriptAction::SetObjectPositionRelativeToPlayer {
                    object_id: 9,
                    dx: -16,
                    dy: 8,
                }
            ))
        );
    }

    #[test]
    fn moves_viewport_over_multiple_script_ticks() {
        let mut runtime =
            ScriptRuntime::new(table(&[[0, 0, 0, 0], [0x007f, 2, 0xffff, 3], [0, 0, 0, 0]]));
        runtime.start(trigger(1));
        for _ in 0..3 {
            assert_eq!(
                runtime.advance(),
                Some(ScriptEvent::Action(ScriptAction::MoveViewport {
                    x: 2,
                    y: -1,
                    frames: 1,
                }))
            );
        }
        assert!(matches!(
            runtime.advance(),
            Some(ScriptEvent::Completed { .. })
        ));
    }

    #[test]
    fn sets_and_restores_viewport_without_repeating() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x007f, 20, 30, 0xffff],
            [0x007f, 0, 0, 0],
            [0, 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::MoveViewport {
                x: 20,
                y: 30,
                frames: -1,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::MoveViewport {
                x: 0,
                y: 0,
                frames: 1,
            }))
        );
        assert!(matches!(
            runtime.advance(),
            Some(ScriptEvent::Completed { .. })
        ));

        let mut single_frame =
            ScriptRuntime::new(table(&[[0, 0, 0, 0], [0x007f, 1, 0, 0], [0, 0, 0, 0]]));
        single_frame.start(trigger(1));
        assert_eq!(
            single_frame.advance(),
            Some(ScriptEvent::Action(ScriptAction::MoveViewport {
                x: 1,
                y: 0,
                frames: 1,
            }))
        );
        assert!(matches!(
            single_frame.advance(),
            Some(ScriptEvent::Completed { .. })
        ));
    }

    #[test]
    fn yields_party_ride_speeds_and_collapse_action() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x003f, 10, 20, 0],
            [0x0044, 11, 21, 1],
            [0x0097, 12, 22, 0],
            [0x0078, 0, 0, 0],
            [0x00a1, 0, 0, 0],
            [0, 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        for (entry, speed) in [(1, 2), (2, 4), (3, 8)] {
            assert!(matches!(
                runtime.advance(),
                Some(ScriptEvent::Action(ScriptAction::RideObjectTo {
                    object_id: 7,
                    speed: actual_speed,
                    repeat_entry,
                    ..
                })) if actual_speed == speed && repeat_entry == entry
            ));
        }
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::CollapseParty))
        );
        assert!(matches!(
            runtime.advance(),
            Some(ScriptEvent::Completed { .. })
        ));
    }

    #[test]
    fn yields_confirmation_and_item_removal() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x000a, 4, 0, 0],
            [0x0020, 99, 0, 6],
            [0, 0, 0, 0],
            [0xffff, 10, 0, 0],
            [0, 0, 0, 0],
            [0xffff, 11, 0, 0],
            [0, 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Confirm { no_entry: 4 })
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::RemoveItem {
                item_id: 99,
                amount: 1,
                insufficient_entry: 6,
            }))
        );
        assert!(runtime.branch_to(6));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Message {
                message_id: 11,
                position: DialogPosition::Lower,
                font_color: 0x4f,
                face_index: None,
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
    fn yields_player_facing_condition() {
        let mut runtime =
            ScriptRuntime::new(table(&[[0, 0, 0, 0], [0x0081, 12, 2, 7], [0, 0, 0, 0]]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Condition(ScriptCondition::PlayerFacesObject {
                object_id: 12,
                range: 2,
                target_entry: 7,
            }))
        );
    }

    #[test]
    fn yields_scene_script_updates() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x006d, 2, 100, 200],
            [0x006d, 3, 0, 0],
            [0, 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::SetSceneScripts {
                scene_number: 2,
                enter_script: Some(100),
                teleport_script: Some(200),
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::SetSceneScripts {
                scene_number: 3,
                enter_script: Some(0),
                teleport_script: Some(0),
            }))
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
    fn updates_object_trigger_fields_and_delays_in_80ms_periods() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x0024, 10, 0x135d, 0],
            [0x0040, 10, 2, 0],
            [0x0025, 10, 0x119e, 0],
            [0x0085, 2, 0, 0],
            [0, 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::SetObjectAutoScript {
                object_id: 10,
                script_entry: 0x135d,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::SetObjectTriggerMode {
                object_id: 10,
                trigger_mode: 2,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::SetObjectTriggerScript {
                object_id: 10,
                script_entry: 0x119e,
            }))
        );
        assert_eq!(runtime.advance(), Some(ScriptEvent::Delay));
        assert_eq!(runtime.advance(), Some(ScriptEvent::Delay));
        assert_eq!(runtime.advance(), Some(ScriptEvent::Delay));
        assert_eq!(runtime.advance(), Some(ScriptEvent::Delay));
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

    #[test]
    fn yields_temporary_hide_fast_walk_and_object_transform_actions() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x0052, 0, 0, 0],
            [0x0079, 37, 9, 0],
            [0x007a, 4, 6, 1],
            [0x007c, 8, 9, 0],
            [0x007d, 0xffff, 0xfffc, 2],
            [0x007e, 12, 0xfff6, 0],
            [0x0082, 10, 11, 1],
            [0, 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::HideObjectTemporarily {
                object_id: 7,
                vanish_time: 800,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Condition(ScriptCondition::PartyContainsName {
                name_word_id: 37,
                target_entry: 9,
            }))
        );
        assert!(matches!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::WalkPlayerTo {
                speed: 4,
                repeat_entry: 3,
                ..
            }))
        ));
        assert!(matches!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::WalkObjectTo {
                object_id: 7,
                speed: 2,
                repeat_entry: 4,
                ..
            }))
        ));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::MoveObjectBy {
                object_id: 7,
                dx: -4,
                dy: 2,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::SetObjectLayer {
                object_id: 12,
                layer: -10,
            }))
        );
        assert!(matches!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::WalkObjectTo {
                speed: 8,
                repeat_entry: 7,
                ..
            }))
        ));
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
            runtime.advance(),
            Some(ScriptEvent::Message {
                message_id: 20,
                position: DialogPosition::Lower,
                font_color: 0x4f,
                face_index: None,
            })
        );
    }

    #[test]
    fn yields_persistent_state_conditions_and_batch_mutation() {
        let mut runtime = ScriptRuntime::new(table(&[
            [0, 0, 0, 0],
            [0x0058, 9, 2, 7],
            [0x0094, 0xffff, 0xffff, 8],
            [0x0095, 3, 9, 0],
            [0x009a, 4, 6, 0xfffe],
            [0, 0, 0, 0],
        ]));
        runtime.start(trigger(1));
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Condition(ScriptCondition::ItemCountLess {
                item_id: 9,
                amount: 2,
                target_entry: 7,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Condition(ScriptCondition::ObjectStateEquals {
                object_id: 7,
                state: -1,
                target_entry: 8,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Condition(ScriptCondition::SceneEquals {
                scene_number: 3,
                target_entry: 9,
            }))
        );
        assert_eq!(
            runtime.advance(),
            Some(ScriptEvent::Action(ScriptAction::SetObjectStates {
                first_object_id: 4,
                last_object_id: 6,
                state: -2,
            }))
        );
    }
}
