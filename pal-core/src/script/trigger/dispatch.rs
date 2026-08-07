use super::{InstructionFlow, ScriptInstructionDebug, ScriptRuntime};
use crate::script::{ScriptEvent, ScriptOpcode};

const MAX_INSTRUCTIONS_PER_ADVANCE: usize = 1024;

impl ScriptRuntime {
    /// Execute until a message, completion, or unsupported instruction yields control.
    pub fn advance(&mut self) -> Option<ScriptEvent> {
        if self.pending_battle.is_some() {
            return None;
        }
        let mut execution = self.execution?;
        if execution.wait_frames > 0 {
            execution.wait_frames -= 1;
            self.execution = Some(execution);
            return Some(if execution.wait_updates_auto_scripts {
                ScriptEvent::Waiting
            } else {
                ScriptEvent::Delay
            });
        }
        for _ in 0..MAX_INSTRUCTIONS_PER_ADVANCE {
            let Some(entry) = self.table.entry(execution.entry).copied() else {
                self.execution = None;
                return Some(ScriptEvent::InvalidEntry {
                    trigger: execution.trigger,
                    entry: execution.entry,
                });
            };
            self.last_instruction = Some(ScriptInstructionDebug {
                object_id: execution.object_id,
                entry: execution.entry,
                opcode: entry.opcode,
                operands: entry.operands,
            });

            let Some(opcode) = ScriptOpcode::from_raw(entry.opcode) else {
                self.execution = None;
                return Some(ScriptEvent::Unsupported {
                    trigger: execution.trigger,
                    entry: execution.entry,
                    opcode: entry.opcode,
                });
            };
            use ScriptOpcode::*;
            let flow = match opcode {
                Stop | StopAndAdvance | StopAndReplace | Jump | Call | JumpByChance
                | AdvanceEntry | WaitFrames | MarkScriptFailed | NoOp | Delay | RandomSelect
                | AutoScriptNoOp => self.dispatch_control(execution, entry, opcode),
                Redraw
                | Confirm
                | OpenBuyMenu
                | OpenSellMenu
                | ShakeScreen
                | SelectRngAnimation
                | PlayRngAnimation
                | DialogCenter
                | DialogUpper
                | DialogLower
                | DialogCenterWindow
                | PlayMusic
                | PlaySound
                | WaitForKey
                | LoadLastSave
                | FadeToRed
                | FadeOut
                | FadeIn
                | UseDayPalette
                | UseNightPalette
                | SetScreenWave
                | FadeScene
                | ShowFbp
                | StopMusic
                | ToggleDayNightPalette
                | SetPalette
                | FadeColor
                | RestoreScreen
                | FadeSceneWithUpdate
                | PlayEndingAnimation
                | FadeToCurrentScene
                | QuitGame
                | PlayCdMusic
                | ScrollFbp
                | ShowFbpWithSprite
                | BackupScreen
                | PrintMessage => self.dispatch_presentation(execution, entry, opcode),
                WalkObjectSouth
                | WalkObjectWest
                | WalkObjectNorth
                | WalkObjectEast
                | SetObjectPose
                | WalkObjectTo
                | WalkObjectToSlow
                | SetObjectPositionRelative
                | SetObjectPosition
                | SetObjectGesture
                | SetPartyMemberPose
                | SetSelectedObjectPose
                | SetObjectAutoScript
                | SetObjectTriggerScript
                | TeleportParty
                | RideObjectSlow
                | SetObjectTriggerMode
                | RideObject
                | SetPartyPosition
                | SetObjectState
                | HideObjectShort
                | ChasePlayer
                | HideObject
                | ChangeScene
                | PauseEnemyChase
                | SpeedUpEnemyChase
                | OffsetObjectAndAnimate
                | SetSceneScripts
                | OffsetParty
                | SyncObjectState
                | WalkParty
                | WalkPartyFast
                | WalkPartyFastest
                | WalkObjectHalfSpeed
                | OffsetObject
                | SetObjectLayer
                | MoveViewport
                | WalkObjectFast
                | PlaceUsedItemObject
                | AnimateObject
                | SetObjectScript
                | RideObjectFast
                | SetSceneMap
                | SetObjectStates => self.dispatch_scene(execution, entry, opcode),
                SetEquipmentEffect
                | EquipItem
                | AdjustPlayerAttribute
                | SetPlayerAttribute
                | AdjustPlayerHp
                | AdjustPlayerMp
                | AdjustPlayerHpMp
                | AdjustCash
                | AddItem
                | RemoveItem
                | RevivePlayer
                | RemoveEquipment
                | AddMagic
                | RemoveMagic
                | SetPlayerSprite
                | SetParty
                | LevelUpPlayer
                | HalveCash
                | SetPartyFollower
                | CollapseParty => self.dispatch_role(execution, entry, opcode),
                StartBattle
                | DamageEnemy
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
                | DrainEnemyHp
                | FleeBattle
                | SimulatePlayerMagic
                | SetBattleMusic
                | SetBattlefield
                | ScaleMagicByMp
                | HalvePlayerHp
                | HalveEnemyHp
                | HideBattleActor
                | KillPlayer
                | KillEnemy
                | ThrowWeapon
                | EnemyCastMagic
                | EnemyEscape
                | StealEnemy
                | BlowEnemiesAway
                | ScaleMagicByCash
                | SetBattleResult
                | EnableAutoBattle
                | PlayerMagicAnimation
                | DivideEnemy
                | SummonEnemy
                | TransformEnemy => self.dispatch_battle(execution, entry, opcode),
                JumpIfItemCountLess
                | JumpIfPlayerLacksPoison
                | JumpIfEnemyLacksPoison
                | JumpIfPlayerNotPoisoned
                | JumpIfEnemyHpAbove
                | JumpIfEnemyTurn
                | JumpIfPartyNotFullHp
                | JumpIfPartyContainsPlayer
                | JumpIfNotFacingObject
                | JumpIfObjectOutsideZone
                | JumpIfItemNotEquipped
                | JumpIfEnemyNotFirstKind
                | JumpIfObjectStateEquals
                | JumpIfSceneEquals => self.dispatch_condition(execution, entry, opcode),
            };
            match flow {
                InstructionFlow::Continue(next) => execution = next,
                InstructionFlow::Yield(next, event) => {
                    self.execution = Some(next);
                    return Some(event);
                }
                InstructionFlow::Halt(event) => {
                    self.execution = None;
                    return Some(event);
                }
            }
        }

        self.execution = None;
        self.call_stack.clear();
        Some(ScriptEvent::InstructionLimit {
            trigger: execution.trigger,
            entry: execution.entry,
        })
    }
}
