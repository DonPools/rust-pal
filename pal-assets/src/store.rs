//! Store inventories stored in `DATA.MKF` chunk 0.

pub const STORE_ITEM_COUNT: usize = 9;
const STORE_BYTES: usize = STORE_ITEM_COUNT * 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Store {
    items: [u16; STORE_ITEM_COUNT],
}

impl Store {
    pub fn items(&self) -> impl Iterator<Item = u16> + '_ {
        self.items.iter().copied().take_while(|&item| item != 0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stores {
    stores: Vec<Store>,
}

impl Stores {
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.is_empty() || !data.len().is_multiple_of(STORE_BYTES) {
            return None;
        }
        let stores = data
            .chunks_exact(STORE_BYTES)
            .map(|record| {
                let items = record
                    .chunks_exact(2)
                    .map(|bytes| Some(u16::from_le_bytes(bytes.try_into().ok()?)))
                    .collect::<Option<Vec<_>>>()?
                    .try_into()
                    .ok()?;
                Some(Store { items })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self { stores })
    }

    pub fn len(&self) -> usize {
        self.stores.len()
    }

    pub fn is_empty(&self) -> bool {
        self.stores.is_empty()
    }

    pub fn get(&self, index: u16) -> Option<&Store> {
        self.stores.get(usize::from(index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_store_items_until_zero_terminator() {
        let data = [10u16, 20, 30, 0, 99, 99, 99, 99, 99]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let stores = Stores::parse(&data).unwrap();
        assert_eq!(stores.len(), 1);
        assert_eq!(
            stores.get(0).unwrap().items().collect::<Vec<_>>(),
            vec![10, 20, 30]
        );
        assert!(stores.get(1).is_none());
    }

    #[test]
    fn rejects_empty_and_partial_records() {
        assert!(Stores::parse(&[]).is_none());
        assert!(Stores::parse(&[0; STORE_BYTES - 1]).is_none());
    }
}
