use crate::game::Game;
use crate::map::Map;
use crate::utils::*;

impl Game {
    pub fn mainloop(&mut self) -> Result<()> {
        self.set_palette(0)?;
        let map = Map::load(&mut self.mkf.map, &mut self.mkf.gop, 35)?;

        let mut x = 300;
        let mut y = 300;
        loop {
            self.draw_map(&map, &Rect{x, y, w: 320, h: 200}, 0);
            self.draw_map(&map, &Rect{x, y, w: 320, h: 200}, 1);

            self.blit_to_screen()?;
            self.process_event();

            if self.input_state.is_pressed(crate::input::PalKey::Left) {
                x -= 30;
            } else if self.input_state.is_pressed(crate::input::PalKey::Right) {
                x += 30;
            } else if self.input_state.is_pressed(crate::input::PalKey::Up) {
                y -= 30;
            } else if self.input_state.is_pressed(crate::input::PalKey::Down) {
                y += 30;
            }

            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        Ok(())
    }
}