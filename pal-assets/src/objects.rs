//! Global object definitions stored in `SSS.MKF` chunk 2.

const DOS_RECORD_WORDS: usize = 6;
const WIN_RECORD_WORDS: usize = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectLayout {
    Dos,
    Win95,
}

/// A normalized object union. Word 5 is the optional description script and word 6 is flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlobalObject {
    pub id: u16,
    pub data: [u16; WIN_RECORD_WORDS],
}

impl GlobalObject {
    pub fn item_bitmap(self) -> u16 {
        self.data[0]
    }

    pub fn item_price(self) -> u16 {
        self.data[1]
    }

    pub fn item_flags(self) -> u16 {
        self.data[6]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalObjects {
    layout: ObjectLayout,
    objects: Vec<GlobalObject>,
}

impl GlobalObjects {
    pub fn parse(data: &[u8], layout: ObjectLayout) -> Option<Self> {
        let words = match layout {
            ObjectLayout::Dos => DOS_RECORD_WORDS,
            ObjectLayout::Win95 => WIN_RECORD_WORDS,
        };
        let record_size = words * 2;
        if data.is_empty() || !data.len().is_multiple_of(record_size) {
            return None;
        }
        let objects = data
            .chunks_exact(record_size)
            .enumerate()
            .map(|(index, record)| {
                let mut normalized = [0; WIN_RECORD_WORDS];
                for (word, bytes) in record.chunks_exact(2).enumerate() {
                    normalized[word] = u16::from_le_bytes(bytes.try_into().ok()?);
                }
                if layout == ObjectLayout::Dos {
                    normalized[6] = normalized[5];
                    normalized[5] = 0;
                }
                Some(GlobalObject {
                    id: u16::try_from(index).ok()?,
                    data: normalized,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self { layout, objects })
    }

    pub fn layout(&self) -> ObjectLayout {
        self.layout
    }

    pub fn len(&self) -> usize {
        self.objects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    pub fn get(&self, id: u16) -> Option<&GlobalObject> {
        self.objects.get(usize::from(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_normalizes_dos_objects() {
        let words = [10u16, 20, 30, 40, 50, 0x1234];
        let data = words
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect::<Vec<_>>();
        let objects = GlobalObjects::parse(&data, ObjectLayout::Dos).unwrap();
        assert_eq!(objects.layout(), ObjectLayout::Dos);
        assert_eq!(objects.len(), 1);
        assert_eq!(
            objects.get(0).unwrap().data,
            [10, 20, 30, 40, 50, 0, 0x1234]
        );
        assert!(objects.get(1).is_none());
    }

    #[test]
    fn preserves_win95_description_and_flags() {
        let words = [10u16, 20, 30, 40, 50, 60, 70];
        let data = words
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect::<Vec<_>>();
        assert_eq!(
            GlobalObjects::parse(&data, ObjectLayout::Win95)
                .unwrap()
                .get(0)
                .unwrap()
                .data,
            words
        );
    }

    #[test]
    fn rejects_empty_truncated_and_wrong_layout_data() {
        assert!(GlobalObjects::parse(&[], ObjectLayout::Dos).is_none());
        assert!(GlobalObjects::parse(&[0; 11], ObjectLayout::Dos).is_none());
        assert!(GlobalObjects::parse(&[0; 12], ObjectLayout::Win95).is_none());
    }
}
