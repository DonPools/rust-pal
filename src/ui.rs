use std::time::{Duration, Instant};

use crate::{game::Game, input::PalKey, sprite::{draw_sprite, sprite_get_count, sprite_get_frame}, utils::*};

pub struct MenuItem {
    pub value: u16,
    pub num_word: u32,
    pub enabled: bool,
    pub x: u16,
    pub y: u16,
}

pub const CHUNKNUM_SPRITEUI: u32 = 9;

pub const MAINMENU_BACKGROUND_FBPNUM: u32 = 60;
pub const RIX_NUM_OPENINGMENU: u32 = 4;

pub const MAINMENU_LABEL_NEWGAME: u32 = 7;
pub const MAINMENU_LABEL_LOADGAME: u32 = 8;
pub const LOADMENU_LABEL_SLOT_FIRST: u32 = 43;

pub const MENUITEM_COLOR: u8 = 0x4f;
pub const MENUITEM_COLOR_SELECTED_FIRST: u32 = 0xF9;
pub const MENUITEM_COLOR_SELECTED_TOTALNUM: u32 = 6;

impl Game {
    pub fn init_ui(&mut self) -> Result<()> {
        let chunk = self.mkf.fp.read_chunk(CHUNKNUM_SPRITEUI)?;
        let count = sprite_get_count(&chunk);
        for i in 0..count {
            let sprite = sprite_get_frame(&chunk, i)?;
            self.ui_sprites.push(sprite);
        }

        Ok(())
    }

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
                        true,
                    );
                }
            }

            self.canvas.set_pixels(|_pixels: &mut [u8]| {
                _pixels.copy_from_slice(&pixels);
            });

            self.blit_to_screen()?;
            self.process_event();

            if self.input.is_pressed(PalKey::Down)
                || self.input.is_pressed(PalKey::Right)
            {
                selected_index = (menu_items.len() + selected_index + 1) % menu_items.len();
            } else if self.input.is_pressed(PalKey::Up)
                || self.input.is_pressed(PalKey::Left)
            {
                selected_index = (menu_items.len() + selected_index - 1) % menu_items.len();
            }

            if self.input.is_pressed(PalKey::Search) {
                return Result::Ok(menu_items[selected_index].value);
            }

            std::thread::sleep(Duration::from_millis(30));
        }
    }

    // len: number of mid box sprites
    pub fn draw_signle_linebox_with_shadow(&mut self, pos: Pos, len : u32) {
        let left_box_sprite = &self.ui_sprites[44];
        let mid_box_sprite = &self.ui_sprites[45];
        let right_box_sprite = &self.ui_sprites[46];

        let mut x = pos.x;
        let y = pos.y;

        self.canvas.set_pixels(|pixels: &mut [u8]| {
            draw_sprite(left_box_sprite, pixels, 320, 200, x, y);
            x += left_box_sprite.width as isize;
            for _ in 0..len {
                draw_sprite(mid_box_sprite, pixels, 320, 200, x, y);
                x += mid_box_sprite.width as isize;
            }
            draw_sprite(right_box_sprite, pixels, 320, 200, x, y);
        });        
    }
}
