use crate::data::ObjectState;
use crate::game::Game;
use crate::input::PalKey;
use crate::map::Map;
use crate::sprite::{ draw_sprite, sprite_get_count, sprite_get_frame, Sprite };
use crate::utils::*;

impl Game {
    pub fn mainloop(&mut self) -> Result<()> {
        self.set_palette(0)?;

        let i = (self.state.scene_num as usize) - 1;
        let scene = &self.data.scenes[i];
        let map = Map::load(&mut self.mkf.map, &mut self.mkf.gop, scene.map_num as u32)?;

        let index = self.data.scenes[i].event_object_index;
        let event_objects_count = self.data.scenes[i + 1].event_object_index - index;

        let event_objects =
            self.data.event_objects[
                index as usize..(index + event_objects_count) as usize
            ].to_vec();

        let mut event_object_sprites: Vec<Vec<Sprite>> = Vec::new();

        for event_object in event_objects.iter() {
            let sprite_num = event_object.sprite_num;
            let mut sprites = Vec::new();

            let chunk = self.mkf.mgo.read_chunk_decompressed(sprite_num as u32)?;
            let count = sprite_get_count(&chunk);
            for j in 0..count {
                let sprite = sprite_get_frame(&chunk, j)?;
                sprites.push(sprite);
            }

            event_object_sprites.push(sprites);
        }

        let mut viewport = Rect { x: 300, y: 300, w: 320, h: 200 };
        loop {
            self.draw_map(&map, &viewport, 0);
            self.draw_map(&map, &viewport, 1);
            self.canvas.set_pixels(|pixels: &mut [u8]| {
                for i in 0..event_objects_count {
                    let event_object = &event_objects[i as usize];
                    if event_object.state != ObjectState::Hidden as u16 {
                        continue;
                    }

                    if
                        (event_object.x as isize) < viewport.x ||
                        (event_object.x as isize) > viewport.x + (viewport.w as isize) ||
                        (event_object.y as isize) < viewport.y ||
                        (event_object.y as isize) > viewport.y + (viewport.h as isize)
                    {
                        continue;
                    }
    
                    let sprites = &event_object_sprites[i as usize];
                    if sprites.len() == 0 {
                        continue;
                    }

                    let x = event_object.x as isize - viewport.x - sprites[0].width as isize;
                    let y = event_object.y as isize - viewport.y - sprites[0].height as isize;
                    let sprite = &sprites[0];
                    draw_sprite(sprite, pixels, 320, 200, x, y);
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
