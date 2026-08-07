//! Battle-owned script scheduling and lifecycle hooks.

use super::types::{
    add_poison, BattleEvent, BattleFlow, BattlePhase, BattleResult, BattleScriptRequest,
    BattleScriptSource, BattleState, BattleStatus, PlayerItemKind, PlayerItemPhase,
};

impl BattleState {
    pub fn has_script_work(&self) -> bool {
        self.active_script.is_some() || !self.pending_scripts.is_empty()
    }

    /// Restart the opening turn-start sequence after the owning game state layers
    /// mutable global-object script fields over freshly loaded enemies.
    pub(crate) fn refresh_initial_enemy_scripts(&mut self) {
        debug_assert_eq!(self.round, 1);
        debug_assert!(matches!(
            self.flow,
            BattleFlow::Command
                | BattleFlow::Finished
                | BattleFlow::TurnStartScripts {
                    completes_round: false,
                    ..
                }
        ));
        debug_assert!(self.active_script.is_none());
        debug_assert!(self.action_queue.is_empty());
        self.pending_scripts.clear();
        if self.phase == BattlePhase::AwaitingCommand {
            self.start_turn_start_scripts(false);
        }
    }

    /// Begin the next queued battle-owned script and retain its source until completion.
    pub fn take_script_request(&mut self) -> Option<BattleScriptRequest> {
        if self.active_script.is_some() {
            return None;
        }
        let request = self.pending_scripts.pop_front()?;
        self.active_script = Some(request);
        Some(request)
    }

    pub fn active_script_request(&self) -> Option<BattleScriptRequest> {
        self.active_script
    }

    /// Persist the next entry returned by a completed battle-owned script.
    pub fn complete_script(&mut self, next_entry: u16) -> bool {
        self.complete_script_with_result(next_entry, true)
    }

    pub fn complete_script_with_result(&mut self, next_entry: u16, succeeded: bool) -> bool {
        let Some(request) = self.active_script.take() else {
            return false;
        };
        match request.source {
            BattleScriptSource::EnemyTurnStart { enemy } => {
                let Some(actor) = self.enemies.get_mut(enemy) else {
                    return false;
                };
                actor.turn_start_script = next_entry;
            }
            BattleScriptSource::EnemyReady { enemy } => {
                let Some(actor) = self.enemies.get_mut(enemy) else {
                    return false;
                };
                actor.ready_script = next_entry;
            }
            BattleScriptSource::EnemyBattleEnd { enemy } => {
                let Some(actor) = self.enemies.get_mut(enemy) else {
                    return false;
                };
                actor.battle_end_script = next_entry;
            }
            BattleScriptSource::PlayerFriendDeath {
                player,
                name_object,
            } => {
                let Some(actor) = self
                    .players
                    .get_mut(player)
                    .filter(|actor| actor.name_word_id == name_object)
                else {
                    return false;
                };
                actor.friend_death_script = next_entry;
            }
            BattleScriptSource::PlayerDying {
                player,
                name_object,
            } => {
                let Some(actor) = self
                    .players
                    .get_mut(player)
                    .filter(|actor| actor.name_word_id == name_object)
                else {
                    return false;
                };
                actor.dying_script = next_entry;
            }
            BattleScriptSource::PlayerPoison { role_id, poison_id } => {
                if let Some(poison) = self
                    .players
                    .iter_mut()
                    .find(|player| player.role_id == role_id)
                    .and_then(|player| {
                        player
                            .poisons
                            .iter_mut()
                            .find(|poison| poison.object_id == poison_id)
                    })
                {
                    poison.script_entry = next_entry;
                }
            }
            BattleScriptSource::EnemyPoison { enemy, poison_id } => {
                if let Some(poison) = self.enemies.get_mut(enemy).and_then(|actor| {
                    actor
                        .poisons
                        .iter_mut()
                        .find(|poison| poison.object_id == poison_id)
                }) {
                    poison.script_entry = next_entry;
                }
            }
            BattleScriptSource::EnemyMagicUse {
                enemy,
                magic_object,
            } => {
                if let Some(magic) = self
                    .enemies
                    .get_mut(enemy)
                    .and_then(|actor| actor.magic.as_mut())
                    .filter(|magic| magic.object_id == magic_object)
                {
                    magic.use_script = next_entry;
                }
                if let BattleFlow::EnemyMagic {
                    enemy: active_enemy,
                    magic: active_magic,
                    use_succeeded,
                    ..
                } = &mut self.flow
                {
                    if *active_enemy == enemy && active_magic.object_id == magic_object {
                        *use_succeeded = succeeded;
                    }
                }
            }
            BattleScriptSource::EnemyMagicSuccess {
                enemy,
                magic_object,
            } => {
                if let Some(magic) = self
                    .enemies
                    .get_mut(enemy)
                    .and_then(|actor| actor.magic.as_mut())
                    .filter(|magic| magic.object_id == magic_object)
                {
                    magic.success_script = next_entry;
                }
            }
            BattleScriptSource::EnemyAttackItem { enemy, item_object } => {
                if let Some(actor) = self
                    .enemies
                    .get_mut(enemy)
                    .filter(|actor| actor.attack_equivalent_item == item_object)
                {
                    actor.attack_equivalent_item_script = next_entry;
                }
            }
            BattleScriptSource::PlayerMagicUse {
                player,
                magic_object,
            } => {
                if let Some(magic) = self.players.get_mut(player).and_then(|actor| {
                    actor
                        .magics
                        .iter_mut()
                        .find(|magic| magic.object_id == magic_object)
                }) {
                    magic.use_script = next_entry;
                }
                if let BattleFlow::PlayerMagic {
                    player: active_player,
                    magic: active_magic,
                    use_succeeded,
                    ..
                } = &mut self.flow
                {
                    if *active_player == player && active_magic.object_id == magic_object {
                        active_magic.use_script = next_entry;
                        *use_succeeded = succeeded;
                    }
                }
            }
            BattleScriptSource::PlayerMagicSuccess {
                player,
                magic_object,
            } => {
                if let Some(magic) = self.players.get_mut(player).and_then(|actor| {
                    actor
                        .magics
                        .iter_mut()
                        .find(|magic| magic.object_id == magic_object)
                }) {
                    magic.success_script = next_entry;
                }
                if let BattleFlow::PlayerMagic {
                    player: active_player,
                    magic: active_magic,
                    ..
                } = &mut self.flow
                {
                    if *active_player == player && active_magic.object_id == magic_object {
                        active_magic.success_script = next_entry;
                    }
                }
            }
            BattleScriptSource::PlayerItemUse {
                player,
                item_object,
            } => {
                let BattleFlow::PlayerItem {
                    player: active_player,
                    item_object: active_item,
                    kind: PlayerItemKind::Use { .. },
                    ..
                } = self.flow
                else {
                    return false;
                };
                if active_player != player || active_item != item_object {
                    return false;
                }
                if let BattleFlow::PlayerItem { phase, .. } = &mut self.flow {
                    *phase = PlayerItemPhase::Resolve;
                }
            }
            BattleScriptSource::PlayerItemThrow {
                player,
                item_object,
            } => {
                let BattleFlow::PlayerItem {
                    player: active_player,
                    item_object: active_item,
                    kind: PlayerItemKind::Throw,
                    ..
                } = self.flow
                else {
                    return false;
                };
                if active_player != player || active_item != item_object {
                    return false;
                }
                if let BattleFlow::PlayerItem { phase, .. } = &mut self.flow {
                    *phase = PlayerItemPhase::Resolve;
                }
            }
        }
        self.detect_script_outcome();
        if self.active_script.is_none() && self.pending_scripts.is_empty() {
            match self.flow {
                BattleFlow::TurnStartScripts { .. } => self.queue_next_turn_start_script(),
                BattleFlow::BattleEndScripts { .. } => self.queue_next_battle_end_script(),
                _ => {}
            }
        }
        true
    }
    pub fn set_script_result(&mut self, raw_result: u16) -> bool {
        let result = match raw_result {
            0 => BattleResult::Terminated,
            1 => BattleResult::Lost,
            3 => BattleResult::Won,
            u16::MAX => BattleResult::Fled,
            _ => return false,
        };
        if self.phase != BattlePhase::AwaitingCommand {
            return false;
        }

        if let BattleFlow::BattleEndScripts { next_slot, .. } = self.flow {
            self.flow = BattleFlow::BattleEndScripts { result, next_slot };
            return true;
        }

        let defer_until_round_end = matches!(
            self.active_script.map(|request| request.source),
            Some(BattleScriptSource::PlayerPoison { .. } | BattleScriptSource::EnemyPoison { .. })
        ) || matches!(
            (self.active_script.map(|request| request.source), self.flow),
            (
                Some(BattleScriptSource::EnemyTurnStart { .. }),
                BattleFlow::TurnStartScripts {
                    completes_round: true,
                    ..
                }
            )
        );
        if defer_until_round_end {
            self.deferred_round_result = Some(result);
            return true;
        }

        self.deferred_round_result = None;
        self.pending_scripts.clear();
        self.active_player = None;
        self.flow = BattleFlow::Outcome(result);
        true
    }

    pub fn enemy_escape(&mut self) -> bool {
        self.pending_events.push_back(BattleEvent::EnemyEscape);
        self.set_script_result(0)
    }

    pub fn poison_enemy(
        &mut self,
        enemy_index: usize,
        poison_id: u16,
        script_entry: u16,
        apply_to_all: bool,
    ) -> bool {
        if self.phase != BattlePhase::AwaitingCommand || poison_id == 0 {
            return false;
        }
        let targets = if apply_to_all {
            self.enemies
                .iter()
                .enumerate()
                .filter_map(|(index, enemy)| enemy.is_present().then_some(index))
                .collect::<Vec<_>>()
        } else if enemy_index < self.enemies.len() {
            vec![enemy_index]
        } else {
            return false;
        };
        for target in targets {
            let resistance = if self.enemies[target].is_present() {
                self.enemies[target].sorcery_resistance.min(9)
            } else {
                0
            };
            if self.random(10) >= u32::from(resistance) {
                let already_present = self.enemies[target]
                    .poisons
                    .iter()
                    .any(|poison| poison.object_id == poison_id);
                if add_poison(&mut self.enemies[target].poisons, poison_id, script_entry)
                    && !already_present
                    && script_entry != 0
                {
                    let Some(object_id) = self.enemy_slot_for_index(target) else {
                        return false;
                    };
                    self.pending_scripts.push_back(BattleScriptRequest {
                        source: BattleScriptSource::EnemyPoison {
                            enemy: target,
                            poison_id,
                        },
                        entry: script_entry,
                        object_id,
                    });
                }
            }
        }
        true
    }

    pub fn queue_player_poison_script(
        &mut self,
        role_id: u16,
        poison_id: u16,
        script_entry: u16,
    ) -> bool {
        if script_entry == 0
            || self
                .players
                .iter()
                .find(|player| player.role_id == role_id)
                .is_none_or(|player| {
                    !player
                        .poisons
                        .iter()
                        .any(|poison| poison.object_id == poison_id)
                })
        {
            return false;
        }
        self.pending_scripts.push_back(BattleScriptRequest {
            source: BattleScriptSource::PlayerPoison { role_id, poison_id },
            entry: script_entry,
            object_id: role_id,
        });
        true
    }

    pub(crate) fn queue_post_action_check(&mut self, check_players: bool) {
        self.collect_defeated_enemy_rewards();
        if !check_players || self.auto_battle {
            return;
        }

        for player in 0..self.players.len() {
            let actor = &self.players[player];
            if actor.hp >= self.previous_player_hp[player] || actor.hp != 0 {
                continue;
            }
            let Some(cover) = self.players.iter().position(|candidate| {
                candidate.role_id == actor.covered_by
                    && candidate.is_alive()
                    && !candidate.statuses.is_active(BattleStatus::Sleep)
                    && !candidate.statuses.is_active(BattleStatus::Paralyzed)
                    && !candidate.statuses.is_active(BattleStatus::Confused)
            }) else {
                continue;
            };
            let cover_actor = &self.players[cover];
            if cover_actor.friend_death_script == 0 {
                continue;
            }
            self.pending_events
                .push_back(BattleEvent::PlayerFriendDeath { player: cover });
            self.pending_scripts.push_back(BattleScriptRequest {
                source: BattleScriptSource::PlayerFriendDeath {
                    player: cover,
                    name_object: cover_actor.name_word_id,
                },
                entry: cover_actor.friend_death_script,
                object_id: cover_actor.role_id,
            });
            return;
        }

        for player in 0..self.players.len() {
            let actor = &self.players[player];
            if actor.statuses.is_active(BattleStatus::Sleep)
                || actor.statuses.is_active(BattleStatus::Confused)
                || actor.hp >= self.previous_player_hp[player]
                || !actor.is_alive()
                || !actor.is_dying()
                || self.previous_player_hp[player] < actor.max_hp / 5
            {
                continue;
            }
            let cover = self
                .players
                .iter()
                .position(|candidate| candidate.role_id == actor.covered_by);
            if cover.is_some_and(|cover| {
                let cover = &self.players[cover];
                cover.statuses.is_active(BattleStatus::Sleep)
                    || cover.statuses.is_active(BattleStatus::Paralyzed)
                    || cover.statuses.is_active(BattleStatus::Confused)
            }) {
                continue;
            }
            self.pending_events
                .push_back(BattleEvent::PlayerDying { player });
            if cover.is_none_or(|cover| !self.players[cover].is_alive()) {
                continue;
            }
            if actor.dying_script != 0 {
                self.pending_scripts.push_back(BattleScriptRequest {
                    source: BattleScriptSource::PlayerDying {
                        player,
                        name_object: actor.name_word_id,
                    },
                    entry: actor.dying_script,
                    object_id: actor.role_id,
                });
            }
            return;
        }
    }

    pub(super) fn round_poison_script_request(
        &self,
        actor: usize,
        poison_slot: usize,
    ) -> Option<BattleScriptRequest> {
        let player_count = self.players.len();
        if actor < player_count {
            let player = self.players.get(actor)?;
            let poison = *player.poisons.get(poison_slot)?;
            return (poison.object_id != 0 && poison.script_entry != 0).then_some(
                BattleScriptRequest {
                    source: BattleScriptSource::PlayerPoison {
                        role_id: player.role_id,
                        poison_id: poison.object_id,
                    },
                    entry: poison.script_entry,
                    object_id: player.role_id,
                },
            );
        }

        let slot = actor.checked_sub(player_count)?;
        let enemy = self.enemy_index_for_slot(slot)?;
        let target = self.enemies.get(enemy)?;
        let poison = *target.poisons.get(poison_slot)?;
        (poison.object_id != 0 && poison.script_entry != 0).then_some(BattleScriptRequest {
            source: BattleScriptSource::EnemyPoison {
                enemy,
                poison_id: poison.object_id,
            },
            entry: poison.script_entry,
            object_id: u16::try_from(slot).ok()?,
        })
    }

    pub(super) fn start_turn_start_scripts(&mut self, completes_round: bool) {
        let next_slot = if self.hiding_time == 0 {
            0
        } else {
            self.enemy_layout_slots
        };
        self.flow = BattleFlow::TurnStartScripts {
            next_slot,
            completes_round,
        };
        self.queue_next_turn_start_script();
    }

    fn queue_next_turn_start_script(&mut self) {
        let BattleFlow::TurnStartScripts {
            mut next_slot,
            completes_round,
        } = self.flow
        else {
            return;
        };
        while next_slot < self.enemy_layout_slots {
            let slot = next_slot;
            next_slot += 1;
            self.flow = BattleFlow::TurnStartScripts {
                next_slot,
                completes_round,
            };
            let Some(enemy) = self.enemy_index_for_slot(slot) else {
                continue;
            };
            let actor = &self.enemies[enemy];
            if !actor.is_alive() || actor.turn_start_script == 0 {
                continue;
            }
            self.pending_scripts.push_back(BattleScriptRequest {
                source: BattleScriptSource::EnemyTurnStart { enemy },
                entry: actor.turn_start_script,
                object_id: u16::try_from(slot).unwrap_or(u16::MAX),
            });
            return;
        }
        if completes_round {
            if let Some(result) = self.deferred_round_result.take() {
                self.active_player = None;
                self.flow = BattleFlow::Outcome(result);
                return;
            }
        }
        if !completes_round {
            self.flow = BattleFlow::Command;
        }
    }

    pub(super) fn start_battle_end_scripts(&mut self, result: BattleResult) {
        self.flow = BattleFlow::BattleEndScripts {
            result,
            next_slot: 0,
        };
        self.queue_next_battle_end_script();
    }

    fn queue_next_battle_end_script(&mut self) {
        let BattleFlow::BattleEndScripts {
            result,
            mut next_slot,
        } = self.flow
        else {
            return;
        };
        while next_slot < self.enemy_layout_slots {
            let slot = next_slot;
            next_slot += 1;
            self.flow = BattleFlow::BattleEndScripts { result, next_slot };
            let Some(enemy) = self.enemy_index_for_slot(slot) else {
                continue;
            };
            let actor = &self.enemies[enemy];
            if actor.battle_end_script == 0 {
                continue;
            }
            self.pending_scripts.push_back(BattleScriptRequest {
                source: BattleScriptSource::EnemyBattleEnd { enemy },
                entry: actor.battle_end_script,
                object_id: u16::try_from(slot).unwrap_or(u16::MAX),
            });
            return;
        }
    }

    pub(super) fn detect_script_outcome(&mut self) {
        if self.phase != BattlePhase::AwaitingCommand
            || self.has_script_work()
            || self.flow != BattleFlow::PerformActions
        {
            return;
        }
        let result = if self.enemies.iter().all(|enemy| !enemy.is_alive()) {
            Some(BattleResult::Won)
        } else if self.players.iter().all(|player| !player.is_combat_active()) {
            Some(BattleResult::Lost)
        } else {
            None
        };
        if let Some(result) = result {
            self.pending_scripts.clear();
            self.active_player = None;
            self.flow = BattleFlow::Outcome(result);
        }
    }
}
