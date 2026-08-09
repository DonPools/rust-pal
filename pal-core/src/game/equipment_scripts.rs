//! Synchronous replay of equipped-item scripts before battle.

use super::*;
use crate::script::executor::{decode_instruction, ScriptCallFrame, ScriptCursor};
use crate::script::{ScriptAction, ScriptOpcode};

impl<M: CollisionMap> GameState<M> {
    /// Re-run equipped-item scripts so battle attributes match current equipment.
    pub(super) fn refresh_equipment_effects(&mut self, scripts: &ScriptTable) -> bool {
        const MAX_EQUIPMENT_SCRIPT_INSTRUCTIONS: usize = 4096;

        let Some(roles) = self.player_roles.as_ref() else {
            return false;
        };
        let Some(objects) = self.global_objects.as_ref() else {
            return false;
        };
        let equipped = roles
            .iter()
            .enumerate()
            .flat_map(|(role_id, role)| {
                role.equipment
                    .iter()
                    .copied()
                    .enumerate()
                    .map(move |(slot, item_id)| (role_id, slot, item_id))
            })
            .filter(|&(_, _, item_id)| item_id != 0)
            .map(|(role_id, slot, item_id)| {
                let object = objects.get(item_id)?;
                let script_entry = self
                    .item_equip_scripts
                    .get(&item_id)
                    .copied()
                    .unwrap_or_else(|| object.item_equip_script());
                Some((
                    u16::try_from(role_id).ok()?,
                    u16::try_from(slot).ok()?,
                    item_id,
                    script_entry,
                ))
            })
            .collect::<Option<Vec<_>>>();
        let Some(equipped) = equipped else {
            return false;
        };

        let saved_roles = self.player_roles.clone();
        let saved_inventory = self.inventory.clone();
        let saved_effects = self.equipment_effects.clone();
        let saved_equip_scripts = self.item_equip_scripts.clone();
        let saved_current_slot = self.current_equipment_slot;
        let saved_statuses = self.player_statuses;
        self.equipment_effects.clear();
        self.current_equipment_slot = None;
        for (role_id, slot, item_id, script_entry) in equipped {
            if script_entry == 0 {
                continue;
            }
            let mut cursor = ScriptCursor::new(role_id, script_entry);
            let mut call_stack = Vec::<ScriptCallFrame>::new();
            let mut completed = false;
            for _ in 0..MAX_EQUIPMENT_SCRIPT_INSTRUCTIONS {
                let Ok(decoded) = decode_instruction(scripts, cursor.entry) else {
                    break;
                };
                let instruction = decoded.instruction;
                let opcode = decoded.opcode;
                use ScriptOpcode::*;
                let applied = match opcode {
                    Stop => {
                        if let Some(frame) = call_stack.pop() {
                            cursor.object_id = frame.object_id;
                            cursor.entry = frame.return_entry;
                            continue;
                        }
                        completed = true;
                        break;
                    }
                    StopAndAdvance => {
                        if let Some(frame) = call_stack.pop() {
                            cursor.object_id = frame.object_id;
                            cursor.entry = frame.return_entry;
                            continue;
                        }
                        cursor.next_entry = cursor.entry.wrapping_add(1);
                        completed = true;
                        break;
                    }
                    StopAndReplace => {
                        if let Some(frame) = call_stack.pop() {
                            cursor.object_id = frame.object_id;
                            cursor.entry = frame.return_entry;
                            continue;
                        }
                        cursor.next_entry = instruction.operands[0];
                        completed = true;
                        break;
                    }
                    Jump if instruction.operands[1] == 0 => {
                        cursor.entry = instruction.operands[0];
                        continue;
                    }
                    Call if instruction.operands[0] != 0 => {
                        call_stack.push(ScriptCallFrame {
                            object_id: cursor.object_id,
                            return_entry: cursor.entry.wrapping_add(1),
                            wait_frames: 0,
                            wait_updates_auto_scripts: false,
                            viewport_frames_remaining: 0,
                        });
                        cursor.entry = instruction.operands[0];
                        continue;
                    }
                    EquipItem if instruction.operands[0] >= 0x0b => {
                        self.apply_script_action(ScriptAction::EquipItem {
                            role_id,
                            slot: instruction.operands[0] - 0x0b,
                            item_id: instruction.operands[1],
                        })
                    }
                    SetEquipmentEffect if instruction.operands[0] >= 0x0b => self
                        .apply_script_action(ScriptAction::SetEquipmentEffect {
                            role_id,
                            attribute: instruction.operands[1],
                            slot: instruction.operands[0] - 0x0b,
                            value: instruction.operands[2] as i16,
                        }),
                    AdjustPlayerAttribute | SetPlayerAttribute => {
                        let target_role = instruction.operands[2].checked_sub(1).unwrap_or(role_id);
                        self.apply_script_action(ScriptAction::ChangePlayerAttribute {
                            role_id: target_role,
                            attribute: instruction.operands[0],
                            value: instruction.operands[1] as i16,
                            absolute: opcode == SetPlayerAttribute,
                        })
                    }
                    SetPlayerStatus => self.set_player_status(
                        role_id,
                        instruction.operands[0],
                        instruction.operands[1],
                    ),
                    _ => false,
                };
                if !applied {
                    break;
                }
                cursor.advance();
            }
            if !completed
                || self
                    .player_role(role_id)
                    .is_none_or(|role| role.equipment[usize::from(slot)] != item_id)
            {
                self.player_roles = saved_roles;
                self.inventory = saved_inventory;
                self.equipment_effects = saved_effects;
                self.item_equip_scripts = saved_equip_scripts;
                self.current_equipment_slot = saved_current_slot;
                self.player_statuses = saved_statuses;
                if let Some(roles) = self.player_roles.as_ref() {
                    self.party.sync_from_roles(roles);
                }
                return false;
            }
            self.item_equip_scripts.insert(item_id, cursor.next_entry);
            self.current_equipment_slot = None;
        }
        true
    }
}
