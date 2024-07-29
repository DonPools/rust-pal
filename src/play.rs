use crate::game::Game;
use crate::map::Map;
use crate::utils::*;

impl Game {
    pub fn mainloop(&mut self) -> Result<()> {
        self.set_palette(0)?;
        let map = Map::load(&mut self.mkf.map, &mut self.mkf.gop, 1)?;

        let mut x = 300;
        let mut y = 300;
        loop {
            self.draw_map(&map, &Rect{x, y, w: 320, h: 200}, 0);
            self.draw_map(&map, &Rect{x, y, w: 320, h: 200}, 1);

            self.blit_to_screen()?;
            self.process_event();

            //println!("dir: {:?}", self.input.dir);
            match self.input.dir {
                Dir::North => y -= 5,
                Dir::East => x += 5,
                Dir::South => y += 5,
                Dir::West => x -= 5,
                _ => {},
            }


            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        Ok(())
    }
}