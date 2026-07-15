//! PAL isometric map loading and tile lookup.

use pal_assets::mkf::MkfArchive;
use pal_assets::rle::RleBitmap;
use pal_assets::sprite::Sprite;
use pal_assets::yj1;

pub const MAP_ROWS: usize = 128;
pub const MAP_COLUMNS: usize = 64;
pub const MAP_HALVES: usize = 2;
pub const MAP_PIXEL_WIDTH: i32 = MAP_COLUMNS as i32 * 32;
pub const MAP_PIXEL_HEIGHT: i32 = MAP_ROWS as i32 * 16 + 8;
const MAP_DATA_LEN: usize = MAP_ROWS * MAP_COLUMNS * MAP_HALVES * 4;

/// Convert an interleaved map tile coordinate to a logical world position.
pub fn tile_to_world(x: usize, y: usize, h: usize) -> Option<(i32, i32)> {
    if x >= MAP_COLUMNS || y >= MAP_ROWS || h >= MAP_HALVES {
        return None;
    }
    Some((x as i32 * 32 + h as i32 * 16, y as i32 * 16 + h as i32 * 8))
}

/// Convert an aligned logical world position to an interleaved map tile.
pub fn world_to_tile(world_x: i32, world_y: i32) -> Option<(usize, usize, usize)> {
    if world_x < 0 || world_y < 0 || world_x % 16 != 0 {
        return None;
    }
    let h = usize::from(world_x % 32 != 0);
    if world_y % 16 != h as i32 * 8 {
        return None;
    }
    let x = usize::try_from(world_x / 32).ok()?;
    let y = usize::try_from(world_y / 16).ok()?;
    (x < MAP_COLUMNS && y < MAP_ROWS).then_some((x, y, h))
}

/// A loaded map and its tile sprite sheet.
pub struct Map {
    /// Tile words in `[y][x][h]` order.
    tiles: Vec<[[u32; MAP_HALVES]; MAP_COLUMNS]>,
    pub tile_sprite: Sprite,
    pub map_num: usize,
}

impl Map {
    /// Load a one-based map number from complete MAP.MKF and GOP.MKF files.
    pub fn load(map_num: usize, map_mkf: &[u8], gop_mkf: &[u8]) -> Option<Self> {
        let map_archive = MkfArchive::new(map_mkf)?;
        let gop_archive = MkfArchive::new(gop_mkf)?;
        if map_num == 0
            || map_num >= map_archive.chunk_count()
            || map_num >= gop_archive.chunk_count()
        {
            return None;
        }

        let decompressed = yj1::decompress(map_archive.read_chunk(map_num)?)?;
        if decompressed.len() != MAP_DATA_LEN {
            return None;
        }

        let mut tiles = vec![[[0; MAP_HALVES]; MAP_COLUMNS]; MAP_ROWS];
        for (index, bytes) in decompressed.chunks_exact(4).enumerate() {
            let y = index / (MAP_COLUMNS * MAP_HALVES);
            let remainder = index % (MAP_COLUMNS * MAP_HALVES);
            let x = remainder / MAP_HALVES;
            let h = remainder % MAP_HALVES;
            tiles[y][x][h] = u32::from_le_bytes(bytes.try_into().ok()?);
        }

        let tile_sprite = Sprite::from_gop_chunk(gop_archive.read_chunk(map_num)?)?;
        Some(Self {
            tiles,
            tile_sprite,
            map_num,
        })
    }

    pub fn tile_word(&self, x: usize, y: usize, h: usize) -> Option<u32> {
        Some(*self.tiles.get(y)?.get(x)?.get(h)?)
    }

    /// Return the first tile that references a non-zero bottom frame.
    pub fn first_occupied_tile(&self) -> Option<(usize, usize, usize)> {
        for y in 0..MAP_ROWS {
            for x in 0..MAP_COLUMNS {
                for h in 0..MAP_HALVES {
                    if self.get_bottom_tile_index(x, y, h)? != 0 {
                        return Some((x, y, h));
                    }
                }
            }
        }
        None
    }

    pub fn get_bottom_tile_index(&self, x: usize, y: usize, h: usize) -> Option<usize> {
        let word = self.tile_word(x, y, h)?;
        Some(((word & 0xff) | ((word >> 4) & 0x100)) as usize)
    }

    pub fn get_top_tile_index(&self, x: usize, y: usize, h: usize) -> Option<usize> {
        let word = self.tile_word(x, y, h)? >> 16;
        let encoded = (word & 0xff) | ((word >> 4) & 0x100);
        encoded.checked_sub(1).map(|index| index as usize)
    }

    pub fn is_tile_blocked(&self, x: usize, y: usize, h: usize) -> bool {
        self.tile_word(x, y, h)
            .map(|word| word & 0x2000 != 0)
            .unwrap_or(true)
    }

    /// Check collision at an aligned logical world position.
    pub fn is_world_blocked(&self, world_x: i32, world_y: i32) -> bool {
        let Some((x, y, h)) = world_to_tile(world_x, world_y) else {
            return true;
        };
        self.is_tile_blocked(x, y, h)
    }

    pub fn tile_height(&self, x: usize, y: usize, h: usize, top: bool) -> Option<u8> {
        let mut word = self.tile_word(x, y, h)?;
        if top {
            word >>= 16;
        }
        Some(((word >> 8) & 0x0f) as u8)
    }

    pub fn decode_bottom_tile(&self, x: usize, y: usize, h: usize) -> Option<RleBitmap> {
        self.tile_sprite
            .decode_frame(self.get_bottom_tile_index(x, y, h)?)
    }

    pub fn decode_top_tile(&self, x: usize, y: usize, h: usize) -> Option<RleBitmap> {
        self.tile_sprite
            .decode_frame(self.get_top_tile_index(x, y, h)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_and_world_coordinates_round_trip() {
        for coordinate in [(0, 0, 0), (1, 2, 1), (MAP_COLUMNS - 1, MAP_ROWS - 1, 1)] {
            let world = tile_to_world(coordinate.0, coordinate.1, coordinate.2).unwrap();
            assert_eq!(world_to_tile(world.0, world.1), Some(coordinate));
        }
    }

    #[test]
    fn world_coordinates_reject_bounds_and_unaligned_positions() {
        assert_eq!(world_to_tile(-16, 0), None);
        assert_eq!(world_to_tile(1, 0), None);
        assert_eq!(world_to_tile(16, 0), None);
        assert_eq!(world_to_tile(MAP_PIXEL_WIDTH, 0), None);
    }
}
