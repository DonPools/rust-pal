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
    EnemyAttackItem { enemy: usize, item_object: u16 },
    PlayerMagicUse { player: usize, magic_object: u16 },
    PlayerMagicSuccess { player: usize, magic_object: u16 },
    PlayerItemUse { player: usize, item_object: u16 },
    PlayerItemThrow { player: usize, item_object: u16 },
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
    PerformActions,
    EnemyAction {
        enemy: usize,
        ready_complete: bool,
    },
    EnemyMagic {
        enemy: usize,
        target: usize,
        magic: BattleMagic,
        phase: EnemyMagicPhase,
        use_succeeded: bool,
    },
    EnemyAttackItem {
        enemy: usize,
    },
    PlayerMagic {
        player: usize,
        target: usize,
        magic: BattleMagic,
        phase: PlayerMagicPhase,
        use_succeeded: bool,
    },
    PlayerItem {
        player: usize,
        item_object: u16,
        target: Option<usize>,
        kind: PlayerItemKind,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlayerMagicPhase {
    UseScript,
    SuccessScript,
    Damage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlayerItemKind {
    Use { consuming: bool },
    Throw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlayerAction {
    Attack {
        target: usize,
    },
    Magic {
        magic: usize,
        target: usize,
    },
    UseItem {
        item_object: u16,
        target: Option<usize>,
        script_entry: u16,
        consuming: bool,
    },
    ThrowItem {
        item_object: u16,
        target: Option<usize>,
        script_entry: u16,
    },
    Flee,
    AttackMate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BattleActorAction {
    Player { player: usize, action: PlayerAction },
    Enemy { enemy: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct QueuedBattleAction {
    action: BattleActorAction,
    dexterity: i32,
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
    pub flee_rate: u16,
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

    pub fn is_dying(&self) -> bool {
        self.hp < (self.max_hp / 5).min(100)
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
    pub attack_equivalent_item_script: u16,
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
    EnemyConfusedAttack {
        enemy: usize,
        target: usize,
        damage: u16,
        defeated: bool,
    },
    PlayerConfusedAttack {
        player: usize,
        target: usize,
        damage: u16,
        defeated: bool,
    },
    SimulatedMagic {
        enemy: usize,
        magic_object: u16,
        damage: u16,
        defeated: bool,
    },
    PlayerUseItem {
        player: usize,
        item_object: u16,
        target: Option<usize>,
        consuming: bool,
    },
    PlayerThrowItem {
        player: usize,
        item_object: u16,
        target: Option<usize>,
    },
    PlayerFlee {
        player: usize,
        succeeded: bool,
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
    player_actions: Vec<Option<PlayerAction>>,
    action_queue: Vec<QueuedBattleAction>,
    action_index: usize,
    temporary_player_stats: Vec<[u16; 6]>,
    base_player_battle_sprites: Vec<u16>,
    round: u32,
    random_state: u32,
    battlefield_magic_effect: [i16; pal_assets::battle::MAGIC_ELEMENT_COUNT],
    flow: BattleFlow,
    pending_scripts: VecDeque<BattleScriptRequest>,
    active_script: Option<BattleScriptRequest>,
    pending_events: VecDeque<BattleEvent>,
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
                let attack_item_script = match enemy.attack_equivalent_item {
                    0 => 0,
                    object_id => objects.get(object_id)?.item_use_script(),
                };
                let mut actor =
                    battle_enemy(slot, position, object_id, enemy_id, enemy, object, magic);
                actor.attack_equivalent_item_script = attack_item_script;
                Some(actor)
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
        let temporary_player_stats = vec![[0; 6]; players.len()];
        let base_player_battle_sprites = players
            .iter()
            .map(|player| player.battle_sprite_num)
            .collect();
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
            player_actions,
            action_queue: Vec::new(),
            action_index: 0,
            temporary_player_stats,
            base_player_battle_sprites,
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
            pending_events: VecDeque::new(),
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
                self.finish_player_item();
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
                self.finish_player_item();
            }
        }
        self.detect_script_outcome();
        true
    }

    /// Advance one non-interactive battle resolution step.
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
                        self.queue_round_poison_scripts();
                        self.flow = BattleFlow::RoundScripts;
                        if self.has_script_work() {
                            return events;
                        }
                        continue;
                    };
                    self.action_index += 1;
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
                                | PlayerAction::Flee
                                | PlayerAction::AttackMate => {}
                            }
                            events.extend(self.perform_player_action(player, action));
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
                    if !actor.can_act() {
                        self.flow = BattleFlow::PerformActions;
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
                        self.flow = BattleFlow::EnemyAction {
                            enemy,
                            ready_complete: true,
                        };
                        return events;
                    }
                    if actor.statuses.is_active(BattleStatus::Confused) {
                        self.flow = BattleFlow::PerformActions;
                        if let Some(event) = self.perform_confused_enemy_action(enemy) {
                            events.push(event);
                        }
                        return events;
                    }
                    let magic_object = actor.magic_object;
                    let magic_rate = actor.magic_rate;
                    let silenced = actor.statuses.is_active(BattleStatus::Silence);
                    let magic = actor.magic;
                    if magic_object != 0 && !silenced && self.random(10) < u32::from(magic_rate) {
                        if magic_object == u16::MAX {
                            self.flow = BattleFlow::PerformActions;
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
                    if let Some(event @ BattleEvent::EnemyAttack { player, .. }) =
                        self.perform_enemy_action(enemy)
                    {
                        let actor = &self.enemies[enemy];
                        let item_object = actor.attack_equivalent_item;
                        let item_rate = actor.attack_equivalent_item_rate;
                        let item_script = actor.attack_equivalent_item_script;
                        let poison_resistance = self.players[player].poison_resistance;
                        let item_triggered = item_object != 0
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
                        if self.players.iter().all(|player| !player.is_combat_active()) {
                            self.flow = BattleFlow::Outcome(BattleResult::Lost);
                        }
                        return events;
                    }
                },
                BattleFlow::EnemyAttackItem { .. } => {
                    self.flow = BattleFlow::PerformActions;
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
                            self.flow = BattleFlow::PerformActions;
                            continue;
                        }
                        if magic.success_script != 0 {
                            self.pending_scripts.push_back(BattleScriptRequest {
                                source: BattleScriptSource::PlayerMagicSuccess {
                                    player,
                                    magic_object: magic.object_id,
                                },
                                entry: magic.success_script,
                                object_id: if magic.attacks_all {
                                    u16::MAX
                                } else {
                                    u16::try_from(target).unwrap_or(u16::MAX)
                                },
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
                        self.flow = BattleFlow::PerformActions;
                        if self.enemies.iter().all(|enemy| !enemy.is_alive()) {
                            self.flow = BattleFlow::Outcome(BattleResult::Won);
                        }
                        return events;
                    }
                },
                BattleFlow::PlayerItem { .. } => {
                    self.finish_player_item();
                    events.extend(self.pending_events.drain(..));
                    self.detect_script_outcome();
                    return events;
                }
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
                    self.player_actions.fill(None);
                    self.action_queue.clear();
                    self.action_index = 0;
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

    /// Commit a normal attack for the active player.
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

        self.commit_player_action(player_index, PlayerAction::Attack { target });
        Some(Vec::new())
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
        self.commit_player_action(
            player_index,
            PlayerAction::Magic {
                magic: magic_index,
                target,
            },
        );
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
            .filter(|action| {
                matches!(
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

    fn perform_confused_enemy_action(&mut self, enemy: usize) -> Option<BattleEvent> {
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
        actor.hp = actor.hp.saturating_sub(damage);
        Some(BattleEvent::EnemyConfusedAttack {
            enemy,
            target,
            damage,
            defeated: !actor.is_alive(),
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

    fn commit_player_action(&mut self, player_index: usize, action: PlayerAction) {
        self.acted[player_index] = true;
        self.player_actions[player_index] = Some(action);
        self.active_player = next_player(&self.players, &self.acted, player_index + 1);
        if self.active_player.is_some() {
            return;
        }
        self.build_action_queue();
    }

    fn build_action_queue(&mut self) {
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
            if !self.players[player].can_act() {
                continue;
            }
            let action = if self.players[player]
                .statuses
                .is_active(BattleStatus::Confused)
            {
                PlayerAction::AttackMate
            } else if let Some(action) = self.player_actions[player] {
                action
            } else {
                continue;
            };
            let dexterity = self.player_action_dexterity(player, action);
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
        if matches!(action, PlayerAction::UseItem { .. }) {
            dexterity = dexterity.saturating_mul(3);
        }
        dexterity = dexterity.min(999);
        if actor.is_dying() {
            dexterity /= 2;
        }
        self.jitter_dexterity(i32::try_from(dexterity).unwrap_or(i32::MAX))
    }

    fn jitter_dexterity(&mut self, dexterity: i32) -> i32 {
        let percent = 90 + i32::try_from(self.random(21)).unwrap_or(0);
        dexterity.saturating_mul(percent) / 100
    }

    fn begin_player_magic(&mut self, player: usize, magic: usize, target: usize) -> bool {
        let Some(actor) = self.players.get(player) else {
            return false;
        };
        if !actor.can_act() || actor.statuses.is_active(BattleStatus::Confused) {
            return false;
        }
        let Some(spell) = actor.magics.get(magic).copied() else {
            return false;
        };
        if actor.statuses.is_active(BattleStatus::Silence) || actor.mp < spell.mp_cost {
            return false;
        }
        self.players[player].mp -= spell.mp_cost;
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

    fn begin_player_item(&mut self, player: usize, action: PlayerAction) -> bool {
        let Some(actor) = self.players.get(player) else {
            return false;
        };
        if !actor.can_act() || actor.statuses.is_active(BattleStatus::Confused) {
            return false;
        }
        let (item_object, target, script_entry, kind, source, object_id) = match action {
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
                    BattleScriptSource::PlayerItemUse {
                        player,
                        item_object,
                    },
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
                    BattleScriptSource::PlayerItemThrow {
                        player,
                        item_object,
                    },
                    target
                        .and_then(|target| u16::try_from(target).ok())
                        .unwrap_or(u16::MAX),
                )
            }
            PlayerAction::Attack { .. }
            | PlayerAction::Magic { .. }
            | PlayerAction::Flee
            | PlayerAction::AttackMate => {
                return false;
            }
        };
        self.flow = BattleFlow::PlayerItem {
            player,
            item_object,
            target,
            kind,
        };
        if script_entry == 0 {
            self.finish_player_item();
        } else {
            self.pending_scripts.push_back(BattleScriptRequest {
                source,
                entry: script_entry,
                object_id,
            });
        }
        true
    }

    fn finish_player_item(&mut self) {
        let BattleFlow::PlayerItem {
            player,
            item_object,
            target,
            kind,
        } = self.flow
        else {
            return;
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
        self.pending_events.push_front(event);
        self.flow = BattleFlow::PerformActions;
    }

    fn perform_player_action(&mut self, player: usize, action: PlayerAction) -> Vec<BattleEvent> {
        if !self.players.get(player).is_some_and(BattlePlayer::can_act) {
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
                    return self.perform_player_attack(player, target);
                };
                if self.players[player]
                    .statuses
                    .is_active(BattleStatus::Silence)
                    || self.players[player].mp < spell.mp_cost
                {
                    return self.perform_player_attack(player, target);
                }
                self.players[player].mp -= spell.mp_cost;
                self.perform_player_magic(player, target, spell)
            }
            PlayerAction::UseItem { .. } | PlayerAction::ThrowItem { .. } => Vec::new(),
            PlayerAction::Flee => vec![self.perform_player_flee(player)],
            PlayerAction::AttackMate => self
                .perform_confused_player_action(player)
                .into_iter()
                .collect(),
        }
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
            let targets = if self.players[player].attacks_all {
                self.enemies
                    .iter()
                    .enumerate()
                    .filter_map(|(index, enemy)| enemy.is_alive().then_some(index))
                    .collect::<Vec<_>>()
            } else if self.enemies.get(target).is_some_and(BattleEnemy::is_alive) {
                vec![target]
            } else if let Some(target) = self.first_living_enemy() {
                vec![target]
            } else {
                Vec::new()
            };
            for enemy in targets {
                let damage = self.player_damage(player, enemy);
                let target = &mut self.enemies[enemy];
                target.hp = target.hp.saturating_sub(damage);
                events.push(BattleEvent::PlayerAttack {
                    player,
                    enemy,
                    damage,
                    defeated: !target.is_alive(),
                });
            }
        }
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
        }
        BattleEvent::PlayerFlee { player, succeeded }
    }

    fn perform_player_magic(
        &mut self,
        player: usize,
        target: usize,
        magic: BattleMagic,
    ) -> Vec<BattleEvent> {
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
        let mut events = Vec::new();
        for enemy in targets {
            let damage = self.magic_damage(player, enemy, magic);
            let target = &mut self.enemies[enemy];
            target.hp = target.hp.saturating_sub(damage);
            events.push(BattleEvent::PlayerMagic {
                player,
                enemy,
                magic_object: magic.object_id,
                damage,
                defeated: !target.is_alive(),
            });
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
        let damage = self.confused_player_damage(player, target);
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
        if magic.base_damage == 0 && base_strength == 0 {
            return true;
        }
        for enemy in targets {
            let damage = self.simulated_magic_damage(enemy, magic, base_strength);
            let actor = &mut self.enemies[enemy];
            actor.hp = actor.hp.saturating_sub(damage);
            self.pending_events.push_back(BattleEvent::SimulatedMagic {
                enemy,
                magic_object,
                damage,
                defeated: !actor.is_alive(),
            });
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

    fn confused_player_damage(&self, player: usize, target: usize) -> u16 {
        let attacker = &self.players[player];
        let defender = &self.players[target];
        let mut damage = physical_damage(
            u32::from(attacker.attack_strength),
            u32::from(defender.defense),
            2,
        );
        if defender.statuses.is_active(BattleStatus::Protect) {
            damage /= 2;
        }
        u16::try_from(damage.max(1)).unwrap_or(u16::MAX)
    }

    fn confused_enemy_damage(&self, enemy: usize, target: usize) -> u16 {
        let attacker = &self.enemies[enemy];
        let defender = &self.enemies[target];
        let raw_attack = i32::from(attacker.attack_strength as i16)
            + i32::from(attacker.level.saturating_add(6)) * 6;
        let raw_defense =
            i32::from(defender.defense as i16) + i32::from(defender.level.saturating_add(6)) * 4;
        let base = physical_damage(
            u32::try_from(raw_attack.max(0)).unwrap_or(0),
            u32::try_from(raw_defense.max(0)).unwrap_or(0),
            0,
        )
        .saturating_mul(2);
        let damage = base
            .checked_div(u32::from(defender.physical_resistance))
            .unwrap_or(base);
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

    fn simulated_magic_damage(
        &mut self,
        enemy: usize,
        magic: BattleMagic,
        base_strength: u16,
    ) -> u16 {
        let strength = u32::from(base_strength).saturating_mul(10 + self.random(2)) / 10;
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
        u16::try_from(damage).unwrap_or(u16::MAX)
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
        attack_equivalent_item_script: 0,
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
        flee_rate: role.flee_rate,
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

fn battle_player_stat_mut(player: &mut BattlePlayer, attribute: u16) -> Option<&mut u16> {
    match attribute {
        17 => Some(&mut player.attack_strength),
        18 => Some(&mut player.magic_strength),
        19 => Some(&mut player.defense),
        20 => Some(&mut player.dexterity),
        21 => Some(&mut player.flee_rate),
        22 => Some(&mut player.poison_resistance),
        _ => None,
    }
}

fn next_player(players: &[BattlePlayer], acted: &[bool], start: usize) -> Option<usize> {
    (start..players.len()).find(|&index| {
        players[index].can_act()
            && !players[index].statuses.is_active(BattleStatus::Confused)
            && !acted[index]
    })
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
            (18, 0),
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

        let mut object_words = vec![0; 10 * 6];
        object_words[6..12].copy_from_slice(&[0, 0, 11, 12, 13, 0]);
        object_words[12..18].copy_from_slice(&[0, 0, 32, 31, 0, 0]);
        object_words[54..60].copy_from_slice(&[0, 0, 33, 0, 0, 0]);
        let objects = GlobalObjects::parse(&words(&object_words), ObjectLayout::Dos).unwrap();

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
        let (data, objects, magics, mut role) = fixture(1, 0, 100);
        role.dexterity = 100;
        let mut battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut battle);
        assert!(battle.attack(0).unwrap().is_empty());
        let first = resolve_until_input_or_finish(&mut battle);
        assert!(matches!(
            first
                .iter()
                .find(|event| matches!(event, BattleEvent::PlayerAttack { .. })),
            Some(BattleEvent::PlayerAttack { defeated: true, .. })
        ));
        assert_eq!(battle.phase(), BattlePhase::AwaitingCommand);
        assert!(battle.attack(1).unwrap().is_empty());
        let finished = resolve_until_input_or_finish(&mut battle);
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
    fn classic_flee_attempt_uses_the_action_queue_and_player_flee_rate() {
        let (data, objects, magics, mut role) = fixture(500, 0, 500);
        role.dexterity = 100;
        role.flee_rate = u16::MAX;
        let mut successful =
            BattleState::new(request(false), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut successful);
        assert!(successful.attempt_flee().unwrap().is_empty());
        assert!(matches!(
            resolve_until_input_or_finish(&mut successful).as_slice(),
            [BattleEvent::PlayerFlee {
                player: 0,
                succeeded: true,
            }]
        ));
        assert_eq!(
            successful.phase(),
            BattlePhase::Finished(BattleResult::Fled)
        );

        role.flee_rate = 0;
        let mut failed =
            BattleState::new(request(false), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut failed);
        failed.random_state = 1;
        assert!(failed.attempt_flee().unwrap().is_empty());
        let events = resolve_until_input_or_finish(&mut failed);
        assert!(events.iter().any(|event| matches!(
            event,
            BattleEvent::PlayerFlee {
                player: 0,
                succeeded: false,
            }
        )));
        assert_eq!(failed.phase(), BattlePhase::AwaitingCommand);

        let mut boss =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut boss);
        assert!(boss.attempt_flee().is_none());
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
        role.dexterity = 100;
        role.magic[0] = 2;
        let mut battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut battle);
        assert_eq!(battle.players[0].magics.len(), 1);
        assert!(battle.cast_magic(0, 0).unwrap().is_empty());
        let events = resolve_until_input_or_finish(&mut battle);
        assert_eq!(battle.players[0].mp, 5);
        assert!(matches!(
            events
                .iter()
                .find(|event| matches!(event, BattleEvent::PlayerMagic { .. })),
            Some(BattleEvent::PlayerMagic {
                magic_object: 2,
                defeated: true,
                ..
            })
        ));
    }

    #[test]
    fn player_magic_runs_use_and_success_scripts_with_mp_scaled_damage() {
        let (data, objects, magics, mut role) = fixture(1000, 0, 500);
        role.mp = 10;
        role.max_mp = 10;
        role.magic_strength = 80;
        role.magic[0] = 2;
        role.dexterity = 100;
        let mut battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut battle);
        assert!(battle.cast_magic(0, 0).unwrap().is_empty());

        assert!(battle.advance_resolution().is_empty());
        let use_script = battle.take_script_request().unwrap();
        assert_eq!(
            use_script,
            BattleScriptRequest {
                source: BattleScriptSource::PlayerMagicUse {
                    player: 0,
                    magic_object: 2,
                },
                entry: 31,
                object_id: 0,
            }
        );
        assert_eq!(battle.players[0].mp, 5);
        assert_eq!(battle.scale_active_magic_by_mp(0, 2, 8), Some(40));
        assert_eq!(battle.players[0].mp, 0);
        assert!(battle.complete_script(41));

        assert!(battle.advance_resolution().is_empty());
        let success_script = battle.take_script_request().unwrap();
        assert_eq!(
            success_script,
            BattleScriptRequest {
                source: BattleScriptSource::PlayerMagicSuccess {
                    player: 0,
                    magic_object: 2,
                },
                entry: 32,
                object_id: 0,
            }
        );
        assert!(battle.complete_script(42));
        assert!(matches!(
            battle.advance_resolution().as_slice(),
            [BattleEvent::PlayerMagic {
                player: 0,
                enemy: 0,
                magic_object: 2,
                damage,
                ..
            }] if *damage >= 40
        ));
        assert_eq!(battle.players[0].magics[0].use_script, 41);
        assert_eq!(battle.players[0].magics[0].success_script, 42);
        assert_eq!(battle.players[0].magics[0].base_damage, 40);
    }

    #[test]
    fn simulated_magic_retargets_and_queues_damage_feedback() {
        let (data, objects, magics, role) = fixture(500, 0, 500);
        let mut battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut battle);
        battle.enemies[0].hp = 0;
        let target_hp = battle.enemies[1].hp;

        assert!(battle.simulate_player_magic(0, 2, 80, &objects, &magics));
        assert!(battle.enemies[1].hp < target_hp);
        assert!(matches!(
            battle.advance_resolution().as_slice(),
            [BattleEvent::SimulatedMagic {
                enemy: 1,
                magic_object: 2,
                damage,
                ..
            }] if *damage > 0
        ));
    }

    #[test]
    fn consuming_and_thrown_items_reserve_inventory_during_command_selection() {
        let (data, objects, magics, mut role) = fixture(500, 0, 500);
        role.dexterity = 100;
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
        complete_pending_scripts(&mut battle);

        assert!(battle.use_item(20, Some(0), 30, true).is_some());
        assert_eq!(battle.reserved_item_count(20), 1);
        assert!(battle.throw_item(21, Some(0), 40).is_some());
        assert_eq!(battle.reserved_item_count(21), 1);

        let mut non_consuming = BattleState::new(
            request(true),
            0,
            7,
            [(0, &role), (1, &other)],
            &data,
            &objects,
            &magics,
        )
        .unwrap();
        complete_pending_scripts(&mut non_consuming);
        assert!(non_consuming.use_item(22, Some(0), 50, false).is_some());
        assert_eq!(non_consuming.reserved_item_count(22), 0);
    }

    #[test]
    fn battle_item_scripts_use_original_owners_and_finish_even_when_failed() {
        let (data, objects, magics, mut role) = fixture(500, 0, 500);
        role.dexterity = 100;
        let mut battle =
            BattleState::new(request(true), 0, 7, [(4, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut battle);
        assert!(battle.use_item(20, Some(0), 30, true).is_some());

        assert!(battle.advance_resolution().is_empty());
        assert_eq!(
            battle.take_script_request(),
            Some(BattleScriptRequest {
                source: BattleScriptSource::PlayerItemUse {
                    player: 0,
                    item_object: 20,
                },
                entry: 30,
                object_id: 4,
            })
        );
        assert!(battle.complete_script_with_result(31, false));
        assert_eq!(
            battle.advance_resolution(),
            vec![BattleEvent::PlayerUseItem {
                player: 0,
                item_object: 20,
                target: Some(0),
                consuming: true,
            }]
        );
    }

    #[test]
    fn all_target_and_empty_item_scripts_keep_original_completion_semantics() {
        let (data, objects, magics, mut role) = fixture(500, 0, 500);
        role.dexterity = 100;
        let mut use_all =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut use_all);
        assert!(use_all.use_item(20, None, 30, true).is_some());
        assert!(use_all.advance_resolution().is_empty());
        assert_eq!(use_all.take_script_request().unwrap().object_id, u16::MAX);

        let mut throw_all =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut throw_all);
        assert!(throw_all.throw_item(21, None, 40).is_some());
        assert!(throw_all.advance_resolution().is_empty());
        assert_eq!(throw_all.take_script_request().unwrap().object_id, u16::MAX);

        let mut empty_script =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut empty_script);
        assert!(empty_script.use_item(22, Some(0), 0, false).is_some());
        assert!(matches!(
            empty_script.advance_resolution().as_slice(),
            [BattleEvent::PlayerUseItem {
                item_object: 22,
                consuming: false,
                ..
            }]
        ));
    }

    #[test]
    fn thrown_weapon_script_uses_enemy_owner_and_acting_player_attack() {
        let (data, objects, magics, mut role) = fixture(500, 0, 500);
        role.dexterity = 100;
        role.attack_strength = 80;
        let mut battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut battle);
        assert!(battle.throw_item(21, Some(0), 40).is_some());

        assert!(battle.advance_resolution().is_empty());
        assert_eq!(
            battle.take_script_request(),
            Some(BattleScriptRequest {
                source: BattleScriptSource::PlayerItemThrow {
                    player: 0,
                    item_object: 21,
                },
                entry: 40,
                object_id: 0,
            })
        );
        battle.random_state = 1;
        assert!(battle.throw_weapon(0, 2, 2, &objects, &magics));
        assert!(battle.complete_script(41));
        assert!(matches!(
            battle.advance_resolution().as_slice(),
            [
                BattleEvent::PlayerThrowItem {
                    player: 0,
                    item_object: 21,
                    target: Some(0),
                },
                BattleEvent::SimulatedMagic {
                    enemy: 0,
                    magic_object: 2,
                    damage,
                    ..
                }
            ] if *damage > 50
        ));
    }

    #[test]
    fn item_is_not_completed_when_player_is_defeated_before_acting() {
        let (data, objects, magics, mut role) = fixture(500, 500, 1);
        role.dexterity = 0;
        let mut battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut battle);
        assert!(battle.use_item(20, Some(0), 30, true).is_some());

        let events = resolve_until_input_or_finish(&mut battle);
        assert!(events
            .iter()
            .any(|event| matches!(event, BattleEvent::EnemyAttack { defeated: true, .. })));
        assert!(!events.iter().any(|event| matches!(
            event,
            BattleEvent::PlayerUseItem { .. } | BattleEvent::PlayerThrowItem { .. }
        )));
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
        assert!(!battle.is_enemy_turn());
        assert!(battle.complete_script(ready.entry));
        assert!(battle.advance_resolution().is_empty());
        assert!(battle.is_enemy_turn());
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
        assert!(battle.is_enemy_turn());
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
        assert!(!battle.is_enemy_turn());
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
    fn confused_enemy_attacks_a_random_living_enemy_instead_of_the_party() {
        let (data, objects, magics, role) = fixture(1000, 200, 500);
        let mut battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut battle);
        battle.enemies[0]
            .statuses
            .set_for_enemy(BattleStatus::Confused, 1);
        let seed = (1..100)
            .find(|&seed| {
                let mut candidate = battle.clone();
                candidate.random_state = seed;
                candidate.perform_confused_enemy_action(0).is_some()
            })
            .unwrap();
        battle.random_state = seed;
        battle.flow = BattleFlow::EnemyAction {
            enemy: 0,
            ready_complete: true,
        };
        let player_hp = battle.players[0].hp;
        let target_hp = battle.enemies[1].hp;

        assert!(matches!(
            battle.advance_resolution().as_slice(),
            [BattleEvent::EnemyConfusedAttack {
                enemy: 0,
                target: 1,
                damage,
                ..
            }] if *damage > 0
        ));
        assert_eq!(battle.players[0].hp, player_hp);
        assert!(battle.enemies[1].hp < target_hp);
    }

    #[test]
    fn confused_player_skips_command_selection_and_attacks_a_living_teammate() {
        let (data, objects, magics, mut role) = fixture(1000, 0, 500);
        role.dexterity = 100;
        let mut teammate = role.clone();
        teammate.dexterity = 1;
        teammate.attack_strength = 1;
        let mut battle = BattleState::new(
            request(true),
            0,
            7,
            [(0, &role), (1, &teammate)],
            &data,
            &objects,
            &magics,
        )
        .unwrap();
        complete_pending_scripts(&mut battle);
        battle.enemies[0].dual_move = false;
        battle.enemies[0]
            .statuses
            .set_for_enemy(BattleStatus::Paralyzed, 1);
        battle.enemies[1].hp = 0;
        battle.players[0]
            .statuses
            .set_for_player(BattleStatus::Confused, 2, true);
        battle.refresh_player_effects();
        assert_eq!(battle.active_player(), Some(1));
        let teammate_hp = battle.players[1].hp;

        assert!(battle.attack(0).unwrap().is_empty());
        let events = resolve_until_input_or_finish(&mut battle);
        assert!(matches!(
            events.iter().find(|event| matches!(event, BattleEvent::PlayerConfusedAttack { .. })),
            Some(BattleEvent::PlayerConfusedAttack {
                player: 0,
                target: 1,
                damage,
                ..
            }) if *damage > 0
        ));
        assert!(battle.players[1].hp < teammate_hp);
    }

    #[test]
    fn haste_uses_classic_triple_dexterity_in_the_round_action_queue() {
        let (data, objects, magics, mut role) = fixture(1000, 1, 500);
        role.attack_strength = 1;
        role.dexterity = 10;
        let mut battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut battle);
        battle.enemies[0].dual_move = false;
        battle.enemies[0].ready_script = 0;
        battle.enemies[1].hp = 0;
        let mut normal = battle.clone();
        assert!(normal.attack(0).unwrap().is_empty());
        assert!(matches!(
            normal.advance_resolution().as_slice(),
            [BattleEvent::EnemyAttack { enemy: 0, .. }]
        ));

        battle.players[0]
            .statuses
            .set_for_player(BattleStatus::Haste, 2, true);
        assert!(battle.attack(0).unwrap().is_empty());

        assert!(matches!(
            battle.advance_resolution().as_slice(),
            [BattleEvent::PlayerAttack { player: 0, .. }]
        ));
        assert!(matches!(
            battle.advance_resolution().as_slice(),
            [BattleEvent::EnemyAttack { enemy: 0, .. }]
        ));
    }

    #[test]
    fn enemy_attack_item_runs_during_enemy_turn_and_obeys_poison_resistance() {
        let (data, objects, magics, role) = fixture(1000, 20, 500);
        let mut battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut battle);
        battle.enemies[0].attack_equivalent_item_rate = 10;
        battle.enemies[1]
            .statuses
            .set_for_enemy(BattleStatus::Paralyzed, 2);
        assert!(battle.attack(0).is_some());
        assert!(battle.advance_resolution().is_empty());
        let ready = battle.take_script_request().unwrap();
        assert_eq!(ready.source, BattleScriptSource::EnemyReady { enemy: 0 });
        assert!(!battle.is_enemy_turn());
        assert!(battle.complete_script(ready.entry));

        assert!(matches!(
            battle.advance_resolution().as_slice(),
            [BattleEvent::EnemyAttack { enemy: 0, .. }]
        ));
        assert!(battle.is_enemy_turn());
        let item = battle.take_script_request().unwrap();
        assert_eq!(
            item,
            BattleScriptRequest {
                source: BattleScriptSource::EnemyAttackItem {
                    enemy: 0,
                    item_object: 9,
                },
                entry: 33,
                object_id: 0,
            }
        );
        assert!(battle.complete_script(34));
        assert_eq!(battle.enemies[0].attack_equivalent_item_script, 34);
        assert!(battle.is_enemy_turn());
        assert!(battle.advance_resolution().is_empty());
        assert!(!battle.is_enemy_turn());

        let mut resisted =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut resisted);
        resisted.enemies[0].attack_equivalent_item_rate = 10;
        resisted.players[0].poison_resistance = 100;
        resisted.enemies[1]
            .statuses
            .set_for_enemy(BattleStatus::Paralyzed, 2);
        assert!(resisted.attack(0).is_some());
        assert!(resisted.advance_resolution().is_empty());
        let ready = resisted.take_script_request().unwrap();
        assert!(resisted.complete_script(ready.entry));
        assert!(matches!(
            resisted.advance_resolution().as_slice(),
            [BattleEvent::EnemyAttack { enemy: 0, .. }]
        ));
        assert!(!resisted.has_script_work());
        assert!(!resisted.is_enemy_turn());
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
    fn temporary_player_stats_replace_the_extra_effect_and_sprite_restores() {
        let (data, objects, magics, role) = fixture(500, 0, 500);
        let mut battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        assert_eq!(battle.players[0].attack_strength, 80);
        assert_eq!(battle.players[0].battle_sprite_num, 0);

        assert!(battle.set_temporary_player_stat(0, 17, 40));
        assert_eq!(battle.players[0].attack_strength, 120);
        assert!(battle.set_temporary_player_stat(0, 17, 20));
        assert_eq!(battle.players[0].attack_strength, 100);
        assert!(battle.set_temporary_player_stat(0, 21, 15));
        assert_eq!(battle.players[0].flee_rate, 35);
        assert!(!battle.set_temporary_player_stat(0, 16, 10));
        assert!(!battle.set_temporary_player_stat(9, 17, 10));

        assert!(battle.set_temporary_player_sprite(0, 5));
        assert_eq!(battle.players[0].battle_sprite_num, 5);
        assert!(battle.set_temporary_player_sprite(0, 0));
        assert_eq!(battle.players[0].battle_sprite_num, 0);
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
        let mut ready_counts = [0; 2];
        for _ in 0..4 {
            while !battle.has_script_work() {
                battle.advance_resolution();
            }
            let request = battle.take_script_request().unwrap();
            let BattleScriptSource::EnemyReady { enemy } = request.source else {
                panic!("expected enemy ready script, got {:?}", request.source);
            };
            assert_eq!(request.entry, ready_entries[enemy]);
            ready_entries[enemy] += 1;
            ready_counts[enemy] += 1;
            assert!(battle.complete_script(ready_entries[enemy]));
            assert!(matches!(
                battle.advance_resolution().as_slice(),
                [BattleEvent::EnemyAttack { enemy: actor, .. }] if *actor == enemy
            ));
        }
        assert_eq!(ready_counts, [2, 2]);

        while !battle.has_script_work() {
            battle.advance_resolution();
        }
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
