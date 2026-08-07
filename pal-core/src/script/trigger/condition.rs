//! Condition trigger-opcode handling.

use pal_assets::script::ScriptEntry;

use super::decode::selected_object;
use super::{Execution, InstructionFlow, ScriptRuntime};
use crate::script::{ScriptAction, ScriptCondition, ScriptEvent, ScriptOpcode};

impl ScriptRuntime {
    // Keep terminal arms structurally aligned with handlers that can also continue.
    #[allow(clippy::needless_return)]
    pub(super) fn dispatch_condition(
        &mut self,
        mut execution: Execution,
        entry: ScriptEntry,
        opcode: ScriptOpcode,
    ) -> InstructionFlow {
        use ScriptOpcode::*;
        match opcode {
            JumpIfItemCountLess => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Condition(ScriptCondition::ItemCountLess {
                        item_id: entry.operands[0],
                        amount: entry.operands[1] as i16,
                        target_entry: entry.operands[2],
                    }),
                );
            }
            JumpIfPlayerLacksPoison => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Condition(ScriptCondition::PlayerLacksPoison {
                        role_id: execution.object_id,
                        poison_id: entry.operands[0],
                        target_entry: entry.operands[1],
                    }),
                );
            }
            JumpIfEnemyLacksPoison => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Condition(ScriptCondition::EnemyLacksPoison {
                        enemy_index: execution.object_id,
                        poison_id: entry.operands[0],
                        target_entry: entry.operands[1],
                    }),
                );
            }
            JumpIfPlayerNotPoisoned => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Condition(ScriptCondition::PlayerNotPoisoned {
                        role_id: execution.object_id,
                        target_entry: entry.operands[0],
                    }),
                );
            }
            JumpIfEnemyHpAbove => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Condition(ScriptCondition::EnemyHpAbove {
                        enemy_index: execution.object_id,
                        percentage: entry.operands[0],
                        target_entry: entry.operands[1],
                    }),
                );
            }
            JumpIfEnemyNotFirstKind => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Condition(ScriptCondition::EnemyNotFirstKind {
                        enemy_index: execution.object_id,
                        target_entry: entry.operands[0],
                    }),
                );
            }
            JumpIfEnemyTurn => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Condition(ScriptCondition::EnemyTurn {
                        target_entry: entry.operands[0],
                    }),
                );
            }
            JumpIfObjectOutsideZone => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::CheckObjectZone {
                        object_id: execution.object_id,
                        target_id: entry.operands[0],
                        range: entry.operands[1],
                        failure_entry: entry.operands[2],
                    }),
                );
            }
            JumpIfPartyNotFullHp => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Condition(ScriptCondition::PartyNotFullHp {
                        target_entry: entry.operands[0],
                    }),
                );
            }
            JumpIfPartyContainsPlayer => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Condition(ScriptCondition::PartyContainsName {
                        name_word_id: entry.operands[0],
                        target_entry: entry.operands[1],
                    }),
                );
            }
            JumpIfNotFacingObject => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Condition(ScriptCondition::PlayerFacesObject {
                        object_id: entry.operands[0],
                        range: entry.operands[1],
                        target_entry: entry.operands[2],
                    }),
                );
            }
            JumpIfItemNotEquipped => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Condition(ScriptCondition::ItemNotEquipped {
                        item_id: entry.operands[0],
                        amount: entry.operands[1],
                        target_entry: entry.operands[2],
                    }),
                );
            }
            JumpIfObjectStateEquals => {
                let object_id = selected_object(entry.operands[0], execution.object_id);
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Condition(ScriptCondition::ObjectStateEquals {
                        object_id,
                        state: entry.operands[1] as i16,
                        target_entry: entry.operands[2],
                    }),
                );
            }
            JumpIfSceneEquals => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Condition(ScriptCondition::SceneEquals {
                        scene_number: entry.operands[0],
                        target_entry: entry.operands[1],
                    }),
                );
            }
            _ => unreachable!("opcode {opcode} is not a condition instruction"),
        }
    }
}
