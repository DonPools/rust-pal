//! Read-only catalog metadata for global items, magics, and enemies.

use std::collections::BTreeMap;

use encoding_rs::BIG5;
use opencc_fmmseg::OpenCC;
use pal_assets::battle::BattleData;
use pal_assets::objects::GlobalObjects;
pub(super) use pal_assets::objects::{
    FIRST_ENEMY_OBJECT, FIRST_ITEM_OBJECT, FIRST_MAGIC_OBJECT, LAST_ENEMY_OBJECT, LAST_ITEM_OBJECT,
    LAST_MAGIC_OBJECT,
};
use pal_assets::player_roles::PlayerRoles;
use pal_assets::script::ScriptTable;
use pal_assets::store::Stores;
use pal_assets::text::{ItemDescriptions, TextLibrary};
use pal_core::script::ScriptOpcode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum ContentKind {
    Item,
    Magic,
    Enemy,
}

impl ContentKind {
    pub(super) const ALL: [Self; 3] = [Self::Item, Self::Magic, Self::Enemy];

    pub(super) const fn title(self) -> &'static str {
        match self {
            Self::Item => "Items",
            Self::Magic => "Magics",
            Self::Enemy => "Enemies",
        }
    }

    pub(super) const fn singular_title(self) -> &'static str {
        match self {
            Self::Item => "ITEM",
            Self::Magic => "MAGIC",
            Self::Enemy => "ENEMY",
        }
    }

    const fn first_id(self) -> u16 {
        match self {
            Self::Item => FIRST_ITEM_OBJECT,
            Self::Magic => FIRST_MAGIC_OBJECT,
            Self::Enemy => FIRST_ENEMY_OBJECT,
        }
    }

    const fn last_id(self) -> u16 {
        match self {
            Self::Item => LAST_ITEM_OBJECT,
            Self::Magic => LAST_MAGIC_OBJECT,
            Self::Enemy => LAST_ENEMY_OBJECT,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct ContentSelection {
    pub(super) kind: ContentKind,
    pub(super) object_id: u16,
}

impl ContentSelection {
    pub(super) fn from_object_id(object_id: u16) -> Option<Self> {
        let kind = match object_id {
            FIRST_ITEM_OBJECT..=LAST_ITEM_OBJECT => ContentKind::Item,
            FIRST_MAGIC_OBJECT..=LAST_MAGIC_OBJECT => ContentKind::Magic,
            FIRST_ENEMY_OBJECT..=LAST_ENEMY_OBJECT => ContentKind::Enemy,
            _ => return None,
        };
        Some(Self { kind, object_id })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DisplayText {
    pub(super) traditional: String,
    pub(super) simplified: String,
}

impl DisplayText {
    fn from_big5(bytes: &[u8], converter: &OpenCC) -> Self {
        let (traditional, _, _) = BIG5.decode(bytes);
        let traditional = traditional.into_owned();
        let simplified = converter.tw2sp(&traditional, false);
        Self {
            traditional,
            simplified,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ContentLabel {
    pub(super) selection: ContentSelection,
    pub(super) name: DisplayText,
    search: String,
}

impl ContentLabel {
    pub(super) fn matches(&self, filter: &str) -> bool {
        let filter = filter.trim().to_lowercase();
        filter.is_empty()
            || filter
                .split_whitespace()
                .all(|part| self.search.contains(part))
    }
}

pub(super) struct ContentCatalog {
    words: Vec<DisplayText>,
    items: Vec<ContentLabel>,
    magics: Vec<ContentLabel>,
    enemies: Vec<ContentLabel>,
    descriptions: BTreeMap<u16, Vec<DisplayText>>,
}

impl ContentCatalog {
    pub(super) fn build(text: &TextLibrary, descriptions: &ItemDescriptions) -> Self {
        let mut converter = OpenCC::new();
        converter.set_parallel(false);
        let words = (0..text.word_count())
            .map(|index| DisplayText::from_big5(text.word(index).unwrap_or_default(), &converter))
            .collect::<Vec<_>>();
        let labels = |kind: ContentKind| {
            (kind.first_id()..=kind.last_id())
                .map(|object_id| {
                    let name = words
                        .get(usize::from(object_id))
                        .cloned()
                        .unwrap_or_else(|| DisplayText {
                            traditional: format!("对象 {object_id}"),
                            simplified: format!("对象 {object_id}"),
                        });
                    let search = format!(
                        "{} {} {} {:04x} 0x{:04x}",
                        name.simplified.to_lowercase(),
                        name.traditional.to_lowercase(),
                        object_id,
                        object_id,
                        object_id
                    );
                    ContentLabel {
                        selection: ContentSelection { kind, object_id },
                        name,
                        search,
                    }
                })
                .collect::<Vec<_>>()
        };
        let description_text = descriptions
            .iter()
            .map(|(object_id, lines)| {
                (
                    object_id,
                    lines
                        .iter()
                        .map(|line| DisplayText::from_big5(line, &converter))
                        .collect(),
                )
            })
            .collect();
        Self {
            items: labels(ContentKind::Item),
            magics: labels(ContentKind::Magic),
            enemies: labels(ContentKind::Enemy),
            words,
            descriptions: description_text,
        }
    }

    pub(super) fn entries(&self, kind: ContentKind) -> &[ContentLabel] {
        match kind {
            ContentKind::Item => &self.items,
            ContentKind::Magic => &self.magics,
            ContentKind::Enemy => &self.enemies,
        }
    }

    pub(super) fn label(&self, selection: ContentSelection) -> Option<&ContentLabel> {
        let index = selection.object_id.checked_sub(selection.kind.first_id())?;
        self.entries(selection.kind).get(usize::from(index))
    }

    pub(super) fn word(&self, word_id: u16) -> Option<&DisplayText> {
        self.words.get(usize::from(word_id))
    }

    pub(super) fn descriptions(&self, object_id: u16) -> &[DisplayText] {
        self.descriptions.get(&object_id).map_or(&[], Vec::as_slice)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum ContentReference {
    Script {
        entry: u16,
        opcode: u16,
    },
    BattleScript {
        entry: u16,
        team: u16,
    },
    Store {
        store: u16,
        slot: u8,
    },
    PlayerEquipment {
        role: u8,
        slot: u8,
    },
    PlayerMagic {
        role: u8,
        slot: u8,
        cooperative: bool,
    },
    LevelUpMagic {
        role: u8,
        level: u16,
    },
    EnemyTeam {
        team: u16,
        slot: u8,
    },
    EnemyMagic {
        enemy_object: u16,
    },
    EnemyAttackItem {
        enemy_object: u16,
    },
    EnemyStealItem {
        enemy_object: u16,
    },
}

#[derive(Debug, Default)]
pub(super) struct ContentReferenceCatalog {
    by_content: BTreeMap<ContentSelection, Vec<ContentReference>>,
}

impl ContentReferenceCatalog {
    pub(super) fn build(
        scripts: &ScriptTable,
        objects: &GlobalObjects,
        battle: &BattleData,
        roles: &PlayerRoles,
        stores: &Stores,
    ) -> Self {
        let mut catalog = Self::default();
        catalog.index_scripts(scripts, battle);
        for store_index in 0..stores.len() {
            let Some(store_id) = u16::try_from(store_index).ok() else {
                break;
            };
            let Some(store) = stores.get(store_id) else {
                continue;
            };
            for (slot, item_id) in store.items().enumerate() {
                catalog.insert_object(
                    item_id,
                    ContentReference::Store {
                        store: store_id,
                        slot: slot as u8,
                    },
                );
            }
        }
        for (role_index, role) in roles.iter().enumerate() {
            for (slot, item_id) in role.equipment.iter().copied().enumerate() {
                catalog.insert_object(
                    item_id,
                    ContentReference::PlayerEquipment {
                        role: role_index as u8,
                        slot: slot as u8,
                    },
                );
            }
            for (slot, magic_id) in role.magic.iter().copied().enumerate() {
                catalog.insert_object(
                    magic_id,
                    ContentReference::PlayerMagic {
                        role: role_index as u8,
                        slot: slot as u8,
                        cooperative: false,
                    },
                );
            }
            catalog.insert_object(
                role.cooperative_magic,
                ContentReference::PlayerMagic {
                    role: role_index as u8,
                    slot: 0,
                    cooperative: true,
                },
            );
        }
        for set in battle.level_up_magics.iter() {
            for (role_index, learned) in set.roles.iter().enumerate() {
                catalog.insert_object(
                    learned.magic,
                    ContentReference::LevelUpMagic {
                        role: role_index as u8,
                        level: learned.level,
                    },
                );
            }
        }
        for team_index in 0..battle.enemy_teams.len() {
            let Some(team_id) = u16::try_from(team_index).ok() else {
                break;
            };
            let Some(team) = battle.enemy_teams.get(team_id) else {
                continue;
            };
            for (slot, object_id) in team.combatants() {
                catalog.insert_object(
                    object_id,
                    ContentReference::EnemyTeam {
                        team: team_id,
                        slot: slot as u8,
                    },
                );
            }
        }
        for enemy_object in FIRST_ENEMY_OBJECT..=LAST_ENEMY_OBJECT {
            let Some(object) = objects.get(enemy_object) else {
                continue;
            };
            let Some(enemy) = battle.enemies.get(object.enemy_id()) else {
                continue;
            };
            catalog.insert_object(enemy.magic, ContentReference::EnemyMagic { enemy_object });
            catalog.insert_object(
                enemy.attack_equivalent_item,
                ContentReference::EnemyAttackItem { enemy_object },
            );
            catalog.insert_object(
                enemy.steal_item,
                ContentReference::EnemyStealItem { enemy_object },
            );
        }
        for references in catalog.by_content.values_mut() {
            references.sort_unstable();
            references.dedup();
        }
        catalog
    }

    pub(super) fn references_to(&self, selection: ContentSelection) -> &[ContentReference] {
        self.by_content.get(&selection).map_or(&[], Vec::as_slice)
    }

    fn index_scripts(&mut self, scripts: &ScriptTable, battle: &BattleData) {
        for index in 0..scripts.len() {
            let Ok(entry) = u16::try_from(index) else {
                break;
            };
            let Some(instruction) = scripts.entry(entry) else {
                continue;
            };
            let Some(opcode) = ScriptOpcode::from_raw(instruction.opcode) else {
                continue;
            };
            let expected = match opcode {
                ScriptOpcode::AddItem
                | ScriptOpcode::RemoveItem
                | ScriptOpcode::JumpIfItemCountLess
                | ScriptOpcode::JumpIfItemNotEquipped => Some(ContentKind::Item),
                ScriptOpcode::SimulatePlayerMagic
                | ScriptOpcode::ScaleMagicByMp
                | ScriptOpcode::ThrowWeapon
                | ScriptOpcode::EnemyCastMagic
                | ScriptOpcode::ScaleMagicByCash => Some(ContentKind::Magic),
                ScriptOpcode::SummonEnemy | ScriptOpcode::TransformEnemy => {
                    Some(ContentKind::Enemy)
                }
                ScriptOpcode::SetObjectScript => None,
                ScriptOpcode::StartBattle => {
                    let team_id = instruction.operands[0];
                    if let Some(team) = battle.enemy_teams.get(team_id) {
                        for (_, object_id) in team.combatants() {
                            self.insert_object(
                                object_id,
                                ContentReference::BattleScript {
                                    entry,
                                    team: team_id,
                                },
                            );
                        }
                    }
                    continue;
                }
                _ => continue,
            };
            let Some(selection) = ContentSelection::from_object_id(instruction.operands[0]) else {
                continue;
            };
            if expected.is_none_or(|kind| kind == selection.kind) {
                self.insert(
                    selection,
                    ContentReference::Script {
                        entry,
                        opcode: opcode.raw(),
                    },
                );
            }
        }
    }

    fn insert_object(&mut self, object_id: u16, reference: ContentReference) {
        if let Some(selection) = ContentSelection::from_object_id(object_id) {
            self.insert(selection, reference);
        }
    }

    fn insert(&mut self, selection: ContentSelection, reference: ContentReference) {
        self.by_content
            .entry(selection)
            .or_default()
            .push(reference);
    }
}

pub(super) fn format_content_id(object_id: u16) -> String {
    format!("#{object_id:04X}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pal_assets::objects::ObjectLayout;

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
    fn classifies_supported_classic_content_ranges() {
        assert_eq!(
            ContentSelection::from_object_id(FIRST_ITEM_OBJECT),
            Some(ContentSelection {
                kind: ContentKind::Item,
                object_id: FIRST_ITEM_OBJECT,
            })
        );
        assert_eq!(
            ContentSelection::from_object_id(LAST_MAGIC_OBJECT).map(|selection| selection.kind),
            Some(ContentKind::Magic)
        );
        assert_eq!(
            ContentSelection::from_object_id(LAST_ENEMY_OBJECT).map(|selection| selection.kind),
            Some(ContentKind::Enemy)
        );
        assert!(ContentSelection::from_object_id(FIRST_ITEM_OBJECT - 1).is_none());
        assert!(ContentSelection::from_object_id(LAST_ENEMY_OBJECT + 1).is_none());
    }

    #[test]
    fn converts_taiwan_traditional_text_to_simplified() {
        let mut converter = OpenCC::new();
        converter.set_parallel(false);
        let converted = DisplayText::from_big5(b"\xae\xf0\xc0\xf8\xb3N", &converter);
        assert_eq!(converted.traditional, "氣療術");
        assert_eq!(converted.simplified, "气疗术");
    }

    #[test]
    fn indexes_typed_script_store_and_enemy_team_references() {
        let mut object_words = vec![0u16; (usize::from(FIRST_ENEMY_OBJECT) + 1) * 6];
        object_words[usize::from(FIRST_ENEMY_OBJECT) * 6] = 0;
        let objects = GlobalObjects::parse(&words(&object_words), ObjectLayout::Dos).unwrap();

        let scripts = ScriptTable::parse(&words(&[
            ScriptOpcode::AddItem.raw(),
            FIRST_ITEM_OBJECT,
            1,
            0,
            ScriptOpcode::SimulatePlayerMagic.raw(),
            FIRST_MAGIC_OBJECT,
            10,
            0,
            ScriptOpcode::SummonEnemy.raw(),
            FIRST_ENEMY_OBJECT,
            1,
            0,
            ScriptOpcode::StartBattle.raw(),
            0,
            0,
            0,
        ]))
        .unwrap();
        let roles = PlayerRoles::parse(&vec![0; 75 * 6 * 2]).unwrap();
        let stores = Stores::parse(&words(&[FIRST_ITEM_OBJECT, 0, 0, 0, 0, 0, 0, 0, 0])).unwrap();
        let mut chunks = vec![Vec::new(); 15];
        chunks[1] = vec![0; 70];
        chunks[2] = words(&[FIRST_ENEMY_OBJECT, u16::MAX, 0, 0, 0]);
        chunks[5] = vec![0; 12];
        chunks[6] = vec![0; 20];
        chunks[13] = vec![0; 100];
        chunks[14] = vec![0; 200];
        let battle = BattleData::parse(&make_mkf(&chunks)).unwrap();

        let catalog = ContentReferenceCatalog::build(&scripts, &objects, &battle, &roles, &stores);
        let item = ContentSelection::from_object_id(FIRST_ITEM_OBJECT).unwrap();
        let magic = ContentSelection::from_object_id(FIRST_MAGIC_OBJECT).unwrap();
        let enemy = ContentSelection::from_object_id(FIRST_ENEMY_OBJECT).unwrap();

        assert!(catalog
            .references_to(item)
            .contains(&ContentReference::Script {
                entry: 0,
                opcode: ScriptOpcode::AddItem.raw(),
            }));
        assert!(catalog
            .references_to(item)
            .contains(&ContentReference::Store { store: 0, slot: 0 }));
        assert!(catalog
            .references_to(magic)
            .contains(&ContentReference::Script {
                entry: 1,
                opcode: ScriptOpcode::SimulatePlayerMagic.raw(),
            }));
        assert!(catalog
            .references_to(enemy)
            .contains(&ContentReference::Script {
                entry: 2,
                opcode: ScriptOpcode::SummonEnemy.raw(),
            }));
        assert!(catalog
            .references_to(enemy)
            .contains(&ContentReference::BattleScript { entry: 3, team: 0 }));
        assert!(catalog
            .references_to(enemy)
            .contains(&ContentReference::EnemyTeam { team: 0, slot: 0 }));
    }
}
