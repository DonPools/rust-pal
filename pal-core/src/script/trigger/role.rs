//! Role trigger-opcode handling.

use pal_assets::script::ScriptEntry;

use super::{Execution, InstructionFlow, ScriptRuntime};
use crate::script::{ScriptAction, ScriptEvent, ScriptOpcode};

impl ScriptRuntime {
    // Keep terminal arms structurally aligned with handlers that can also continue.
    #[allow(clippy::needless_return)]
    pub(super) fn dispatch_role(
        &mut self,
        mut execution: Execution,
        entry: ScriptEntry,
        opcode: ScriptOpcode,
    ) -> InstructionFlow {
        use ScriptOpcode::*;
        match opcode {
            SetEquipmentEffect if entry.operands[0] >= 0x0b => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::SetEquipmentEffect {
                        role_id: execution.object_id,
                        attribute: entry.operands[1],
                        slot: entry.operands[0] - 0x0b,
                        value: entry.operands[2] as i16,
                    }),
                );
            }
            EquipItem if entry.operands[0] >= 0x0b => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::EquipItem {
                        role_id: execution.object_id,
                        slot: entry.operands[0] - 0x0b,
                        item_id: entry.operands[1],
                    }),
                );
            }
            AdjustPlayerAttribute | SetPlayerAttribute => {
                let role_id = entry.operands[2]
                    .checked_sub(1)
                    .unwrap_or(execution.object_id);
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::ChangePlayerAttribute {
                        role_id,
                        attribute: entry.operands[0],
                        value: entry.operands[1] as i16,
                        absolute: opcode == SetPlayerAttribute,
                    }),
                );
            }
            AdjustPlayerHp | AdjustPlayerMp | AdjustPlayerHpMp => {
                let (hp, mp) = match opcode {
                    AdjustPlayerHp => (entry.operands[1] as i16, 0),
                    AdjustPlayerMp => (0, entry.operands[1] as i16),
                    _ => (entry.operands[1] as i16, entry.operands[1] as i16),
                };
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::AdjustPlayerHealth {
                        role_id: execution.object_id,
                        hp,
                        mp,
                        apply_to_all: entry.operands[0] != 0,
                    }),
                );
            }
            AdjustCash => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::AdjustCash {
                        amount: entry.operands[0] as i16,
                        insufficient_entry: entry.operands[1],
                    }),
                );
            }
            AddItem => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::AddItem {
                        item_id: entry.operands[0],
                        amount: entry.operands[1] as i16,
                    }),
                );
            }
            RemoveItem => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::RemoveItem {
                        item_id: entry.operands[0],
                        amount: entry.operands[1].max(1),
                        insufficient_entry: entry.operands[2],
                    }),
                );
            }
            RevivePlayer => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::RevivePlayer {
                        role_id: execution.object_id,
                        hp_tenths: entry.operands[1],
                        apply_to_all: entry.operands[0] != 0,
                    }),
                );
            }
            LevelUpPlayer => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::LevelUpPlayer {
                        role_id: execution.object_id,
                        levels: entry.operands[0],
                    }),
                );
            }
            HalveCash => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::HalveCash),
                );
            }
            RemoveEquipment => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::RemoveEquipment {
                        role_id: entry.operands[0],
                        slot: entry.operands[1].checked_sub(1),
                    }),
                );
            }
            SetPlayerSprite => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::SetPlayerSprite {
                        role_id: entry.operands[0],
                        sprite_index: usize::from(entry.operands[1]),
                        reload: entry.operands[2] != 0,
                    }),
                );
            }
            SetParty => {
                execution.advance();
                let mut members = entry.operands.map(|role| role.checked_sub(1));
                if members.iter().all(Option::is_none) {
                    members[0] = Some(0);
                }
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::SetParty { members }),
                );
            }
            SetPartyFollower => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::SetPartyFollowers {
                        followers: [entry.operands[0], entry.operands[1]]
                            .map(|role_id| (role_id != 0).then_some(role_id)),
                    }),
                );
            }
            CollapseParty => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::CollapseParty),
                );
            }
            AddMagic | RemoveMagic => {
                let role_id = entry.operands[1]
                    .checked_sub(1)
                    .unwrap_or(execution.object_id);
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::ChangeMagic {
                        role_id,
                        magic_id: entry.operands[0],
                        add: opcode == AddMagic,
                    }),
                );
            }
            SetEquipmentEffect | EquipItem => {
                return InstructionFlow::Halt(ScriptEvent::Unsupported {
                    trigger: execution.trigger,
                    entry: execution.entry,
                    opcode: opcode.raw(),
                });
            }
            // ChasePlayer is handled by automatic scripts, not trigger scripts.
            _ => unreachable!("opcode {opcode} is not a role instruction"),
        }
    }
}
