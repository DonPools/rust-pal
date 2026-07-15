//! Selected fields from the `PLAYERROLES` table in `DATA.MKF` chunk 3.

pub const PLAYER_ROLE_COUNT: usize = 6;
const PLAYER_ARRAY_BYTES: usize = PLAYER_ROLE_COUNT * 2;
const SPRITE_NUM_ARRAY_INDEX: usize = 2;
const WALK_FRAMES_ARRAY_INDEX: usize = 64;

/// Graphics settings for one playable role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerRoleGraphics {
    /// Normal-scene sprite slot in `MGO.MKF`.
    pub sprite_num: u16,
    /// Frames stored for each walking direction. Zero uses the legacy default.
    pub walk_frames: u16,
}

impl PlayerRoleGraphics {
    /// Read one role from an uncompressed `PLAYERROLES` table.
    pub fn parse(data: &[u8], role_index: usize) -> Option<Self> {
        if role_index >= PLAYER_ROLE_COUNT {
            return None;
        }
        Some(Self {
            sprite_num: read_player_array(data, SPRITE_NUM_ARRAY_INDEX, role_index)?,
            walk_frames: read_player_array(data, WALK_FRAMES_ARRAY_INDEX, role_index)?,
        })
    }
}

fn read_player_array(data: &[u8], array_index: usize, role_index: usize) -> Option<u16> {
    let offset = array_index
        .checked_mul(PLAYER_ARRAY_BYTES)?
        .checked_add(role_index.checked_mul(2)?)?;
    Some(u16::from_le_bytes(
        data.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_graphics_fields_for_selected_role() {
        let mut data = vec![0; WALK_FRAMES_ARRAY_INDEX * PLAYER_ARRAY_BYTES + PLAYER_ARRAY_BYTES];
        let role_index = PLAYER_ROLE_COUNT - 1;
        let sprite_offset = SPRITE_NUM_ARRAY_INDEX * PLAYER_ARRAY_BYTES + role_index * 2;
        let walk_offset = WALK_FRAMES_ARRAY_INDEX * PLAYER_ARRAY_BYTES + role_index * 2;
        data[sprite_offset..sprite_offset + 2].copy_from_slice(&42u16.to_le_bytes());
        data[walk_offset..walk_offset + 2].copy_from_slice(&3u16.to_le_bytes());

        assert_eq!(
            PlayerRoleGraphics::parse(&data, role_index),
            Some(PlayerRoleGraphics {
                sprite_num: 42,
                walk_frames: 3,
            })
        );
    }

    #[test]
    fn rejects_truncated_data_and_out_of_range_role() {
        assert_eq!(PlayerRoleGraphics::parse(&[], 0), None);
        let data = vec![0; WALK_FRAMES_ARRAY_INDEX * PLAYER_ARRAY_BYTES + PLAYER_ARRAY_BYTES];
        assert_eq!(PlayerRoleGraphics::parse(&data, PLAYER_ROLE_COUNT), None);
    }
}
