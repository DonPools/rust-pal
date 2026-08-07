//! Battle trigger-opcode handling.

use pal_assets::script::ScriptEntry;

use super::{Execution, InstructionFlow, ScriptRuntime};
use crate::battle::BattleRequest;
use crate::script::{ScriptAction, ScriptEvent, ScriptOpcode};

impl ScriptRuntime {
    // Keep terminal arms structurally aligned with handlers that can also continue.
    #[allow(clippy::needless_return)]
    pub(super) fn dispatch_battle(
        &mut self,
        mut execution: Execution,
        entry: ScriptEntry,
        opcode: ScriptOpcode,
    ) -> InstructionFlow {
        use ScriptOpcode::*;
        match opcode {
            StartBattle => {
                let request = BattleRequest {
                    enemy_team: entry.operands[0],
                    lost_entry: entry.operands[1],
                    flee_entry: entry.operands[2],
                    is_boss: entry.operands[2] == 0,
                };
                execution.advance();
                self.pending_battle = Some(request);
                return InstructionFlow::Yield(execution, ScriptEvent::StartBattle(request));
            }
            SetBattleMusic => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::SetBattleMusic {
                        music_id: entry.operands[0],
                    }),
                );
            }
            SetBattlefield => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::SetBattlefield {
                        battlefield_id: entry.operands[0],
                    }),
                );
            }
            DamageEnemy => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::DamageEnemy {
                        enemy_index: execution.object_id,
                        amount: entry.operands[1],
                        apply_to_all: entry.operands[0] != 0,
                    }),
                );
            }
            PoisonEnemy => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::PoisonEnemy {
                        enemy_index: execution.object_id,
                        poison_id: entry.operands[1],
                        apply_to_all: entry.operands[0] != 0,
                    }),
                );
            }
            PoisonPlayer => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::PoisonPlayer {
                        role_id: execution.object_id,
                        poison_id: entry.operands[1],
                        apply_to_all: entry.operands[0] != 0,
                    }),
                );
            }
            CureEnemyPoison => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::CureEnemyPoison {
                        enemy_index: execution.object_id,
                        poison_id: entry.operands[1],
                        apply_to_all: entry.operands[0] != 0,
                    }),
                );
            }
            CurePlayerPoison => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::CurePlayerPoison {
                        role_id: execution.object_id,
                        poison_id: entry.operands[1],
                        apply_to_all: entry.operands[0] != 0,
                    }),
                );
            }
            CurePoisonByLevel => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::CurePlayerPoisonByLevel {
                        role_id: execution.object_id,
                        maximum_level: entry.operands[1],
                        apply_to_all: entry.operands[0] != 0,
                    }),
                );
            }
            SetPlayerStatus => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::SetPlayerStatus {
                        role_id: execution.object_id,
                        status: entry.operands[0],
                        rounds: entry.operands[1],
                    }),
                );
            }
            SetEnemyStatus => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::SetEnemyStatus {
                        enemy_index: execution.object_id,
                        status: entry.operands[0],
                        rounds: entry.operands[1],
                        resisted_entry: entry.operands[2],
                    }),
                );
            }
            RemovePlayerStatus => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::RemovePlayerStatus {
                        role_id: execution.object_id,
                        status: entry.operands[0],
                    }),
                );
            }
            AdjustTemporaryPlayerStat => {
                let role_id = entry.operands[2]
                    .checked_sub(1)
                    .unwrap_or(execution.object_id);
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::AdjustTemporaryPlayerStat {
                        role_id,
                        attribute: entry.operands[0],
                        percent: entry.operands[1] as i16,
                    }),
                );
            }
            SetTemporaryBattleSprite => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::SetTemporaryBattleSprite {
                        role_id: execution.object_id,
                        sprite: entry.operands[0],
                    }),
                );
            }
            CollectEnemy => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::CollectEnemy {
                        enemy_index: execution.object_id,
                        failure_entry: entry.operands[0],
                    }),
                );
            }
            TransmuteCollectedEnemies => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::TransmuteCollectedEnemies),
                );
            }
            SimulatePlayerMagic => {
                let selected = (entry.operands[2] as i16).wrapping_sub(1);
                let enemy_index = if selected < 0 {
                    execution.object_id
                } else {
                    selected as u16
                };
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::SimulatePlayerMagic {
                        enemy_index,
                        magic_object: entry.operands[0],
                        base_strength: entry.operands[1],
                    }),
                );
            }
            ThrowWeapon => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::ThrowWeapon {
                        enemy_index: execution.object_id,
                        magic_object: entry.operands[0],
                        multiplier: entry.operands[1],
                    }),
                );
            }
            ScaleMagicByMp => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::ScaleMagicByMp {
                        role_id: execution.object_id,
                        magic_object: entry.operands[0],
                        multiplier: if entry.operands[1] == 0 {
                            8
                        } else {
                            entry.operands[1]
                        },
                    }),
                );
            }
            ScaleMagicByCash => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::ScaleMagicByCash {
                        magic_object: entry.operands[0],
                    }),
                );
            }
            PlayerMagicAnimation => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::PlayerMagicAnimation {
                        player: entry.operands[0].checked_sub(1),
                    }),
                );
            }
            DrainEnemyHp => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::DrainEnemyHp {
                        enemy_index: execution.object_id,
                        amount: entry.operands[0],
                    }),
                );
            }
            FleeBattle => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::FleeBattle {
                        failure_entry: entry.operands[0],
                    }),
                );
            }
            HalvePlayerHp => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::HalvePlayerHp {
                        role_id: execution.object_id,
                    }),
                );
            }
            HalveEnemyHp => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::HalveEnemyHp {
                        enemy_index: execution.object_id,
                        maximum_damage: entry.operands[0],
                    }),
                );
            }
            HideBattleActor => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::HideBattleActor {
                        rounds: entry.operands[0],
                    }),
                );
            }
            KillPlayer => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::KillPlayer {
                        role_id: execution.object_id,
                    }),
                );
            }
            KillEnemy => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::KillEnemy {
                        enemy_index: execution.object_id,
                    }),
                );
            }
            EnemyCastMagic => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::SetEnemyMagic {
                        enemy_index: execution.object_id,
                        magic_object: entry.operands[0],
                        rate: entry.operands[1],
                    }),
                );
            }
            EnemyEscape => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::EnemyEscape),
                );
            }
            StealEnemy => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::StealEnemy {
                        enemy_index: execution.object_id,
                        rate: entry.operands[0],
                    }),
                );
            }
            BlowEnemiesAway => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::SetBattleBlow {
                        amount: entry.operands[0] as i16,
                    }),
                );
            }
            SetBattleResult => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::SetBattleResult {
                        result: entry.operands[0],
                    }),
                );
            }
            EnableAutoBattle => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::EnableAutoBattle),
                );
            }
            DivideEnemy => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::DivideEnemy {
                        enemy_index: execution.object_id,
                        copies: entry.operands[0],
                        failure_entry: entry.operands[1],
                    }),
                );
            }
            SummonEnemy => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::SummonEnemy {
                        enemy_index: execution.object_id,
                        object_id: entry.operands[0],
                        count: entry.operands[1],
                        failure_entry: entry.operands[2],
                    }),
                );
            }
            TransformEnemy => {
                execution.advance();
                return InstructionFlow::Yield(
                    execution,
                    ScriptEvent::Action(ScriptAction::TransformEnemy {
                        enemy_index: execution.object_id,
                        object_id: entry.operands[0],
                    }),
                );
            }
            _ => unreachable!("opcode {opcode} is not a battle instruction"),
        }
    }
}
