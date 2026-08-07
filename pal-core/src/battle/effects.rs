//! Action execution, damage, rewards, and script-driven combat effects.

use pal_assets::magic::Magics;
use pal_assets::objects::GlobalObjects;

use super::helpers::{
    battle_magic, battle_player_stat_mut, classic_player_magic_damage, next_player, physical_damage,
};
use super::types::{
    cure_poison, BattleActorAction, BattleEnemy, BattleEvent, BattleFlow, BattleMagic, BattlePhase,
    BattlePlayer, BattleResult, BattleRewards, BattleScriptRequest, BattleScriptSource,
    BattleState, BattleStatus, BattleSteal, BattleTarget, MagicEventPhase, PlayerAction,
    PlayerItemKind, PlayerItemPhase, PlayerMagicPhase, QueuedBattleAction, VictorySettlementStage,
    HIDDEN_EXPERIENCE_CATEGORY_COUNT, HIDDEN_EXP_ATTACK, HIDDEN_EXP_DEFENSE, HIDDEN_EXP_FLEE,
    HIDDEN_EXP_HEALTH, HIDDEN_EXP_MAGIC, HIDDEN_EXP_MAGIC_POWER,
};
use crate::random;

impl BattleState {
    pub(super) fn perform_enemy_action(
        &mut self,
        enemy: usize,
        player: usize,
    ) -> Option<BattleEvent> {
        if !self.enemies.get(enemy)?.can_act() {
            return None;
        }
        if !self.players.get(player)?.is_alive() {
            return None;
        }
        let mut auto_defended = self.random(17) >= 10;
        let bad_status = self.players[player]
            .statuses
            .is_active(BattleStatus::Confused)
            || self.players[player].statuses.is_active(BattleStatus::Sleep)
            || self.players[player]
                .statuses
                .is_active(BattleStatus::Paralyzed);
        let protected_by = if (self.players[player].is_dying() || bad_status) && auto_defended {
            let covered_by = self.players[player].covered_by;
            self.players.iter().position(|candidate| {
                candidate.role_id == covered_by
                    && !candidate.is_dying()
                    && !candidate.statuses.is_active(BattleStatus::Confused)
                    && !candidate.statuses.is_active(BattleStatus::Sleep)
                    && !candidate.statuses.is_active(BattleStatus::Paralyzed)
            })
        } else {
            None
        };
        if protected_by.is_none() && bad_status {
            auto_defended = false;
        }
        let damage = if auto_defended {
            0
        } else {
            self.enemy_damage(enemy, player)
        }
        .min(self.players[player].hp);
        let target = &mut self.players[player];
        target.hp = target.hp.saturating_sub(damage);
        Some(BattleEvent::EnemyAttack {
            enemy,
            player,
            damage,
            protected_by,
            auto_defended,
            defeated: !target.is_alive(),
        })
    }

    /// Classic samples party slots until it reaches a living target, consuming a roll for
    /// every dead slot encountered rather than sampling a compacted list.
    pub(super) fn random_living_player(&mut self) -> Option<usize> {
        if self.players.iter().all(|player| !player.is_alive()) {
            return None;
        }
        loop {
            let player = self.random(self.players.len() as u32) as usize;
            if self.players.get(player).is_some_and(BattlePlayer::is_alive) {
                return Some(player);
            }
        }
    }

    pub(super) fn perform_confused_enemy_action(&mut self, enemy: usize) -> Option<BattleEvent> {
        let living_enemies = self
            .enemies
            .iter()
            .enumerate()
            .filter_map(|(index, actor)| actor.is_alive().then_some(index))
            .collect::<Vec<_>>();
        let target = living_enemies
            .get(self.random(living_enemies.len() as u32) as usize)
            .copied()?;
        if target == enemy {
            return None;
        }
        let damage = self.confused_enemy_damage(enemy, target);
        let actor = &mut self.enemies[target];
        actor.hp = actor.hp.wrapping_sub(damage);
        Some(BattleEvent::EnemyConfusedAttack {
            enemy,
            target,
            damage,
            defeated: !actor.is_alive(),
        })
    }

    pub(super) fn perform_enemy_magic(
        &mut self,
        enemy: usize,
        target: usize,
        magic: BattleMagic,
    ) -> Vec<BattleEvent> {
        let mut blow = self.magic_blow;
        if magic.base_damage as i16 <= 0 {
            return Vec::new();
        }
        let targets = if magic.attacks_all {
            self.players
                .iter()
                .enumerate()
                .filter_map(|(index, player)| player.is_alive().then_some(index))
                .collect::<Vec<_>>()
        } else if self.players.get(target).is_some_and(BattlePlayer::is_alive) {
            vec![target]
        } else {
            Vec::new()
        };
        let mut events = Vec::with_capacity(targets.len());
        let mut visual = true;
        for player in targets {
            let auto_defended = !self.players[player].statuses.is_active(BattleStatus::Sleep)
                && !self.players[player]
                    .statuses
                    .is_active(BattleStatus::Paralyzed)
                && !self.players[player]
                    .statuses
                    .is_active(BattleStatus::Confused)
                && self.random(3) == 0;
            let damage = self
                .enemy_magic_damage(enemy, player, magic, auto_defended)
                .min(self.players[player].hp);
            let target = &mut self.players[player];
            target.hp = target.hp.saturating_sub(damage);
            events.push(BattleEvent::EnemyMagic {
                enemy,
                player,
                magic_object: magic.object_id,
                blow,
                damage,
                phase: MagicEventPhase::Feedback,
                visual,
                auto_defended,
                defeated: !target.is_alive(),
            });
            blow = 0;
            visual = false;
        }
        events
    }

    pub(super) fn commit_player_action(&mut self, player_index: usize, action: PlayerAction) {
        self.commit_player_action_with_auto_attack(player_index, action, false);
    }

    pub(super) fn commit_player_action_with_auto_attack(
        &mut self,
        player_index: usize,
        action: PlayerAction,
        automatic_attack: bool,
    ) {
        self.acted[player_index] = true;
        self.player_actions[player_index] = Some(action);
        self.automatic_player_attacks[player_index] = automatic_attack;
        self.active_player = next_player(&self.players, &self.acted, player_index + 1);
        if self.active_player.is_some() {
            return;
        }
        self.build_action_queue();
    }

    pub(super) fn build_action_queue(&mut self) {
        if !self.repeating_round {
            self.previous_player_actions
                .clone_from(&self.player_actions);
            self.previous_automatic_player_attacks
                .clone_from(&self.automatic_player_attacks);
            self.previous_auto_attack = self.auto_attack_mode;
        }
        self.repeating_round = false;
        self.execution_auto_attack = false;

        let mut queue = Vec::new();
        for enemy in 0..self.enemies.len() {
            if !self.enemies[enemy].is_alive() {
                continue;
            }
            let actions = if self.enemies[enemy].dual_move { 2 } else { 1 };
            for _ in 0..actions {
                let dexterity = self.enemy_action_dexterity(enemy);
                queue.push(QueuedBattleAction {
                    action: BattleActorAction::Enemy { enemy },
                    dexterity,
                });
            }
        }
        for player in 0..self.players.len() {
            let disabled = !self.players[player].is_alive()
                || self.players[player].statuses.is_active(BattleStatus::Sleep)
                || self.players[player]
                    .statuses
                    .is_active(BattleStatus::Paralyzed);
            let action = if disabled {
                PlayerAction::Attack {
                    target: self.first_living_enemy().unwrap_or_default(),
                }
            } else if self.players[player]
                .statuses
                .is_active(BattleStatus::Confused)
            {
                PlayerAction::AttackMate
            } else if let Some(action) = self.player_actions[player] {
                action
            } else {
                PlayerAction::Attack {
                    target: self.first_living_enemy().unwrap_or_default(),
                }
            };
            let dexterity = if disabled {
                0
            } else {
                self.player_action_dexterity(player, action)
            };
            queue.push(QueuedBattleAction {
                action: BattleActorAction::Player { player, action },
                dexterity,
            });
        }
        queue.sort_by_key(|queued| std::cmp::Reverse(queued.dexterity));
        self.action_queue = queue;
        self.action_index = 0;
        self.active_player = None;
        self.flow = BattleFlow::PerformActions;
    }

    pub(super) fn propagate_execution_auto_attack(
        &mut self,
        mut queued: QueuedBattleAction,
    ) -> QueuedBattleAction {
        let BattleActorAction::Player { player, action } = queued.action else {
            return queued;
        };
        let can_propagate = self.players.get(player).is_some_and(|actor| {
            actor.is_alive()
                && !actor.statuses.is_active(BattleStatus::Sleep)
                && !actor.statuses.is_active(BattleStatus::Paralyzed)
                && !actor.statuses.is_active(BattleStatus::Confused)
        });
        if !can_propagate {
            return queued;
        }
        if matches!(action, PlayerAction::Attack { .. })
            && self
                .automatic_player_attacks
                .get(player)
                .copied()
                .unwrap_or(false)
        {
            self.execution_auto_attack = true;
            return queued;
        }
        if !self.execution_auto_attack {
            return queued;
        }

        let target = match action {
            PlayerAction::Attack { target } => target,
            PlayerAction::Magic {
                target: BattleTarget::Enemy(target) | BattleTarget::Player(target),
                ..
            }
            | PlayerAction::CooperativeMagic {
                target: BattleTarget::Enemy(target) | BattleTarget::Player(target),
            }
            | PlayerAction::UseItem {
                target: Some(target),
                ..
            }
            | PlayerAction::ThrowItem {
                target: Some(target),
                ..
            } => target,
            PlayerAction::Magic { .. }
            | PlayerAction::CooperativeMagic { .. }
            | PlayerAction::UseItem { .. }
            | PlayerAction::ThrowItem { .. }
            | PlayerAction::Flee
            | PlayerAction::Defend
            | PlayerAction::AttackMate => self.first_living_enemy().unwrap_or_default(),
        };
        let action = PlayerAction::Attack { target };
        queued.action = BattleActorAction::Player { player, action };
        self.player_actions[player] = Some(action);
        self.automatic_player_attacks[player] = true;
        queued
    }

    /// Refresh the inventory view used by Classic's execution-time item validation.
    pub(crate) fn set_inventory_amounts(&mut self, inventory: &[(u16, u16)]) {
        self.inventory_amounts = Some(inventory.to_vec());
    }

    pub(super) fn validate_queued_item(
        &mut self,
        mut queued: QueuedBattleAction,
    ) -> QueuedBattleAction {
        let Some(inventory) = self.inventory_amounts.as_ref() else {
            return queued;
        };
        let default_target = self.first_living_enemy().unwrap_or_default();
        let BattleActorAction::Player { player, action } = queued.action else {
            return queued;
        };
        let available = |item_object| {
            inventory
                .iter()
                .find_map(|&(id, amount)| (id == item_object).then_some(amount))
                .unwrap_or(0)
                > 0
        };
        let fallback = match action {
            PlayerAction::UseItem { item_object, .. } if !available(item_object) => {
                Some(PlayerAction::Defend)
            }
            PlayerAction::ThrowItem { item_object, .. } if !available(item_object) => {
                Some(PlayerAction::Attack {
                    target: default_target,
                })
            }
            _ => None,
        };
        if let Some(action) = fallback {
            queued.action = BattleActorAction::Player { player, action };
            self.player_actions[player] = Some(action);
            self.automatic_player_attacks[player] = false;
        }
        queued
    }

    pub(super) fn validate_queued_target(
        &mut self,
        mut queued: QueuedBattleAction,
    ) -> QueuedBattleAction {
        let BattleActorAction::Player { player, action } = queued.action else {
            return queued;
        };
        let retarget = |battle: &Self, target: usize| {
            battle
                .enemies
                .get(target)
                .filter(|enemy| enemy.is_alive())
                .map(|_| target)
                .or_else(|| battle.living_enemy_from(target))
        };
        let action = match action {
            PlayerAction::Attack { target } => retarget(self, target)
                .map(|target| PlayerAction::Attack { target })
                .unwrap_or(action),
            PlayerAction::Magic {
                magic,
                target: BattleTarget::Enemy(target),
            } => retarget(self, target)
                .map(|target| PlayerAction::Magic {
                    magic,
                    target: BattleTarget::Enemy(target),
                })
                .unwrap_or(action),
            PlayerAction::CooperativeMagic {
                target: BattleTarget::Enemy(target),
            } => retarget(self, target)
                .map(|target| PlayerAction::CooperativeMagic {
                    target: BattleTarget::Enemy(target),
                })
                .unwrap_or(action),
            PlayerAction::ThrowItem {
                item_object,
                target: Some(target),
                script_entry,
            } => retarget(self, target)
                .map(|target| PlayerAction::ThrowItem {
                    item_object,
                    target: Some(target),
                    script_entry,
                })
                .unwrap_or(action),
            PlayerAction::Magic { .. }
            | PlayerAction::CooperativeMagic { .. }
            | PlayerAction::UseItem { .. }
            | PlayerAction::ThrowItem { target: None, .. }
            | PlayerAction::Flee
            | PlayerAction::Defend
            | PlayerAction::AttackMate => action,
        };
        queued.action = BattleActorAction::Player { player, action };
        self.player_actions[player] = Some(action);
        queued
    }

    fn enemy_action_dexterity(&mut self, enemy: usize) -> i32 {
        let actor = &self.enemies[enemy];
        let base = i32::from(actor.dexterity as i16)
            .saturating_add(i32::from(actor.level.saturating_add(6)) * 3);
        self.jitter_dexterity(base)
    }

    fn player_action_dexterity(&mut self, player: usize, action: PlayerAction) -> i32 {
        let actor = &self.players[player];
        let mut dexterity = u32::from(actor.dexterity);
        if actor.statuses.is_active(BattleStatus::Haste) {
            dexterity = dexterity.saturating_mul(3);
        }
        dexterity = dexterity.min(999);
        dexterity = match action {
            PlayerAction::Defend => dexterity.saturating_mul(5),
            PlayerAction::CooperativeMagic { .. } => dexterity.saturating_mul(10),
            PlayerAction::Magic { magic, .. }
                if self.players[player]
                    .magics
                    .get(magic)
                    .is_some_and(|magic| !magic.usable_to_enemy()) =>
            {
                dexterity.saturating_mul(3)
            }
            PlayerAction::UseItem { .. } => dexterity.saturating_mul(3),
            PlayerAction::Flee => dexterity / 2,
            PlayerAction::Attack { .. }
            | PlayerAction::Magic { .. }
            | PlayerAction::ThrowItem { .. }
            | PlayerAction::AttackMate => dexterity,
        };
        if actor.is_dying() {
            dexterity /= 2;
        }
        self.jitter_dexterity(i32::try_from(dexterity).unwrap_or(i32::MAX))
    }

    pub(super) fn jitter_dexterity(&mut self, dexterity: i32) -> i32 {
        (dexterity as f32 * self.random_float(0.9, 1.1)) as i32
    }

    pub(super) fn begin_player_magic(
        &mut self,
        player: usize,
        magic: usize,
        target: BattleTarget,
    ) -> bool {
        let Some(actor) = self.players.get(player) else {
            return false;
        };
        if !actor.can_act() || actor.statuses.is_active(BattleStatus::Confused) {
            return false;
        }
        let Some(spell) = actor.magics.get(magic).copied() else {
            return false;
        };
        if actor.statuses.is_active(BattleStatus::Silence)
            || actor.mp < spell.mp_cost
            || !self.magic_target_is_valid(spell, target)
        {
            return false;
        }
        self.magic_blow = 0;
        if !self.auto_battle {
            self.players[player].mp -= spell.mp_cost;
        }
        self.flow = BattleFlow::PlayerMagic {
            player,
            target,
            magic: spell,
            phase: PlayerMagicPhase::UseScript,
            use_succeeded: true,
        };
        if spell.use_script != 0 {
            self.pending_scripts.push_back(BattleScriptRequest {
                source: BattleScriptSource::PlayerMagicUse {
                    player,
                    magic_object: spell.object_id,
                },
                entry: spell.use_script,
                object_id: self.players[player].role_id,
            });
        }
        true
    }

    pub(super) fn magic_success_owner(&self, target: BattleTarget) -> u16 {
        match target {
            BattleTarget::Enemy(enemy) => self.enemy_slot_for_index(enemy).unwrap_or(u16::MAX),
            BattleTarget::Player(player) => self
                .players
                .get(player)
                .map_or(u16::MAX, |actor| actor.role_id),
            BattleTarget::AllEnemies | BattleTarget::AllPlayers => u16::MAX,
        }
    }

    pub(super) fn begin_player_item(&mut self, player: usize, action: PlayerAction) -> bool {
        let Some(actor) = self.players.get(player) else {
            return false;
        };
        if !actor.can_act() || actor.statuses.is_active(BattleStatus::Confused) {
            return false;
        }
        self.magic_blow = 0;
        let (item_object, target, script_entry, kind, object_id) = match action {
            PlayerAction::UseItem {
                item_object,
                target,
                script_entry,
                consuming,
            } => {
                let object_id = match target {
                    Some(target) => {
                        let Some(target) = self.players.get(target) else {
                            return false;
                        };
                        target.role_id
                    }
                    None => u16::MAX,
                };
                (
                    item_object,
                    target,
                    script_entry,
                    PlayerItemKind::Use { consuming },
                    object_id,
                )
            }
            PlayerAction::ThrowItem {
                item_object,
                target,
                script_entry,
            } => {
                let target_required = target.is_some();
                let target = match target {
                    Some(target) if self.enemies.get(target).is_some_and(BattleEnemy::is_alive) => {
                        Some(target)
                    }
                    Some(_) => self.first_living_enemy(),
                    None => None,
                };
                if target_required && target.is_none() {
                    return false;
                }
                (
                    item_object,
                    target,
                    script_entry,
                    PlayerItemKind::Throw,
                    target
                        .and_then(|target| self.enemy_slot_for_index(target))
                        .unwrap_or(u16::MAX),
                )
            }
            PlayerAction::Attack { .. }
            | PlayerAction::Magic { .. }
            | PlayerAction::CooperativeMagic { .. }
            | PlayerAction::Flee
            | PlayerAction::Defend
            | PlayerAction::AttackMate => {
                return false;
            }
        };
        self.flow = BattleFlow::PlayerItem {
            player,
            item_object,
            target,
            kind,
            script_entry,
            object_id,
            phase: PlayerItemPhase::Animation,
        };
        let event = match kind {
            PlayerItemKind::Use { consuming } => BattleEvent::PlayerUseItem {
                player,
                item_object,
                target,
                consuming,
            },
            PlayerItemKind::Throw => BattleEvent::PlayerThrowItem {
                player,
                item_object,
                target,
            },
        };
        self.pending_events.push_back(event);
        true
    }

    pub(super) fn finish_player_item(&mut self) {
        let BattleFlow::PlayerItem {
            player,
            item_object,
            kind,
            ..
        } = self.flow
        else {
            return;
        };
        self.pending_events
            .push_front(BattleEvent::PlayerItemFeedback {
                player,
                item_object,
                consume: matches!(
                    kind,
                    PlayerItemKind::Throw | PlayerItemKind::Use { consuming: true }
                ),
            });
        self.flow = BattleFlow::PerformActions;
        self.queue_post_action_check(false);
    }

    pub(super) fn perform_player_action(
        &mut self,
        player: usize,
        action: PlayerAction,
    ) -> Vec<BattleEvent> {
        if !self.players.get(player).is_some_and(BattlePlayer::can_act) {
            return Vec::new();
        }
        if self.cooperative_magic_performed
            && !matches!(action, PlayerAction::CooperativeMagic { .. })
        {
            return Vec::new();
        }
        if self.players[player]
            .statuses
            .is_active(BattleStatus::Confused)
        {
            return self
                .perform_confused_player_action(player)
                .into_iter()
                .collect();
        }
        match action {
            PlayerAction::Attack { target } => self.perform_player_attack(player, target),
            PlayerAction::Magic { magic, target } => {
                let Some(spell) = self.players[player].magics.get(magic).copied() else {
                    return self.perform_player_attack(
                        player,
                        self.first_living_enemy().unwrap_or_default(),
                    );
                };
                if self.players[player]
                    .statuses
                    .is_active(BattleStatus::Silence)
                    || self.players[player].mp < spell.mp_cost
                    || !self.magic_target_is_valid(spell, target)
                {
                    if spell.usable_to_enemy() {
                        return self.perform_player_attack(
                            player,
                            match target {
                                BattleTarget::Enemy(enemy) => enemy,
                                BattleTarget::Player(_)
                                | BattleTarget::AllEnemies
                                | BattleTarget::AllPlayers => {
                                    self.first_living_enemy().unwrap_or_default()
                                }
                            },
                        );
                    }
                    self.perform_player_defend(player);
                    return Vec::new();
                }
                if !self.auto_battle {
                    self.players[player].mp -= spell.mp_cost;
                }
                self.perform_player_magic(player, target, spell)
            }
            PlayerAction::CooperativeMagic { target } => {
                self.perform_cooperative_magic(player, target)
            }
            PlayerAction::UseItem { .. } | PlayerAction::ThrowItem { .. } => Vec::new(),
            PlayerAction::Flee => vec![self.perform_player_flee(player)],
            PlayerAction::Defend => {
                self.perform_player_defend(player);
                Vec::new()
            }
            PlayerAction::AttackMate => self
                .perform_confused_player_action(player)
                .into_iter()
                .collect(),
        }
    }

    fn perform_player_defend(&mut self, player: usize) {
        self.players[player].defending = true;
        self.add_hidden_experience(player, HIDDEN_EXP_DEFENSE, 2);
    }

    fn perform_player_attack(&mut self, player: usize, target: usize) -> Vec<BattleEvent> {
        let mut events = Vec::new();
        let attack_count = if self.players[player]
            .statuses
            .is_active(BattleStatus::DualAttack)
        {
            2
        } else {
            1
        };
        for _ in 0..attack_count {
            let attacks_all = self.players[player].attacks_all;
            let critical = attacks_all
                && (self.random(6) == 0
                    || self.players[player]
                        .statuses
                        .is_active(BattleStatus::Bravery));
            let targets = if attacks_all {
                const SLOT_ORDER: [usize; 5] = [2, 1, 0, 4, 3];
                SLOT_ORDER
                    .into_iter()
                    .flat_map(|slot| {
                        self.enemies
                            .iter()
                            .enumerate()
                            .filter_map(move |(index, enemy)| {
                                (enemy.slot == slot && enemy.is_alive()).then_some(index)
                            })
                    })
                    .collect::<Vec<_>>()
            } else if self.enemies.get(target).is_some_and(BattleEnemy::is_alive) {
                vec![target]
            } else if let Some(target) = self.first_living_enemy() {
                vec![target]
            } else {
                Vec::new()
            };
            let mut division = 1u32;
            let mut visual = true;
            for enemy in targets {
                let (damage, event_critical) = if attacks_all {
                    let damage = self.player_attack_all_damage(player, enemy, critical, division);
                    division = division.saturating_mul(2);
                    (damage, critical)
                } else {
                    self.player_single_attack_damage(player, enemy)
                };
                let target = &mut self.enemies[enemy];
                target.hp = target.hp.wrapping_sub(damage);
                events.push(BattleEvent::PlayerAttack {
                    player,
                    enemy,
                    damage,
                    critical: event_critical,
                    visual,
                    defeated: !target.is_alive(),
                });
                visual = false;
            }
        }
        self.add_hidden_experience(player, HIDDEN_EXP_ATTACK, 1);
        let health = 2 + u16::try_from(self.random(2)).unwrap_or(0);
        self.add_hidden_experience(player, HIDDEN_EXP_HEALTH, health);
        events
    }

    fn perform_player_flee(&mut self, player: usize) -> BattleEvent {
        let defense =
            self.enemies
                .iter()
                .filter(|enemy| enemy.is_alive())
                .fold(0u16, |total, enemy| {
                    total
                        .wrapping_add(enemy.dexterity)
                        .wrapping_add(enemy.level.saturating_add(6).wrapping_mul(4))
                });
        let defense = u32::from(if defense as i16 >= 0 { defense } else { 0 });
        let succeeded = !self.is_boss
            && u32::from(self.players[player].flee_rate) >= self.random(defense.saturating_add(1));
        if succeeded {
            self.phase = BattlePhase::Finished(BattleResult::Fled);
            self.flow = BattleFlow::Finished;
            self.active_player = None;
            self.pending_scripts.clear();
            self.active_script = None;
        } else {
            self.add_hidden_experience(player, HIDDEN_EXP_FLEE, 2);
        }
        BattleEvent::PlayerFlee { player, succeeded }
    }

    pub(super) fn record_magic_experience(&mut self, player: usize) {
        let magic = 2 + u16::try_from(self.random(2)).unwrap_or(0);
        self.add_hidden_experience(player, HIDDEN_EXP_MAGIC, magic);
        self.add_hidden_experience(player, HIDDEN_EXP_MAGIC_POWER, 1);
    }

    fn add_hidden_experience(&mut self, player: usize, category: usize, amount: u16) {
        if let Some(count) = self
            .hidden_experience_counts
            .get_mut(player)
            .and_then(|counts| counts.get_mut(category))
        {
            *count = count.saturating_add(amount);
        }
    }

    pub(super) fn perform_player_magic(
        &mut self,
        player: usize,
        target: BattleTarget,
        magic: BattleMagic,
    ) -> Vec<BattleEvent> {
        if !magic.usable_to_enemy() {
            return Vec::new();
        }
        if magic.base_damage as i16 <= 0 {
            return Vec::new();
        }
        let mut blow = self.magic_blow;
        let targets = match target {
            BattleTarget::AllEnemies => self
                .enemies
                .iter()
                .enumerate()
                .filter_map(|(index, enemy)| enemy.is_alive().then_some(index))
                .collect::<Vec<_>>(),
            BattleTarget::Enemy(target)
                if self.enemies.get(target).is_some_and(BattleEnemy::is_alive) =>
            {
                vec![target]
            }
            BattleTarget::Enemy(_) => self.first_living_enemy().into_iter().collect(),
            BattleTarget::Player(_) | BattleTarget::AllPlayers => Vec::new(),
        };
        let mut events = Vec::new();
        let mut visual = true;
        for enemy in targets {
            let damage = self.magic_damage(player, enemy, magic);
            let target = &mut self.enemies[enemy];
            target.hp = target.hp.wrapping_sub(damage);
            events.push(BattleEvent::PlayerMagic {
                player,
                enemy,
                magic_object: magic.object_id,
                blow,
                damage,
                phase: MagicEventPhase::Feedback,
                visual,
                defeated: !target.is_alive(),
            });
            blow = 0;
            visual = false;
        }
        events
    }

    pub(super) fn player_magic_visual_event(
        &self,
        player: usize,
        target: BattleTarget,
        magic: BattleMagic,
    ) -> Option<BattleEvent> {
        if !magic.usable_to_enemy() {
            return Some(BattleEvent::PlayerDefensiveMagic {
                player,
                target,
                magic_object: magic.object_id,
            });
        }
        let enemy = match target {
            BattleTarget::Enemy(enemy)
                if self.enemies.get(enemy).is_some_and(BattleEnemy::is_alive) =>
            {
                enemy
            }
            BattleTarget::Enemy(_) | BattleTarget::AllEnemies => self.first_living_enemy()?,
            BattleTarget::Player(_) | BattleTarget::AllPlayers => return None,
        };
        Some(BattleEvent::PlayerMagic {
            player,
            enemy,
            magic_object: magic.object_id,
            blow: 0,
            damage: 0,
            phase: MagicEventPhase::Visual,
            visual: true,
            defeated: false,
        })
    }

    fn perform_cooperative_magic(
        &mut self,
        player: usize,
        target: BattleTarget,
    ) -> Vec<BattleEvent> {
        let contributors = self
            .players
            .iter()
            .enumerate()
            .filter_map(|(index, actor)| Self::coop_healthy(actor).then_some(index))
            .collect::<Vec<_>>();
        let Some(magic) = self.players[player].cooperative_magic else {
            return self
                .perform_player_attack(player, self.first_living_enemy().unwrap_or_default());
        };
        if contributors.len() <= 1
            || !contributors.contains(&player)
            || !self.magic_target_is_valid(magic, target)
        {
            return self
                .perform_player_attack(player, self.first_living_enemy().unwrap_or_default());
        }
        self.cooperative_magic_performed = true;
        let strength = contributors.iter().fold(0u32, |total, &contributor| {
            total
                .saturating_add(u32::from(self.players[contributor].attack_strength))
                .saturating_add(u32::from(self.players[contributor].magic_strength))
        }) / 4;
        for &contributor in &contributors {
            self.players[contributor].hp = self.players[contributor]
                .hp
                .saturating_sub(magic.mp_cost)
                .max(1);
        }
        let targets = match target {
            BattleTarget::AllEnemies => self
                .enemies
                .iter()
                .enumerate()
                .filter_map(|(index, enemy)| enemy.is_alive().then_some(index))
                .collect::<Vec<_>>(),
            BattleTarget::Enemy(enemy)
                if self.enemies.get(enemy).is_some_and(BattleEnemy::is_alive) =>
            {
                vec![enemy]
            }
            BattleTarget::Enemy(_) => self.first_living_enemy().into_iter().collect(),
            BattleTarget::Player(_) | BattleTarget::AllPlayers => Vec::new(),
        };
        let base_strength = u16::try_from(strength).unwrap_or(u16::MAX);
        let mut events = Vec::with_capacity(targets.len());
        let mut visual = true;
        for enemy in targets {
            let damage = self
                .player_magic_damage_from_strength(enemy, magic, u32::from(base_strength), false)
                .max(1) as u16;
            let target = &mut self.enemies[enemy];
            target.hp = target.hp.wrapping_sub(damage);
            events.push(BattleEvent::PlayerCooperativeMagic {
                player,
                enemy,
                magic_object: magic.object_id,
                damage,
                visual,
                defeated: !target.is_alive(),
            });
            visual = false;
        }
        events
    }

    fn perform_confused_player_action(&mut self, player: usize) -> Option<BattleEvent> {
        if self.players.get(player)?.is_dying() {
            return None;
        }
        let targets = self
            .players
            .iter()
            .enumerate()
            .filter_map(|(index, actor)| (index != player && actor.is_alive()).then_some(index))
            .collect::<Vec<_>>();
        let target = targets
            .get(self.random(targets.len() as u32) as usize)
            .copied()?;
        let damage = self
            .confused_player_damage(player, target)
            .min(self.players[target].hp);
        let actor = &mut self.players[target];
        actor.hp = actor.hp.saturating_sub(damage);
        Some(BattleEvent::PlayerConfusedAttack {
            player,
            target,
            damage,
            defeated: !actor.is_alive(),
        })
    }

    pub fn scale_active_magic_by_mp(
        &mut self,
        role_id: u16,
        magic_object: u16,
        multiplier: u16,
    ) -> Option<u16> {
        let BattleFlow::PlayerMagic {
            player,
            magic: active_magic,
            ..
        } = &mut self.flow
        else {
            return None;
        };
        let actor = self.players.get_mut(*player)?;
        if actor.role_id != role_id || active_magic.object_id != magic_object {
            return None;
        }
        let base_damage = actor.mp.saturating_mul(multiplier);
        actor.mp = 0;
        if let Some(magic) = actor
            .magics
            .iter_mut()
            .find(|magic| magic.object_id == magic_object)
        {
            magic.base_damage = base_damage;
        }
        active_magic.base_damage = base_damage;
        Some(base_damage)
    }

    pub fn set_active_magic_base_damage(&mut self, magic_object: u16, base_damage: u16) -> bool {
        let BattleFlow::PlayerMagic {
            player,
            magic: active_magic,
            ..
        } = &mut self.flow
        else {
            return false;
        };
        if active_magic.object_id != magic_object {
            return false;
        }
        active_magic.base_damage = base_damage;
        if let Some(magic) = self.players[*player]
            .magics
            .iter_mut()
            .find(|magic| magic.object_id == magic_object)
        {
            magic.base_damage = base_damage;
        }
        true
    }

    pub fn set_magic_blow(&mut self, amount: i16) -> bool {
        if self.phase != BattlePhase::AwaitingCommand {
            return false;
        }
        self.magic_blow = amount;
        true
    }

    pub fn queue_player_magic_animation(&mut self, player: Option<usize>) -> bool {
        if self.phase != BattlePhase::AwaitingCommand
            || player.is_some_and(|player| player >= self.players.len())
        {
            return false;
        }
        self.pending_events
            .push_back(BattleEvent::PlayerMagicAnimation { player });
        true
    }

    pub fn simulate_player_magic(
        &mut self,
        target: usize,
        magic_object: u16,
        base_strength: u16,
        objects: &GlobalObjects,
        magics: &Magics,
    ) -> bool {
        if self.phase != BattlePhase::AwaitingCommand {
            return false;
        }
        let Some(magic) = battle_magic(magic_object, objects, magics) else {
            return false;
        };
        let targets = if magic.attacks_all {
            self.enemies
                .iter()
                .enumerate()
                .filter_map(|(index, enemy)| enemy.is_alive().then_some(index))
                .collect::<Vec<_>>()
        } else if self.enemies.get(target).is_some_and(BattleEnemy::is_alive) {
            vec![target]
        } else {
            self.first_living_enemy().into_iter().collect()
        };
        if targets.is_empty() {
            return false;
        }
        let mut blow = self.magic_blow;
        if magic.base_damage == 0 && base_strength == 0 {
            return true;
        }
        let mut visual = true;
        for enemy in targets {
            let damage = self.simulated_magic_damage(enemy, magic, base_strength);
            let actor = &mut self.enemies[enemy];
            actor.hp = actor.hp.wrapping_sub(damage);
            self.pending_events.push_back(BattleEvent::SimulatedMagic {
                enemy,
                magic,
                blow,
                damage,
                visual,
                defeated: !actor.is_alive(),
            });
            blow = 0;
            visual = false;
        }
        true
    }

    pub fn throw_weapon(
        &mut self,
        target: usize,
        magic_object: u16,
        multiplier: u16,
        objects: &GlobalObjects,
        magics: &Magics,
    ) -> bool {
        let BattleFlow::PlayerItem {
            player,
            kind: PlayerItemKind::Throw,
            ..
        } = self.flow
        else {
            return false;
        };
        let Some(attack_strength) = self
            .players
            .get(player)
            .map(|player| player.attack_strength)
        else {
            return false;
        };
        let strength = u32::from(multiplier)
            .saturating_mul(5)
            .saturating_add(u32::from(attack_strength).saturating_mul(self.random(4)));
        self.simulate_player_magic(
            target,
            magic_object,
            u16::try_from(strength).unwrap_or(u16::MAX),
            objects,
            magics,
        )
    }

    pub(super) fn backup_player_hp(&mut self) {
        self.previous_player_hp = self.players.iter().map(|player| player.hp).collect();
    }

    pub(super) fn collect_defeated_enemy_rewards(&mut self) {
        for enemy in &mut self.enemies {
            if enemy.is_defeated() && !enemy.rewards_collected {
                self.gained_rewards.experience = self
                    .gained_rewards
                    .experience
                    .saturating_add(u32::from(enemy.experience));
                self.gained_rewards.cash = self
                    .gained_rewards
                    .cash
                    .saturating_add(u32::from(enemy.cash));
                enemy.rewards_collected = true;
                enemy.object_id = 0;
            }
        }
    }

    /// Mirror the player-sensitive half of `PAL_BattlePostActionCheck`.
    /// Only the first eligible script is scheduled for one completed enemy action.
    pub(super) fn player_single_attack_damage(
        &mut self,
        player: usize,
        enemy: usize,
    ) -> (u16, bool) {
        let attacker = &self.players[player];
        let defender = &self.enemies[enemy];
        let defense = u32::from(defender.effective_defense());
        let mut damage = physical_damage(
            u32::from(attacker.attack_strength),
            defense,
            u32::from(defender.physical_resistance),
        )
        .saturating_add(1 + self.random(2));
        let critical = self.random(6) == 0
            || self.players[player]
                .statuses
                .is_active(BattleStatus::Bravery);
        if critical {
            damage = damage.saturating_mul(3);
        }
        let bonus_hit = self.players[player].role_id == 0 && self.random(12) == 0;
        if bonus_hit {
            damage = damage.saturating_mul(2);
        }
        damage = (damage as f32 * self.random_float(1.0, 1.125)) as u32;
        (
            u16::try_from(damage.max(1)).unwrap_or(u16::MAX),
            critical || bonus_hit,
        )
    }

    fn player_attack_all_damage(
        &self,
        player: usize,
        enemy: usize,
        critical: bool,
        division: u32,
    ) -> u16 {
        let attacker = &self.players[player];
        let defender = &self.enemies[enemy];
        let defense = u32::from(defender.effective_defense());
        let mut damage = physical_damage(
            u32::from(attacker.attack_strength),
            defense,
            u32::from(defender.physical_resistance),
        );
        if critical {
            damage = damage.saturating_mul(3);
        }
        damage /= division.max(1);
        u16::try_from(damage.max(1)).unwrap_or(u16::MAX)
    }

    pub(super) fn enemy_damage(&mut self, enemy: usize, player: usize) -> u16 {
        let attacker = &self.enemies[enemy];
        let attack = u32::from(attacker.effective_attack_strength()) + self.random(3);
        let defense = u32::from(self.players[player].defense)
            .saturating_mul(if self.players[player].defending { 2 } else { 1 });
        let mut damage = physical_damage(attack, defense, 2).saturating_add(self.random(2));
        if self.players[player]
            .statuses
            .is_active(BattleStatus::Protect)
        {
            damage /= 2;
        }
        u16::try_from(damage.max(1)).unwrap_or(u16::MAX)
    }

    fn confused_player_damage(&self, player: usize, target: usize) -> u16 {
        let attacker = &self.players[player];
        let defender = &self.players[target];
        let defense =
            u32::from(defender.defense).saturating_mul(if defender.defending { 2 } else { 1 });
        let mut damage = physical_damage(u32::from(attacker.attack_strength), defense, 2);
        if defender.statuses.is_active(BattleStatus::Protect) {
            damage /= 2;
        }
        u16::try_from(damage.max(1)).unwrap_or(u16::MAX)
    }

    fn confused_enemy_damage(&self, enemy: usize, target: usize) -> u16 {
        let attacker = &self.enemies[enemy];
        let defender = &self.enemies[target];
        let base = physical_damage(
            u32::from(attacker.effective_attack_strength()),
            u32::from(defender.simulated_magic_defense()),
            0,
        )
        .saturating_mul(2);
        let damage = base
            .checked_div(u32::from(defender.physical_resistance))
            .unwrap_or(base);
        u16::try_from(damage.max(1)).unwrap_or(u16::MAX)
    }

    fn magic_damage(&mut self, player: usize, enemy: usize, magic: BattleMagic) -> u16 {
        self.player_magic_damage_from_strength(
            enemy,
            magic,
            u32::from(self.players[player].magic_strength),
            false,
        )
        .max(1) as u16
    }

    fn simulated_magic_damage(
        &mut self,
        enemy: usize,
        magic: BattleMagic,
        base_strength: u16,
    ) -> u16 {
        self.player_magic_damage_from_strength(enemy, magic, u32::from(base_strength), true)
            .max(0) as u16
    }

    fn player_magic_damage_from_strength(
        &mut self,
        enemy: usize,
        magic: BattleMagic,
        base_strength: u32,
        simulated: bool,
    ) -> i16 {
        let strength = self.randomized_magic_strength(base_strength);
        let defender = &self.enemies[enemy];
        let defense = if simulated {
            defender.simulated_magic_defense()
        } else {
            defender.effective_defense()
        };
        classic_player_magic_damage(
            strength,
            defense,
            defender.elemental_resistance,
            defender.poison_resistance,
            self.battlefield_magic_effect,
            magic,
        )
    }

    pub(super) fn enemy_magic_damage(
        &mut self,
        enemy: usize,
        player: usize,
        magic: BattleMagic,
        auto_defended: bool,
    ) -> u16 {
        let attacker = &self.enemies[enemy];
        let raw_strength = i32::from(attacker.magic_strength as i16)
            + i32::from(attacker.level.saturating_add(6)) * 6;
        let strength =
            self.randomized_magic_strength(u32::try_from(raw_strength.max(0)).unwrap_or(u32::MAX));
        let defender = &self.players[player];
        let mut damage = physical_damage(strength, u32::from(defender.defense), 0) / 4
            + u32::from(magic.base_damage);
        let element = usize::from(magic.elemental);
        if (1..=pal_assets::player_roles::MAGIC_ELEMENT_COUNT).contains(&element) {
            let resistance = u32::from(defender.elemental_resistance[element - 1].min(100));
            damage = damage.saturating_mul(100 - resistance) / 100;
            let field = i32::from(self.battlefield_magic_effect[element - 1]) + 10;
            damage = damage.saturating_mul(u32::try_from(field.max(0)).unwrap_or(0)) / 10;
        } else if element > pal_assets::player_roles::MAGIC_ELEMENT_COUNT {
            let resistance = u32::from(defender.poison_resistance.min(100));
            damage = damage.saturating_mul(100 - resistance) / 100;
        }
        let divisor = (if defender.defending { 2 } else { 1 })
            * (if defender.statuses.is_active(BattleStatus::Protect) {
                2
            } else {
                1
            })
            + usize::from(auto_defended);
        damage /= u32::try_from(divisor).unwrap_or(1).max(1);
        u16::try_from(damage.max(1)).unwrap_or(u16::MAX)
    }

    pub(super) fn random(&mut self, upper_exclusive: u32) -> u32 {
        if upper_exclusive <= 1 {
            return 0;
        }
        random::random_long(&mut self.random_state, 0, upper_exclusive - 1)
    }

    /// Match Classic's inclusive floating-point interpolation before the caller's integer cast.
    pub(super) fn random_float(&mut self, from: f32, to: f32) -> f32 {
        if to <= from {
            return from;
        }
        random::random_float(&mut self.random_state, from, to)
    }

    /// Classic truncates once after multiplying by `RandomFloat(10, 11)`, then divides by 10.
    pub(super) fn randomized_magic_strength(&mut self, strength: u32) -> u32 {
        (strength as f32 * self.random_float(10.0, 11.0)) as u32 / 10
    }

    pub fn collect_enemy(&self, enemy_index: usize) -> Option<u16> {
        (self.phase == BattlePhase::AwaitingCommand)
            .then(|| self.enemies.get(enemy_index))
            .flatten()
            .map(|enemy| enemy.collect_value)
            .filter(|&value| value != 0)
    }

    pub fn steal_enemy(&mut self, enemy_index: usize, rate: u16) -> Option<BattleSteal> {
        if self.phase != BattlePhase::AwaitingCommand {
            return None;
        }
        let enemy = self.enemies.get(enemy_index)?;
        let item = enemy.steal_item;
        let count = enemy.steal_item_count;
        if count == 0 || (rate != 0 && self.random(11) > u32::from(rate)) {
            return Some(BattleSteal::Nothing);
        }
        if item == 0 {
            let amount = count / u16::try_from(self.random(2) + 2).ok()?;
            self.enemies.get_mut(enemy_index)?.steal_item_count = count.saturating_sub(amount);
            return Some(if amount == 0 {
                BattleSteal::Nothing
            } else {
                BattleSteal::Cash(amount)
            });
        }
        self.enemies.get_mut(enemy_index)?.steal_item_count = count - 1;
        Some(BattleSteal::Item(item))
    }

    pub fn hide_players(&mut self, rounds: u16) -> bool {
        if self.phase != BattlePhase::AwaitingCommand {
            return false;
        }
        self.hiding_time = rounds;
        true
    }

    pub fn hiding_time(&self) -> u16 {
        self.hiding_time
    }

    pub fn rewards(&self) -> BattleRewards {
        self.gained_rewards
    }

    pub fn victory_rewards_pending(&self) -> bool {
        self.victory_settlement == VictorySettlementStage::RewardsPending
    }

    pub fn mark_victory_rewards_applied(&mut self) -> bool {
        if self.phase != BattlePhase::Finished(BattleResult::Won)
            || self.victory_settlement != VictorySettlementStage::RewardsPending
        {
            return false;
        }
        self.victory_settlement = VictorySettlementStage::ScriptsPending;
        true
    }

    pub fn begin_battle_end_scripts(&mut self) -> bool {
        if self.victory_settlement != VictorySettlementStage::ScriptsPending {
            return false;
        }
        let BattlePhase::Finished(result) = self.phase else {
            return false;
        };
        self.phase = BattlePhase::AwaitingCommand;
        self.victory_settlement = VictorySettlementStage::ScriptsRunning;
        self.start_battle_end_scripts(result);
        true
    }

    pub fn ready_to_leave(&self) -> bool {
        matches!(self.phase, BattlePhase::Finished(_))
            && (self.victory_settlement == VictorySettlementStage::ReadyToLeave
                || self.victory_settlement == VictorySettlementStage::NotApplicable)
    }

    pub fn victory_rewards_were_applied(&self) -> bool {
        matches!(
            self.victory_settlement,
            VictorySettlementStage::ScriptsPending
                | VictorySettlementStage::ScriptsRunning
                | VictorySettlementStage::ReadyToLeave
        )
    }

    pub(crate) fn set_auto_battle(&mut self, enabled: bool) {
        self.auto_battle = enabled;
    }

    pub fn hidden_experience_counts(
        &self,
        player: usize,
    ) -> Option<[u16; HIDDEN_EXPERIENCE_CATEGORY_COUNT]> {
        self.hidden_experience_counts.get(player).copied()
    }

    /// Return rewards actually granted by the finished result.
    pub fn settled_rewards(&self) -> Option<BattleRewards> {
        match self.phase {
            BattlePhase::AwaitingCommand => None,
            BattlePhase::Finished(BattleResult::Won) => Some(self.rewards()),
            BattlePhase::Finished(
                BattleResult::Lost | BattleResult::Fled | BattleResult::Terminated,
            ) => Some(BattleRewards::default()),
        }
    }

    pub fn damage_enemy(&mut self, enemy_index: usize, amount: u16, apply_to_all: bool) -> bool {
        if self.phase != BattlePhase::AwaitingCommand {
            return false;
        }
        if apply_to_all {
            for enemy in self.enemies.iter_mut().filter(|enemy| enemy.is_present()) {
                enemy.hp = enemy.hp.wrapping_sub(amount);
            }
            return true;
        }
        let Some(enemy) = self.enemies.get_mut(enemy_index) else {
            return false;
        };
        enemy.hp = enemy.hp.wrapping_sub(amount);
        true
    }

    pub fn drain_enemy_hp(&mut self, enemy_index: usize, amount: u16) -> bool {
        if self.phase != BattlePhase::AwaitingCommand {
            return false;
        }
        let Some(player_index) = self.active_player else {
            return false;
        };
        let Some(enemy) = self.enemies.get_mut(enemy_index) else {
            return false;
        };
        enemy.hp = enemy.hp.wrapping_sub(amount);
        let player = &mut self.players[player_index];
        player.hp = player.hp.wrapping_add(amount);
        if player.hp > player.max_hp {
            player.hp = player.max_hp;
        }
        true
    }

    pub fn halve_enemy_hp(&mut self, enemy_index: usize, maximum_damage: u16) -> bool {
        if self.phase != BattlePhase::AwaitingCommand {
            return false;
        }
        let Some(enemy) = self.enemies.get_mut(enemy_index) else {
            return false;
        };
        let damage = (enemy.hp / 2 + 1).min(maximum_damage);
        enemy.hp = enemy.hp.wrapping_sub(damage);
        true
    }

    pub fn kill_enemy(&mut self, enemy_index: usize) -> bool {
        let Some(enemy) = self.enemies.get_mut(enemy_index) else {
            return false;
        };
        enemy.hp = 0;
        true
    }

    pub fn enemy_hp_above(&self, enemy_index: usize, percentage: u16) -> Option<bool> {
        let enemy = self.enemies.get(enemy_index)?;
        self.enemy_hp_above_with_max(enemy_index, percentage, enemy.max_hp)
    }

    pub fn enemy_hp_above_with_max(
        &self,
        enemy_index: usize,
        percentage: u16,
        definition_hp: u16,
    ) -> Option<bool> {
        let enemy = self.enemies.get(enemy_index)?;
        Some(
            u32::from(enemy.hp).saturating_mul(100)
                > u32::from(definition_hp).saturating_mul(u32::from(percentage)),
        )
    }

    pub fn enemy_not_first_kind(&self, enemy_index: usize) -> Option<bool> {
        let enemy = self.enemies.get(enemy_index)?;
        let mut matching = 0usize;
        for slot in 0..self.enemy_layout_slots {
            let object_id = self
                .enemy_index_for_slot(slot)
                .and_then(|index| self.enemies.get(index))
                .map_or(0, |actor| actor.object_id);
            if object_id == enemy.object_id {
                matching += 1;
                if slot == enemy.slot {
                    return Some(matching > 1);
                }
            }
        }
        Some(false)
    }

    pub fn set_enemy_magic(
        &mut self,
        enemy_index: usize,
        magic_object: u16,
        rate: u16,
        objects: &GlobalObjects,
        magics: &Magics,
    ) -> bool {
        let magic = match magic_object {
            0 | u16::MAX => None,
            object_id => {
                let Some(magic) = battle_magic(object_id, objects, magics) else {
                    return false;
                };
                Some(magic)
            }
        };
        let Some(enemy) = self.enemies.get_mut(enemy_index) else {
            return false;
        };
        enemy.magic_object = magic_object;
        enemy.magic_rate = if rate == 0 { 10 } else { rate };
        enemy.magic = magic;
        true
    }

    /// Replace one temporary extra-equipment effect for a battle player.
    pub fn set_temporary_player_stat(&mut self, role_id: u16, attribute: u16, value: u16) -> bool {
        let Some(slot) = attribute
            .checked_sub(17)
            .map(usize::from)
            .filter(|&slot| slot < 6)
        else {
            return false;
        };
        let Some(player) = self
            .players
            .iter()
            .position(|player| player.role_id == role_id)
        else {
            return false;
        };
        let Some(target) = battle_player_stat_mut(&mut self.players[player], attribute) else {
            return false;
        };
        let previous = self.temporary_player_stats[player][slot];
        *target = target.wrapping_sub(previous).wrapping_add(value);
        self.temporary_player_stats[player][slot] = value;
        true
    }

    /// Set the extra equipment battle sprite; zero restores the pre-battle value.
    pub fn set_temporary_player_sprite(&mut self, role_id: u16, sprite: u16) -> bool {
        let Some(player) = self
            .players
            .iter()
            .position(|player| player.role_id == role_id)
        else {
            return false;
        };
        self.players[player].battle_sprite_num = if sprite == 0 {
            self.base_player_battle_sprites[player]
        } else {
            sprite
        };
        true
    }

    pub fn cure_enemy_poison(
        &mut self,
        enemy_index: usize,
        poison_id: u16,
        apply_to_all: bool,
    ) -> bool {
        if self.phase != BattlePhase::AwaitingCommand || poison_id == 0 {
            return false;
        }
        if apply_to_all {
            for enemy in self.enemies.iter_mut().filter(|enemy| enemy.is_present()) {
                cure_poison(&mut enemy.poisons, poison_id);
            }
            return true;
        }
        let Some(enemy) = self.enemies.get_mut(enemy_index) else {
            return false;
        };
        cure_poison(&mut enemy.poisons, poison_id);
        true
    }

    pub fn set_enemy_status(
        &mut self,
        enemy_index: usize,
        status: BattleStatus,
        rounds: u16,
    ) -> Option<bool> {
        if self.phase != BattlePhase::AwaitingCommand || enemy_index >= self.enemies.len() {
            return None;
        }
        let resistance = if self.enemies[enemy_index].is_present() {
            self.enemies[enemy_index].sorcery_resistance.min(9)
        } else {
            0
        };
        if self.random(10) <= u32::from(resistance) {
            return Some(false);
        }
        self.enemies[enemy_index]
            .statuses
            .set_for_enemy(status, rounds);
        Some(true)
    }
}
