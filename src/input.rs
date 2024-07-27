use minifb::{Key, KeyRepeat};

use crate::game::{Game, GameState};
use crate::utils::*;

#[derive(Debug, Clone, Copy)]
pub enum PalKey {
    None = 0,
    Menu = 1 << 0,
    Search = 1 << 1,
    Down = 1 << 2,
    Left = 1 << 3,
    Up = 1 << 4,
    Right = 1 << 5,
    PgUp = 1 << 6,
    PgDn = 1 << 7,
    Repeat = 1 << 8,
    Auto = 1 << 9,
    Defend = 1 << 10,
    UseItem = 1 << 11,
    ThrowItem = 1 << 12,
    Flee = 1 << 13,
    Status = 1 << 14,
    Force = 1 << 15,
    Home = 1 << 16,
    End = 1 << 17,
}

struct KeyCode {
    pub code: Key,
    pub key: PalKey,
}
const KEY_COUNT: usize = 33;

const KEY_MAP: [KeyCode; KEY_COUNT] = [
    KeyCode {
        code: Key::Up,
        key: PalKey::Up,
    },
    KeyCode {
        code: Key::NumPad8,
        key: PalKey::Up,
    },
    KeyCode {
        code: Key::Down,
        key: PalKey::Down,
    },
    KeyCode {
        code: Key::NumPad2,
        key: PalKey::Down,
    },
    KeyCode {
        code: Key::Left,
        key: PalKey::Left,
    },
    KeyCode {
        code: Key::NumPad4,
        key: PalKey::Left,
    },
    KeyCode {
        code: Key::Right,
        key: PalKey::Right,
    },
    KeyCode {
        code: Key::NumPad6,
        key: PalKey::Right,
    },
    KeyCode {
        code: Key::Escape,
        key: PalKey::Menu,
    },
    KeyCode {
        code: Key::Insert,
        key: PalKey::Menu,
    },
    KeyCode {
        code: Key::LeftAlt,
        key: PalKey::Menu,
    },
    KeyCode {
        code: Key::RightAlt,
        key: PalKey::Menu,
    },
    KeyCode {
        code: Key::NumPad0,
        key: PalKey::Menu,
    },
    KeyCode {
        code: Key::Enter,
        key: PalKey::Search,
    },
    KeyCode {
        code: Key::Space,
        key: PalKey::Search,
    },
    KeyCode {
        code: Key::NumPadEnter,
        key: PalKey::Search,
    },
    KeyCode {
        code: Key::LeftCtrl,
        key: PalKey::Search,
    },
    KeyCode {
        code: Key::PageUp,
        key: PalKey::PgUp,
    },
    KeyCode {
        code: Key::NumPad9,
        key: PalKey::PgUp,
    },
    KeyCode {
        code: Key::PageDown,
        key: PalKey::PgDn,
    },
    KeyCode {
        code: Key::NumPad3,
        key: PalKey::PgDn,
    },
    KeyCode {
        code: Key::Home,
        key: PalKey::Home,
    },
    KeyCode {
        code: Key::NumPad7,
        key: PalKey::Home,
    },
    KeyCode {
        code: Key::End,
        key: PalKey::End,
    },
    KeyCode {
        code: Key::NumPad1,
        key: PalKey::End,
    },
    KeyCode {
        code: Key::R,
        key: PalKey::Repeat,
    },
    KeyCode {
        code: Key::A,
        key: PalKey::Auto,
    },
    KeyCode {
        code: Key::D,
        key: PalKey::Defend,
    },
    KeyCode {
        code: Key::E,
        key: PalKey::UseItem,
    },
    KeyCode {
        code: Key::W,
        key: PalKey::ThrowItem,
    },
    KeyCode {
        code: Key::Q,
        key: PalKey::Flee,
    },
    KeyCode {
        code: Key::F,
        key: PalKey::Force,
    },
    KeyCode {
        code: Key::S,
        key: PalKey::Status,
    },
];

pub struct InputState {
    pub dir: Dir,
    pub prev_dir: Dir,
    pub key_press: u32,
    pub key_order: [u32; 4],
    pub key_max_count: u32,
    pub key_last_time: [u32; KEY_COUNT],
}

impl InputState {
    pub fn new() -> Self {
        InputState {
            dir: Dir::Unknown,
            prev_dir: Dir::Unknown,
            key_press: 0,
            key_order: [0; 4],
            key_max_count: 0,
            key_last_time: [0; KEY_COUNT],
        }
    }

    pub fn is_pressed(&self, key: PalKey) -> bool {
        self.key_press & key as u32 != 0
    }

    pub fn is_any_pressed(&self) -> bool {
        self.key_press != 0
    }
}

impl Game {
    pub fn update_keyboard_state(&mut self) {
        let cur_time = self.ticks();

        //println!("keyboard_state: {:?}", keyboard_state);
        for i in 0..KEY_COUNT {
            let key_code = &KEY_MAP[i];
            if self.window.is_key_pressed(key_code.code, KeyRepeat::Yes) {
                if cur_time > self.input_state.key_last_time[i] {
                    self.input_state.key_press |= key_code.key as u32;
                    let is_repeat = self.input_state.key_last_time[i] != 0;
                    let delay = if self.input_state.key_last_time[i] == 0 {
                        200 as u32
                    } else {
                        75 as u32
                    };
                    self.input_state.key_last_time[i] = cur_time + delay;
                    if !is_repeat {                        
                        // TODO
                    }
                }                 
            } else {
                self.input_state.key_last_time[i] = 0;
            }
        }
    }

    pub fn process_event(&mut self) {
        if !self.window.is_open() {
            std::process::exit(0);
        }                
        self.input_state.key_press = 0;
        self.update_keyboard_state();        
        //println!("key_press: {}", self.input_state.key_press);
    }
}
