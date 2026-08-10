//! Global object definitions stored in `SSS.MKF` chunk 2.

const DOS_RECORD_WORDS: usize = 6;
const WIN_RECORD_WORDS: usize = 7;

pub const FIRST_PLAYER_OBJECT: u16 = 0x0024;
pub const LAST_PLAYER_OBJECT: u16 = 0x0029;
pub const FIRST_ITEM_OBJECT: u16 = 0x003d;
pub const LAST_ITEM_OBJECT: u16 = 0x0126;
pub const FIRST_MAGIC_OBJECT: u16 = 0x0127;
pub const LAST_MAGIC_OBJECT: u16 = 0x018d;
pub const FIRST_ENEMY_OBJECT: u16 = 0x018e;
pub const LAST_ENEMY_OBJECT: u16 = 0x0226;
pub const FIRST_POISON_OBJECT: u16 = 0x0227;
pub const LAST_POISON_OBJECT: u16 = 0x0232;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectLayout {
    Dos,
    Win95,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassicObjectKind {
    Player,
    Item,
    Magic,
    Enemy,
    Poison,
}

pub const fn classic_object_kind(object_id: u16) -> Option<ClassicObjectKind> {
    match object_id {
        FIRST_PLAYER_OBJECT..=LAST_PLAYER_OBJECT => Some(ClassicObjectKind::Player),
        FIRST_ITEM_OBJECT..=LAST_ITEM_OBJECT => Some(ClassicObjectKind::Item),
        FIRST_MAGIC_OBJECT..=LAST_MAGIC_OBJECT => Some(ClassicObjectKind::Magic),
        FIRST_ENEMY_OBJECT..=LAST_ENEMY_OBJECT => Some(ClassicObjectKind::Enemy),
        FIRST_POISON_OBJECT..=LAST_POISON_OBJECT => Some(ClassicObjectKind::Poison),
        _ => None,
    }
}

/// A normalized object union. Word 5 is the optional description script and word 6 is flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlobalObject {
    pub id: u16,
    pub data: [u16; WIN_RECORD_WORDS],
}

impl GlobalObject {
    pub const fn classic_kind(self) -> Option<ClassicObjectKind> {
        classic_object_kind(self.id)
    }

    pub fn player_friend_death_script(self) -> u16 {
        self.data[2]
    }

    pub fn player_dying_script(self) -> u16 {
        self.data[3]
    }

    pub fn item_bitmap(self) -> u16 {
        self.data[0]
    }

    pub fn item_price(self) -> u16 {
        self.data[1]
    }

    pub fn item_use_script(self) -> u16 {
        self.data[2]
    }

    pub fn item_equip_script(self) -> u16 {
        self.data[3]
    }

    pub fn item_throw_script(self) -> u16 {
        self.data[4]
    }

    pub fn item_flags(self) -> u16 {
        self.data[6]
    }

    pub fn magic_number(self) -> u16 {
        self.data[0]
    }

    pub fn magic_success_script(self) -> u16 {
        self.data[2]
    }

    pub fn magic_use_script(self) -> u16 {
        self.data[3]
    }

    pub fn magic_flags(self) -> u16 {
        self.data[6]
    }

    pub fn enemy_id(self) -> u16 {
        self.data[0]
    }

    pub fn enemy_sorcery_resistance(self) -> u16 {
        self.data[1]
    }

    pub fn enemy_turn_start_script(self) -> u16 {
        self.data[2]
    }

    pub fn enemy_battle_end_script(self) -> u16 {
        self.data[3]
    }

    pub fn enemy_ready_script(self) -> u16 {
        self.data[4]
    }

    pub fn poison_level(self) -> u16 {
        self.data[0]
    }

    pub fn poison_color(self) -> u16 {
        self.data[1]
    }

    pub fn poison_player_script(self) -> u16 {
        self.data[2]
    }

    pub fn poison_enemy_script(self) -> u16 {
        self.data[4]
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

    pub fn get_mut(&mut self, id: u16) -> Option<&mut GlobalObject> {
        self.objects.get_mut(usize::from(id))
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = &GlobalObject> {
        self.objects.iter()
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
        assert_eq!(objects.get(0).unwrap().item_use_script(), 30);
        assert_eq!(objects.get(0).unwrap().player_friend_death_script(), 30);
        assert_eq!(objects.get(0).unwrap().player_dying_script(), 40);
        assert_eq!(objects.get(0).unwrap().item_equip_script(), 40);
        assert_eq!(objects.get(0).unwrap().item_throw_script(), 50);
        assert_eq!(objects.get(0).unwrap().magic_success_script(), 30);
        assert_eq!(objects.get(0).unwrap().magic_use_script(), 40);
        assert_eq!(objects.get(0).unwrap().enemy_id(), 10);
        assert_eq!(objects.get(0).unwrap().enemy_sorcery_resistance(), 20);
        assert_eq!(objects.get(0).unwrap().enemy_turn_start_script(), 30);
        assert_eq!(objects.get(0).unwrap().enemy_battle_end_script(), 40);
        assert_eq!(objects.get(0).unwrap().enemy_ready_script(), 50);
        assert_eq!(objects.get(0).unwrap().poison_level(), 10);
        assert_eq!(objects.get(0).unwrap().poison_color(), 20);
        assert_eq!(objects.get(0).unwrap().poison_player_script(), 30);
        assert_eq!(objects.get(0).unwrap().poison_enemy_script(), 50);
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

    #[test]
    fn classifies_classic_union_ranges_without_inspecting_field_values() {
        assert_eq!(
            classic_object_kind(FIRST_ITEM_OBJECT),
            Some(ClassicObjectKind::Item)
        );
        assert_eq!(
            classic_object_kind(LAST_MAGIC_OBJECT),
            Some(ClassicObjectKind::Magic)
        );
        assert_eq!(
            classic_object_kind(LAST_ENEMY_OBJECT),
            Some(ClassicObjectKind::Enemy)
        );
        assert_eq!(classic_object_kind(LAST_POISON_OBJECT + 1), None);
    }
}
