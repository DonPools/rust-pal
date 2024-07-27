use std::time::Duration;

use crate::{input::PalKey, pal::Pal, utils::*};

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
pub const MENUITEM_COLOR: u8 = 0x4f;

pub const MENUITEM_COLOR_SELECTED_FIRST: u32 = 0xF9;
pub const MENUITEM_COLOR_SELECTED_TOTALNUM: u32 = 6;

impl Pal {
    pub fn menu_color_selected(&self) -> u8 {
        let ticks = self.start_time.elapsed().as_millis() as u32;
        let ticks = ticks / (600 / MENUITEM_COLOR_SELECTED_TOTALNUM);
        let ticks = ticks % MENUITEM_COLOR_SELECTED_TOTALNUM;
        ticks as u8 + MENUITEM_COLOR_SELECTED_FIRST as u8
    }

    pub fn read_menu(&mut self, menu_items: &[MenuItem]) -> Result<u16> {
        let mut menu_selected = 0;
        let mut pixels = [0; 320 * 200];
        self.canvas.set_pixels(|_pixels: &mut [u8]| {
            pixels.copy_from_slice(_pixels);
        });

        loop {
            for item in menu_items.iter() {
                let color = if item.value == menu_selected {
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

            self.blit_to_screen();
            self.process_event();

            if self.input_state.is_pressed(PalKey::Up) || self.input_state.is_pressed(PalKey::Down)
            {
                menu_selected = (menu_selected + 1) % menu_items.len() as u16;
            }

            self.clear_keyboard_state();

            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
