//! Deterministic exploration state and update rules.

use crate::map::{Map, MAP_PIXEL_HEIGHT, MAP_PIXEL_WIDTH};
use crate::role::{Direction, Role};
use crate::scene::{
    blocks_position, find_search_trigger, find_touch_trigger, SceneObject, TriggerRequest,
};

pub const UPDATE_INTERVAL_MS: u64 = 50;

/// Platform-independent commands sampled for one fixed update.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct GameInput {
    pub direction: Option<Direction>,
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

/// State for the current exploration scene.
pub struct GameState<M = Map> {
    pub map: M,
    pub player: Role,
    pub scene_objects: Vec<SceneObject>,
    pub pending_trigger: Option<TriggerRequest>,
    pub camera: Camera,
}

impl<M: CollisionMap> GameState<M> {
    pub fn new(map: M, player: Role, viewport_width: u32, viewport_height: u32) -> Self {
        let mut state = Self {
            map,
            player,
            scene_objects: Vec::new(),
            pending_trigger: None,
            camera: Camera::new(viewport_width, viewport_height),
        };
        state.follow_player();
        state
    }

    pub fn with_scene_objects(mut self, scene_objects: Vec<SceneObject>) -> Self {
        self.scene_objects = scene_objects;
        self
    }

    pub fn take_trigger(&mut self) -> Option<TriggerRequest> {
        self.pending_trigger.take()
    }

    /// Advance one fixed update and report whether visible state changed.
    pub fn update(&mut self, input: GameInput) -> bool {
        if self.pending_trigger.is_some() {
            return false;
        }
        let mut changed = false;
        for object in &mut self.scene_objects {
            changed |= object.update_vanish_time();
        }
        self.pending_trigger = find_touch_trigger(
            &mut self.scene_objects,
            self.player.world_x,
            self.player.world_y,
        );
        if self.pending_trigger.is_none() && input.confirm {
            self.pending_trigger = find_search_trigger(
                &mut self.scene_objects,
                self.player.world_x,
                self.player.world_y,
                self.player.direction,
            );
        }
        if self.pending_trigger.is_some() {
            return true;
        }
        let Some(direction) = input.direction else {
            if self.player.anim_frame == 0 {
                return changed;
            }
            self.player.anim_frame = 0;
            return true;
        };

        changed |= self.player.direction != direction;
        self.player.direction = direction;
        let (dx, dy) = direction.step();
        let target = (self.player.world_x + dx, self.player.world_y + dy);
        if !self.map.is_world_blocked(target.0, target.1)
            && !blocks_position(&self.scene_objects, target.0, target.1)
        {
            self.player.world_x = target.0;
            self.player.world_y = target.1;
            self.player.anim_frame =
                (self.player.anim_frame + 1) % self.player.frames_per_direction.max(1);
            self.follow_player();
            changed = true;
        } else if self.player.anim_frame != 0 {
            self.player.anim_frame = 0;
            changed = true;
        }
        changed
    }

    fn follow_player(&mut self) {
        self.camera.follow(
            (self.player.world_x, self.player.world_y),
            self.map.world_size(),
        );
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn blocking_object(world_x: i32, world_y: i32) -> SceneObject {
        SceneObject {
            id: 1,
            world_x,
            world_y,
            layer: 0,
            trigger_script: 0,
            auto_script: 0,
            state: 2,
            trigger_mode: 0,
            sprite_index: Some(1),
            frames_per_direction: 3,
            sprite_frame_count: 12,
            direction: Direction::South,
            current_frame: 0,
            vanish_time: 0,
        }
    }

    struct TestMap {
        blocked: HashSet<(i32, i32)>,
        size: (i32, i32),
    }

    impl CollisionMap for TestMap {
        fn is_world_blocked(&self, world_x: i32, world_y: i32) -> bool {
            self.blocked.contains(&(world_x, world_y))
        }

        fn world_size(&self) -> (i32, i32) {
            self.size
        }
    }

    fn state(blocked: &[(i32, i32)]) -> GameState<TestMap> {
        GameState::new(
            TestMap {
                blocked: blocked.iter().copied().collect(),
                size: (1000, 800),
            },
            Role {
                sprite_index: 0,
                world_x: 320,
                world_y: 240,
                direction: Direction::South,
                anim_frame: 0,
                frames_per_direction: 4,
            },
            320,
            200,
        )
    }

    #[test]
    fn walking_updates_position_animation_and_camera() {
        let mut state = state(&[]);
        assert!(state.update(GameInput {
            direction: Some(Direction::East),
            ..GameInput::default()
        }));
        assert_eq!((state.player.world_x, state.player.world_y), (336, 248));
        assert_eq!(state.player.anim_frame, 1);
        assert_eq!((state.camera.x, state.camera.y), (176, 148));
    }

    #[test]
    fn blocked_walking_only_changes_facing() {
        let mut state = state(&[(336, 232)]);
        assert!(state.update(GameInput {
            direction: Some(Direction::North),
            ..GameInput::default()
        }));
        assert_eq!((state.player.world_x, state.player.world_y), (320, 240));
        assert_eq!(state.player.direction, Direction::North);
        assert_eq!(state.player.anim_frame, 0);
    }

    #[test]
    fn blocked_walking_resets_an_active_animation() {
        let mut state = state(&[(336, 248)]);
        state.player.anim_frame = 2;
        assert!(state.update(GameInput {
            direction: Some(Direction::East),
            ..GameInput::default()
        }));
        assert_eq!((state.player.world_x, state.player.world_y), (320, 240));
        assert_eq!(state.player.anim_frame, 0);
    }

    #[test]
    fn event_object_blockers_prevent_walking() {
        let mut state = state(&[]).with_scene_objects(vec![blocking_object(336, 248)]);
        assert!(state.update(GameInput {
            direction: Some(Direction::East),
            ..GameInput::default()
        }));
        assert_eq!((state.player.world_x, state.player.world_y), (320, 240));
        assert_eq!(state.player.direction, Direction::East);
    }

    #[test]
    fn touch_trigger_is_queued_and_pauses_exploration_until_consumed() {
        let mut object = blocking_object(330, 240);
        object.state = 1;
        object.trigger_mode = 4;
        object.trigger_script = 99;
        let mut state = state(&[]).with_scene_objects(vec![object]);

        assert!(state.update(GameInput {
            direction: Some(Direction::East),
            ..GameInput::default()
        }));
        assert_eq!((state.player.world_x, state.player.world_y), (320, 240));
        assert_eq!(state.pending_trigger.unwrap().script_entry, 99);
        assert!(!state.update(GameInput {
            direction: Some(Direction::East),
            ..GameInput::default()
        }));
        assert_eq!(state.take_trigger().unwrap().object_id, 1);
        assert!(state.pending_trigger.is_none());
    }

    #[test]
    fn confirm_queues_search_trigger_in_front_of_player() {
        let mut object = blocking_object(336, 248);
        object.state = 1;
        object.trigger_mode = 1;
        object.trigger_script = 77;
        let mut state = state(&[]).with_scene_objects(vec![object]);
        state.player.direction = Direction::East;

        assert!(state.update(GameInput {
            confirm: true,
            ..GameInput::default()
        }));
        let trigger = state.take_trigger().unwrap();
        assert_eq!(trigger.object_id, 1);
        assert_eq!(trigger.script_entry, 77);
        assert_eq!(state.scene_objects[0].direction, Direction::West);
    }

    #[test]
    fn idle_resets_animation_without_moving() {
        let mut state = state(&[]);
        state.player.anim_frame = 2;
        assert!(state.update(GameInput::default()));
        assert_eq!((state.player.world_x, state.player.world_y), (320, 240));
        assert_eq!(state.player.anim_frame, 0);
        assert!(!state.update(GameInput::default()));
    }

    #[test]
    fn camera_clamps_to_world_edges() {
        let mut camera = Camera::new(320, 200);
        camera.follow((10, 10), (1000, 800));
        assert_eq!((camera.x, camera.y), (0, 0));
        camera.follow((990, 790), (1000, 800));
        assert_eq!((camera.x, camera.y), (680, 600));
    }

    #[test]
    fn camera_handles_a_world_smaller_than_the_viewport() {
        let mut camera = Camera::new(320, 200);
        camera.follow((50, 40), (100, 80));
        assert_eq!((camera.x, camera.y), (0, 0));
    }
}
