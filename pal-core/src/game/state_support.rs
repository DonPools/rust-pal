use std::collections::BTreeMap;

use pal_assets::player_roles::PlayerRole;

pub(super) fn valid_role_attribute(attribute: u16) -> bool {
    matches!(attribute, 0..=4 | 6..=27 | 31..=74)
}

pub(super) fn unique_script_entries(entries: Vec<(u16, u16)>) -> Option<BTreeMap<u16, u16>> {
    let count = entries.len();
    let entries = entries.into_iter().collect::<BTreeMap<_, _>>();
    (entries.len() == count && !entries.contains_key(&0)).then_some(entries)
}

pub(super) fn apply_role_attribute(
    role: &mut PlayerRole,
    attribute: u16,
    value: i16,
    absolute: bool,
) -> Option<()> {
    fn update(target: &mut u16, value: i16, absolute: bool) {
        *target = if absolute {
            value as u16
        } else {
            target.wrapping_add_signed(value)
        };
    }

    match attribute {
        0 => update(&mut role.avatar, value, absolute),
        1 => {
            if value != 0 {
                role.battle_sprite_num = value as u16;
            }
        }
        2 => update(&mut role.scene_sprite_num, value, absolute),
        3 => update(&mut role.name_word_id, value, absolute),
        4 => {
            role.attack_all = if absolute {
                value != 0
            } else {
                role.attack_all || value != 0
            }
        }
        6 => update(&mut role.level, value, absolute),
        7 => update(&mut role.max_hp, value, absolute),
        8 => update(&mut role.max_mp, value, absolute),
        9 => update(&mut role.hp, value, absolute),
        10 => update(&mut role.mp, value, absolute),
        11..=16 => update(
            role.equipment.get_mut(usize::from(attribute - 11))?,
            value,
            absolute,
        ),
        17 => update(&mut role.attack_strength, value, absolute),
        18 => update(&mut role.magic_strength, value, absolute),
        19 => update(&mut role.defense, value, absolute),
        20 => update(&mut role.dexterity, value, absolute),
        21 => update(&mut role.flee_rate, value, absolute),
        22 => update(&mut role.poison_resistance, value, absolute),
        23..=27 => update(
            role.elemental_resistance
                .get_mut(usize::from(attribute - 23))?,
            value,
            absolute,
        ),
        31 => update(&mut role.covered_by, value, absolute),
        32..=63 => update(
            role.magic.get_mut(usize::from(attribute - 32))?,
            value,
            absolute,
        ),
        64 => update(&mut role.walk_frames, value, absolute),
        65 => update(&mut role.cooperative_magic, value, absolute),
        66 => update(&mut role.unknown_5, value, absolute),
        67 => update(&mut role.unknown_6, value, absolute),
        68 => update(&mut role.death_sound, value, absolute),
        69 => update(&mut role.attack_sound, value, absolute),
        70 => update(&mut role.weapon_sound, value, absolute),
        71 => update(&mut role.critical_sound, value, absolute),
        72 => update(&mut role.magic_sound, value, absolute),
        73 => update(&mut role.cover_sound, value, absolute),
        74 => update(&mut role.dying_sound, value, absolute),
        _ => return None,
    }
    Some(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoScriptError {
    InvalidEntry {
        object_id: u16,
        entry: u16,
    },
    MissingObject {
        object_id: u16,
        entry: u16,
        target_id: u16,
    },
    Unsupported {
        object_id: u16,
        entry: u16,
        opcode: u16,
    },
    /// The instruction is implemented, but the synchronous headless helper
    /// cannot choose the platform-owned result needed to continue it.
    HostRequired {
        object_id: u16,
        entry: u16,
        opcode: u16,
    },
    InstructionLimit {
        object_id: u16,
        entry: u16,
    },
}

/// Result of advancing every active scene object's automatic script once.
///
/// An error from one object does not roll back mutations already made by other
/// objects, so callers that render the scene must preserve `changed` even when
/// `error` is present.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoScriptUpdate {
    pub changed: bool,
    pub error: Option<AutoScriptError>,
}

impl AutoScriptUpdate {
    pub fn into_result(self) -> Result<bool, AutoScriptError> {
        self.error.map_or(Ok(self.changed), Err)
    }
}
