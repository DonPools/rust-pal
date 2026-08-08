//! Battle-facing GameState integration, status, poison, and settlement rules.

use super::*;

impl<M: CollisionMap> GameState<M> {
    pub fn battle(&self) -> Option<&BattleState> {
        self.active_battle.as_ref()
    }

    pub fn battle_mut(&mut self) -> Option<&mut BattleState> {
        self.active_battle.as_mut()
    }

    pub fn auto_battle(&self) -> bool {
        self.auto_battle
    }

    pub fn is_enemy_turn(&self) -> bool {
        self.active_battle
            .as_ref()
            .is_some_and(BattleState::is_enemy_turn)
    }

    pub fn advance_battle_resolution(&mut self) -> Vec<BattleEvent> {
        let inventory = self.inventory.clone();
        let auto_battle = self.auto_battle;
        let events = self
            .active_battle
            .as_mut()
            .map(|battle| {
                battle.set_auto_battle(auto_battle);
                battle.set_inventory_amounts(&inventory);
                battle.advance_resolution()
            })
            .unwrap_or_default();
        for event in &events {
            match *event {
                BattleEvent::PlayerItemFeedback {
                    item_object,
                    consume: true,
                    ..
                } => {
                    let _ = self.consume_inventory_item(item_object);
                }
                BattleEvent::PlayerItemFeedback { consume: false, .. }
                | BattleEvent::PlayerUseItem { .. }
                | BattleEvent::PlayerThrowItem { .. }
                | BattleEvent::PlayerAttack { .. }
                | BattleEvent::PlayerMagic { .. }
                | BattleEvent::EnemyAttack { .. }
                | BattleEvent::EnemyMagic { .. }
                | BattleEvent::EnemyConfusedAttack { .. }
                | BattleEvent::PlayerConfusedAttack { .. }
                | BattleEvent::SimulatedMagic { .. }
                | BattleEvent::PlayerFlee { .. }
                | BattleEvent::PlayerDefend { .. }
                | BattleEvent::PlayerDefensiveMagic { .. }
                | BattleEvent::PlayerCooperativeMagic { .. }
                | BattleEvent::PlayerMagicAnimation { .. }
                | BattleEvent::PlayerFriendDeath { .. }
                | BattleEvent::PlayerDying { .. }
                | BattleEvent::EnemyDivide { .. }
                | BattleEvent::EnemySummon { .. }
                | BattleEvent::EnemyTransform { .. }
                | BattleEvent::EnemyEscape
                | BattleEvent::RoundCompleted
                | BattleEvent::Finished(_) => {}
            }
        }
        events
    }

    pub fn take_battle_script(&mut self) -> Option<TriggerRequest> {
        let request = self.active_battle.as_mut()?.take_script_request()?;
        Some(TriggerRequest {
            object_id: request.object_id,
            script_entry: request.entry,
            kind: TriggerKind::Battle,
        })
    }

    pub fn finish_battle_script(&mut self, next_entry: u16, succeeded: bool) -> bool {
        let (source, updates) = {
            let Some(battle) = self.active_battle.as_mut() else {
                return false;
            };
            let Some(source) = battle.active_script_request().map(|request| request.source) else {
                return false;
            };
            if !battle.complete_script_with_result(next_entry, succeeded) {
                return false;
            }
            (
                source,
                battle
                    .players
                    .iter()
                    .map(|player| (player.role_id, player.statuses, player.poisons))
                    .collect::<Vec<_>>(),
            )
        };
        match source {
            BattleScriptSource::EnemyMagicUse { magic_object, .. } => {
                self.magic_use_scripts.insert(magic_object, next_entry);
            }
            BattleScriptSource::EnemyMagicSuccess { magic_object, .. } => {
                self.magic_success_scripts.insert(magic_object, next_entry);
            }
            BattleScriptSource::PlayerMagicUse { magic_object, .. } => {
                self.magic_use_scripts.insert(magic_object, next_entry);
            }
            BattleScriptSource::PlayerMagicSuccess { magic_object, .. } => {
                self.magic_success_scripts.insert(magic_object, next_entry);
            }
            BattleScriptSource::EnemyAttackItem { item_object, .. } => {
                self.item_use_scripts.insert(item_object, next_entry);
            }
            BattleScriptSource::PlayerItemUse { item_object, .. } => {
                self.item_use_scripts.insert(item_object, next_entry);
            }
            BattleScriptSource::PlayerItemThrow { item_object, .. } => {
                self.item_throw_scripts.insert(item_object, next_entry);
            }
            BattleScriptSource::PlayerFriendDeath { name_object, .. } => {
                self.object_script_overrides
                    .insert((name_object, 0), next_entry);
            }
            BattleScriptSource::PlayerDying { name_object, .. } => {
                self.object_script_overrides
                    .insert((name_object, 1), next_entry);
            }
            BattleScriptSource::EnemyTurnStart { .. }
            | BattleScriptSource::EnemyReady { .. }
            | BattleScriptSource::EnemyBattleEnd { .. }
            | BattleScriptSource::PlayerPoison { .. }
            | BattleScriptSource::EnemyPoison { .. } => {}
        }
        for (role_id, statuses, poisons) in updates {
            let role = usize::from(role_id);
            let (Some(saved_statuses), Some(saved_poisons)) = (
                self.player_statuses.get_mut(role),
                self.player_poisons.get_mut(role),
            ) else {
                return false;
            };
            *saved_statuses = statuses;
            *saved_poisons = poisons;
        }
        true
    }

    pub fn player_experience(&self, role_id: u16) -> Option<u32> {
        self.role_experience.get(usize::from(role_id)).copied()
    }

    pub fn player_next_level_experience(&self, role_id: u16) -> Option<u32> {
        let level = self.player_role(role_id)?.level;
        self.battle_data
            .as_ref()?
            .level_up_experience
            .for_level(level)
            .map(u32::from)
    }

    pub fn poison_level_and_color(&self, poison_id: u16) -> Option<(u16, u16)> {
        let poison = self.global_objects.as_ref()?.get(poison_id)?;
        Some((poison.poison_level(), poison.poison_color()))
    }

    pub fn player_status_duration(&self, role_id: u16, status: BattleStatus) -> Option<u16> {
        if let Some(player) = self.active_battle.as_ref().and_then(|battle| {
            battle
                .players
                .iter()
                .find(|player| player.role_id == role_id)
        }) {
            return Some(player.statuses.duration(status));
        }
        Some(
            self.player_statuses
                .get(usize::from(role_id))?
                .duration(status),
        )
    }

    pub fn player_poisons(&self, role_id: u16) -> Option<&[BattlePoison; MAX_BATTLE_POISONS]> {
        if let Some(player) = self.active_battle.as_ref().and_then(|battle| {
            battle
                .players
                .iter()
                .find(|player| player.role_id == role_id)
        }) {
            return Some(&player.poisons);
        }
        self.player_poisons.get(usize::from(role_id))
    }

    pub fn player_has_poison(&self, role_id: u16, poison_id: u16) -> bool {
        poison_id != 0
            && self
                .player_poisons(role_id)
                .is_some_and(|poisons| poisons.iter().any(|poison| poison.object_id == poison_id))
    }

    pub fn enemy_has_poison(&self, enemy_index: u16, poison_id: u16) -> bool {
        poison_id != 0
            && self
                .active_battle
                .as_ref()
                .and_then(|battle| {
                    battle
                        .enemy_index_for_slot(usize::from(enemy_index))
                        .and_then(|index| battle.enemies.get(index))
                })
                .is_some_and(|enemy| {
                    enemy
                        .poisons
                        .iter()
                        .any(|poison| poison.object_id == poison_id)
                })
    }

    pub fn enemy_hp_above(&self, enemy_index: u16, percentage: u16) -> bool {
        let (Some(battle), Some(objects), Some(data)) = (
            self.active_battle.as_ref(),
            self.global_objects.as_ref(),
            self.battle_data.as_ref(),
        ) else {
            return false;
        };
        let Some(index) = battle.enemy_index_for_slot(usize::from(enemy_index)) else {
            return false;
        };
        let Some(definition_hp) = battle
            .enemies
            .get(index)
            .and_then(|enemy| objects.get(enemy.object_id))
            .and_then(|object| data.enemies.get(object.enemy_id()))
            .map(|enemy| enemy.health)
        else {
            return false;
        };
        battle
            .enemy_hp_above_with_max(index, percentage, definition_hp)
            .unwrap_or(false)
    }

    pub fn enemy_not_first_kind(&self, enemy_index: u16) -> bool {
        self.active_battle
            .as_ref()
            .and_then(|battle| {
                battle
                    .enemy_index_for_slot(usize::from(enemy_index))
                    .and_then(|index| battle.enemy_not_first_kind(index))
            })
            .unwrap_or(false)
    }

    pub(super) fn battle_enemy_index(&self, enemy_slot: u16) -> Option<usize> {
        self.active_battle
            .as_ref()?
            .enemy_index_for_slot(usize::from(enemy_slot))
    }

    /// Resolve the signed target used by Classic's simulated player magic.
    /// `0xFFFF` is the script representation of `-1`; all-target magic ignores the
    /// concrete index, while single-target magic falls back to a living enemy.
    pub(super) fn battle_simulated_magic_target(&self, enemy_slot: u16) -> Option<usize> {
        if enemy_slot == u16::MAX {
            self.active_battle.as_ref()?.first_living_enemy()
        } else {
            self.battle_enemy_index(enemy_slot)
        }
    }

    /// Transform a battle enemy selected by its original slot.
    ///
    /// Returns `Some(false)` when the instruction is valid but an active status suppresses it.
    pub fn transform_enemy(&mut self, enemy_slot: u16, object_id: u16) -> Option<bool> {
        let enemy_index = self.battle_enemy_index(enemy_slot)?;
        let object_script_overrides = &self.object_script_overrides;
        let magic_use_scripts = &self.magic_use_scripts;
        let magic_success_scripts = &self.magic_success_scripts;
        let item_use_scripts = &self.item_use_scripts;
        let (battle, data, objects, magics) = (
            self.active_battle.as_mut()?,
            self.battle_data.as_ref()?,
            self.global_objects.as_ref()?,
            self.magics.as_ref()?,
        );
        let transformed = battle.transform_enemy(enemy_index, object_id, data, objects, magics)?;
        if transformed {
            let enemy = battle.enemies.get_mut(enemy_index)?;
            apply_enemy_script_overrides(
                enemy,
                object_script_overrides,
                magic_use_scripts,
                magic_success_scripts,
                item_use_scripts,
                false,
            );
        }
        Some(transformed)
    }

    pub fn collect_value(&self) -> u16 {
        self.collect_value
    }

    pub(super) fn transmute_collected_enemies(&mut self) -> bool {
        if self.collect_value == 0 {
            return true;
        }
        let Some(items) = self
            .stores
            .as_ref()
            .and_then(|stores| stores.get(0))
            .map(|store| store.items().collect::<Vec<_>>())
            .filter(|items| items.len() >= usize::from(self.collect_value.min(9)))
        else {
            return false;
        };
        let spent = u16::try_from(self.growth_random(u32::from(self.collect_value)) + 1)
            .unwrap_or(1)
            .min(9);
        let item_id = items[usize::from(spent - 1)];
        let amount = self.inventory_count(item_id).saturating_add(1).min(99);
        if !self.set_inventory_amount(item_id, amount) {
            return false;
        }
        self.collect_value -= spent;
        true
    }

    pub fn magic_sound(&self, magic_object: u16) -> Option<u16> {
        let magic_number = self
            .global_objects
            .as_ref()?
            .get(magic_object)?
            .magic_number();
        u16::try_from(self.magics.as_ref()?.get(magic_number)?.sound).ok()
    }

    pub fn has_magic_definition(&self, magic_object: u16) -> bool {
        self.global_objects
            .as_ref()
            .and_then(|objects| objects.get(magic_object))
            .and_then(|object| self.magics.as_ref()?.get(object.magic_number()))
            .is_some()
    }

    pub(super) fn set_player_status(&mut self, role_id: u16, status: u16, rounds: u16) -> bool {
        let Some(status) = BattleStatus::from_raw(status) else {
            return false;
        };
        if let Some(battle) = self.active_battle.as_mut() {
            let Some(player) = battle
                .players
                .iter_mut()
                .find(|player| player.role_id == role_id)
            else {
                return false;
            };
            if !player
                .statuses
                .set_for_player(status, rounds, player.is_alive())
            {
                return false;
            }
            let statuses = player.statuses;
            let Some(saved) = self.player_statuses.get_mut(usize::from(role_id)) else {
                return false;
            };
            *saved = statuses;
            battle.refresh_player_effects();
            return true;
        }
        let Some(alive) = self.player_role(role_id).map(|role| role.hp != 0) else {
            return false;
        };
        let Some(statuses) = self.player_statuses.get_mut(usize::from(role_id)) else {
            return false;
        };
        if !statuses.set_for_player(status, rounds, alive) {
            return false;
        }
        if let Some(player) = self.active_battle.as_mut().and_then(|battle| {
            battle
                .players
                .iter_mut()
                .find(|player| player.role_id == role_id)
        }) {
            player.statuses = *statuses;
        }
        true
    }

    pub(super) fn remove_player_status(&mut self, role_id: u16, status: u16) -> bool {
        let Some(status) = BattleStatus::from_raw(status) else {
            return false;
        };
        if let Some(battle) = self.active_battle.as_mut() {
            let Some(player) = battle
                .players
                .iter_mut()
                .find(|player| player.role_id == role_id)
            else {
                return false;
            };
            player.statuses.remove_from_player(status);
            let statuses = player.statuses;
            let Some(saved) = self.player_statuses.get_mut(usize::from(role_id)) else {
                return false;
            };
            *saved = statuses;
            battle.refresh_player_effects();
            return true;
        }
        let Some(statuses) = self.player_statuses.get_mut(usize::from(role_id)) else {
            return false;
        };
        statuses.remove_from_player(status);
        if let Some(player) = self.active_battle.as_mut().and_then(|battle| {
            battle
                .players
                .iter_mut()
                .find(|player| player.role_id == role_id)
        }) {
            player.statuses = *statuses;
        }
        true
    }

    pub(super) fn poison_player(
        &mut self,
        role_id: u16,
        poison_id: u16,
        apply_to_all: bool,
    ) -> bool {
        let Some(poison_object) = self
            .global_objects
            .as_ref()
            .and_then(|objects| objects.get(poison_id))
            .copied()
        else {
            return false;
        };
        let poison_script = self
            .object_script_overrides
            .get(&(poison_id, 0))
            .copied()
            .unwrap_or_else(|| poison_object.poison_player_script());
        let targets = if apply_to_all {
            self.party
                .members()
                .iter()
                .map(|member| member.role_id)
                .collect::<Vec<_>>()
        } else if self
            .party
            .members()
            .iter()
            .any(|member| member.role_id == role_id)
        {
            vec![role_id]
        } else {
            return false;
        };
        for target in targets {
            let Some(resistance) = self
                .active_battle
                .as_ref()
                .and_then(|battle| {
                    battle
                        .players
                        .iter()
                        .find(|player| player.role_id == target)
                        .map(|player| player.poison_resistance.min(100))
                })
                .or_else(|| {
                    self.effective_player_role(target)
                        .map(|role| role.poison_resistance.min(100))
                })
            else {
                return false;
            };
            if self.growth_random(100) < u32::from(resistance) {
                continue;
            }
            let target_index = usize::from(target);
            let already_present = self.player_poisons[target_index]
                .iter()
                .any(|poison| poison.object_id == poison_id);
            let added = add_poison(
                &mut self.player_poisons[target_index],
                poison_id,
                poison_script,
            );
            let face_color = self
                .global_objects
                .as_ref()
                .and_then(|objects| poison_face_color(&self.player_poisons[target_index], objects));
            if let Some(battle) = self.active_battle.as_mut() {
                if let Some(player) = battle
                    .players
                    .iter_mut()
                    .find(|player| player.role_id == target)
                {
                    player.poisons = self.player_poisons[target_index];
                    player.poison_face_color = face_color;
                }
                if added && !already_present {
                    battle.queue_player_poison_script(target, poison_id, poison_script);
                }
            }
        }
        true
    }

    pub(super) fn cure_player_poison(
        &mut self,
        role_id: u16,
        poison_id: u16,
        apply_to_all: bool,
    ) -> bool {
        if poison_id == 0 {
            return false;
        }
        let Some(targets) = self.poison_targets(role_id, apply_to_all) else {
            return false;
        };
        for target in targets {
            let target_index = usize::from(target);
            cure_poison(&mut self.player_poisons[target_index], poison_id);
            let face_color = self
                .global_objects
                .as_ref()
                .and_then(|objects| poison_face_color(&self.player_poisons[target_index], objects));
            if let Some(player) = self.active_battle.as_mut().and_then(|battle| {
                battle
                    .players
                    .iter_mut()
                    .find(|player| player.role_id == target)
            }) {
                player.poisons = self.player_poisons[target_index];
                player.poison_face_color = face_color;
            }
        }
        true
    }

    pub(super) fn cure_player_poison_by_level(
        &mut self,
        role_id: u16,
        maximum_level: u16,
        apply_to_all: bool,
    ) -> bool {
        let Some(targets) = self.poison_targets(role_id, apply_to_all) else {
            return false;
        };
        let Some(objects) = self.global_objects.as_ref() else {
            return false;
        };
        for target in targets {
            let target_index = usize::from(target);
            cure_poison_by_level(
                &mut self.player_poisons[target_index],
                maximum_level,
                objects,
            );
            let face_color = poison_face_color(&self.player_poisons[target_index], objects);
            if let Some(player) = self.active_battle.as_mut().and_then(|battle| {
                battle
                    .players
                    .iter_mut()
                    .find(|player| player.role_id == target)
            }) {
                player.poisons = self.player_poisons[target_index];
                player.poison_face_color = face_color;
            }
        }
        true
    }

    pub(super) fn poison_targets(&self, role_id: u16, apply_to_all: bool) -> Option<Vec<u16>> {
        if apply_to_all {
            return Some(
                self.party
                    .members()
                    .iter()
                    .map(|member| member.role_id)
                    .collect(),
            );
        }
        self.party
            .members()
            .iter()
            .any(|member| member.role_id == role_id)
            .then_some(vec![role_id])
    }

    /// Create a battle from the current party and script-selected battle configuration.
    pub fn start_battle(&mut self, request: BattleRequest, scripts: &ScriptTable) -> bool {
        if self.active_battle.is_some() {
            return false;
        }
        let role_ids = self
            .party
            .members()
            .iter()
            .map(|member| member.role_id)
            .collect::<Vec<_>>();
        if let Some(roles) = self.player_roles.as_mut() {
            for &role_id in &role_ids {
                let role_index = usize::from(role_id);
                let Some(role) = roles.role_mut(role_index) else {
                    return false;
                };
                if role.hp == 0 {
                    role.hp = 1;
                    if let Some(statuses) = self.player_statuses.get_mut(role_index) {
                        statuses.remove_from_player(BattleStatus::Puppet);
                    }
                }
            }
            self.party.sync_from_roles(roles);
        }
        if !self.refresh_equipment_effects(scripts) {
            return false;
        }
        let Some(roles) = role_ids
            .iter()
            .map(|&role_id| Some((role_id, self.effective_player_role(role_id)?)))
            .collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        let Some(data) = self.battle_data.as_ref() else {
            return false;
        };
        let Some(objects) = self.global_objects.as_ref() else {
            return false;
        };
        let Some(magics) = self.magics.as_ref() else {
            return false;
        };
        let Some(mut battle) = BattleState::new(
            request,
            self.current_battlefield,
            self.current_battle_music,
            roles.iter().map(|(role_id, role)| (*role_id, role)),
            data,
            objects,
            magics,
        ) else {
            return false;
        };
        battle.set_random_state(self.growth_random_state);
        for enemy in &mut battle.enemies {
            apply_enemy_script_overrides(
                enemy,
                &self.object_script_overrides,
                &self.magic_use_scripts,
                &self.magic_success_scripts,
                &self.item_use_scripts,
                true,
            );
        }
        battle.refresh_initial_enemy_scripts();
        for player in &mut battle.players {
            player.friend_death_script = self
                .object_script_overrides
                .get(&(player.name_word_id, 0))
                .copied()
                .unwrap_or(player.friend_death_script);
            player.dying_script = self
                .object_script_overrides
                .get(&(player.name_word_id, 1))
                .copied()
                .unwrap_or(player.dying_script);
            for magic in &mut player.magics {
                magic.use_script = self
                    .magic_use_scripts
                    .get(&magic.object_id)
                    .copied()
                    .unwrap_or(magic.use_script);
                magic.success_script = self
                    .magic_success_scripts
                    .get(&magic.object_id)
                    .copied()
                    .unwrap_or(magic.success_script);
            }
            let role_index = usize::from(player.role_id);
            let (Some(statuses), Some(poisons)) = (
                self.player_statuses.get(role_index),
                self.player_poisons.get(role_index),
            ) else {
                return false;
            };
            player.statuses = *statuses;
            player.poisons = *poisons;
            player.poison_face_color = poison_face_color(poisons, objects);
        }
        battle.refresh_player_effects();
        battle.set_auto_battle(self.auto_battle);
        self.active_battle = Some(battle);
        true
    }

    /// Apply victory rewards before the enemy battle-end scripts.
    pub fn prepare_battle_victory(&mut self) -> Option<BattleRewards> {
        self.prepare_battle_victory_settlement()
            .map(|settlement| settlement.rewards)
    }

    /// Apply victory rewards in Classic's per-player order and return presentation details.
    pub fn prepare_battle_victory_settlement(&mut self) -> Option<BattleVictorySettlement> {
        let battle = self.active_battle.as_ref()?.clone();
        if battle.phase() != BattlePhase::Finished(BattleResult::Won)
            || !battle.victory_rewards_pending()
        {
            return None;
        }
        if let Some(roles) = self.player_roles.as_mut() {
            for player in &battle.players {
                let role = roles.role_mut(usize::from(player.role_id))?;
                role.hp = player.hp;
                role.mp = player.mp;
            }
            self.party.sync_from_roles(roles);
        }
        let rewards = battle.rewards();
        self.cash = self.cash.saturating_add(rewards.cash);
        let mut player_settlements = Vec::new();
        for (player_index, player) in battle.players.iter().enumerate() {
            self.clear_hidden_experience_counts(player.role_id);
            if !player.is_alive() {
                continue;
            }
            let levels_gained =
                self.award_battle_experience_for_role(player.role_id, rewards.experience);
            let hidden_growth = self.award_hidden_battle_experience_for_player(
                &battle,
                player_index,
                rewards.experience,
            );
            if levels_gained != 0 {
                self.restore_role_after_battle_level_up(player.role_id)?;
            }
            let learned_magics = self.learn_eligible_magics_for_role(player.role_id);
            player_settlements.push(BattlePlayerSettlement {
                role_id: player.role_id,
                levels_gained,
                hidden_growth,
                learned_magics,
            });
        }

        let updated = battle
            .players
            .iter()
            .filter_map(|player| {
                let role = self.player_role(player.role_id)?;
                Some((
                    player.role_id,
                    role.level,
                    role.hp,
                    role.max_hp,
                    role.mp,
                    role.max_mp,
                ))
            })
            .collect::<Vec<_>>();
        let active = self.active_battle.as_mut()?;
        for (role_id, level, hp, max_hp, mp, max_mp) in updated {
            let player = active
                .players
                .iter_mut()
                .find(|player| player.role_id == role_id)?;
            player.level = level;
            player.hp = hp;
            player.max_hp = max_hp;
            player.mp = mp;
            player.max_mp = max_mp;
        }
        active
            .mark_victory_rewards_applied()
            .then_some(BattleVictorySettlement {
                rewards,
                players: player_settlements,
            })
    }

    pub fn begin_battle_end_scripts(&mut self) -> bool {
        self.active_battle
            .as_mut()
            .is_some_and(BattleState::begin_battle_end_scripts)
    }

    /// Apply post-script recovery and cleanup, then leave the finished battle state.
    pub fn settle_battle(&mut self) -> Option<(BattleResult, BattleRewards)> {
        let battle_state = self.active_battle.as_ref()?;
        let result = match battle_state.phase() {
            BattlePhase::Finished(result) if battle_state.ready_to_leave() => result,
            BattlePhase::Finished(_) | BattlePhase::AwaitingCommand => return None,
        };
        let victory_rewards_applied = battle_state.victory_rewards_were_applied();
        let rewards = if victory_rewards_applied {
            battle_state.rewards()
        } else {
            BattleRewards::default()
        };
        let objects = self.global_objects.as_ref()?;
        let battle = self.active_battle.take()?;
        self.growth_random_state = battle.random_state();
        if let Some(roles) = self.player_roles.as_mut() {
            for player in &battle.players {
                let role = roles.role_mut(usize::from(player.role_id))?;
                role.hp = player.hp;
                role.mp = player.mp;
                if victory_rewards_applied {
                    role.hp = role
                        .hp
                        .saturating_add(role.max_hp.saturating_sub(role.hp) / 2);
                    role.mp = role
                        .mp
                        .saturating_add(role.max_mp.saturating_sub(role.mp) / 2);
                }
            }
            self.party.sync_from_roles(roles);
        }
        for player in &battle.players {
            let role_index = usize::from(player.role_id);
            *self.player_statuses.get_mut(role_index)? = player.statuses;
            *self.player_poisons.get_mut(role_index)? = player.poisons;
            cure_poison_by_level(&mut self.player_poisons[role_index], 3, objects);
        }
        for statuses in &mut self.player_statuses {
            for status in BattleStatus::ALL {
                statuses.remove_from_player(status);
            }
        }
        self.auto_battle = false;
        Some((result, rewards))
    }

    pub(super) fn award_battle_experience_for_role(&mut self, role_id: u16, gained: u32) -> u16 {
        let role_index = usize::from(role_id);
        let clamped_level = {
            let Some(role) = self
                .player_roles
                .as_mut()
                .and_then(|roles| roles.role_mut(role_index))
            else {
                return 0;
            };
            let clamped = role.level > 99;
            role.level = role.level.min(99);
            clamped
        };
        if clamped_level {
            if let Some(roles) = self.player_roles.as_ref() {
                self.party.sync_from_roles(roles);
            }
            if let Some(player) = self.active_battle.as_mut().and_then(|battle| {
                battle
                    .players
                    .iter_mut()
                    .find(|player| player.role_id == role_id)
            }) {
                player.level = 99;
            }
        }
        let Some(current) = self.role_experience.get_mut(role_index) else {
            return 0;
        };
        *current = current.saturating_add(gained);
        let mut levels_gained = 0u16;
        while let Some(level) = self
            .player_roles
            .as_ref()
            .and_then(|roles| roles.role(role_index))
            .map(|role| role.level)
        {
            let Some(required) = self
                .battle_data
                .as_ref()
                .and_then(|data| data.level_up_experience.for_level(level))
                .map(u32::from)
                .filter(|&required| required > 0)
            else {
                break;
            };
            if self.role_experience[role_index] < required {
                break;
            }
            self.role_experience[role_index] -= required;
            if level < 99 {
                if !self.level_up_role(role_id) {
                    break;
                }
                levels_gained = levels_gained.saturating_add(1);
            }
        }
        levels_gained
    }

    pub(super) fn clear_hidden_experience_counts(&mut self, role_id: u16) {
        const SAVE_CATEGORY_OFFSET: usize = 1;
        let role_index = usize::from(role_id);
        for category in 0..HIDDEN_EXPERIENCE_CATEGORY_COUNT {
            if let Some(saved) = self
                .save_experience
                .get_mut(category + SAVE_CATEGORY_OFFSET)
                .and_then(|roles| roles.get_mut(role_index))
            {
                saved.count = 0;
            }
        }
    }

    pub(super) fn award_hidden_battle_experience_for_player(
        &mut self,
        battle: &BattleState,
        player_index: usize,
        gained: u32,
    ) -> [u16; HIDDEN_EXPERIENCE_CATEGORY_COUNT] {
        const SAVE_CATEGORY_OFFSET: usize = 1;
        let mut growth_totals = [0u16; HIDDEN_EXPERIENCE_CATEGORY_COUNT];
        let Some(player) = battle.players.get(player_index) else {
            return growth_totals;
        };
        let role_index = usize::from(player.role_id);
        let Some(counts) = battle.hidden_experience_counts(player_index) else {
            return growth_totals;
        };
        let total = counts
            .iter()
            .fold(0u32, |total, count| total.saturating_add(u32::from(*count)));
        if total == 0 {
            return growth_totals;
        }
        for (category, count) in counts.into_iter().enumerate() {
            let save_category = category + SAVE_CATEGORY_OFFSET;
            let Some(saved) = self
                .save_experience
                .get(save_category)
                .and_then(|roles| roles.get(role_index))
                .copied()
            else {
                continue;
            };
            let mut experience = gained
                .saturating_mul(u32::from(count))
                .checked_div(total)
                .unwrap_or(0)
                .saturating_mul(2)
                .saturating_add(u32::from(saved.experience));
            let mut level = saved.level.min(99);
            while let Some(required) = self
                .battle_data
                .as_ref()
                .and_then(|data| data.level_up_experience.for_level(level))
                .map(u32::from)
                .filter(|required| *required > 0)
            {
                if experience < required {
                    break;
                }
                experience -= required;
                let growth = u16::try_from(1 + self.growth_random(2)).unwrap_or(1);
                if !self.apply_hidden_battle_growth(player.role_id, category, growth) {
                    break;
                }
                growth_totals[category] = growth_totals[category].wrapping_add(growth);
                if level < 99 {
                    level += 1;
                }
            }
            if let Some(saved) = self
                .save_experience
                .get_mut(save_category)
                .and_then(|roles| roles.get_mut(role_index))
            {
                saved.experience = u16::try_from(experience).unwrap_or(u16::MAX);
                saved.level = level;
                saved.count = 0;
            }
        }
        growth_totals
    }

    pub(super) fn apply_hidden_battle_growth(
        &mut self,
        role_id: u16,
        category: usize,
        growth: u16,
    ) -> bool {
        let role_index = usize::from(role_id);
        let Some(role) = self
            .player_roles
            .as_mut()
            .and_then(|roles| roles.role_mut(role_index))
        else {
            return false;
        };
        let attribute = match category {
            HIDDEN_EXP_HEALTH => &mut role.max_hp,
            HIDDEN_EXP_MAGIC => &mut role.max_mp,
            HIDDEN_EXP_ATTACK => &mut role.attack_strength,
            HIDDEN_EXP_MAGIC_POWER => &mut role.magic_strength,
            HIDDEN_EXP_DEFENSE => &mut role.defense,
            HIDDEN_EXP_DEXTERITY => &mut role.dexterity,
            HIDDEN_EXP_FLEE => &mut role.flee_rate,
            _ => return false,
        };
        *attribute = attribute.wrapping_add(growth);
        if let Some(player) = self.active_battle.as_mut().and_then(|battle| {
            battle
                .players
                .iter_mut()
                .find(|player| player.role_id == role_id)
        }) {
            let attribute = match category {
                HIDDEN_EXP_HEALTH => &mut player.max_hp,
                HIDDEN_EXP_MAGIC => &mut player.max_mp,
                HIDDEN_EXP_ATTACK => &mut player.attack_strength,
                HIDDEN_EXP_MAGIC_POWER => &mut player.magic_strength,
                HIDDEN_EXP_DEFENSE => &mut player.defense,
                HIDDEN_EXP_DEXTERITY => &mut player.dexterity,
                HIDDEN_EXP_FLEE => &mut player.flee_rate,
                _ => return false,
            };
            *attribute = attribute.wrapping_add(growth);
        }
        true
    }

    pub(super) fn restore_role_after_battle_level_up(&mut self, role_id: u16) -> Option<()> {
        let role_index = usize::from(role_id);
        let role = self.player_roles.as_mut()?.role_mut(role_index)?;
        role.hp = role.max_hp;
        role.mp = role.max_mp;
        let (hp, max_hp, mp, max_mp) = (role.hp, role.max_hp, role.mp, role.max_mp);
        if let Some(player) = self.active_battle.as_mut().and_then(|battle| {
            battle
                .players
                .iter_mut()
                .find(|player| player.role_id == role_id)
        }) {
            player.hp = hp;
            player.max_hp = max_hp;
            player.mp = mp;
            player.max_mp = max_mp;
        }
        Some(())
    }

    pub(super) fn increase_player_level(&mut self, role_id: u16, levels: u16) -> bool {
        let role_index = usize::from(role_id);
        if self
            .player_roles
            .as_ref()
            .and_then(|roles| roles.role(role_index))
            .is_none()
        {
            return false;
        }
        let mut max_hp = 0u16;
        let mut max_mp = 0u16;
        let mut attack = 0u16;
        let mut magic = 0u16;
        let mut defense = 0u16;
        let mut dexterity = 0u16;
        for _ in 0..levels {
            max_hp = max_hp.saturating_add((10 + self.growth_random(8)) as u16);
            max_mp = max_mp.saturating_add((8 + self.growth_random(6)) as u16);
            attack = attack.saturating_add((4 + self.growth_random(2)) as u16);
            magic = magic.saturating_add((4 + self.growth_random(2)) as u16);
            defense = defense.saturating_add((2 + self.growth_random(2)) as u16);
            dexterity = dexterity.saturating_add((2 + self.growth_random(2)) as u16);
        }
        let Some(roles) = self.player_roles.as_mut() else {
            return false;
        };
        let Some(role) = roles.role_mut(role_index) else {
            return false;
        };
        role.level = role.level.saturating_add(levels).min(99);
        role.max_hp = role.max_hp.saturating_add(max_hp).min(999);
        role.max_mp = role.max_mp.saturating_add(max_mp).min(999);
        role.attack_strength = role.attack_strength.saturating_add(attack).min(999);
        role.magic_strength = role.magic_strength.saturating_add(magic).min(999);
        role.defense = role.defense.saturating_add(defense).min(999);
        role.dexterity = role.dexterity.saturating_add(dexterity).min(999);
        role.flee_rate = role
            .flee_rate
            .saturating_add(levels.saturating_mul(2))
            .min(999);
        let updated = role.clone();
        self.role_experience[role_index] = 0;
        self.party.sync_from_roles(roles);

        if let Some(player) = self.active_battle.as_mut().and_then(|battle| {
            battle
                .players
                .iter_mut()
                .find(|player| player.role_id == role_id)
        }) {
            player.level = updated.level;
            player.max_hp = player.max_hp.saturating_add(max_hp).min(999);
            player.max_mp = player.max_mp.saturating_add(max_mp).min(999);
            player.attack_strength = player.attack_strength.saturating_add(attack).min(999);
            player.magic_strength = player.magic_strength.saturating_add(magic).min(999);
            player.defense = player.defense.saturating_add(defense).min(999);
            player.dexterity = player.dexterity.saturating_add(dexterity).min(999);
            player.flee_rate = player
                .flee_rate
                .saturating_add(levels.saturating_mul(2))
                .min(999);
        }
        true
    }

    pub(super) fn level_up_role(&mut self, role_id: u16) -> bool {
        let role_index = usize::from(role_id);
        let hp = 10 + self.growth_random(8);
        let mp = 8 + self.growth_random(6);
        let attack = 4 + self.growth_random(2);
        let magic = 4 + self.growth_random(2);
        let defense = 2 + self.growth_random(2);
        let dexterity = 2 + self.growth_random(2);
        let Some(roles) = self.player_roles.as_mut() else {
            return false;
        };
        let Some(role) = roles.role_mut(role_index) else {
            return false;
        };
        role.level = role.level.saturating_add(1).min(99);
        role.max_hp = role.max_hp.saturating_add(hp as u16).min(999);
        role.max_mp = role.max_mp.saturating_add(mp as u16).min(999);
        role.attack_strength = role.attack_strength.saturating_add(attack as u16).min(999);
        role.magic_strength = role.magic_strength.saturating_add(magic as u16).min(999);
        role.defense = role.defense.saturating_add(defense as u16).min(999);
        role.dexterity = role.dexterity.saturating_add(dexterity as u16).min(999);
        role.flee_rate = role.flee_rate.saturating_add(2).min(999);
        role.hp = role.max_hp;
        role.mp = role.max_mp;
        self.party.sync_from_roles(roles);
        true
    }

    pub(super) fn learn_eligible_magics_for_role(&mut self, role_id: u16) -> Vec<u16> {
        let role_index = usize::from(role_id);
        if role_index >= pal_assets::battle::LEVEL_UP_ROLE_COUNT {
            return Vec::new();
        }
        let Some(level) = self
            .player_roles
            .as_ref()
            .and_then(|roles| roles.role(role_index))
            .map(|role| role.level)
        else {
            return Vec::new();
        };
        let Some(data) = self.battle_data.as_ref() else {
            return Vec::new();
        };
        let eligible = data
            .level_up_magics
            .iter()
            .map(|set| set.roles[role_index])
            .filter(|entry| entry.magic != 0 && entry.level <= level)
            .map(|entry| entry.magic)
            .collect::<Vec<_>>();
        let Some(roles) = self.player_roles.as_mut() else {
            return Vec::new();
        };
        let Some(role) = roles.role_mut(role_index) else {
            return Vec::new();
        };
        let mut learned = Vec::new();
        for magic in eligible {
            if role.magic.contains(&magic) {
                continue;
            }
            let Some(slot) = role.magic.iter_mut().find(|slot| **slot == 0) else {
                break;
            };
            *slot = magic;
            learned.push(magic);
        }
        self.party.sync_from_roles(roles);
        learned
    }

    pub(super) fn growth_random(&mut self, upper_exclusive: u32) -> u32 {
        if upper_exclusive <= 1 {
            return 0;
        }
        if let Some(battle) = self.active_battle.as_mut() {
            let state = battle.random_state();
            let mut state = state;
            let value = random::random_long(&mut state, 0, upper_exclusive - 1);
            battle.set_random_state(state);
            value
        } else {
            random::random_long(&mut self.growth_random_state, 0, upper_exclusive - 1)
        }
    }
}
