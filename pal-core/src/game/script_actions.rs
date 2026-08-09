//! Application of typed actions yielded by trigger scripts.

use super::*;
use crate::script::ScriptAction;

impl<M: CollisionMap> GameState<M> {
    /// Apply a platform-independent world mutation yielded by the script runtime.
    pub fn apply_script_action(&mut self, action: ScriptAction) -> bool {
        match action {
            ScriptAction::AddItem { item_id, amount } => {
                if item_id == 0 {
                    return false;
                }
                let amount = if amount == 0 { 1 } else { amount };
                let current = i32::from(self.inventory_count(item_id));
                let updated = (current + i32::from(amount)).clamp(0, 99) as u16;
                if !self.set_inventory_amount(item_id, updated) {
                    return false;
                }
            }
            ScriptAction::RemoveItem {
                item_id,
                amount,
                insufficient_entry,
            } => return self.remove_item(item_id, amount, insufficient_entry),
            ScriptAction::AdjustCash { amount, .. } => return self.adjust_cash(amount),
            ScriptAction::PlayMusic { music_id, .. } => {
                self.current_music = (music_id != 0).then_some(music_id);
            }
            ScriptAction::PlaySound { .. } => {}
            ScriptAction::SetBattleMusic { music_id } => {
                self.current_battle_music = music_id;
            }
            ScriptAction::SetBattlefield { battlefield_id } => {
                self.current_battlefield = battlefield_id;
            }
            ScriptAction::MoveObject {
                object_id,
                direction,
            } => {
                let Some(object) = self.object_mut(object_id) else {
                    return false;
                };
                object.direction = direction;
                let (dx, dy) = direction.step_at_speed(2);
                object.world_x += dx;
                object.world_y += dy;
                object.advance_animation();
            }
            ScriptAction::ChaseObject {
                object_id,
                speed,
                range,
                floating,
            } => return self.chase_object(object_id, speed, range, floating),
            ScriptAction::SetObjectPose {
                object_id,
                direction,
                frame,
            } => {
                let Some(object) = self.object_mut(object_id) else {
                    return false;
                };
                if let Some(direction) = direction {
                    object.direction = direction;
                }
                if let Some(frame) = frame {
                    object.current_frame = frame;
                }
            }
            ScriptAction::SetObjectPosition { object_id, x, y } => {
                let Some(object) = self.object_mut(object_id) else {
                    return false;
                };
                object.world_x = x;
                object.world_y = y;
            }
            ScriptAction::SetObjectPositionRelativeToPlayer { object_id, dx, dy } => {
                let position = (self.player.world_x + dx, self.player.world_y + dy);
                let Some(object) = self.object_mut(object_id) else {
                    return false;
                };
                object.world_x = position.0;
                object.world_y = position.1;
            }
            ScriptAction::PlaceObjectInFront {
                object_id, state, ..
            } => return self.place_object_in_front(object_id, state),
            ScriptAction::OffsetObject { object_id, dx, dy } => {
                let Some(object) = self.object_mut(object_id) else {
                    return false;
                };
                object.world_x += dx;
                object.world_y += dy;
                object.advance_animation();
            }
            ScriptAction::MoveObjectBy { object_id, dx, dy } => {
                let Some(object) = self.object_mut(object_id) else {
                    return false;
                };
                object.world_x += dx;
                object.world_y += dy;
            }
            ScriptAction::SetObjectLayer { object_id, layer } => {
                let Some(object) = self.object_mut(object_id) else {
                    return false;
                };
                object.layer = layer;
            }
            ScriptAction::SetObjectState { object_id, state } => {
                let Some(object) = self.object_mut(object_id) else {
                    return false;
                };
                object.state = state;
            }
            ScriptAction::SetObjectVanishTime {
                object_id,
                vanish_time,
            } => {
                let Some(object) = self.object_mut(object_id) else {
                    return false;
                };
                object.vanish_time = vanish_time;
            }
            ScriptAction::HideObjectTemporarily {
                object_id,
                vanish_time,
            } => {
                let Some(object) = self.object_mut(object_id) else {
                    return false;
                };
                object.state = object.state.saturating_neg();
                object.vanish_time = vanish_time;
            }
            ScriptAction::SyncObjectState {
                object_id,
                source_object_id,
                state,
            } => {
                let Some(source_state) = self.object_state(source_object_id) else {
                    return false;
                };
                if source_state == state {
                    let Some(object) = self.object_mut(object_id) else {
                        return false;
                    };
                    object.state = state;
                }
            }
            ScriptAction::AnimateObject { object_id } => {
                let Some(object) = self.object_mut(object_id) else {
                    return false;
                };
                object.advance_animation();
            }
            ScriptAction::SetObjectTriggerScript {
                object_id,
                script_entry,
            } => {
                let Some(object) = self.object_mut(object_id) else {
                    return false;
                };
                object.trigger_script = script_entry;
            }
            ScriptAction::SetObjectAutoScript {
                object_id,
                script_entry,
            } => {
                let Some(object) = self.object_mut(object_id) else {
                    return false;
                };
                object.auto_script = script_entry;
                object.auto_script_idle_frame = 0;
            }
            ScriptAction::SetObjectTriggerMode {
                object_id,
                trigger_mode,
            } => {
                let Some(object) = self.object_mut(object_id) else {
                    return false;
                };
                object.trigger_mode = trigger_mode;
            }
            ScriptAction::SetObjectStates {
                first_object_id,
                last_object_id,
                state,
            } => {
                let expected = usize::from(last_object_id - first_object_id) + 1;
                let current_count = self
                    .scene_objects
                    .iter()
                    .filter(|object| (first_object_id..=last_object_id).contains(&object.id))
                    .count();
                let inactive_count = self
                    .inactive_objects
                    .range(first_object_id..=last_object_id)
                    .count();
                if current_count + inactive_count != expected {
                    return false;
                }
                for object in &mut self.scene_objects {
                    if (first_object_id..=last_object_id).contains(&object.id) {
                        object.state = state;
                    }
                }
                for (_, object) in self
                    .inactive_objects
                    .range_mut(first_object_id..=last_object_id)
                {
                    object.state = state;
                }
            }
            ScriptAction::SetPlayerPose {
                direction,
                frame,
                party_index,
            } => {
                let slot_index = usize::from(party_index);
                if slot_index >= MAX_PARTY_MEMBERS {
                    return false;
                }
                self.sync_active_party_slots();
                self.player.direction = direction;
                if party_index == 0 {
                    self.player.anim_frame = frame;
                } else if let Some(follower) = self
                    .party_followers
                    .get_mut(usize::from(party_index.saturating_sub(1)))
                {
                    follower.direction = direction;
                    follower.anim_frame = frame;
                }
                let slot = &mut self.party_slots[slot_index];
                slot.direction = direction;
                slot.anim_frame = frame;
            }
            ScriptAction::SetPlayerSprite {
                role_id,
                sprite_index,
                reload,
            } => {
                let Some(roles) = self.player_roles.as_mut() else {
                    return false;
                };
                let Some(role) = roles.role_mut(usize::from(role_id)) else {
                    return false;
                };
                let Some(sprite_index) = u16::try_from(sprite_index).ok() else {
                    return false;
                };
                role.scene_sprite_num = sprite_index;
                let frames_per_direction = role.frames_per_direction();
                self.party.sync_from_roles(roles);
                if reload && self.active_battle.is_none() {
                    self.sync_active_party_slots();
                    if self
                        .party
                        .leader()
                        .is_some_and(|member| member.role_id == role_id)
                    {
                        self.player.sprite_index = usize::from(sprite_index);
                        self.player.frames_per_direction = frames_per_direction;
                        self.player.anim_frame = 0;
                    }
                    self.rebuild_party_followers();
                }
            }
            ScriptAction::CheckObjectZone {
                object_id,
                target_id,
                range,
                ..
            } => return self.objects_within_zone(object_id, target_id, range),
            ScriptAction::AdjustPlayerHealth {
                role_id,
                hp,
                mp,
                apply_to_all,
            } => return self.adjust_player_health(role_id, hp, mp, apply_to_all),
            ScriptAction::RevivePlayer {
                role_id,
                hp_tenths,
                apply_to_all,
            } => return self.revive_player(role_id, hp_tenths, apply_to_all),
            ScriptAction::DamageEnemy {
                enemy_index,
                amount,
                apply_to_all,
            } => {
                let enemy_index = if apply_to_all {
                    0
                } else if let Some(index) = self.battle_enemy_index(enemy_index) {
                    index
                } else {
                    return false;
                };
                return self
                    .active_battle
                    .as_mut()
                    .is_some_and(|battle| battle.damage_enemy(enemy_index, amount, apply_to_all));
            }
            ScriptAction::PoisonEnemy {
                enemy_index,
                poison_id,
                apply_to_all,
            } => {
                let Some(poison_object) = self
                    .global_objects
                    .as_ref()
                    .and_then(|objects| objects.get(poison_id))
                else {
                    return false;
                };
                let script_entry = self
                    .object_script_overrides
                    .get(&(poison_id, 2))
                    .copied()
                    .unwrap_or_else(|| poison_object.poison_enemy_script());
                let enemy_index = if apply_to_all {
                    0
                } else if let Some(index) = self.battle_enemy_index(enemy_index) {
                    index
                } else {
                    return false;
                };
                return self.active_battle.as_mut().is_some_and(|battle| {
                    battle.poison_enemy(enemy_index, poison_id, script_entry, apply_to_all)
                });
            }
            ScriptAction::PoisonPlayer {
                role_id,
                poison_id,
                apply_to_all,
            } => return self.poison_player(role_id, poison_id, apply_to_all),
            ScriptAction::CureEnemyPoison {
                enemy_index,
                poison_id,
                apply_to_all,
            } => {
                let enemy_index = if apply_to_all {
                    0
                } else if let Some(index) = self.battle_enemy_index(enemy_index) {
                    index
                } else {
                    return false;
                };
                return self.active_battle.as_mut().is_some_and(|battle| {
                    battle.cure_enemy_poison(enemy_index, poison_id, apply_to_all)
                });
            }
            ScriptAction::CurePlayerPoison {
                role_id,
                poison_id,
                apply_to_all,
            } => return self.cure_player_poison(role_id, poison_id, apply_to_all),
            ScriptAction::CurePlayerPoisonByLevel {
                role_id,
                maximum_level,
                apply_to_all,
            } => {
                return self.cure_player_poison_by_level(role_id, maximum_level, apply_to_all);
            }
            ScriptAction::SetPlayerStatus {
                role_id,
                status,
                rounds,
            } => return self.set_player_status(role_id, status, rounds),
            ScriptAction::SetEnemyStatus {
                enemy_index,
                status,
                rounds,
                ..
            } => {
                let Some(status) = BattleStatus::from_raw(status) else {
                    return false;
                };
                let Some(enemy_index) = self.battle_enemy_index(enemy_index) else {
                    return false;
                };
                return self
                    .active_battle
                    .as_mut()
                    .and_then(|battle| battle.set_enemy_status(enemy_index, status, rounds))
                    .unwrap_or(false);
            }
            ScriptAction::RemovePlayerStatus { role_id, status } => {
                return self.remove_player_status(role_id, status);
            }
            ScriptAction::AdjustTemporaryPlayerStat {
                role_id,
                attribute,
                percent,
            } => {
                let Some(role) = self.player_role(role_id) else {
                    return false;
                };
                let base = match attribute {
                    17 => role.attack_strength,
                    18 => role.magic_strength,
                    19 => role.defense,
                    20 => role.dexterity,
                    21 => role.flee_rate,
                    22 => role.poison_resistance,
                    _ => return false,
                };
                let value = (i32::from(base) * i32::from(percent) / 100) as u16;
                return self.active_battle.as_mut().is_some_and(|battle| {
                    battle.set_temporary_player_stat(role_id, attribute, value)
                });
            }
            ScriptAction::SetTemporaryBattleSprite { role_id, sprite } => {
                return self
                    .active_battle
                    .as_mut()
                    .is_some_and(|battle| battle.set_temporary_player_sprite(role_id, sprite));
            }
            ScriptAction::CollectEnemy { enemy_index, .. } => {
                let Some(enemy_index) = self.battle_enemy_index(enemy_index) else {
                    return false;
                };
                let Some(value) = self
                    .active_battle
                    .as_ref()
                    .and_then(|battle| battle.collect_enemy(enemy_index))
                else {
                    return false;
                };
                self.collect_value = self.collect_value.wrapping_add(value);
                return true;
            }
            ScriptAction::TransmuteCollectedEnemies => {
                return self.transmute_collected_enemies();
            }
            ScriptAction::HideBattleActor { rounds } => {
                return self
                    .active_battle
                    .as_mut()
                    .is_some_and(|battle| battle.hide_players(rounds));
            }
            ScriptAction::StealEnemy { enemy_index, rate } => {
                let Some(enemy_index) = self.battle_enemy_index(enemy_index) else {
                    return false;
                };
                let Some(stolen) = self
                    .active_battle
                    .as_mut()
                    .and_then(|battle| battle.steal_enemy(enemy_index, rate))
                else {
                    return false;
                };
                match stolen {
                    BattleSteal::Nothing => {}
                    BattleSteal::Cash(amount) => {
                        self.cash = self.cash.saturating_add(u32::from(amount));
                    }
                    BattleSteal::Item(item_id) => {
                        let amount = self.inventory_count(item_id).saturating_add(1).min(99);
                        if !self.set_inventory_amount(item_id, amount) {
                            return false;
                        }
                    }
                }
                return true;
            }
            ScriptAction::SetBattleBlow { amount } => {
                return self
                    .active_battle
                    .as_mut()
                    .is_some_and(|battle| battle.set_magic_blow(amount));
            }
            ScriptAction::PlayerMagicAnimation { player } => {
                let player = player.map(usize::from);
                return self
                    .active_battle
                    .as_mut()
                    .is_some_and(|battle| battle.queue_player_magic_animation(player));
            }
            ScriptAction::EnableAutoBattle => {
                self.auto_battle = true;
                if let Some(battle) = self.active_battle.as_mut() {
                    battle.set_auto_battle(true);
                }
            }
            ScriptAction::DrainEnemyHp {
                enemy_index,
                amount,
            } => {
                let Some(enemy_index) = self.battle_enemy_index(enemy_index) else {
                    return false;
                };
                return self
                    .active_battle
                    .as_mut()
                    .is_some_and(|battle| battle.drain_enemy_hp(enemy_index, amount));
            }
            ScriptAction::FleeBattle { .. } => {
                return self
                    .active_battle
                    .as_mut()
                    .and_then(BattleState::flee)
                    .is_some();
            }
            ScriptAction::HalvePlayerHp { role_id } => {
                return self.set_player_hp(role_id, true);
            }
            ScriptAction::HalveEnemyHp {
                enemy_index,
                maximum_damage,
            } => {
                let Some(enemy_index) = self.battle_enemy_index(enemy_index) else {
                    return false;
                };
                return self
                    .active_battle
                    .as_mut()
                    .is_some_and(|battle| battle.halve_enemy_hp(enemy_index, maximum_damage));
            }
            ScriptAction::KillPlayer { role_id } => return self.set_player_hp(role_id, false),
            ScriptAction::KillEnemy { enemy_index } => {
                let Some(enemy_index) = self.battle_enemy_index(enemy_index) else {
                    return false;
                };
                return self
                    .active_battle
                    .as_mut()
                    .is_some_and(|battle| battle.kill_enemy(enemy_index));
            }
            ScriptAction::SetEnemyMagic {
                enemy_index,
                magic_object,
                rate,
            } => {
                let Some(enemy_index) = self.battle_enemy_index(enemy_index) else {
                    return false;
                };
                let persisted_use = self.magic_use_scripts.get(&magic_object).copied();
                let persisted_success = self.magic_success_scripts.get(&magic_object).copied();
                let (Some(battle), Some(objects), Some(magics)) = (
                    self.active_battle.as_mut(),
                    self.global_objects.as_ref(),
                    self.magics.as_ref(),
                ) else {
                    return false;
                };
                if !battle.set_enemy_magic(enemy_index, magic_object, rate, objects, magics) {
                    return false;
                }
                if let Some(magic) = battle
                    .enemies
                    .get_mut(enemy_index)
                    .and_then(|enemy| enemy.magic.as_mut())
                {
                    magic.use_script = persisted_use.unwrap_or(magic.use_script);
                    magic.success_script = persisted_success.unwrap_or(magic.success_script);
                }
                return true;
            }
            ScriptAction::DivideEnemy {
                enemy_index,
                copies,
                ..
            } => {
                let Some(enemy_index) = self.battle_enemy_index(enemy_index) else {
                    return false;
                };
                let (Some(battle), Some(data)) =
                    (self.active_battle.as_mut(), self.battle_data.as_ref())
                else {
                    return false;
                };
                return battle
                    .divide_enemy(enemy_index, copies, &data.enemy_positions)
                    .is_some();
            }
            ScriptAction::SummonEnemy {
                enemy_index,
                object_id,
                count,
                ..
            } => {
                let Some(enemy_index) = self.battle_enemy_index(enemy_index) else {
                    return false;
                };
                let object_script_overrides = &self.object_script_overrides;
                let magic_use_scripts = &self.magic_use_scripts;
                let magic_success_scripts = &self.magic_success_scripts;
                let item_use_scripts = &self.item_use_scripts;
                let (Some(battle), Some(data), Some(objects), Some(magics)) = (
                    self.active_battle.as_mut(),
                    self.battle_data.as_ref(),
                    self.global_objects.as_ref(),
                    self.magics.as_ref(),
                ) else {
                    return false;
                };
                let Some(added) =
                    battle.summon_enemy(enemy_index, object_id, count, data, objects, magics)
                else {
                    return false;
                };
                for index in added {
                    let Some(enemy) = battle.enemies.get_mut(index) else {
                        return false;
                    };
                    apply_enemy_script_overrides(
                        enemy,
                        object_script_overrides,
                        magic_use_scripts,
                        magic_success_scripts,
                        item_use_scripts,
                        true,
                    );
                }
                return true;
            }
            ScriptAction::TransformEnemy {
                enemy_index,
                object_id,
            } => return self.transform_enemy(enemy_index, object_id).is_some(),
            ScriptAction::EnemyEscape => {
                return self
                    .active_battle
                    .as_mut()
                    .is_some_and(BattleState::enemy_escape);
            }
            ScriptAction::SetBattleResult { result } => {
                return self
                    .active_battle
                    .as_mut()
                    .is_some_and(|battle| battle.set_script_result(result));
            }
            ScriptAction::SimulatePlayerMagic {
                enemy_index,
                magic_object,
                base_strength,
            } => {
                let Some(enemy_index) = self.battle_simulated_magic_target(enemy_index) else {
                    return false;
                };
                let (Some(battle), Some(objects), Some(magics)) = (
                    self.active_battle.as_mut(),
                    self.global_objects.as_ref(),
                    self.magics.as_ref(),
                ) else {
                    return false;
                };
                return battle.simulate_player_magic(
                    enemy_index,
                    magic_object,
                    base_strength,
                    objects,
                    magics,
                );
            }
            ScriptAction::ThrowWeapon {
                enemy_index,
                magic_object,
                multiplier,
            } => {
                let Some(enemy_index) = self.battle_simulated_magic_target(enemy_index) else {
                    return false;
                };
                let (Some(battle), Some(objects), Some(magics)) = (
                    self.active_battle.as_mut(),
                    self.global_objects.as_ref(),
                    self.magics.as_ref(),
                ) else {
                    return false;
                };
                return battle.throw_weapon(enemy_index, magic_object, multiplier, objects, magics);
            }
            ScriptAction::ScaleMagicByMp {
                role_id,
                magic_object,
                multiplier,
            } => {
                let Some(_base_damage) = self.active_battle.as_mut().and_then(|battle| {
                    battle.scale_active_magic_by_mp(role_id, magic_object, multiplier)
                }) else {
                    return false;
                };
                let Some(roles) = self.player_roles.as_mut() else {
                    return false;
                };
                let Some(role) = roles.role_mut(usize::from(role_id)) else {
                    return false;
                };
                role.mp = 0;
                self.party.sync_from_roles(roles);
                return true;
            }
            ScriptAction::ScaleMagicByCash { magic_object } => {
                let spent = self.cash.min(5_000);
                let base_damage = u16::try_from(spent.saturating_mul(2) / 5).unwrap_or(u16::MAX);
                let Some(battle) = self.active_battle.as_mut() else {
                    return false;
                };
                if !battle.set_active_magic_base_damage(magic_object, base_damage) {
                    return false;
                }
                self.cash -= spent;
                return true;
            }
            ScriptAction::SetEnemyChase { range, cycles } => {
                self.chase_range = range;
                self.chase_speed_change_cycles = cycles;
            }
            ScriptAction::LevelUpPlayer { role_id, levels } => {
                return self.increase_player_level(role_id, levels);
            }
            ScriptAction::HalveCash => self.cash /= 2,
            ScriptAction::SetObjectScript {
                object_id,
                script_entry,
                field,
            } => {
                if field > 2
                    || self
                        .global_objects
                        .as_ref()
                        .and_then(|objects| objects.get(object_id))
                        .is_none()
                {
                    return false;
                }
                self.object_script_overrides
                    .insert((object_id, field), script_entry);
                if object_id != 0 {
                    match field {
                        0 => {
                            self.item_use_scripts.insert(object_id, script_entry);
                            self.magic_success_scripts.insert(object_id, script_entry);
                        }
                        1 => {
                            self.item_equip_scripts.insert(object_id, script_entry);
                            self.magic_use_scripts.insert(object_id, script_entry);
                        }
                        2 => {
                            self.item_throw_scripts.insert(object_id, script_entry);
                        }
                        _ => unreachable!("object script field was validated above"),
                    }
                }
            }
            ScriptAction::SetEquipmentEffect {
                role_id,
                attribute,
                slot,
                value,
            } => return self.set_equipment_effect(role_id, slot, attribute, value),
            ScriptAction::EquipItem {
                role_id,
                slot,
                item_id,
            } => return self.equip_item(role_id, slot, item_id),
            ScriptAction::ChangePlayerAttribute {
                role_id,
                attribute,
                value,
                absolute,
            } => {
                if absolute {
                    if let Some(slot) = self.current_equipment_slot {
                        return self.set_equipment_effect(role_id, slot, attribute, value);
                    }
                }
                let Some(roles) = self.player_roles.as_mut() else {
                    return false;
                };
                let Some(role) = roles.role_mut(usize::from(role_id)) else {
                    return false;
                };
                if apply_role_attribute(role, attribute, value, absolute).is_none() {
                    return false;
                }
                self.party.sync_from_roles(roles);
            }
            ScriptAction::RemoveEquipment { role_id, slot } => {
                return self.remove_equipment(role_id, slot);
            }
            ScriptAction::ChangeMagic {
                role_id,
                magic_id,
                add,
            } => return self.change_magic(role_id, magic_id, add),
            ScriptAction::OffsetPlayer { dx, dy } => {
                self.shift_party(dx, dy);
            }
            ScriptAction::SetPlayerPosition {
                tile_x,
                tile_y,
                half,
            } => {
                let Some((x, y)) =
                    tile_to_world(usize::from(tile_x), usize::from(tile_y), usize::from(half))
                else {
                    return false;
                };
                self.player.world_x = x;
                self.player.world_y = y;
                self.place_party();
                self.follow_player();
            }
            ScriptAction::ChangeScene { .. } => return false,
            ScriptAction::SetSceneScripts {
                scene_number,
                enter_script,
                teleport_script,
            } => {
                if scene_number == 0 {
                    return false;
                }
                if let Some(entry) = enter_script {
                    self.scene_enter_scripts.insert(scene_number, entry);
                }
                if let Some(entry) = teleport_script {
                    self.scene_teleport_scripts.insert(scene_number, entry);
                }
            }
            ScriptAction::SetSceneMap {
                scene_number,
                map_number,
            } => {
                let scene_number = scene_number.unwrap_or(self.scene_number);
                if scene_number == 0 || map_number == 0 {
                    return false;
                }
                self.scene_maps.insert(scene_number, map_number);
            }
            ScriptAction::WalkObjectTo { .. } => return false,
            ScriptAction::WalkPlayerTo { .. } => return false,
            ScriptAction::RideObjectTo { .. } => return false,
            ScriptAction::MoveViewport { x, y, frames } => {
                if x == 0 && y == 0 {
                    self.viewport_locked = false;
                    self.follow_player();
                } else if frames == -1 {
                    self.viewport_locked = true;
                    self.camera.x = i32::from(x) * 32 - 160;
                    self.camera.y = i32::from(y) * 16 - 112;
                } else {
                    self.viewport_locked = true;
                    self.camera.x += i32::from(x);
                    self.camera.y += i32::from(y);
                }
            }
            ScriptAction::CollapseParty => self.collapse_party(),
            ScriptAction::SetParty { members } => {
                self.sync_active_party_slots();
                let role_ids = members.into_iter().flatten().collect::<Vec<_>>();
                let Some(player_roles) = self.player_roles.as_ref() else {
                    return false;
                };
                if !self.party.replace(&role_ids, player_roles) {
                    return false;
                }
                let leader = self
                    .party
                    .leader()
                    .expect("a successfully replaced party is non-empty");
                self.player.sprite_index = usize::from(leader.attributes.scene_sprite_num);
                self.player.frames_per_direction = leader.attributes.frames_per_direction();
                self.extra_follower_ids.clear();
                self.rebuild_party_followers();
            }
            ScriptAction::SetPartyFollowers { followers } => {
                self.sync_active_party_slots();
                let follower_ids = followers.into_iter().flatten().collect::<Vec<_>>();
                let Some(player_roles) = self.player_roles.as_ref() else {
                    return false;
                };
                if follower_ids.len() > 2
                    || self.party.members().len() + follower_ids.len() > MAX_PARTY_MEMBERS
                    || follower_ids.iter().enumerate().any(|(index, &role_id)| {
                        player_roles.role(usize::from(role_id)).is_none()
                            || self
                                .party
                                .members()
                                .iter()
                                .any(|member| member.role_id == role_id)
                            || follower_ids[..index].contains(&role_id)
                    })
                {
                    return false;
                }
                self.extra_follower_ids = follower_ids;
                self.rebuild_party_followers();
                self.collapse_party();
            }
        }
        true
    }

    fn adjust_player_health(&mut self, role_id: u16, hp: i16, mp: i16, apply_to_all: bool) -> bool {
        let role_ids = if apply_to_all {
            self.party
                .members()
                .iter()
                .map(|member| member.role_id)
                .collect::<Vec<_>>()
        } else {
            vec![role_id]
        };
        if let Some(battle) = self.active_battle.as_mut() {
            let mut changed = false;
            let mut updated = Vec::new();
            for role_id in role_ids {
                let Some(player) = battle
                    .players
                    .iter_mut()
                    .find(|player| player.role_id == role_id)
                else {
                    continue;
                };
                if !player.is_alive() {
                    continue;
                }
                let new_hp = (i32::from(player.hp) + i32::from(hp))
                    .clamp(0, i32::from(player.max_hp)) as u16;
                let new_mp = (i32::from(player.mp) + i32::from(mp))
                    .clamp(0, i32::from(player.max_mp)) as u16;
                changed |= player.hp != new_hp || player.mp != new_mp;
                player.hp = new_hp;
                player.mp = new_mp;
                updated.push((role_id, new_hp, new_mp));
            }
            let Some(roles) = self.player_roles.as_mut() else {
                return false;
            };
            for (role_id, hp, mp) in updated {
                let Some(role) = roles.role_mut(usize::from(role_id)) else {
                    return false;
                };
                role.hp = hp;
                role.mp = mp;
            }
            self.party.sync_from_roles(roles);
            return changed;
        }
        let Some(roles) = self.player_roles.as_mut() else {
            return false;
        };
        let mut changed = false;
        for role_id in role_ids {
            let Some(role) = roles.role_mut(usize::from(role_id)) else {
                continue;
            };
            if role.hp == 0 {
                continue;
            }
            let new_hp = (i32::from(role.hp) + i32::from(hp)).clamp(0, i32::from(role.max_hp));
            let new_mp = (i32::from(role.mp) + i32::from(mp)).clamp(0, i32::from(role.max_mp));
            changed |= i32::from(role.hp) != new_hp || i32::from(role.mp) != new_mp;
            role.hp = new_hp as u16;
            role.mp = new_mp as u16;
        }
        self.party.sync_from_roles(roles);
        changed
    }

    fn set_player_hp(&mut self, role_id: u16, halve: bool) -> bool {
        let battle_hp = self.active_battle.as_mut().and_then(|battle| {
            let player = battle
                .players
                .iter_mut()
                .find(|player| player.role_id == role_id)?;
            player.hp = if halve { player.hp / 2 } else { 0 };
            Some(player.hp)
        });
        let Some(roles) = self.player_roles.as_mut() else {
            return false;
        };
        let Some(role) = roles.role_mut(usize::from(role_id)) else {
            return false;
        };
        role.hp = battle_hp.unwrap_or(if halve { role.hp / 2 } else { 0 });
        self.party.sync_from_roles(roles);
        true
    }

    fn revive_player(&mut self, role_id: u16, hp_tenths: u16, apply_to_all: bool) -> bool {
        let role_ids = if apply_to_all {
            self.party
                .members()
                .iter()
                .map(|member| member.role_id)
                .collect::<Vec<_>>()
        } else {
            vec![role_id]
        };
        let revived = if let Some(battle) = self.active_battle.as_mut() {
            let mut revived = Vec::new();
            for role_id in role_ids {
                let Some(player) = battle
                    .players
                    .iter_mut()
                    .find(|player| player.role_id == role_id)
                else {
                    continue;
                };
                if player.is_alive() {
                    continue;
                }
                player.hp = u32::from(player.max_hp)
                    .saturating_mul(u32::from(hp_tenths))
                    .checked_div(10)
                    .unwrap_or(0)
                    .min(u32::from(u16::MAX)) as u16;
                revived.push((role_id, player.hp));
            }
            let Some(roles) = self.player_roles.as_mut() else {
                return false;
            };
            for &(role_id, hp) in &revived {
                let Some(role) = roles.role_mut(usize::from(role_id)) else {
                    return false;
                };
                role.hp = hp;
            }
            self.party.sync_from_roles(roles);
            revived
        } else {
            let Some(roles) = self.player_roles.as_mut() else {
                return false;
            };
            let mut revived = Vec::new();
            for role_id in role_ids {
                let Some(role) = roles.role_mut(usize::from(role_id)) else {
                    continue;
                };
                if role.hp != 0 {
                    continue;
                }
                role.hp = u32::from(role.max_hp)
                    .saturating_mul(u32::from(hp_tenths))
                    .checked_div(10)
                    .unwrap_or(0)
                    .min(u32::from(u16::MAX)) as u16;
                revived.push((role_id, role.hp));
            }
            self.party.sync_from_roles(roles);
            revived
        };
        for &(role_id, hp) in &revived {
            let role_index = usize::from(role_id);
            if let Some(objects) = self.global_objects.as_ref() {
                cure_poison_by_level(&mut self.player_poisons[role_index], 3, objects);
            }
            for status in BattleStatus::ALL {
                self.player_statuses[role_index].remove_from_player(status);
            }
            if let Some(player) = self.active_battle.as_mut().and_then(|battle| {
                battle
                    .players
                    .iter_mut()
                    .find(|player| player.role_id == role_id)
            }) {
                let face_color = self.global_objects.as_ref().and_then(|objects| {
                    poison_face_color(&self.player_poisons[role_index], objects)
                });
                player.hp = hp;
                player.statuses = self.player_statuses[role_index];
                player.poisons = self.player_poisons[role_index];
                player.poison_face_color = face_color;
            }
        }
        !revived.is_empty()
    }
}
