//! Original-save document state used by the inspector's editable save panels.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use pal_assets::objects::{
    ObjectLayout, FIRST_ITEM_OBJECT, FIRST_MAGIC_OBJECT, LAST_ITEM_OBJECT, LAST_MAGIC_OBJECT,
};
use pal_assets::player_roles::{PlayerRole, PLAYER_MAGIC_COUNT, PLAYER_ROLE_COUNT};
use pal_assets::save::{OriginalSave, SaveInventoryEntry, SAVE_INVENTORY_CAPACITY};

const SAVE_SLOTS: std::ops::RangeInclusive<u8> = 1..=5;
const MAX_ITEM_AMOUNT: u16 = 99;
const MAX_PLAYER_LEVEL: u16 = 99;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SaveSlotState {
    Empty,
    Invalid,
    Valid {
        layout: ObjectLayout,
        saved_times: u16,
        scene_number: u16,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SaveSlot {
    pub(super) number: u8,
    pub(super) path: Option<PathBuf>,
    pub(super) state: SaveSlotState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SaveChangeKind {
    Cash,
    Inventory,
    Role,
    Magic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SaveChange {
    pub(super) kind: SaveChangeKind,
    pub(super) text: String,
}

pub(super) struct SaveDocument {
    pub(super) path: PathBuf,
    original: OriginalSave,
    pub(super) save: OriginalSave,
}

impl SaveDocument {
    pub(super) fn is_dirty(&self) -> bool {
        self.save != self.original
    }

    pub(super) fn changes(&self) -> Vec<SaveChange> {
        let mut changes = Vec::new();
        if self.save.cash != self.original.cash {
            changes.push(SaveChange {
                kind: SaveChangeKind::Cash,
                text: format!("金钱：{} -> {}", self.original.cash, self.save.cash),
            });
        }

        let original_inventory = inventory_entries(&self.original);
        let current_inventory = inventory_entries(&self.save);
        let item_ids = original_inventory
            .iter()
            .chain(&current_inventory)
            .map(|(item_id, _)| *item_id)
            .collect::<BTreeSet<_>>();
        for item_id in item_ids {
            let before = inventory_amount(&original_inventory, item_id);
            let after = inventory_amount(&current_inventory, item_id);
            if before != after {
                changes.push(SaveChange {
                    kind: SaveChangeKind::Inventory,
                    text: format!("物品 #{item_id:04X}：{before} -> {after}"),
                });
            }
        }

        for role_index in 0..PLAYER_ROLE_COUNT {
            let Some(before) = self.original.player_roles.role(role_index) else {
                continue;
            };
            let Some(after) = self.save.player_roles.role(role_index) else {
                continue;
            };
            let before_experience = self.original.experience[0][role_index].experience;
            let after_experience = self.save.experience[0][role_index].experience;
            if before.level != after.level || before_experience != after_experience {
                changes.push(SaveChange {
                    kind: SaveChangeKind::Role,
                    text: format!(
                        "角色 #{role_index} 等级/经验：{}/{} -> {}/{}",
                        before.level, before_experience, after.level, after_experience
                    ),
                });
            }
            if (before.hp, before.max_hp, before.mp, before.max_mp)
                != (after.hp, after.max_hp, after.mp, after.max_mp)
            {
                changes.push(SaveChange {
                    kind: SaveChangeKind::Role,
                    text: format!(
                        "角色 #{role_index} HP/MP：{}/{}, {}/{} -> {}/{}, {}/{}",
                        before.hp,
                        before.max_hp,
                        before.mp,
                        before.max_mp,
                        after.hp,
                        after.max_hp,
                        after.mp,
                        after.max_mp
                    ),
                });
            }
            if role_statistics(before) != role_statistics(after) {
                changes.push(SaveChange {
                    kind: SaveChangeKind::Role,
                    text: format!("角色 #{role_index} 基础战斗属性已修改"),
                });
            }
            if before.magic != after.magic {
                let before_count = before.magic.iter().filter(|magic| **magic != 0).count();
                let after_count = after.magic.iter().filter(|magic| **magic != 0).count();
                changes.push(SaveChange {
                    kind: SaveChangeKind::Magic,
                    text: format!("角色 #{role_index} 仙术：{before_count} 项 -> {after_count} 项"),
                });
            }
        }
        changes
    }
}

pub(super) struct SaveEditor {
    data_dir: PathBuf,
    pub(super) slots: Vec<SaveSlot>,
    pub(super) document: Option<SaveDocument>,
    pub(super) status: Option<String>,
}

impl SaveEditor {
    pub(super) fn new(data_dir: PathBuf) -> Self {
        let mut editor = Self {
            data_dir,
            slots: Vec::new(),
            document: None,
            status: None,
        };
        editor.refresh_slots();
        let latest = editor
            .slots
            .iter()
            .filter_map(|slot| match (&slot.path, &slot.state) {
                (Some(path), SaveSlotState::Valid { saved_times, .. }) => {
                    Some((*saved_times, slot.number, path.clone()))
                }
                _ => None,
            })
            .max_by_key(|(saved_times, slot, _)| (*saved_times, *slot));
        if let Some((_, _, path)) = latest {
            let _ = editor.open_path(path);
        }
        editor
    }

    pub(super) fn refresh_slots(&mut self) {
        self.slots = SAVE_SLOTS
            .map(|number| {
                let path = slot_paths(&self.data_dir, number)
                    .into_iter()
                    .find(|path| path.exists());
                let state = match path.as_ref() {
                    None => SaveSlotState::Empty,
                    Some(path) => fs::read(path)
                        .ok()
                        .and_then(|bytes| OriginalSave::parse(&bytes))
                        .map_or(SaveSlotState::Invalid, |save| SaveSlotState::Valid {
                            layout: save.layout,
                            saved_times: save.saved_times,
                            scene_number: save.scene_number,
                        }),
                };
                SaveSlot {
                    number,
                    path,
                    state,
                }
            })
            .collect();
    }

    pub(super) fn open_path(&mut self, path: PathBuf) -> Result<(), String> {
        if !has_rpg_extension(&path) {
            return Err("只支持原版 .rpg 存档".to_owned());
        }
        let bytes =
            fs::read(&path).map_err(|error| format!("无法读取 {}：{error}", path.display()))?;
        let save = OriginalSave::parse(&bytes)
            .ok_or_else(|| format!("{} 不是有效的 DOS/Win95 .rpg 存档", path.display()))?;
        self.document = Some(SaveDocument {
            path: path.clone(),
            original: save.clone(),
            save,
        });
        self.status = Some(format!("已打开 {}", path.display()));
        Ok(())
    }

    pub(super) fn reload(&mut self) -> Result<(), String> {
        let path = self
            .document
            .as_ref()
            .map(|document| document.path.clone())
            .ok_or_else(|| "没有已打开的存档".to_owned())?;
        self.open_path(path)
    }

    pub(super) fn is_dirty(&self) -> bool {
        self.document.as_ref().is_some_and(SaveDocument::is_dirty)
    }

    pub(super) fn current_path(&self) -> Option<&Path> {
        self.document
            .as_ref()
            .map(|document| document.path.as_path())
    }

    pub(super) fn changes(&self) -> Vec<SaveChange> {
        self.document
            .as_ref()
            .map_or_else(Vec::new, SaveDocument::changes)
    }

    pub(super) fn set_cash(&mut self, cash: u32) {
        if let Some(document) = self.document.as_mut() {
            document.save.cash = cash;
        }
    }

    pub(super) fn set_inventory_amount(&mut self, index: usize, amount: u16) -> Result<(), String> {
        if amount == 0 {
            return self.remove_inventory(index);
        }
        if amount > MAX_ITEM_AMOUNT {
            return Err(format!("物品数量必须在 1..={MAX_ITEM_AMOUNT}"));
        }
        let entry = self
            .document
            .as_mut()
            .and_then(|document| document.save.inventory.get_mut(index))
            .ok_or_else(|| "背包槽位不存在".to_owned())?;
        if !(FIRST_ITEM_OBJECT..=LAST_ITEM_OBJECT).contains(&entry.item_id) {
            return Err(format!("#{:04X} 不是合法物品对象", entry.item_id));
        }
        entry.amount = amount;
        entry.amount_in_use = entry.amount_in_use.min(amount);
        Ok(())
    }

    pub(super) fn add_inventory(&mut self, item_id: u16, amount: u16) -> Result<(), String> {
        if !(FIRST_ITEM_OBJECT..=LAST_ITEM_OBJECT).contains(&item_id) {
            return Err(format!("#{item_id:04X} 不是合法物品对象"));
        }
        if !(1..=MAX_ITEM_AMOUNT).contains(&amount) {
            return Err(format!("物品数量必须在 1..={MAX_ITEM_AMOUNT}"));
        }
        let save = &mut self
            .document
            .as_mut()
            .ok_or_else(|| "没有已打开的存档".to_owned())?
            .save;
        if save
            .inventory
            .iter()
            .any(|entry| entry.item_id == item_id && entry.amount != 0)
        {
            return Err(format!("物品 #{item_id:04X} 已在背包中"));
        }
        let entry = save
            .inventory
            .iter_mut()
            .find(|entry| entry.item_id == 0 || entry.amount == 0)
            .ok_or_else(|| format!("背包已达到 {SAVE_INVENTORY_CAPACITY} 个槽位"))?;
        *entry = SaveInventoryEntry {
            item_id,
            amount,
            amount_in_use: 0,
        };
        compact_inventory(&mut save.inventory);
        Ok(())
    }

    pub(super) fn remove_inventory(&mut self, index: usize) -> Result<(), String> {
        let inventory = &mut self
            .document
            .as_mut()
            .ok_or_else(|| "没有已打开的存档".to_owned())?
            .save
            .inventory;
        if index >= inventory.len() || inventory[index].item_id == 0 {
            return Err("背包槽位不存在".to_owned());
        }
        inventory[index] = empty_inventory_entry();
        compact_inventory(inventory);
        Ok(())
    }

    pub(super) fn replace_editable_role(
        &mut self,
        role_index: usize,
        edited: &PlayerRole,
    ) -> Result<(), String> {
        if edited.level == 0 || edited.level > MAX_PLAYER_LEVEL {
            return Err(format!("角色等级必须在 1..={MAX_PLAYER_LEVEL}"));
        }
        let save = &mut self
            .document
            .as_mut()
            .ok_or_else(|| "没有已打开的存档".to_owned())?
            .save;
        let role = save
            .player_roles
            .role_mut(role_index)
            .ok_or_else(|| "角色编号不存在".to_owned())?;
        role.attack_all = edited.attack_all;
        role.level = edited.level;
        role.max_hp = edited.max_hp;
        role.max_mp = edited.max_mp;
        role.hp = edited.hp.min(edited.max_hp);
        role.mp = edited.mp.min(edited.max_mp);
        role.attack_strength = edited.attack_strength;
        role.magic_strength = edited.magic_strength;
        role.defense = edited.defense;
        role.dexterity = edited.dexterity;
        role.flee_rate = edited.flee_rate;
        role.poison_resistance = edited.poison_resistance;
        role.elemental_resistance = edited.elemental_resistance;
        save.experience[0][role_index].level = edited.level;
        Ok(())
    }

    pub(super) fn set_role_experience(
        &mut self,
        role_index: usize,
        experience: u16,
    ) -> Result<(), String> {
        let record = self
            .document
            .as_mut()
            .and_then(|document| document.save.experience[0].get_mut(role_index))
            .ok_or_else(|| "角色编号不存在".to_owned())?;
        record.experience = experience;
        Ok(())
    }

    pub(super) fn add_magic(&mut self, role_index: usize, magic_id: u16) -> Result<(), String> {
        if !(FIRST_MAGIC_OBJECT..=LAST_MAGIC_OBJECT).contains(&magic_id) {
            return Err(format!("#{magic_id:04X} 不是合法仙术对象"));
        }
        let magic = &mut self
            .document
            .as_mut()
            .and_then(|document| document.save.player_roles.role_mut(role_index))
            .ok_or_else(|| "角色编号不存在".to_owned())?
            .magic;
        if magic.contains(&magic_id) {
            return Err(format!("仙术 #{magic_id:04X} 已经习得"));
        }
        let slot = magic
            .iter_mut()
            .find(|magic| **magic == 0)
            .ok_or_else(|| format!("仙术栏已达到 {PLAYER_MAGIC_COUNT} 项"))?;
        *slot = magic_id;
        Ok(())
    }

    pub(super) fn remove_magic(
        &mut self,
        role_index: usize,
        magic_index: usize,
    ) -> Result<(), String> {
        let magic = &mut self
            .document
            .as_mut()
            .and_then(|document| document.save.player_roles.role_mut(role_index))
            .ok_or_else(|| "角色编号不存在".to_owned())?
            .magic;
        if magic_index >= magic.len() || magic[magic_index] == 0 {
            return Err("仙术槽位不存在".to_owned());
        }
        magic[magic_index] = 0;
        compact_words(magic);
        Ok(())
    }

    pub(super) fn save_to(&mut self, path: PathBuf) -> Result<Option<PathBuf>, String> {
        if !has_rpg_extension(&path) {
            return Err("存档文件必须使用 .rpg 扩展名".to_owned());
        }
        let document = self
            .document
            .as_mut()
            .ok_or_else(|| "没有已打开的存档".to_owned())?;
        let bytes = validate_and_encode(&document.save)?;
        let backup = write_verified(&path, &bytes, &document.save)?;
        document.path = path.clone();
        document.original = document.save.clone();
        self.status = Some(match &backup {
            Some(backup) => format!("已保存 {}，备份位于 {}", path.display(), backup.display()),
            None => format!("已保存 {}", path.display()),
        });
        self.refresh_slots();
        Ok(backup)
    }
}

fn inventory_entries(save: &OriginalSave) -> Vec<(u16, u16)> {
    save.inventory
        .iter()
        .filter(|entry| entry.item_id != 0 && entry.amount != 0)
        .map(|entry| (entry.item_id, entry.amount))
        .collect()
}

fn inventory_amount(inventory: &[(u16, u16)], item_id: u16) -> u16 {
    inventory
        .iter()
        .find_map(|(id, amount)| (*id == item_id).then_some(*amount))
        .unwrap_or(0)
}

fn role_statistics(role: &PlayerRole) -> (bool, u16, u16, u16, u16, u16, u16, [u16; 5]) {
    (
        role.attack_all,
        role.attack_strength,
        role.magic_strength,
        role.defense,
        role.dexterity,
        role.flee_rate,
        role.poison_resistance,
        role.elemental_resistance,
    )
}

fn empty_inventory_entry() -> SaveInventoryEntry {
    SaveInventoryEntry {
        item_id: 0,
        amount: 0,
        amount_in_use: 0,
    }
}

fn compact_inventory(inventory: &mut [SaveInventoryEntry; SAVE_INVENTORY_CAPACITY]) {
    let mut output = [empty_inventory_entry(); SAVE_INVENTORY_CAPACITY];
    for (target, entry) in output.iter_mut().zip(
        inventory
            .iter()
            .copied()
            .filter(|entry| entry.item_id != 0 && entry.amount != 0),
    ) {
        *target = entry;
    }
    *inventory = output;
}

fn compact_words<const N: usize>(words: &mut [u16; N]) {
    let mut output = [0; N];
    for (target, word) in output
        .iter_mut()
        .zip(words.iter().copied().filter(|word| *word != 0))
    {
        *target = word;
    }
    *words = output;
}

fn validate_and_encode(save: &OriginalSave) -> Result<Vec<u8>, String> {
    let mut seen_items = BTreeSet::new();
    for entry in save
        .inventory
        .iter()
        .filter(|entry| entry.item_id != 0 || entry.amount != 0 || entry.amount_in_use != 0)
    {
        if !(FIRST_ITEM_OBJECT..=LAST_ITEM_OBJECT).contains(&entry.item_id) {
            return Err(format!("背包包含非法物品 #{:04X}", entry.item_id));
        }
        if !(1..=MAX_ITEM_AMOUNT).contains(&entry.amount) {
            return Err(format!(
                "物品 #{:04X} 数量 {} 不在 1..={MAX_ITEM_AMOUNT}",
                entry.item_id, entry.amount
            ));
        }
        if entry.amount_in_use > entry.amount {
            return Err(format!(
                "物品 #{:04X} 的占用数量超过持有数量",
                entry.item_id
            ));
        }
        if !seen_items.insert(entry.item_id) {
            return Err(format!("背包重复包含物品 #{:04X}", entry.item_id));
        }
    }

    for (role_index, role) in save.player_roles.iter().enumerate() {
        if role.level > MAX_PLAYER_LEVEL {
            return Err(format!("角色 #{role_index} 等级超过 {MAX_PLAYER_LEVEL}"));
        }
        if role.hp > role.max_hp || role.mp > role.max_mp {
            return Err(format!("角色 #{role_index} 的当前 HP/MP 超过上限"));
        }
        let mut seen_magics = BTreeSet::new();
        for magic_id in role.magic.iter().copied().filter(|magic| *magic != 0) {
            if !(FIRST_MAGIC_OBJECT..=LAST_MAGIC_OBJECT).contains(&magic_id) {
                return Err(format!("角色 #{role_index} 包含非法仙术 #{magic_id:04X}"));
            }
            if !seen_magics.insert(magic_id) {
                return Err(format!("角色 #{role_index} 重复包含仙术 #{magic_id:04X}"));
            }
        }
    }

    let scene_index = usize::from(save.scene_number)
        .checked_sub(1)
        .ok_or_else(|| "当前场景编号无效".to_owned())?;
    let scene = save
        .scenes
        .get(scene_index)
        .ok_or_else(|| "当前场景记录不存在".to_owned())?;
    let next_scene = save
        .scenes
        .get(scene_index + 1)
        .ok_or_else(|| "当前场景缺少事件对象结束边界".to_owned())?;
    if scene.event_object_index > next_scene.event_object_index
        || usize::from(next_scene.event_object_index) > save.event_objects.len()
    {
        return Err("当前场景的事件对象边界无效".to_owned());
    }

    let bytes = save.encode().ok_or_else(|| "存档结构无法编码".to_owned())?;
    let reparsed =
        OriginalSave::parse(&bytes).ok_or_else(|| "编码后的存档无法重新解析".to_owned())?;
    if reparsed != *save {
        return Err("存档重新解析后与编辑状态不一致".to_owned());
    }
    Ok(bytes)
}

fn write_verified(
    path: &Path,
    bytes: &[u8],
    expected: &OriginalSave,
) -> Result<Option<PathBuf>, String> {
    let temporary = appended_path(path, ".tmp");
    let mut file = File::create(&temporary)
        .map_err(|error| format!("无法创建临时文件 {}：{error}", temporary.display()))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("无法写入临时文件 {}：{error}", temporary.display()))?;
    let temporary_bytes = fs::read(&temporary)
        .map_err(|error| format!("无法校验临时文件 {}：{error}", temporary.display()))?;
    if OriginalSave::parse(&temporary_bytes).as_ref() != Some(expected) {
        let _ = fs::remove_file(&temporary);
        return Err("临时文件校验失败，原存档未被修改".to_owned());
    }

    let backup = path.exists().then(|| next_backup_path(path));
    if let Some(backup) = &backup {
        fs::copy(path, backup)
            .map_err(|error| format!("无法备份到 {}：{error}", backup.display()))?;
    }

    if let Err(rename_error) = fs::rename(&temporary, path) {
        if !path.exists() {
            return Err(format!("无法替换 {}：{rename_error}", path.display()));
        }
        fs::remove_file(path).map_err(|error| format!("无法替换 {}：{error}", path.display()))?;
        if let Err(error) = fs::rename(&temporary, path) {
            if let Some(backup) = &backup {
                let _ = fs::copy(backup, path);
            }
            return Err(format!("无法替换 {}：{error}", path.display()));
        }
    }

    let written = fs::read(path)
        .ok()
        .and_then(|bytes| OriginalSave::parse(&bytes));
    if written.as_ref() != Some(expected) {
        if let Some(backup) = &backup {
            let _ = fs::copy(backup, path);
        } else {
            let _ = fs::remove_file(path);
        }
        return Err("写入后校验失败，已恢复原存档".to_owned());
    }
    Ok(backup)
}

fn slot_paths(data_dir: &Path, slot: u8) -> [PathBuf; 4] {
    [
        data_dir.join(format!("{slot}.RPG")),
        data_dir.join(format!("{slot}.rpg")),
        data_dir.join("SAVES").join(format!("{slot}.RPG")),
        data_dir.join("SAVES").join(format!("{slot}.rpg")),
    ]
}

fn has_rpg_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("rpg"))
}

fn appended_path(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .map(OsString::from)
        .unwrap_or_else(|| OsString::from("save.rpg"));
    name.push(suffix);
    path.with_file_name(name)
}

fn next_backup_path(path: &Path) -> PathBuf {
    let first = appended_path(path, ".bak");
    if !first.exists() {
        return first;
    }
    (1u32..)
        .map(|index| appended_path(path, &format!(".bak.{index}")))
        .find(|candidate| !candidate.exists())
        .expect("backup suffix space is inexhaustible")
}

#[cfg(test)]
mod tests {
    use pal_assets::save::DOS_SAVE_FIXED_SIZE;

    use super::*;

    fn write_u16(data: &mut [u8], offset: usize, value: u16) {
        data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn minimal_save(saved_times: u16) -> Vec<u8> {
        let mut data = vec![0; DOS_SAVE_FIXED_SIZE];
        write_u16(&mut data, 0, saved_times);
        write_u16(&mut data, 6, 0);
        write_u16(&mut data, 8, 1);
        write_u16(&mut data, 12, 0);
        write_u16(&mut data, 34, 0x1234);
        write_u16(&mut data, 36, 0x5678);
        write_u16(&mut data, 38, 0x9abc);
        data
    }

    fn test_directory(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "rust-pal-save-editor-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ))
    }

    #[test]
    fn scans_slots_and_opens_latest_valid_save() {
        let directory = test_directory("slots");
        fs::create_dir_all(directory.join("SAVES")).unwrap();
        fs::write(directory.join("1.RPG"), minimal_save(3)).unwrap();
        fs::write(directory.join("SAVES/2.rpg"), minimal_save(7)).unwrap();
        fs::write(directory.join("3.RPG"), b"invalid").unwrap();

        let editor = SaveEditor::new(directory.clone());
        assert_eq!(editor.slots.len(), 5);
        let current = editor.current_path().unwrap();
        assert_eq!(current.parent(), Some(directory.join("SAVES").as_path()));
        assert!(current
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("2.rpg")));
        assert!(matches!(editor.slots[2].state, SaveSlotState::Invalid));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn edits_supported_fields_and_preserves_read_only_role_data() {
        let directory = test_directory("edit");
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("1.RPG");
        fs::write(&path, minimal_save(1)).unwrap();
        let mut editor = SaveEditor::new(directory.clone());

        editor.set_cash(123_456);
        editor.add_inventory(FIRST_ITEM_OBJECT, 9).unwrap();
        let mut role = editor
            .document
            .as_ref()
            .unwrap()
            .save
            .player_roles
            .role(0)
            .unwrap()
            .clone();
        role.level = 42;
        role.max_hp = 999;
        role.hp = 888;
        role.avatar = 77;
        editor.replace_editable_role(0, &role).unwrap();
        editor.set_role_experience(0, 321).unwrap();
        editor.add_magic(0, FIRST_MAGIC_OBJECT).unwrap();

        let save = &editor.document.as_ref().unwrap().save;
        let saved_role = save.player_roles.role(0).unwrap();
        assert_eq!(save.cash, 123_456);
        assert_eq!(save.inventory[0].amount, 9);
        assert_eq!(
            (saved_role.level, saved_role.hp, saved_role.max_hp),
            (42, 888, 999)
        );
        assert_eq!(saved_role.avatar, 0);
        assert_eq!(save.experience[0][0].level, 42);
        assert_eq!(save.experience[0][0].experience, 321);
        assert_eq!(saved_role.magic[0], FIRST_MAGIC_OBJECT);
        assert!(editor.is_dirty());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn failed_open_or_reload_keeps_current_changes() {
        let directory = test_directory("failed-open");
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("1.RPG");
        fs::write(&path, minimal_save(1)).unwrap();
        let mut editor = SaveEditor::new(directory.clone());
        editor.set_cash(77_777);

        let invalid = directory.join("broken.rpg");
        fs::write(&invalid, b"invalid").unwrap();
        assert!(editor.open_path(invalid).is_err());
        assert_eq!(editor.document.as_ref().unwrap().save.cash, 77_777);
        assert!(editor.is_dirty());

        fs::write(&path, b"invalid").unwrap();
        assert!(editor.reload().is_err());
        assert_eq!(editor.document.as_ref().unwrap().save.cash, 77_777);
        assert!(editor.is_dirty());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn verified_write_creates_backup_and_reloads_cleanly() {
        let directory = test_directory("write");
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("1.RPG");
        let original = minimal_save(1);
        fs::write(&path, &original).unwrap();
        let mut editor = SaveEditor::new(directory.clone());
        editor.set_cash(654_321);

        let backup = editor.save_to(path.clone()).unwrap().unwrap();
        assert_eq!(fs::read(backup).unwrap(), original);
        let written = OriginalSave::parse(&fs::read(path).unwrap()).unwrap();
        assert_eq!(written.cash, 654_321);
        assert_eq!(written.reserved, [0x1234, 0x5678, 0x9abc]);
        assert!(!editor.is_dirty());
        fs::remove_dir_all(directory).unwrap();
    }
}
