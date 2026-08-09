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
    Jump,
    Call,
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

/// Read-only incoming-reference index for scene-owned scripts and control flow.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScriptReferenceCatalog {
    by_entry: BTreeMap<u16, Vec<ScriptReferenceSource>>,
    object_scenes: BTreeMap<u16, u16>,
}

impl ScriptReferenceCatalog {
    /// Index every scene script slot, event-object script slot, jump and call.
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
            match record.flow {
                ScriptControlFlow::Jump { target, .. } => catalog.insert(
                    target,
                    ScriptReferenceSource::Instruction {
                        entry,
                        kind: ScriptInstructionReferenceKind::Jump,
                    },
                ),
                ScriptControlFlow::Call { target, .. } => catalog.insert(
                    target,
                    ScriptReferenceSource::Instruction {
                        entry,
                        kind: ScriptInstructionReferenceKind::Call,
                    },
                ),
                ScriptControlFlow::Next
                | ScriptControlFlow::Stop
                | ScriptControlFlow::Random { .. }
                | ScriptControlFlow::Unknown => {}
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
