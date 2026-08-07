//! Shared builders and Classic arithmetic helpers.

use pal_assets::battle::{BattleData, BattlePosition, Enemy};
use pal_assets::magic::Magics;
use pal_assets::objects::{GlobalObject, GlobalObjects};
use pal_assets::player_roles::PlayerRole;

use super::types::{
    BattleEnemy, BattleMagic, BattleMagicVisual, BattlePlayer, BattlePoison, BattleStatus,
    BattleStatuses, BattleTarget, PlayerAction, MAGIC_FLAG_APPLY_TO_ALL, MAX_BATTLE_POISONS,
};

pub(super) fn battle_enemy_from_object(
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

pub(super) fn battle_enemy(
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

pub(super) fn battle_magic(
    object_id: u16,
    objects: &GlobalObjects,
    magics: &Magics,
) -> Option<BattleMagic> {
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

pub(super) fn battle_magic_visual(
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

pub(super) fn battle_player(
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

pub(super) fn battle_player_stat_mut(
    player: &mut BattlePlayer,
    attribute: u16,
) -> Option<&mut u16> {
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

pub(super) fn player_action_target_index(action: PlayerAction) -> Option<usize> {
    match action {
        PlayerAction::Attack { target }
        | PlayerAction::Magic {
            target: BattleTarget::Enemy(target) | BattleTarget::Player(target),
            ..
        }
        | PlayerAction::CooperativeMagic {
            target: BattleTarget::Enemy(target) | BattleTarget::Player(target),
        } => Some(target),
        PlayerAction::UseItem { target, .. } | PlayerAction::ThrowItem { target, .. } => target,
        PlayerAction::Magic { .. }
        | PlayerAction::CooperativeMagic { .. }
        | PlayerAction::Flee
        | PlayerAction::Defend
        | PlayerAction::AttackMate => None,
    }
}

pub(super) fn next_player(players: &[BattlePlayer], acted: &[bool], start: usize) -> Option<usize> {
    (start..players.len()).find(|&index| {
        players[index].is_alive()
            && !players[index].statuses.is_active(BattleStatus::Sleep)
            && !players[index].statuses.is_active(BattleStatus::Paralyzed)
            && !players[index].statuses.is_active(BattleStatus::Confused)
            && !acted[index]
    })
}

pub(super) fn physical_damage(attack: u32, defense: u32, resistance: u32) -> u32 {
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

pub(super) fn classic_player_magic_damage(
    strength: u32,
    defense: u16,
    elemental_resistance: [u16; pal_assets::battle::MAGIC_ELEMENT_COUNT],
    poison_resistance: u16,
    battlefield_magic_effect: [i16; pal_assets::battle::MAGIC_ELEMENT_COUNT],
    magic: BattleMagic,
) -> i16 {
    let base = physical_damage(strength, u32::from(defense), 0) / 4;
    let mut damage = classic_short(i64::from(base) + i64::from(magic.base_damage));
    let element = usize::from(magic.elemental);
    if (1..=pal_assets::battle::MAGIC_ELEMENT_COUNT).contains(&element) {
        damage =
            classic_short(i64::from(damage) * (10 - i64::from(elemental_resistance[element - 1])));
        damage /= 5;
        damage = classic_short(
            i64::from(damage) * (10 + i64::from(battlefield_magic_effect[element - 1])),
        );
        damage /= 10;
    } else if element > pal_assets::battle::MAGIC_ELEMENT_COUNT {
        damage = classic_short(i64::from(damage) * (10 - i64::from(poison_resistance)));
        damage /= 5;
    }
    damage
}

pub(super) fn classic_short(value: i64) -> i16 {
    value as u16 as i16
}
