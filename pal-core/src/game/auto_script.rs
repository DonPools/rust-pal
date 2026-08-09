//! Per-object automatic script scheduling and execution.

use super::*;
use crate::script::executor::{decode_instruction, selected_object, DecodeError};
use crate::script::{
    ScriptAction, ScriptCondition, ScriptEvent, ScriptInstructionStep, ScriptOpcode, ScriptRuntime,
};

impl<M: CollisionMap> GameState<M> {
    /// Run one auto-script instruction for each active event object.
    pub fn update_auto_scripts(&mut self, scripts: &ScriptTable) -> Result<bool, AutoScriptError> {
        let update = self.update_auto_scripts_report(scripts);
        let mut changed = update.into_result()?;
        if self
            .pending_trigger
            .is_some_and(|trigger| trigger.kind == TriggerKind::Auto)
        {
            changed |= self.run_pending_auto_trigger_headless(scripts)?;
        }
        Ok(changed)
    }

    /// Run automatic scripts while retaining visible changes made before an error.
    pub fn update_auto_scripts_report(&mut self, scripts: &ScriptTable) -> AutoScriptUpdate {
        if self.pending_trigger.is_some() || !self.pending_auto_events.is_empty() {
            return AutoScriptUpdate {
                changed: false,
                error: None,
            };
        }
        self.script_frame = self.script_frame.wrapping_add(1);
        let mut changed = false;
        let mut first_error = None;
        for index in 0..self.scene_objects.len() {
            if self.scene_objects[index].state <= 0
                || self.scene_objects[index].vanish_time != 0
                || self.scene_objects[index].auto_script == 0
            {
                continue;
            }
            match self.advance_auto_script(index, scripts) {
                Ok(object_changed) => changed |= object_changed,
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            };
            if self.pending_trigger.is_some() || !self.pending_auto_events.is_empty() {
                break;
            }
        }
        if self.chase_speed_change_cycles > 0 {
            self.chase_speed_change_cycles -= 1;
            if self.chase_speed_change_cycles == 0 {
                self.chase_range = 1;
            }
        }
        AutoScriptUpdate {
            changed,
            error: first_error,
        }
    }

    fn apply_auto_world_action(
        &mut self,
        object_id: u16,
        script_entry: u16,
        target_id: u16,
        action: ScriptAction,
    ) -> Result<(), AutoScriptError> {
        self.apply_script_action(action)
            .then_some(())
            .ok_or(AutoScriptError::MissingObject {
                object_id,
                entry: script_entry,
                target_id,
            })
    }

    fn advance_with_auto_world_action(
        &mut self,
        object_index: usize,
        script_entry: u16,
        target_id: u16,
        action: ScriptAction,
    ) -> Result<bool, AutoScriptError> {
        let object_id = self.scene_objects[object_index].id;
        self.apply_auto_world_action(object_id, script_entry, target_id, action)?;
        self.scene_objects[object_index].auto_script = script_entry.wrapping_add(1);
        Ok(true)
    }

    fn dispatch_general_auto_instruction(
        &mut self,
        scripts: &ScriptTable,
        object_index: usize,
        script_entry: u16,
        entry: pal_assets::script::ScriptEntry,
        opcode: ScriptOpcode,
    ) -> Result<bool, AutoScriptError> {
        let object_id = self.scene_objects[object_index].id;
        let mut runtime = self
            .auto_instruction_runtime
            .take()
            .unwrap_or_else(|| ScriptRuntime::new(scripts.clone()));
        runtime.set_random_state(self.random_state());
        let step = runtime.dispatch_single_instruction(
            TriggerRequest {
                object_id,
                script_entry,
                kind: TriggerKind::Auto,
            },
            entry,
            opcode,
        );
        self.set_random_state(runtime.random_state());
        self.auto_instruction_runtime = Some(runtime);
        let next_entry = self.apply_auto_instruction_step(object_id, script_entry, opcode, step)?;
        self.scene_objects[object_index].auto_script = next_entry;
        Ok(true)
    }

    fn apply_auto_instruction_step(
        &mut self,
        object_id: u16,
        script_entry: u16,
        opcode: ScriptOpcode,
        step: ScriptInstructionStep,
    ) -> Result<u16, AutoScriptError> {
        let mut next_entry = step.next_entry;
        self.pending_auto_script_failure |= !step.succeeded;
        let Some(event) = step.event else {
            return Ok(next_entry);
        };
        match event {
            ScriptEvent::Action(action) => {
                if let Some(target) = self.apply_dispatched_auto_action(action) {
                    next_entry = target;
                }
            }
            ScriptEvent::Condition(condition) => {
                if let Some(target) = self.auto_condition_target(condition) {
                    next_entry = target;
                }
            }
            ScriptEvent::Teleport { failure_entry } => {
                let teleport_entry = self.scene_teleport_script(0);
                if teleport_entry == 0 {
                    self.pending_auto_script_failure = true;
                    next_entry = failure_entry;
                } else {
                    self.pending_trigger = Some(TriggerRequest {
                        object_id: u16::MAX,
                        script_entry: teleport_entry,
                        kind: TriggerKind::Auto,
                    });
                }
            }
            ScriptEvent::FadeScene { speed } => {
                self.pending_auto_events.push(ScriptEvent::Visual(
                    crate::script::ScriptVisual::FadeToCurrentScene { speed },
                ));
            }
            event @ (ScriptEvent::Visual(_)
            | ScriptEvent::OpenBuyMenu { .. }
            | ScriptEvent::OpenSellMenu
            | ScriptEvent::WaitForKey
            | ScriptEvent::LoadLastSave
            | ScriptEvent::QuitGame) => self.pending_auto_events.push(event),
            ScriptEvent::Waiting | ScriptEvent::Delay => {}
            ScriptEvent::Unsupported { entry, opcode, .. } => {
                return Err(AutoScriptError::Unsupported {
                    object_id,
                    entry,
                    opcode,
                });
            }
            ScriptEvent::InvalidEntry { entry, .. } => {
                return Err(AutoScriptError::InvalidEntry { object_id, entry });
            }
            ScriptEvent::InstructionLimit { entry, .. } => {
                return Err(AutoScriptError::InstructionLimit { object_id, entry });
            }
            ScriptEvent::Message { .. }
            | ScriptEvent::Confirm { .. }
            | ScriptEvent::StartBattle(_)
            | ScriptEvent::Completed { .. } => {
                return Err(AutoScriptError::Unsupported {
                    object_id,
                    entry: script_entry,
                    opcode: opcode.raw(),
                });
            }
        }
        Ok(next_entry)
    }

    fn run_pending_auto_trigger_headless(
        &mut self,
        scripts: &ScriptTable,
    ) -> Result<bool, AutoScriptError> {
        const MAX_HEADLESS_EVENTS: usize = 4096;

        let Some(trigger) = self.pending_trigger.take() else {
            return Ok(false);
        };
        if trigger.kind != TriggerKind::Auto {
            self.pending_trigger = Some(trigger);
            return Ok(false);
        }
        let mut runtime = ScriptRuntime::new(scripts.clone());
        if !runtime.start(trigger) {
            return Err(AutoScriptError::InvalidEntry {
                object_id: trigger.object_id,
                entry: trigger.script_entry,
            });
        }
        for _ in 0..MAX_HEADLESS_EVENTS {
            runtime.set_random_state(self.random_state());
            let event = runtime.advance();
            self.set_random_state(runtime.random_state());
            match event {
                Some(ScriptEvent::Action(action)) => {
                    if let Some(target) = self.apply_dispatched_auto_action(action) {
                        let _ = runtime.branch_to(target);
                    }
                }
                Some(ScriptEvent::Condition(condition)) => {
                    if let Some(target) = self.auto_condition_target(condition) {
                        let _ = runtime.branch_to(target);
                    }
                }
                Some(ScriptEvent::Teleport { failure_entry }) => {
                    let entry = self.scene_teleport_script(0);
                    if entry == 0 || !runtime.call(entry, u16::MAX) {
                        let _ = runtime.set_success(false);
                        let _ = runtime.branch_to(failure_entry);
                    }
                }
                Some(
                    ScriptEvent::Message { .. }
                    | ScriptEvent::Waiting
                    | ScriptEvent::Delay
                    | ScriptEvent::OpenBuyMenu { .. }
                    | ScriptEvent::OpenSellMenu
                    | ScriptEvent::FadeScene { .. }
                    | ScriptEvent::Visual(_)
                    | ScriptEvent::WaitForKey
                    | ScriptEvent::Confirm { .. },
                ) => {}
                Some(ScriptEvent::Completed { succeeded, .. }) => {
                    self.pending_auto_script_failure |= !succeeded;
                    self.pending_auto_events.clear();
                    return Ok(true);
                }
                Some(ScriptEvent::LoadLastSave) | Some(ScriptEvent::QuitGame) => {
                    self.pending_auto_events.clear();
                    return Ok(true);
                }
                Some(ScriptEvent::Unsupported { entry, opcode, .. }) => {
                    return Err(AutoScriptError::Unsupported {
                        object_id: trigger.object_id,
                        entry,
                        opcode,
                    });
                }
                Some(ScriptEvent::InvalidEntry { entry, .. }) => {
                    return Err(AutoScriptError::InvalidEntry {
                        object_id: trigger.object_id,
                        entry,
                    });
                }
                Some(ScriptEvent::InstructionLimit { entry, .. }) => {
                    return Err(AutoScriptError::InstructionLimit {
                        object_id: trigger.object_id,
                        entry,
                    });
                }
                Some(ScriptEvent::StartBattle(_)) => {
                    let entry = runtime
                        .debug_snapshot()
                        .last_instruction
                        .map_or(trigger.script_entry, |instruction| instruction.entry);
                    return Err(AutoScriptError::HostRequired {
                        object_id: trigger.object_id,
                        entry,
                        opcode: ScriptOpcode::StartBattle.raw(),
                    });
                }
                None => {
                    return Err(AutoScriptError::InvalidEntry {
                        object_id: trigger.object_id,
                        entry: trigger.script_entry,
                    });
                }
            }
        }
        Err(AutoScriptError::InstructionLimit {
            object_id: trigger.object_id,
            entry: trigger.script_entry,
        })
    }

    fn apply_dispatched_auto_action(&mut self, action: ScriptAction) -> Option<u16> {
        match action {
            ScriptAction::AdjustCash {
                amount,
                insufficient_entry,
            } => {
                if !self.adjust_cash(amount) {
                    return Some(insufficient_entry);
                }
            }
            ScriptAction::RemoveItem {
                item_id,
                amount,
                insufficient_entry,
            } => {
                if !self.remove_item(item_id, amount, insufficient_entry) {
                    return Some(insufficient_entry);
                }
            }
            action @ ScriptAction::SetEnemyStatus { resisted_entry, .. } => {
                if !self.apply_script_action(action) {
                    return Some(resisted_entry);
                }
            }
            action @ ScriptAction::FleeBattle { failure_entry } => {
                if !self.apply_script_action(action) {
                    return Some(failure_entry);
                }
            }
            action @ ScriptAction::DivideEnemy { failure_entry, .. }
            | action @ ScriptAction::CollectEnemy { failure_entry, .. }
            | action @ ScriptAction::SummonEnemy { failure_entry, .. } => {
                if !self.apply_script_action(action) && failure_entry != 0 {
                    return Some(failure_entry);
                }
            }
            action @ ScriptAction::PlaceObjectInFront { blocked_entry, .. } => {
                if !self.apply_script_action(action) {
                    self.pending_auto_script_failure = true;
                    return Some(blocked_entry);
                }
            }
            action @ (ScriptAction::AdjustPlayerHealth { .. }
            | ScriptAction::RevivePlayer { .. }
            | ScriptAction::SetPlayerStatus { .. }) => {
                if !self.apply_script_action(action) {
                    self.pending_auto_script_failure = true;
                }
            }
            ScriptAction::WalkObjectTo {
                object_id,
                tile_x,
                tile_y,
                half,
                speed,
                repeat_entry,
            } => {
                if self
                    .walk_object_to(object_id, tile_x, tile_y, half, speed)
                    .is_some_and(|completed| !completed)
                {
                    return Some(repeat_entry);
                }
            }
            ScriptAction::WalkPlayerTo {
                tile_x,
                tile_y,
                half,
                speed,
                repeat_entry,
            } => {
                if self
                    .walk_player_to(tile_x, tile_y, half, speed)
                    .is_some_and(|completed| !completed)
                {
                    return Some(repeat_entry);
                }
            }
            ScriptAction::RideObjectTo {
                object_id,
                tile_x,
                tile_y,
                half,
                speed,
                repeat_entry,
            } => {
                if self
                    .ride_object_to(object_id, tile_x, tile_y, half, speed)
                    .is_some_and(|completed| !completed)
                {
                    return Some(repeat_entry);
                }
            }
            action @ ScriptAction::PlayMusic { .. } => {
                let _ = self.apply_script_action(action);
                self.pending_auto_events.push(ScriptEvent::Action(action));
            }
            ScriptAction::PlaySound { sound_id } => self.pending_auto_sounds.push(sound_id),
            action @ ScriptAction::ChangeScene { .. } => {
                self.pending_auto_events.push(ScriptEvent::Action(action));
            }
            action @ ScriptAction::SetSceneMap { .. } => {
                let _ = self.apply_script_action(action);
                self.pending_auto_events.push(ScriptEvent::Action(action));
            }
            action => {
                let _ = self.apply_script_action(action);
            }
        }
        None
    }

    fn auto_condition_target(&mut self, condition: ScriptCondition) -> Option<u16> {
        let (matches, target_entry) = match condition {
            ScriptCondition::ItemCountLess {
                item_id,
                amount,
                target_entry,
            } => (
                i32::from(self.item_count(item_id)) < i32::from(amount),
                target_entry,
            ),
            ScriptCondition::ObjectStateEquals {
                object_id,
                state,
                target_entry,
            } => (self.object_state(object_id) == Some(state), target_entry),
            ScriptCondition::SceneEquals {
                scene_number,
                target_entry,
            } => (self.scene_number == scene_number, target_entry),
            ScriptCondition::PartyContainsName {
                name_word_id,
                target_entry,
            } => (self.party_contains_name(name_word_id), target_entry),
            ScriptCondition::PlayerFacesObject {
                object_id,
                range,
                target_entry,
            } => (!self.player_faces_object(object_id, range), target_entry),
            ScriptCondition::PartyNotFullHp { target_entry } => {
                (self.party_not_full_hp(), target_entry)
            }
            ScriptCondition::ItemNotEquipped {
                item_id,
                amount,
                target_entry,
            } => (self.equipped_item_count(item_id) < amount, target_entry),
            ScriptCondition::PlayerLacksPoison {
                role_id,
                poison_id,
                target_entry,
            } => (!self.player_has_poison(role_id, poison_id), target_entry),
            ScriptCondition::EnemyLacksPoison {
                enemy_index,
                poison_id,
                target_entry,
            } => (!self.enemy_has_poison(enemy_index, poison_id), target_entry),
            ScriptCondition::PlayerNotPoisoned {
                role_id,
                target_entry,
            } => (
                self.player_poisons(role_id)
                    .is_none_or(|poisons| poisons.iter().all(|poison| poison.object_id == 0)),
                target_entry,
            ),
            ScriptCondition::EnemyHpAbove {
                enemy_index,
                percentage,
                target_entry,
            } => (self.enemy_hp_above(enemy_index, percentage), target_entry),
            ScriptCondition::EnemyNotFirstKind {
                enemy_index,
                target_entry,
            } => (self.enemy_not_first_kind(enemy_index), target_entry),
            ScriptCondition::EnemyTurn { target_entry } => (self.is_enemy_turn(), target_entry),
        };
        matches.then_some(target_entry)
    }

    fn advance_auto_script(
        &mut self,
        object_index: usize,
        scripts: &ScriptTable,
    ) -> Result<bool, AutoScriptError> {
        const MAX_AUTO_JUMPS: usize = 1024;

        for _ in 0..MAX_AUTO_JUMPS {
            let object_id = self.scene_objects[object_index].id;
            let script_entry = self.scene_objects[object_index].auto_script;
            let decoded = match decode_instruction(scripts, script_entry) {
                Ok(decoded) => decoded,
                Err(DecodeError::InvalidEntry { entry }) => {
                    return Err(AutoScriptError::InvalidEntry { object_id, entry });
                }
                Err(DecodeError::Unsupported { entry, opcode }) => {
                    return Err(AutoScriptError::Unsupported {
                        object_id,
                        entry,
                        opcode,
                    });
                }
            };
            let entry = decoded.instruction;
            let opcode = decoded.opcode;
            use ScriptOpcode::*;
            match opcode {
                Stop => return Ok(false),
                StopAndAdvance => {
                    self.scene_objects[object_index].auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                StopAndReplace => {
                    let object = &mut self.scene_objects[object_index];
                    if entry.operands[1] == 0 {
                        object.auto_script = entry.operands[0];
                    } else if object.auto_script_idle_frame.wrapping_add(1) < entry.operands[1] {
                        object.auto_script_idle_frame =
                            object.auto_script_idle_frame.wrapping_add(1);
                        object.auto_script = entry.operands[0];
                    } else {
                        object.auto_script_idle_frame = 0;
                        object.auto_script = script_entry.wrapping_add(1);
                    }
                    return Ok(true);
                }
                Jump => {
                    let object = &mut self.scene_objects[object_index];
                    if entry.operands[1] == 0 {
                        object.auto_script = entry.operands[0];
                        continue;
                    } else if object.auto_script_idle_frame.wrapping_add(1) < entry.operands[1] {
                        object.auto_script_idle_frame =
                            object.auto_script_idle_frame.wrapping_add(1);
                        object.auto_script = entry.operands[0];
                        continue;
                    }
                    object.auto_script_idle_frame = 0;
                    object.auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                Call => {
                    let called_object_id = if entry.operands[1] == 0 {
                        object_id
                    } else {
                        entry.operands[1]
                    };
                    self.pending_trigger = Some(TriggerRequest {
                        object_id: called_object_id,
                        script_entry: entry.operands[0],
                        kind: TriggerKind::Auto,
                    });
                    self.scene_objects[object_index].auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                JumpByChance => {
                    let mut random_state = self.random_state();
                    let roll = random::random_long(&mut random_state, 1, 100) as u16;
                    self.set_random_state(random_state);
                    let object = &mut self.scene_objects[object_index];
                    if roll >= entry.operands[0] {
                        if entry.operands[1] != 0 {
                            object.auto_script = entry.operands[1];
                            continue;
                        }
                    } else {
                        object.auto_script = script_entry.wrapping_add(1);
                    }
                    return Ok(true);
                }
                WaitFrames => {
                    let object = &mut self.scene_objects[object_index];
                    object.auto_script_idle_frame = object.auto_script_idle_frame.wrapping_add(1);
                    if object.auto_script_idle_frame >= entry.operands[0] {
                        object.auto_script_idle_frame = 0;
                        object.auto_script = script_entry.wrapping_add(1);
                    }
                    return Ok(true);
                }
                SetObjectPose => {
                    let direction = if entry.operands[0] == u16::MAX {
                        None
                    } else {
                        Some(Direction::from_pal(entry.operands[0]).ok_or(
                            AutoScriptError::Unsupported {
                                object_id,
                                entry: script_entry,
                                opcode: entry.opcode,
                            },
                        )?)
                    };
                    return self.advance_with_auto_world_action(
                        object_index,
                        script_entry,
                        object_id,
                        ScriptAction::SetObjectPose {
                            object_id,
                            direction,
                            frame: (entry.operands[1] != u16::MAX).then_some(entry.operands[1]),
                        },
                    );
                }
                WalkObjectSouth | WalkObjectWest | WalkObjectNorth | WalkObjectEast => {
                    let direction = Direction::from_pal(opcode.raw() - WalkObjectSouth.raw())
                        .expect("valid walk opcode");
                    return self.advance_with_auto_world_action(
                        object_index,
                        script_entry,
                        object_id,
                        ScriptAction::MoveObject {
                            object_id,
                            direction,
                        },
                    );
                }
                WalkObjectTo | WalkObjectToSlow | WalkObjectHalfSpeed | WalkObjectFast => {
                    let target = tile_to_world(
                        usize::from(entry.operands[0]),
                        usize::from(entry.operands[1]),
                        usize::from(entry.operands[2]),
                    )
                    .ok_or(AutoScriptError::Unsupported {
                        object_id,
                        entry: script_entry,
                        opcode: entry.opcode,
                    })?;
                    let object = &mut self.scene_objects[object_index];
                    let should_move = opcode != WalkObjectToSlow
                        || !self
                            .script_frame
                            .wrapping_add(u32::from(object_id))
                            .is_multiple_of(2);
                    let speed = match opcode {
                        WalkObjectTo => 3,
                        WalkObjectToSlow | WalkObjectHalfSpeed => 2,
                        WalkObjectFast => 8,
                        _ => unreachable!("matched object-walk opcode"),
                    };
                    if should_move && walk_scene_object_to(object, target, speed) {
                        object.auto_script = script_entry.wrapping_add(1);
                    }
                    return Ok(true);
                }
                SetObjectGesture => {
                    return self.advance_with_auto_world_action(
                        object_index,
                        script_entry,
                        object_id,
                        ScriptAction::SetObjectPose {
                            object_id,
                            direction: Some(Direction::South),
                            frame: Some(entry.operands[0]),
                        },
                    );
                }
                SetPartyMemberPose => {
                    let direction = Direction::from_pal(entry.operands[0]).ok_or(
                        AutoScriptError::Unsupported {
                            object_id,
                            entry: script_entry,
                            opcode: entry.opcode,
                        },
                    )?;
                    return self.advance_with_auto_world_action(
                        object_index,
                        script_entry,
                        object_id,
                        ScriptAction::SetPlayerPose {
                            direction,
                            frame: u8::try_from(entry.operands[1]).unwrap_or(u8::MAX),
                            party_index: entry.operands[2],
                        },
                    );
                }
                SetSelectedObjectPose => {
                    if entry.operands[0] != 0 {
                        let target_id = selected_object(entry.operands[0], object_id);
                        let direction = Direction::from_pal(entry.operands[1]).ok_or(
                            AutoScriptError::Unsupported {
                                object_id,
                                entry: script_entry,
                                opcode: entry.opcode,
                            },
                        )?;
                        self.apply_auto_world_action(
                            object_id,
                            script_entry,
                            target_id,
                            ScriptAction::SetObjectPose {
                                object_id: target_id,
                                direction: Some(direction),
                                frame: Some(entry.operands[2]),
                            },
                        )?;
                    }
                    self.scene_objects[object_index].auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                SetObjectAutoScript if entry.operands[0] != 0 => {
                    let target_id = selected_object(entry.operands[0], object_id);
                    return self.advance_with_auto_world_action(
                        object_index,
                        script_entry,
                        target_id,
                        ScriptAction::SetObjectAutoScript {
                            object_id: target_id,
                            script_entry: entry.operands[1],
                        },
                    );
                }
                SetObjectAutoScript => {
                    self.scene_objects[object_index].auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                SetObjectTriggerScript if entry.operands[0] != 0 => {
                    let target_id = selected_object(entry.operands[0], object_id);
                    return self.advance_with_auto_world_action(
                        object_index,
                        script_entry,
                        target_id,
                        ScriptAction::SetObjectTriggerScript {
                            object_id: target_id,
                            script_entry: entry.operands[1],
                        },
                    );
                }
                SetObjectTriggerScript => {
                    self.scene_objects[object_index].auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                SetObjectTriggerMode if entry.operands[0] != 0 => {
                    let target_id = selected_object(entry.operands[0], object_id);
                    return self.advance_with_auto_world_action(
                        object_index,
                        script_entry,
                        target_id,
                        ScriptAction::SetObjectTriggerMode {
                            object_id: target_id,
                            trigger_mode: entry.operands[1],
                        },
                    );
                }
                SetObjectTriggerMode => {
                    self.scene_objects[object_index].auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                SetObjectState if entry.operands[0] != 0 => {
                    let target_id = selected_object(entry.operands[0], object_id);
                    return self.advance_with_auto_world_action(
                        object_index,
                        script_entry,
                        target_id,
                        ScriptAction::SetObjectState {
                            object_id: target_id,
                            state: entry.operands[1] as i16,
                        },
                    );
                }
                SetObjectState => {
                    self.scene_objects[object_index].auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                PlaySound => {
                    self.pending_auto_sounds.push(entry.operands[0]);
                    self.scene_objects[object_index].auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                HideObjectShort => {
                    return self.advance_with_auto_world_action(
                        object_index,
                        script_entry,
                        object_id,
                        ScriptAction::SetObjectVanishTime {
                            object_id,
                            vanish_time: -15,
                        },
                    );
                }
                ChasePlayer => {
                    self.chase_player(
                        object_index,
                        if entry.operands[1] == 0 {
                            4
                        } else {
                            entry.operands[1]
                        },
                        if entry.operands[0] == 0 {
                            8
                        } else {
                            entry.operands[0]
                        },
                        entry.operands[2] != 0,
                    );
                    self.scene_objects[object_index].auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                OffsetObjectAndAnimate => {
                    let target_id = selected_object(entry.operands[0], object_id);
                    return self.advance_with_auto_world_action(
                        object_index,
                        script_entry,
                        target_id,
                        ScriptAction::OffsetObject {
                            object_id: target_id,
                            dx: i32::from(entry.operands[1] as i16),
                            dy: i32::from(entry.operands[2] as i16),
                        },
                    );
                }
                OffsetObject => {
                    let target_id = selected_object(entry.operands[0], object_id);
                    return self.advance_with_auto_world_action(
                        object_index,
                        script_entry,
                        target_id,
                        ScriptAction::MoveObjectBy {
                            object_id: target_id,
                            dx: i32::from(entry.operands[1] as i16),
                            dy: i32::from(entry.operands[2] as i16),
                        },
                    );
                }
                SetObjectLayer => {
                    let target_id = selected_object(entry.operands[0], object_id);
                    return self.advance_with_auto_world_action(
                        object_index,
                        script_entry,
                        target_id,
                        ScriptAction::SetObjectLayer {
                            object_id: target_id,
                            layer: entry.operands[1] as i16,
                        },
                    );
                }
                MoveViewport => {
                    let frames = entry.operands[2] as i16;
                    let immediate =
                        (entry.operands[0] == 0 && entry.operands[1] == 0) || frames == -1;
                    let _ = self.apply_script_action(ScriptAction::MoveViewport {
                        x: entry.operands[0] as i16,
                        y: entry.operands[1] as i16,
                        frames: if frames == -1 { -1 } else { 1 },
                    });
                    let object = &mut self.scene_objects[object_index];
                    if immediate {
                        object.auto_script_idle_frame = 0;
                        object.auto_script = script_entry.wrapping_add(1);
                    } else {
                        object.auto_script_idle_frame =
                            object.auto_script_idle_frame.wrapping_add(1);
                        if object.auto_script_idle_frame >= frames.max(1) as u16 {
                            object.auto_script_idle_frame = 0;
                            object.auto_script = script_entry.wrapping_add(1);
                        }
                    }
                    return Ok(true);
                }
                JumpIfObjectOutsideZone => {
                    let within_zone =
                        self.objects_within_zone(object_id, entry.operands[0], entry.operands[1]);
                    self.pending_auto_script_failure |= !within_zone;
                    self.scene_objects[object_index].auto_script = if within_zone {
                        script_entry.wrapping_add(1)
                    } else {
                        entry.operands[2]
                    };
                    return Ok(true);
                }
                SyncObjectState => {
                    let source_id = selected_object(entry.operands[0], object_id);
                    return self.advance_with_auto_world_action(
                        object_index,
                        script_entry,
                        source_id,
                        ScriptAction::SyncObjectState {
                            object_id,
                            source_object_id: source_id,
                            state: entry.operands[1] as i16,
                        },
                    );
                }
                AnimateObject => {
                    return self.advance_with_auto_world_action(
                        object_index,
                        script_entry,
                        object_id,
                        ScriptAction::AnimateObject { object_id },
                    );
                }
                AutoScriptNoOp => {
                    self.scene_objects[object_index].auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                Delay => {
                    let milliseconds = u64::from(entry.operands[0]) * 80;
                    let delay_frames = milliseconds
                        .div_ceil(EXPLORATION_FRAME_MS)
                        .max(1)
                        .min(u64::from(u16::MAX)) as u16;
                    let object = &mut self.scene_objects[object_index];
                    object.auto_script_idle_frame = object.auto_script_idle_frame.wrapping_add(1);
                    if object.auto_script_idle_frame >= delay_frames {
                        object.auto_script_idle_frame = 0;
                        object.auto_script = script_entry.wrapping_add(1);
                    }
                    return Ok(true);
                }
                PrintMessage => {
                    self.scene_objects[object_index].auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                // These instructions are handled only by PAL_RunTriggerScript in
                // the reference implementation. A direct autoscript dispatch
                // reaches PAL_InterpretInstruction's invalid-opcode branch.
                Redraw | StartBattle | AdvanceEntry | Confirm | DialogCenter | DialogUpper
                | DialogLower | DialogCenterWindow | RestoreScreen => {
                    return Err(AutoScriptError::Unsupported {
                        object_id,
                        entry: script_entry,
                        opcode: opcode.raw(),
                    })
                }
                _ => {
                    return self.dispatch_general_auto_instruction(
                        scripts,
                        object_index,
                        script_entry,
                        entry,
                        opcode,
                    )
                }
            }
        }
        let object = &self.scene_objects[object_index];
        Err(AutoScriptError::InstructionLimit {
            object_id: object.id,
            entry: object.auto_script,
        })
    }

    fn chase_player(&mut self, object_index: usize, speed: u16, range: u16, floating: bool) {
        let player = (self.player.world_x, self.player.world_y);
        let chase_range = self.chase_range;
        let script_frame = self.script_frame;
        let object_id = self.scene_objects[object_index].id;
        if chase_range == 0 {
            let object = &mut self.scene_objects[object_index];
            if !script_frame.is_multiple_of(2) {
                object.direction = Direction::from_pal((object.direction as u16 + 1) % 4)
                    .unwrap_or(object.direction);
            }
            object.advance_animation();
            return;
        }
        let original = (
            self.scene_objects[object_index].world_x,
            self.scene_objects[object_index].world_y,
        );
        let mut x_offset = player.0 - original.0;
        let mut y_offset = player.1 - original.1;
        if x_offset == 0 || y_offset == 0 {
            let mut random_state = self.random_state();
            if x_offset == 0 {
                x_offset = if random::random_long(&mut random_state, 0, 1) != 0 {
                    -1
                } else {
                    1
                };
            }
            if y_offset == 0 {
                y_offset = if random::random_long(&mut random_state, 0, 1) != 0 {
                    -1
                } else {
                    1
                };
            }
            self.set_random_state(random_state);
        }
        let snapped = snap_chasing_object_to_tile(original.0, original.1);
        let mut position = original;
        let mut direction = self.scene_objects[object_index].direction;
        let mut movement_speed = 0;
        if i64::from(x_offset.abs()) + i64::from(y_offset.abs()) * 2
            < i64::from(range) * 32 * i64::from(chase_range)
        {
            direction = direction_toward(x_offset, y_offset);
            let target = (
                original.0 + x_offset.signum() * 16,
                original.1 + y_offset.signum() * 8,
            );
            if floating || !self.chase_position_blocked(target, object_id, true) {
                movement_speed = speed;
            } else {
                position = snapped;
            }
            if !floating {
                for (dx, dy) in [(-4, 2), (-4, -2), (4, -2), (4, 2)] {
                    position.0 += dx;
                    position.1 += dy;
                    if self.chase_position_blocked(position, object_id, false) {
                        position = snapped;
                    }
                }
            }
        }
        let object = &mut self.scene_objects[object_index];
        object.world_x = position.0;
        object.world_y = position.1;
        object.direction = direction;
        let (dx, dy) = direction.step_at_speed(i32::from(movement_speed));
        object.world_x += dx;
        object.world_y += dy;
        object.advance_animation();
    }

    fn chase_position_blocked(
        &self,
        position: (i32, i32),
        object_id: u16,
        check_event_objects: bool,
    ) -> bool {
        self.map.is_world_blocked(position.0, position.1)
            || check_event_objects
                && self.scene_objects.iter().any(|object| {
                    object.id != object_id
                        && object.is_blocker()
                        && i64::from(object.world_x.abs_diff(position.0))
                            + i64::from(object.world_y.abs_diff(position.1)) * 2
                            < 16
                })
    }

    pub(super) fn chase_object(
        &mut self,
        object_id: u16,
        speed: u16,
        range: u16,
        floating: bool,
    ) -> bool {
        let Some(object_index) = self
            .scene_objects
            .iter()
            .position(|object| object.id == object_id)
        else {
            return false;
        };
        self.chase_player(object_index, speed, range, floating);
        true
    }
}

fn snap_chasing_object_to_tile(world_x: i32, world_y: i32) -> (i32, i32) {
    let mut tile_x = world_x.div_euclid(32);
    let mut tile_y = world_y.div_euclid(16);
    let x = world_x.rem_euclid(32);
    let y = world_y.rem_euclid(16);
    let mut half = 0;
    if x + y * 2 >= 16 {
        if x + y * 2 >= 48 {
            tile_x += 1;
            tile_y += 1;
        } else if 32 - x + y * 2 < 16 {
            tile_x += 1;
        } else if 32 - x + y * 2 < 48 {
            half = 1;
        } else {
            tile_y += 1;
        }
    }
    (tile_x * 32 + half * 16, tile_y * 16 + half * 8)
}
