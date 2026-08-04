//! Battle definitions stored in `DATA.MKF`.

use crate::mkf::MkfArchive;
use crate::rle::RleBitmap;
use crate::sprite::{sprite_from_yj1_chunk, Sprite};

pub const MAX_ENEMIES_IN_TEAM: usize = 5;
pub const MAGIC_ELEMENT_COUNT: usize = 5;
pub const LEVEL_UP_ROLE_COUNT: usize = 5;
pub const LEVEL_UP_EXP_COUNT: usize = 100;

const ENEMY_RECORD_WORDS: usize = 35;
const ENEMY_RECORD_BYTES: usize = ENEMY_RECORD_WORDS * 2;
const ENEMY_TEAM_BYTES: usize = MAX_ENEMIES_IN_TEAM * 2;
const BATTLEFIELD_RECORD_BYTES: usize = (1 + MAGIC_ELEMENT_COUNT) * 2;
const LEVEL_UP_MAGIC_RECORD_BYTES: usize = LEVEL_UP_ROLE_COUNT * 4;
const ENEMY_POSITIONS_BYTES: usize = MAX_ENEMIES_IN_TEAM * MAX_ENEMIES_IN_TEAM * 4;
const LEVEL_UP_EXP_BYTES: usize = LEVEL_UP_EXP_COUNT * 2;

/// Static statistics and animation metadata for one enemy kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Enemy {
    pub idle_frames: u16,
    pub magic_frames: u16,
    pub attack_frames: u16,
    pub idle_animation_speed: u16,
    pub action_wait_frames: u16,
    pub y_offset: u16,
    pub attack_sound: i16,
    pub action_sound: i16,
    pub magic_sound: i16,
    pub death_sound: i16,
    pub call_sound: i16,
    pub health: u16,
    pub experience: u16,
    pub cash: u16,
    pub level: u16,
    pub magic: u16,
    pub magic_rate: u16,
    pub attack_equivalent_item: u16,
    pub attack_equivalent_item_rate: u16,
    pub steal_item: u16,
    pub steal_item_count: u16,
    pub attack_strength: u16,
    pub magic_strength: u16,
    pub defense: u16,
    pub dexterity: u16,
    pub flee_rate: u16,
    pub poison_resistance: u16,
    pub elemental_resistance: [u16; MAGIC_ELEMENT_COUNT],
    pub physical_resistance: u16,
    pub dual_move: bool,
    pub collect_value: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enemies {
    entries: Vec<Enemy>,
}

impl Enemies {
    pub fn parse(data: &[u8]) -> Option<Self> {
        let entries = parse_records(data, ENEMY_RECORD_BYTES, parse_enemy)?;
        Some(Self { entries })
    }

    pub fn get(&self, index: u16) -> Option<&Enemy> {
        self.entries.get(usize::from(index))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Five original object slots making up an enemy team.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnemyTeam {
    pub object_ids: [u16; MAX_ENEMIES_IN_TEAM],
}

impl EnemyTeam {
    /// Iterate combatants while preserving their original slot index.
    pub fn combatants(&self) -> impl Iterator<Item = (usize, u16)> + '_ {
        self.object_ids
            .iter()
            .copied()
            .enumerate()
            .filter(|&(_, object_id)| object_id != 0 && object_id != u16::MAX)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnemyTeams {
    entries: Vec<EnemyTeam>,
}

impl EnemyTeams {
    pub fn parse(data: &[u8]) -> Option<Self> {
        let entries = parse_records(data, ENEMY_TEAM_BYTES, |record| {
            Some(EnemyTeam {
                object_ids: read_u16_array(record, 0)?,
            })
        })?;
        Some(Self { entries })
    }

    pub fn get(&self, index: u16) -> Option<&EnemyTeam> {
        self.entries.get(usize::from(index))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Battlefield {
    pub screen_wave: u16,
    pub magic_effect: [i16; MAGIC_ELEMENT_COUNT],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Battlefields {
    entries: Vec<Battlefield>,
}

impl Battlefields {
    pub fn parse(data: &[u8]) -> Option<Self> {
        let entries = parse_records(data, BATTLEFIELD_RECORD_BYTES, |record| {
            Some(Battlefield {
                screen_wave: read_u16(record, 0)?,
                magic_effect: read_i16_array(record, 2)?,
            })
        })?;
        Some(Self { entries })
    }

    pub fn get(&self, index: u16) -> Option<&Battlefield> {
        self.entries.get(usize::from(index))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelUpMagic {
    pub level: u16,
    pub magic: u16,
}

/// Magic-learning thresholds for the five roles that can join the party.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelUpMagicSet {
    pub roles: [LevelUpMagic; LEVEL_UP_ROLE_COUNT],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelUpMagics {
    entries: Vec<LevelUpMagicSet>,
}

impl LevelUpMagics {
    pub fn parse(data: &[u8]) -> Option<Self> {
        let entries = parse_records(data, LEVEL_UP_MAGIC_RECORD_BYTES, |record| {
            let role_options: [Option<LevelUpMagic>; LEVEL_UP_ROLE_COUNT] =
                std::array::from_fn(|role| {
                    let offset = role * 4;
                    Some(LevelUpMagic {
                        level: read_u16(record, offset)?,
                        magic: read_u16(record, offset + 2)?,
                    })
                });
            let roles = role_options
                .into_iter()
                .collect::<Option<Vec<_>>>()?
                .try_into()
                .ok()?;
            Some(LevelUpMagicSet { roles })
        })?;
        Some(Self { entries })
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = &LevelUpMagicSet> {
        self.entries.iter()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BattlePosition {
    pub x: u16,
    pub y: u16,
}

/// Screen positions indexed first by enemy slot and then by team size minus one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnemyPositions {
    positions: [[BattlePosition; MAX_ENEMIES_IN_TEAM]; MAX_ENEMIES_IN_TEAM],
}

impl EnemyPositions {
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() != ENEMY_POSITIONS_BYTES {
            return None;
        }
        let mut positions = [[BattlePosition::default(); MAX_ENEMIES_IN_TEAM]; MAX_ENEMIES_IN_TEAM];
        for (index, record) in data.chunks_exact(4).enumerate() {
            positions[index / MAX_ENEMIES_IN_TEAM][index % MAX_ENEMIES_IN_TEAM] = BattlePosition {
                x: read_u16(record, 0)?,
                y: read_u16(record, 2)?,
            };
        }
        Some(Self { positions })
    }

    pub fn get(&self, team_size: usize, slot: usize) -> Option<BattlePosition> {
        let size_index = team_size.checked_sub(1)?;
        self.positions.get(slot)?.get(size_index).copied()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelUpExperience {
    thresholds: [u16; LEVEL_UP_EXP_COUNT],
}

impl LevelUpExperience {
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() != LEVEL_UP_EXP_BYTES {
            return None;
        }
        Some(Self {
            thresholds: read_u16_array(data, 0)?,
        })
    }

    pub fn for_level(&self, level: u16) -> Option<u16> {
        self.thresholds.get(usize::from(level)).copied()
    }
}

/// Battle-related tables loaded together from one `DATA.MKF` archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BattleData {
    pub enemies: Enemies,
    pub enemy_teams: EnemyTeams,
    pub battlefields: Battlefields,
    pub level_up_magics: LevelUpMagics,
    pub enemy_positions: EnemyPositions,
    pub level_up_experience: LevelUpExperience,
}

/// Index-preserving GOP sprite archive used by `ABC.MKF` and `F.MKF`.
pub struct BattleSpriteArchive {
    sprites: Vec<Option<Sprite>>,
}

impl BattleSpriteArchive {
    pub fn load(data: &[u8]) -> Option<Self> {
        let archive = MkfArchive::new(data)?;
        let sprites = (0..archive.chunk_count())
            .map(|index| {
                let chunk = archive.read_chunk(index)?;
                if chunk.is_empty() {
                    return Some(None);
                }
                Some(Sprite::from_gop_chunk(chunk).or_else(|| sprite_from_yj1_chunk(chunk)))
            })
            .collect::<Option<Vec<_>>>()?;
        sprites
            .iter()
            .any(Option::is_some)
            .then_some(Self { sprites })
    }

    pub fn len(&self) -> usize {
        self.sprites.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sprites.is_empty()
    }

    pub fn frame_count(&self, sprite: usize) -> Option<usize> {
        Some(self.sprites.get(sprite)?.as_ref()?.frame_count())
    }

    pub fn decode_frame(&self, sprite: usize, frame: usize) -> Option<RleBitmap> {
        self.sprites.get(sprite)?.as_ref()?.decode_frame(frame)
    }
}

impl BattleData {
    pub fn parse(data_mkf: &[u8]) -> Option<Self> {
        let archive = MkfArchive::new(data_mkf)?;
        Some(Self {
            enemies: Enemies::parse(archive.read_chunk(1)?)?,
            enemy_teams: EnemyTeams::parse(archive.read_chunk(2)?)?,
            battlefields: Battlefields::parse(archive.read_chunk(5)?)?,
            level_up_magics: LevelUpMagics::parse(archive.read_chunk(6)?)?,
            enemy_positions: EnemyPositions::parse(archive.read_chunk(13)?)?,
            level_up_experience: LevelUpExperience::parse(archive.read_chunk(14)?)?,
        })
    }
}

fn parse_enemy(record: &[u8]) -> Option<Enemy> {
    Some(Enemy {
        idle_frames: read_u16(record, 0)?,
        magic_frames: read_u16(record, 2)?,
        attack_frames: read_u16(record, 4)?,
        idle_animation_speed: read_u16(record, 6)?,
        action_wait_frames: read_u16(record, 8)?,
        y_offset: read_u16(record, 10)?,
        attack_sound: read_i16(record, 12)?,
        action_sound: read_i16(record, 14)?,
        magic_sound: read_i16(record, 16)?,
        death_sound: read_i16(record, 18)?,
        call_sound: read_i16(record, 20)?,
        health: read_u16(record, 22)?,
        experience: read_u16(record, 24)?,
        cash: read_u16(record, 26)?,
        level: read_u16(record, 28)?,
        magic: read_u16(record, 30)?,
        magic_rate: read_u16(record, 32)?,
        attack_equivalent_item: read_u16(record, 34)?,
        attack_equivalent_item_rate: read_u16(record, 36)?,
        steal_item: read_u16(record, 38)?,
        steal_item_count: read_u16(record, 40)?,
        attack_strength: read_u16(record, 42)?,
        magic_strength: read_u16(record, 44)?,
        defense: read_u16(record, 46)?,
        dexterity: read_u16(record, 48)?,
        flee_rate: read_u16(record, 50)?,
        poison_resistance: read_u16(record, 52)?,
        elemental_resistance: read_u16_array(record, 54)?,
        physical_resistance: read_u16(record, 64)?,
        dual_move: read_u16(record, 66)? != 0,
        collect_value: read_u16(record, 68)?,
    })
}

fn parse_records<T>(
    data: &[u8],
    record_bytes: usize,
    parse: impl Fn(&[u8]) -> Option<T>,
) -> Option<Vec<T>> {
    if data.is_empty() || !data.len().is_multiple_of(record_bytes) {
        return None;
    }
    data.chunks_exact(record_bytes).map(parse).collect()
}

fn read_u16(data: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        data.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn read_i16(data: &[u8], offset: usize) -> Option<i16> {
    Some(i16::from_le_bytes(
        data.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn read_u16_array<const N: usize>(data: &[u8], offset: usize) -> Option<[u16; N]> {
    let values: [Option<u16>; N] = std::array::from_fn(|index| read_u16(data, offset + index * 2));
    values
        .into_iter()
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()
}

fn read_i16_array<const N: usize>(data: &[u8], offset: usize) -> Option<[i16; N]> {
    let values: [Option<i16>; N] = std::array::from_fn(|index| read_i16(data, offset + index * 2));
    values
        .into_iter()
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn parses_enemy_statistics_and_signed_sounds() {
        let mut values = (0..ENEMY_RECORD_WORDS as u16).collect::<Vec<_>>();
        values[6] = (-7i16) as u16;
        values[11] = 360;
        values[12] = 26;
        values[13] = 48;
        values[32] = 4;
        values[33] = 1;
        let enemies = Enemies::parse(&words(&values)).unwrap();
        let enemy = enemies.get(0).unwrap();
        assert_eq!(enemy.attack_sound, -7);
        assert_eq!(enemy.health, 360);
        assert_eq!(enemy.experience, 26);
        assert_eq!(enemy.cash, 48);
        assert_eq!(enemy.elemental_resistance, [27, 28, 29, 30, 31]);
        assert_eq!(enemy.physical_resistance, 4);
        assert!(enemy.dual_move);
    }

    #[test]
    fn parses_teams_battlefields_and_learning_tables() {
        let teams = EnemyTeams::parse(&words(&[495, 0, u16::MAX, 496, 0])).unwrap();
        assert_eq!(
            teams.get(0).unwrap().combatants().collect::<Vec<_>>(),
            vec![(0, 495), (3, 496)]
        );

        let fields = Battlefields::parse(&words(&[3, 1, (-2i16) as u16, 3, 4, 5])).unwrap();
        assert_eq!(fields.get(0).unwrap().screen_wave, 3);
        assert_eq!(fields.get(0).unwrap().magic_effect, [1, -2, 3, 4, 5]);

        let learning =
            LevelUpMagics::parse(&words(&[2, 101, 3, 102, 4, 103, 5, 104, 6, 105])).unwrap();
        assert_eq!(learning.iter().next().unwrap().roles[1].level, 3);
        assert_eq!(learning.iter().next().unwrap().roles[4].magic, 105);
    }

    #[test]
    fn parses_positions_experience_and_complete_archive() {
        let positions = (0..MAX_ENEMIES_IN_TEAM * MAX_ENEMIES_IN_TEAM)
            .flat_map(|index| [index as u16, index as u16 + 100])
            .collect::<Vec<_>>();
        let positions = EnemyPositions::parse(&words(&positions)).unwrap();
        assert_eq!(positions.get(2, 1), Some(BattlePosition { x: 6, y: 106 }));
        assert!(positions.get(0, 0).is_none());

        let experience =
            LevelUpExperience::parse(&words(&(0..LEVEL_UP_EXP_COUNT as u16).collect::<Vec<_>>()))
                .unwrap();
        assert_eq!(experience.for_level(37), Some(37));
        assert_eq!(experience.for_level(100), None);

        let mut chunks = vec![Vec::new(); 15];
        chunks[1] = vec![0; ENEMY_RECORD_BYTES];
        chunks[2] = vec![0; ENEMY_TEAM_BYTES];
        chunks[5] = vec![0; BATTLEFIELD_RECORD_BYTES];
        chunks[6] = vec![0; LEVEL_UP_MAGIC_RECORD_BYTES];
        chunks[13] = vec![0; ENEMY_POSITIONS_BYTES];
        chunks[14] = vec![0; LEVEL_UP_EXP_BYTES];
        let battle = BattleData::parse(&make_mkf(&chunks)).unwrap();
        assert_eq!(battle.enemies.len(), 1);
        assert_eq!(battle.enemy_teams.len(), 1);
        assert_eq!(battle.battlefields.len(), 1);
    }

    #[test]
    fn rejects_empty_partial_and_wrong_sized_tables() {
        assert!(Enemies::parse(&[]).is_none());
        assert!(Enemies::parse(&[0; ENEMY_RECORD_BYTES - 1]).is_none());
        assert!(EnemyTeams::parse(&[0; ENEMY_TEAM_BYTES - 1]).is_none());
        assert!(Battlefields::parse(&[0; BATTLEFIELD_RECORD_BYTES - 1]).is_none());
        assert!(LevelUpMagics::parse(&[0; LEVEL_UP_MAGIC_RECORD_BYTES - 1]).is_none());
        assert!(EnemyPositions::parse(&[0; ENEMY_POSITIONS_BYTES - 1]).is_none());
        assert!(LevelUpExperience::parse(&[0; LEVEL_UP_EXP_BYTES - 1]).is_none());
        assert!(BattleData::parse(&[]).is_none());
    }

    #[test]
    fn battle_sprite_archive_preserves_slots_and_decodes_frames() {
        let frame = [2, 0, 1, 0, 2, 7, 8];
        let sprite = [2u16, 0]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .chain(frame)
            .collect::<Vec<_>>();
        let archive =
            BattleSpriteArchive::load(&make_mkf(&[Vec::new(), sprite, b"invalid".to_vec()]))
                .unwrap();
        assert_eq!(archive.len(), 3);
        assert_eq!(archive.frame_count(0), None);
        assert_eq!(archive.frame_count(1), Some(1));
        assert_eq!(archive.decode_frame(1, 0).unwrap().pixels, [7, 8]);
        assert_eq!(archive.frame_count(2), None);
    }
}
