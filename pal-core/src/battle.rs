//! Deterministic, platform-independent battle state and minimum combat rules.

use std::collections::VecDeque;

use pal_assets::battle::{BattleData, BattlePosition, Enemy};
use pal_assets::magic::Magics;
use pal_assets::objects::{GlobalObject, GlobalObjects};
use pal_assets::player_roles::PlayerRole;

pub const BATTLE_STATUS_COUNT: usize = 9;
pub const MAX_BATTLE_POISONS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum BattleStatus {
    Confused = 0,
    Paralyzed = 1,
    Sleep = 2,
    Silence = 3,
    Puppet = 4,
    Bravery = 5,
    Protect = 6,
    Haste = 7,
    DualAttack = 8,
}

impl BattleStatus {
    pub const ALL: [Self; BATTLE_STATUS_COUNT] = [
        Self::Confused,
        Self::Paralyzed,
        Self::Sleep,
        Self::Silence,
        Self::Puppet,
        Self::Bravery,
        Self::Protect,
        Self::Haste,
        Self::DualAttack,
    ];

    pub const fn from_raw(value: u16) -> Option<Self> {
        match value {
            0 => Some(Self::Confused),
            1 => Some(Self::Paralyzed),
            2 => Some(Self::Sleep),
            3 => Some(Self::Silence),
            4 => Some(Self::Puppet),
            5 => Some(Self::Bravery),
            6 => Some(Self::Protect),
            7 => Some(Self::Haste),
            8 => Some(Self::DualAttack),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BattleStatuses {
    durations: [u16; BATTLE_STATUS_COUNT],
}

impl BattleStatuses {
    pub const fn from_durations(durations: [u16; BATTLE_STATUS_COUNT]) -> Self {
        Self { durations }
    }

    pub const fn durations(self) -> [u16; BATTLE_STATUS_COUNT] {
        self.durations
    }

    pub fn duration(&self, status: BattleStatus) -> u16 {
        self.durations[status as usize]
    }

    pub fn is_active(&self, status: BattleStatus) -> bool {
        self.duration(status) != 0
    }

    /// Apply original player-status replacement and alive/dead restrictions.
    pub fn set_for_player(&mut self, status: BattleStatus, rounds: u16, alive: bool) -> bool {
        let duration = &mut self.durations[status as usize];
        match status {
            BattleStatus::Confused
            | BattleStatus::Paralyzed
            | BattleStatus::Sleep
            | BattleStatus::Silence => {
                if *duration == 0 {
                    *duration = rounds;
                }
            }
            BattleStatus::Puppet => {
                if alive {
                    return false;
                }
                *duration = (*duration).max(rounds);
            }
            BattleStatus::Bravery
            | BattleStatus::Protect
            | BattleStatus::Haste
            | BattleStatus::DualAttack => {
                if alive {
                    *duration = (*duration).max(rounds);
                }
            }
        }
        true
    }

    pub fn set_for_enemy(&mut self, status: BattleStatus, rounds: u16) {
        self.durations[status as usize] = rounds;
    }

    /// Equipment statuses use values above 999 and are not removable by scripts.
    pub fn remove_from_player(&mut self, status: BattleStatus) {
        let duration = &mut self.durations[status as usize];
        if *duration <= 999 {
            *duration = 0;
        }
    }

    pub fn decrement_round(&mut self) {
        for duration in &mut self.durations {
            *duration = duration.saturating_sub(1);
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BattlePoison {
    pub object_id: u16,
    pub script_entry: u16,
}

pub fn add_poison(
    poisons: &mut [BattlePoison; MAX_BATTLE_POISONS],
    object_id: u16,
    script_entry: u16,
) -> bool {
    if object_id == 0 {
        return false;
    }
    if poisons.iter().any(|poison| poison.object_id == object_id) {
        return true;
    }
    let Some(slot) = poisons.iter_mut().find(|poison| poison.object_id == 0) else {
        return false;
    };
    *slot = BattlePoison {
        object_id,
        script_entry,
    };
    true
}

pub fn cure_poison(poisons: &mut [BattlePoison; MAX_BATTLE_POISONS], object_id: u16) -> bool {
    let mut removed = false;
    for poison in poisons.iter_mut() {
        if poison.object_id == object_id {
            *poison = BattlePoison::default();
            removed = true;
        }
    }
    compact_poisons(poisons);
    removed
}

pub fn cure_poison_by_level(
    poisons: &mut [BattlePoison; MAX_BATTLE_POISONS],
    maximum_level: u16,
    objects: &GlobalObjects,
) -> bool {
    let mut removed = false;
    for poison in poisons.iter_mut() {
        if poison.object_id != 0
            && objects
                .get(poison.object_id)
                .is_some_and(|object| object.poison_level() <= maximum_level)
        {
            *poison = BattlePoison::default();
            removed = true;
        }
    }
    compact_poisons(poisons);
    removed
}

fn compact_poisons(poisons: &mut [BattlePoison; MAX_BATTLE_POISONS]) {
    let retained = poisons
        .iter()
        .copied()
        .filter(|poison| poison.object_id != 0)
        .collect::<Vec<_>>();
    poisons.fill(BattlePoison::default());
    poisons[..retained.len()].copy_from_slice(&retained);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattleResult {
    Won,
    Lost,
    Fled,
    /// Script-controlled termination continues the caller without victory rewards.
    Terminated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattlePhase {
    AwaitingCommand,
    Finished(BattleResult),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BattleRequest {
    pub enemy_team: u16,
    pub lost_entry: u16,
    pub flee_entry: u16,
    pub is_boss: bool,
}

/// Persistent script entry owned by one battle actor or poison instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattleScriptSource {
    EnemyTurnStart { enemy: usize },
    EnemyReady { enemy: usize },
    EnemyBattleEnd { enemy: usize },
    PlayerPoison { role_id: u16, poison_id: u16 },
    EnemyPoison { enemy: usize, poison_id: u16 },
    EnemyMagicUse { enemy: usize, magic_object: u16 },
    EnemyMagicSuccess { enemy: usize, magic_object: u16 },
}

/// A battle-owned script invocation consumed by a platform script driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BattleScriptRequest {
    pub source: BattleScriptSource,
    pub entry: u16,
    pub object_id: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BattleFlow {
    Command,
    EnemyRound {
        enemy: usize,
        action: u8,
        ready_complete: bool,
    },
    EnemyMagic {
        enemy: usize,
        action: u8,
        target: usize,
        magic: BattleMagic,
        phase: EnemyMagicPhase,
        use_succeeded: bool,
    },
    RoundScripts,
    TurnStartScripts,
    Outcome(BattleResult),
    BattleEndScripts(BattleResult),
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnemyMagicPhase {
    UseScript,
    SuccessScript,
    Damage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BattlePlayer {
    pub role_id: u16,
    pub battle_sprite_num: u16,
    pub name_word_id: u16,
    pub level: u16,
    pub hp: u16,
    pub max_hp: u16,
    pub mp: u16,
    pub max_mp: u16,
    pub attack_strength: u16,
    pub magic_strength: u16,
    pub defense: u16,
    pub dexterity: u16,
    pub poison_resistance: u16,
    pub elemental_resistance: [u16; pal_assets::player_roles::MAGIC_ELEMENT_COUNT],
    pub attacks_all: bool,
    pub magics: Vec<BattleMagic>,
    pub death_sound: u16,
    pub attack_sound: u16,
    pub weapon_sound: u16,
    pub magic_sound: u16,
    pub statuses: BattleStatuses,
    pub poisons: [BattlePoison; MAX_BATTLE_POISONS],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BattleMagic {
    pub object_id: u16,
    pub magic_type: u16,
    pub mp_cost: u16,
    pub base_damage: u16,
    pub elemental: u16,
    pub attacks_all: bool,
    pub sound: i16,
    pub use_script: u16,
    pub success_script: u16,
}

impl BattlePlayer {
    pub fn is_alive(&self) -> bool {
        self.hp > 0
    }

    pub fn can_act(&self) -> bool {
        self.is_combat_active()
            && !self.statuses.is_active(BattleStatus::Sleep)
            && !self.statuses.is_active(BattleStatus::Paralyzed)
    }

    pub fn is_combat_active(&self) -> bool {
        self.is_alive() || self.statuses.is_active(BattleStatus::Puppet)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BattleEnemy {
    pub slot: usize,
    pub object_id: u16,
    pub enemy_id: u16,
    pub position: BattlePosition,
    pub y_offset: u16,
    pub hp: u16,
    pub max_hp: u16,
    pub experience: u16,
    pub cash: u16,
    pub level: u16,
    pub attack_strength: u16,
    pub magic_strength: u16,
    pub defense: u16,
    pub dexterity: u16,
    pub physical_resistance: u16,
    pub poison_resistance: u16,
    pub elemental_resistance: [u16; pal_assets::battle::MAGIC_ELEMENT_COUNT],
    pub idle_frames: u16,
    pub magic_frames: u16,
    pub attack_frames: u16,
    pub idle_animation_speed: u16,
    pub attack_sound: i16,
    pub action_sound: i16,
    pub magic_sound: i16,
    pub death_sound: i16,
    pub call_sound: i16,
    pub turn_start_script: u16,
    pub battle_end_script: u16,
    pub ready_script: u16,
    pub magic_object: u16,
    pub magic_rate: u16,
    pub magic: Option<BattleMagic>,
    pub attack_equivalent_item: u16,
    pub attack_equivalent_item_rate: u16,
    pub steal_item: u16,
    pub steal_item_count: u16,
    pub dual_move: bool,
    pub collect_value: u16,
    pub sorcery_resistance: u16,
    pub statuses: BattleStatuses,
    pub poisons: [BattlePoison; MAX_BATTLE_POISONS],
}

impl BattleEnemy {
    pub fn is_alive(&self) -> bool {
        self.hp > 0
    }

    pub fn can_act(&self) -> bool {
        self.is_alive()
            && !self.statuses.is_active(BattleStatus::Sleep)
            && !self.statuses.is_active(BattleStatus::Paralyzed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattleEvent {
    PlayerAttack {
        player: usize,
        enemy: usize,
        damage: u16,
        defeated: bool,
    },
    PlayerMagic {
        player: usize,
        enemy: usize,
        magic_object: u16,
        damage: u16,
        defeated: bool,
    },
    EnemyAttack {
        enemy: usize,
        player: usize,
        damage: u16,
        defeated: bool,
    },
    EnemyMagic {
        enemy: usize,
        player: usize,
        magic_object: u16,
        damage: u16,
        defeated: bool,
    },
    RoundCompleted,
    Finished(BattleResult),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BattleRewards {
    pub experience: u32,
    pub cash: u32,
}

/// A minimal turn-based battle used by the first complete combat milestone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BattleState {
    pub enemy_team: u16,
    pub battlefield: u16,
    pub music: u16,
    pub is_boss: bool,
    pub players: Vec<BattlePlayer>,
    pub enemies: Vec<BattleEnemy>,
    phase: BattlePhase,
    active_player: Option<usize>,
    acted: Vec<bool>,
    round: u32,
    random_state: u32,
    battlefield_magic_effect: [i16; pal_assets::battle::MAGIC_ELEMENT_COUNT],
    flow: BattleFlow,
    pending_scripts: VecDeque<BattleScriptRequest>,
    active_script: Option<BattleScriptRequest>,
}

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
                let object = objects.get(object_id)?;
                let enemy_id = object.enemy_id();
                let enemy = data.enemies.get(enemy_id)?;
                let position = data.enemy_positions.get(layout.len(), slot)?;
                let magic = match enemy.magic {
                    0 | u16::MAX => None,
                    object_id => Some(battle_magic(object_id, objects, magics)?),
                };
                Some(battle_enemy(
                    slot, position, object_id, enemy_id, enemy, object, magic,
                ))
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
        let active_player = next_player(&players, &acted, 0);
        let phase = if active_player.is_some() {
            BattlePhase::AwaitingCommand
        } else {
            BattlePhase::Finished(BattleResult::Lost)
        };
        let pending_scripts = if phase == BattlePhase::AwaitingCommand {
            enemies
                .iter()
                .enumerate()
                .filter_map(|(enemy, actor)| {
                    (actor.turn_start_script != 0).then_some(BattleScriptRequest {
                        source: BattleScriptSource::EnemyTurnStart { enemy },
                        entry: actor.turn_start_script,
                        object_id: u16::try_from(enemy).ok()?,
                    })
                })
                .collect()
        } else {
            VecDeque::new()
        };
        Some(Self {
            enemy_team: request.enemy_team,
            battlefield,
            music,
            is_boss: request.is_boss,
            players,
            enemies,
            phase,
            active_player,
            acted,
            round: 1,
            random_state: 0x6d2b_79f5,
            battlefield_magic_effect: battlefield_definition.magic_effect,
            flow: if phase == BattlePhase::AwaitingCommand {
                BattleFlow::Command
            } else {
                BattleFlow::Finished
            },
            pending_scripts,
            active_script: None,
        })
    }

    pub fn phase(&self) -> BattlePhase {
        self.phase
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

    /// Skip players disabled by sleep/paralysis and run an enemy-only round when needed.
    pub fn advance_automatic_turns(&mut self) -> Vec<BattleEvent> {
        if self.phase != BattlePhase::AwaitingCommand
            || self.flow != BattleFlow::Command
            || !self.pending_scripts.is_empty()
            || self.active_script.is_some()
            || self
                .active_player
                .is_some_and(|index| self.players[index].can_act() && !self.acted[index])
        {
            return Vec::new();
        }
        let start = self.active_player.map_or(0, |index| {
            self.acted[index] = true;
            index + 1
        });
        self.active_player = next_player(&self.players, &self.acted, start);
        if self.active_player.is_some() {
            return Vec::new();
        }
        self.flow = BattleFlow::EnemyRound {
            enemy: 0,
            action: 0,
            ready_complete: false,
        };
        self.advance_resolution()
    }

    pub fn has_script_work(&self) -> bool {
        self.active_script.is_some() || !self.pending_scripts.is_empty()
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
        }
        self.detect_script_outcome();
        true
    }

    /// Advance one non-interactive battle resolution step.
    pub fn advance_resolution(&mut self) -> Vec<BattleEvent> {
        if self.phase != BattlePhase::AwaitingCommand || self.has_script_work() {
            return Vec::new();
        }
        let mut events = Vec::new();
        loop {
            match self.flow {
                BattleFlow::Command | BattleFlow::Finished => return events,
                BattleFlow::EnemyRound {
                    enemy,
                    action,
                    ready_complete,
                } => {
                    let Some(actor) = self.enemies.get(enemy) else {
                        self.queue_round_poison_scripts();
                        self.flow = BattleFlow::RoundScripts;
                        if self.has_script_work() {
                            return events;
                        }
                        continue;
                    };
                    let action_count = if actor.dual_move { 2 } else { 1 };
                    if !actor.can_act() || action >= action_count {
                        self.flow = BattleFlow::EnemyRound {
                            enemy: enemy + 1,
                            action: 0,
                            ready_complete: false,
                        };
                        continue;
                    }
                    if !ready_complete && actor.ready_script != 0 {
                        let Some(object_id) = u16::try_from(enemy).ok() else {
                            self.flow = BattleFlow::Outcome(BattleResult::Lost);
                            continue;
                        };
                        self.pending_scripts.push_back(BattleScriptRequest {
                            source: BattleScriptSource::EnemyReady { enemy },
                            entry: actor.ready_script,
                            object_id,
                        });
                        self.flow = BattleFlow::EnemyRound {
                            enemy,
                            action,
                            ready_complete: true,
                        };
                        return events;
                    }
                    let magic_object = actor.magic_object;
                    let magic_rate = actor.magic_rate;
                    let silenced = actor.statuses.is_active(BattleStatus::Silence);
                    let magic = actor.magic;
                    if magic_object != 0 && !silenced && self.random(10) < u32::from(magic_rate) {
                        if magic_object == u16::MAX {
                            self.flow = BattleFlow::EnemyRound {
                                enemy,
                                action: action + 1,
                                ready_complete: false,
                            };
                            return events;
                        }
                        if let Some(magic) = magic {
                            let living_players = self
                                .players
                                .iter()
                                .enumerate()
                                .filter_map(|(index, player)| player.is_alive().then_some(index))
                                .collect::<Vec<_>>();
                            let Some(&target) = living_players
                                .get(self.random(living_players.len() as u32) as usize)
                            else {
                                self.flow = BattleFlow::Outcome(BattleResult::Lost);
                                continue;
                            };
                            self.flow = BattleFlow::EnemyMagic {
                                enemy,
                                action,
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
                    self.flow = BattleFlow::EnemyRound {
                        enemy,
                        action: action + 1,
                        ready_complete: false,
                    };
                    if let Some(event) = self.perform_enemy_action(enemy) {
                        events.push(event);
                    }
                    if self.players.iter().all(|player| !player.is_combat_active()) {
                        self.flow = BattleFlow::Outcome(BattleResult::Lost);
                    }
                    return events;
                }
                BattleFlow::EnemyMagic {
                    enemy,
                    action,
                    target,
                    magic,
                    phase,
                    use_succeeded,
                } => match phase {
                    EnemyMagicPhase::UseScript => {
                        if use_succeeded && magic.success_script != 0 {
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
                                action,
                                target,
                                magic,
                                phase: EnemyMagicPhase::SuccessScript,
                                use_succeeded,
                            };
                            return events;
                        }
                        self.flow = BattleFlow::EnemyMagic {
                            enemy,
                            action,
                            target,
                            magic,
                            phase: EnemyMagicPhase::Damage,
                            use_succeeded,
                        };
                    }
                    EnemyMagicPhase::SuccessScript => {
                        self.flow = BattleFlow::EnemyMagic {
                            enemy,
                            action,
                            target,
                            magic,
                            phase: EnemyMagicPhase::Damage,
                            use_succeeded,
                        };
                    }
                    EnemyMagicPhase::Damage => {
                        events.extend(self.perform_enemy_magic(enemy, target, magic));
                        self.flow = BattleFlow::EnemyRound {
                            enemy,
                            action: action + 1,
                            ready_complete: false,
                        };
                        if self.players.iter().all(|player| !player.is_combat_active()) {
                            self.flow = BattleFlow::Outcome(BattleResult::Lost);
                        }
                        return events;
                    }
                },
                BattleFlow::RoundScripts => {
                    for player in &mut self.players {
                        player.statuses.decrement_round();
                    }
                    for enemy in &mut self.enemies {
                        enemy.statuses.decrement_round();
                    }
                    if self.enemies.iter().all(|enemy| !enemy.is_alive()) {
                        self.flow = BattleFlow::Outcome(BattleResult::Won);
                        continue;
                    }
                    if self.players.iter().all(|player| !player.is_combat_active()) {
                        self.flow = BattleFlow::Outcome(BattleResult::Lost);
                        continue;
                    }
                    self.queue_turn_start_scripts();
                    self.flow = BattleFlow::TurnStartScripts;
                    if self.has_script_work() {
                        return events;
                    }
                }
                BattleFlow::TurnStartScripts => {
                    self.round = self.round.saturating_add(1);
                    self.acted.fill(false);
                    self.active_player = next_player(&self.players, &self.acted, 0);
                    self.flow = BattleFlow::Command;
                    events.push(BattleEvent::RoundCompleted);
                    return events;
                }
                BattleFlow::Outcome(result) => {
                    self.finish(result, &mut events);
                    return events;
                }
                BattleFlow::BattleEndScripts(result) => {
                    self.phase = BattlePhase::Finished(result);
                    self.flow = BattleFlow::Finished;
                    events.push(BattleEvent::Finished(result));
                    return events;
                }
            }
        }
    }

    pub fn first_living_enemy(&self) -> Option<usize> {
        self.enemies.iter().position(BattleEnemy::is_alive)
    }

    pub fn rewards(&self) -> BattleRewards {
        BattleRewards {
            experience: self
                .enemies
                .iter()
                .map(|enemy| u32::from(enemy.experience))
                .sum(),
            cash: self.enemies.iter().map(|enemy| u32::from(enemy.cash)).sum(),
        }
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
            for enemy in &mut self.enemies {
                enemy.hp = enemy.hp.saturating_sub(amount);
            }
            return true;
        }
        let Some(enemy) = self.enemies.get_mut(enemy_index) else {
            return false;
        };
        enemy.hp = enemy.hp.saturating_sub(amount);
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
        enemy.hp = enemy.hp.saturating_sub(amount);
        let player = &mut self.players[player_index];
        player.hp = player.hp.saturating_add(amount).min(player.max_hp);
        true
    }

    pub fn halve_enemy_hp(&mut self, enemy_index: usize, maximum_damage: u16) -> bool {
        if self.phase != BattlePhase::AwaitingCommand {
            return false;
        }
        let Some(enemy) = self.enemies.get_mut(enemy_index) else {
            return false;
        };
        let damage = (enemy.hp / 2).saturating_add(1).min(maximum_damage);
        enemy.hp = enemy.hp.saturating_sub(damage);
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
        Some(
            u32::from(enemy.hp).saturating_mul(100)
                > u32::from(enemy.max_hp).saturating_mul(u32::from(percentage)),
        )
    }

    pub fn enemy_not_first_kind(&self, enemy_index: usize) -> Option<bool> {
        let enemy = self.enemies.get(enemy_index)?;
        Some(
            self.enemies[..enemy_index]
                .iter()
                .any(|other| other.is_alive() && other.object_id == enemy.object_id),
        )
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
        self.pending_scripts.clear();
        self.active_player = None;
        self.flow = BattleFlow::Outcome(result);
        true
    }

    pub fn enemy_escape(&mut self) -> bool {
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
            (0..self.enemies.len()).collect::<Vec<_>>()
        } else if enemy_index < self.enemies.len() {
            vec![enemy_index]
        } else {
            return false;
        };
        for target in targets {
            let resistance = self.enemies[target].sorcery_resistance.min(9);
            if self.random(10) >= u32::from(resistance) {
                let already_present = self.enemies[target]
                    .poisons
                    .iter()
                    .any(|poison| poison.object_id == poison_id);
                if add_poison(&mut self.enemies[target].poisons, poison_id, script_entry)
                    && !already_present
                    && script_entry != 0
                {
                    let Some(object_id) = u16::try_from(target).ok() else {
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
            for enemy in &mut self.enemies {
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

    /// Returns `Some(false)` when a valid status was resisted.
    pub fn set_enemy_status(
        &mut self,
        enemy_index: usize,
        status: BattleStatus,
        rounds: u16,
    ) -> Option<bool> {
        if self.phase != BattlePhase::AwaitingCommand || enemy_index >= self.enemies.len() {
            return None;
        }
        let resistance = self.enemies[enemy_index].sorcery_resistance.min(9);
        if self.random(10) <= u32::from(resistance) {
            return Some(false);
        }
        self.enemies[enemy_index]
            .statuses
            .set_for_enemy(status, rounds);
        Some(true)
    }

    /// Execute a normal attack and, after every living player has acted, the enemy round.
    pub fn attack(&mut self, target: usize) -> Option<Vec<BattleEvent>> {
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

        let mut events = Vec::new();
        let attack_count = if self.players[player_index]
            .statuses
            .is_active(BattleStatus::DualAttack)
        {
            2
        } else {
            1
        };
        for _ in 0..attack_count {
            let targets = if self.players[player_index].attacks_all {
                self.enemies
                    .iter()
                    .enumerate()
                    .filter_map(|(index, enemy)| enemy.is_alive().then_some(index))
                    .collect::<Vec<_>>()
            } else if self.enemies[target].is_alive() {
                vec![target]
            } else {
                Vec::new()
            };
            for enemy_index in targets {
                let damage = self.player_damage(player_index, enemy_index);
                let enemy = &mut self.enemies[enemy_index];
                enemy.hp = enemy.hp.saturating_sub(damage);
                events.push(BattleEvent::PlayerAttack {
                    player: player_index,
                    enemy: enemy_index,
                    damage,
                    defeated: !enemy.is_alive(),
                });
            }
        }
        self.end_player_action(player_index, &mut events);
        Some(events)
    }

    pub fn cast_magic(&mut self, magic_index: usize, target: usize) -> Option<Vec<BattleEvent>> {
        if self.phase != BattlePhase::AwaitingCommand
            || self.flow != BattleFlow::Command
            || self.has_script_work()
            || !self.enemies.get(target)?.is_alive()
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
        if self.players[player_index].mp < magic.mp_cost {
            return None;
        }
        self.players[player_index].mp -= magic.mp_cost;

        let targets = if magic.attacks_all {
            self.enemies
                .iter()
                .enumerate()
                .filter_map(|(index, enemy)| enemy.is_alive().then_some(index))
                .collect::<Vec<_>>()
        } else {
            vec![target]
        };
        let mut events = Vec::new();
        for enemy_index in targets {
            let damage = self.magic_damage(player_index, enemy_index, magic);
            let enemy = &mut self.enemies[enemy_index];
            enemy.hp = enemy.hp.saturating_sub(damage);
            events.push(BattleEvent::PlayerMagic {
                player: player_index,
                enemy: enemy_index,
                magic_object: magic.object_id,
                damage,
                defeated: !enemy.is_alive(),
            });
        }
        self.end_player_action(player_index, &mut events);
        Some(events)
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

    fn perform_enemy_action(&mut self, enemy: usize) -> Option<BattleEvent> {
        if !self.enemies.get(enemy)?.can_act() {
            return None;
        }
        let living_players = self
            .players
            .iter()
            .enumerate()
            .filter_map(|(index, player)| player.is_alive().then_some(index))
            .collect::<Vec<_>>();
        let player = living_players
            .get(self.random(living_players.len() as u32) as usize)
            .copied()?;
        let damage = self.enemy_damage(enemy, player);
        let target = &mut self.players[player];
        target.hp = target.hp.saturating_sub(damage);
        Some(BattleEvent::EnemyAttack {
            enemy,
            player,
            damage,
            defeated: !target.is_alive(),
        })
    }

    fn perform_enemy_magic(
        &mut self,
        enemy: usize,
        target: usize,
        magic: BattleMagic,
    ) -> Vec<BattleEvent> {
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
        for player in targets {
            let damage = self
                .enemy_magic_damage(enemy, player, magic)
                .min(self.players[player].hp);
            let target = &mut self.players[player];
            target.hp = target.hp.saturating_sub(damage);
            events.push(BattleEvent::EnemyMagic {
                enemy,
                player,
                magic_object: magic.object_id,
                damage,
                defeated: !target.is_alive(),
            });
        }
        events
    }

    fn end_player_action(&mut self, player_index: usize, events: &mut Vec<BattleEvent>) {
        self.acted[player_index] = true;
        if self.enemies.iter().all(|enemy| !enemy.is_alive()) {
            self.finish(BattleResult::Won, events);
            return;
        }
        self.active_player = next_player(&self.players, &self.acted, player_index + 1);
        if self.active_player.is_some() {
            return;
        }
        self.flow = BattleFlow::EnemyRound {
            enemy: 0,
            action: 0,
            ready_complete: false,
        };
    }

    fn queue_round_poison_scripts(&mut self) {
        let player_scripts = self.players.iter().flat_map(|player| {
            player.poisons.iter().filter_map(|poison| {
                (poison.object_id != 0 && poison.script_entry != 0).then_some(BattleScriptRequest {
                    source: BattleScriptSource::PlayerPoison {
                        role_id: player.role_id,
                        poison_id: poison.object_id,
                    },
                    entry: poison.script_entry,
                    object_id: player.role_id,
                })
            })
        });
        let enemy_scripts = self.enemies.iter().enumerate().flat_map(|(enemy, actor)| {
            actor.poisons.iter().filter_map(move |poison| {
                let object_id = u16::try_from(enemy).ok()?;
                (poison.object_id != 0 && poison.script_entry != 0).then_some(BattleScriptRequest {
                    source: BattleScriptSource::EnemyPoison {
                        enemy,
                        poison_id: poison.object_id,
                    },
                    entry: poison.script_entry,
                    object_id,
                })
            })
        });
        self.pending_scripts
            .extend(player_scripts.chain(enemy_scripts));
    }

    fn queue_turn_start_scripts(&mut self) {
        self.pending_scripts
            .extend(
                self.enemies
                    .iter()
                    .enumerate()
                    .filter_map(|(enemy, actor)| {
                        if actor.is_alive() && actor.turn_start_script != 0 {
                            Some(BattleScriptRequest {
                                source: BattleScriptSource::EnemyTurnStart { enemy },
                                entry: actor.turn_start_script,
                                object_id: u16::try_from(enemy).ok()?,
                            })
                        } else {
                            None
                        }
                    }),
            );
    }

    fn queue_battle_end_scripts(&mut self) {
        self.pending_scripts
            .extend(
                self.enemies
                    .iter()
                    .enumerate()
                    .filter_map(|(enemy, actor)| {
                        if actor.battle_end_script != 0 {
                            Some(BattleScriptRequest {
                                source: BattleScriptSource::EnemyBattleEnd { enemy },
                                entry: actor.battle_end_script,
                                object_id: u16::try_from(enemy).ok()?,
                            })
                        } else {
                            None
                        }
                    }),
            );
    }

    fn detect_script_outcome(&mut self) {
        if self.phase != BattlePhase::AwaitingCommand
            || matches!(
                self.flow,
                BattleFlow::Outcome(_) | BattleFlow::BattleEndScripts(_) | BattleFlow::Finished
            )
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

    fn player_damage(&mut self, player: usize, enemy: usize) -> u16 {
        let attacker = &self.players[player];
        let defender = &self.enemies[enemy];
        let defense = u32::from(defender.defense)
            .saturating_add(u32::from(defender.level.saturating_add(6)) * 4);
        let mut damage = physical_damage(
            u32::from(attacker.attack_strength),
            defense,
            u32::from(defender.physical_resistance),
        )
        .saturating_add(1 + self.random(2));
        if self.players[player]
            .statuses
            .is_active(BattleStatus::Bravery)
        {
            damage = damage.saturating_mul(3);
        }
        u16::try_from(damage.max(1)).unwrap_or(u16::MAX)
    }

    fn enemy_damage(&mut self, enemy: usize, player: usize) -> u16 {
        let attacker = &self.enemies[enemy];
        let raw_strength = i32::from(attacker.attack_strength as i16)
            + i32::from(attacker.level.saturating_add(6)) * 6;
        let attack = u32::try_from(raw_strength.max(0)).unwrap_or(0) + self.random(3);
        let mut damage = physical_damage(attack, u32::from(self.players[player].defense), 2)
            .saturating_add(self.random(2));
        if self.players[player]
            .statuses
            .is_active(BattleStatus::Protect)
        {
            damage /= 2;
        }
        u16::try_from(damage.max(1)).unwrap_or(u16::MAX)
    }

    fn magic_damage(&mut self, player: usize, enemy: usize, magic: BattleMagic) -> u16 {
        let strength =
            u32::from(self.players[player].magic_strength).saturating_mul(10 + self.random(2)) / 10;
        let defender = &self.enemies[enemy];
        let defense = u32::from(defender.defense)
            .saturating_add(u32::from(defender.level.saturating_add(6)) * 4);
        let mut damage = physical_damage(strength, defense, 0) / 4 + u32::from(magic.base_damage);
        let element = usize::from(magic.elemental);
        if (1..=pal_assets::battle::MAGIC_ELEMENT_COUNT).contains(&element) {
            let resistance = u32::from(defender.elemental_resistance[element - 1]);
            damage = damage.saturating_mul(10u32.saturating_sub(resistance)) / 5;
            let field = i32::from(self.battlefield_magic_effect[element - 1]) + 10;
            damage = damage.saturating_mul(u32::try_from(field.max(0)).unwrap_or(0)) / 10;
        } else if element > pal_assets::battle::MAGIC_ELEMENT_COUNT {
            damage = damage
                .saturating_mul(10u32.saturating_sub(u32::from(defender.poison_resistance)))
                / 5;
        }
        u16::try_from(damage.max(1)).unwrap_or(u16::MAX)
    }

    fn enemy_magic_damage(&mut self, enemy: usize, player: usize, magic: BattleMagic) -> u16 {
        let attacker = &self.enemies[enemy];
        let raw_strength = i32::from(attacker.magic_strength as i16)
            + i32::from(attacker.level.saturating_add(6)) * 6;
        let strength = u32::try_from(raw_strength.max(0))
            .unwrap_or(0)
            .saturating_mul(10 + self.random(2))
            / 10;
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
        if defender.statuses.is_active(BattleStatus::Protect) {
            damage /= 2;
        }
        u16::try_from(damage.max(1)).unwrap_or(u16::MAX)
    }

    fn random(&mut self, upper_exclusive: u32) -> u32 {
        if upper_exclusive <= 1 {
            return 0;
        }
        let mut x = self.random_state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.random_state = x;
        x % upper_exclusive
    }

    fn finish(&mut self, result: BattleResult, events: &mut Vec<BattleEvent>) {
        self.active_player = None;
        self.pending_scripts.clear();
        if result == BattleResult::Won {
            self.queue_battle_end_scripts();
        }
        if self.pending_scripts.is_empty() {
            self.phase = BattlePhase::Finished(result);
            self.flow = BattleFlow::Finished;
            events.push(BattleEvent::Finished(result));
        } else {
            self.flow = BattleFlow::BattleEndScripts(result);
        }
    }
}

fn battle_enemy(
    slot: usize,
    position: BattlePosition,
    object_id: u16,
    enemy_id: u16,
    enemy: &Enemy,
    object: &GlobalObject,
    magic: Option<BattleMagic>,
) -> BattleEnemy {
    BattleEnemy {
        slot,
        object_id,
        enemy_id,
        position,
        y_offset: enemy.y_offset,
        hp: enemy.health,
        max_hp: enemy.health,
        experience: enemy.experience,
        cash: enemy.cash,
        level: enemy.level,
        attack_strength: enemy.attack_strength,
        magic_strength: enemy.magic_strength,
        defense: enemy.defense,
        dexterity: enemy.dexterity,
        physical_resistance: enemy.physical_resistance,
        poison_resistance: enemy.poison_resistance,
        elemental_resistance: enemy.elemental_resistance,
        idle_frames: enemy.idle_frames,
        magic_frames: enemy.magic_frames,
        attack_frames: enemy.attack_frames,
        idle_animation_speed: enemy.idle_animation_speed,
        attack_sound: enemy.attack_sound,
        action_sound: enemy.action_sound,
        magic_sound: enemy.magic_sound,
        death_sound: enemy.death_sound,
        call_sound: enemy.call_sound,
        turn_start_script: object.enemy_turn_start_script(),
        battle_end_script: object.enemy_battle_end_script(),
        ready_script: object.enemy_ready_script(),
        magic_object: enemy.magic,
        magic_rate: enemy.magic_rate,
        magic,
        attack_equivalent_item: enemy.attack_equivalent_item,
        attack_equivalent_item_rate: enemy.attack_equivalent_item_rate,
        steal_item: enemy.steal_item,
        steal_item_count: enemy.steal_item_count,
        dual_move: enemy.dual_move,
        collect_value: enemy.collect_value,
        sorcery_resistance: object.enemy_sorcery_resistance(),
        statuses: BattleStatuses::default(),
        poisons: [BattlePoison::default(); MAX_BATTLE_POISONS],
    }
}

fn battle_magic(object_id: u16, objects: &GlobalObjects, magics: &Magics) -> Option<BattleMagic> {
    let object = objects.get(object_id)?;
    let definition = magics.get(object.magic_number())?;
    Some(BattleMagic {
        object_id,
        magic_type: definition.magic_type,
        mp_cost: definition.mp_cost,
        base_damage: definition.base_damage,
        elemental: definition.elemental,
        attacks_all: definition.magic_type != 0,
        sound: definition.sound,
        use_script: object.magic_use_script(),
        success_script: object.magic_success_script(),
    })
}

fn battle_player(
    role_id: u16,
    role: &PlayerRole,
    objects: &GlobalObjects,
    magics: &Magics,
) -> Option<BattlePlayer> {
    let battle_magics = role
        .magic
        .iter()
        .copied()
        .filter(|&object_id| object_id != 0)
        .filter_map(|object_id| {
            let magic = battle_magic(object_id, objects, magics)?;
            (magic.magic_type <= 3 && magic.base_damage > 0).then_some(magic)
        })
        .collect();
    Some(BattlePlayer {
        role_id,
        battle_sprite_num: role.battle_sprite_num,
        name_word_id: role.name_word_id,
        level: role.level,
        hp: role.hp,
        max_hp: role.max_hp,
        mp: role.mp,
        max_mp: role.max_mp,
        attack_strength: role.attack_strength,
        magic_strength: role.magic_strength,
        defense: role.defense,
        dexterity: role.dexterity,
        poison_resistance: role.poison_resistance,
        elemental_resistance: role.elemental_resistance,
        attacks_all: role.attack_all,
        magics: battle_magics,
        death_sound: role.death_sound,
        attack_sound: role.attack_sound,
        weapon_sound: role.weapon_sound,
        magic_sound: role.magic_sound,
        statuses: BattleStatuses::default(),
        poisons: [BattlePoison::default(); MAX_BATTLE_POISONS],
    })
}

fn next_player(players: &[BattlePlayer], acted: &[bool], start: usize) -> Option<usize> {
    (start..players.len()).find(|&index| players[index].can_act() && !acted[index])
}

fn physical_damage(attack: u32, defense: u32, resistance: u32) -> u32 {
    let base = if attack > defense {
        attack
            .saturating_mul(2)
            .saturating_sub((defense.saturating_mul(8) + 2) / 5)
    } else if attack.saturating_mul(5) > defense.saturating_mul(3) {
        attack.saturating_sub((defense.saturating_mul(3) + 2) / 5)
    } else {
        0
    };
    base.checked_div(resistance).unwrap_or(base)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pal_assets::objects::{GlobalObjects, ObjectLayout};

    const ENEMY_BYTES: usize = 70;

    fn words(values: &[u16]) -> Vec<u8> {
        values.iter().flat_map(|word| word.to_le_bytes()).collect()
    }

    fn make_mkf(chunks: &[Vec<u8>]) -> Vec<u8> {
        let table_size = (chunks.len() + 1) * 4;
        let mut offset = table_size as u32;
        let mut data = Vec::new();
        data.extend_from_slice(&offset.to_le_bytes());
        for chunk in chunks {
            offset += chunk.len() as u32;
            data.extend_from_slice(&offset.to_le_bytes());
        }
        for chunk in chunks {
            data.extend_from_slice(chunk);
        }
        data
    }

    fn fixture(
        enemy_hp: u16,
        enemy_attack: u16,
        player_hp: u16,
    ) -> (BattleData, GlobalObjects, Magics, PlayerRole) {
        fixture_with_enemy_magic(enemy_hp, enemy_attack, player_hp, 0, 0, 0)
    }

    fn fixture_with_enemy_magic(
        enemy_hp: u16,
        enemy_attack: u16,
        player_hp: u16,
        enemy_magic: u16,
        magic_rate: u16,
        magic_type: u16,
    ) -> (BattleData, GlobalObjects, Magics, PlayerRole) {
        let mut chunks = vec![Vec::new(); 15];
        let mut enemy = vec![0; ENEMY_BYTES];
        for (word, value) in [
            (11, enemy_hp),
            (12, 26),
            (13, 48),
            (14, 2),
            (15, enemy_magic),
            (16, magic_rate),
            (17, 9),
            (18, 3),
            (19, 8),
            (20, 2),
            (21, enemy_attack),
            (22, 30),
            (23, 0),
            (32, 1),
            (33, 1),
            (34, 5),
        ] {
            enemy[word * 2..word * 2 + 2].copy_from_slice(&value.to_le_bytes());
        }
        chunks[1] = enemy;
        chunks[2] = words(&[1, 1, u16::MAX, u16::MAX, u16::MAX]);
        chunks[5] = vec![0; 12];
        chunks[6] = vec![0; 20];
        chunks[13] = (0..25u16)
            .flat_map(|index| [index + 10, index + 20])
            .flat_map(u16::to_le_bytes)
            .collect();
        chunks[14] = vec![0; 200];
        let battle_data = BattleData::parse(&make_mkf(&chunks)).unwrap();

        let objects = GlobalObjects::parse(
            &words(&[
                0, 0, 0, 0, 0, 0, // object 0
                0, 0, 11, 12, 13, 0, // enemy object 1 -> enemy definition 0
                0, 0, 32, 31, 0, 0, // magic object 2 -> definition 0 and scripts
            ]),
            ObjectLayout::Dos,
        )
        .unwrap();

        let role = PlayerRole {
            avatar: 0,
            battle_sprite_num: 0,
            scene_sprite_num: 0,
            name_word_id: 0,
            attack_all: false,
            level: 1,
            max_hp: player_hp,
            max_mp: 0,
            hp: player_hp,
            mp: 0,
            equipment: [0; 6],
            attack_strength: 80,
            magic_strength: 0,
            defense: 20,
            dexterity: 20,
            flee_rate: 20,
            poison_resistance: 0,
            elemental_resistance: [0; 5],
            covered_by: 0,
            magic: [0; 32],
            walk_frames: 3,
            cooperative_magic: 0,
            unknown_5: 0,
            unknown_6: 0,
            death_sound: 21,
            attack_sound: 22,
            weapon_sound: 23,
            critical_sound: 0,
            magic_sound: 24,
            cover_sound: 0,
            dying_sound: 0,
        };
        let mut magic_data = [0; 32];
        magic_data[2..4].copy_from_slice(&magic_type.to_le_bytes());
        magic_data[24..26].copy_from_slice(&5u16.to_le_bytes());
        magic_data[26..28].copy_from_slice(&50u16.to_le_bytes());
        magic_data[30..32].copy_from_slice(&9i16.to_le_bytes());
        let magics = Magics::parse(&magic_data).unwrap();
        (battle_data, objects, magics, role)
    }

    fn request(is_boss: bool) -> BattleRequest {
        BattleRequest {
            enemy_team: 0,
            lost_entry: 40,
            flee_entry: 50,
            is_boss,
        }
    }

    fn complete_pending_scripts(battle: &mut BattleState) {
        while let Some(request) = battle.take_script_request() {
            assert!(battle.complete_script(request.entry));
        }
    }

    fn resolve_until_input_or_finish(battle: &mut BattleState) -> Vec<BattleEvent> {
        let mut events = Vec::new();
        for _ in 0..128 {
            complete_pending_scripts(battle);
            events.extend(battle.advance_resolution());
            if battle.phase() != BattlePhase::AwaitingCommand
                || (battle.flow == BattleFlow::Command && !battle.has_script_work())
            {
                return events;
            }
        }
        panic!("battle resolution did not settle");
    }

    #[test]
    fn creates_enemy_instances_from_team_objects_and_positions() {
        let (data, objects, magics, role) = fixture_with_enemy_magic(100, 10, 100, 2, 7, 0);
        let battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        assert_eq!(battle.players.len(), 1);
        assert_eq!(battle.enemies.len(), 2);
        assert_eq!(battle.enemies[0].position, BattlePosition { x: 11, y: 21 });
        assert_eq!(battle.enemies[1].position, BattlePosition { x: 16, y: 26 });
        assert_eq!(battle.enemies[0].turn_start_script, 11);
        assert_eq!(battle.enemies[0].magic_object, 2);
        assert_eq!(battle.enemies[0].magic_rate, 7);
        assert_eq!(battle.enemies[0].magic_strength, 30);
        assert!(battle.enemies[0].dual_move);
        assert_eq!(battle.enemies[0].collect_value, 5);
        assert_eq!(battle.players[0].death_sound, 21);
        assert_eq!(battle.players[0].attack_sound, 22);
        assert_eq!(battle.players[0].weapon_sound, 23);
        assert_eq!(battle.players[0].magic_sound, 24);
        assert_eq!(
            battle.rewards(),
            BattleRewards {
                experience: 52,
                cash: 96
            }
        );
    }

    #[test]
    fn attacks_kill_enemies_and_finish_with_victory() {
        let (data, objects, magics, role) = fixture(1, 0, 100);
        let mut battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut battle);
        let first = battle.attack(0).unwrap();
        assert!(matches!(
            first[0],
            BattleEvent::PlayerAttack { defeated: true, .. }
        ));
        assert_eq!(battle.phase(), BattlePhase::AwaitingCommand);
        resolve_until_input_or_finish(&mut battle);
        let second = battle.attack(1).unwrap();
        let mut finished = second;
        finished.extend(resolve_until_input_or_finish(&mut battle));
        assert_eq!(
            finished.last(),
            Some(&BattleEvent::Finished(BattleResult::Won))
        );
        assert_eq!(battle.phase(), BattlePhase::Finished(BattleResult::Won));
        assert_eq!(
            battle.settled_rewards(),
            Some(BattleRewards {
                experience: 52,
                cash: 96,
            })
        );
        assert!(battle.attack(1).is_none());
    }

    #[test]
    fn enemy_round_can_defeat_the_party() {
        let (data, objects, magics, mut role) = fixture(500, 500, 1);
        role.attack_strength = 1;
        let mut battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut battle);
        let mut events = battle.attack(0).unwrap();
        events.extend(resolve_until_input_or_finish(&mut battle));
        assert!(events
            .iter()
            .any(|event| matches!(event, BattleEvent::EnemyAttack { defeated: true, .. })));
        assert_eq!(
            events.last(),
            Some(&BattleEvent::Finished(BattleResult::Lost))
        );
        assert_eq!(battle.phase(), BattlePhase::Finished(BattleResult::Lost));
        assert_eq!(battle.settled_rewards(), Some(BattleRewards::default()));
    }

    #[test]
    fn only_non_boss_battles_can_flee() {
        let (data, objects, magics, role) = fixture(100, 10, 100);
        let mut boss =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        assert!(boss.flee().is_none());
        let mut normal =
            BattleState::new(request(false), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        assert_eq!(
            normal.flee(),
            Some(BattleEvent::Finished(BattleResult::Fled))
        );
        assert_eq!(normal.settled_rewards(), Some(BattleRewards::default()));
    }

    #[test]
    fn scripted_termination_continues_without_victory_rewards() {
        let (data, objects, magics, role) = fixture(100, 10, 100);
        let mut battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut battle);
        assert!(!battle.enemy_not_first_kind(0).unwrap());
        assert!(battle.enemy_not_first_kind(1).unwrap());
        assert!(battle.set_enemy_magic(0, 2, 0, &objects, &magics));
        assert_eq!(battle.enemies[0].magic_object, 2);
        assert_eq!(battle.enemies[0].magic_rate, 10);

        assert!(battle.set_script_result(0));
        assert_eq!(
            battle.advance_resolution(),
            vec![BattleEvent::Finished(BattleResult::Terminated)]
        );
        assert_eq!(battle.settled_rewards(), Some(BattleRewards::default()));
    }

    #[test]
    fn defeated_players_are_not_revived_when_battle_is_created() {
        let (data, objects, magics, mut role) = fixture(100, 10, 100);
        role.hp = 0;
        let battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();

        assert_eq!(battle.players[0].hp, 0);
        assert_eq!(battle.active_player(), None);
        assert_eq!(battle.phase(), BattlePhase::Finished(BattleResult::Lost));
    }

    #[test]
    fn offensive_magic_consumes_mp_and_uses_magic_damage() {
        let (data, objects, magics, mut role) = fixture(40, 0, 100);
        role.mp = 10;
        role.max_mp = 10;
        role.magic_strength = 80;
        role.magic[0] = 2;
        let mut battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut battle);
        assert_eq!(battle.players[0].magics.len(), 1);
        let events = battle.cast_magic(0, 0).unwrap();
        assert_eq!(battle.players[0].mp, 5);
        assert!(matches!(
            events[0],
            BattleEvent::PlayerMagic {
                magic_object: 2,
                defeated: true,
                ..
            }
        ));
    }

    #[test]
    fn enemy_magic_runs_use_and_success_scripts_before_damage() {
        let (data, objects, magics, role) = fixture_with_enemy_magic(1000, 0, 200, 2, 10, 0);
        let mut battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut battle);
        battle.enemies[1]
            .statuses
            .set_for_enemy(BattleStatus::Paralyzed, 2);
        assert!(battle.attack(0).is_some());

        assert!(battle.advance_resolution().is_empty());
        let ready = battle.take_script_request().unwrap();
        assert_eq!(ready.source, BattleScriptSource::EnemyReady { enemy: 0 });
        assert!(battle.complete_script(ready.entry));
        assert!(battle.advance_resolution().is_empty());
        let use_script = battle.take_script_request().unwrap();
        assert_eq!(
            use_script,
            BattleScriptRequest {
                source: BattleScriptSource::EnemyMagicUse {
                    enemy: 0,
                    magic_object: 2,
                },
                entry: 31,
                object_id: 0,
            }
        );
        assert!(battle.complete_script_with_result(41, true));
        assert!(battle.advance_resolution().is_empty());
        let success_script = battle.take_script_request().unwrap();
        assert_eq!(
            success_script,
            BattleScriptRequest {
                source: BattleScriptSource::EnemyMagicSuccess {
                    enemy: 0,
                    magic_object: 2,
                },
                entry: 32,
                object_id: 0,
            }
        );
        assert!(battle.complete_script(42));
        assert!(matches!(
            battle.advance_resolution().as_slice(),
            [BattleEvent::EnemyMagic {
                enemy: 0,
                player: 0,
                magic_object: 2,
                damage,
                ..
            }] if *damage > 0
        ));
        assert!(battle.players[0].hp < 200);
        assert_eq!(battle.enemies[0].magic.unwrap().use_script, 41);
        assert_eq!(battle.enemies[0].magic.unwrap().success_script, 42);
    }

    #[test]
    fn failed_enemy_magic_use_skips_success_but_keeps_base_damage() {
        let (data, objects, magics, role) = fixture_with_enemy_magic(1000, 0, 200, 2, 10, 0);
        let mut battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut battle);
        battle.enemies[1]
            .statuses
            .set_for_enemy(BattleStatus::Paralyzed, 2);
        assert!(battle.attack(0).is_some());
        assert!(battle.advance_resolution().is_empty());
        let ready = battle.take_script_request().unwrap();
        assert!(battle.complete_script(ready.entry));
        assert!(battle.advance_resolution().is_empty());
        let use_script = battle.take_script_request().unwrap();
        assert_eq!(
            use_script.source,
            BattleScriptSource::EnemyMagicUse {
                enemy: 0,
                magic_object: 2,
            }
        );
        assert!(battle.complete_script_with_result(41, false));

        assert!(matches!(
            battle.advance_resolution().as_slice(),
            [BattleEvent::EnemyMagic { enemy: 0, damage, .. }] if *damage > 0
        ));
        assert!(!battle.has_script_work());
        assert!(battle.players[0].hp < 200);
    }

    #[test]
    fn enemy_magic_targets_all_living_players_and_protect_reduces_damage() {
        let (data, objects, magics, role) = fixture_with_enemy_magic(1000, 0, 500, 2, 10, 1);
        let other = role.clone();
        let mut battle = BattleState::new(
            request(true),
            0,
            7,
            [(0, &role), (1, &other)],
            &data,
            &objects,
            &magics,
        )
        .unwrap();
        let magic = battle.enemies[0].magic.unwrap();
        let events = battle.perform_enemy_magic(0, 0, magic);
        assert!(matches!(
            events.as_slice(),
            [
                BattleEvent::EnemyMagic { player: 0, .. },
                BattleEvent::EnemyMagic { player: 1, .. }
            ]
        ));

        battle.players[0].hp = 500;
        battle.random_state = 1234;
        let normal = battle.enemy_magic_damage(0, 0, magic);
        battle.players[0]
            .statuses
            .set_for_player(BattleStatus::Protect, 1, true);
        battle.random_state = 1234;
        let protected = battle.enemy_magic_damage(0, 0, magic);
        assert_eq!(protected, (normal / 2).max(1));
    }

    #[test]
    fn silence_forces_an_enemy_with_magic_to_attack_normally() {
        let (data, objects, magics, role) = fixture_with_enemy_magic(1000, 20, 500, 2, 10, 0);
        let mut battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut battle);
        battle.enemies[0]
            .statuses
            .set_for_enemy(BattleStatus::Silence, 1);
        battle.enemies[1]
            .statuses
            .set_for_enemy(BattleStatus::Paralyzed, 1);
        assert!(battle.attack(0).is_some());
        let events = resolve_until_input_or_finish(&mut battle);
        assert!(events
            .iter()
            .any(|event| matches!(event, BattleEvent::EnemyAttack { enemy: 0, .. })));
        assert!(!events
            .iter()
            .any(|event| matches!(event, BattleEvent::EnemyMagic { enemy: 0, .. })));
    }

    #[test]
    fn physical_damage_matches_pal_piecewise_boundaries() {
        assert_eq!(physical_damage(100, 50, 2), 60);
        assert_eq!(physical_damage(80, 100, 1), 20);
        assert_eq!(physical_damage(50, 100, 1), 0);
        assert_eq!(physical_damage(100, 50, 0), 120);
    }

    #[test]
    fn status_rules_preserve_original_duration_and_alive_restrictions() {
        let mut statuses = BattleStatuses::default();
        assert!(statuses.set_for_player(BattleStatus::Sleep, 3, true));
        assert!(statuses.set_for_player(BattleStatus::Sleep, 8, true));
        assert_eq!(statuses.duration(BattleStatus::Sleep), 3);

        assert!(statuses.set_for_player(BattleStatus::Protect, 2, true));
        assert!(statuses.set_for_player(BattleStatus::Protect, 5, true));
        assert_eq!(statuses.duration(BattleStatus::Protect), 5);
        assert!(!statuses.set_for_player(BattleStatus::Puppet, 4, true));
        assert!(statuses.set_for_player(BattleStatus::Puppet, 4, false));
        assert_eq!(statuses.duration(BattleStatus::Puppet), 4);

        statuses.set_for_enemy(BattleStatus::Haste, 1000);
        statuses.remove_from_player(BattleStatus::Haste);
        assert_eq!(statuses.duration(BattleStatus::Haste), 1000);
        statuses.decrement_round();
        assert_eq!(statuses.duration(BattleStatus::Haste), 999);
    }

    #[test]
    fn poison_slots_reject_duplicates_and_cure_by_object_level() {
        let objects = GlobalObjects::parse(
            &words(&[
                0, 0, 0, 0, 0, 0, // empty object 0
                1, 2, 10, 0, 11, 0, // level-one poison
                4, 3, 20, 0, 21, 0, // level-four poison
            ]),
            ObjectLayout::Dos,
        )
        .unwrap();
        let mut poisons = [BattlePoison::default(); MAX_BATTLE_POISONS];
        assert!(add_poison(&mut poisons, 1, 10));
        assert!(add_poison(&mut poisons, 1, 99));
        assert_eq!(poisons[0].script_entry, 10);
        assert!(add_poison(&mut poisons, 2, 20));
        assert!(cure_poison_by_level(&mut poisons, 1, &objects));
        assert_eq!(poisons[0].object_id, 2);
        assert_eq!(poisons[1], BattlePoison::default());
        assert!(cure_poison(&mut poisons, 2));
        assert!(!cure_poison(&mut poisons, 2));
    }

    #[test]
    fn statuses_gate_magic_double_attacks_and_expire_after_enemy_round() {
        let (data, objects, magics, mut role) = fixture(1000, 20, 200);
        role.mp = 10;
        role.max_mp = 10;
        role.magic_strength = 80;
        role.magic[0] = 2;
        let mut battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut battle);
        assert!(battle.poison_enemy(0, 40, 77, false));
        assert_eq!(battle.enemies[0].poisons[0].object_id, 40);
        assert_eq!(battle.enemies[0].poisons[0].script_entry, 77);
        let poison_script = battle.take_script_request().unwrap();
        assert_eq!(
            poison_script.source,
            BattleScriptSource::EnemyPoison {
                enemy: 0,
                poison_id: 40,
            }
        );
        assert!(battle.complete_script(78));
        assert_eq!(battle.enemies[0].poisons[0].script_entry, 78);
        assert!(battle.cure_enemy_poison(0, 40, false));
        assert_eq!(battle.enemies[0].poisons[0], BattlePoison::default());
        assert!(
            (0..20).any(|_| { battle.set_enemy_status(0, BattleStatus::Sleep, 2) == Some(true) })
        );
        battle.enemies[0]
            .statuses
            .remove_from_player(BattleStatus::Sleep);
        battle.players[0]
            .statuses
            .set_for_player(BattleStatus::Silence, 1, true);
        assert!(battle.cast_magic(0, 0).is_none());
        assert_eq!(battle.players[0].mp, 10);
        battle.players[0]
            .statuses
            .remove_from_player(BattleStatus::Silence);
        battle.players[0]
            .statuses
            .set_for_player(BattleStatus::DualAttack, 2, true);
        battle.enemies[0]
            .statuses
            .set_for_enemy(BattleStatus::Paralyzed, 1);
        battle.enemies[1]
            .statuses
            .set_for_enemy(BattleStatus::Paralyzed, 1);

        let mut events = battle.attack(0).unwrap();
        events.extend(resolve_until_input_or_finish(&mut battle));
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, BattleEvent::PlayerAttack { .. }))
                .count(),
            2
        );
        assert!(!events
            .iter()
            .any(|event| matches!(event, BattleEvent::EnemyAttack { .. })));
        assert_eq!(
            battle.players[0]
                .statuses
                .duration(BattleStatus::DualAttack),
            1
        );
        assert_eq!(
            battle.enemies[0].statuses.duration(BattleStatus::Paralyzed),
            0
        );
    }

    #[test]
    fn lifecycle_and_round_poison_scripts_run_in_original_order() {
        let (data, objects, magics, role) = fixture(1000, 0, 1000);
        let mut battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();

        for (enemy, next_entry) in [(0, 21), (1, 22)] {
            let request = battle.take_script_request().unwrap();
            assert_eq!(
                request,
                BattleScriptRequest {
                    source: BattleScriptSource::EnemyTurnStart { enemy },
                    entry: 11,
                    object_id: enemy as u16,
                }
            );
            assert!(battle.complete_script(next_entry));
        }
        assert_eq!(battle.active_player(), Some(0));

        battle.players[0].poisons[0] = BattlePoison {
            object_id: 40,
            script_entry: 31,
        };
        battle.enemies[0].poisons[0] = BattlePoison {
            object_id: 41,
            script_entry: 32,
        };
        assert!(battle.attack(0).is_some());

        let mut ready_entries = [13, 13];
        for enemy in [0, 0, 1, 1] {
            assert!(battle.advance_resolution().is_empty());
            let request = battle.take_script_request().unwrap();
            assert_eq!(request.source, BattleScriptSource::EnemyReady { enemy });
            assert_eq!(request.entry, ready_entries[enemy]);
            ready_entries[enemy] += 1;
            assert!(battle.complete_script(ready_entries[enemy]));
            assert!(matches!(
                battle.advance_resolution().as_slice(),
                [BattleEvent::EnemyAttack { enemy: actor, .. }] if *actor == enemy
            ));
        }

        assert!(battle.advance_resolution().is_empty());
        let player_poison = battle.take_script_request().unwrap();
        assert_eq!(
            player_poison.source,
            BattleScriptSource::PlayerPoison {
                role_id: 0,
                poison_id: 40,
            }
        );
        assert!(battle.complete_script(33));
        let enemy_poison = battle.take_script_request().unwrap();
        assert_eq!(
            enemy_poison.source,
            BattleScriptSource::EnemyPoison {
                enemy: 0,
                poison_id: 41,
            }
        );
        assert!(battle.complete_script(34));

        assert!(battle.advance_resolution().is_empty());
        for (enemy, entry, next_entry) in [(0, 21, 23), (1, 22, 24)] {
            let request = battle.take_script_request().unwrap();
            assert_eq!(
                request,
                BattleScriptRequest {
                    source: BattleScriptSource::EnemyTurnStart { enemy },
                    entry,
                    object_id: enemy as u16,
                }
            );
            assert!(battle.complete_script(next_entry));
        }
        assert_eq!(
            battle.advance_resolution(),
            vec![BattleEvent::RoundCompleted]
        );
        assert_eq!(battle.round(), 2);
        assert_eq!(battle.players[0].poisons[0].script_entry, 33);
        assert_eq!(battle.enemies[0].poisons[0].script_entry, 34);

        assert!(battle.set_script_result(3));
        assert!(battle.advance_resolution().is_empty());
        for (enemy, next_entry) in [(0, 25), (1, 26)] {
            let request = battle.take_script_request().unwrap();
            assert_eq!(request.source, BattleScriptSource::EnemyBattleEnd { enemy });
            assert_eq!(request.entry, 12);
            assert!(battle.complete_script(next_entry));
        }
        assert_eq!(
            battle.advance_resolution(),
            vec![BattleEvent::Finished(BattleResult::Won)]
        );
    }

    #[test]
    fn fully_disabled_party_advances_rounds_without_being_declared_defeated() {
        let (data, objects, magics, role) = fixture(1000, 0, 1000);
        let mut battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut battle);
        battle.players[0]
            .statuses
            .set_for_player(BattleStatus::Sleep, 2, true);
        battle.refresh_player_effects();
        assert_eq!(battle.phase(), BattlePhase::AwaitingCommand);
        assert_eq!(battle.active_player(), None);

        let mut first = battle.advance_automatic_turns();
        first.extend(resolve_until_input_or_finish(&mut battle));
        assert!(matches!(first.last(), Some(BattleEvent::RoundCompleted)));
        assert_eq!(battle.phase(), BattlePhase::AwaitingCommand);
        assert_eq!(battle.active_player(), None);
        let mut second = battle.advance_automatic_turns();
        second.extend(resolve_until_input_or_finish(&mut battle));
        assert!(matches!(second.last(), Some(BattleEvent::RoundCompleted)));
        assert_eq!(battle.active_player(), Some(0));
        assert_eq!(battle.players[0].statuses.duration(BattleStatus::Sleep), 0);
    }

    #[test]
    fn scripted_hp_mutations_are_bounded_and_use_original_thresholds() {
        let (data, objects, magics, role) = fixture(100, 0, 50);
        let mut battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        battle.players[0].hp = 20;
        assert!(battle.drain_enemy_hp(0, 20));
        assert_eq!(battle.enemies[0].hp, 80);
        assert_eq!(battle.players[0].hp, 40);
        assert_eq!(battle.enemy_hp_above(0, 79), Some(true));
        assert_eq!(battle.enemy_hp_above(0, 80), Some(false));
        assert!(battle.halve_enemy_hp(0, 10));
        assert_eq!(battle.enemies[0].hp, 70);
        assert!(battle.kill_enemy(0));
        assert_eq!(battle.enemies[0].hp, 0);
        assert!(!battle.damage_enemy(99, 1, false));
    }
}
