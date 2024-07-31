use crate::data::ObjectState;
use crate::game::Game;
use crate::input::PalKey;
use crate::map::Map;
use crate::sprite::{ draw_sprite_frame, sprite_get_frames, SpriteFrame };
use crate::utils::*;

impl Game {
    pub fn mainloop(&mut self) -> Result<()> {
        self.set_palette(0)?;

        let i = (self.state.scene_num as usize) - 1;
        let scene = &self.state.scenes[i];
        let map = Map::load(&mut self.mkf.map, &mut self.mkf.gop, scene.map_num as u32)?;

        let index = self.state.scenes[i].event_object_index;
        let event_objects_count = self.state.scenes[i + 1].event_object_index - index;

        let mut event_object_sprites: Vec<Vec<SpriteFrame>> = Vec::new();

        for event_object in self.state.event_objects[
            index as usize..(index + event_objects_count) as usize
        ].iter() {
            let sprite_num = event_object.sprite_num;

            let chunk = self.mkf.mgo.read_chunk_decompressed(sprite_num as u32)?;
            let sprite = sprite_get_frames(&chunk)?;

            event_object_sprites.push(sprite);
        }

        let mut viewport = Rect { x: 300, y: 300, w: 320, h: 200 };
        loop {
            self.draw_map(&map, &viewport, 0);
            self.draw_map(&map, &viewport, 1);
            self.canvas.set_pixels(|pixels: &mut [u8]| {
                for i in 0..event_objects_count {
                    let event_object = &self.state.event_objects[(index + i) as usize];
                    let sprites = &event_object_sprites[i as usize];

                    if event_object.state != (ObjectState::Hidden as u16) || sprites.len() == 0 {
                        continue;
                    }

                    let frame = &sprites[0];
                    let x = (event_object.x as isize) - viewport.x - ((frame.width / 2) as isize);
                    if x < -(frame.width as isize) || x >= 320 {
                        continue;
                    }
                    let y =
                        (event_object.y as isize) -
                        viewport.y +
                        ((event_object.layer as isize) * 8 + 9);
                    let vy = y - (frame.height as isize) - (event_object.layer as isize) * 8 + 2;
                    if vy >= 200 || vy < -(frame.height as isize) {
                        continue;
                    }

                    draw_sprite_frame(
                        frame,
                        pixels,
                        320,
                        200,
                        x,
                        y - (frame.height as isize) - (event_object.layer as isize)
                    );
                }
            });

            self.blit_to_screen()?;
            self.process_event();

            //println!("dir: {:?}", self.input.dir);
            match self.input.dir {
                Dir::North => {
                    viewport.y -= 30;
                }
                Dir::East => {
                    viewport.x += 30;
                }
                Dir::South => {
                    viewport.y += 30;
                }
                Dir::West => {
                    viewport.x -= 30;
                }
                _ => {}
            }

            if self.input.is_pressed(PalKey::Search) {
                break;
            }

            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        Ok(())
    }
}
