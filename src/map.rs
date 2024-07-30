use crate::game::Game;
use crate::sprite::{draw_sprite_frame, sprite_get_frames};
use crate::utils::{Rect, Result};
use crate::{mkf::MKF, sprite::SpriteFrame};

pub struct Map {
    pub tiles: Vec<u32>,
    pub tile_sprite: Vec<SpriteFrame>,
    pub map_num: u32,
}

impl Map {
    pub fn load(map_mkf: &mut MKF, gop_mkf: &mut MKF, map_num: u32) -> Result<Self> {
        let mut tiles = vec![0; 128 * 64 * 2];
        if map_num >= map_mkf.chunk_count() || map_num >= gop_mkf.chunk_count() {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Index out of bounds",
            )));
        }

        let map_chunk = map_mkf.read_chunk_decompressed(map_num)?;
        for i in 0..128 * 64 * 2 {
            tiles[i] = u32::from_le_bytes([
                map_chunk[i * 4],
                map_chunk[i * 4 + 1],
                map_chunk[i * 4 + 2],
                map_chunk[i * 4 + 3],
            ]);
        }

        let gop_chunk = gop_mkf.read_chunk(map_num)?;
        let tile_sprite = sprite_get_frames(&gop_chunk)?;

        Ok(Self { tiles, tile_sprite, map_num })
    }

    pub fn get_tile_sprite(&self, x: isize, y: isize, h: isize, layer: usize) -> Option<&SpriteFrame> {
        if x >= 64 || y >= 128 || h > 1 || h < 0 || x < 0 || y < 0 {
            return None;
        }

        let i = (y * 64 + x) * 2 + h;
        let mut d = self.tiles[i as usize] as isize;
        if layer == 0 {
            let id = (d & 0xFF) | ((d >> 4) & 0x100);
            self.tile_sprite.get(id as usize)
        } else {
            d = d >> 16;
            d = ((d & 0xFF) | ((d >> 4) & 0x100)) - 1;
            self.tile_sprite.get(d as usize)
        }
    }
}

impl Game {
    pub fn draw_map(&mut self, map: &Map, rect: &Rect, layer: usize) {
        let sy = rect.y / 16 - 1;
        let dy = (rect.y + rect.h as isize) / 16 + 2;
        let sx = rect.x / 32 - 1;
        let dx = (rect.x + rect.w as isize) / 32 + 2;

        self.canvas.set_pixels(|pixels: &mut [u8]| {
            let mut y_pos = sy * 16 - 8 - rect.y;

            for y in sy..dy {
                for h in 0..2 {
                    let mut x_pos = sx * 32 + h * 16 - 16 - rect.x;
                    for x in sx..dx {
                        let sprite = match map.get_tile_sprite(x, y, h, layer) {
                            Some(sprite) => Some(sprite),
                            None => map.get_tile_sprite(0, 0, 0, layer),
                        };

                        if let Some(frame) = sprite {
                            draw_sprite_frame(frame, pixels, 320, 200, x_pos, y_pos);
                        }
                        x_pos += 32;
                    }
                    y_pos += 8;
                }
            }
        });
    }
}
