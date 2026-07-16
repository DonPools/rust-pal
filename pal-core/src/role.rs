//! Scene-character sprite management and isometric positioning.

use pal_assets::mkf::MkfArchive;
use pal_assets::rle::RleBitmap;
use pal_assets::sprite::{sprite_from_yj1_chunk, Sprite};

/// PAL direction constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    South = 0,
    West = 1,
    North = 2,
    East = 3,
}

impl Direction {
    pub const ALL: [Self; 4] = [Self::South, Self::West, Self::North, Self::East];

    pub fn from_pal(value: u16) -> Option<Self> {
        match value {
            0 => Some(Self::South),
            1 => Some(Self::West),
            2 => Some(Self::North),
            3 => Some(Self::East),
            _ => None,
        }
    }

    /// One PAL walking step in logical world pixels.
    pub fn step(self) -> (i32, i32) {
        match self {
            Self::South => (-16, 8),
            Self::West => (-16, -8),
            Self::North => (16, -8),
            Self::East => (16, 8),
        }
    }

    pub fn opposite(self) -> Self {
        match self {
            Self::South => Self::North,
            Self::West => Self::East,
            Self::North => Self::South,
            Self::East => Self::West,
        }
    }
}

/// A character instance on the map.
#[derive(Debug, Clone)]
pub struct Role {
    /// Original `MGO.MKF` chunk index.
    pub sprite_index: usize,
    /// Logical world position used by movement and collision.
    pub world_x: i32,
    pub world_y: i32,
    pub direction: Direction,
    /// Animation-frame offset within the current direction.
    pub anim_frame: u8,
    /// Number of walking frames stored for each direction.
    pub frames_per_direction: u8,
}

impl Role {
    /// Compute the absolute sprite frame for a direction-grouped sprite.
    pub fn frame_index(&self) -> usize {
        self.direction as usize * self.frames_per_direction as usize + self.anim_frame as usize
    }

    /// Convert the map position to a bottom-center pixel anchor.
    pub fn screen_anchor(&self) -> (i32, i32) {
        // PAL draws party frames with their feet four pixels below the
        // logical position used by movement and obstacle checks.
        (self.world_x, self.world_y + 4)
    }
}

/// Scene-character sprites loaded from `MGO.MKF`.
///
/// Every non-empty chunk should be a YJ_1-compressed GOP sprite. Empty or
/// malformed chunks remain unavailable so their original MKF indices are
/// preserved and other valid sprites can still be used.
pub struct RoleSprites {
    sprites: Vec<Option<Sprite>>,
}

impl RoleSprites {
    /// Parse all sprite slots from raw `MGO.MKF` bytes.
    pub fn load(data: &[u8]) -> Option<Self> {
        let archive = MkfArchive::new(data)?;
        let sprites: Vec<Option<Sprite>> = (0..archive.chunk_count())
            .map(|index| {
                let chunk = archive.read_chunk(index)?;
                (!chunk.is_empty())
                    .then(|| sprite_from_yj1_chunk(chunk))
                    .flatten()
            })
            .collect();
        sprites
            .iter()
            .any(Option::is_some)
            .then_some(Self { sprites })
    }

    /// Number of sprite slots, including empty MKF chunks.
    pub fn character_count(&self) -> usize {
        self.sprites.len()
    }

    /// First MKF index containing a valid sprite.
    pub fn first_available_index(&self) -> Option<usize> {
        self.sprites.iter().position(Option::is_some)
    }

    pub fn character_frame_count(&self, index: usize) -> Option<usize> {
        Some(self.sprites.get(index)?.as_ref()?.frame_count())
    }

    /// Check that every frame in a direction-grouped walking animation decodes.
    pub fn has_directional_animation(&self, sprite_index: usize, frames_per_direction: u8) -> bool {
        let Some(required_frames) = Direction::ALL
            .len()
            .checked_mul(frames_per_direction as usize)
        else {
            return false;
        };
        self.character_frame_count(sprite_index)
            .is_some_and(|count| count >= required_frames)
            && (0..required_frames).all(|frame| self.decode_frame(sprite_index, frame).is_some())
    }

    pub fn decode_role_frame(&self, role: &Role) -> Option<RleBitmap> {
        self.decode_frame(role.sprite_index, role.frame_index())
    }

    pub fn decode_frame(&self, sprite_index: usize, frame_index: usize) -> Option<RleBitmap> {
        self.sprites
            .get(sprite_index)?
            .as_ref()?
            .decode_frame(frame_index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn role(direction: Direction, anim_frame: u8) -> Role {
        Role {
            sprite_index: 1,
            world_x: 32,
            world_y: 32,
            direction,
            anim_frame,
            frames_per_direction: 4,
        }
    }

    #[test]
    fn direction_values_match_pal() {
        assert_eq!(Direction::South as usize, 0);
        assert_eq!(Direction::West as usize, 1);
        assert_eq!(Direction::North as usize, 2);
        assert_eq!(Direction::East as usize, 3);
        assert_eq!(Direction::ALL.len(), 4);
    }

    #[test]
    fn frame_index_uses_direction_stride() {
        assert_eq!(role(Direction::South, 0).frame_index(), 0);
        assert_eq!(role(Direction::East, 2).frame_index(), 14);
        let three_frame_role = Role {
            frames_per_direction: 3,
            ..role(Direction::East, 2)
        };
        assert_eq!(three_frame_role.frame_index(), 11);
    }

    #[test]
    fn screen_anchor_uses_interleaved_map_coordinates() {
        assert_eq!(role(Direction::South, 0).screen_anchor(), (32, 36));
        let shifted = Role {
            world_x: 48,
            world_y: 40,
            ..role(Direction::South, 0)
        };
        assert_eq!(shifted.screen_anchor(), (48, 44));
    }

    #[test]
    fn walking_steps_match_pal_isometric_directions() {
        assert_eq!(Direction::South.step(), (-16, 8));
        assert_eq!(Direction::West.step(), (-16, -8));
        assert_eq!(Direction::North.step(), (16, -8));
        assert_eq!(Direction::East.step(), (16, 8));
    }

    #[test]
    fn rejects_invalid_archive() {
        assert!(RoleSprites::load(&[]).is_none());
    }
}
