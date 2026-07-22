use crate::map::{Map, MAP_PIXEL_HEIGHT, MAP_PIXEL_WIDTH};
use crate::role::Direction;

/// Platform-independent commands sampled for one fixed update.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct GameInput {
    pub direction: Option<Direction>,
    /// Direction pressed during this sample, for edge-triggered UI controls.
    pub direction_pressed: Option<Direction>,
    pub confirm: bool,
    pub cancel: bool,
}

/// Collision boundary required by the exploration state.
pub trait CollisionMap {
    fn is_world_blocked(&self, world_x: i32, world_y: i32) -> bool;
    fn world_size(&self) -> (i32, i32);
}

impl CollisionMap for Map {
    fn is_world_blocked(&self, world_x: i32, world_y: i32) -> bool {
        self.is_world_blocked(world_x, world_y)
    }

    fn world_size(&self) -> (i32, i32) {
        (MAP_PIXEL_WIDTH, MAP_PIXEL_HEIGHT)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Camera {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Camera {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            x: 0,
            y: 0,
            width,
            height,
        }
    }

    pub fn follow(&mut self, target: (i32, i32), world_size: (i32, i32)) {
        let max_x = (world_size.0 - self.width as i32).max(0);
        let max_y = (world_size.1 - self.height as i32).max(0);
        self.x = (target.0 - self.width as i32 / 2).clamp(0, max_x);
        self.y = (target.1 - self.height as i32 / 2).clamp(0, max_y);
    }
}
