//! Original PAL `.rpg` save-game parser.
//!
//! Save files contain a fixed common prefix, either DOS or Win95 global-object
//! records, and a variable number of 32-byte event-object records.

use crate::{
    objects::{GlobalObjects, ObjectLayout},
    player_roles::PlayerRoles,
    scene::{EventObject, Scene},
};

pub const SAVE_PARTY_CAPACITY: usize = 5;
pub const SAVE_ROLE_COUNT: usize = 6;
pub const SAVE_EXPERIENCE_KINDS: usize = 8;
pub const SAVE_POISON_SLOTS: usize = 16;
pub const SAVE_INVENTORY_CAPACITY: usize = 256;
pub const SAVE_SCENE_CAPACITY: usize = 300;
pub const SAVE_OBJECT_CAPACITY: usize = 600;
pub const SAVE_EVENT_OBJECT_CAPACITY: usize = 5500;

const HEADER_SIZE: usize = 44;
const PARTY_RECORD_SIZE: usize = 10;
const TRAIL_RECORD_SIZE: usize = 6;
const EXPERIENCE_RECORD_SIZE: usize = 8;
const PLAYER_ROLES_SIZE: usize = 900;
const POISON_RECORD_SIZE: usize = 4;
const INVENTORY_RECORD_SIZE: usize = 6;
const SCENE_RECORD_SIZE: usize = 8;
const DOS_OBJECT_RECORD_SIZE: usize = 12;
const WIN_OBJECT_RECORD_SIZE: usize = 14;
const EVENT_OBJECT_RECORD_SIZE: usize = 32;

const COMMON_SIZE: usize = HEADER_SIZE
    + PARTY_RECORD_SIZE * SAVE_PARTY_CAPACITY
    + TRAIL_RECORD_SIZE * SAVE_PARTY_CAPACITY
    + EXPERIENCE_RECORD_SIZE * SAVE_ROLE_COUNT * SAVE_EXPERIENCE_KINDS
    + PLAYER_ROLES_SIZE
    + POISON_RECORD_SIZE * SAVE_POISON_SLOTS * SAVE_PARTY_CAPACITY
    + INVENTORY_RECORD_SIZE * SAVE_INVENTORY_CAPACITY
    + SCENE_RECORD_SIZE * SAVE_SCENE_CAPACITY;
pub const DOS_SAVE_FIXED_SIZE: usize = COMMON_SIZE + DOS_OBJECT_RECORD_SIZE * SAVE_OBJECT_CAPACITY;
pub const WIN_SAVE_FIXED_SIZE: usize = COMMON_SIZE + WIN_OBJECT_RECORD_SIZE * SAVE_OBJECT_CAPACITY;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SavePartyMember {
    pub role_id: u16,
    pub x: i16,
    pub y: i16,
    pub frame: u16,
    pub image_offset: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveTrail {
    pub x: i16,
    pub y: i16,
    pub direction: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveExperience {
    pub experience: u16,
    pub reserved: u16,
    pub level: u16,
    pub count: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SavePoison {
    pub poison_id: u16,
    pub script: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveInventoryEntry {
    pub item_id: u16,
    pub amount: u16,
    pub amount_in_use: u16,
}

/// Fully parsed original save data. The layout identifies the source edition.
#[derive(Debug, Clone)]
pub struct OriginalSave {
    pub layout: ObjectLayout,
    pub saved_times: u16,
    pub viewport_x: i16,
    pub viewport_y: i16,
    /// Original format stores the maximum active party index, not a count.
    pub party_member_index: u16,
    pub scene_number: u16,
    pub night_palette: bool,
    pub party_direction: u16,
    pub music_number: u16,
    pub battle_music_number: u16,
    pub battlefield_number: u16,
    pub screen_wave: u16,
    pub battle_speed: u16,
    pub collect_value: u16,
    pub layer: u16,
    pub chase_range: u16,
    pub chase_speed_change_cycles: u16,
    pub follower_count: u16,
    pub cash: u32,
    pub party: [SavePartyMember; SAVE_PARTY_CAPACITY],
    pub trail: [SaveTrail; SAVE_PARTY_CAPACITY],
    pub experience: [[SaveExperience; SAVE_ROLE_COUNT]; SAVE_EXPERIENCE_KINDS],
    pub player_roles: PlayerRoles,
    pub poisons: [[SavePoison; SAVE_PARTY_CAPACITY]; SAVE_POISON_SLOTS],
    pub inventory: [SaveInventoryEntry; SAVE_INVENTORY_CAPACITY],
    pub scenes: [Scene; SAVE_SCENE_CAPACITY],
    pub objects: GlobalObjects,
    pub event_objects: Vec<EventObject>,
}

impl OriginalSave {
    /// Parse a complete DOS or Win95 `.rpg` file.
    pub fn parse(data: &[u8]) -> Option<Self> {
        let (layout, object_record_size, event_count) = detect_layout(data.len())?;
        let mut cursor = Cursor::new(data);

        let saved_times = cursor.u16()?;
        let viewport_x = cursor.i16()?;
        let viewport_y = cursor.i16()?;
        let party_member_index = cursor.u16()?;
        let scene_number = cursor.u16()?;
        let palette_offset = cursor.u16()?;
        let party_direction = cursor.u16()?;
        let music_number = cursor.u16()?;
        let battle_music_number = cursor.u16()?;
        let battlefield_number = cursor.u16()?;
        let screen_wave = cursor.u16()?;
        let battle_speed = cursor.u16()?;
        let collect_value = cursor.u16()?;
        let layer = cursor.u16()?;
        let chase_range = cursor.u16()?;
        let chase_speed_change_cycles = cursor.u16()?;
        let follower_count = cursor.u16()?;
        cursor.take(6)?; // three reserved words
        let cash = cursor.u32()?;

        if usize::from(party_member_index) >= SAVE_PARTY_CAPACITY
            || scene_number == 0
            || usize::from(scene_number) >= SAVE_SCENE_CAPACITY
            || party_direction > 3
            || usize::from(follower_count) > SAVE_PARTY_CAPACITY
        {
            return None;
        }

        let party = parse_array(&mut cursor, |cursor| {
            Some(SavePartyMember {
                role_id: cursor.u16()?,
                x: cursor.i16()?,
                y: cursor.i16()?,
                frame: cursor.u16()?,
                image_offset: cursor.u16()?,
            })
        })?;
        let party_count = usize::from(party_member_index) + 1;
        let follower_end = party_count.checked_add(usize::from(follower_count))?;
        if follower_end > SAVE_PARTY_CAPACITY
            || party[..follower_end]
                .iter()
                .any(|member| usize::from(member.role_id) >= SAVE_ROLE_COUNT)
            || party[..party_count]
                .iter()
                .enumerate()
                .any(|(index, member)| {
                    party[..index]
                        .iter()
                        .any(|other| other.role_id == member.role_id)
                })
        {
            return None;
        }

        let trail = parse_array(&mut cursor, |cursor| {
            Some(SaveTrail {
                x: cursor.i16()?,
                y: cursor.i16()?,
                direction: cursor.u16()?,
            })
        })?;
        let experience = parse_array(&mut cursor, |cursor| {
            parse_array(cursor, |cursor| {
                Some(SaveExperience {
                    experience: cursor.u16()?,
                    reserved: cursor.u16()?,
                    level: cursor.u16()?,
                    count: cursor.u16()?,
                })
            })
        })?;
        let player_roles = PlayerRoles::parse(cursor.take(PLAYER_ROLES_SIZE)?)?;
        let poisons = parse_array(&mut cursor, |cursor| {
            parse_array(cursor, |cursor| {
                Some(SavePoison {
                    poison_id: cursor.u16()?,
                    script: cursor.u16()?,
                })
            })
        })?;
        let inventory = parse_array(&mut cursor, |cursor| {
            Some(SaveInventoryEntry {
                item_id: cursor.u16()?,
                amount: cursor.u16()?,
                amount_in_use: cursor.u16()?,
            })
        })?;
        let scenes = parse_array(&mut cursor, parse_scene)?;

        let object_bytes = object_record_size.checked_mul(SAVE_OBJECT_CAPACITY)?;
        let objects = GlobalObjects::parse(cursor.take(object_bytes)?, layout)?;
        let event_objects = (0..event_count)
            .map(|_| parse_event_object(&mut cursor))
            .collect::<Option<Vec<_>>>()?;
        if cursor.remaining() != 0 {
            return None;
        }

        Some(Self {
            layout,
            saved_times,
            viewport_x,
            viewport_y,
            party_member_index,
            scene_number,
            night_palette: palette_offset != 0,
            party_direction,
            music_number,
            battle_music_number,
            battlefield_number,
            screen_wave,
            battle_speed,
            collect_value,
            layer,
            chase_range,
            chase_speed_change_cycles,
            follower_count,
            cash,
            party,
            trail,
            experience,
            player_roles,
            poisons,
            inventory,
            scenes,
            objects,
            event_objects,
        })
    }

    pub fn party_member_count(&self) -> usize {
        usize::from(self.party_member_index) + 1
    }
}

fn detect_layout(length: usize) -> Option<(ObjectLayout, usize, usize)> {
    let candidate = |fixed_size: usize| {
        let tail = length.checked_sub(fixed_size)?;
        tail.is_multiple_of(EVENT_OBJECT_RECORD_SIZE)
            .then_some(tail / EVENT_OBJECT_RECORD_SIZE)
            .filter(|&count| count <= SAVE_EVENT_OBJECT_CAPACITY)
    };
    match (
        candidate(DOS_SAVE_FIXED_SIZE),
        candidate(WIN_SAVE_FIXED_SIZE),
    ) {
        (Some(event_count), None) => Some((ObjectLayout::Dos, DOS_OBJECT_RECORD_SIZE, event_count)),
        (None, Some(event_count)) => {
            Some((ObjectLayout::Win95, WIN_OBJECT_RECORD_SIZE, event_count))
        }
        _ => None,
    }
}

fn parse_scene(cursor: &mut Cursor<'_>) -> Option<Scene> {
    Some(Scene {
        map_num: cursor.u16()?,
        script_on_enter: cursor.u16()?,
        script_on_teleport: cursor.u16()?,
        event_object_index: cursor.u16()?,
    })
}

fn parse_event_object(cursor: &mut Cursor<'_>) -> Option<EventObject> {
    Some(EventObject {
        vanish_time: cursor.i16()?,
        x: cursor.u16()?,
        y: cursor.u16()?,
        layer: cursor.i16()?,
        trigger_script: cursor.u16()?,
        auto_script: cursor.u16()?,
        state: cursor.i16()?,
        trigger_mode: cursor.u16()?,
        sprite_num: cursor.u16()?,
        sprite_frames: cursor.u16()?,
        direction: cursor.u16()?,
        current_frame: cursor.u16()?,
        script_idle_frame: cursor.u16()?,
        sprite_ptr_offset: cursor.u16()?,
        auto_sprite_frames: cursor.u16()?,
        auto_script_idle_frame: cursor.u16()?,
    })
}

fn parse_array<T, const N: usize>(
    cursor: &mut Cursor<'_>,
    mut parse: impl FnMut(&mut Cursor<'_>) -> Option<T>,
) -> Option<[T; N]> {
    (0..N)
        .map(|_| parse(cursor))
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()
}

struct Cursor<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len() - self.offset
    }

    fn take(&mut self, length: usize) -> Option<&'a [u8]> {
        let end = self.offset.checked_add(length)?;
        let bytes = self.data.get(self.offset..end)?;
        self.offset = end;
        Some(bytes)
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }

    fn i16(&mut self) -> Option<i16> {
        Some(i16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_u16(data: &mut [u8], offset: usize, value: u16) {
        data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn write_i16(data: &mut [u8], offset: usize, value: i16) {
        data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn fixture(layout: ObjectLayout, event_count: usize) -> Vec<u8> {
        let (fixed_size, object_size) = match layout {
            ObjectLayout::Dos => (DOS_SAVE_FIXED_SIZE, DOS_OBJECT_RECORD_SIZE),
            ObjectLayout::Win95 => (WIN_SAVE_FIXED_SIZE, WIN_OBJECT_RECORD_SIZE),
        };
        let mut data = vec![0; fixed_size + event_count * EVENT_OBJECT_RECORD_SIZE];
        write_u16(&mut data, 0, 12);
        write_i16(&mut data, 2, -24);
        write_i16(&mut data, 4, 48);
        write_u16(&mut data, 6, 1);
        write_u16(&mut data, 8, 7);
        write_u16(&mut data, 10, 0x180);
        write_u16(&mut data, 12, 2);
        write_u16(&mut data, 14, 31);
        write_u16(&mut data, 16, 5);
        write_u16(&mut data, 18, 9);
        write_u16(&mut data, 32, 1);
        data[40..44].copy_from_slice(&123_456u32.to_le_bytes());

        write_u16(&mut data, HEADER_SIZE, 2);
        write_i16(&mut data, HEADER_SIZE + 2, -10);
        write_i16(&mut data, HEADER_SIZE + 4, 20);
        write_u16(&mut data, HEADER_SIZE + PARTY_RECORD_SIZE, 4);

        let object_start = COMMON_SIZE;
        write_u16(&mut data, object_start, 10);
        write_u16(&mut data, object_start + (object_size - 2), 0x55aa);

        if event_count > 0 {
            write_i16(&mut data, fixed_size, -3);
            write_u16(&mut data, fixed_size + 2, 100);
            write_u16(&mut data, fixed_size + 4, 200);
            write_u16(&mut data, fixed_size + 8, 345);
        }
        data
    }

    #[test]
    fn parses_dos_save_and_normalizes_objects() {
        let save = OriginalSave::parse(&fixture(ObjectLayout::Dos, 2)).unwrap();
        assert_eq!(save.layout, ObjectLayout::Dos);
        assert_eq!(save.saved_times, 12);
        assert_eq!((save.viewport_x, save.viewport_y), (-24, 48));
        assert_eq!(save.party_member_count(), 2);
        assert_eq!(save.scene_number, 7);
        assert!(save.night_palette);
        assert_eq!(save.music_number, 31);
        assert_eq!(save.cash, 123_456);
        assert_eq!(save.party[0].role_id, 2);
        assert_eq!(save.party[1].role_id, 4);
        assert_eq!(save.objects.layout(), ObjectLayout::Dos);
        assert_eq!(save.objects.get(0).unwrap().data[0], 10);
        assert_eq!(save.objects.get(0).unwrap().data[6], 0x55aa);
        assert_eq!(save.event_objects.len(), 2);
        assert_eq!(save.event_objects[0].vanish_time, -3);
        assert_eq!(save.event_objects[0].trigger_script, 345);
    }

    #[test]
    fn parses_win95_save_object_layout() {
        let save = OriginalSave::parse(&fixture(ObjectLayout::Win95, 1)).unwrap();
        assert_eq!(save.layout, ObjectLayout::Win95);
        assert_eq!(save.objects.get(0).unwrap().data[6], 0x55aa);
        assert_eq!(save.event_objects.len(), 1);
    }

    #[test]
    fn accepts_zero_event_objects_and_rejects_bad_lengths() {
        assert!(OriginalSave::parse(&fixture(ObjectLayout::Dos, 0)).is_some());
        assert!(OriginalSave::parse(&fixture(ObjectLayout::Win95, 0)).is_some());
        assert!(OriginalSave::parse(&vec![0; DOS_SAVE_FIXED_SIZE - 1]).is_none());

        let mut trailing = fixture(ObjectLayout::Dos, 0);
        trailing.push(0);
        assert!(OriginalSave::parse(&trailing).is_none());

        let too_many = vec![
            0;
            DOS_SAVE_FIXED_SIZE
                + (SAVE_EVENT_OBJECT_CAPACITY + 1) * EVENT_OBJECT_RECORD_SIZE
        ];
        assert!(OriginalSave::parse(&too_many).is_none());
    }

    #[test]
    fn rejects_invalid_header_and_active_party_indices() {
        let mut save = fixture(ObjectLayout::Dos, 0);
        write_u16(&mut save, 6, SAVE_PARTY_CAPACITY as u16);
        assert!(OriginalSave::parse(&save).is_none());

        let mut save = fixture(ObjectLayout::Dos, 0);
        write_u16(&mut save, 8, 0);
        assert!(OriginalSave::parse(&save).is_none());

        let mut save = fixture(ObjectLayout::Dos, 0);
        write_u16(&mut save, 8, SAVE_SCENE_CAPACITY as u16);
        assert!(OriginalSave::parse(&save).is_none());

        let mut save = fixture(ObjectLayout::Dos, 0);
        write_u16(&mut save, 12, 4);
        assert!(OriginalSave::parse(&save).is_none());

        let mut save = fixture(ObjectLayout::Dos, 0);
        write_u16(&mut save, HEADER_SIZE, SAVE_ROLE_COUNT as u16);
        assert!(OriginalSave::parse(&save).is_none());
    }
}
