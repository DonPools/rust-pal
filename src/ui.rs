use std::time::Duration;

use crate::{
    game::Game,
    input::PalKey,
    utils::*,
};

pub struct MenuItem {
    pub value: u16,
    pub num_word: u32,
    pub enabled: bool,
    pub x: u16,
    pub y: u16,
}

pub const MAINMENU_BACKGROUND_FBPNUM: u32 = 60;
pub const RIX_NUM_OPENINGMENU: u32 = 4;

pub const MAINMENU_LABEL_NEWGAME: u32 = 7;
pub const MAINMENU_LABEL_LOADGAME: u32 = 8;
pub const LOADMENU_LABEL_SLOT_FIRST: u32 =43;

pub const MENUITEM_COLOR: u8 = 0x4f;


pub const MENUITEM_COLOR_SELECTED_FIRST: u32 = 0xF9;
pub const MENUITEM_COLOR_SELECTED_TOTALNUM: u32 = 6;

impl Game {
    pub fn menu_color_selected(&self) -> u8 {
        let ticks = self.ticks();
        let ticks = ticks / (600 / MENUITEM_COLOR_SELECTED_TOTALNUM);
        let ticks = ticks % MENUITEM_COLOR_SELECTED_TOTALNUM;
        ticks as u8 + MENUITEM_COLOR_SELECTED_FIRST as u8
    }

    pub fn read_menu(&mut self, menu_items: &[MenuItem]) -> Result<u16> {
        let mut selected_index = 0;
        let mut pixels = self.canvas.get_pixels().to_vec();
        loop {
            for i in 0..menu_items.len() {
                let item = &menu_items[i];
                let color = if i == selected_index {
                    self.menu_color_selected()
                } else {
                    MENUITEM_COLOR
                };

                if item.enabled {
                    self.draw_word(
                        &mut pixels,
                        320,
                        200,
                        item.x as i32,
                        item.y as i32,
                        item.num_word as usize,
                        color,
                    );
                }
            }

            self.canvas.set_pixels(|_pixels: &mut [u8]| {
                _pixels.copy_from_slice(&pixels);
            });

            self.blit_to_screen()?;
            self.process_event();            

            if self.input_state.is_pressed(PalKey::Down) || self.input_state.is_pressed(PalKey::Right) {
                selected_index = (menu_items.len() + selected_index + 1) % menu_items.len();
            } else if self.input_state.is_pressed(PalKey::Up) || self.input_state.is_pressed(PalKey::Left) {
                selected_index = (menu_items.len() + selected_index - 1) % menu_items.len();
            }

            if self.input_state.is_pressed(PalKey::Search) {                
                return Result::Ok(menu_items[selected_index].value);
            }

            std::thread::sleep(Duration::from_millis(30));
        }
    }
}
