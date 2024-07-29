use crate::game::Game;
use crate::input::PalKey;
use crate::map::Map;
use crate::utils::*;

impl Game {
    pub fn mainloop(&mut self) -> Result<()> {
        self.set_palette(0)?;
        let mut map_id = 1;
        loop {
            let map = Map::load(&mut self.mkf.map, &mut self.mkf.gop, map_id)?;

            let mut x = 300;
            let mut y = 300;
            loop {
                self.draw_map(&map, &Rect{x, y, w: 320, h: 200}, 0);
                self.draw_map(&map, &Rect{x, y, w: 320, h: 200}, 1);

                self.blit_to_screen()?;
                self.process_event();

                //println!("dir: {:?}", self.input.dir);
                match self.input.dir {
                    Dir::North => y -= 30,
                    Dir::East => x += 30,
                    Dir::South => y += 30,
                    Dir::West => x -= 30,
                    _ => {},
                }

                if self.input.is_pressed(PalKey::Search) {
                    break;
                }    

                std::thread::sleep(std::time::Duration::from_millis(30));
            }

            map_id += 1;
        }                
        Ok(())
    }
}