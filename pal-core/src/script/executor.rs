//! Shared low-level script decoding and program-counter state.

use pal_assets::script::{ScriptEntry, ScriptTable};

use super::ScriptOpcode;

/// A decoded script record shared by all execution modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DecodedInstruction {
    pub(crate) entry: u16,
    pub(crate) instruction: ScriptEntry,
    pub(crate) opcode: ScriptOpcode,
}

/// Errors produced before a mode-specific handler receives an instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DecodeError {
    InvalidEntry { entry: u16 },
    Unsupported { entry: u16, opcode: u16 },
}

/// Decode and validate one script record without applying mode-specific behavior.
pub(crate) fn decode_instruction(
    table: &ScriptTable,
    entry: u16,
) -> Result<DecodedInstruction, DecodeError> {
    let instruction = table
        .entry(entry)
        .copied()
        .ok_or(DecodeError::InvalidEntry { entry })?;
    let opcode = ScriptOpcode::from_raw(instruction.opcode).ok_or(DecodeError::Unsupported {
        entry,
        opcode: instruction.opcode,
    })?;
    Ok(DecodedInstruction {
        entry,
        instruction,
        opcode,
    })
}

/// Resolve PAL's current-object sentinels for object-selecting operands.
pub(crate) const fn selected_object(selector: u16, current: u16) -> u16 {
    if selector == 0 || selector == u16::MAX {
        current
    } else {
        selector
    }
}

/// Minimal program-counter state used by synchronous script modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScriptCursor {
    pub(crate) object_id: u16,
    pub(crate) entry: u16,
    pub(crate) next_entry: u16,
}

impl ScriptCursor {
    pub(crate) const fn new(object_id: u16, entry: u16) -> Self {
        Self {
            object_id,
            entry,
            next_entry: entry,
        }
    }

    pub(crate) fn advance(&mut self) {
        self.entry = self.entry.wrapping_add(1);
    }
}

/// Shared call-frame representation for nested scripts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScriptCallFrame {
    pub(crate) object_id: u16,
    pub(crate) return_entry: u16,
    pub(crate) wait_frames: u16,
    pub(crate) wait_updates_auto_scripts: bool,
    pub(crate) viewport_frames_remaining: u16,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(entries: &[[u16; 4]]) -> ScriptTable {
        let data = entries
            .iter()
            .flat_map(|entry| entry.iter().flat_map(|value| value.to_le_bytes()))
            .collect::<Vec<_>>();
        ScriptTable::parse(&data).expect("test script table must parse")
    }

    #[test]
    fn decodes_valid_instruction_and_rejects_invalid_opcode_or_entry() {
        let scripts = table(&[[ScriptOpcode::Stop.raw(), 1, 2, 3]]);
        let decoded = decode_instruction(&scripts, 0).unwrap();
        assert_eq!(decoded.entry, 0);
        assert_eq!(decoded.opcode, ScriptOpcode::Stop);
        assert_eq!(decoded.instruction.operands, [1, 2, 3]);
        assert_eq!(
            decode_instruction(&scripts, 1),
            Err(DecodeError::InvalidEntry { entry: 1 })
        );

        let invalid = table(&[[0xffff - 1, 0, 0, 0]]);
        assert_eq!(
            decode_instruction(&invalid, 0),
            Err(DecodeError::Unsupported {
                entry: 0,
                opcode: 0xfffe,
            })
        );
    }

    #[test]
    fn cursor_advances_without_overflow_panics() {
        let mut cursor = ScriptCursor::new(7, u16::MAX);
        assert_eq!(cursor.object_id, 7);
        cursor.advance();
        assert_eq!(cursor.entry, 0);
        assert_eq!(cursor.next_entry, u16::MAX);
    }

    #[test]
    fn selected_object_resolves_both_current_object_sentinels() {
        assert_eq!(selected_object(0, 7), 7);
        assert_eq!(selected_object(u16::MAX, 7), 7);
        assert_eq!(selected_object(9, 7), 9);
    }
}
