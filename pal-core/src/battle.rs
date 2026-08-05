//! Deterministic, platform-independent Classic battle state and combat rules.

use std::collections::VecDeque;

use pal_assets::battle::{BattleData, BattlePosition, Enemy, EnemyPositions, MAX_ENEMIES_IN_TEAM};
use pal_assets::magic::Magics;
use pal_assets::objects::{GlobalObject, GlobalObjects};
use pal_assets::player_roles::PlayerRole;

pub const BATTLE_STATUS_COUNT: usize = 9;
pub const MAX_BATTLE_POISONS: usize = 16;
pub const HIDDEN_EXPERIENCE_CATEGORY_COUNT: usize = 7;
pub const HIDDEN_EXP_HEALTH: usize = 0;
pub const HIDDEN_EXP_MAGIC: usize = 1;
pub const HIDDEN_EXP_ATTACK: usize = 2;
pub const HIDDEN_EXP_MAGIC_POWER: usize = 3;
pub const HIDDEN_EXP_DEFENSE: usize = 4;
pub const HIDDEN_EXP_DEXTERITY: usize = 5;
pub const HIDDEN_EXP_FLEE: usize = 6;
pub const MAGIC_FLAG_USABLE_IN_BATTLE: u16 = 1 << 1;
pub const MAGIC_FLAG_USABLE_TO_ENEMY: u16 = 1 << 3;
pub const MAGIC_FLAG_APPLY_TO_ALL: u16 = 1 << 4;

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
    PlayerFriendDeath { player: usize, name_object: u16 },
    PlayerDying { player: usize, name_object: u16 },
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
        target: BattleTarget,
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
    RoundScripts {
        actor: usize,
        poison_slot: usize,
    },
    TurnStartScripts {
        next_slot: usize,
        completes_round: bool,
    },
    Outcome(BattleResult),
    BattleEndScripts {
        result: BattleResult,
        next_slot: usize,
    },
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
        target: BattleTarget,
    },
    CooperativeMagic {
        target: BattleTarget,
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
    Defend,
    AttackMate,
}

/// A target selected for a player magic in Classic battle mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattleTarget {
    Enemy(usize),
    Player(usize),
    AllEnemies,
    AllPlayers,
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
    pub cooperative_magic: Option<BattleMagic>,
    pub defending: bool,
    pub covered_by: u16,
    pub friend_death_script: u16,
    pub dying_script: u16,
    pub death_sound: u16,
    pub attack_sound: u16,
    pub weapon_sound: u16,
    pub critical_sound: u16,
    pub magic_sound: u16,
    pub cover_sound: u16,
    pub dying_sound: u16,
    pub statuses: BattleStatuses,
    pub poisons: [BattlePoison; MAX_BATTLE_POISONS],
    pub poison_face_color: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BattleMagicVisual {
    pub object_id: u16,
    pub flags: u16,
    pub effect: u16,
    pub magic_type: u16,
    pub x_offset: i16,
    pub y_offset: i16,
    pub specific: i16,
    pub speed: i16,
    pub keep_effect: u16,
    pub fire_delay: u16,
    pub effect_times: u16,
    pub shake: u16,
    pub wave: u16,
    pub sound: i16,
}

impl BattleMagicVisual {
    pub fn usable_to_enemy(self) -> bool {
        self.flags & MAGIC_FLAG_USABLE_TO_ENEMY != 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BattleMagic {
    pub object_id: u16,
    pub flags: u16,
    pub effect: u16,
    pub magic_type: u16,
    pub x_offset: i16,
    pub y_offset: i16,
    pub specific: i16,
    pub speed: i16,
    pub keep_effect: u16,
    pub fire_delay: u16,
    pub effect_times: u16,
    pub shake: u16,
    pub wave: u16,
    pub mp_cost: u16,
    pub base_damage: u16,
    pub elemental: u16,
    pub attacks_all: bool,
    pub sound: i16,
    pub use_script: u16,
    pub success_script: u16,
    /// For summon magic, the offensive magic played after the summoned body.
    pub summon_effect: Option<BattleMagicVisual>,
}

impl BattleMagic {
    pub fn visual(self) -> BattleMagicVisual {
        BattleMagicVisual {
            object_id: self.object_id,
            flags: self.flags,
            effect: self.effect,
            magic_type: self.magic_type,
            x_offset: self.x_offset,
            y_offset: self.y_offset,
            specific: self.specific,
            speed: self.speed,
            keep_effect: self.keep_effect,
            fire_delay: self.fire_delay,
            effect_times: self.effect_times,
            shake: self.shake,
            wave: self.wave,
            sound: self.sound,
        }
    }

    pub fn effect_visual(self) -> BattleMagicVisual {
        self.summon_effect.unwrap_or_else(|| self.visual())
    }

    pub fn usable_in_battle(self) -> bool {
        self.flags & MAGIC_FLAG_USABLE_IN_BATTLE != 0
    }

    pub fn usable_to_enemy(self) -> bool {
        self.flags & MAGIC_FLAG_USABLE_TO_ENEMY != 0
    }

    pub fn apply_to_all(self) -> bool {
        self.flags & MAGIC_FLAG_APPLY_TO_ALL != 0
    }

    pub fn target(self, selected: usize) -> BattleTarget {
        match (self.usable_to_enemy(), self.apply_to_all()) {
            (true, true) => BattleTarget::AllEnemies,
            (true, false) => BattleTarget::Enemy(selected),
            (false, true) => BattleTarget::AllPlayers,
            (false, false) => BattleTarget::Player(selected),
        }
    }
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
    pub action_wait_frames: u16,
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
    rewards_collected: bool,
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
        critical: bool,
        defeated: bool,
    },
    PlayerMagic {
        player: usize,
        enemy: usize,
        magic_object: u16,
        blow: i16,
        damage: u16,
        visual: bool,
        defeated: bool,
    },
    EnemyAttack {
        enemy: usize,
        player: usize,
        damage: u16,
        protected_by: Option<usize>,
        auto_defended: bool,
        defeated: bool,
    },
    EnemyMagic {
        enemy: usize,
        player: usize,
        magic_object: u16,
        blow: i16,
        damage: u16,
        visual: bool,
        auto_defended: bool,
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
        blow: i16,
        damage: u16,
        visual: bool,
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
    PlayerDefend {
        player: usize,
    },
    PlayerDefensiveMagic {
        player: usize,
        target: BattleTarget,
        magic_object: u16,
    },
    PlayerCooperativeMagic {
        player: usize,
        enemy: usize,
        magic_object: u16,
        damage: u16,
        visual: bool,
        defeated: bool,
    },
    PlayerMagicAnimation {
        player: Option<usize>,
    },
    PlayerFriendDeath {
        player: usize,
    },
    PlayerDying {
        player: usize,
    },
    RoundCompleted,
    Finished(BattleResult),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BattleRewards {
    pub experience: u32,
    pub cash: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattleSteal {
    Nothing,
    Cash(u16),
    Item(u16),
}

/// The platform-independent Classic battle state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BattleState {
    pub enemy_team: u16,
    pub battlefield: u16,
    pub music: u16,
    pub is_boss: bool,
    pub players: Vec<BattlePlayer>,
    pub enemies: Vec<BattleEnemy>,
    enemy_layout_slots: usize,
    hiding_time: u16,
    phase: BattlePhase,
    active_player: Option<usize>,
    acted: Vec<bool>,
    player_actions: Vec<Option<PlayerAction>>,
    previous_player_actions: Vec<Option<PlayerAction>>,
    automatic_player_attacks: Vec<bool>,
    previous_automatic_player_attacks: Vec<bool>,
    repeating_round: bool,
    auto_attack_mode: bool,
    previous_auto_attack: bool,
    execution_auto_attack: bool,
    hidden_experience_counts: Vec<[u16; HIDDEN_EXPERIENCE_CATEGORY_COUNT]>,
    cooperative_magic_performed: bool,
    action_queue: Vec<QueuedBattleAction>,
    action_index: usize,
    temporary_player_stats: Vec<[u16; 6]>,
    base_player_battle_sprites: Vec<u16>,
    inventory_amounts: Option<Vec<(u16, u16)>>,
    previous_player_hp: Vec<u16>,
    gained_rewards: BattleRewards,
    auto_battle: bool,
    round: u32,
    random_state: u32,
    battlefield_magic_effect: [i16; pal_assets::battle::MAGIC_ELEMENT_COUNT],
    magic_blow: i16,
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
            gained_rewards: BattleRewards::default(),
            auto_battle: false,
            round: 1,
            random_state: 0x6d2b_79f5,
            battlefield_magic_effect: battlefield_definition.magic_effect,
            magic_blow: 0,
            flow: BattleFlow::Finished,
            pending_scripts: VecDeque::new(),
            active_script: None,
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
        if self.active_script.is_none() && self.pending_scripts.is_empty() {
            match self.flow {
                BattleFlow::TurnStartScripts { .. } => self.queue_next_turn_start_script(),
                BattleFlow::BattleEndScripts { .. } => self.queue_next_battle_end_script(),
                _ => {}
            }
        }
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
                        for player in &mut self.players {
                            player.defending = false;
                        }
                        self.flow = BattleFlow::RoundScripts {
                            actor: 0,
                            poison_slot: 0,
                        };
                        continue;
                    };
                    self.action_index += 1;
                    let queued = self.propagate_execution_auto_attack(queued);
                    let queued = self.validate_queued_item(queued);
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
                    if let Some(
                        event @ BattleEvent::EnemyAttack {
                            player,
                            protected_by,
                            auto_defended,
                            ..
                        },
                    ) = self.perform_enemy_action(enemy)
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
                BattleFlow::PlayerItem { .. } => {
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
                        self.flow = BattleFlow::Outcome(BattleResult::Won);
                        continue;
                    }
                    if self.players.iter().all(|player| !player.is_combat_active()) {
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
                    self.player_actions.fill(None);
                    self.automatic_player_attacks.fill(false);
                    self.execution_auto_attack = false;
                    self.action_queue.clear();
                    self.action_index = 0;
                    self.active_player = next_player(&self.players, &self.acted, 0);
                    self.flow = BattleFlow::Command;
                    events.push(BattleEvent::RoundCompleted);
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
                    events.push(BattleEvent::Finished(result));
                    return events;
                }
            }
        }
    }

    pub fn first_living_enemy(&self) -> Option<usize> {
        self.enemies.iter().position(BattleEnemy::is_alive)
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
            || self.enemies.iter().filter(|enemy| enemy.is_alive()).count() != 1
        {
            return None;
        }
        let source = self.enemies.get(enemy_index)?.clone();
        if !source.is_alive() || source.hp <= 1 {
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
            .filter(|enemy| enemy.is_alive())
            .map(|enemy| enemy.slot)
            .chain(free_slots.iter().copied())
            .max()?
            .checked_add(1)?;
        if layout_slots > MAX_ENEMIES_IN_TEAM
            || self
                .enemies
                .iter()
                .filter(|enemy| enemy.is_alive())
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
        for enemy in self.enemies.iter_mut().filter(|enemy| enemy.is_alive()) {
            enemy.position = positions.get(layout_slots, enemy.slot)?;
        }
        self.enemies.get_mut(enemy_index)?.hp = shared_hp;

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
        if self.phase != BattlePhase::AwaitingCommand || self.hiding_time != 0 {
            return None;
        }
        let source = self.enemies.get(enemy_index)?;
        if !source.is_alive()
            || source.statuses.is_active(BattleStatus::Sleep)
            || source.statuses.is_active(BattleStatus::Paralyzed)
            || source.statuses.is_active(BattleStatus::Confused)
        {
            return None;
        }
        let summoned_object = match object_id {
            0 | u16::MAX => source.object_id,
            object_id => object_id,
        };
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
            return None;
        }
        let actors = free_slots
            .iter()
            .copied()
            .map(|slot| {
                let position = data.enemy_positions.get(self.enemy_layout_slots, slot)?;
                battle_enemy_from_object(slot, position, summoned_object, data, objects, magics)
            })
            .collect::<Option<Vec<_>>>()?;
        for slot in free_slots {
            self.retire_defeated_slot(slot);
        }
        let first = self.enemies.len();
        self.enemies.extend(actors);
        Some((first..self.enemies.len()).collect())
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

    pub fn collect_enemy(&self, enemy_index: usize) -> Option<u16> {
        (self.phase == BattlePhase::AwaitingCommand)
            .then(|| self.enemies.get(enemy_index))
            .flatten()
            .filter(|enemy| enemy.is_alive())
            .map(|enemy| enemy.collect_value)
            .filter(|&value| value != 0)
    }

    pub fn steal_enemy(&mut self, enemy_index: usize, rate: u16) -> Option<BattleSteal> {
        if self.phase != BattlePhase::AwaitingCommand {
            return None;
        }
        let enemy = self.enemies.get(enemy_index)?;
        if !enemy.is_alive() {
            return None;
        }
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
        Some(self.enemies.iter().any(|other| {
            other.is_alive() && other.object_id == enemy.object_id && other.slot < enemy.slot
        }))
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

    fn magic_target_is_valid(&self, magic: BattleMagic, target: BattleTarget) -> bool {
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
        self.commit_player_action(player, PlayerAction::CooperativeMagic { target });
        Some(Vec::new())
    }

    fn coop_healthy(player: &BattlePlayer) -> bool {
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
        let action = self
            .previous_player_actions
            .get(player)
            .copied()
            .flatten()
            .unwrap_or(PlayerAction::Attack {
                target: default_target,
            });
        let action = match action {
            PlayerAction::Attack { target }
                if self.enemies.get(target).is_some_and(BattleEnemy::is_alive) =>
            {
                PlayerAction::Attack { target }
            }
            PlayerAction::Attack { .. } | PlayerAction::AttackMate => PlayerAction::Attack {
                target: default_target,
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
                    BattleTarget::Enemy(enemy)
                        if self.enemies.get(enemy).is_some_and(BattleEnemy::is_alive) =>
                    {
                        BattleTarget::Enemy(enemy)
                    }
                    BattleTarget::Enemy(_) => BattleTarget::Enemy(default_target),
                    BattleTarget::Player(target) if target < self.players.len() => {
                        BattleTarget::Player(target)
                    }
                    BattleTarget::Player(_) => BattleTarget::Player(player),
                    BattleTarget::AllEnemies => BattleTarget::AllEnemies,
                    BattleTarget::AllPlayers => BattleTarget::AllPlayers,
                };
                if self.players[player]
                    .statuses
                    .is_active(BattleStatus::Silence)
                    || self.players[player].mp < spell.mp_cost
                    || !self.magic_target_is_valid(spell, target)
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
                if self.players[player]
                    .cooperative_magic
                    .is_some_and(|magic| self.magic_target_is_valid(magic, target))
                {
                    PlayerAction::CooperativeMagic { target }
                } else {
                    PlayerAction::Attack {
                        target: default_target,
                    }
                }
            }
            PlayerAction::UseItem {
                item_object,
                target,
                script_entry,
                consuming,
            } => PlayerAction::UseItem {
                item_object,
                target: target.filter(|&target| target < self.players.len()),
                script_entry,
                consuming,
            },
            PlayerAction::ThrowItem {
                item_object,
                target,
                script_entry,
            } => PlayerAction::ThrowItem {
                item_object,
                target: match target {
                    None => None,
                    Some(target) if self.enemies.get(target).is_some_and(BattleEnemy::is_alive) => {
                        Some(target)
                    }
                    Some(_) => Some(default_target),
                },
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
        };
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
                visual,
                auto_defended,
                defeated: !target.is_alive(),
            });
            blow = 0;
            visual = false;
        }
        events
    }

    fn commit_player_action(&mut self, player_index: usize, action: PlayerAction) {
        self.commit_player_action_with_auto_attack(player_index, action, false);
    }

    fn commit_player_action_with_auto_attack(
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

    fn build_action_queue(&mut self) {
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

    fn propagate_execution_auto_attack(
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

    fn validate_queued_item(&mut self, mut queued: QueuedBattleAction) -> QueuedBattleAction {
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

    fn jitter_dexterity(&mut self, dexterity: i32) -> i32 {
        (dexterity as f32 * self.random_float(0.9, 1.1)) as i32
    }

    fn begin_player_magic(&mut self, player: usize, magic: usize, target: BattleTarget) -> bool {
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

    fn magic_success_owner(&self, target: BattleTarget) -> u16 {
        match target {
            BattleTarget::Enemy(enemy) => self.enemy_slot_for_index(enemy).unwrap_or(u16::MAX),
            BattleTarget::Player(player) => self
                .players
                .get(player)
                .map_or(u16::MAX, |actor| actor.role_id),
            BattleTarget::AllEnemies | BattleTarget::AllPlayers => u16::MAX,
        }
    }

    fn begin_player_item(&mut self, player: usize, action: PlayerAction) -> bool {
        let Some(actor) = self.players.get(player) else {
            return false;
        };
        if !actor.can_act() || actor.statuses.is_active(BattleStatus::Confused) {
            return false;
        }
        self.magic_blow = 0;
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
        self.queue_post_action_check(false);
    }

    fn perform_player_action(&mut self, player: usize, action: PlayerAction) -> Vec<BattleEvent> {
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
                    return vec![self.perform_player_defend(player)];
                }
                self.players[player].mp -= spell.mp_cost;
                self.perform_player_magic(player, target, spell)
            }
            PlayerAction::CooperativeMagic { target } => {
                self.perform_cooperative_magic(player, target)
            }
            PlayerAction::UseItem { .. } | PlayerAction::ThrowItem { .. } => Vec::new(),
            PlayerAction::Flee => vec![self.perform_player_flee(player)],
            PlayerAction::Defend => vec![self.perform_player_defend(player)],
            PlayerAction::AttackMate => self
                .perform_confused_player_action(player)
                .into_iter()
                .collect(),
        }
    }

    fn perform_player_defend(&mut self, player: usize) -> BattleEvent {
        self.players[player].defending = true;
        self.add_hidden_experience(player, HIDDEN_EXP_DEFENSE, 2);
        BattleEvent::PlayerDefend { player }
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
            let critical = self.random(6) == 0
                || self.players[player]
                    .statuses
                    .is_active(BattleStatus::Bravery);
            let bonus_hit =
                !attacks_all && self.players[player].role_id == 0 && self.random(12) == 0;
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
            for enemy in targets {
                let (damage, event_critical) = if attacks_all {
                    let damage = self.player_attack_all_damage(player, enemy, critical, division);
                    division = division.saturating_mul(2);
                    (damage, critical)
                } else {
                    self.player_single_attack_damage(player, enemy, critical, bonus_hit)
                };
                let target = &mut self.enemies[enemy];
                target.hp = target.hp.saturating_sub(damage);
                events.push(BattleEvent::PlayerAttack {
                    player,
                    enemy,
                    damage,
                    critical: event_critical,
                    defeated: !target.is_alive(),
                });
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

    fn record_magic_experience(&mut self, player: usize) {
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

    fn perform_player_magic(
        &mut self,
        player: usize,
        target: BattleTarget,
        magic: BattleMagic,
    ) -> Vec<BattleEvent> {
        if !magic.usable_to_enemy() {
            return vec![BattleEvent::PlayerDefensiveMagic {
                player,
                target,
                magic_object: magic.object_id,
            }];
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
            target.hp = target.hp.saturating_sub(damage);
            events.push(BattleEvent::PlayerMagic {
                player,
                enemy,
                magic_object: magic.object_id,
                blow,
                damage,
                visual,
                defeated: !target.is_alive(),
            });
            blow = 0;
            visual = false;
        }
        events
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
                .simulated_magic_damage(enemy, magic, base_strength)
                .max(1);
            let target = &mut self.enemies[enemy];
            target.hp = target.hp.saturating_sub(damage);
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
            actor.hp = actor.hp.saturating_sub(damage);
            self.pending_events.push_back(BattleEvent::SimulatedMagic {
                enemy,
                magic_object,
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

    fn backup_player_hp(&mut self) {
        self.previous_player_hp = self.players.iter().map(|player| player.hp).collect();
    }

    fn collect_defeated_enemy_rewards(&mut self) {
        for enemy in &mut self.enemies {
            if enemy.object_id != 0 && enemy.hp == 0 && !enemy.rewards_collected {
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

    fn round_poison_script_request(
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

    fn start_turn_start_scripts(&mut self, completes_round: bool) {
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
        if !completes_round {
            self.flow = BattleFlow::Command;
        }
    }

    fn start_battle_end_scripts(&mut self, result: BattleResult) {
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

    fn detect_script_outcome(&mut self) {
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

    fn player_single_attack_damage(
        &mut self,
        player: usize,
        enemy: usize,
        critical: bool,
        bonus_hit: bool,
    ) -> (u16, bool) {
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
        if critical {
            damage = damage.saturating_mul(3);
        }
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
        let defense = u32::from(defender.defense)
            .saturating_add(u32::from(defender.level.saturating_add(6)) * 4);
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

    fn enemy_damage(&mut self, enemy: usize, player: usize) -> u16 {
        let attacker = &self.enemies[enemy];
        let raw_strength = i32::from(attacker.attack_strength as i16)
            + i32::from(attacker.level.saturating_add(6)) * 6;
        let attack = u32::try_from(raw_strength.max(0)).unwrap_or(0) + self.random(3);
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
            self.randomized_magic_strength(u32::from(self.players[player].magic_strength));
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
        let strength = self.randomized_magic_strength(u32::from(base_strength));
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

    fn enemy_magic_damage(
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

    fn random(&mut self, upper_exclusive: u32) -> u32 {
        if upper_exclusive <= 1 {
            return 0;
        }
        self.next_random() % upper_exclusive
    }

    fn next_random(&mut self) -> u32 {
        let mut x = self.random_state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.random_state = x;
        x
    }

    /// Match Classic's inclusive floating-point interpolation before the caller's integer cast.
    fn random_float(&mut self, from: f32, to: f32) -> f32 {
        if to <= from {
            return from;
        }
        let roll = self.next_random() >> 1;
        from + roll as f32 / i32::MAX as f32 * (to - from)
    }

    /// Classic truncates once after multiplying by `RandomFloat(10, 11)`, then divides by 10.
    fn randomized_magic_strength(&mut self, strength: u32) -> u32 {
        (strength as f32 * self.random_float(10.0, 11.0)) as u32 / 10
    }

    fn finish(&mut self, result: BattleResult, events: &mut Vec<BattleEvent>) {
        self.active_player = None;
        self.pending_scripts.clear();
        if result == BattleResult::Won {
            self.start_battle_end_scripts(result);
        }
        if self.pending_scripts.is_empty() {
            self.phase = BattlePhase::Finished(result);
            self.flow = BattleFlow::Finished;
            events.push(BattleEvent::Finished(result));
        }
    }
}

fn battle_enemy_from_object(
    slot: usize,
    position: BattlePosition,
    object_id: u16,
    data: &BattleData,
    objects: &GlobalObjects,
    magics: &Magics,
) -> Option<BattleEnemy> {
    let object = objects.get(object_id)?;
    let enemy_id = object.enemy_id();
    let enemy = data.enemies.get(enemy_id)?;
    let magic = match enemy.magic {
        0 | u16::MAX => None,
        object_id => Some(battle_magic(object_id, objects, magics)?),
    };
    let attack_equivalent_item_script = match enemy.attack_equivalent_item {
        0 => 0,
        object_id => objects.get(object_id)?.item_use_script(),
    };
    let mut actor = battle_enemy(slot, position, object_id, enemy_id, enemy, object, magic);
    actor.attack_equivalent_item_script = attack_equivalent_item_script;
    Some(actor)
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
        action_wait_frames: enemy.action_wait_frames,
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
        rewards_collected: false,
    }
}

fn battle_magic(object_id: u16, objects: &GlobalObjects, magics: &Magics) -> Option<BattleMagic> {
    let object = objects.get(object_id)?;
    let definition = magics.get(object.magic_number())?;
    let flags = object.magic_flags();
    let summon_effect = if definition.magic_type == 9 {
        Some((0..objects.len()).find_map(|index| {
            let object_id = u16::try_from(index).ok()?;
            let object = objects.get(object_id)?;
            (object.magic_number() == definition.effect)
                .then(|| battle_magic_visual(object_id, objects, magics))?
        })?)
    } else {
        None
    };
    Some(BattleMagic {
        object_id,
        flags,
        effect: definition.effect,
        magic_type: definition.magic_type,
        x_offset: definition.x_offset,
        y_offset: definition.y_offset,
        specific: definition.specific,
        speed: definition.speed,
        keep_effect: definition.keep_effect,
        fire_delay: definition.fire_delay,
        effect_times: definition.effect_times,
        shake: definition.shake,
        wave: definition.wave,
        mp_cost: definition.mp_cost,
        base_damage: definition.base_damage,
        elemental: definition.elemental,
        attacks_all: flags & MAGIC_FLAG_APPLY_TO_ALL != 0,
        sound: definition.sound,
        use_script: object.magic_use_script(),
        success_script: object.magic_success_script(),
        summon_effect,
    })
}

fn battle_magic_visual(
    object_id: u16,
    objects: &GlobalObjects,
    magics: &Magics,
) -> Option<BattleMagicVisual> {
    let object = objects.get(object_id)?;
    let definition = magics.get(object.magic_number())?;
    Some(BattleMagicVisual {
        object_id,
        flags: object.magic_flags(),
        effect: definition.effect,
        magic_type: definition.magic_type,
        x_offset: definition.x_offset,
        y_offset: definition.y_offset,
        specific: definition.specific,
        speed: definition.speed,
        keep_effect: definition.keep_effect,
        fire_delay: definition.fire_delay,
        effect_times: definition.effect_times,
        shake: definition.shake,
        wave: definition.wave,
        sound: definition.sound,
    })
}

fn battle_player(
    role_id: u16,
    role: &PlayerRole,
    objects: &GlobalObjects,
    magics: &Magics,
) -> Option<BattlePlayer> {
    let player_object = objects.get(role.name_word_id)?;
    let battle_magics = role
        .magic
        .iter()
        .copied()
        .filter(|&object_id| object_id != 0)
        .filter_map(|object_id| {
            let magic = battle_magic(object_id, objects, magics)?;
            magic.usable_in_battle().then_some(magic)
        })
        .collect();
    let cooperative_magic = match role.cooperative_magic {
        0 => None,
        object_id => Some(battle_magic(object_id, objects, magics)?),
    };
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
        cooperative_magic,
        defending: false,
        covered_by: role.covered_by,
        friend_death_script: player_object.player_friend_death_script(),
        dying_script: player_object.player_dying_script(),
        death_sound: role.death_sound,
        attack_sound: role.attack_sound,
        weapon_sound: role.weapon_sound,
        critical_sound: role.critical_sound,
        magic_sound: role.magic_sound,
        cover_sound: role.cover_sound,
        dying_sound: role.dying_sound,
        statuses: BattleStatuses::default(),
        poisons: [BattlePoison::default(); MAX_BATTLE_POISONS],
        poison_face_color: None,
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
        players[index].is_alive()
            && !players[index].statuses.is_active(BattleStatus::Sleep)
            && !players[index].statuses.is_active(BattleStatus::Paralyzed)
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
        let mut transformed_enemy = enemy.clone();
        for (word, value) in [
            (5, 7u16),
            (11, 240),
            (12, 77),
            (13, 88),
            (14, 9),
            (21, 99),
            (22, 91),
            (23, 83),
            (24, 75),
        ] {
            transformed_enemy[word * 2..word * 2 + 2].copy_from_slice(&value.to_le_bytes());
        }
        chunks[1] = [enemy, transformed_enemy].concat();
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
        object_words[12..18].copy_from_slice(&[
            0,
            0,
            32,
            31,
            0,
            MAGIC_FLAG_USABLE_IN_BATTLE
                | MAGIC_FLAG_USABLE_TO_ENEMY
                | if magic_type != 0 {
                    MAGIC_FLAG_APPLY_TO_ALL
                } else {
                    0
                },
        ]);
        object_words[18..24].copy_from_slice(&[1, 4, 21, 22, 23, 0]);
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
        assert_eq!(battle.rewards(), BattleRewards::default());
    }

    #[test]
    fn division_uses_free_slots_and_requested_health_divisor() {
        let (data, objects, magics, role) = fixture(100, 10, 100);
        let mut battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        assert!(battle.divide_enemy(0, 2, &data.enemy_positions).is_none());

        battle.enemies[0]
            .statuses
            .set_for_enemy(BattleStatus::Protect, 4);
        assert!(battle.kill_enemy(1));
        battle.queue_post_action_check(false);
        let added = battle
            .divide_enemy(0, 2, &data.enemy_positions)
            .expect("sole living enemy should divide");
        assert_eq!(added, vec![2, 3]);
        assert_eq!(battle.enemies[0].hp, 34);
        assert_eq!(battle.enemies[2].hp, 34);
        assert_eq!(battle.enemies[3].hp, 34);
        assert_eq!(battle.enemies[2].slot, 1);
        assert_eq!(battle.enemies[3].slot, 2);
        assert_eq!(battle.enemies[1].object_id, 0);
        assert_eq!(battle.enemies[1].battle_end_script, 0);
        assert_eq!(battle.enemy_index_for_slot(1), Some(2));
        assert_eq!(battle.enemies[0].position, BattlePosition { x: 12, y: 22 });
        assert_eq!(battle.enemies[2].position, BattlePosition { x: 17, y: 27 });
        assert_eq!(battle.enemies[3].position, BattlePosition { x: 22, y: 32 });
        assert_eq!(battle.enemies[2].turn_start_script, 11);
        assert!(!battle.enemies[2].statuses.is_active(BattleStatus::Protect));
    }

    #[test]
    fn summon_fills_only_current_layout_holes_and_loads_static_enemy_data() {
        let (data, objects, magics, role) = fixture(100, 10, 100);
        let mut battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        assert!(battle
            .summon_enemy(0, 3, 1, &data, &objects, &magics)
            .is_none());
        assert!(battle.kill_enemy(1));
        battle.queue_post_action_check(false);
        assert!(battle
            .summon_enemy(0, 3, 2, &data, &objects, &magics)
            .is_none());

        let added = battle
            .summon_enemy(0, 3, 0, &data, &objects, &magics)
            .expect("zero count should summon one enemy");
        assert_eq!(added, vec![2]);
        let summoned = &battle.enemies[2];
        assert_eq!(summoned.slot, 1);
        assert_eq!(summoned.object_id, 3);
        assert_eq!(summoned.enemy_id, 1);
        assert_eq!(summoned.hp, 240);
        assert_eq!(summoned.attack_strength, 99);
        assert_eq!(summoned.turn_start_script, 21);
        assert_eq!(summoned.battle_end_script, 22);
        assert_eq!(summoned.ready_script, 23);
        assert_eq!(summoned.position, BattlePosition { x: 16, y: 26 });
    }

    #[test]
    fn transform_replaces_static_data_but_preserves_runtime_state_and_scripts() {
        let (data, objects, magics, role) = fixture(100, 10, 100);
        let mut battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        battle.enemies[0].hp = 37;
        battle.enemies[0].turn_start_script = 41;
        battle.enemies[0].battle_end_script = 42;
        battle.enemies[0].ready_script = 43;
        battle.enemies[0]
            .statuses
            .set_for_enemy(BattleStatus::Protect, 4);
        battle.enemies[0].poisons[0] = BattlePoison {
            object_id: 6,
            script_entry: 90,
        };

        assert_eq!(
            battle.transform_enemy(0, 3, &data, &objects, &magics),
            Some(true)
        );
        let enemy = &battle.enemies[0];
        assert_eq!(enemy.object_id, 3);
        assert_eq!(enemy.enemy_id, 1);
        assert_eq!(enemy.hp, 37);
        assert_eq!(enemy.max_hp, 240);
        assert_eq!(enemy.attack_strength, 99);
        assert_eq!(enemy.magic_strength, 91);
        assert_eq!(enemy.defense, 83);
        assert_eq!(enemy.dexterity, 75);
        assert_eq!(enemy.y_offset, 7);
        assert_eq!(enemy.turn_start_script, 41);
        assert_eq!(enemy.battle_end_script, 42);
        assert_eq!(enemy.ready_script, 43);
        assert!(enemy.statuses.is_active(BattleStatus::Protect));
        assert_eq!(enemy.poisons[0].object_id, 6);

        battle.enemies[0]
            .statuses
            .set_for_enemy(BattleStatus::Confused, 1);
        assert_eq!(
            battle.transform_enemy(0, 1, &data, &objects, &magics),
            Some(false)
        );
        assert_eq!(battle.enemies[0].object_id, 3);
    }

    #[test]
    fn collect_steal_and_hiding_follow_battle_instance_state() {
        let (data, objects, magics, mut role) = fixture(1_000, 100, 500);
        role.attack_strength = 10;
        let mut battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut battle);
        assert_eq!(battle.collect_enemy(0), Some(5));
        assert_eq!(battle.steal_enemy(0, 0), Some(BattleSteal::Item(8)));
        assert_eq!(battle.enemies[0].steal_item_count, 1);

        battle.enemies[0].steal_item = 0;
        battle.enemies[0].steal_item_count = 6;
        assert!(matches!(
            battle.steal_enemy(0, 0),
            Some(BattleSteal::Cash(2 | 3))
        ));

        assert!(battle.hide_players(1));
        assert!(battle.attack(0).unwrap().is_empty());
        let events = resolve_until_input_or_finish(&mut battle);
        assert!(events
            .iter()
            .all(|event| !matches!(event, BattleEvent::EnemyAttack { .. })));
        assert_eq!(battle.hiding_time(), 0);
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
        complete_pending_scripts(&mut boss);
        assert!(boss.flee().is_none());
        let mut normal =
            BattleState::new(request(false), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut normal);
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
        assert!(battle.set_magic_blow(-3));
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
                blow: -3,
                damage,
                defeated: _,
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

        assert!(battle.set_magic_blow(-2));
        assert!(battle.simulate_player_magic(0, 2, 80, &objects, &magics));
        assert!(battle.enemies[1].hp < target_hp);
        assert!(matches!(
            battle.advance_resolution().as_slice(),
            [BattleEvent::SimulatedMagic {
                enemy: 1,
                magic_object: 2,
                blow: -2,
                damage,
                defeated: _,
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
    fn unavailable_items_fall_back_when_the_queued_action_executes() {
        let (data, objects, magics, role) = fixture(1_000, 0, 500);
        let mut used =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut used);
        for enemy in &mut used.enemies {
            enemy.statuses.set_for_enemy(BattleStatus::Paralyzed, 2);
        }
        assert!(used.use_item(20, Some(0), 30, true).is_some());
        used.set_inventory_amounts(&[]);
        let used_events = resolve_until_input_or_finish(&mut used);
        assert!(used_events
            .iter()
            .any(|event| matches!(event, BattleEvent::PlayerDefend { player: 0 })));
        assert!(!used_events
            .iter()
            .any(|event| matches!(event, BattleEvent::PlayerUseItem { .. })));

        let mut thrown =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut thrown);
        for enemy in &mut thrown.enemies {
            enemy.statuses.set_for_enemy(BattleStatus::Paralyzed, 2);
        }
        assert!(thrown.throw_item(20, Some(0), 30).is_some());
        thrown.set_inventory_amounts(&[]);
        let thrown_events = resolve_until_input_or_finish(&mut thrown);
        assert!(thrown_events
            .iter()
            .any(|event| matches!(event, BattleEvent::PlayerAttack { player: 0, .. })));
        assert!(!thrown_events
            .iter()
            .any(|event| matches!(event, BattleEvent::PlayerThrowItem { .. })));

        let mut replenished = BattleState::new(
            request(true),
            0,
            7,
            [(0, &role), (1, &role)],
            &data,
            &objects,
            &magics,
        )
        .unwrap();
        complete_pending_scripts(&mut replenished);
        for enemy in &mut replenished.enemies {
            enemy.statuses.set_for_enemy(BattleStatus::Paralyzed, 2);
        }
        assert!(replenished.defend().is_some());
        assert!(replenished.use_item(20, Some(0), 0, true).is_some());
        replenished.set_inventory_amounts(&[]);
        assert_eq!(
            replenished.advance_resolution(),
            vec![BattleEvent::PlayerDefend { player: 0 }]
        );
        replenished.set_inventory_amounts(&[(20, 1)]);
        assert!(matches!(
            replenished.advance_resolution().as_slice(),
            [BattleEvent::PlayerUseItem {
                player: 1,
                item_object: 20,
                ..
            }]
        ));
    }

    #[test]
    fn disabled_players_keep_a_zero_dexterity_recovery_action() {
        let (data, objects, magics, role) = fixture(1_000, 0, 500);
        let mut disabled = role.clone();
        disabled.hp = 0;
        let mut battle = BattleState::new(
            request(true),
            0,
            7,
            [(0, &role), (1, &disabled)],
            &data,
            &objects,
            &magics,
        )
        .unwrap();
        complete_pending_scripts(&mut battle);
        for enemy in &mut battle.enemies {
            enemy.statuses.set_for_enemy(BattleStatus::Paralyzed, 2);
        }

        assert_eq!(battle.active_player(), Some(0));
        assert!(battle.attack(0).is_some());
        assert_eq!(battle.active_player(), None);
        assert!(battle.action_queue.iter().any(|queued| {
            matches!(
                queued,
                QueuedBattleAction {
                    action: BattleActorAction::Player {
                        player: 1,
                        action: PlayerAction::Attack { .. }
                    },
                    dexterity: 0,
                }
            )
        }));

        battle.players[1].hp = 500;
        let events = resolve_until_input_or_finish(&mut battle);
        assert!(events
            .iter()
            .any(|event| matches!(event, BattleEvent::PlayerAttack { player: 1, .. })));

        let mut puppet = BattleState::new(
            request(true),
            0,
            7,
            [(0, &role), (1, &disabled)],
            &data,
            &objects,
            &magics,
        )
        .unwrap();
        complete_pending_scripts(&mut puppet);
        assert!(puppet.players[1]
            .statuses
            .set_for_player(BattleStatus::Puppet, 2, false,));
        for enemy in &mut puppet.enemies {
            enemy.statuses.set_for_enemy(BattleStatus::Paralyzed, 2);
        }
        assert!(puppet.attack(0).is_some());
        assert_eq!(puppet.active_player(), None);
        let events = resolve_until_input_or_finish(&mut puppet);
        assert!(events
            .iter()
            .any(|event| matches!(event, BattleEvent::PlayerAttack { player: 1, .. })));
    }

    #[test]
    fn post_action_checks_queue_friend_death_and_dying_scripts() {
        let (data, mut objects, magics, mut victim) = fixture(1_000, 0, 100);
        objects.get_mut(0).unwrap().data[2] = 70;
        objects.get_mut(0).unwrap().data[3] = 80;
        victim.covered_by = 1;
        victim.dying_sound = 77;
        let cover = victim.clone();

        let mut death = BattleState::new(
            request(true),
            0,
            7,
            [(0, &victim), (1, &cover)],
            &data,
            &objects,
            &magics,
        )
        .unwrap();
        complete_pending_scripts(&mut death);
        death.backup_player_hp();
        death.players[0].hp = 0;
        death.queue_post_action_check(true);
        assert_eq!(
            death.advance_resolution(),
            vec![BattleEvent::PlayerFriendDeath { player: 1 }]
        );
        let script_request = death.take_script_request().unwrap();
        assert_eq!(
            script_request.source,
            BattleScriptSource::PlayerFriendDeath {
                player: 1,
                name_object: 0,
            }
        );
        assert_eq!(script_request.object_id, 1);
        assert!(death.complete_script(71));
        assert_eq!(death.players[1].friend_death_script, 71);

        let mut dying = BattleState::new(
            request(true),
            0,
            7,
            [(0, &victim), (1, &cover)],
            &data,
            &objects,
            &magics,
        )
        .unwrap();
        complete_pending_scripts(&mut dying);
        dying.backup_player_hp();
        dying.players[0].hp = 10;
        dying.queue_post_action_check(true);
        assert_eq!(
            dying.advance_resolution(),
            vec![BattleEvent::PlayerDying { player: 0 }]
        );
        let script_request = dying.take_script_request().unwrap();
        assert_eq!(
            script_request.source,
            BattleScriptSource::PlayerDying {
                player: 0,
                name_object: 0,
            }
        );
        assert_eq!(script_request.object_id, 0);
        assert!(dying.complete_script(81));
        assert_eq!(dying.players[0].dying_script, 81);
        assert_eq!(dying.players[0].dying_sound, 77);
    }

    #[test]
    fn scripted_victory_rewards_only_enemies_already_defeated() {
        let (data, objects, magics, role) = fixture(100, 0, 500);
        let mut battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut battle);
        battle.enemies[0].hp = 0;
        battle.queue_post_action_check(false);
        assert_eq!(
            battle.rewards(),
            BattleRewards {
                experience: 26,
                cash: 48,
            }
        );

        assert!(battle.set_script_result(3));
        assert!(battle.advance_resolution().is_empty());
        complete_pending_scripts(&mut battle);
        assert_eq!(
            battle.advance_resolution(),
            vec![BattleEvent::Finished(BattleResult::Won)]
        );
        assert_eq!(
            battle.settled_rewards(),
            Some(BattleRewards {
                experience: 26,
                cash: 48,
            })
        );
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
        assert!(battle.set_magic_blow(2));
        let events = battle.perform_enemy_magic(0, 0, magic);
        assert!(matches!(
            events.as_slice(),
            [
                BattleEvent::EnemyMagic {
                    player: 0,
                    blow: 2,
                    visual: true,
                    ..
                },
                BattleEvent::EnemyMagic {
                    player: 1,
                    blow: 0,
                    visual: false,
                    ..
                }
            ]
        ));

        battle.players[0].hp = 500;
        battle.random_state = 1234;
        let normal = battle.enemy_magic_damage(0, 0, magic, false);
        battle.random_state = 1234;
        let auto_defended = battle.enemy_magic_damage(0, 0, magic, true);
        assert_eq!(auto_defended, (normal / 2).max(1));
        battle.players[0]
            .statuses
            .set_for_player(BattleStatus::Protect, 1, true);
        battle.random_state = 1234;
        let protected = battle.enemy_magic_damage(0, 0, magic, false);
        assert_eq!(protected, (normal / 2).max(1));
    }

    #[test]
    fn all_target_magic_plays_the_full_visual_only_for_the_first_target() {
        let (data, objects, magics, role) = fixture_with_enemy_magic(1000, 0, 500, 0, 0, 1);
        let mut battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut battle);
        let magic = battle_magic(2, &objects, &magics).unwrap();

        let player_events = battle.perform_player_magic(0, BattleTarget::AllEnemies, magic);
        assert!(matches!(
            player_events.as_slice(),
            [
                BattleEvent::PlayerMagic { visual: true, .. },
                BattleEvent::PlayerMagic { visual: false, .. }
            ]
        ));

        battle.enemies.iter_mut().for_each(|enemy| enemy.hp = 1000);
        assert!(battle.simulate_player_magic(0, 2, 20, &objects, &magics));
        assert!(matches!(
            battle.pending_events.make_contiguous(),
            [
                BattleEvent::SimulatedMagic { visual: true, .. },
                BattleEvent::SimulatedMagic { visual: false, .. }
            ]
        ));
    }

    #[test]
    fn summon_magic_resolves_the_referenced_offensive_visual() {
        let mut object_words = vec![0u16; 4 * 6];
        object_words[2 * 6] = 0;
        object_words[2 * 6 + 5] = MAGIC_FLAG_USABLE_IN_BATTLE | MAGIC_FLAG_USABLE_TO_ENEMY;
        object_words[3 * 6] = 1;
        object_words[3 * 6 + 5] =
            MAGIC_FLAG_USABLE_IN_BATTLE | MAGIC_FLAG_USABLE_TO_ENEMY | MAGIC_FLAG_APPLY_TO_ALL;
        let objects = GlobalObjects::parse(&words(&object_words), ObjectLayout::Dos).unwrap();

        let mut magic_data = vec![0u8; 64];
        magic_data[0..2].copy_from_slice(&1u16.to_le_bytes());
        magic_data[2..4].copy_from_slice(&9u16.to_le_bytes());
        magic_data[8..10].copy_from_slice(&4i16.to_le_bytes());
        magic_data[32..34].copy_from_slice(&7u16.to_le_bytes());
        magic_data[34..36].copy_from_slice(&2u16.to_le_bytes());
        magic_data[38..40].copy_from_slice(&(-3i16).to_le_bytes());
        magic_data[46..48].copy_from_slice(&2u16.to_le_bytes());
        magic_data[48..50].copy_from_slice(&3u16.to_le_bytes());
        let magics = Magics::parse(&magic_data).unwrap();

        let summon = battle_magic(2, &objects, &magics).unwrap();
        let effect = summon.summon_effect.unwrap();
        assert_eq!(summon.magic_type, 9);
        assert_eq!(summon.specific, 4);
        assert_eq!(effect.object_id, 3);
        assert_eq!(effect.effect, 7);
        assert_eq!(effect.magic_type, 2);
        assert_eq!(effect.y_offset, -3);
        assert_eq!(effect.fire_delay, 2);
        assert_eq!(effect.effect_times, 3);
        assert!(effect.usable_to_enemy());

        magic_data[0..2].copy_from_slice(&9u16.to_le_bytes());
        let invalid_magics = Magics::parse(&magic_data).unwrap();
        assert!(battle_magic(2, &objects, &invalid_magics).is_none());
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
    fn random_float_is_applied_before_classic_integer_truncation() {
        fn next(mut value: u32) -> u32 {
            value ^= value << 13;
            value ^= value >> 17;
            value ^= value << 5;
            value
        }

        let (data, objects, magics, role) = fixture(1000, 0, 1000);
        let mut battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut battle);

        battle.random_state = 1;
        let roll = next(1);
        let expected = (1000.0 * (0.9 + (roll >> 1) as f32 / i32::MAX as f32 * 0.2)) as i32;
        assert_eq!(battle.jitter_dexterity(1000), expected);

        battle.random_state = 1;
        let expected = (1000.0 * (10.0 + (roll >> 1) as f32 / i32::MAX as f32)) as u32 / 10;
        assert_eq!(battle.randomized_magic_strength(1000), expected);

        battle.random_state = 1;
        let attack_roll = roll;
        let float_roll = next(attack_roll);
        let defender = &battle.enemies[0];
        let defense = u32::from(defender.defense)
            .saturating_add(u32::from(defender.level.saturating_add(6)) * 4);
        let base = physical_damage(
            u32::from(battle.players[0].attack_strength),
            defense,
            u32::from(defender.physical_resistance),
        )
        .saturating_add(1 + attack_roll % 2);
        let expected =
            (base as f32 * (1.0 + (float_roll >> 1) as f32 / i32::MAX as f32 * 0.125)) as u16;
        assert_eq!(
            battle.player_single_attack_damage(0, 0, false, false),
            (expected.max(1), false)
        );
    }

    #[test]
    fn player_attack_critical_bonus_and_attack_all_follow_classic_rules() {
        let (data, objects, magics, mut role) = fixture(1000, 0, 500);
        role.attack_all = true;
        role.dexterity = 100;
        let mut battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut battle);
        battle.players[0]
            .statuses
            .set_for_player(BattleStatus::Bravery, 2, true);
        for enemy in &mut battle.enemies {
            enemy.statuses.set_for_enemy(BattleStatus::Paralyzed, 2);
        }
        assert!(battle.attack(0).is_some());
        let attacks = resolve_until_input_or_finish(&mut battle)
            .into_iter()
            .filter_map(|event| match event {
                BattleEvent::PlayerAttack {
                    enemy,
                    damage,
                    critical,
                    ..
                } => Some((enemy, damage, critical)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(attacks.len(), 2);
        assert_eq!([attacks[0].0, attacks[1].0], [1, 0]);
        assert!(attacks.iter().all(|attack| attack.2));
        assert!(attacks[0].1 >= attacks[1].1.saturating_mul(2));
        let counts = battle.hidden_experience_counts(0).unwrap();
        assert_eq!(counts[HIDDEN_EXP_ATTACK], 1);
        assert!((2..=3).contains(&counts[HIDDEN_EXP_HEALTH]));

        let (data, objects, magics, role) = fixture(1000, 0, 500);
        let base =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        let mut normal = base.clone();
        let mut critical = base.clone();
        let mut bonus = base;
        let normal_damage = normal.player_single_attack_damage(0, 0, false, false).0;
        let critical_damage = critical.player_single_attack_damage(0, 0, true, false).0;
        let bonus_damage = bonus.player_single_attack_damage(0, 0, false, true).0;
        assert!(critical_damage >= normal_damage.saturating_mul(3).saturating_sub(2));
        assert!(bonus_damage >= normal_damage.saturating_mul(2).saturating_sub(1));
    }

    #[test]
    fn enemy_physical_attack_can_be_covered_or_auto_defended() {
        let (data, objects, magics, mut role) = fixture(1000, 80, 500);
        role.hp = 20;
        role.covered_by = 1;
        let mut cover = role.clone();
        cover.hp = 500;
        cover.covered_by = 0;
        let mut battle = BattleState::new(
            request(true),
            0,
            7,
            [(0, &role), (1, &cover)],
            &data,
            &objects,
            &magics,
        )
        .unwrap();
        complete_pending_scripts(&mut battle);

        let mut covered = false;
        let mut defended = false;
        for _ in 0..256 {
            battle.players[0].hp = 20;
            battle.players[1].hp = 500;
            match battle.perform_enemy_action(0) {
                Some(BattleEvent::EnemyAttack {
                    player: 0,
                    damage: 0,
                    protected_by: Some(1),
                    auto_defended: true,
                    ..
                }) => covered = true,
                Some(BattleEvent::EnemyAttack {
                    damage: 0,
                    protected_by: None,
                    auto_defended: true,
                    ..
                }) => defended = true,
                _ => {}
            }
            if covered && defended {
                break;
            }
        }
        assert!(covered);
        assert!(defended);
    }

    #[test]
    fn forced_action_uses_randomized_offensive_magic_and_skips_ultimate_moves() {
        let (data, objects, magics, mut role) = fixture(1000, 0, 500);
        role.magic[0] = 2;
        role.max_mp = 10;
        role.mp = 10;
        let mut battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut battle);
        assert!(battle.commit_auto_action(60).is_some());
        assert!(matches!(
            battle.player_actions[0],
            Some(PlayerAction::Magic { magic: 0, .. })
        ));

        let mut ultimate =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut ultimate);
        ultimate.players[0].magics[0].mp_cost = 1;
        assert!(ultimate.commit_auto_action(9999).is_some());
        assert!(matches!(
            ultimate.player_actions[0],
            Some(PlayerAction::Attack { .. })
        ));
    }

    #[test]
    fn repeat_round_fallback_does_not_replace_the_previous_action_cache() {
        let (data, objects, magics, mut role) = fixture(10_000, 0, 500);
        role.magic[0] = 2;
        role.mp = 5;
        role.max_mp = 5;
        role.dexterity = 100;
        let mut battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut battle);
        for enemy in &mut battle.enemies {
            enemy.statuses.set_for_enemy(BattleStatus::Paralyzed, 8);
        }

        assert!(battle.cast_magic(0, 0).is_some());
        let events = resolve_until_input_or_finish(&mut battle);
        assert!(matches!(events.last(), Some(BattleEvent::RoundCompleted)));
        assert_eq!(battle.players[0].mp, 0);
        assert!(matches!(
            battle.previous_player_actions[0],
            Some(PlayerAction::Magic { magic: 0, .. })
        ));

        assert!(battle.repeat_last_action().is_some());
        assert!(matches!(
            battle.player_actions[0],
            Some(PlayerAction::Attack { .. })
        ));
        let events = resolve_until_input_or_finish(&mut battle);
        assert!(matches!(events.last(), Some(BattleEvent::RoundCompleted)));
        assert!(matches!(
            battle.previous_player_actions[0],
            Some(PlayerAction::Magic { magic: 0, .. })
        ));

        battle.players[0].mp = 5;
        assert!(battle.repeat_last_action().is_some());
        assert!(matches!(
            battle.player_actions[0],
            Some(PlayerAction::Magic { magic: 0, .. })
        ));

        let mut item_battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut item_battle);
        item_battle.previous_player_actions[0] = Some(PlayerAction::UseItem {
            item_object: 20,
            target: Some(0),
            script_entry: 30,
            consuming: true,
        });
        assert!(item_battle.repeat_unavailable_item(false).is_some());
        assert!(matches!(
            item_battle.previous_player_actions[0],
            Some(PlayerAction::UseItem {
                item_object: 20,
                ..
            })
        ));
    }

    #[test]
    fn automatic_attack_propagates_during_execution_without_reordering_actions() {
        let (data, objects, magics, mut leader) = fixture(10_000, 0, 500);
        leader.dexterity = 1000;
        let mut follower = leader.clone();
        follower.dexterity = 1;
        let mut battle = BattleState::new(
            request(true),
            0,
            7,
            [(0, &leader), (1, &follower)],
            &data,
            &objects,
            &magics,
        )
        .unwrap();
        complete_pending_scripts(&mut battle);
        for enemy in &mut battle.enemies {
            enemy.statuses.set_for_enemy(BattleStatus::Paralyzed, 2);
        }

        assert!(battle.attack_automatically(0).is_some());
        assert!(battle.defend().is_some());
        assert!(battle.previous_round_used_auto_attack());
        let follower_dexterity = battle
            .action_queue
            .iter()
            .find_map(|queued| match queued.action {
                BattleActorAction::Player {
                    player: 1,
                    action: PlayerAction::Defend,
                } => Some(queued.dexterity),
                _ => None,
            })
            .expect("the follower should enter the queue with defend dexterity");
        assert!(follower_dexterity < 10);

        assert!(matches!(
            battle.advance_resolution().as_slice(),
            [BattleEvent::PlayerAttack { player: 0, .. }]
        ));
        assert!(matches!(
            battle.advance_resolution().as_slice(),
            [BattleEvent::PlayerAttack { player: 1, .. }]
        ));
        assert!(matches!(
            battle.player_actions[1],
            Some(PlayerAction::Attack { .. })
        ));

        let mut stopped = BattleState::new(
            request(true),
            0,
            7,
            [(0, &leader), (1, &follower)],
            &data,
            &objects,
            &magics,
        )
        .unwrap();
        complete_pending_scripts(&mut stopped);
        assert!(stopped.attack_automatically(0).is_some());
        stopped.set_auto_attack_mode(false);
        assert!(stopped.defend().is_some());
        assert!(!stopped.previous_round_used_auto_attack());
    }

    #[test]
    fn defensive_magic_uses_player_targets_and_original_script_owners() {
        let (data, mut objects, magics, mut role) = fixture_with_enemy_magic(500, 20, 500, 0, 0, 4);
        role.max_mp = 20;
        role.mp = 20;
        role.magic[0] = 2;
        objects.get_mut(2).unwrap().data[6] = MAGIC_FLAG_USABLE_IN_BATTLE;
        let mut battle = BattleState::new(
            request(true),
            0,
            7,
            [(0, &role), (1, &role)],
            &data,
            &objects,
            &magics,
        )
        .unwrap();
        complete_pending_scripts(&mut battle);

        assert!(battle.cast_magic_at(0, BattleTarget::Player(1)).is_some());
        assert!(battle.attack(0).is_some());
        assert!(battle.advance_resolution().is_empty());
        let use_script = battle.take_script_request().unwrap();
        assert_eq!(use_script.object_id, 0);
        assert!(battle.complete_script(use_script.entry + 1));
        assert!(battle.advance_resolution().is_empty());
        let success_script = battle.take_script_request().unwrap();
        assert_eq!(success_script.object_id, 1);
        assert!(battle.complete_script(success_script.entry + 1));
        assert_eq!(
            battle.advance_resolution(),
            vec![BattleEvent::PlayerDefensiveMagic {
                player: 0,
                target: BattleTarget::Player(1),
                magic_object: 2,
            }]
        );
        assert_eq!(battle.players[0].mp, 15);
    }

    #[test]
    fn defend_reduces_damage_and_expires_after_the_round() {
        let (data, objects, magics, role) = fixture_with_enemy_magic(500, 20, 500, 0, 0, 0);
        let mut plain =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        let mut guarded = plain.clone();
        complete_pending_scripts(&mut plain);
        complete_pending_scripts(&mut guarded);
        let plain_damage = plain.enemy_damage(0, 0);
        guarded.players[0].defending = true;
        let guarded_damage = guarded.enemy_damage(0, 0);
        assert!(guarded_damage < plain_damage);

        guarded.players[0].defending = false;
        assert!(guarded.defend().is_some());
        let events = resolve_until_input_or_finish(&mut guarded);
        assert!(events
            .iter()
            .any(|event| matches!(event, BattleEvent::PlayerDefend { player: 0 })));
        assert!(!guarded.players[0].defending);
        assert_eq!(
            guarded.hidden_experience_counts(0).unwrap()[HIDDEN_EXP_DEFENSE],
            2
        );
    }

    #[test]
    fn cooperative_magic_uses_healthy_party_hp_and_skips_their_other_actions() {
        let (data, objects, magics, mut role) = fixture_with_enemy_magic(500, 0, 500, 0, 0, 0);
        role.cooperative_magic = 2;
        role.magic_strength = 80;
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
        for enemy in &mut battle.enemies {
            enemy.statuses.set_for_enemy(BattleStatus::Paralyzed, 2);
        }

        assert!(battle.can_use_cooperative_magic());
        assert!(battle
            .cast_cooperative_magic(BattleTarget::Enemy(0))
            .is_some());
        assert!(battle.attack(0).is_some());
        let events = resolve_until_input_or_finish(&mut battle);

        assert!(events.iter().any(|event| matches!(
            event,
            BattleEvent::PlayerCooperativeMagic {
                player: 0,
                enemy: 0,
                magic_object: 2,
                damage,
                ..
            } if *damage > 0
        )));
        assert!(!events
            .iter()
            .any(|event| matches!(event, BattleEvent::PlayerAttack { .. })));
        assert_eq!(battle.players[0].hp, 495);
        assert_eq!(battle.players[1].hp, 495);
    }

    #[test]
    fn command_undo_releases_item_reservations_and_flee_can_cover_the_party() {
        let (data, objects, magics, role) = fixture_with_enemy_magic(500, 20, 500, 0, 0, 0);
        let mut battle = BattleState::new(
            request(false),
            0,
            7,
            [(0, &role), (1, &role)],
            &data,
            &objects,
            &magics,
        )
        .unwrap();
        complete_pending_scripts(&mut battle);
        assert!(battle.use_item(7, Some(0), 0, true).is_some());
        assert_eq!(battle.reserved_item_count(7), 1);
        assert_eq!(battle.undo_last_command(), Some(0));
        assert_eq!(battle.reserved_item_count(7), 0);
        assert_eq!(battle.active_player(), Some(0));

        assert!(battle.attempt_flee_all().is_some());
        assert!(battle
            .player_actions
            .iter()
            .all(|action| { matches!(action, Some(PlayerAction::Flee)) }));
        assert!(matches!(battle.flow, BattleFlow::PerformActions));
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
        battle.players[0]
            .statuses
            .set_for_player(BattleStatus::Protect, 2, true);
        battle.enemies[0]
            .statuses
            .set_for_enemy(BattleStatus::Protect, 2);
        battle.players[0].defending = true;
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
        assert!(!battle.players[0].defending);
        assert_eq!(
            player_poison.source,
            BattleScriptSource::PlayerPoison {
                role_id: 0,
                poison_id: 40,
            }
        );
        assert!(battle.complete_script(33));
        assert_eq!(
            battle.players[0].statuses.duration(BattleStatus::Protect),
            2
        );
        assert!(battle.advance_resolution().is_empty());
        assert_eq!(
            battle.players[0].statuses.duration(BattleStatus::Protect),
            1
        );
        assert_eq!(
            battle.enemies[0].statuses.duration(BattleStatus::Protect),
            2
        );
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
        assert_eq!(
            battle.enemies[0].statuses.duration(BattleStatus::Protect),
            1
        );
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
    fn lifecycle_scripts_read_each_enemy_slot_after_the_previous_script() {
        let (data, objects, magics, role) = fixture(1000, 0, 1000);
        let mut battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();

        let first = battle.take_script_request().unwrap();
        assert_eq!(
            first.source,
            BattleScriptSource::EnemyTurnStart { enemy: 0 }
        );
        battle.enemies[1].turn_start_script = 71;
        assert!(battle.complete_script(21));
        let second = battle.take_script_request().unwrap();
        assert_eq!(
            second,
            BattleScriptRequest {
                source: BattleScriptSource::EnemyTurnStart { enemy: 1 },
                entry: 71,
                object_id: 1,
            }
        );
        assert!(battle.complete_script(72));

        assert!(battle.set_script_result(3));
        assert!(battle.advance_resolution().is_empty());
        let first = battle.take_script_request().unwrap();
        assert_eq!(
            first.source,
            BattleScriptSource::EnemyBattleEnd { enemy: 0 }
        );
        battle.enemies[1].battle_end_script = 81;
        assert!(battle.complete_script(22));
        let second = battle.take_script_request().unwrap();
        assert_eq!(
            second,
            BattleScriptRequest {
                source: BattleScriptSource::EnemyBattleEnd { enemy: 1 },
                entry: 81,
                object_id: 1,
            }
        );
        assert!(battle.complete_script(82));
        assert_eq!(
            battle.advance_resolution(),
            vec![BattleEvent::Finished(BattleResult::Won)]
        );
    }

    #[test]
    fn round_poison_scripts_read_one_live_slot_at_a_time() {
        let (data, objects, magics, role) = fixture(1000, 0, 1000);
        let mut battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        complete_pending_scripts(&mut battle);
        for enemy in &mut battle.enemies {
            enemy.turn_start_script = 0;
        }
        battle.players[0].poisons[0] = BattlePoison {
            object_id: 40,
            script_entry: 31,
        };
        battle.players[0].poisons[1] = BattlePoison {
            object_id: 41,
            script_entry: 32,
        };
        battle.players[0]
            .statuses
            .set_for_player(BattleStatus::Protect, 2, true);
        battle.flow = BattleFlow::RoundScripts {
            actor: 0,
            poison_slot: 0,
        };

        assert!(battle.advance_resolution().is_empty());
        let first = battle.take_script_request().unwrap();
        assert_eq!(
            first.source,
            BattleScriptSource::PlayerPoison {
                role_id: 0,
                poison_id: 40,
            }
        );
        assert!(cure_poison(&mut battle.players[0].poisons, 40));
        assert!(battle.complete_script(33));

        assert_eq!(
            battle.advance_resolution(),
            vec![BattleEvent::RoundCompleted]
        );
        assert!(!battle.has_script_work());
        assert_eq!(battle.players[0].poisons[0].object_id, 41);
        assert_eq!(battle.players[0].poisons[0].script_entry, 32);
        assert_eq!(
            battle.players[0].statuses.duration(BattleStatus::Protect),
            1
        );

        battle.players[0].poisons[0] = BattlePoison {
            object_id: 40,
            script_entry: 41,
        };
        battle.players[0].poisons[1] = BattlePoison {
            object_id: 41,
            script_entry: 42,
        };
        battle.flow = BattleFlow::RoundScripts {
            actor: 0,
            poison_slot: 0,
        };
        assert!(battle.advance_resolution().is_empty());
        let first = battle.take_script_request().unwrap();
        assert_eq!(first.entry, 41);
        battle.players[0].poisons[1] = BattlePoison {
            object_id: 42,
            script_entry: 99,
        };
        assert!(battle.complete_script(43));
        assert!(battle.advance_resolution().is_empty());
        let second = battle.take_script_request().unwrap();
        assert_eq!(
            second,
            BattleScriptRequest {
                source: BattleScriptSource::PlayerPoison {
                    role_id: 0,
                    poison_id: 42,
                },
                entry: 99,
                object_id: 0,
            }
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
