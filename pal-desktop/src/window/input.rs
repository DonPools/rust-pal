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
}

impl HeldInput {
    pub(super) fn set_key(&mut self, key: KeyCode, pressed: bool, repeat: bool) {
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
            _ => {}
        }
    }

    pub(super) fn sample(&mut self) -> GameInput {
        let input = GameInput {
            direction: self.active_direction,
            direction_pressed: self.direction_pressed,
            confirm: self.confirm,
            cancel: self.cancel,
        };
        self.direction_pressed = None;
        self.confirm = false;
        self.cancel = false;
        input
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

        let first = input.sample();
        assert_eq!(first.direction, Some(Direction::South));
        assert_eq!(first.direction_pressed, Some(Direction::South));

        let second = input.sample();
        assert_eq!(second.direction, Some(Direction::South));
        assert_eq!(second.direction_pressed, None);
    }

    #[test]
    fn repeated_keydown_does_not_repeat_ui_direction() {
        let mut input = HeldInput::default();
        input.set_key(KeyCode::ArrowDown, true, false);
        input.sample();
        input.set_key(KeyCode::ArrowDown, true, true);

        assert_eq!(input.sample().direction_pressed, None);
    }
}
