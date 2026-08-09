//! Read-only script-record metadata used by development tools.

use std::collections::BTreeMap;

use pal_assets::scene::SceneData;
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

/// Kind of instruction-level reference to another script entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScriptInstructionReferenceKind {
    Next,
    Jump,
    Branch,
    Call,
    CallReturn,
    RandomChoice { choice: u16 },
    BattleWon,
    BattleLost,
    BattleFled,
    Failure,
    PersistNext,
    ReplaceCurrent,
    SetObjectAuto { object_id: u16 },
    SetObjectTrigger { object_id: u16 },
    SetSceneEnter { scene: u16 },
    SetSceneTeleport { scene: u16 },
    SetGlobalObjectScript { object_id: u16, field: u16 },
}

/// Broad relationship class used to group instruction references in tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScriptInstructionReferenceCategory {
    ControlFlow,
    EntryWrite,
}

impl ScriptInstructionReferenceKind {
    pub const fn category(self) -> ScriptInstructionReferenceCategory {
        use ScriptInstructionReferenceCategory::{ControlFlow, EntryWrite};

        match self {
            Self::Next
            | Self::Jump
            | Self::Branch
            | Self::Call
            | Self::CallReturn
            | Self::RandomChoice { .. }
            | Self::BattleWon
            | Self::BattleLost
            | Self::BattleFled
            | Self::Failure => ControlFlow,
            Self::PersistNext
            | Self::ReplaceCurrent
            | Self::SetObjectAuto { .. }
            | Self::SetObjectTrigger { .. }
            | Self::SetSceneEnter { .. }
            | Self::SetSceneTeleport { .. }
            | Self::SetGlobalObjectScript { .. } => EntryWrite,
        }
    }
}

/// One statically recognizable script-entry target referenced by an instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ScriptInstructionTarget {
    pub entry: u16,
    pub kind: ScriptInstructionReferenceKind,
}

/// Static source that stores or refers to a script entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScriptReferenceSource {
    SceneEnter {
        scene: u16,
    },
    SceneTeleport {
        scene: u16,
    },
    ObjectTrigger {
        scene: u16,
        object_id: u16,
    },
    ObjectAuto {
        scene: u16,
        object_id: u16,
    },
    Instruction {
        entry: u16,
        kind: ScriptInstructionReferenceKind,
    },
}

/// Read-only incoming-reference index for entry owners, control flow and entry writes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScriptReferenceCatalog {
    by_entry: BTreeMap<u16, Vec<ScriptReferenceSource>>,
    object_scenes: BTreeMap<u16, u16>,
}

impl ScriptReferenceCatalog {
    /// Index scene and event-object slots plus every recognized instruction target.
    pub fn build(scenes: &SceneData, scripts: &ScriptTable) -> Self {
        let mut catalog = Self::default();
        for scene_number in 1..=scenes.scene_count() {
            let Some(scene) = scenes.scene(scene_number) else {
                continue;
            };
            let Ok(scene_number) = u16::try_from(scene_number) else {
                continue;
            };
            catalog.insert(
                scene.scene.script_on_enter,
                ScriptReferenceSource::SceneEnter {
                    scene: scene_number,
                },
            );
            catalog.insert(
                scene.scene.script_on_teleport,
                ScriptReferenceSource::SceneTeleport {
                    scene: scene_number,
                },
            );
            let Some(first_object_id) = scene.scene.event_object_index.checked_add(1) else {
                continue;
            };
            for (index, object) in scene.event_objects.iter().enumerate() {
                let Some(object_id) = u16::try_from(index)
                    .ok()
                    .and_then(|index| first_object_id.checked_add(index))
                else {
                    continue;
                };
                catalog.object_scenes.insert(object_id, scene_number);
                catalog.insert(
                    object.trigger_script,
                    ScriptReferenceSource::ObjectTrigger {
                        scene: scene_number,
                        object_id,
                    },
                );
                catalog.insert(
                    object.auto_script,
                    ScriptReferenceSource::ObjectAuto {
                        scene: scene_number,
                        object_id,
                    },
                );
            }
        }
        for index in 0..scripts.len() {
            let Ok(entry) = u16::try_from(index) else {
                break;
            };
            let Some(record) = inspect_script_record(scripts, entry) else {
                continue;
            };
            for target in inspect_script_targets(record) {
                catalog.insert(
                    target.entry,
                    ScriptReferenceSource::Instruction {
                        entry,
                        kind: target.kind,
                    },
                );
            }
        }
        for references in catalog.by_entry.values_mut() {
            references.sort_unstable();
            references.dedup();
        }
        catalog
    }

    pub fn references_to(&self, entry: u16) -> &[ScriptReferenceSource] {
        self.by_entry.get(&entry).map_or(&[], Vec::as_slice)
    }

    pub fn scene_for_object(&self, object_id: u16) -> Option<u16> {
        self.object_scenes.get(&object_id).copied()
    }

    fn insert(&mut self, entry: u16, source: ScriptReferenceSource) {
        if entry != 0 {
            self.by_entry.entry(entry).or_default().push(source);
        }
    }
}

/// Inspect one script record without executing it.
pub fn inspect_script_record(table: &ScriptTable, entry: u16) -> Option<ScriptRecordInspection> {
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
}

/// Return every statically recognizable script-entry target used by one record.
///
/// Control-flow edges and writes to mutable PAL entry slots are deliberately
/// distinguished. For example, `SetSceneScripts` continues to the next record
/// immediately but only installs its scene entry for a later invocation.
pub fn inspect_script_targets(record: ScriptRecordInspection) -> Vec<ScriptInstructionTarget> {
    use ScriptInstructionReferenceKind as Reference;
    use ScriptOpcode::*;

    let Some(opcode) = record.opcode else {
        return Vec::new();
    };
    let [op0, op1, op2] = record.instruction.operands;
    let next = record.entry.wrapping_add(1);
    let mut targets = Vec::new();
    let mut push = |entry, kind| {
        let target = ScriptInstructionTarget { entry, kind };
        if entry != 0 && !targets.contains(&target) {
            targets.push(target);
        }
    };

    match opcode {
        Stop | LoadLastSave | QuitGame => {}
        StopAndAdvance => push(next, Reference::PersistNext),
        StopAndReplace => {
            push(op0, Reference::ReplaceCurrent);
            if op1 != 0 {
                push(next, Reference::Next);
            }
        }
        AdvanceEntry => {
            push(next, Reference::Next);
            push(next, Reference::PersistNext);
        }
        StartBattle => {
            push(next, Reference::BattleWon);
            push(op1, Reference::BattleLost);
            push(op2, Reference::BattleFled);
        }
        PlaceUsedItemObject | SetEnemyStatus | SummonEnemy => {
            push(next, Reference::Next);
            push(op2, Reference::Failure);
        }
        FleeBattle | CollectEnemy => {
            push(next, Reference::Next);
            push(op0, Reference::Failure);
        }
        DivideEnemy => {
            push(next, Reference::Next);
            push(op1, Reference::Failure);
        }
        _ => match record.flow {
            ScriptControlFlow::Next => push(next, Reference::Next),
            ScriptControlFlow::Stop | ScriptControlFlow::Unknown => {}
            ScriptControlFlow::Jump {
                target,
                conditional: false,
            } => push(target, Reference::Jump),
            ScriptControlFlow::Jump {
                target,
                conditional: true,
            } => {
                push(target, Reference::Branch);
                push(next, Reference::Next);
            }
            ScriptControlFlow::Call {
                target,
                return_entry,
            } => {
                push(target, Reference::Call);
                push(return_entry, Reference::CallReturn);
            }
            ScriptControlFlow::Random { choices } => {
                for choice in 0..choices {
                    push(
                        record.entry.wrapping_add(choice).wrapping_add(1),
                        Reference::RandomChoice { choice: choice + 1 },
                    );
                }
            }
        },
    }

    match opcode {
        SetObjectAutoScript if op0 != 0 => {
            push(op1, Reference::SetObjectAuto { object_id: op0 });
        }
        SetObjectTriggerScript if op0 != 0 => {
            push(op1, Reference::SetObjectTrigger { object_id: op0 });
        }
        SetSceneScripts if op0 != 0 => {
            push(op1, Reference::SetSceneEnter { scene: op0 });
            push(op2, Reference::SetSceneTeleport { scene: op0 });
        }
        SetObjectScript if op2 <= 2 => {
            push(
                op1,
                Reference::SetGlobalObjectScript {
                    object_id: op0,
                    field: op2,
                },
            );
        }
        _ => {}
    }

    targets
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
            inspect_script_record(table, entry)
        })
        .collect()
}

fn control_flow(opcode: ScriptOpcode, instruction: ScriptEntry, entry: u16) -> ScriptControlFlow {
    use ScriptOpcode::*;

    match opcode {
        Stop | StopAndAdvance | StopAndReplace | LoadLastSave | QuitGame => ScriptControlFlow::Stop,
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

    fn make_mkf(chunks: &[&[u8]]) -> Vec<u8> {
        let table_size = (chunks.len() + 1) * 4;
        let mut offset = table_size as u32;
        let mut data = Vec::new();
        data.extend_from_slice(&offset.to_le_bytes());
        for chunk in chunks {
            offset += chunk.len() as u32;
            data.extend_from_slice(&offset.to_le_bytes());
        }
        for chunk in chunks {
            data.extend_from_slice(chunk);
        }
        data
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

    #[test]
    fn extracts_control_flow_outcomes_and_entry_writes() {
        let scripts = table(&[
            [ScriptOpcode::Stop.raw(), 0, 0, 0],
            [ScriptOpcode::NoOp.raw(), 0, 0, 0],
            [ScriptOpcode::SetSceneScripts.raw(), 4, 0x1f02, 0],
            [ScriptOpcode::StartBattle.raw(), 8, 9, 10],
            [ScriptOpcode::Call.raw(), 12, 0, 0],
            [ScriptOpcode::StopAndAdvance.raw(), 0, 0, 0],
        ]);

        assert_eq!(
            inspect_script_targets(inspect_script_record(&scripts, 1).unwrap()),
            [ScriptInstructionTarget {
                entry: 2,
                kind: ScriptInstructionReferenceKind::Next,
            }]
        );
        assert_eq!(
            inspect_script_targets(inspect_script_record(&scripts, 2).unwrap()),
            [
                ScriptInstructionTarget {
                    entry: 3,
                    kind: ScriptInstructionReferenceKind::Next,
                },
                ScriptInstructionTarget {
                    entry: 0x1f02,
                    kind: ScriptInstructionReferenceKind::SetSceneEnter { scene: 4 },
                },
            ]
        );
        assert_eq!(
            inspect_script_targets(inspect_script_record(&scripts, 3).unwrap()),
            [
                ScriptInstructionTarget {
                    entry: 4,
                    kind: ScriptInstructionReferenceKind::BattleWon,
                },
                ScriptInstructionTarget {
                    entry: 9,
                    kind: ScriptInstructionReferenceKind::BattleLost,
                },
                ScriptInstructionTarget {
                    entry: 10,
                    kind: ScriptInstructionReferenceKind::BattleFled,
                },
            ]
        );
        assert_eq!(
            inspect_script_targets(inspect_script_record(&scripts, 4).unwrap()),
            [
                ScriptInstructionTarget {
                    entry: 12,
                    kind: ScriptInstructionReferenceKind::Call,
                },
                ScriptInstructionTarget {
                    entry: 5,
                    kind: ScriptInstructionReferenceKind::CallReturn,
                },
            ]
        );
        assert_eq!(
            inspect_script_targets(inspect_script_record(&scripts, 5).unwrap()),
            [ScriptInstructionTarget {
                entry: 6,
                kind: ScriptInstructionReferenceKind::PersistNext,
            }]
        );
    }

    #[test]
    fn catalogs_fallthrough_and_entry_writes_but_not_rows_after_stop() {
        let scene_records = [
            1u16.to_le_bytes(),
            0u16.to_le_bytes(),
            0u16.to_le_bytes(),
            0u16.to_le_bytes(),
            0u16.to_le_bytes(),
            0u16.to_le_bytes(),
            0u16.to_le_bytes(),
            0u16.to_le_bytes(),
        ]
        .concat();
        let scene_data = SceneData::parse(&make_mkf(&[&[], &scene_records])).unwrap();
        let scripts = table(&[
            [ScriptOpcode::Stop.raw(), 0, 0, 0],
            [ScriptOpcode::NoOp.raw(), 0, 0, 0],
            [ScriptOpcode::Stop.raw(), 0, 0, 0],
            [ScriptOpcode::SetSceneScripts.raw(), 4, 0x1f02, 0],
        ]);

        let catalog = ScriptReferenceCatalog::build(&scene_data, &scripts);
        assert_eq!(
            catalog.references_to(2),
            [ScriptReferenceSource::Instruction {
                entry: 1,
                kind: ScriptInstructionReferenceKind::Next,
            }]
        );
        assert!(catalog.references_to(3).is_empty());
        assert_eq!(
            catalog.references_to(0x1f02),
            [ScriptReferenceSource::Instruction {
                entry: 3,
                kind: ScriptInstructionReferenceKind::SetSceneEnter { scene: 4 },
            }]
        );
    }

    #[test]
    fn catalogs_scene_slots_control_flow_and_object_ownership() {
        let mut event = [0; 32];
        event[8..10].copy_from_slice(&3u16.to_le_bytes());
        event[10..12].copy_from_slice(&4u16.to_le_bytes());
        let mut scene_records = Vec::new();
        for values in [[1u16, 1, 2, 0], [0, 0, 0, 1]] {
            for value in values {
                scene_records.extend_from_slice(&value.to_le_bytes());
            }
        }
        let scene_data = SceneData::parse(&make_mkf(&[&event, &scene_records])).unwrap();
        let scripts = table(&[
            [0, 0, 0, 0],
            [0, 0, 0, 0],
            [0, 0, 0, 0],
            [0, 0, 0, 0],
            [0, 0, 0, 0],
            [ScriptOpcode::Jump.raw(), 3, 0, 0],
            [ScriptOpcode::Call.raw(), 4, 0, 0],
        ]);

        let catalog = ScriptReferenceCatalog::build(&scene_data, &scripts);
        assert_eq!(
            catalog.references_to(3),
            [
                ScriptReferenceSource::ObjectTrigger {
                    scene: 1,
                    object_id: 1
                },
                ScriptReferenceSource::Instruction {
                    entry: 5,
                    kind: ScriptInstructionReferenceKind::Jump
                }
            ]
        );
        assert_eq!(
            catalog.references_to(4),
            [
                ScriptReferenceSource::ObjectAuto {
                    scene: 1,
                    object_id: 1
                },
                ScriptReferenceSource::Instruction {
                    entry: 6,
                    kind: ScriptInstructionReferenceKind::Call
                }
            ]
        );
        assert_eq!(catalog.scene_for_object(1), Some(1));
        assert_eq!(catalog.scene_for_object(2), None);
    }
}
