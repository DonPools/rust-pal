use std::path::{Path, PathBuf};

use pal_assets::save::OriginalSave;
use pal_core::game::GameState;
use pal_core::role::RoleSprites;
use pal_core::scene::SceneObject;

use super::LoadedScene;

pub(super) const ORIGINAL_SAVE_SLOTS: std::ops::RangeInclusive<u8> = 1..=5;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct OriginalSaveSlot {
    pub(super) slot: u8,
    pub(super) saved_times: u16,
    pub(super) available: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct OriginalSaveEnvironment {
    pub(super) slot: u8,
    pub(super) night_palette: bool,
    pub(super) screen_wave: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RestoreOriginalSaveError {
    Unavailable,
    Invalid,
    SceneUnavailable,
}

pub(super) fn latest_original_save_slot(directory: &Path) -> Option<u8> {
    original_save_slots(directory)
        .into_iter()
        .filter(|slot| slot.available)
        .map(|slot| (slot.saved_times, slot.slot))
        .max()
        .map(|(_, slot)| slot)
}

pub(super) fn original_save_slots(directory: &Path) -> [OriginalSaveSlot; 5] {
    std::array::from_fn(|index| {
        let slot = u8::try_from(index + 1).expect("five save slots fit in u8");
        let save = read_slot(directory, slot).and_then(|bytes| OriginalSave::parse(&bytes));
        OriginalSaveSlot {
            slot,
            saved_times: save.as_ref().map_or(0, |save| save.saved_times),
            available: save.is_some(),
        }
    })
}

pub(super) fn restore_original_save<L>(
    directory: &Path,
    slot: u8,
    game: &mut GameState,
    role_sprites: &RoleSprites,
    load_scene: &mut L,
) -> Result<OriginalSaveEnvironment, RestoreOriginalSaveError>
where
    L: FnMut(u16, Option<u16>, &RoleSprites) -> Option<LoadedScene>,
{
    if !ORIGINAL_SAVE_SLOTS.contains(&slot) {
        return Err(RestoreOriginalSaveError::Unavailable);
    }
    let bytes = read_slot(directory, slot).ok_or(RestoreOriginalSaveError::Unavailable)?;
    let save = OriginalSave::parse(&bytes).ok_or(RestoreOriginalSaveError::Invalid)?;
    let scene_index = usize::from(save.scene_number)
        .checked_sub(1)
        .ok_or(RestoreOriginalSaveError::Invalid)?;
    let map_number = save
        .scenes
        .get(scene_index)
        .ok_or(RestoreOriginalSaveError::Invalid)?
        .map_num;
    let leader_role = save.party[0].role_id;
    let leader = save
        .player_roles
        .role(usize::from(leader_role))
        .ok_or(RestoreOriginalSaveError::Invalid)?;
    if !role_sprites.has_directional_animation(
        usize::from(leader.scene_sprite_num),
        leader.frames_per_direction(),
    ) {
        return Err(RestoreOriginalSaveError::Invalid);
    }
    let all_event_objects = save
        .event_objects
        .iter()
        .enumerate()
        .map(|(index, event)| {
            let id = u16::try_from(index).ok()?.checked_add(1)?;
            let frame_count = if event.sprite_num == 0 {
                0
            } else {
                role_sprites.character_frame_count(usize::from(event.sprite_num))?
            };
            SceneObject::from_asset(id, event, frame_count)
        })
        .collect::<Option<Vec<_>>>()
        .ok_or(RestoreOriginalSaveError::Invalid)?;
    let scene = load_scene(save.scene_number, Some(map_number), role_sprites)
        .ok_or(RestoreOriginalSaveError::SceneUnavailable)?;
    let environment = OriginalSaveEnvironment {
        slot,
        night_palette: save.night_palette,
        screen_wave: save.screen_wave,
    };
    if !game.restore_original_save(save, scene.map, all_event_objects) {
        return Err(RestoreOriginalSaveError::Invalid);
    }
    Ok(environment)
}

fn read_slot(directory: &Path, slot: u8) -> Option<Vec<u8>> {
    slot_paths(directory, slot)
        .into_iter()
        .find_map(|path| std::fs::read(path).ok())
}

fn slot_paths(directory: &Path, slot: u8) -> [PathBuf; 4] {
    [
        directory.join(format!("{slot}.RPG")),
        directory.join(format!("{slot}.rpg")),
        directory.join("SAVES").join(format!("{slot}.RPG")),
        directory.join("SAVES").join(format!("{slot}.rpg")),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use pal_assets::save::DOS_SAVE_FIXED_SIZE;

    fn write_u16(data: &mut [u8], offset: usize, value: u16) {
        data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn minimal_save(saved_times: u16) -> Vec<u8> {
        let mut data = vec![0; DOS_SAVE_FIXED_SIZE];
        write_u16(&mut data, 0, saved_times);
        write_u16(&mut data, 6, 0);
        write_u16(&mut data, 8, 1);
        write_u16(&mut data, 12, 0);
        data
    }

    #[test]
    fn latest_slot_uses_saved_counter_and_accepts_both_filename_cases() {
        let directory = std::env::temp_dir().join(format!(
            "rust-pal-save-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("1.RPG"), minimal_save(3)).unwrap();
        std::fs::write(directory.join("2.rpg"), minimal_save(9)).unwrap();
        std::fs::write(directory.join("3.RPG"), b"invalid").unwrap();
        std::fs::create_dir(directory.join("SAVES")).unwrap();
        std::fs::write(directory.join("SAVES/4.rpg"), minimal_save(7)).unwrap();
        assert_eq!(latest_original_save_slot(&directory), Some(2));
        assert_eq!(
            original_save_slots(&directory),
            [
                OriginalSaveSlot {
                    slot: 1,
                    saved_times: 3,
                    available: true,
                },
                OriginalSaveSlot {
                    slot: 2,
                    saved_times: 9,
                    available: true,
                },
                OriginalSaveSlot {
                    slot: 3,
                    saved_times: 0,
                    available: false,
                },
                OriginalSaveSlot {
                    slot: 4,
                    saved_times: 7,
                    available: true,
                },
                OriginalSaveSlot {
                    slot: 5,
                    saved_times: 0,
                    available: false,
                },
            ]
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
