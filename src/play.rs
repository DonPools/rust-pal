use crate::data::ObjectState;
use crate::game::Game;
use crate::input::PalKey;
use crate::scene::Map;
use crate::sprite::{ draw_sprite_frame, sprite_get_frames, Sprite, SpriteFrame };
use crate::utils::*;

pub struct Resource {
    pub map: Map,
    pub event_object_sprites: Vec<Sprite>,
    pub player_sprites: Vec<Sprite>,
}

impl Game {
    pub fn load_resource(&mut self) -> Result<()> {
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

        self.resource = Some(Resource {
            map,
            event_object_sprites,
            player_sprites: Vec::new(),
        });

        Ok(())
    }

    pub fn mainloop(&mut self) -> Result<()> {
        self.set_palette(0)?;
        loop {
            self.load_resource()?;

            loop {
                self.make_scence();
                self.blit_to_screen()?;
                self.process_event();

                //println!("dir: {:?}", self.input.dir);
                match self.input.dir {
                    Dir::North => {
                        self.state.viewport.y -= 30;
                    }
                    Dir::East => {
                        self.state.viewport.x += 30;
                    }
                    Dir::South => {
                        self.state.viewport.y += 30;
                    }
                    Dir::West => {
                        self.state.viewport.x -= 30;
                    }
                    _ => {}
                }

                if self.input.is_pressed(PalKey::Search) {
                    self.state.entering_scene = true;
                    self.state.scene_num += 1;
                    break;
                }

                std::thread::sleep(std::time::Duration::from_millis(30));
            }
        }

        Ok(())
    }
}
