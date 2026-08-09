//! Typed yield protocol between the trigger runtime and its host.

use crate::battle::BattleRequest;
use crate::scene::TriggerRequest;

use super::{ScriptAction, ScriptCondition, ScriptVisual};

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
        playing_rng: bool,
    },
    /// One original 100 ms scene frame elapsed while a trigger script waits.
    Waiting {
        /// Classic passes this flag to `PAL_GameUpdate`; when false only
        /// automatic scripts advance.
        process_triggers: bool,
        /// Reset party sprites to standing poses before drawing the frame.
        update_party_gestures: bool,
    },
    /// Redraw the current scene, then retain the script delay requested by the
    /// instruction.
    Redraw {
        update_party_gestures: bool,
    },
    Delay,
    Confirm {
        no_entry: u16,
    },
    OpenBuyMenu {
        store_number: u16,
    },
    OpenSellMenu,
    StartBattle(BattleRequest),
    Teleport {
        failure_entry: u16,
    },
    FadeScene {
        speed: u16,
    },
    Visual(ScriptVisual),
    WaitForKey,
    LoadLastSave,
    QuitGame,
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
