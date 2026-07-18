//! Magic definitions stored in `DATA.MKF` chunk 4.

const MAGIC_RECORD_WORDS: usize = 16;
const MAGIC_RECORD_BYTES: usize = MAGIC_RECORD_WORDS * 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Magic {
    pub mp_cost: u16,
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
                    mp_cost: u16::from_le_bytes(record.get(26..28)?.try_into().ok()?),
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
        data[26..28].copy_from_slice(&12u16.to_le_bytes());
        data[MAGIC_RECORD_BYTES + 26..MAGIC_RECORD_BYTES + 28]
            .copy_from_slice(&34u16.to_le_bytes());
        let magics = Magics::parse(&data).unwrap();
        assert_eq!(magics.len(), 2);
        assert_eq!(magics.get(0).unwrap().mp_cost, 12);
        assert_eq!(magics.get(1).unwrap().mp_cost, 34);
        assert!(magics.get(2).is_none());
        assert!(Magics::parse(&[]).is_none());
        assert!(Magics::parse(&data[..data.len() - 1]).is_none());
    }
}
