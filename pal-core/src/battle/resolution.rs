//! Deterministic battle-flow state machine.

use super::helpers::next_player;
use super::types::{
    BattleActorAction, BattleEvent, BattleFlow, BattlePhase, BattleResult, BattleScriptRequest,
    BattleScriptSource, BattleState, BattleStatus, EnemyMagicPhase, MagicEventPhase, PlayerAction,
    PlayerItemKind, PlayerItemPhase, PlayerMagicPhase, VictorySettlementStage, MAX_BATTLE_POISONS,
};

impl BattleState {
    pub fn advance_automatic_turns(&mut self) -> Vec<BattleEvent> {
        if self.phase != BattlePhase::AwaitingCommand
            || self.flow != BattleFlow::Command
            || !self.pending_scripts.is_empty()
            || self.active_script.is_some()
            || self.active_player.is_some()
        {
            return Vec::new();
        }
        self.build_action_queue();
        self.advance_resolution()
    }
    pub fn advance_resolution(&mut self) -> Vec<BattleEvent> {
        if !self.pending_events.is_empty() {
            return self.pending_events.drain(..).collect();
        }
        if self.phase != BattlePhase::AwaitingCommand || self.has_script_work() {
            return Vec::new();
        }
        let mut events = Vec::new();
        loop {
            match self.flow {
                BattleFlow::Command | BattleFlow::Finished => return events,
                BattleFlow::PerformActions => {
                    if self.enemies.iter().all(|enemy| !enemy.is_alive()) {
                        self.flow = BattleFlow::Outcome(BattleResult::Won);
                        continue;
                    }
                    if self.players.iter().all(|player| !player.is_combat_active()) {
                        self.flow = BattleFlow::Outcome(BattleResult::Lost);
                        continue;
                    }
                    let Some(queued) = self.action_queue.get(self.action_index).copied() else {
                        for player in &mut self.players {
                            player.defending = false;
                        }
                        self.round_start_player_hp =
                            self.players.iter().map(|player| player.hp).collect();
                        self.round_start_enemy_hp =
                            self.enemies.iter().map(|enemy| enemy.hp).collect();
                        self.flow = BattleFlow::RoundScripts {
                            actor: 0,
                            poison_slot: 0,
                        };
                        continue;
                    };
                    self.action_index += 1;
                    let queued = self.propagate_execution_auto_attack(queued);
                    let queued = self.validate_queued_item(queued);
                    let queued = self.validate_queued_target(queued);
                    match queued.action {
                        BattleActorAction::Player { player, action } => {
                            match action {
                                PlayerAction::Magic { magic, target } => {
                                    if self.begin_player_magic(player, magic, target) {
                                        if self.has_script_work() {
                                            return events;
                                        }
                                        continue;
                                    }
                                }
                                PlayerAction::UseItem { .. } | PlayerAction::ThrowItem { .. } => {
                                    if self.begin_player_item(player, action) {
                                        if self.has_script_work() {
                                            return events;
                                        }
                                        events.extend(self.pending_events.drain(..));
                                        self.detect_script_outcome();
                                        return events;
                                    }
                                }
                                PlayerAction::Attack { .. }
                                | PlayerAction::CooperativeMagic { .. }
                                | PlayerAction::Flee
                                | PlayerAction::Defend
                                | PlayerAction::AttackMate => {}
                            }
                            events.extend(self.perform_player_action(player, action));
                            self.queue_post_action_check(false);
                            if self.enemies.iter().all(|enemy| !enemy.is_alive()) {
                                self.flow = BattleFlow::Outcome(BattleResult::Won);
                            } else if self.players.iter().all(|actor| !actor.is_combat_active()) {
                                self.flow = BattleFlow::Outcome(BattleResult::Lost);
                            }
                            if events.is_empty() {
                                continue;
                            }
                            return events;
                        }
                        BattleActorAction::Enemy { enemy } => {
                            self.flow = BattleFlow::EnemyAction {
                                enemy,
                                ready_complete: false,
                            };
                            continue;
                        }
                    }
                }
                BattleFlow::EnemyAction {
                    enemy,
                    ready_complete,
                } => {
                    let Some(actor) = self.enemies.get(enemy) else {
                        self.flow = BattleFlow::PerformActions;
                        continue;
                    };
                    if !actor.can_act() || self.hiding_time != 0 {
                        self.flow = BattleFlow::PerformActions;
                        continue;
                    }
                    if !ready_complete && actor.ready_script != 0 {
                        let Some(object_id) = u16::try_from(actor.slot).ok() else {
                            self.flow = BattleFlow::Outcome(BattleResult::Lost);
                            continue;
                        };
                        self.pending_scripts.push_back(BattleScriptRequest {
                            source: BattleScriptSource::EnemyReady { enemy },
                            entry: actor.ready_script,
                            object_id,
                        });
                        self.flow = BattleFlow::EnemyAction {
                            enemy,
                            ready_complete: true,
                        };
                        return events;
                    }
                    let confused = actor.statuses.is_active(BattleStatus::Confused);
                    let magic_object = actor.magic_object;
                    let magic_rate = actor.magic_rate;
                    let silenced = actor.statuses.is_active(BattleStatus::Silence);
                    let magic = actor.magic;
                    self.backup_player_hp();
                    self.magic_blow = 0;
                    let Some(target) = self.random_living_player() else {
                        self.flow = BattleFlow::Outcome(BattleResult::Lost);
                        continue;
                    };
                    if confused {
                        self.flow = BattleFlow::PerformActions;
                        if let Some(event) = self.perform_confused_enemy_action(enemy) {
                            events.push(event);
                        }
                        self.queue_post_action_check(false);
                        return events;
                    }
                    if magic_object != 0 && !silenced && self.random(10) < u32::from(magic_rate) {
                        if magic_object == u16::MAX {
                            self.flow = BattleFlow::PerformActions;
                            return events;
                        }
                        if let Some(magic) = magic {
                            self.flow = BattleFlow::EnemyMagic {
                                enemy,
                                target,
                                magic,
                                phase: EnemyMagicPhase::UseScript,
                                use_succeeded: true,
                            };
                            if magic.use_script != 0 {
                                self.pending_scripts.push_back(BattleScriptRequest {
                                    source: BattleScriptSource::EnemyMagicUse {
                                        enemy,
                                        magic_object: magic.object_id,
                                    },
                                    entry: magic.use_script,
                                    object_id: self.players[target].role_id,
                                });
                                return events;
                            }
                            continue;
                        }
                    }
                    if let Some(
                        event @ BattleEvent::EnemyAttack {
                            player,
                            protected_by,
                            auto_defended,
                            ..
                        },
                    ) = self.perform_enemy_action(enemy, target)
                    {
                        let actor = &self.enemies[enemy];
                        let item_object = actor.attack_equivalent_item;
                        let item_rate = actor.attack_equivalent_item_rate;
                        let item_script = actor.attack_equivalent_item_script;
                        let poison_resistance = self.players[player].poison_resistance;
                        let item_triggered = protected_by.is_none()
                            && !auto_defended
                            && item_object != 0
                            && self.random(10) < u32::from(item_rate)
                            && self.random(100) + 1 > u32::from(poison_resistance);
                        if item_triggered && item_script != 0 {
                            self.pending_scripts.push_back(BattleScriptRequest {
                                source: BattleScriptSource::EnemyAttackItem { enemy, item_object },
                                entry: item_script,
                                object_id: self.players[player].role_id,
                            });
                            self.flow = BattleFlow::EnemyAttackItem { enemy };
                        } else {
                            self.flow = BattleFlow::PerformActions;
                            self.queue_post_action_check(true);
                        }
                        events.push(event);
                    } else {
                        self.flow = BattleFlow::PerformActions;
                    }
                    if !self.has_script_work()
                        && self.players.iter().all(|player| !player.is_combat_active())
                    {
                        self.flow = BattleFlow::Outcome(BattleResult::Lost);
                    }
                    return events;
                }
                BattleFlow::EnemyMagic {
                    enemy,
                    target,
                    magic,
                    phase,
                    use_succeeded,
                } => match phase {
                    EnemyMagicPhase::UseScript => {
                        if use_succeeded {
                            self.flow = BattleFlow::EnemyMagic {
                                enemy,
                                target,
                                magic,
                                phase: EnemyMagicPhase::Animation,
                                use_succeeded,
                            };
                            events.push(BattleEvent::EnemyMagic {
                                enemy,
                                player: target,
                                magic_object: magic.object_id,
                                blow: 0,
                                damage: 0,
                                phase: MagicEventPhase::Visual,
                                visual: true,
                                auto_defended: false,
                                defeated: false,
                            });
                            return events;
                        }
                        self.flow = BattleFlow::EnemyMagic {
                            enemy,
                            target,
                            magic,
                            phase: EnemyMagicPhase::Damage,
                            use_succeeded,
                        };
                    }
                    EnemyMagicPhase::Animation => {
                        if magic.success_script != 0 {
                            self.pending_scripts.push_back(BattleScriptRequest {
                                source: BattleScriptSource::EnemyMagicSuccess {
                                    enemy,
                                    magic_object: magic.object_id,
                                },
                                entry: magic.success_script,
                                object_id: self.players[target].role_id,
                            });
                            self.flow = BattleFlow::EnemyMagic {
                                enemy,
                                target,
                                magic,
                                phase: EnemyMagicPhase::SuccessScript,
                                use_succeeded,
                            };
                            return events;
                        }
                        self.flow = BattleFlow::EnemyMagic {
                            enemy,
                            target,
                            magic,
                            phase: EnemyMagicPhase::Damage,
                            use_succeeded,
                        };
                    }
                    EnemyMagicPhase::SuccessScript => {
                        self.flow = BattleFlow::EnemyMagic {
                            enemy,
                            target,
                            magic,
                            phase: EnemyMagicPhase::Damage,
                            use_succeeded,
                        };
                    }
                    EnemyMagicPhase::Damage => {
                        events.extend(self.perform_enemy_magic(enemy, target, magic));
                        self.flow = BattleFlow::PerformActions;
                        self.queue_post_action_check(true);
                        if !self.has_script_work()
                            && self.players.iter().all(|player| !player.is_combat_active())
                        {
                            self.flow = BattleFlow::Outcome(BattleResult::Lost);
                        }
                        return events;
                    }
                },
                BattleFlow::EnemyAttackItem { .. } => {
                    self.flow = BattleFlow::PerformActions;
                    self.queue_post_action_check(true);
                    return events;
                }
                BattleFlow::PlayerMagic {
                    player,
                    target,
                    magic,
                    phase,
                    use_succeeded,
                } => match phase {
                    PlayerMagicPhase::UseScript => {
                        if !use_succeeded {
                            self.record_magic_experience(player);
                            self.flow = BattleFlow::PerformActions;
                            continue;
                        }
                        self.flow = BattleFlow::PlayerMagic {
                            player,
                            target,
                            magic,
                            phase: PlayerMagicPhase::Animation,
                            use_succeeded,
                        };
                        if let Some(event) = self.player_magic_visual_event(player, target, magic) {
                            events.push(event);
                            return events;
                        }
                    }
                    PlayerMagicPhase::Animation => {
                        if magic.success_script != 0 {
                            self.pending_scripts.push_back(BattleScriptRequest {
                                source: BattleScriptSource::PlayerMagicSuccess {
                                    player,
                                    magic_object: magic.object_id,
                                },
                                entry: magic.success_script,
                                object_id: self.magic_success_owner(target),
                            });
                            self.flow = BattleFlow::PlayerMagic {
                                player,
                                target,
                                magic,
                                phase: PlayerMagicPhase::SuccessScript,
                                use_succeeded,
                            };
                            return events;
                        }
                        self.flow = BattleFlow::PlayerMagic {
                            player,
                            target,
                            magic,
                            phase: PlayerMagicPhase::Damage,
                            use_succeeded,
                        };
                    }
                    PlayerMagicPhase::SuccessScript => {
                        self.flow = BattleFlow::PlayerMagic {
                            player,
                            target,
                            magic,
                            phase: PlayerMagicPhase::Damage,
                            use_succeeded,
                        };
                    }
                    PlayerMagicPhase::Damage => {
                        events.extend(self.perform_player_magic(player, target, magic));
                        self.record_magic_experience(player);
                        self.flow = BattleFlow::PerformActions;
                        self.queue_post_action_check(false);
                        if self.enemies.iter().all(|enemy| !enemy.is_alive()) {
                            self.flow = BattleFlow::Outcome(BattleResult::Won);
                        }
                        return events;
                    }
                },
                BattleFlow::PlayerItem {
                    player,
                    item_object,
                    target,
                    kind,
                    script_entry,
                    object_id,
                    player_stats_before,
                    phase: PlayerItemPhase::Animation,
                } => {
                    self.flow = BattleFlow::PlayerItem {
                        player,
                        item_object,
                        target,
                        kind,
                        script_entry,
                        object_id,
                        player_stats_before,
                        phase: PlayerItemPhase::Script,
                    };
                    if script_entry != 0 {
                        let source = match kind {
                            PlayerItemKind::Use { .. } => BattleScriptSource::PlayerItemUse {
                                player,
                                item_object,
                            },
                            PlayerItemKind::Throw => BattleScriptSource::PlayerItemThrow {
                                player,
                                item_object,
                            },
                        };
                        self.pending_scripts.push_back(BattleScriptRequest {
                            source,
                            entry: script_entry,
                            object_id,
                        });
                        return events;
                    }
                    if let BattleFlow::PlayerItem { phase, .. } = &mut self.flow {
                        *phase = PlayerItemPhase::Resolve;
                    }
                }
                BattleFlow::PlayerItem {
                    phase: PlayerItemPhase::Script,
                    ..
                } => return events,
                BattleFlow::PlayerItem {
                    phase: PlayerItemPhase::Resolve,
                    ..
                } => {
                    self.finish_player_item();
                    events.extend(self.pending_events.drain(..));
                    self.detect_script_outcome();
                    return events;
                }
                BattleFlow::RoundScripts { actor, poison_slot } => {
                    let player_count = self.players.len();
                    let actor_count = player_count.saturating_add(self.enemy_layout_slots);
                    if actor < actor_count {
                        if poison_slot < MAX_BATTLE_POISONS {
                            self.flow = BattleFlow::RoundScripts {
                                actor,
                                poison_slot: poison_slot + 1,
                            };
                            if let Some(request) =
                                self.round_poison_script_request(actor, poison_slot)
                            {
                                self.pending_scripts.push_back(request);
                                return events;
                            }
                            continue;
                        }
                        if actor < player_count {
                            self.players[actor].statuses.decrement_round();
                        } else if let Some(enemy) = self.enemy_index_for_slot(actor - player_count)
                        {
                            self.enemies[enemy].statuses.decrement_round();
                        }
                        self.flow = BattleFlow::RoundScripts {
                            actor: actor + 1,
                            poison_slot: 0,
                        };
                        continue;
                    }
                    self.queue_post_action_check(false);
                    self.hiding_time = self.hiding_time.saturating_sub(1);
                    if self.enemies.iter().all(|enemy| !enemy.is_alive()) {
                        self.deferred_round_result = None;
                        self.flow = BattleFlow::Outcome(BattleResult::Won);
                        continue;
                    }
                    if self.players.iter().all(|player| !player.is_combat_active()) {
                        self.deferred_round_result = None;
                        self.flow = BattleFlow::Outcome(BattleResult::Lost);
                        continue;
                    }
                    self.start_turn_start_scripts(true);
                    if self.has_script_work() {
                        return events;
                    }
                }
                BattleFlow::TurnStartScripts {
                    completes_round: true,
                    ..
                } => {
                    self.round = self.round.saturating_add(1);
                    self.acted.fill(false);
                    self.cooperative_magic_performed = false;
                    self.execution_auto_attack = false;
                    self.action_queue.clear();
                    self.action_index = 0;
                    self.active_player = next_player(&self.players, &self.acted, 0);
                    self.flow = BattleFlow::Command;
                    let stats_changed = self
                        .players
                        .iter()
                        .map(|player| player.hp)
                        .ne(self.round_start_player_hp.iter().copied())
                        || self
                            .enemies
                            .iter()
                            .map(|enemy| enemy.hp)
                            .ne(self.round_start_enemy_hp.iter().copied());
                    if stats_changed {
                        events.push(BattleEvent::RoundCompleted);
                    }
                    return events;
                }
                BattleFlow::TurnStartScripts {
                    completes_round: false,
                    ..
                } => {
                    self.flow = BattleFlow::Command;
                    return events;
                }
                BattleFlow::Outcome(result) => {
                    self.finish(result, &mut events);
                    return events;
                }
                BattleFlow::BattleEndScripts { result, .. } => {
                    self.phase = BattlePhase::Finished(result);
                    self.flow = BattleFlow::Finished;
                    self.victory_settlement = VictorySettlementStage::ReadyToLeave;
                    events.push(BattleEvent::Finished(result));
                    return events;
                }
            }
        }
    }
    fn finish(&mut self, result: BattleResult, events: &mut Vec<BattleEvent>) {
        self.active_player = None;
        self.pending_scripts.clear();
        self.phase = BattlePhase::Finished(result);
        self.flow = BattleFlow::Finished;
        self.victory_settlement = if result == BattleResult::Won {
            VictorySettlementStage::RewardsPending
        } else {
            VictorySettlementStage::NotApplicable
        };
        events.push(BattleEvent::Finished(result));
    }
}
