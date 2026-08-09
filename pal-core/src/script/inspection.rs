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

/// Operand shape that explicitly names one or more scene event objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScriptObjectOperand {
    /// A zero-based operand slot contains one object ID.
    Direct { index: u8 },
    /// Operands 0 and 1 contain an inclusive object-ID range.
    Range { first: u16, last: u16 },
}

/// One concrete event object explicitly targeted by a script instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ScriptObjectTarget {
    pub object_id: u16,
    pub operand: ScriptObjectOperand,
}

/// One script instruction that explicitly refers to an event object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptObjectReference {
    pub entry: u16,
    pub opcode: ScriptOpcode,
    pub operand: ScriptObjectOperand,
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

/// Read-only indexes for script-entry and explicit event-object references.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScriptReferenceCatalog {
    by_entry: BTreeMap<u16, Vec<ScriptReferenceSource>>,
    by_object: BTreeMap<u16, Vec<ScriptObjectReference>>,
    object_scenes: BTreeMap<u16, u16>,
}

impl ScriptReferenceCatalog {
    /// Index scene/object entry slots and every recognized instruction target.
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
            if let Some(opcode) = record.opcode {
                for target in inspect_script_object_targets(record) {
                    catalog.by_object.entry(target.object_id).or_default().push(
                        ScriptObjectReference {
                            entry,
                            opcode,
                            operand: target.operand,
                        },
                    );
                }
            }
        }
        for references in catalog.by_entry.values_mut() {
            references.sort_unstable();
            references.dedup();
        }
        for references in catalog.by_object.values_mut() {
            references.sort_unstable_by_key(|reference| {
                (reference.entry, reference.opcode.raw(), reference.operand)
            });
            references.dedup();
        }
        catalog
    }

    pub fn references_to(&self, entry: u16) -> &[ScriptReferenceSource] {
        self.by_entry.get(&entry).map_or(&[], Vec::as_slice)
    }

    /// Return every instruction whose operands explicitly name `object_id`.
    pub fn references_to_object(&self, object_id: u16) -> &[ScriptObjectReference] {
        self.by_object.get(&object_id).map_or(&[], Vec::as_slice)
    }

    /// Find the nearest statically recognizable script context containing `entry`.
    ///
    /// The search walks backward through physically adjacent fallthrough edges
    /// and stops at a scene/object owner, call target, mutable entry write, or
    /// the first record without an adjacent predecessor. It is intentionally a
    /// navigation aid rather than a claim that PAL scripts have physical bounds.
    pub fn script_context_root(&self, entry: u16) -> u16 {
        let mut current = entry;
        loop {
            let references = self.references_to(current);
            if references.iter().copied().any(is_script_entry_source) {
                return current;
            }
            let Some(previous) = current.checked_sub(1) else {
                return current;
            };
            let has_adjacent_predecessor = references.iter().copied().any(|source| {
                matches!(
                    source,
                    ScriptReferenceSource::Instruction { entry, kind }
                        if entry == previous && is_adjacent_control_flow(kind)
                )
            });
            if !has_adjacent_predecessor {
                return current;
            }
            current = previous;
        }
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

const fn is_script_entry_source(source: ScriptReferenceSource) -> bool {
    match source {
        ScriptReferenceSource::SceneEnter { .. }
        | ScriptReferenceSource::SceneTeleport { .. }
        | ScriptReferenceSource::ObjectTrigger { .. }
        | ScriptReferenceSource::ObjectAuto { .. } => true,
        ScriptReferenceSource::Instruction { kind, .. } => {
            matches!(kind, ScriptInstructionReferenceKind::Call)
                || matches!(
                    kind.category(),
                    ScriptInstructionReferenceCategory::EntryWrite
                )
        }
    }
}

const fn is_adjacent_control_flow(kind: ScriptInstructionReferenceKind) -> bool {
    matches!(
        kind,
        ScriptInstructionReferenceKind::Next
            | ScriptInstructionReferenceKind::CallReturn
            | ScriptInstructionReferenceKind::BattleWon
    )
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

/// Return the concrete scene event objects explicitly named by one record.
///
/// Object selectors `0` and `0xFFFF` depend on the current script owner and are
/// intentionally omitted: resolving them statically would require call-context
/// analysis and could otherwise report the wrong object. `SetObjectStates`
/// expands its valid inclusive range so every object in the range can be found.
pub fn inspect_script_object_targets(record: ScriptRecordInspection) -> Vec<ScriptObjectTarget> {
    use ScriptOpcode::*;

    let Some(opcode) = record.opcode else {
        return Vec::new();
    };
    let [op0, op1, _] = record.instruction.operands;
    if opcode == SetObjectStates {
        if op0 == 0 || op0 > op1 {
            return Vec::new();
        }
        let operand = ScriptObjectOperand::Range {
            first: op0,
            last: op1,
        };
        return (op0..=op1)
            .filter(|&object_id| object_id != u16::MAX)
            .map(|object_id| ScriptObjectTarget { object_id, operand })
            .collect();
    }

    let indices: &[u8] = match opcode {
        Call => &[1],
        SetObjectPosition
        | SetObjectPositionRelative
        | SetSelectedObjectPose
        | SetObjectAutoScript
        | SetObjectTriggerScript
        | SetObjectState
        | SetObjectTriggerMode
        | OffsetObjectAndAnimate
        | OffsetObject
        | SetObjectLayer
        | SyncObjectState
        | PlaceUsedItemObject
        | JumpIfNotFacingObject
        | JumpIfObjectOutsideZone
        | JumpIfObjectStateEquals => &[0],
        _ => &[],
    };
    indices
        .iter()
        .filter_map(|&index| {
            let object_id = record.instruction.operands[usize::from(index)];
            (object_id != 0 && object_id != u16::MAX).then_some(ScriptObjectTarget {
                object_id,
                operand: ScriptObjectOperand::Direct { index },
            })
        })
        .collect()
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

    fn record(entry: u16, opcode: ScriptOpcode, operands: [u16; 3]) -> ScriptRecordInspection {
        let instruction = ScriptEntry {
            opcode: opcode.raw(),
            operands,
        };
        ScriptRecordInspection {
            entry,
            instruction,
            opcode: Some(opcode),
            flow: control_flow(opcode, instruction, entry),
        }
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
    fn extracts_explicit_object_operands_and_ignores_context_selectors() {
        let direct_op0 = [
            ScriptOpcode::SetObjectPosition,
            ScriptOpcode::SetObjectPositionRelative,
            ScriptOpcode::SetSelectedObjectPose,
            ScriptOpcode::SetObjectAutoScript,
            ScriptOpcode::SetObjectTriggerScript,
            ScriptOpcode::SetObjectState,
            ScriptOpcode::SetObjectTriggerMode,
            ScriptOpcode::OffsetObjectAndAnimate,
            ScriptOpcode::OffsetObject,
            ScriptOpcode::SetObjectLayer,
            ScriptOpcode::SyncObjectState,
            ScriptOpcode::PlaceUsedItemObject,
            ScriptOpcode::JumpIfNotFacingObject,
            ScriptOpcode::JumpIfObjectOutsideZone,
            ScriptOpcode::JumpIfObjectStateEquals,
        ];
        for opcode in direct_op0 {
            assert_eq!(
                inspect_script_object_targets(record(1, opcode, [0x007e, 2, 3])),
                [ScriptObjectTarget {
                    object_id: 0x007e,
                    operand: ScriptObjectOperand::Direct { index: 0 },
                }],
                "missing object operand for {opcode}"
            );
        }
        assert_eq!(
            inspect_script_object_targets(record(
                0x1db4,
                ScriptOpcode::SetObjectTriggerScript,
                [0x007e, 0x1ed4, 0],
            )),
            [ScriptObjectTarget {
                object_id: 0x007e,
                operand: ScriptObjectOperand::Direct { index: 0 },
            }]
        );
        assert_eq!(
            inspect_script_object_targets(record(2, ScriptOpcode::Call, [7, 0x007e, 0])),
            [ScriptObjectTarget {
                object_id: 0x007e,
                operand: ScriptObjectOperand::Direct { index: 1 },
            }]
        );
        for selector in [0, u16::MAX] {
            assert!(inspect_script_object_targets(record(
                1,
                ScriptOpcode::SetObjectPosition,
                [selector, 2, 3],
            ))
            .is_empty());
        }
    }

    #[test]
    fn expands_valid_object_ranges_to_every_concrete_object() {
        let operand = ScriptObjectOperand::Range { first: 8, last: 10 };
        assert_eq!(
            inspect_script_object_targets(record(1, ScriptOpcode::SetObjectStates, [8, 10, 1],)),
            [
                ScriptObjectTarget {
                    object_id: 8,
                    operand,
                },
                ScriptObjectTarget {
                    object_id: 9,
                    operand,
                },
                ScriptObjectTarget {
                    object_id: 10,
                    operand,
                },
            ]
        );
        assert!(inspect_script_object_targets(record(
            1,
            ScriptOpcode::SetObjectStates,
            [10, 8, 1],
        ))
        .is_empty());
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

    #[test]
    fn finds_nearest_owned_called_or_physical_script_context() {
        let mut event = [0; 32];
        event[8..10].copy_from_slice(&2u16.to_le_bytes());
        let mut scene_records = Vec::new();
        for values in [[1u16, 0, 0, 0], [0, 0, 0, 1]] {
            for value in values {
                scene_records.extend_from_slice(&value.to_le_bytes());
            }
        }
        let scene_data = SceneData::parse(&make_mkf(&[&event, &scene_records])).unwrap();
        let scripts = table(&[
            [ScriptOpcode::Stop.raw(), 0, 0, 0],
            [ScriptOpcode::NoOp.raw(), 0, 0, 0],
            [ScriptOpcode::NoOp.raw(), 0, 0, 0],
            [ScriptOpcode::SetObjectState.raw(), 7, 1, 0],
            [ScriptOpcode::Stop.raw(), 0, 0, 0],
            [ScriptOpcode::Call.raw(), 7, 0, 0],
            [ScriptOpcode::Stop.raw(), 0, 0, 0],
            [ScriptOpcode::NoOp.raw(), 0, 0, 0],
            [ScriptOpcode::SetObjectState.raw(), 8, 1, 0],
            [ScriptOpcode::Stop.raw(), 0, 0, 0],
            [ScriptOpcode::SetObjectState.raw(), 9, 1, 0],
        ]);

        let catalog = ScriptReferenceCatalog::build(&scene_data, &scripts);
        assert_eq!(catalog.script_context_root(3), 2);
        assert_eq!(catalog.script_context_root(8), 7);
        assert_eq!(catalog.script_context_root(10), 10);
        assert_eq!(catalog.script_context_root(0), 0);
    }

    #[test]
    fn catalogs_instructions_by_explicit_object_id() {
        let mut scene_records = Vec::new();
        for values in [[1u16, 0, 0, 0], [0, 0, 0, 0]] {
            for value in values {
                scene_records.extend_from_slice(&value.to_le_bytes());
            }
        }
        let scene_data = SceneData::parse(&make_mkf(&[&[], &scene_records])).unwrap();
        let scripts = table(&[
            [ScriptOpcode::Stop.raw(), 0, 0, 0],
            [ScriptOpcode::SetObjectTriggerScript.raw(), 0x007e, 8, 0],
            [ScriptOpcode::SetObjectStates.raw(), 0x007d, 0x007f, 1],
            [ScriptOpcode::Call.raw(), 8, 0x007e, 0],
            [ScriptOpcode::SetObjectPosition.raw(), 0, 2, 3],
            [ScriptOpcode::SetObjectPosition.raw(), u16::MAX, 2, 3],
        ]);

        let catalog = ScriptReferenceCatalog::build(&scene_data, &scripts);
        assert_eq!(
            catalog.references_to_object(0x007e),
            [
                ScriptObjectReference {
                    entry: 1,
                    opcode: ScriptOpcode::SetObjectTriggerScript,
                    operand: ScriptObjectOperand::Direct { index: 0 },
                },
                ScriptObjectReference {
                    entry: 2,
                    opcode: ScriptOpcode::SetObjectStates,
                    operand: ScriptObjectOperand::Range {
                        first: 0x007d,
                        last: 0x007f,
                    },
                },
                ScriptObjectReference {
                    entry: 3,
                    opcode: ScriptOpcode::Call,
                    operand: ScriptObjectOperand::Direct { index: 1 },
                },
            ]
        );
        assert_eq!(catalog.references_to_object(0x007d).len(), 1);
        assert_eq!(catalog.references_to_object(0x007f).len(), 1);
        assert!(catalog.references_to_object(0).is_empty());
        assert!(catalog.references_to_object(u16::MAX).is_empty());
    }
}
