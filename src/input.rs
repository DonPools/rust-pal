
use sdl2::keyboard::Scancode;
use sdl2::event::Event;
use crate::pal::Pal;
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
    pub code: Scancode,
    pub key: PalKey,
}
const KEY_COUNT: usize = 33;
const KEY_MAP: [KeyCode; KEY_COUNT] = [
    KeyCode { code: Scancode::Up, key: PalKey::Up },
    KeyCode { code: Scancode::Kp8, key: PalKey::Up },
    KeyCode { code: Scancode::Down, key: PalKey::Down },
    KeyCode { code: Scancode::Kp2, key: PalKey::Down },
    KeyCode { code: Scancode::Left, key: PalKey::Left },
    KeyCode { code: Scancode::Kp4, key: PalKey::Left },
    KeyCode { code: Scancode::Right, key: PalKey::Right },
    KeyCode { code: Scancode::Kp6, key: PalKey::Right },
    KeyCode { code: Scancode::Escape, key: PalKey::Menu },
    KeyCode { code: Scancode::Insert, key: PalKey::Menu },
    KeyCode { code: Scancode::LAlt, key: PalKey::Menu },
    KeyCode { code: Scancode::RAlt, key: PalKey::Menu },
    KeyCode { code: Scancode::Kp0, key: PalKey::Menu },
    KeyCode { code: Scancode::Return, key: PalKey::Search },
    KeyCode { code: Scancode::Space, key: PalKey::Search },
    KeyCode { code: Scancode::KpEnter, key: PalKey::Search },
    KeyCode { code: Scancode::LCtrl, key: PalKey::Search },
    KeyCode { code: Scancode::PageUp, key: PalKey::PgUp },
    KeyCode { code: Scancode::Kp9, key: PalKey::PgUp },
    KeyCode { code: Scancode::PageDown, key: PalKey::PgDn },
    KeyCode { code: Scancode::Kp3, key: PalKey::PgDn },
    KeyCode { code: Scancode::Home, key: PalKey::Home },
    KeyCode { code: Scancode::Kp7, key: PalKey::Home },
    KeyCode { code: Scancode::End, key: PalKey::End },
    KeyCode { code: Scancode::Kp1, key: PalKey::End },
    KeyCode { code: Scancode::R, key: PalKey::Repeat },
    KeyCode { code: Scancode::A, key: PalKey::Auto },
    KeyCode { code: Scancode::D, key: PalKey::Defend },
    KeyCode { code: Scancode::E, key: PalKey::UseItem },
    KeyCode { code: Scancode::W, key: PalKey::ThrowItem },
    KeyCode { code: Scancode::Q, key: PalKey::Flee },
    KeyCode { code: Scancode::F, key: PalKey::Force },
    KeyCode { code: Scancode::S, key: PalKey::Status }
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

    pub fn is_press(&self, key: PalKey) -> bool {
        self.key_press & key as u32 != 0
    }
}

impl Pal {
    pub fn update_keyboard_state(&mut self) {
        let cur_time = self.timer.ticks();

        let keyboard_state = sdl2::keyboard::KeyboardState::new(&self.event_pump);
        //println!("keyboard_state: {:?}", keyboard_state);
        for key_code in KEY_MAP.iter() {
            if keyboard_state.is_scancode_pressed(key_code.code) {
                self.input_state.key_press |= key_code.key as u32;               
            } else {

            }
        }
    }

    pub fn clear_keyboard_state(&mut self) {
        self.input_state.key_press = 0;
    }

    pub fn process_event(&mut self) {
        for event in self.event_pump.poll_iter() {
            match event {
                Event::Quit { .. } => {
                    unsafe {
                        sdl2::sys::SDL_Quit();
                    }                    
                    return;
                }
                _ => {}
            }
        }

        self.update_keyboard_state();
    }
}