//! Per-object automatic script scheduling and execution.

use super::*;
use crate::script::executor::{decode_instruction, selected_object, DecodeError};
use crate::script::{ScriptAction, ScriptOpcode};

impl<M: CollisionMap> GameState<M> {
    /// Run one auto-script instruction for each active event object.
    pub fn update_auto_scripts(&mut self, scripts: &ScriptTable) -> Result<bool, AutoScriptError> {
        self.update_auto_scripts_report(scripts).into_result()
    }

    /// Run automatic scripts while retaining visible changes made before an error.
    pub fn update_auto_scripts_report(&mut self, scripts: &ScriptTable) -> AutoScriptUpdate {
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
                    self.run_immediate_auto_subscript(
                        scripts,
                        entry.operands[0],
                        called_object_id,
                        0,
                    )?;
                    self.scene_objects[object_index].auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                JumpByChance => {
                    let roll = ((u32::from(object_id)
                        .wrapping_mul(1_103_515_245)
                        .wrapping_add(u32::from(script_entry))
                        .wrapping_add(self.script_frame.wrapping_mul(12_345))
                        >> 16)
                        % 100
                        + 1) as u16;
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
                SetObjectTriggerScript => {
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
                SetObjectTriggerMode => {
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
                SetObjectState => {
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
                JumpIfObjectOutsideZone => {
                    let within_zone =
                        self.objects_within_zone(object_id, entry.operands[0], entry.operands[1]);
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
                PrintMessage => {
                    self.scene_objects[object_index].auto_script = script_entry.wrapping_add(1);
                    return Ok(true);
                }
                // Known instructions that the auto-script scheduler does not implement yet.
                Redraw
                | StartBattle
                | AdvanceEntry
                | Confirm
                | SetObjectPositionRelative
                | SetObjectPosition
                | SetPartyMemberPose
                | SetEquipmentEffect
                | EquipItem
                | AdjustPlayerAttribute
                | SetPlayerAttribute
                | AdjustPlayerHp
                | AdjustPlayerMp
                | AdjustPlayerHpMp
                | AdjustCash
                | AddItem
                | RemoveItem
                | DamageEnemy
                | RevivePlayer
                | RemoveEquipment
                | SetObjectAutoScript
                | OpenBuyMenu
                | OpenSellMenu
                | PoisonEnemy
                | PoisonPlayer
                | CureEnemyPoison
                | CurePlayerPoison
                | CurePoisonByLevel
                | SetPlayerStatus
                | SetEnemyStatus
                | RemovePlayerStatus
                | AdjustTemporaryPlayerStat
                | SetTemporaryBattleSprite
                | CollectEnemy
                | TransmuteCollectedEnemies
                | ShakeScreen
                | SelectRngAnimation
                | PlayRngAnimation
                | TeleportParty
                | DrainEnemyHp
                | FleeBattle
                | DialogCenter
                | DialogUpper
                | DialogLower
                | DialogCenterWindow
                | RideObjectSlow
                | MarkScriptFailed
                | SimulatePlayerMagic
                | PlayMusic
                | RideObject
                | SetBattleMusic
                | SetPartyPosition
                | SetBattlefield
                | WaitForKey
                | LoadLastSave
                | FadeToRed
                | FadeOut
                | FadeIn
                | HideObject
                | UseDayPalette
                | UseNightPalette
                | AddMagic
                | RemoveMagic
                | ScaleMagicByMp
                | JumpIfItemCountLess
                | ChangeScene
                | HalvePlayerHp
                | HalveEnemyHp
                | HideBattleActor
                | JumpIfPlayerLacksPoison
                | JumpIfEnemyLacksPoison
                | KillPlayer
                | KillEnemy
                | JumpIfPlayerNotPoisoned
                | PauseEnemyChase
                | SpeedUpEnemyChase
                | JumpIfEnemyHpAbove
                | SetPlayerSprite
                | ThrowWeapon
                | EnemyCastMagic
                | JumpIfEnemyTurn
                | EnemyEscape
                | StealEnemy
                | BlowEnemiesAway
                | SetSceneScripts
                | OffsetParty
                | WalkParty
                | SetScreenWave
                | FadeScene
                | JumpIfPartyNotFullHp
                | SetParty
                | ShowFbp
                | StopMusic
                | NoOp
                | JumpIfPartyContainsPlayer
                | WalkPartyFast
                | WalkPartyFastest
                | MoveViewport
                | ToggleDayNightPalette
                | JumpIfNotFacingObject
                | PlaceUsedItemObject
                | Delay
                | JumpIfItemNotEquipped
                | ScaleMagicByCash
                | SetBattleResult
                | EnableAutoBattle
                | SetPalette
                | FadeColor
                | LevelUpPlayer
                | RestoreScreen
                | HalveCash
                | SetObjectScript
                | JumpIfEnemyNotFirstKind
                | PlayerMagicAnimation
                | FadeSceneWithUpdate
                | JumpIfObjectStateEquals
                | JumpIfSceneEquals
                | PlayEndingAnimation
                | RideObjectFast
                | SetPartyFollower
                | SetSceneMap
                | SetObjectStates
                | FadeToCurrentScene
                | DivideEnemy
                | SummonEnemy
                | TransformEnemy
                | QuitGame
                | CollapseParty
                | RandomSelect
                | PlayCdMusic
                | ScrollFbp
                | ShowFbpWithSprite
                | BackupScreen => {
                    return Err(AutoScriptError::Unsupported {
                        object_id,
                        entry: script_entry,
                        opcode: opcode.raw(),
                    })
                }
            }
        }
        let object = &self.scene_objects[object_index];
        Err(AutoScriptError::InstructionLimit {
            object_id: object.id,
            entry: object.auto_script,
        })
    }

    fn run_immediate_auto_subscript(
        &mut self,
        scripts: &ScriptTable,
        mut script_entry: u16,
        object_id: u16,
        depth: usize,
    ) -> Result<(), AutoScriptError> {
        const MAX_CALL_DEPTH: usize = 32;
        const MAX_INSTRUCTIONS: usize = 1024;

        if depth >= MAX_CALL_DEPTH {
            return Err(AutoScriptError::InstructionLimit {
                object_id,
                entry: script_entry,
            });
        }
        for _ in 0..MAX_INSTRUCTIONS {
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
                Stop | StopAndAdvance | StopAndReplace => return Ok(()),
                Jump => {
                    script_entry = entry.operands[0];
                    continue;
                }
                Call => {
                    let called_object_id = if entry.operands[1] == 0 {
                        object_id
                    } else {
                        entry.operands[1]
                    };
                    self.run_immediate_auto_subscript(
                        scripts,
                        entry.operands[0],
                        called_object_id,
                        depth + 1,
                    )?;
                }
                AutoScriptNoOp => {}
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
                    self.apply_auto_world_action(
                        object_id,
                        script_entry,
                        object_id,
                        ScriptAction::SetObjectPose {
                            object_id,
                            direction,
                            frame: (entry.operands[1] != u16::MAX).then_some(entry.operands[1]),
                        },
                    )?;
                }
                SetObjectGesture => {
                    self.apply_auto_world_action(
                        object_id,
                        script_entry,
                        object_id,
                        ScriptAction::SetObjectPose {
                            object_id,
                            direction: Some(Direction::South),
                            frame: Some(entry.operands[0]),
                        },
                    )?;
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
                }
                PlaySound => self.pending_auto_sounds.push(entry.operands[0]),
                SetObjectState => {
                    let target_id = selected_object(entry.operands[0], object_id);
                    self.apply_auto_world_action(
                        object_id,
                        script_entry,
                        target_id,
                        ScriptAction::SetObjectState {
                            object_id: target_id,
                            state: entry.operands[1] as i16,
                        },
                    )?;
                }
                HideObjectShort => {
                    self.apply_auto_world_action(
                        object_id,
                        script_entry,
                        object_id,
                        ScriptAction::SetObjectVanishTime {
                            object_id,
                            vanish_time: -15,
                        },
                    )?;
                }
                OffsetObjectAndAnimate => {
                    let target_id = selected_object(entry.operands[0], object_id);
                    self.apply_auto_world_action(
                        object_id,
                        script_entry,
                        target_id,
                        ScriptAction::OffsetObject {
                            object_id: target_id,
                            dx: i32::from(entry.operands[1] as i16),
                            dy: i32::from(entry.operands[2] as i16),
                        },
                    )?;
                }
                AnimateObject => {
                    self.apply_auto_world_action(
                        object_id,
                        script_entry,
                        object_id,
                        ScriptAction::AnimateObject { object_id },
                    )?;
                }
                // Known instructions that immediate auto-subscripts do not implement yet.
                Redraw
                | JumpByChance
                | StartBattle
                | AdvanceEntry
                | WaitFrames
                | Confirm
                | WalkObjectSouth
                | WalkObjectWest
                | WalkObjectNorth
                | WalkObjectEast
                | WalkObjectTo
                | WalkObjectToSlow
                | SetObjectPositionRelative
                | SetObjectPosition
                | SetPartyMemberPose
                | SetEquipmentEffect
                | EquipItem
                | AdjustPlayerAttribute
                | SetPlayerAttribute
                | AdjustPlayerHp
                | AdjustPlayerMp
                | AdjustPlayerHpMp
                | AdjustCash
                | AddItem
                | RemoveItem
                | DamageEnemy
                | RevivePlayer
                | RemoveEquipment
                | SetObjectAutoScript
                | SetObjectTriggerScript
                | OpenBuyMenu
                | OpenSellMenu
                | PoisonEnemy
                | PoisonPlayer
                | CureEnemyPoison
                | CurePlayerPoison
                | CurePoisonByLevel
                | SetPlayerStatus
                | SetEnemyStatus
                | RemovePlayerStatus
                | AdjustTemporaryPlayerStat
                | SetTemporaryBattleSprite
                | CollectEnemy
                | TransmuteCollectedEnemies
                | ShakeScreen
                | SelectRngAnimation
                | PlayRngAnimation
                | TeleportParty
                | DrainEnemyHp
                | FleeBattle
                | DialogCenter
                | DialogUpper
                | DialogLower
                | DialogCenterWindow
                | RideObjectSlow
                | SetObjectTriggerMode
                | MarkScriptFailed
                | SimulatePlayerMagic
                | PlayMusic
                | RideObject
                | SetBattleMusic
                | SetPartyPosition
                | SetBattlefield
                | ChasePlayer
                | WaitForKey
                | LoadLastSave
                | FadeToRed
                | FadeOut
                | FadeIn
                | HideObject
                | UseDayPalette
                | UseNightPalette
                | AddMagic
                | RemoveMagic
                | ScaleMagicByMp
                | JumpIfItemCountLess
                | ChangeScene
                | HalvePlayerHp
                | HalveEnemyHp
                | HideBattleActor
                | JumpIfPlayerLacksPoison
                | JumpIfEnemyLacksPoison
                | KillPlayer
                | KillEnemy
                | JumpIfPlayerNotPoisoned
                | PauseEnemyChase
                | SpeedUpEnemyChase
                | JumpIfEnemyHpAbove
                | SetPlayerSprite
                | ThrowWeapon
                | EnemyCastMagic
                | JumpIfEnemyTurn
                | EnemyEscape
                | StealEnemy
                | BlowEnemiesAway
                | SetSceneScripts
                | OffsetParty
                | SyncObjectState
                | WalkParty
                | SetScreenWave
                | FadeScene
                | JumpIfPartyNotFullHp
                | SetParty
                | ShowFbp
                | StopMusic
                | NoOp
                | JumpIfPartyContainsPlayer
                | WalkPartyFast
                | WalkPartyFastest
                | WalkObjectHalfSpeed
                | OffsetObject
                | SetObjectLayer
                | MoveViewport
                | ToggleDayNightPalette
                | JumpIfNotFacingObject
                | WalkObjectFast
                | JumpIfObjectOutsideZone
                | PlaceUsedItemObject
                | Delay
                | JumpIfItemNotEquipped
                | ScaleMagicByCash
                | SetBattleResult
                | EnableAutoBattle
                | SetPalette
                | FadeColor
                | LevelUpPlayer
                | RestoreScreen
                | HalveCash
                | SetObjectScript
                | JumpIfEnemyNotFirstKind
                | PlayerMagicAnimation
                | FadeSceneWithUpdate
                | JumpIfObjectStateEquals
                | JumpIfSceneEquals
                | PlayEndingAnimation
                | RideObjectFast
                | SetPartyFollower
                | SetSceneMap
                | SetObjectStates
                | FadeToCurrentScene
                | DivideEnemy
                | SummonEnemy
                | TransformEnemy
                | QuitGame
                | CollapseParty
                | RandomSelect
                | PlayCdMusic
                | ScrollFbp
                | ShowFbpWithSprite
                | BackupScreen
                | PrintMessage => {
                    return Err(AutoScriptError::Unsupported {
                        object_id,
                        entry: script_entry,
                        opcode: opcode.raw(),
                    })
                }
            }
            script_entry = script_entry.wrapping_add(1);
        }
        Err(AutoScriptError::InstructionLimit {
            object_id,
            entry: script_entry,
        })
    }

    fn chase_player(&mut self, object_index: usize, speed: u16, range: u16, floating: bool) {
        let player = (self.player.world_x, self.player.world_y);
        let chase_range = self.chase_range;
        let script_frame = self.script_frame;
        let object = &mut self.scene_objects[object_index];
        if chase_range == 0 {
            if !script_frame.is_multiple_of(2) {
                object.direction = Direction::from_pal((object.direction as u16 + 1) % 4)
                    .unwrap_or(object.direction);
            }
            object.advance_animation();
            return;
        }
        let x_offset = player.0 - object.world_x;
        let y_offset = player.1 - object.world_y;
        if i64::from(x_offset.abs()) + i64::from(y_offset.abs()) * 2
            < i64::from(range) * 32 * i64::from(chase_range)
        {
            object.direction = direction_toward(x_offset, y_offset);
            let (dx, dy) = object.direction.step_at_speed(i32::from(speed));
            let target = (object.world_x + dx, object.world_y + dy);
            if floating || !self.map.is_world_blocked(target.0, target.1) {
                object.world_x = target.0;
                object.world_y = target.1;
            }
        }
        object.advance_animation();
    }
}
