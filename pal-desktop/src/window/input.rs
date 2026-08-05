use pal_core::game::GameInput;
use pal_core::role::Direction;
use winit::keyboard::KeyCode;

#[derive(Default)]
pub(super) struct HeldInput {
    south: bool,
    west: bool,
    north: bool,
    east: bool,
    active_direction: Option<Direction>,
    direction_pressed: Option<Direction>,
    confirm: bool,
    cancel: bool,
    battle_repeat: bool,
    battle_auto: bool,
    battle_defend: bool,
    battle_use_item: bool,
    battle_throw_item: bool,
    battle_flee: bool,
    battle_status: bool,
    battle_force: bool,
    any_pressed: bool,
}

impl HeldInput {
    pub(super) fn set_key(&mut self, key: KeyCode, pressed: bool, repeat: bool) {
        if pressed && !repeat {
            self.any_pressed = true;
        }
        let direction = match key {
            KeyCode::ArrowDown | KeyCode::KeyS => Some(Direction::South),
            KeyCode::ArrowLeft | KeyCode::KeyA => Some(Direction::West),
            KeyCode::ArrowUp | KeyCode::KeyW => Some(Direction::North),
            KeyCode::ArrowRight | KeyCode::KeyD => Some(Direction::East),
            _ => None,
        };
        if let Some(direction) = direction {
            *self.direction_held_mut(direction) = pressed;
            if pressed {
                self.active_direction = Some(direction);
                if !repeat {
                    self.direction_pressed = Some(direction);
                }
            } else if self.active_direction == Some(direction) {
                self.active_direction = self.first_held_direction();
            }
        }

        match key {
            KeyCode::Enter | KeyCode::Space if pressed && !repeat => self.confirm = true,
            KeyCode::Escape | KeyCode::Backspace if pressed && !repeat => self.cancel = true,
            KeyCode::KeyR if pressed && !repeat => self.battle_repeat = true,
            KeyCode::KeyA if pressed && !repeat => self.battle_auto = true,
            KeyCode::KeyD if pressed && !repeat => self.battle_defend = true,
            KeyCode::KeyE if pressed && !repeat => self.battle_use_item = true,
            KeyCode::KeyW if pressed && !repeat => self.battle_throw_item = true,
            KeyCode::KeyQ if pressed && !repeat => self.battle_flee = true,
            KeyCode::KeyS if pressed && !repeat => self.battle_status = true,
            KeyCode::KeyF if pressed && !repeat => self.battle_force = true,
            _ => {}
        }
    }

    pub(super) fn sample(&mut self) -> (GameInput, bool) {
        let input = GameInput {
            direction: self.active_direction,
            direction_pressed: self.direction_pressed,
            confirm: self.confirm,
            cancel: self.cancel,
            battle_repeat: self.battle_repeat,
            battle_auto: self.battle_auto,
            battle_defend: self.battle_defend,
            battle_use_item: self.battle_use_item,
            battle_throw_item: self.battle_throw_item,
            battle_flee: self.battle_flee,
            battle_status: self.battle_status,
            battle_force: self.battle_force,
        };
        self.direction_pressed = None;
        self.confirm = false;
        self.cancel = false;
        self.battle_repeat = false;
        self.battle_auto = false;
        self.battle_defend = false;
        self.battle_use_item = false;
        self.battle_throw_item = false;
        self.battle_flee = false;
        self.battle_status = false;
        self.battle_force = false;
        let any_pressed = std::mem::take(&mut self.any_pressed);
        (input, any_pressed)
    }

    fn direction_held_mut(&mut self, direction: Direction) -> &mut bool {
        match direction {
            Direction::South => &mut self.south,
            Direction::West => &mut self.west,
            Direction::North => &mut self.north,
            Direction::East => &mut self.east,
        }
    }

    fn first_held_direction(&self) -> Option<Direction> {
        [
            (Direction::South, self.south),
            (Direction::West, self.west),
            (Direction::North, self.north),
            (Direction::East, self.east),
        ]
        .into_iter()
        .find_map(|(direction, held)| held.then_some(direction))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direction_press_is_reported_once_but_held_direction_continues() {
        let mut input = HeldInput::default();
        input.set_key(KeyCode::ArrowDown, true, false);

        let (first, _) = input.sample();
        assert_eq!(first.direction, Some(Direction::South));
        assert_eq!(first.direction_pressed, Some(Direction::South));

        let (second, _) = input.sample();
        assert_eq!(second.direction, Some(Direction::South));
        assert_eq!(second.direction_pressed, None);
    }

    #[test]
    fn repeated_keydown_does_not_repeat_ui_direction() {
        let mut input = HeldInput::default();
        input.set_key(KeyCode::ArrowDown, true, false);
        input.sample();
        input.set_key(KeyCode::ArrowDown, true, true);

        assert_eq!(input.sample().0.direction_pressed, None);
    }

    #[test]
    fn any_non_repeated_key_is_reported_once() {
        let mut input = HeldInput::default();
        input.set_key(KeyCode::KeyQ, true, false);
        assert!(input.sample().1);
        assert!(!input.sample().1);
    }

    #[test]
    fn original_battle_shortcuts_are_edge_triggered() {
        let mut input = HeldInput::default();
        for key in [
            KeyCode::KeyR,
            KeyCode::KeyA,
            KeyCode::KeyD,
            KeyCode::KeyE,
            KeyCode::KeyW,
            KeyCode::KeyQ,
            KeyCode::KeyS,
            KeyCode::KeyF,
        ] {
            input.set_key(key, true, false);
        }
        let shortcuts = input.sample().0;
        assert!(shortcuts.battle_repeat);
        assert!(shortcuts.battle_auto);
        assert!(shortcuts.battle_defend);
        assert!(shortcuts.battle_use_item);
        assert!(shortcuts.battle_throw_item);
        assert!(shortcuts.battle_flee);
        assert!(shortcuts.battle_status);
        assert!(shortcuts.battle_force);

        let cleared = input.sample().0;
        assert!(!cleared.battle_repeat);
        assert!(!cleared.battle_auto);
        assert!(!cleared.battle_defend);
        assert!(!cleared.battle_use_item);
        assert!(!cleared.battle_throw_item);
        assert!(!cleared.battle_flee);
        assert!(!cleared.battle_status);
        assert!(!cleared.battle_force);
    }
}
