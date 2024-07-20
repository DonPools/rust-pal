use crate::pal::Pal;

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

/*
#define MENUITEM_COLOR_SELECTED                                    \
   (MENUITEM_COLOR_SELECTED_FIRST +                                \
      SDL_GetTicks() / (600 / MENUITEM_COLOR_SELECTED_TOTALNUM)    \
      % MENUITEM_COLOR_SELECTED_TOTALNUM)
 */

impl Pal {
    pub fn menu_color_selected(&self) -> u8 {
        let ticks = self.timer.ticks();
        let ticks = ticks / (600 / MENUITEM_COLOR_SELECTED_TOTALNUM);
        let ticks = ticks % MENUITEM_COLOR_SELECTED_TOTALNUM;
        ticks as u8 + MENUITEM_COLOR_SELECTED_FIRST as u8
    }
}
