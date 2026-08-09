//! Read-only script-record metadata used by development tools.

use pal_assets::script::{ScriptEntry, ScriptTable};

use super::ScriptOpcode;

/// Control-flow summary for one script record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptControlFlow {
    Next,
    Stop,
    Jump { target: u16, conditional: bool },
    Call { target: u16, return_entry: u16 },
    Random { choices: u16 },
    Unknown,
}

/// One decoded record in a bounded, linear script-table inspection window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptRecordInspection {
    pub entry: u16,
    pub instruction: ScriptEntry,
    pub opcode: Option<ScriptOpcode>,
    pub flow: ScriptControlFlow,
}

/// Inspect a bounded window without executing instructions or following branches.
///
/// PAL entry points can share tails and jump into arbitrary records, so this API
/// deliberately exposes a scrollable table window rather than guessing script
/// boundaries. Control-flow targets are annotated for editor UIs.
pub fn inspect_script_records(
    table: &ScriptTable,
    start: u16,
    offset: usize,
    limit: usize,
) -> Vec<ScriptRecordInspection> {
    (0..limit)
        .filter_map(|index| {
            let delta = offset.checked_add(index)?;
            let delta = u16::try_from(delta).ok()?;
            let entry = start.checked_add(delta)?;
            let instruction = *table.entry(entry)?;
            let opcode = ScriptOpcode::from_raw(instruction.opcode);
            Some(ScriptRecordInspection {
                entry,
                instruction,
                opcode,
                flow: opcode.map_or(ScriptControlFlow::Unknown, |opcode| {
                    control_flow(opcode, instruction, entry)
                }),
            })
        })
        .collect()
}

fn control_flow(opcode: ScriptOpcode, instruction: ScriptEntry, entry: u16) -> ScriptControlFlow {
    use ScriptOpcode::*;

    match opcode {
        Stop | StopAndAdvance | StopAndReplace | QuitGame => ScriptControlFlow::Stop,
        Jump => ScriptControlFlow::Jump {
            target: instruction.operands[0],
            conditional: instruction.operands[1] != 0,
        },
        Call => ScriptControlFlow::Call {
            target: instruction.operands[0],
            return_entry: entry.wrapping_add(1),
        },
        JumpByChance => ScriptControlFlow::Jump {
            target: instruction.operands[1],
            conditional: true,
        },
        RandomSelect => ScriptControlFlow::Random {
            choices: instruction.operands[0],
        },
        Confirm
        | JumpIfPlayerNotPoisoned
        | JumpIfEnemyNotFirstKind
        | JumpIfEnemyTurn
        | JumpIfPartyNotFullHp
        | CollectEnemy
        | TeleportParty => ScriptControlFlow::Jump {
            target: instruction.operands[0],
            conditional: true,
        },
        AdjustCash
        | JumpIfPlayerLacksPoison
        | JumpIfEnemyLacksPoison
        | JumpIfEnemyHpAbove
        | JumpIfPartyContainsPlayer
        | JumpIfSceneEquals => ScriptControlFlow::Jump {
            target: instruction.operands[1],
            conditional: true,
        },
        RemoveItem
        | JumpIfItemCountLess
        | JumpIfObjectOutsideZone
        | JumpIfNotFacingObject
        | JumpIfItemNotEquipped
        | JumpIfObjectStateEquals => ScriptControlFlow::Jump {
            target: instruction.operands[2],
            conditional: true,
        },
        _ => ScriptControlFlow::Next,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(entries: &[[u16; 4]]) -> ScriptTable {
        let data = entries
            .iter()
            .flat_map(|entry| entry.iter().flat_map(|value| value.to_le_bytes()))
            .collect::<Vec<_>>();
        ScriptTable::parse(&data).unwrap()
    }

    #[test]
    fn inspects_bounded_records_and_annotates_control_flow() {
        let scripts = table(&[
            [0, 0, 0, 0],
            [ScriptOpcode::Call.raw(), 4, 0, 0],
            [ScriptOpcode::Jump.raw(), 1, 0, 0],
            [ScriptOpcode::JumpIfSceneEquals.raw(), 3, 8, 0],
            [ScriptOpcode::Stop.raw(), 0, 0, 0],
        ]);

        let records = inspect_script_records(&scripts, 1, 0, 8);
        assert_eq!(records.len(), 4);
        assert_eq!(
            records[0].flow,
            ScriptControlFlow::Call {
                target: 4,
                return_entry: 2
            }
        );
        assert_eq!(
            records[1].flow,
            ScriptControlFlow::Jump {
                target: 1,
                conditional: false
            }
        );
        assert_eq!(
            records[2].flow,
            ScriptControlFlow::Jump {
                target: 8,
                conditional: true
            }
        );
        assert_eq!(records[3].flow, ScriptControlFlow::Stop);
    }

    #[test]
    fn handles_offsets_unknown_opcodes_and_address_overflow() {
        let scripts = table(&[[0xfffe, 1, 2, 3], [ScriptOpcode::Stop.raw(), 0, 0, 0]]);
        let records = inspect_script_records(&scripts, 0, 0, 1);
        assert_eq!(records[0].opcode, None);
        assert_eq!(records[0].flow, ScriptControlFlow::Unknown);
        assert_eq!(inspect_script_records(&scripts, 0, 1, 4).len(), 1);
        assert!(inspect_script_records(&scripts, u16::MAX, 1, 1).is_empty());
    }
}
