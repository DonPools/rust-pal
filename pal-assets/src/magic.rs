//! Magic definitions stored in `DATA.MKF` chunk 4.

const MAGIC_RECORD_WORDS: usize = 16;
const MAGIC_RECORD_BYTES: usize = MAGIC_RECORD_WORDS * 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Magic {
    pub effect: u16,
    pub magic_type: u16,
    pub mp_cost: u16,
    pub base_damage: u16,
    pub elemental: u16,
    pub sound: i16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Magics {
    entries: Vec<Magic>,
}

impl Magics {
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.is_empty() || !data.len().is_multiple_of(MAGIC_RECORD_BYTES) {
            return None;
        }
        let entries = data
            .chunks_exact(MAGIC_RECORD_BYTES)
            .map(|record| {
                Some(Magic {
                    effect: u16::from_le_bytes(record.get(0..2)?.try_into().ok()?),
                    magic_type: u16::from_le_bytes(record.get(2..4)?.try_into().ok()?),
                    mp_cost: u16::from_le_bytes(record.get(24..26)?.try_into().ok()?),
                    base_damage: u16::from_le_bytes(record.get(26..28)?.try_into().ok()?),
                    elemental: u16::from_le_bytes(record.get(28..30)?.try_into().ok()?),
                    sound: i16::from_le_bytes(record.get(30..32)?.try_into().ok()?),
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self { entries })
    }

    pub fn get(&self, index: u16) -> Option<&Magic> {
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
    fn parses_mp_cost_and_rejects_invalid_lengths() {
        let mut data = vec![0; MAGIC_RECORD_BYTES * 2];
        data[24..26].copy_from_slice(&12u16.to_le_bytes());
        data[26..28].copy_from_slice(&45u16.to_le_bytes());
        data[28..30].copy_from_slice(&3u16.to_le_bytes());
        data[MAGIC_RECORD_BYTES + 24..MAGIC_RECORD_BYTES + 26]
            .copy_from_slice(&34u16.to_le_bytes());
        let magics = Magics::parse(&data).unwrap();
        assert_eq!(magics.len(), 2);
        assert_eq!(magics.get(0).unwrap().mp_cost, 12);
        assert_eq!(magics.get(0).unwrap().base_damage, 45);
        assert_eq!(magics.get(0).unwrap().elemental, 3);
        assert_eq!(magics.get(1).unwrap().mp_cost, 34);
        assert!(magics.get(2).is_none());
        assert!(Magics::parse(&[]).is_none());
        assert!(Magics::parse(&data[..data.len() - 1]).is_none());
    }
}
