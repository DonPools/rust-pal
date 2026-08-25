//! Scene updates, trigger polling, and scripted movement helpers.

use super::*;

impl<M: CollisionMap> GameState<M> {
    /// Move an event object one script tick toward a PAL tile position.
    pub fn walk_object_to(
        &mut self,
        object_id: u16,
        tile_x: u16,
        tile_y: u16,
        half: u16,
        speed: u8,
    ) -> Option<bool> {
        let target = tile_to_world(usize::from(tile_x), usize::from(tile_y), usize::from(half))?;
        let object = self.object_mut(object_id)?;
        Some(walk_scene_object_to(object, target, i32::from(speed)))
    }

    pub fn walk_player_to(
        &mut self,
        tile_x: u16,
        tile_y: u16,
        half: u16,
        speed: u8,
    ) -> Option<bool> {
        let target = tile_to_world(usize::from(tile_x), usize::from(tile_y), usize::from(half))?;
        let old_position = (self.player.world_x, self.player.world_y);
        let trail_direction = self.player.direction;
        let x_offset = target.0 - self.player.world_x;
        let y_offset = target.1 - self.player.world_y;
        self.player.direction = direction_toward(x_offset, y_offset);
        let speed = i32::from(speed);
        let completed = if x_offset.abs() < speed * 2 || y_offset.abs() < speed * 2 {
            self.player.world_x = target.0;
            self.player.world_y = target.1;
            self.player.anim_frame = 0;
            true
        } else {
            let (dx, dy) = self.player.direction.step_at_speed(speed);
            self.player.world_x += dx;
            self.player.world_y += dy;
            self.player.anim_frame =
                (self.player.anim_frame + 1) % self.player.frames_per_direction.max(1);
            false
        };
        if old_position != (self.player.world_x, self.player.world_y) {
            self.record_party_step(old_position, trail_direction);
        }
        self.follow_player();
        Some(completed)
    }

    pub fn ride_object_to(
        &mut self,
        object_id: u16,
        tile_x: u16,
        tile_y: u16,
        half: u16,
        speed: u8,
    ) -> Option<bool> {
        let target = tile_to_world(usize::from(tile_x), usize::from(tile_y), usize::from(half))?;
        self.object_state(object_id)?;
        let x_offset = target.0 - self.player.world_x;
        let y_offset = target.1 - self.player.world_y;
        self.player.direction = direction_toward(x_offset, y_offset);
        let speed = i32::from(speed);
        let dx = x_offset.clamp(-speed * 2, speed * 2);
        let dy = y_offset.clamp(-speed, speed);
        self.player.world_x += dx;
        self.player.world_y += dy;
        let completed = (self.player.world_x, self.player.world_y) == target;
        for follower in &mut self.party_followers {
            follower.world_x += dx;
            follower.world_y += dy;
        }
        {
            let object = self.object_mut(object_id)?;
            object.world_x += dx;
            object.world_y += dy;
        }
        self.party_trail.rotate_right(1);
        self.party_trail[0] = TrailPoint {
            world_x: self.player.world_x,
            world_y: self.player.world_y,
            direction: self.player.direction,
        };
        self.follow_player();
        Some(completed)
    }

    pub fn take_auto_script_sounds(&mut self) -> Vec<u16> {
        std::mem::take(&mut self.pending_auto_sounds)
    }

    pub fn take_auto_script_events(&mut self) -> Vec<ScriptEvent> {
        std::mem::take(&mut self.pending_auto_events)
    }

    pub fn take_auto_script_failure(&mut self) -> bool {
        std::mem::take(&mut self.pending_auto_script_failure)
    }

    pub fn party_contains_name(&self, name_word_id: u16) -> bool {
        self.party
            .members()
            .iter()
            .any(|member| member.attributes.name_word_id == name_word_id)
    }

    pub(super) fn object_mut(&mut self, id: u16) -> Option<&mut SceneObject> {
        if let Some(object) = self.scene_objects.iter_mut().find(|object| object.id == id) {
            return Some(object);
        }
        self.inactive_objects.get_mut(&id)
    }

    /// Advance one fixed update and report whether visible state changed.
    pub fn update(&mut self, input: GameInput) -> bool {
        if self.pending_trigger.is_some() {
            return false;
        }
        let mut changed = false;
        for object in &mut self.scene_objects {
            if object.update_vanish_time() {
                changed = true;
                continue;
            }
            if object.state < 0
                && (object.world_x < self.camera.x
                    || object.world_x > self.camera.x + 320
                    || object.world_y < self.camera.y
                    || object.world_y > self.camera.y + 320)
            {
                object.state = object.state.saturating_abs();
                object.current_frame = 0;
                changed = true;
            }
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
            return changed | self.stop_party_walking_animation();
        };

        changed |= self.player.direction != direction;
        self.player.direction = direction;
        let (dx, dy) = direction.step();
        let target = (self.player.world_x + dx, self.player.world_y + dy);
        if (!self.map_collision_enabled || !self.map.is_world_blocked(target.0, target.1))
            && !blocks_position(&self.scene_objects, target.0, target.1)
        {
            let old_position = (self.player.world_x, self.player.world_y);
            self.player.world_x = target.0;
            self.player.world_y = target.1;
            self.player.anim_frame =
                (self.player.anim_frame + 1) % self.player.frames_per_direction.max(1);
            self.record_party_step(old_position, direction);
            self.follow_player();
            changed = true;
        } else if self.player.anim_frame != 0 {
            self.player.anim_frame = 0;
            changed = true;
        }
        changed
    }
}
