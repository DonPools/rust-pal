//! Script-location history and editor-specific reference formatting.

use pal_core::script::{
    ScriptControlFlow, ScriptInstructionReferenceKind, ScriptObjectOperand, ScriptObjectReference,
    ScriptRecordInspection, ScriptReferenceSource,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ScriptLocation {
    pub(super) root: u16,
    pub(super) entry: u16,
}

#[derive(Debug, Default)]
pub(super) struct ScriptNavigation {
    current: Option<ScriptLocation>,
    selected_instruction: Option<u16>,
    back: Vec<ScriptLocation>,
    forward: Vec<ScriptLocation>,
}

impl ScriptNavigation {
    pub(super) fn reset(&mut self, root: u16) {
        self.current = (root != 0).then_some(ScriptLocation { root, entry: root });
        self.selected_instruction = (root != 0).then_some(root);
        self.back.clear();
        self.forward.clear();
    }

    pub(super) fn current(&self) -> Option<ScriptLocation> {
        self.current
    }

    pub(super) fn selected_instruction(&self) -> Option<u16> {
        self.selected_instruction
    }

    pub(super) fn select_instruction(&mut self, entry: u16) {
        self.selected_instruction = Some(entry);
    }

    pub(super) fn navigate(&mut self, entry: u16) -> bool {
        if entry == 0 {
            return false;
        }
        let Some(current) = self.current else {
            self.current = Some(ScriptLocation { root: entry, entry });
            self.selected_instruction = Some(entry);
            return true;
        };
        if current.entry == entry {
            self.selected_instruction = Some(entry);
            return false;
        }
        self.back.push(current);
        self.forward.clear();
        self.current = Some(ScriptLocation {
            root: current.root,
            entry,
        });
        self.selected_instruction = Some(entry);
        true
    }

    pub(super) fn go_back(&mut self) -> bool {
        let (Some(current), Some(previous)) = (self.current, self.back.pop()) else {
            return false;
        };
        self.forward.push(current);
        self.current = Some(previous);
        self.selected_instruction = Some(previous.entry);
        true
    }

    pub(super) fn go_forward(&mut self) -> bool {
        let (Some(current), Some(next)) = (self.current, self.forward.pop()) else {
            return false;
        };
        self.back.push(current);
        self.current = Some(next);
        self.selected_instruction = Some(next.entry);
        true
    }

    pub(super) fn go_root(&mut self) -> bool {
        self.current
            .is_some_and(|location| self.navigate(location.root))
    }

    pub(super) fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub(super) fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }
}

pub(super) fn navigable_target(record: ScriptRecordInspection) -> Option<u16> {
    match record.flow {
        ScriptControlFlow::Jump { target, .. } | ScriptControlFlow::Call { target, .. } => {
            (target != 0).then_some(target)
        }
        ScriptControlFlow::Next
        | ScriptControlFlow::Stop
        | ScriptControlFlow::Random { .. }
        | ScriptControlFlow::Unknown => None,
    }
}

pub(super) fn format_reference(source: ScriptReferenceSource) -> String {
    match source {
        ScriptReferenceSource::SceneEnter { scene } => format!("SCENE {scene} ENTER"),
        ScriptReferenceSource::SceneTeleport { scene } => format!("SCENE {scene} TELEPORT"),
        ScriptReferenceSource::ObjectTrigger { scene, object_id } => {
            format!(
                "SCENE {scene} OBJECT {} TRIGGER",
                format_object_id(object_id)
            )
        }
        ScriptReferenceSource::ObjectAuto { scene, object_id } => {
            format!("SCENE {scene} OBJECT {} AUTO", format_object_id(object_id))
        }
        ScriptReferenceSource::Instruction { entry, kind } => {
            format!("{} FROM @{entry:04X}", format_instruction_kind(kind))
        }
    }
}

pub(super) fn format_instruction_kind(kind: ScriptInstructionReferenceKind) -> String {
    use ScriptInstructionReferenceKind::*;

    match kind {
        Next => "NEXT".to_owned(),
        Jump => "JUMP".to_owned(),
        Branch => "BRANCH".to_owned(),
        Call => "CALL".to_owned(),
        CallReturn => "CALL RETURN".to_owned(),
        RandomChoice { choice } => format!("RANDOM CHOICE {choice}"),
        BattleWon => "ON BATTLE WON".to_owned(),
        BattleLost => "ON BATTLE LOST".to_owned(),
        BattleFled => "ON BATTLE FLED".to_owned(),
        Failure => "ON FAILURE".to_owned(),
        PersistNext => "PERSIST NEXT ENTRY".to_owned(),
        ReplaceCurrent => "REPLACE CURRENT ENTRY".to_owned(),
        SetObjectAuto { object_id } => {
            format!("SET {} AUTO", format_object_selector(object_id))
        }
        SetObjectTrigger { object_id } => {
            format!("SET {} TRIGGER", format_object_selector(object_id))
        }
        SetSceneEnter { scene } => format!("SET SCENE {scene} ENTER"),
        SetSceneTeleport { scene } => format!("SET SCENE {scene} TELEPORT"),
        SetGlobalObjectScript { object_id, field } => {
            format!(
                "SET GLOBAL OBJECT {} SCRIPT FIELD {field}",
                format_object_id(object_id)
            )
        }
    }
}

pub(super) const fn show_instruction_target(kind: ScriptInstructionReferenceKind) -> bool {
    !matches!(
        kind,
        ScriptInstructionReferenceKind::Next
            | ScriptInstructionReferenceKind::CallReturn
            | ScriptInstructionReferenceKind::BattleWon
    )
}

fn format_object_selector(object_id: u16) -> String {
    if object_id == u16::MAX {
        "CURRENT OBJECT".to_owned()
    } else {
        format!("OBJECT {}", format_object_id(object_id))
    }
}

pub(super) fn format_object_id(object_id: u16) -> String {
    format!("#{object_id:04X}")
}

pub(super) fn format_object_reference(object_id: u16, reference: ScriptObjectReference) -> String {
    let operand = match reference.operand {
        ScriptObjectOperand::Direct { index } => {
            format!("OP{index}={}", format_object_id(object_id))
        }
        ScriptObjectOperand::Range { first, last } => format!(
            "OP0..OP1={}..{}",
            format_object_id(first),
            format_object_id(last)
        ),
    };
    format!(
        "@{:04X} {} {operand}",
        reference.entry,
        reference.opcode.name()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_tracks_back_forward_and_root() {
        let mut navigation = ScriptNavigation::default();
        navigation.reset(10);
        assert_eq!(navigation.current().unwrap().entry, 10);
        assert!(navigation.navigate(20));
        assert!(navigation.navigate(30));
        assert!(navigation.can_go_back());
        assert!(navigation.go_back());
        assert_eq!(navigation.current().unwrap().entry, 20);
        assert!(navigation.go_forward());
        assert_eq!(navigation.current().unwrap().entry, 30);
        assert!(navigation.go_root());
        assert_eq!(navigation.current().unwrap().entry, 10);
        assert!(!navigation.can_go_forward());
    }

    #[test]
    fn formats_direct_and_range_object_references() {
        assert_eq!(
            format_object_reference(
                0x007e,
                ScriptObjectReference {
                    entry: 0x1db4,
                    opcode: pal_core::script::ScriptOpcode::SetObjectTriggerScript,
                    operand: ScriptObjectOperand::Direct { index: 0 },
                },
            ),
            "@1DB4 SetObjectTriggerScript OP0=#007E"
        );
        assert_eq!(
            format_object_reference(
                0x007e,
                ScriptObjectReference {
                    entry: 0x1db5,
                    opcode: pal_core::script::ScriptOpcode::SetObjectStates,
                    operand: ScriptObjectOperand::Range {
                        first: 0x007a,
                        last: 0x0080,
                    },
                },
            ),
            "@1DB5 SetObjectStates OP0..OP1=#007A..#0080"
        );
    }

    #[test]
    fn formats_control_flow_and_entry_write_references_distinctly() {
        assert_eq!(
            format_reference(ScriptReferenceSource::Instruction {
                entry: 0x1200,
                kind: ScriptInstructionReferenceKind::Next,
            }),
            "NEXT FROM @1200"
        );
        assert_eq!(
            format_reference(ScriptReferenceSource::Instruction {
                entry: 0x1db3,
                kind: ScriptInstructionReferenceKind::SetSceneEnter { scene: 4 },
            }),
            "SET SCENE 4 ENTER FROM @1DB3"
        );
        assert_eq!(
            format_instruction_kind(ScriptInstructionReferenceKind::SetObjectAuto {
                object_id: u16::MAX,
            }),
            "SET CURRENT OBJECT AUTO"
        );
        assert_eq!(format_object_id(421), "#01A5");
    }
}
