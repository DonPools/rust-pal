//! Script records stored in chunk 4 of `SSS.MKF`.

const SCRIPT_RECORD_SIZE: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptEntry {
    pub opcode: u16,
    pub operands: [u16; 3],
}

#[derive(Debug)]
pub struct ScriptTable {
    entries: Vec<ScriptEntry>,
}

impl ScriptTable {
    /// Parse a complete script chunk into directly addressable records.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.is_empty() || !data.len().is_multiple_of(SCRIPT_RECORD_SIZE) {
            return None;
        }
        let entries = data
            .chunks_exact(SCRIPT_RECORD_SIZE)
            .map(|record| ScriptEntry {
                opcode: u16::from_le_bytes([record[0], record[1]]),
                operands: [
                    u16::from_le_bytes([record[2], record[3]]),
                    u16::from_le_bytes([record[4], record[5]]),
                    u16::from_le_bytes([record[6], record[7]]),
                ],
            })
            .collect();
        Some(Self { entries })
    }

    pub fn entry(&self, index: u16) -> Option<&ScriptEntry> {
        self.entries.get(usize::from(index))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_little_endian_records() {
        let table = ScriptTable::parse(&[0xff, 0xff, 0x34, 0x12, 0x78, 0x56, 0xbc, 0x9a]).unwrap();
        assert_eq!(table.len(), 1);
        assert_eq!(
            table.entry(0),
            Some(&ScriptEntry {
                opcode: 0xffff,
                operands: [0x1234, 0x5678, 0x9abc],
            })
        );
        assert!(table.entry(1).is_none());
    }

    #[test]
    fn rejects_empty_and_partial_records() {
        assert!(ScriptTable::parse(&[]).is_none());
        assert!(ScriptTable::parse(&[0; SCRIPT_RECORD_SIZE - 1]).is_none());
        assert!(ScriptTable::parse(&[0; SCRIPT_RECORD_SIZE + 1]).is_none());
    }
}
