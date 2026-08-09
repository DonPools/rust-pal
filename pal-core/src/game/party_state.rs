//! Party slots, followers, trail movement, and camera following.

use super::*;

impl<M: CollisionMap> GameState<M> {
    /// Reset the party to standing frames without advancing the world tick.
    ///
    /// Input adapters can use this when the final direction key is released so
    /// the 10 FPS exploration step does not add visible release latency.
    pub fn stop_party_walking_animation(&mut self) -> bool {
        let mut changed = self.player.anim_frame != 0;
        self.player.anim_frame = 0;
        for follower in &mut self.party_followers {
            changed |= follower.anim_frame != 0;
            follower.anim_frame = 0;
        }
        changed
    }

    pub(super) fn rebuild_party_followers(&mut self) {
        let mut follower_roles = self
            .party
            .members()
            .iter()
            .skip(1)
            .map(|member| member.attributes.clone())
            .collect::<Vec<_>>();
        if let Some(roles) = &self.player_roles {
            follower_roles.extend(
                self.extra_follower_ids
                    .iter()
                    .filter_map(|&role_id| roles.role(usize::from(role_id)).cloned()),
            );
        }
        self.party_followers = follower_roles
            .into_iter()
            .enumerate()
            .map(|(index, attributes)| {
                let slot = self.party_slots[index + 1];
                Role {
                    sprite_index: usize::from(attributes.scene_sprite_num),
                    world_x: slot.world_x,
                    world_y: slot.world_y,
                    direction: slot.direction,
                    anim_frame: slot.anim_frame,
                    frames_per_direction: attributes.frames_per_direction(),
                }
            })
            .collect();
    }

    pub(super) fn current_party_slots(&self) -> [PartySlotState; MAX_PARTY_MEMBERS] {
        let mut slots = self.party_slots;
        slots[0] = PartySlotState::from_role(&self.player);
        for (slot, follower) in slots.iter_mut().skip(1).zip(&self.party_followers) {
            *slot = PartySlotState::from_role(follower);
        }
        slots
    }

    pub(super) fn sync_active_party_slots(&mut self) {
        self.party_slots = self.current_party_slots();
    }

    pub(super) fn record_party_step(&mut self, old_position: (i32, i32), direction: Direction) {
        self.party_trail.rotate_right(1);
        self.party_trail[0] = TrailPoint {
            world_x: old_position.0,
            world_y: old_position.1,
            direction,
        };
        self.update_party_followers(true);
    }

    pub(super) fn update_party_followers(&mut self, walking: bool) {
        let base = self.party_trail[1];
        let facing = self.party_trail[2].direction;
        for (index, follower) in self.party_followers.iter_mut().enumerate() {
            let party_index = index + 1;
            let (dx, dy) = if party_index == 2 {
                (
                    if matches!(base.direction, Direction::East | Direction::West) {
                        -16
                    } else {
                        16
                    },
                    8,
                )
            } else {
                (
                    if matches!(base.direction, Direction::West | Direction::South) {
                        16
                    } else {
                        -16
                    },
                    if matches!(base.direction, Direction::West | Direction::North) {
                        8
                    } else {
                        -8
                    },
                )
            };
            follower.world_x = base.world_x + dx;
            follower.world_y = base.world_y + dy;
            follower.direction = facing;
            follower.anim_frame = if walking {
                (follower.anim_frame + 1) % follower.frames_per_direction.max(1)
            } else {
                0
            };
        }
    }

    pub(super) fn collapse_party(&mut self) {
        let point = TrailPoint {
            world_x: self.player.world_x,
            world_y: self.player.world_y,
            direction: self.player.direction,
        };
        self.party_trail.fill(point);
        for follower in &mut self.party_followers {
            follower.world_x = self.player.world_x;
            follower.world_y = self.player.world_y - 1;
            follower.direction = self.player.direction;
            follower.anim_frame = 0;
        }
        self.party_slots[0] = PartySlotState::from_role(&self.player);
        for slot in self.party_slots.iter_mut().skip(1) {
            *slot = PartySlotState {
                world_x: self.player.world_x,
                world_y: self.player.world_y - 1,
                direction: self.player.direction,
                anim_frame: 0,
            };
        }
    }

    pub(super) fn place_party(&mut self) {
        let (dx, dy) = (
            if matches!(self.player.direction, Direction::West | Direction::South) {
                16
            } else {
                -16
            },
            if matches!(self.player.direction, Direction::West | Direction::North) {
                8
            } else {
                -8
            },
        );
        for (index, point) in self.party_trail.iter_mut().enumerate() {
            point.world_x = self.player.world_x + dx * index as i32;
            point.world_y = self.player.world_y + dy * index as i32;
            point.direction = self.player.direction;
        }
        for (index, slot) in self.party_slots.iter_mut().enumerate() {
            *slot = PartySlotState {
                world_x: self.player.world_x + dx * index as i32,
                world_y: self.player.world_y + dy * index as i32,
                direction: self.player.direction,
                anim_frame: 0,
            };
        }
        self.party_slots[0].apply_to(&mut self.player);
        for (slot, follower) in self
            .party_slots
            .iter()
            .skip(1)
            .zip(&mut self.party_followers)
        {
            slot.apply_to(follower);
        }
    }

    pub(super) fn shift_party(&mut self, dx: i32, dy: i32) {
        let old_position = (self.player.world_x, self.player.world_y);
        self.player.world_x += dx;
        self.player.world_y += dy;
        if dx != 0 || dy != 0 {
            self.player.anim_frame =
                (self.player.anim_frame + 1) % self.player.frames_per_direction.max(1);
            self.record_party_step(old_position, self.player.direction);
        }
        self.follow_player();
    }

    pub(super) fn follow_player(&mut self) {
        if self.viewport_locked {
            self.camera.x = self.player.world_x - self.party_screen_position.0;
            self.camera.y = self.player.world_y - self.party_screen_position.1;
            return;
        }
        self.camera.follow_at(
            (self.player.world_x, self.player.world_y),
            DEFAULT_PARTY_SCREEN_POSITION,
            self.map.world_size(),
        );
        self.party_screen_position = (
            self.player.world_x - self.camera.x,
            self.player.world_y - self.camera.y,
        );
    }
}
