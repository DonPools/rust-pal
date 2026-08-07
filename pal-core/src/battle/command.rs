//! Player command validation and submission.

use super::helpers::player_action_target_index;
use super::types::{
    BattleEnemy, BattleEvent, BattleFlow, BattleMagic, BattlePhase, BattlePlayer, BattleResult,
    BattleState, BattleStatus, BattleTarget, PlayerAction,
};

impl BattleState {
    pub fn attack(&mut self, target: usize) -> Option<Vec<BattleEvent>> {
        self.commit_attack(target, false)
    }

    /// Commit an attack selected while Classic's persistent auto-attack mode is active.
    pub fn attack_automatically(&mut self, target: usize) -> Option<Vec<BattleEvent>> {
        self.commit_attack(target, true)
    }

    fn commit_attack(&mut self, target: usize, automatic: bool) -> Option<Vec<BattleEvent>> {
        if self.phase != BattlePhase::AwaitingCommand
            || self.flow != BattleFlow::Command
            || self.has_script_work()
            || !self.enemies.get(target)?.is_alive()
        {
            return None;
        }
        let player_index = self.active_player?;
        if !self.players.get(player_index)?.can_act() || self.acted[player_index] {
            return None;
        }

        if automatic {
            self.auto_attack_mode = true;
        }
        self.commit_player_action_with_auto_attack(
            player_index,
            PlayerAction::Attack { target },
            automatic,
        );
        Some(Vec::new())
    }

    pub fn cast_magic(&mut self, magic_index: usize, target: usize) -> Option<Vec<BattleEvent>> {
        self.cast_magic_at(magic_index, BattleTarget::Enemy(target))
    }

    /// Commit any battle-usable magic with its original enemy/player target side.
    pub fn cast_magic_at(
        &mut self,
        magic_index: usize,
        target: BattleTarget,
    ) -> Option<Vec<BattleEvent>> {
        if self.phase != BattlePhase::AwaitingCommand
            || self.flow != BattleFlow::Command
            || self.has_script_work()
        {
            return None;
        }
        let player_index = self.active_player?;
        if !self.players.get(player_index)?.can_act()
            || self.players[player_index]
                .statuses
                .is_active(BattleStatus::Silence)
            || self.acted[player_index]
        {
            return None;
        }
        let magic = *self.players[player_index].magics.get(magic_index)?;
        if !magic.usable_in_battle()
            || self.players[player_index].mp < magic.mp_cost
            || !self.magic_target_is_valid(magic, target)
        {
            return None;
        }
        self.commit_player_action(
            player_index,
            PlayerAction::Magic {
                magic: magic_index,
                target,
            },
        );
        Some(Vec::new())
    }

    pub(super) fn magic_target_is_valid(&self, magic: BattleMagic, target: BattleTarget) -> bool {
        match target {
            BattleTarget::Enemy(enemy) => {
                magic.usable_to_enemy()
                    && !magic.apply_to_all()
                    && self.enemies.get(enemy).is_some_and(BattleEnemy::is_alive)
            }
            BattleTarget::Player(player) => {
                !magic.usable_to_enemy()
                    && !magic.apply_to_all()
                    && self.players.get(player).is_some()
            }
            BattleTarget::AllEnemies => {
                magic.usable_to_enemy()
                    && magic.apply_to_all()
                    && self.enemies.iter().any(BattleEnemy::is_alive)
            }
            BattleTarget::AllPlayers => {
                !magic.usable_to_enemy() && magic.apply_to_all() && !self.players.is_empty()
            }
        }
    }

    /// Commit a defend action for the active player.
    pub fn defend(&mut self) -> Option<Vec<BattleEvent>> {
        if !self.can_commit_player_action() {
            return None;
        }
        let player = self.active_player?;
        self.commit_player_action(player, PlayerAction::Defend);
        Some(Vec::new())
    }

    /// Commit the original forced/automatic offensive choice for the active player.
    ///
    /// A range of 60 is used by the Force shortcut, while script-forced auto battle
    /// uses 9999 so the chosen spell varies much more aggressively.
    pub fn commit_auto_action(&mut self, random_range: u16) -> Option<Vec<BattleEvent>> {
        if !self.can_commit_player_action() {
            return None;
        }
        let player = self.active_player?;
        let living = self
            .enemies
            .iter()
            .enumerate()
            .filter_map(|(index, enemy)| enemy.is_alive().then_some(index))
            .collect::<Vec<_>>();
        let target = *living.get(self.random(living.len() as u32) as usize)?;
        let candidates = if self.players[player]
            .statuses
            .is_active(BattleStatus::Silence)
        {
            Vec::new()
        } else {
            self.players[player]
                .magics
                .iter()
                .copied()
                .enumerate()
                .filter(|(_, magic)| {
                    magic.usable_in_battle()
                        && magic.usable_to_enemy()
                        && magic.mp_cost != 1
                        && magic.mp_cost <= self.players[player].mp
                        && (magic.base_damage as i16) > 0
                })
                .collect::<Vec<_>>()
        };
        let mut selected = None;
        let mut maximum_power = 0u32;
        for (magic_index, magic) in candidates {
            let power = u32::from(magic.base_damage)
                .saturating_add(self.random(u32::from(random_range).saturating_add(1)));
            if power > maximum_power {
                maximum_power = power;
                selected = Some((magic_index, magic));
            }
        }
        let action = if let Some((magic, spell)) = selected {
            PlayerAction::Magic {
                magic,
                target: if spell.apply_to_all() {
                    BattleTarget::AllEnemies
                } else {
                    BattleTarget::Enemy(target)
                },
            }
        } else {
            PlayerAction::Attack { target }
        };
        self.commit_player_action(player, action);
        Some(Vec::new())
    }

    pub fn active_cooperative_magic(&self) -> Option<BattleMagic> {
        let player = self.active_player()?;
        self.players.get(player)?.cooperative_magic
    }

    pub fn can_use_cooperative_magic(&self) -> bool {
        let Some(player) = self.active_player() else {
            return false;
        };
        self.players.get(player).is_some_and(Self::coop_healthy)
            && self
                .players
                .iter()
                .filter(|actor| Self::coop_healthy(actor))
                .count()
                > 1
            && self.players[player]
                .cooperative_magic
                .is_some_and(|magic| magic.usable_in_battle() && magic.usable_to_enemy())
    }

    pub fn cast_cooperative_magic(&mut self, target: BattleTarget) -> Option<Vec<BattleEvent>> {
        if !self.can_commit_player_action() || !self.can_use_cooperative_magic() {
            return None;
        }
        let player = self.active_player?;
        let magic = self.players[player].cooperative_magic?;
        if !self.magic_target_is_valid(magic, target) {
            return None;
        }
        self.acted[player] = true;
        self.player_actions[player] = Some(PlayerAction::CooperativeMagic { target });
        self.automatic_player_attacks[player] = false;
        self.acted[player + 1..].fill(true);
        self.active_player = None;
        self.build_action_queue();
        Some(Vec::new())
    }

    pub(super) fn coop_healthy(player: &BattlePlayer) -> bool {
        player.is_alive()
            && !player.is_dying()
            && !player.statuses.is_active(BattleStatus::Sleep)
            && !player.statuses.is_active(BattleStatus::Confused)
            && !player.statuses.is_active(BattleStatus::Silence)
            && !player.statuses.is_active(BattleStatus::Paralyzed)
            && !player.statuses.is_active(BattleStatus::Puppet)
    }

    /// Revert the most recently committed command while the party is still choosing actions.
    pub fn undo_last_command(&mut self) -> Option<usize> {
        if self.phase != BattlePhase::AwaitingCommand
            || self.flow != BattleFlow::Command
            || self.has_script_work()
        {
            return None;
        }
        let current = self.active_player?;
        let previous = (0..current).rev().find(|&player| self.acted[player])?;
        self.acted[previous] = false;
        self.player_actions[previous] = None;
        self.automatic_player_attacks[previous] = false;
        self.active_player = Some(previous);
        Some(previous)
    }

    /// Item object required by the active player's previous command.
    /// The boolean is true for a thrown item and false for a used item.
    pub fn repeated_item_requirement(&self) -> Option<(u16, bool)> {
        let player = self.active_player()?;
        match self
            .previous_player_actions
            .get(player)
            .copied()
            .flatten()?
        {
            PlayerAction::UseItem { item_object, .. } => Some((item_object, false)),
            PlayerAction::ThrowItem { item_object, .. } => Some((item_object, true)),
            PlayerAction::Attack { .. }
            | PlayerAction::Magic { .. }
            | PlayerAction::CooperativeMagic { .. }
            | PlayerAction::Flee
            | PlayerAction::Defend
            | PlayerAction::AttackMate => None,
        }
    }

    /// Repeat the active player's previous-round command using Classic fallbacks.
    pub fn repeat_last_action(&mut self) -> Option<Vec<BattleEvent>> {
        if !self.can_commit_player_action() {
            return None;
        }
        let player = self.active_player?;
        self.repeating_round = true;
        self.auto_attack_mode = self.previous_auto_attack;
        let automatic_attack = self
            .previous_automatic_player_attacks
            .get(player)
            .copied()
            .unwrap_or(false);
        let default_target = self.first_living_enemy()?;
        let preserved_target = self
            .player_actions
            .get(player)
            .copied()
            .flatten()
            .and_then(player_action_target_index);
        let action = self
            .previous_player_actions
            .get(player)
            .copied()
            .flatten()
            .unwrap_or(PlayerAction::Attack {
                target: default_target,
            });
        let action = match action {
            PlayerAction::Attack { target } => PlayerAction::Attack {
                target: preserved_target.unwrap_or(target),
            },
            PlayerAction::AttackMate => PlayerAction::Attack {
                target: preserved_target.unwrap_or(default_target),
            },
            PlayerAction::Magic { magic, target } => {
                let Some(spell) = self.players[player].magics.get(magic).copied() else {
                    self.commit_player_action(
                        player,
                        PlayerAction::Attack {
                            target: default_target,
                        },
                    );
                    return Some(Vec::new());
                };
                let target = match target {
                    BattleTarget::Enemy(enemy) => {
                        BattleTarget::Enemy(preserved_target.unwrap_or(enemy))
                    }
                    BattleTarget::Player(target) => {
                        BattleTarget::Player(preserved_target.unwrap_or(target))
                    }
                    BattleTarget::AllEnemies => BattleTarget::AllEnemies,
                    BattleTarget::AllPlayers => BattleTarget::AllPlayers,
                };
                if self.players[player]
                    .statuses
                    .is_active(BattleStatus::Silence)
                    || self.players[player].mp < spell.mp_cost
                {
                    if spell.usable_to_enemy() {
                        PlayerAction::Attack {
                            target: default_target,
                        }
                    } else {
                        PlayerAction::Defend
                    }
                } else {
                    PlayerAction::Magic { magic, target }
                }
            }
            PlayerAction::CooperativeMagic { target } => {
                let target = match target {
                    BattleTarget::Enemy(enemy) => {
                        BattleTarget::Enemy(preserved_target.unwrap_or(enemy))
                    }
                    BattleTarget::Player(target) => {
                        BattleTarget::Player(preserved_target.unwrap_or(target))
                    }
                    BattleTarget::AllEnemies => BattleTarget::AllEnemies,
                    BattleTarget::AllPlayers => BattleTarget::AllPlayers,
                };
                PlayerAction::CooperativeMagic { target }
            }
            PlayerAction::UseItem {
                item_object,
                target,
                script_entry,
                consuming,
            } => PlayerAction::UseItem {
                item_object,
                target: target.map(|target| preserved_target.unwrap_or(target)),
                script_entry,
                consuming,
            },
            PlayerAction::ThrowItem {
                item_object,
                target,
                script_entry,
            } => PlayerAction::ThrowItem {
                item_object,
                target: target.map(|target| preserved_target.unwrap_or(target)),
                script_entry,
            },
            PlayerAction::Flee => PlayerAction::Flee,
            PlayerAction::Defend => PlayerAction::Defend,
        };
        self.commit_player_action_with_auto_attack(
            player,
            action,
            automatic_attack && matches!(action, PlayerAction::Attack { .. }),
        );
        Some(Vec::new())
    }

    pub fn previous_round_used_auto_attack(&self) -> bool {
        self.previous_auto_attack
    }

    pub fn set_auto_attack_mode(&mut self, enabled: bool) {
        self.auto_attack_mode = enabled;
    }

    /// Preserve the previous action cache when Repeat falls back for an unavailable item.
    pub(crate) fn repeat_unavailable_item(&mut self, thrown: bool) -> Option<Vec<BattleEvent>> {
        if !self.can_commit_player_action() {
            return None;
        }
        let player = self.active_player?;
        self.repeating_round = true;
        let action = if thrown {
            PlayerAction::Attack {
                target: self.first_living_enemy()?,
            }
        } else {
            PlayerAction::Defend
        };
        self.commit_player_action(player, action);
        Some(Vec::new())
    }

    /// Commit flee attempts for every remaining actionable party member.
    pub fn attempt_flee_all(&mut self) -> Option<Vec<BattleEvent>> {
        if self.is_boss || !self.can_commit_player_action() {
            return None;
        }
        for player in 0..self.players.len() {
            if !self.acted[player]
                && self.players[player].is_alive()
                && !self.players[player].statuses.is_active(BattleStatus::Sleep)
                && !self.players[player]
                    .statuses
                    .is_active(BattleStatus::Paralyzed)
                && !self.players[player]
                    .statuses
                    .is_active(BattleStatus::Confused)
            {
                self.acted[player] = true;
                self.player_actions[player] = Some(PlayerAction::Flee);
                self.automatic_player_attacks[player] = false;
            }
        }
        self.active_player = None;
        self.build_action_queue();
        Some(Vec::new())
    }

    pub fn use_item(
        &mut self,
        item_object: u16,
        target: Option<usize>,
        script_entry: u16,
        consuming: bool,
    ) -> Option<Vec<BattleEvent>> {
        if !self.can_commit_player_action()
            || target.is_some_and(|target| target >= self.players.len())
        {
            return None;
        }
        let player = self.active_player?;
        self.commit_player_action(
            player,
            PlayerAction::UseItem {
                item_object,
                target,
                script_entry,
                consuming,
            },
        );
        Some(Vec::new())
    }

    pub fn throw_item(
        &mut self,
        item_object: u16,
        target: Option<usize>,
        script_entry: u16,
    ) -> Option<Vec<BattleEvent>> {
        if !self.can_commit_player_action()
            || target.is_some_and(|target| {
                self.enemies
                    .get(target)
                    .is_none_or(|enemy| !enemy.is_alive())
            })
        {
            return None;
        }
        let player = self.active_player?;
        self.commit_player_action(
            player,
            PlayerAction::ThrowItem {
                item_object,
                target,
                script_entry,
            },
        );
        Some(Vec::new())
    }

    /// Commit a normal Classic-mode flee attempt for the active player.
    pub fn attempt_flee(&mut self) -> Option<Vec<BattleEvent>> {
        if self.is_boss || !self.can_commit_player_action() {
            return None;
        }
        let player = self.active_player?;
        self.commit_player_action(player, PlayerAction::Flee);
        Some(Vec::new())
    }

    pub fn reserved_item_count(&self, item_object: u16) -> u16 {
        let count = self
            .player_actions
            .iter()
            .enumerate()
            .filter(|&(player, action)| {
                self.acted.get(player).copied().unwrap_or(false)
                    && matches!(
                        action,
                        Some(PlayerAction::UseItem {
                            item_object: reserved,
                            consuming: true,
                            ..
                        } | PlayerAction::ThrowItem {
                            item_object: reserved,
                            ..
                        }) if *reserved == item_object
                    )
            })
            .count();
        u16::try_from(count).unwrap_or(u16::MAX)
    }

    fn can_commit_player_action(&self) -> bool {
        self.phase == BattlePhase::AwaitingCommand
            && self.flow == BattleFlow::Command
            && !self.has_script_work()
            && self.active_player.is_some_and(|player| {
                self.players.get(player).is_some_and(BattlePlayer::can_act) && !self.acted[player]
            })
    }

    pub fn flee(&mut self) -> Option<BattleEvent> {
        if self.phase != BattlePhase::AwaitingCommand
            || self.flow != BattleFlow::Command
            || self.is_boss
        {
            return None;
        }
        self.phase = BattlePhase::Finished(BattleResult::Fled);
        self.flow = BattleFlow::Finished;
        self.active_player = None;
        self.pending_scripts.clear();
        Some(BattleEvent::Finished(BattleResult::Fled))
    }
}
