//! Deterministic, platform-independent battle state and minimum combat rules.

use pal_assets::battle::{BattleData, BattlePosition, Enemy};
use pal_assets::magic::Magics;
use pal_assets::objects::{GlobalObject, GlobalObjects};
use pal_assets::player_roles::PlayerRole;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattleResult {
    Won,
    Lost,
    Fled,
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
    pub attacks_all: bool,
    pub magics: Vec<BattleMagic>,
    pub death_sound: u16,
    pub attack_sound: u16,
    pub weapon_sound: u16,
    pub magic_sound: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BattleMagic {
    pub object_id: u16,
    pub mp_cost: u16,
    pub base_damage: u16,
    pub elemental: u16,
    pub attacks_all: bool,
    pub sound: i16,
}

impl BattlePlayer {
    pub fn is_alive(&self) -> bool {
        self.hp > 0
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
    pub defense: u16,
    pub dexterity: u16,
    pub physical_resistance: u16,
    pub poison_resistance: u16,
    pub elemental_resistance: [u16; pal_assets::battle::MAGIC_ELEMENT_COUNT],
    pub idle_frames: u16,
    pub attack_frames: u16,
    pub idle_animation_speed: u16,
    pub attack_sound: i16,
    pub action_sound: i16,
    pub death_sound: i16,
    pub call_sound: i16,
    pub turn_start_script: u16,
    pub battle_end_script: u16,
    pub ready_script: u16,
}

impl BattleEnemy {
    pub fn is_alive(&self) -> bool {
        self.hp > 0
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
                battle_enemy(slot, layout.len(), object_id, enemy_id, enemy, object, data)
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
        })
    }

    pub fn phase(&self) -> BattlePhase {
        self.phase
    }

    pub fn round(&self) -> u32 {
        self.round
    }

    pub fn active_player(&self) -> Option<usize> {
        (self.phase == BattlePhase::AwaitingCommand)
            .then_some(self.active_player)
            .flatten()
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
            BattlePhase::Finished(BattleResult::Lost | BattleResult::Fled) => {
                Some(BattleRewards::default())
            }
        }
    }

    /// Execute a normal attack and, after every living player has acted, the enemy round.
    pub fn attack(&mut self, target: usize) -> Option<Vec<BattleEvent>> {
        if self.phase != BattlePhase::AwaitingCommand || !self.enemies.get(target)?.is_alive() {
            return None;
        }
        let player_index = self.active_player?;
        if !self.players.get(player_index)?.is_alive() || self.acted[player_index] {
            return None;
        }

        let mut events = Vec::new();
        let targets = if self.players[player_index].attacks_all {
            self.enemies
                .iter()
                .enumerate()
                .filter_map(|(index, enemy)| enemy.is_alive().then_some(index))
                .collect::<Vec<_>>()
        } else {
            vec![target]
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
        self.end_player_action(player_index, &mut events);
        Some(events)
    }

    pub fn cast_magic(&mut self, magic_index: usize, target: usize) -> Option<Vec<BattleEvent>> {
        if self.phase != BattlePhase::AwaitingCommand || !self.enemies.get(target)?.is_alive() {
            return None;
        }
        let player_index = self.active_player?;
        if !self.players.get(player_index)?.is_alive() || self.acted[player_index] {
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
        if self.phase != BattlePhase::AwaitingCommand || self.is_boss {
            return None;
        }
        self.phase = BattlePhase::Finished(BattleResult::Fled);
        self.active_player = None;
        Some(BattleEvent::Finished(BattleResult::Fled))
    }

    fn run_enemy_round(&mut self, events: &mut Vec<BattleEvent>) {
        for enemy_index in 0..self.enemies.len() {
            if !self.enemies[enemy_index].is_alive() {
                continue;
            }
            let living_players = self
                .players
                .iter()
                .enumerate()
                .filter_map(|(index, player)| player.is_alive().then_some(index))
                .collect::<Vec<_>>();
            if living_players.is_empty() {
                break;
            }
            let target = living_players[self.random(living_players.len() as u32) as usize];
            let damage = self.enemy_damage(enemy_index, target);
            let player = &mut self.players[target];
            player.hp = player.hp.saturating_sub(damage);
            events.push(BattleEvent::EnemyAttack {
                enemy: enemy_index,
                player: target,
                damage,
                defeated: !player.is_alive(),
            });
        }
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
        self.run_enemy_round(events);
        if self.players.iter().all(|player| !player.is_alive()) {
            self.finish(BattleResult::Lost, events);
            return;
        }
        self.round = self.round.saturating_add(1);
        self.acted.fill(false);
        self.active_player = next_player(&self.players, &self.acted, 0);
        events.push(BattleEvent::RoundCompleted);
    }

    fn player_damage(&mut self, player: usize, enemy: usize) -> u16 {
        let attacker = &self.players[player];
        let defender = &self.enemies[enemy];
        let defense = u32::from(defender.defense)
            .saturating_add(u32::from(defender.level.saturating_add(6)) * 4);
        let damage = physical_damage(
            u32::from(attacker.attack_strength),
            defense,
            u32::from(defender.physical_resistance),
        )
        .saturating_add(1 + self.random(2));
        u16::try_from(damage.max(1)).unwrap_or(u16::MAX)
    }

    fn enemy_damage(&mut self, enemy: usize, player: usize) -> u16 {
        let attacker = &self.enemies[enemy];
        let raw_strength = i32::from(attacker.attack_strength as i16)
            + i32::from(attacker.level.saturating_add(6)) * 6;
        let attack = u32::try_from(raw_strength.max(0)).unwrap_or(0) + self.random(3);
        let damage = physical_damage(attack, u32::from(self.players[player].defense), 2)
            .saturating_add(self.random(2));
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
        self.phase = BattlePhase::Finished(result);
        self.active_player = None;
        events.push(BattleEvent::Finished(result));
    }
}

fn battle_enemy(
    slot: usize,
    team_size: usize,
    object_id: u16,
    enemy_id: u16,
    enemy: &Enemy,
    object: &GlobalObject,
    data: &BattleData,
) -> Option<BattleEnemy> {
    Some(BattleEnemy {
        slot,
        object_id,
        enemy_id,
        position: data.enemy_positions.get(team_size, slot)?,
        y_offset: enemy.y_offset,
        hp: enemy.health,
        max_hp: enemy.health,
        experience: enemy.experience,
        cash: enemy.cash,
        level: enemy.level,
        attack_strength: enemy.attack_strength,
        defense: enemy.defense,
        dexterity: enemy.dexterity,
        physical_resistance: enemy.physical_resistance,
        poison_resistance: enemy.poison_resistance,
        elemental_resistance: enemy.elemental_resistance,
        idle_frames: enemy.idle_frames,
        attack_frames: enemy.attack_frames,
        idle_animation_speed: enemy.idle_animation_speed,
        attack_sound: enemy.attack_sound,
        action_sound: enemy.action_sound,
        death_sound: enemy.death_sound,
        call_sound: enemy.call_sound,
        turn_start_script: object.enemy_turn_start_script(),
        battle_end_script: object.enemy_battle_end_script(),
        ready_script: object.enemy_ready_script(),
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
            let definition = magics.get(objects.get(object_id)?.magic_number())?;
            (definition.magic_type <= 3 && definition.base_damage > 0).then_some(BattleMagic {
                object_id,
                mp_cost: definition.mp_cost,
                base_damage: definition.base_damage,
                elemental: definition.elemental,
                attacks_all: matches!(definition.magic_type, 1..=3),
                sound: definition.sound,
            })
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
        attacks_all: role.attack_all,
        magics: battle_magics,
        death_sound: role.death_sound,
        attack_sound: role.attack_sound,
        weapon_sound: role.weapon_sound,
        magic_sound: role.magic_sound,
    })
}

fn next_player(players: &[BattlePlayer], acted: &[bool], start: usize) -> Option<usize> {
    (start..players.len()).find(|&index| players[index].is_alive() && !acted[index])
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
        let mut chunks = vec![Vec::new(); 15];
        let mut enemy = vec![0; ENEMY_BYTES];
        for (word, value) in [
            (11, enemy_hp),
            (12, 26),
            (13, 48),
            (14, 2),
            (21, enemy_attack),
            (23, 0),
            (32, 1),
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
                0, 0, 0, 0, 0, 0, // magic object 2 -> magic definition 0
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

    #[test]
    fn creates_enemy_instances_from_team_objects_and_positions() {
        let (data, objects, magics, role) = fixture(100, 10, 100);
        let battle =
            BattleState::new(request(true), 0, 7, [(0, &role)], &data, &objects, &magics).unwrap();
        assert_eq!(battle.players.len(), 1);
        assert_eq!(battle.enemies.len(), 2);
        assert_eq!(battle.enemies[0].position, BattlePosition { x: 11, y: 21 });
        assert_eq!(battle.enemies[1].position, BattlePosition { x: 16, y: 26 });
        assert_eq!(battle.enemies[0].turn_start_script, 11);
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
        let first = battle.attack(0).unwrap();
        assert!(matches!(
            first[0],
            BattleEvent::PlayerAttack { defeated: true, .. }
        ));
        assert_eq!(battle.phase(), BattlePhase::AwaitingCommand);
        let second = battle.attack(1).unwrap();
        assert_eq!(
            second.last(),
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
        let events = battle.attack(0).unwrap();
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
    fn physical_damage_matches_pal_piecewise_boundaries() {
        assert_eq!(physical_damage(100, 50, 2), 60);
        assert_eq!(physical_damage(80, 100, 1), 20);
        assert_eq!(physical_damage(50, 100, 1), 0);
        assert_eq!(physical_damage(100, 50, 0), 120);
    }
}
