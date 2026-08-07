//! Battle construction and dynamic enemy-roster changes.

use std::collections::VecDeque;

use pal_assets::battle::{BattleData, EnemyPositions, MAX_ENEMIES_IN_TEAM};
use pal_assets::magic::Magics;
use pal_assets::objects::GlobalObjects;
use pal_assets::player_roles::PlayerRole;

use super::helpers::{battle_enemy_from_object, battle_player, next_player};
use super::types::{
    BattleEnemy, BattleEvent, BattleFlow, BattlePhase, BattlePlayer, BattlePoison, BattleRequest,
    BattleResult, BattleRewards, BattleState, BattleStatus, BattleStatuses, VictorySettlementStage,
    HIDDEN_EXPERIENCE_CATEGORY_COUNT, MAX_BATTLE_POISONS,
};
use crate::random;

impl BattleState {
    pub fn new<'a>(
        request: BattleRequest,
        battlefield: u16,
        music: u16,
        roles: impl IntoIterator<Item = (u16, &'a PlayerRole)>,
        data: &BattleData,
        objects: &GlobalObjects,
        magics: &Magics,
    ) -> Option<Self> {
        let battlefield_definition = data.battlefields.get(battlefield)?;
        let team = data.enemy_teams.get(request.enemy_team)?;
        let layout = team
            .object_ids
            .iter()
            .copied()
            .take_while(|&object_id| object_id != u16::MAX)
            .collect::<Vec<_>>();
        if layout.is_empty() {
            return None;
        }

        let enemies = layout
            .iter()
            .copied()
            .enumerate()
            .filter(|&(_, object_id)| object_id != 0)
            .map(|(slot, object_id)| {
                let position = data.enemy_positions.get(layout.len(), slot)?;
                battle_enemy_from_object(slot, position, object_id, data, objects, magics)
            })
            .collect::<Option<Vec<_>>>()?;
        if enemies.is_empty() {
            return None;
        }

        let players = roles
            .into_iter()
            .map(|(role_id, role)| battle_player(role_id, role, objects, magics))
            .collect::<Option<Vec<_>>>()?;
        if players.is_empty() {
            return None;
        }

        let acted = vec![false; players.len()];
        let player_actions = vec![None; players.len()];
        let previous_player_actions = vec![None; players.len()];
        let automatic_player_attacks = vec![false; players.len()];
        let previous_automatic_player_attacks = vec![false; players.len()];
        let hidden_experience_counts = vec![[0; HIDDEN_EXPERIENCE_CATEGORY_COUNT]; players.len()];
        let temporary_player_stats = vec![[0; 6]; players.len()];
        let base_player_battle_sprites = players
            .iter()
            .map(|player| player.battle_sprite_num)
            .collect();
        let previous_player_hp = players.iter().map(|player| player.hp).collect();
        let round_start_player_hp = players.iter().map(|player| player.hp).collect();
        let round_start_enemy_hp = enemies.iter().map(|enemy| enemy.hp).collect();
        let active_player = next_player(&players, &acted, 0);
        let phase = if active_player.is_some() {
            BattlePhase::AwaitingCommand
        } else {
            BattlePhase::Finished(BattleResult::Lost)
        };
        let mut battle = Self {
            enemy_team: request.enemy_team,
            battlefield,
            music,
            is_boss: request.is_boss,
            players,
            enemies,
            enemy_layout_slots: layout.len(),
            hiding_time: 0,
            phase,
            active_player,
            acted,
            player_actions,
            previous_player_actions,
            automatic_player_attacks,
            previous_automatic_player_attacks,
            repeating_round: false,
            auto_attack_mode: false,
            previous_auto_attack: false,
            execution_auto_attack: false,
            hidden_experience_counts,
            cooperative_magic_performed: false,
            action_queue: Vec::new(),
            action_index: 0,
            temporary_player_stats,
            base_player_battle_sprites,
            inventory_amounts: None,
            previous_player_hp,
            round_start_player_hp,
            round_start_enemy_hp,
            gained_rewards: BattleRewards::default(),
            auto_battle: false,
            round: 1,
            random_state: random::DEFAULT_RANDOM_SEED,
            battlefield_magic_effect: battlefield_definition.magic_effect,
            magic_blow: 0,
            flow: BattleFlow::Finished,
            pending_scripts: VecDeque::new(),
            active_script: None,
            deferred_round_result: None,
            victory_settlement: VictorySettlementStage::NotApplicable,
            pending_events: VecDeque::new(),
        };
        if phase == BattlePhase::AwaitingCommand {
            battle.start_turn_start_scripts(false);
        }
        Some(battle)
    }

    pub fn phase(&self) -> BattlePhase {
        self.phase
    }

    /// Whether Classic is accepting player commands rather than performing the round.
    pub fn is_command_phase(&self) -> bool {
        self.phase == BattlePhase::AwaitingCommand && self.flow == BattleFlow::Command
    }

    /// Return the next state of Classic's process-wide random sequence.
    pub fn random_state(&self) -> u32 {
        self.random_state
    }

    /// Hand the process-wide Classic random sequence to this battle instance.
    pub fn set_random_state(&mut self, state: u32) {
        self.random_state = state;
    }

    pub fn round(&self) -> u32 {
        self.round
    }

    pub fn active_player(&self) -> Option<usize> {
        (self.phase == BattlePhase::AwaitingCommand
            && self.flow == BattleFlow::Command
            && self.pending_scripts.is_empty()
            && self.active_script.is_none())
        .then_some(self.active_player)
        .flatten()
    }

    pub fn is_enemy_turn(&self) -> bool {
        self.phase == BattlePhase::AwaitingCommand
            && matches!(
                self.flow,
                BattleFlow::EnemyMagic { .. } | BattleFlow::EnemyAttackItem { .. }
            )
    }

    pub fn refresh_player_effects(&mut self) {
        self.active_player = next_player(&self.players, &self.acted, 0);
        self.phase = if self.players.iter().any(BattlePlayer::is_combat_active) {
            BattlePhase::AwaitingCommand
        } else {
            BattlePhase::Finished(BattleResult::Lost)
        };
        if self.phase != BattlePhase::AwaitingCommand {
            self.flow = BattleFlow::Finished;
            self.pending_scripts.clear();
            self.active_script = None;
        }
    }

    /// Commit automatic actions for disabled/confused players and start round resolution.
    pub fn first_living_enemy(&self) -> Option<usize> {
        self.enemies.iter().position(BattleEnemy::is_alive)
    }

    pub(super) fn living_enemy_from(&self, enemy_index: usize) -> Option<usize> {
        let start_slot = self.enemies.get(enemy_index).map_or(0, |enemy| enemy.slot);
        (0..self.enemy_layout_slots).find_map(|offset| {
            let slot = (start_slot + offset) % self.enemy_layout_slots;
            self.enemy_index_for_slot(slot)
                .filter(|&index| self.enemies[index].is_alive())
        })
    }

    /// Resolve an original five-slot enemy owner to the stable runtime actor index.
    pub fn enemy_index_for_slot(&self, slot: usize) -> Option<usize> {
        self.enemies
            .iter()
            .rposition(|enemy| enemy.slot == slot && enemy.is_alive())
            .or_else(|| self.enemies.iter().rposition(|enemy| enemy.slot == slot))
    }

    pub fn enemy_slot_for_index(&self, enemy_index: usize) -> Option<u16> {
        u16::try_from(self.enemies.get(enemy_index)?.slot).ok()
    }

    /// Divide the sole living enemy, preserving its current definition and script entries.
    ///
    /// The original health divisor uses the requested copy count even when only the first four
    /// free slots can be populated.
    pub fn divide_enemy(
        &mut self,
        enemy_index: usize,
        copies: u16,
        positions: &EnemyPositions,
    ) -> Option<Vec<usize>> {
        if self.phase != BattlePhase::AwaitingCommand
            || self
                .enemies
                .iter()
                .filter(|enemy| enemy.is_present())
                .count()
                != 1
        {
            return None;
        }
        let source = self.enemies.get(enemy_index)?.clone();
        if !source.is_present() || source.hp <= 1 {
            return None;
        }

        let requested = usize::from(copies.max(1));
        let free_slots = (0..MAX_ENEMIES_IN_TEAM)
            .filter(|&slot| {
                !self
                    .enemies
                    .iter()
                    .any(|enemy| enemy.slot == slot && enemy.object_id != 0)
            })
            .take(requested)
            .collect::<Vec<_>>();
        let layout_slots = self
            .enemies
            .iter()
            .filter(|enemy| enemy.is_present())
            .map(|enemy| enemy.slot)
            .chain(free_slots.iter().copied())
            .max()?
            .checked_add(1)?;
        if layout_slots > MAX_ENEMIES_IN_TEAM
            || self
                .enemies
                .iter()
                .filter(|enemy| enemy.is_present())
                .map(|enemy| enemy.slot)
                .chain(free_slots.iter().copied())
                .any(|slot| positions.get(layout_slots, slot).is_none())
        {
            return None;
        }

        let shared_hp = u16::try_from(
            (u32::from(source.hp) + u32::try_from(requested).ok()?)
                / (u32::try_from(requested).ok()? + 1),
        )
        .ok()?;
        self.enemy_layout_slots = layout_slots;
        for enemy in self.enemies.iter_mut().filter(|enemy| enemy.is_present()) {
            enemy.position = positions.get(layout_slots, enemy.slot)?;
        }
        self.enemies.get_mut(enemy_index)?.hp = shared_hp;
        let origin = self.enemies.get(enemy_index)?.position;

        let mut added = Vec::with_capacity(free_slots.len());
        for slot in free_slots {
            self.retire_defeated_slot(slot);
            let mut copy = source.clone();
            copy.slot = slot;
            copy.position = positions.get(layout_slots, slot)?;
            copy.hp = shared_hp;
            copy.statuses = BattleStatuses::default();
            copy.poisons = [BattlePoison::default(); MAX_BATTLE_POISONS];
            added.push(self.enemies.len());
            self.enemies.push(copy);
        }
        self.pending_events
            .push_back(BattleEvent::EnemyDivide { origin });
        Some(added)
    }

    /// Summon enemies into empty slots inside the current layout boundary.
    pub fn summon_enemy(
        &mut self,
        enemy_index: usize,
        object_id: u16,
        count: u16,
        data: &BattleData,
        objects: &GlobalObjects,
        magics: &Magics,
    ) -> Option<Vec<usize>> {
        if self.phase != BattlePhase::AwaitingCommand {
            return None;
        }
        let (source_object, blocked) = {
            let source = self.enemies.get(enemy_index)?;
            (
                source.object_id,
                self.hiding_time != 0
                    || source.statuses.is_active(BattleStatus::Sleep)
                    || source.statuses.is_active(BattleStatus::Paralyzed)
                    || source.statuses.is_active(BattleStatus::Confused),
            )
        };
        let summoned_object = match object_id {
            0 | u16::MAX => source_object,
            object_id => object_id,
        };
        let added = if blocked {
            None
        } else {
            'summon: {
                let requested = if count as i16 <= 0 {
                    1
                } else {
                    usize::from(count)
                };
                let free_slots = (0..self.enemy_layout_slots)
                    .filter(|&slot| {
                        !self
                            .enemies
                            .iter()
                            .any(|enemy| enemy.slot == slot && enemy.object_id != 0)
                    })
                    .take(requested)
                    .collect::<Vec<_>>();
                if free_slots.len() != requested {
                    break 'summon None;
                }
                let Some(actors) = free_slots
                    .iter()
                    .copied()
                    .map(|slot| {
                        let position = data.enemy_positions.get(self.enemy_layout_slots, slot)?;
                        battle_enemy_from_object(
                            slot,
                            position,
                            summoned_object,
                            data,
                            objects,
                            magics,
                        )
                    })
                    .collect::<Option<Vec<_>>>()
                else {
                    break 'summon None;
                };
                for slot in free_slots {
                    self.retire_defeated_slot(slot);
                }
                let first = self.enemies.len();
                self.enemies.extend(actors);
                Some((first..self.enemies.len()).collect::<Vec<_>>())
            }
        };
        let summoned_mask = added.as_ref().map_or(0, |added| {
            added.iter().fold(0u8, |mask, &index| {
                let slot = self
                    .enemies
                    .get(index)
                    .map_or(usize::MAX, |enemy| enemy.slot);
                mask | 1u8.checked_shl(slot as u32).unwrap_or(0)
            })
        });
        self.pending_events.push_back(BattleEvent::EnemySummon {
            caster: enemy_index,
            summoned_mask,
        });
        added
    }

    /// Replace an enemy's static definition while retaining HP, statuses and lifecycle scripts.
    ///
    /// `Some(false)` is a valid no-op caused by hiding or an incapacitating status.
    pub fn transform_enemy(
        &mut self,
        enemy_index: usize,
        object_id: u16,
        data: &BattleData,
        objects: &GlobalObjects,
        magics: &Magics,
    ) -> Option<bool> {
        if self.phase != BattlePhase::AwaitingCommand {
            return None;
        }
        let source = self.enemies.get(enemy_index)?;
        if self.hiding_time != 0
            || source.statuses.is_active(BattleStatus::Sleep)
            || source.statuses.is_active(BattleStatus::Paralyzed)
            || source.statuses.is_active(BattleStatus::Confused)
        {
            return Some(false);
        }
        let previous_enemy_id = source.enemy_id;
        let previous_y_offset = source.y_offset;
        let position = data
            .enemy_positions
            .get(self.enemy_layout_slots, source.slot)?;
        let mut replacement =
            battle_enemy_from_object(source.slot, position, object_id, data, objects, magics)?;
        replacement.hp = source.hp;
        replacement.turn_start_script = source.turn_start_script;
        replacement.battle_end_script = source.battle_end_script;
        replacement.ready_script = source.ready_script;
        replacement.statuses = source.statuses;
        replacement.poisons = source.poisons;
        self.enemies[enemy_index] = replacement;
        self.pending_events.push_back(BattleEvent::EnemyTransform {
            enemy: enemy_index,
            previous_enemy_id,
            previous_y_offset,
        });
        Some(true)
    }

    fn retire_defeated_slot(&mut self, slot: usize) {
        for enemy in self
            .enemies
            .iter_mut()
            .filter(|enemy| enemy.slot == slot && !enemy.is_alive())
        {
            enemy.object_id = 0;
            enemy.turn_start_script = 0;
            enemy.battle_end_script = 0;
            enemy.ready_script = 0;
            enemy.statuses = BattleStatuses::default();
            enemy.poisons = [BattlePoison::default(); MAX_BATTLE_POISONS];
        }
    }
}
