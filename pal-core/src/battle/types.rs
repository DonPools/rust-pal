use std::collections::VecDeque;

use pal_assets::battle::BattlePosition;
use pal_assets::objects::GlobalObjects;

use crate::party::MAX_PARTY_MEMBERS;

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
pub(super) enum BattleFlow {
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
        script_entry: u16,
        object_id: u16,
        player_stats_before: [Option<(u16, u16)>; MAX_PARTY_MEMBERS],
        phase: PlayerItemPhase,
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
pub(super) enum VictorySettlementStage {
    NotApplicable,
    RewardsPending,
    ScriptsPending,
    ScriptsRunning,
    ReadyToLeave,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EnemyMagicPhase {
    UseScript,
    Animation,
    SuccessScript,
    Damage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PlayerMagicPhase {
    UseScript,
    Animation,
    SuccessScript,
    Damage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PlayerItemKind {
    Use { consuming: bool },
    Throw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PlayerItemPhase {
    Animation,
    Script,
    Resolve,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PlayerAction {
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
pub(super) enum BattleActorAction {
    Player { player: usize, action: PlayerAction },
    Enemy { enemy: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct QueuedBattleAction {
    pub(super) action: BattleActorAction,
    pub(super) dexterity: i32,
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
    pub(super) rewards_collected: bool,
}

impl BattleEnemy {
    pub fn is_alive(&self) -> bool {
        self.object_id != 0 && (self.hp as i16) > 0
    }

    pub(super) fn is_present(&self) -> bool {
        self.object_id != 0
    }

    pub(super) fn is_defeated(&self) -> bool {
        self.is_present() && (self.hp as i16) <= 0
    }

    pub fn can_act(&self) -> bool {
        self.is_alive()
            && !self.statuses.is_active(BattleStatus::Sleep)
            && !self.statuses.is_active(BattleStatus::Paralyzed)
    }

    /// Classic treats the raw enemy attack as signed before applying its level correction.
    pub fn effective_attack_strength(&self) -> u16 {
        let attack = i32::from(self.attack_strength as i16) + (i32::from(self.level) + 6) * 6;
        attack.max(0) as u16
    }

    /// Classic adds the level correction while the defense value is still a 16-bit word.
    pub fn effective_defense(&self) -> u16 {
        self.defense
            .wrapping_add(self.level.wrapping_add(6).wrapping_mul(4))
    }

    /// Script-simulated magic first treats the raw defense as signed and clamps negatives.
    pub fn simulated_magic_defense(&self) -> u16 {
        let defense = i32::from(self.defense as i16) + (i32::from(self.level) + 6) * 4;
        defense.max(0) as u16
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattleEvent {
    PlayerAttack {
        player: usize,
        enemy: usize,
        damage: u16,
        critical: bool,
        visual: bool,
        defeated: bool,
    },
    PlayerMagic {
        player: usize,
        enemy: usize,
        magic_object: u16,
        blow: i16,
        damage: u16,
        phase: MagicEventPhase,
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
        phase: MagicEventPhase,
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
        magic: BattleMagic,
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
    PlayerItemFeedback {
        player: usize,
        item_object: u16,
        consume: bool,
        player_changes: [BattlePlayerStatChange; MAX_PARTY_MEMBERS],
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
    EnemyDivide {
        origin: BattlePosition,
    },
    EnemySummon {
        caster: usize,
        summoned_mask: u8,
    },
    EnemyTransform {
        enemy: usize,
        previous_enemy_id: u16,
        previous_y_offset: u16,
    },
    EnemyEscape,
    RoundCompleted,
    Finished(BattleResult),
}

/// Whether a magic event is the pre-damage effect or the post-damage feedback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MagicEventPhase {
    Visual,
    Feedback,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BattleRewards {
    pub experience: u32,
    pub cash: u32,
}

/// Signed HP/MP changes displayed after a battle item script completes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BattlePlayerStatChange {
    pub hp: i16,
    pub mp: i16,
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
    pub(super) enemy_layout_slots: usize,
    pub(super) hiding_time: u16,
    pub(super) phase: BattlePhase,
    pub(super) active_player: Option<usize>,
    pub(super) acted: Vec<bool>,
    pub(super) player_actions: Vec<Option<PlayerAction>>,
    pub(super) previous_player_actions: Vec<Option<PlayerAction>>,
    pub(super) automatic_player_attacks: Vec<bool>,
    pub(super) previous_automatic_player_attacks: Vec<bool>,
    pub(super) repeating_round: bool,
    pub(super) auto_attack_mode: bool,
    pub(super) previous_auto_attack: bool,
    pub(super) execution_auto_attack: bool,
    pub(super) hidden_experience_counts: Vec<[u16; HIDDEN_EXPERIENCE_CATEGORY_COUNT]>,
    pub(super) cooperative_magic_performed: bool,
    pub(super) action_queue: Vec<QueuedBattleAction>,
    pub(super) action_index: usize,
    pub(super) temporary_player_stats: Vec<[u16; 6]>,
    pub(super) base_player_battle_sprites: Vec<u16>,
    pub(super) inventory_amounts: Option<Vec<(u16, u16)>>,
    pub(super) previous_player_hp: Vec<u16>,
    pub(super) round_start_player_hp: Vec<u16>,
    pub(super) round_start_enemy_hp: Vec<u16>,
    pub(super) gained_rewards: BattleRewards,
    pub(super) auto_battle: bool,
    pub(super) round: u32,
    pub(super) random_state: u32,
    pub(super) battlefield_magic_effect: [i16; pal_assets::battle::MAGIC_ELEMENT_COUNT],
    pub(super) magic_blow: i16,
    pub(super) flow: BattleFlow,
    pub(super) pending_scripts: VecDeque<BattleScriptRequest>,
    pub(super) active_script: Option<BattleScriptRequest>,
    pub(super) deferred_round_result: Option<BattleResult>,
    pub(super) victory_settlement: VictorySettlementStage,
    pub(super) pending_events: VecDeque<BattleEvent>,
}
