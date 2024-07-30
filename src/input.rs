use std::path::Display;

use minifb::{Key, KeyRepeat, Window};

use crate::game::Game;
use crate::utils::*;

#[derive(Debug, Clone, Copy, PartialEq)]
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
    KeyCode { code: Key::Up, key: PalKey::Up },
    KeyCode { code: Key::NumPad8, key: PalKey::Up },
    KeyCode { code: Key::Down, key: PalKey::Down },
    KeyCode { code: Key::NumPad2, key: PalKey::Down },
    KeyCode { code: Key::Left, key: PalKey::Left },
    KeyCode { code: Key::NumPad4, key: PalKey::Left },
    KeyCode { code: Key::Right, key: PalKey::Right },
    KeyCode { code: Key::NumPad6, key: PalKey::Right },
    KeyCode { code: Key::Escape, key: PalKey::Menu },
    KeyCode { code: Key::Insert, key: PalKey::Menu },
    KeyCode { code: Key::LeftAlt, key: PalKey::Menu },
    KeyCode { code: Key::RightAlt, key: PalKey::Menu },
    KeyCode { code: Key::NumPad0, key: PalKey::Menu },
    KeyCode { code: Key::Enter, key: PalKey::Search },
    KeyCode { code: Key::Space, key: PalKey::Search },
    KeyCode { code: Key::NumPadEnter, key: PalKey::Search },
    KeyCode { code: Key::LeftCtrl, key: PalKey::Search },
    KeyCode { code: Key::PageUp, key: PalKey::PgUp },
    KeyCode { code: Key::NumPad9, key: PalKey::PgUp },
    KeyCode { code: Key::PageDown, key: PalKey::PgDn },
    KeyCode { code: Key::NumPad3, key: PalKey::PgDn },
    KeyCode { code: Key::Home, key: PalKey::Home },
    KeyCode { code: Key::NumPad7, key: PalKey::Home },
    KeyCode { code: Key::End, key: PalKey::End },
    KeyCode { code: Key::NumPad1, key: PalKey::End },
    KeyCode { code: Key::R, key: PalKey::Repeat },
    KeyCode { code: Key::A, key: PalKey::Auto },
    KeyCode { code: Key::D, key: PalKey::Defend },
    KeyCode { code: Key::E, key: PalKey::UseItem },
    KeyCode { code: Key::W, key: PalKey::ThrowItem },
    KeyCode { code: Key::Q, key: PalKey::Flee },
    KeyCode { code: Key::F, key: PalKey::Force },
    KeyCode { code: Key::S, key: PalKey::Status },
];

pub struct InputState {
    pub dir: Dir,
    pub key_press: u32,
    pub key_order: [u32; 4],
    pub key_max_count: u32,
    pub key_last_time: [u32; KEY_COUNT],
}

impl InputState {
    pub fn new() -> Self {
        InputState {
            dir: Dir::Unknown,
            key_press: 0,
            key_order: [0; 4],
            key_max_count: 0,
            key_last_time: [0; KEY_COUNT],
        }
    }

    #[inline]
    pub fn is_pressed(&self, key: PalKey) -> bool {
        (self.key_press & (key as u32)) != 0
    }

    #[inline]
    pub fn is_any_pressed(&self) -> bool {
        self.key_press != 0
    }

    fn key_to_dir(&self, key: PalKey) -> Dir {
        match key {
            PalKey::Left => Dir::West,
            PalKey::Right => Dir::East,
            PalKey::Up => Dir::North,
            PalKey::Down => Dir::South,
            _ => Dir::Unknown,
        }
    }

    fn get_max_key_count(&self) -> (u32, usize) {
        let mut max_count = 0;
        let mut idx = 0;
        for i in 0..4 {
            if self.key_order[i] > max_count {
                max_count = self.key_order[i];
                idx = i;
            }
        }

        (max_count, idx)
    }

    fn get_cur_dir(&self) -> Dir {
        let (_, idx) = self.get_max_key_count();
        if self.key_order[idx] == 0 {
            return Dir::Unknown;
        }

        return Dir::from_u8(idx as u8);
    }

    fn update_state(&mut self, window: &Window, ticks: u32) {
        let cur_time = ticks;

        for i in 0..KEY_COUNT {
            let key_code = &KEY_MAP[i];
            if window.is_key_down(key_code.code) {
                if cur_time > self.key_last_time[i] {
                    let is_repeat = self.key_last_time[i] != 0;
                    let delay = if self.key_last_time[i] == 0 {
                        200 as u32
                    } else {
                        75 as u32
                    };

                    self.key_last_time[i] = cur_time + delay;
                    if !is_repeat {
                        let dir = InputState::key_to_dir(self, key_code.key);

                        if dir != Dir::Unknown {
                            self.key_max_count += 1;
                            self.key_order[dir as usize] = self.key_max_count;
                            self.dir = self.get_cur_dir();
                        }
                    }

                    self.key_press |= key_code.key as u32;
                }
            } else {
                if self.key_last_time[i] != 0 {
                    let dir = InputState::key_to_dir(self, key_code.key);
                    if dir != Dir::Unknown {
                        self.key_order[dir as usize] = 0;
                        let cur_dir = self.get_cur_dir();
                        self.key_max_count = if cur_dir == Dir::Unknown {
                            0
                        } else {
                            self.key_order[cur_dir.clone() as usize]
                        };
                        self.dir = cur_dir;
                    }

                    self.key_last_time[i] = 0;
                }
            }
        }
    }
}

impl Game {
    pub fn update_keyboard_state(&mut self) {}

    pub fn process_event(&mut self) {
        if !self.window.is_open() {
            std::process::exit(0);
        }

        self.input.key_press = 0;
        self.input.update_state(&self.window, self.ticks())
        //println!("key_press: {}", self.input_state.key_press);
    }
}
