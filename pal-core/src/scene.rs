//! Platform-independent runtime state for scene event objects.

use pal_assets::scene::EventObject;

use crate::role::Direction;

pub const OBJECT_STATE_HIDDEN: i16 = 0;
pub const OBJECT_STATE_BLOCKER: i16 = 2;

/// Mutable state for an event object in the current scene.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneObject {
    /// One-based ID in the global `SSS.MKF` event-object table.
    pub id: u16,
    pub world_x: i32,
    pub world_y: i32,
    pub layer: i16,
    pub trigger_script: u16,
    pub auto_script: u16,
    pub state: i16,
    pub trigger_mode: u16,
    pub sprite_index: Option<usize>,
    pub frames_per_direction: u16,
    pub sprite_frame_count: usize,
    pub direction: Direction,
    pub current_frame: u16,
    pub vanish_time: i16,
}

impl SceneObject {
    /// Build runtime state and validate values needed for rendering.
    pub fn from_asset(id: u16, event: &EventObject, sprite_frame_count: usize) -> Option<Self> {
        let direction = Direction::from_pal(event.direction)?;
        let sprite_index = (event.sprite_num != 0).then_some(event.sprite_num as usize);
        if sprite_index.is_some() && sprite_frame_count == 0 {
            return None;
        }

        Some(Self {
            id,
            world_x: i32::from(event.x),
            world_y: i32::from(event.y),
            layer: event.layer,
            trigger_script: event.trigger_script,
            auto_script: event.auto_script,
            state: event.state,
            trigger_mode: event.trigger_mode,
            sprite_index,
            frames_per_direction: event.sprite_frames,
            sprite_frame_count,
            direction,
            current_frame: event.current_frame,
            vanish_time: event.vanish_time,
        })
    }

    /// Hidden and negative states are not rendered; positive vanish time also hides an object.
    pub fn is_visible(&self) -> bool {
        self.state > OBJECT_STATE_HIDDEN && self.vanish_time <= 0 && self.sprite_index.is_some()
    }

    pub fn is_blocker(&self) -> bool {
        self.state >= OBJECT_STATE_BLOCKER
    }

    /// Resolve PAL's direction-grouped frame layout and three-frame walk remapping.
    pub fn frame_index(&self) -> Option<usize> {
        self.sprite_index?;
        let current = usize::from(self.current_frame);
        let frame = if self.frames_per_direction == 0 {
            current
        } else {
            let animation_frame = if self.frames_per_direction == 3 {
                match current {
                    2 => 0,
                    3 => 2,
                    _ => current,
                }
            } else {
                current
            };
            (self.direction as usize)
                .checked_mul(self.frames_per_direction as usize)?
                .checked_add(animation_frame)?
        };
        (frame < self.sprite_frame_count).then_some(frame)
    }

    /// Advance one movement animation step. Script opcodes will call this when moving NPCs.
    pub fn advance_animation(&mut self) {
        let cycle = if self.frames_per_direction == 3 {
            4
        } else if self.frames_per_direction > 0 {
            self.frames_per_direction
        } else {
            u16::try_from(self.sprite_frame_count).unwrap_or(u16::MAX)
        };
        if cycle > 0 {
            self.current_frame = (self.current_frame + 1) % cycle;
        }
    }

    /// Move a temporary vanish timer toward zero as the original scene loop does.
    pub fn update_vanish_time(&mut self) -> bool {
        match self.vanish_time.cmp(&0) {
            std::cmp::Ordering::Less => self.vanish_time += 1,
            std::cmp::Ordering::Greater => self.vanish_time -= 1,
            std::cmp::Ordering::Equal => return false,
        }
        true
    }
}

/// PAL event-object collision uses a compressed vertical diamond distance.
pub fn blocks_position(objects: &[SceneObject], world_x: i32, world_y: i32) -> bool {
    objects.iter().any(|object| {
        object.is_blocker()
            && u64::from(object.world_x.abs_diff(world_x))
                + u64::from(object.world_y.abs_diff(world_y)) * 2
                < 16
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset() -> EventObject {
        EventObject {
            vanish_time: 0,
            x: 320,
            y: 200,
            layer: 1,
            trigger_script: 10,
            auto_script: 20,
            state: 1,
            trigger_mode: 2,
            sprite_num: 7,
            sprite_frames: 3,
            direction: 3,
            current_frame: 0,
            script_idle_frame: 0,
            sprite_ptr_offset: 0,
            auto_sprite_frames: 0,
            auto_script_idle_frame: 0,
        }
    }

    #[test]
    fn builds_runtime_state_and_maps_three_frame_animation() {
        let mut object = SceneObject::from_asset(42, &asset(), 12).unwrap();
        assert_eq!(object.id, 42);
        assert_eq!(object.sprite_index, Some(7));
        assert_eq!(object.direction, Direction::East);
        assert_eq!(object.frame_index(), Some(9));

        object.current_frame = 2;
        assert_eq!(object.frame_index(), Some(9));
        object.current_frame = 3;
        assert_eq!(object.frame_index(), Some(11));
        object.advance_animation();
        assert_eq!(object.current_frame, 0);
    }

    #[test]
    fn visibility_follows_state_vanish_time_and_sprite_presence() {
        let mut object = SceneObject::from_asset(1, &asset(), 12).unwrap();
        assert!(object.is_visible());
        object.vanish_time = 1;
        assert!(!object.is_visible());
        assert!(object.update_vanish_time());
        assert!(object.is_visible());
        object.state = OBJECT_STATE_HIDDEN;
        assert!(!object.is_visible());

        let mut no_sprite = asset();
        no_sprite.sprite_num = 0;
        assert!(!SceneObject::from_asset(2, &no_sprite, 0)
            .unwrap()
            .is_visible());
    }

    #[test]
    fn blocker_collision_uses_pal_diamond_distance() {
        let mut object = SceneObject::from_asset(1, &asset(), 12).unwrap();
        object.state = OBJECT_STATE_BLOCKER;
        assert!(blocks_position(&[object.clone()], 327, 204));
        assert!(!blocks_position(&[object.clone()], 328, 204));
        object.state = 1;
        assert!(!blocks_position(&[object], 320, 200));
    }

    #[test]
    fn rejects_invalid_direction_and_missing_sprite_data() {
        let mut event = asset();
        event.direction = 4;
        assert!(SceneObject::from_asset(1, &event, 12).is_none());
        event.direction = 0;
        assert!(SceneObject::from_asset(1, &event, 0).is_none());
    }
}
