//! Player role definitions stored in `DATA.MKF` chunk 3.

pub const PLAYER_ROLE_COUNT: usize = 6;
pub const PLAYER_EQUIPMENT_COUNT: usize = 6;
pub const PLAYER_MAGIC_COUNT: usize = 32;
pub const MAGIC_ELEMENT_COUNT: usize = 5;

const PLAYER_ARRAY_BYTES: usize = PLAYER_ROLE_COUNT * 2;
const PLAYER_ROLE_ARRAY_COUNT: usize = 75;
const PLAYER_ROLES_BYTES: usize = PLAYER_ROLE_ARRAY_COUNT * PLAYER_ARRAY_BYTES;

/// Initial attributes and resource references for one playable role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerRole {
    pub avatar: u16,
    pub battle_sprite_num: u16,
    pub scene_sprite_num: u16,
    pub name_word_id: u16,
    pub attack_all: bool,
    pub level: u16,
    pub max_hp: u16,
    pub max_mp: u16,
    pub hp: u16,
    pub mp: u16,
    pub equipment: [u16; PLAYER_EQUIPMENT_COUNT],
    pub attack_strength: u16,
    pub magic_strength: u16,
    pub defense: u16,
    pub dexterity: u16,
    pub flee_rate: u16,
    pub poison_resistance: u16,
    pub elemental_resistance: [u16; MAGIC_ELEMENT_COUNT],
    pub covered_by: u16,
    pub magic: [u16; PLAYER_MAGIC_COUNT],
    pub walk_frames: u16,
    pub cooperative_magic: u16,
    pub unknown_5: u16,
    pub unknown_6: u16,
    pub death_sound: u16,
    pub attack_sound: u16,
    pub weapon_sound: u16,
    pub critical_sound: u16,
    pub magic_sound: u16,
    pub cover_sound: u16,
    pub dying_sound: u16,
}

impl PlayerRole {
    /// PAL stores four-frame walks explicitly and otherwise uses three frames.
    pub fn frames_per_direction(&self) -> u8 {
        if self.walk_frames == 4 {
            4
        } else {
            3
        }
    }
}

/// The complete six-role `PLAYERROLES` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerRoles {
    roles: [PlayerRole; PLAYER_ROLE_COUNT],
    raw: [u8; PLAYER_ROLES_BYTES],
}

impl PlayerRoles {
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < PLAYER_ROLES_BYTES {
            return None;
        }
        let roles: [Option<PlayerRole>; PLAYER_ROLE_COUNT] =
            std::array::from_fn(|role_index| parse_role(data, role_index));
        Some(Self {
            roles: roles
                .into_iter()
                .collect::<Option<Vec<_>>>()?
                .try_into()
                .ok()?,
            raw: data.get(..PLAYER_ROLES_BYTES)?.try_into().ok()?,
        })
    }

    pub fn role(&self, role_index: usize) -> Option<&PlayerRole> {
        self.roles.get(role_index)
    }

    pub fn role_mut(&mut self, role_index: usize) -> Option<&mut PlayerRole> {
        self.roles.get_mut(role_index)
    }

    pub fn from_roles(roles: [PlayerRole; PLAYER_ROLE_COUNT]) -> Self {
        Self {
            roles,
            raw: [0; PLAYER_ROLES_BYTES],
        }
    }

    pub fn cloned_roles(&self) -> [PlayerRole; PLAYER_ROLE_COUNT] {
        self.roles.clone()
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = &PlayerRole> {
        self.roles.iter()
    }

    /// Encode mutable role fields while retaining unmodeled original words.
    pub fn encode(&self) -> [u8; PLAYER_ROLES_BYTES] {
        let mut data = self.raw;
        for (role_index, role) in self.roles.iter().enumerate() {
            let mut write = |array: usize, value: u16| {
                let offset = (array * PLAYER_ROLE_COUNT + role_index) * 2;
                data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
            };
            write(0, role.avatar);
            write(1, role.battle_sprite_num);
            write(2, role.scene_sprite_num);
            write(3, role.name_word_id);
            write(4, u16::from(role.attack_all));
            write(6, role.level);
            write(7, role.max_hp);
            write(8, role.max_mp);
            write(9, role.hp);
            write(10, role.mp);
            for (index, &value) in role.equipment.iter().enumerate() {
                write(11 + index, value);
            }
            write(17, role.attack_strength);
            write(18, role.magic_strength);
            write(19, role.defense);
            write(20, role.dexterity);
            write(21, role.flee_rate);
            write(22, role.poison_resistance);
            for (index, &value) in role.elemental_resistance.iter().enumerate() {
                write(23 + index, value);
            }
            write(31, role.covered_by);
            for (index, &value) in role.magic.iter().enumerate() {
                write(32 + index, value);
            }
            write(64, role.walk_frames);
            write(65, role.cooperative_magic);
            write(66, role.unknown_5);
            write(67, role.unknown_6);
            write(68, role.death_sound);
            write(69, role.attack_sound);
            write(70, role.weapon_sound);
            write(71, role.critical_sound);
            write(72, role.magic_sound);
            write(73, role.cover_sound);
            write(74, role.dying_sound);
        }
        data
    }
}

/// Backwards-compatible graphics-only view used by scene setup code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerRoleGraphics {
    pub sprite_num: u16,
    pub walk_frames: u16,
}

impl PlayerRoleGraphics {
    pub fn parse(data: &[u8], role_index: usize) -> Option<Self> {
        let roles = PlayerRoles::parse(data)?;
        let role = roles.role(role_index)?;
        Some(Self {
            sprite_num: role.scene_sprite_num,
            walk_frames: role.walk_frames,
        })
    }
}

fn parse_role(data: &[u8], role: usize) -> Option<PlayerRole> {
    Some(PlayerRole {
        avatar: read_player_array(data, 0, role)?,
        battle_sprite_num: read_player_array(data, 1, role)?,
        scene_sprite_num: read_player_array(data, 2, role)?,
        name_word_id: read_player_array(data, 3, role)?,
        attack_all: read_player_array(data, 4, role)? != 0,
        level: read_player_array(data, 6, role)?,
        max_hp: read_player_array(data, 7, role)?,
        max_mp: read_player_array(data, 8, role)?,
        hp: read_player_array(data, 9, role)?,
        mp: read_player_array(data, 10, role)?,
        equipment: read_array(data, 11, role)?,
        attack_strength: read_player_array(data, 17, role)?,
        magic_strength: read_player_array(data, 18, role)?,
        defense: read_player_array(data, 19, role)?,
        dexterity: read_player_array(data, 20, role)?,
        flee_rate: read_player_array(data, 21, role)?,
        poison_resistance: read_player_array(data, 22, role)?,
        elemental_resistance: read_array(data, 23, role)?,
        covered_by: read_player_array(data, 31, role)?,
        magic: read_array(data, 32, role)?,
        walk_frames: read_player_array(data, 64, role)?,
        cooperative_magic: read_player_array(data, 65, role)?,
        unknown_5: read_player_array(data, 66, role)?,
        unknown_6: read_player_array(data, 67, role)?,
        death_sound: read_player_array(data, 68, role)?,
        attack_sound: read_player_array(data, 69, role)?,
        weapon_sound: read_player_array(data, 70, role)?,
        critical_sound: read_player_array(data, 71, role)?,
        magic_sound: read_player_array(data, 72, role)?,
        cover_sound: read_player_array(data, 73, role)?,
        dying_sound: read_player_array(data, 74, role)?,
    })
}

fn read_array<const N: usize>(data: &[u8], first_array: usize, role: usize) -> Option<[u16; N]> {
    let values: [Option<u16>; N] =
        std::array::from_fn(|index| read_player_array(data, first_array + index, role));
    values
        .into_iter()
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()
}

fn read_player_array(data: &[u8], array: usize, role: usize) -> Option<u16> {
    if role >= PLAYER_ROLE_COUNT {
        return None;
    }
    let offset = array
        .checked_mul(PLAYER_ARRAY_BYTES)?
        .checked_add(role.checked_mul(2)?)?;
    Some(u16::from_le_bytes(
        data.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Vec<u8> {
        let mut data = vec![0; PLAYER_ROLES_BYTES];
        for array in 0..PLAYER_ROLE_ARRAY_COUNT {
            for role in 0..PLAYER_ROLE_COUNT {
                let value = (array * 10 + role) as u16;
                let offset = array * PLAYER_ARRAY_BYTES + role * 2;
                data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
            }
        }
        data
    }

    #[test]
    fn parses_role_attributes_and_nested_arrays() {
        let roles = PlayerRoles::parse(&table()).unwrap();
        let role = roles.role(5).unwrap();
        assert_eq!(role.scene_sprite_num, 25);
        assert_eq!(role.name_word_id, 35);
        assert_eq!(role.level, 65);
        assert_eq!(role.equipment, [115, 125, 135, 145, 155, 165]);
        assert_eq!(role.elemental_resistance, [235, 245, 255, 265, 275]);
        assert_eq!(role.magic[0], 325);
        assert_eq!(role.magic[31], 635);
        assert_eq!(role.walk_frames, 645);
        assert_eq!(role.cooperative_magic, 655);
        assert_eq!(role.death_sound, 685);
        assert_eq!(role.dying_sound, 745);
        assert_eq!(roles.iter().len(), PLAYER_ROLE_COUNT);
    }

    #[test]
    fn normalizes_walking_frame_count() {
        let mut data = table();
        let offset = 64 * PLAYER_ARRAY_BYTES;
        data[offset..offset + 2].copy_from_slice(&4u16.to_le_bytes());
        assert_eq!(
            PlayerRoles::parse(&data)
                .unwrap()
                .role(0)
                .unwrap()
                .frames_per_direction(),
            4
        );
        assert_eq!(
            PlayerRoles::parse(&data)
                .unwrap()
                .role(1)
                .unwrap()
                .frames_per_direction(),
            3
        );
    }

    #[test]
    fn rejects_truncated_data_and_out_of_range_role() {
        assert!(PlayerRoles::parse(&table()[..PLAYER_ROLES_BYTES - 1]).is_none());
        let roles = PlayerRoles::parse(&table()).unwrap();
        assert!(roles.role(PLAYER_ROLE_COUNT).is_none());
        assert!(PlayerRoleGraphics::parse(&table(), PLAYER_ROLE_COUNT).is_none());
    }

    #[test]
    fn encoding_retains_unmodeled_words_and_applies_role_changes() {
        let data = table();
        let mut roles = PlayerRoles::parse(&data).unwrap();
        roles.role_mut(2).unwrap().level = 99;

        let encoded = roles.encode();
        let level_offset = (6 * PLAYER_ROLE_COUNT + 2) * 2;
        assert_eq!(
            u16::from_le_bytes(encoded[level_offset..level_offset + 2].try_into().unwrap()),
            99
        );
        for array in [5, 28, 29, 30] {
            let start = array * PLAYER_ARRAY_BYTES;
            let end = start + PLAYER_ARRAY_BYTES;
            assert_eq!(&encoded[start..end], &data[start..end]);
        }
    }
}
