use pal_assets::fbp::FbpArchive;
use pal_assets::mkf::MkfArchive;
use pal_assets::rng::{apply_frame_delta_checked, RngArchive, RNG_FRAME_PIXELS};
use pal_core::battle::{BattlePhase, BattleResult, BattleStatus};
use pal_core::script::{ScriptAction, ScriptCondition, ScriptEvent, ScriptOpcode, ScriptRuntime};
use pal_desktop::audio::{validate_midi_output, validate_sound_font};
use pal_desktop::window::{
    render_battle, render_tile_map, BattleRenderResources, BattleRenderState, Viewport,
};

use crate::assets::{load_runtime_scene, validate_music, validate_sound_effects};
use crate::bootstrap::BootstrappedGame;
use crate::{SCREEN_HEIGHT, SCREEN_WIDTH};

pub(super) fn check_assets(boot: BootstrappedGame) {
    let BootstrappedGame {
        data_dir,
        scene_data,
        initial_scene_number,
        initial_enter_script,
        initial_event_sprite_numbers,
        text,
        font,
        script_table,
        voc_mkf,
        midi_mkf,
        sound_font,
        player_roles,
        global_objects,
        battle_data,
        enemy_battle_sprites,
        player_battle_sprites,
        battle_backgrounds,
        role_sprites,
        dialog_faces,
        dialog_icons,
        mut game,
        mut renderer,
        ..
    } = boot;
    let viewport = Viewport::from(game.camera);
    let sound_effect_count = validate_sound_effects(&voc_mkf)
        .expect("VOC.MKF contains an invalid or unsupported sound effect");
    let music_count = validate_music(&midi_mkf).expect("MIDI.MKF contains invalid MIDI music");
    let rng_data = std::fs::read(data_dir.join("RNG.MKF")).expect("failed to read RNG.MKF");
    let rng_archive = RngArchive::new(&rng_data).expect("invalid RNG.MKF archive");
    let mut rng_frame_count = 0usize;
    let mut rng_non_empty_frames = 0usize;
    for animation_index in 0..rng_archive.animation_count() {
        let animation = rng_archive
            .animation(animation_index)
            .unwrap_or_else(|| panic!("invalid RNG.MKF animation {animation_index}"));
        let mut canvas = vec![0; RNG_FRAME_PIXELS];
        for frame_index in 0..animation.frame_count() {
            let compressed = animation
                .compressed_frame(frame_index)
                .expect("RNG frame index disappeared");
            if compressed.is_empty() {
                continue;
            }
            let commands = animation.decompress_frame(frame_index).unwrap_or_else(|| {
                panic!(
                    "invalid YJ_1 stream in RNG.MKF animation {animation_index} frame \
                     {frame_index}: {} bytes, prefix {:02x?}",
                    compressed.len(),
                    &compressed[..compressed.len().min(8)]
                )
            });
            apply_frame_delta_checked(&commands, &mut canvas).unwrap_or_else(|error| {
                panic!("invalid RNG.MKF animation {animation_index} frame {frame_index}: {error:?}")
            });
            rng_frame_count += 1;
            rng_non_empty_frames += usize::from(canvas.iter().any(|&pixel| pixel != 0));
        }
    }
    assert!(rng_frame_count > 0, "RNG.MKF contains no animation frames");
    assert!(
        rng_non_empty_frames > 0,
        "RNG.MKF animations never produce a non-empty framebuffer"
    );
    let fbp_data = std::fs::read(data_dir.join("FBP.MKF")).expect("failed to read FBP.MKF");
    let fbp_archive = FbpArchive::new(&fbp_data).expect("invalid FBP.MKF archive");
    let mut fbp_frame_count = 0usize;
    for frame_index in 0..fbp_archive.len() {
        if fbp_archive
            .raw_frame(frame_index)
            .expect("FBP frame index disappeared")
            .is_empty()
        {
            continue;
        }
        fbp_archive
            .frame(frame_index)
            .unwrap_or_else(|| panic!("invalid FBP.MKF frame {frame_index}"));
        fbp_frame_count += 1;
    }
    assert!(fbp_frame_count > 0, "FBP.MKF contains no pictures");
    let mut fbp_script_references = 0usize;
    let mut fbp_black_fallbacks = 0usize;
    let mut enemy_turn_jumps = 0usize;
    let mut player_confusion_scripts = 0usize;
    let mut player_haste_scripts = 0usize;
    let mut temporary_stat_scripts = 0usize;
    let mut temporary_sprite_scripts = 0usize;
    let mut simulated_magic_scripts = 0usize;
    let mut thrown_weapon_scripts = 0usize;
    let mut mp_scaled_magic_scripts = 0usize;
    let mut cash_scaled_magic_scripts = 0usize;
    let mut divide_enemy_scripts = 0usize;
    let mut summon_enemy_scripts = 0usize;
    let mut transform_enemy_scripts = 0usize;
    let mut pause_chase_scripts = 0usize;
    let mut speed_chase_scripts = 0usize;
    let mut level_up_scripts = 0usize;
    let mut halve_cash_scripts = 0usize;
    let mut set_object_script_scripts = 0usize;
    let mut player_sprite_scripts = 0usize;
    let mut outside_zone_scripts = 0usize;
    let mut cd_music_scripts = 0usize;
    let mut collect_enemy_scripts = 0usize;
    let mut transmute_scripts = 0usize;
    let mut hide_battle_scripts = 0usize;
    let mut steal_enemy_scripts = 0usize;
    let mut auto_battle_scripts = 0usize;
    let mut battle_blow_amounts = Vec::new();
    let mut player_magic_animation_players = Vec::new();
    let mut ending_sprite_references = std::collections::BTreeSet::new();
    for index in 0..script_table.len() {
        let entry_index = u16::try_from(index).expect("script table exceeds addressable range");
        let entry = script_table
            .entry(entry_index)
            .expect("script entry index disappeared");
        enemy_turn_jumps += usize::from(entry.opcode == ScriptOpcode::JumpIfEnemyTurn.raw());
        if entry.opcode == ScriptOpcode::SetPlayerStatus.raw() {
            player_confusion_scripts +=
                usize::from(entry.operands[0] == BattleStatus::Confused as u16);
            player_haste_scripts += usize::from(entry.operands[0] == BattleStatus::Haste as u16);
        }
        if entry.opcode == ScriptOpcode::AdjustTemporaryPlayerStat.raw() {
            assert!(
                (17..=22).contains(&entry.operands[0]),
                "temporary-stat script {index} references unsupported attribute {}",
                entry.operands[0]
            );
            assert!(
                usize::from(entry.operands[2]) <= pal_assets::player_roles::PLAYER_ROLE_COUNT,
                "temporary-stat script {index} references unavailable player {}",
                entry.operands[2]
            );
            temporary_stat_scripts += 1;
        }
        if entry.opcode == ScriptOpcode::SetTemporaryBattleSprite.raw() {
            assert!(
                player_battle_sprites
                    .frame_count(usize::from(entry.operands[0]))
                    .is_some(),
                "temporary-sprite script {index} references unavailable F.MKF slot {}",
                entry.operands[0]
            );
            temporary_sprite_scripts += 1;
        }
        if matches!(
            ScriptOpcode::from_raw(entry.opcode),
            Some(
                ScriptOpcode::SimulatePlayerMagic
                    | ScriptOpcode::ThrowWeapon
                    | ScriptOpcode::ScaleMagicByMp
                    | ScriptOpcode::ScaleMagicByCash
            )
        ) {
            assert!(
                game.has_magic_definition(entry.operands[0]),
                "script {index} references unavailable magic object {}",
                entry.operands[0]
            );
        }
        simulated_magic_scripts +=
            usize::from(entry.opcode == ScriptOpcode::SimulatePlayerMagic.raw());
        thrown_weapon_scripts += usize::from(entry.opcode == ScriptOpcode::ThrowWeapon.raw());
        mp_scaled_magic_scripts += usize::from(entry.opcode == ScriptOpcode::ScaleMagicByMp.raw());
        cash_scaled_magic_scripts +=
            usize::from(entry.opcode == ScriptOpcode::ScaleMagicByCash.raw());
        divide_enemy_scripts += usize::from(entry.opcode == ScriptOpcode::DivideEnemy.raw());
        summon_enemy_scripts += usize::from(entry.opcode == ScriptOpcode::SummonEnemy.raw());
        transform_enemy_scripts += usize::from(entry.opcode == ScriptOpcode::TransformEnemy.raw());
        pause_chase_scripts += usize::from(entry.opcode == ScriptOpcode::PauseEnemyChase.raw());
        speed_chase_scripts += usize::from(entry.opcode == ScriptOpcode::SpeedUpEnemyChase.raw());
        level_up_scripts += usize::from(entry.opcode == ScriptOpcode::LevelUpPlayer.raw());
        halve_cash_scripts += usize::from(entry.opcode == ScriptOpcode::HalveCash.raw());
        if entry.opcode == ScriptOpcode::SetObjectScript.raw() {
            assert!(
                global_objects.get(entry.operands[0]).is_some(),
                "object-script mutation {index} references unavailable object {}",
                entry.operands[0]
            );
            assert!(
                entry.operands[2] <= 2,
                "object-script mutation {index} references unavailable field {}",
                entry.operands[2]
            );
            set_object_script_scripts += 1;
        }
        if entry.opcode == ScriptOpcode::SetPlayerSprite.raw() {
            assert!(
                usize::from(entry.operands[0]) < pal_assets::player_roles::PLAYER_ROLE_COUNT,
                "player-sprite script {index} references unavailable role {}",
                entry.operands[0]
            );
            assert!(
                role_sprites
                    .character_frame_count(usize::from(entry.operands[1]))
                    .is_some(),
                "player-sprite script {index} references unavailable MGO sprite {}",
                entry.operands[1]
            );
            player_sprite_scripts += 1;
        }
        outside_zone_scripts +=
            usize::from(entry.opcode == ScriptOpcode::JumpIfObjectOutsideZone.raw());
        if entry.opcode == ScriptOpcode::PlayCdMusic.raw() {
            assert!(
                usize::from(entry.operands[1]) < music_count,
                "CD fallback script {index} references unavailable MIDI slot {}",
                entry.operands[1]
            );
            cd_music_scripts += 1;
        }
        collect_enemy_scripts += usize::from(entry.opcode == ScriptOpcode::CollectEnemy.raw());
        transmute_scripts +=
            usize::from(entry.opcode == ScriptOpcode::TransmuteCollectedEnemies.raw());
        hide_battle_scripts += usize::from(entry.opcode == ScriptOpcode::HideBattleActor.raw());
        steal_enemy_scripts += usize::from(entry.opcode == ScriptOpcode::StealEnemy.raw());
        auto_battle_scripts += usize::from(entry.opcode == ScriptOpcode::EnableAutoBattle.raw());
        if entry.opcode == ScriptOpcode::BlowEnemiesAway.raw() {
            battle_blow_amounts.push(entry.operands[0] as i16);
        }
        if entry.opcode == ScriptOpcode::PlayerMagicAnimation.raw() {
            player_magic_animation_players.push(entry.operands[0]);
        }
        if entry.opcode == ScriptOpcode::TransformEnemy.raw()
            || (entry.opcode == ScriptOpcode::SummonEnemy.raw()
                && !matches!(entry.operands[0], 0 | u16::MAX))
        {
            let object = global_objects.get(entry.operands[0]).unwrap_or_else(|| {
                panic!(
                    "dynamic enemy script {index} references unavailable object {}",
                    entry.operands[0]
                )
            });
            let enemy_id = object.enemy_id();
            battle_data.enemies.get(enemy_id).unwrap_or_else(|| {
                panic!(
                    "dynamic enemy script {index} references unavailable enemy definition \
                     {enemy_id} through object {}",
                    entry.operands[0]
                )
            });
            assert!(
                enemy_battle_sprites
                    .frame_count(usize::from(enemy_id))
                    .is_some(),
                "dynamic enemy script {index} references unavailable ABC sprite {enemy_id}"
            );
        }
        if let Some(
            ScriptOpcode::ShowFbp | ScriptOpcode::ScrollFbp | ScriptOpcode::ShowFbpWithSprite,
        ) = ScriptOpcode::from_raw(entry.opcode)
        {
            if fbp_archive.frame(usize::from(entry.operands[0])).is_none() {
                if entry.operands[0] == u16::MAX && entry.opcode != ScriptOpcode::ScrollFbp.raw() {
                    fbp_black_fallbacks += 1;
                } else {
                    panic!(
                        "script {index} ({:?}, operands {:04x?}) references unavailable FBP \
                         picture {}",
                        ScriptOpcode::from_raw(entry.opcode),
                        entry.operands,
                        entry.operands[0],
                    );
                }
            }
            fbp_script_references += 1;
            if entry.opcode == ScriptOpcode::ShowFbpWithSprite.raw()
                && !matches!(entry.operands[1], 0 | 0xffff)
            {
                ending_sprite_references.insert(entry.operands[1]);
            }
        }
    }
    for &sprite in &ending_sprite_references {
        let sprite_index = usize::from(sprite);
        let frame_count = role_sprites
            .character_frame_count(sprite_index)
            .unwrap_or_else(|| panic!("script references unavailable MGO sprite {sprite}"));
        assert!(
            (0..frame_count).all(|frame| role_sprites.decode_frame(sprite_index, frame).is_some()),
            "ending effect MGO sprite {sprite} contains an invalid frame"
        );
    }
    assert!(
        fbp_script_references > 0,
        "scripts contain no FBP picture references"
    );
    assert!(
        fbp_black_fallbacks > 0,
        "scripts do not exercise the FBP black-screen fallback"
    );
    assert!(
        enemy_turn_jumps > 0,
        "scripts do not exercise the enemy-turn condition"
    );
    assert!(
        player_confusion_scripts > 0 && player_haste_scripts > 0,
        "scripts do not exercise player confusion and haste statuses"
    );
    assert!(
        simulated_magic_scripts > 0 && mp_scaled_magic_scripts > 0 && cash_scaled_magic_scripts > 0,
        "scripts do not exercise simulated and dynamically scaled magic"
    );
    assert_eq!(
        thrown_weapon_scripts, 32,
        "real scripts no longer match the expected weapon-throw coverage"
    );
    assert_eq!(
        (temporary_stat_scripts, temporary_sprite_scripts),
        (14, 1),
        "real scripts no longer match temporary player-effect coverage"
    );
    assert_eq!(
        (
            divide_enemy_scripts,
            summon_enemy_scripts,
            transform_enemy_scripts,
        ),
        (2, 32, 4),
        "real scripts no longer match dynamic enemy behavior coverage"
    );
    assert_eq!(
        (
            pause_chase_scripts,
            speed_chase_scripts,
            level_up_scripts,
            halve_cash_scripts,
            set_object_script_scripts,
        ),
        (1, 1, 1, 1, 3),
        "real scripts no longer match chase, level-up, cash and object-script coverage"
    );
    assert!(
        player_sprite_scripts > 0,
        "real scripts contain no player-sprite changes"
    );
    assert_eq!(
        (outside_zone_scripts, cd_music_scripts),
        (2, 6),
        "real scripts no longer match object-zone and CD fallback coverage"
    );
    assert_eq!(
        (
            collect_enemy_scripts,
            transmute_scripts,
            hide_battle_scripts,
            steal_enemy_scripts,
            auto_battle_scripts,
        ),
        (1, 1, 1, 1, 1),
        "real scripts no longer match collection, hiding, stealing and auto-battle coverage"
    );
    battle_blow_amounts.sort_unstable();
    assert_eq!(
        battle_blow_amounts,
        [-3, -2],
        "real scripts no longer match signed magic-blow coverage"
    );
    assert_eq!(
        player_magic_animation_players,
        [2],
        "real scripts no longer match player magic-animation coverage"
    );
    let (usable_item_definitions, throwable_item_definitions) = (0..global_objects.len())
        .filter_map(|index| global_objects.get(u16::try_from(index).ok()?))
        .fold((0usize, 0usize), |(usable, throwable), object| {
            let flags = object.item_flags();
            (
                usable + usize::from(flags & (1 << 0) != 0),
                throwable + usize::from(flags & (1 << 2) != 0),
            )
        });
    assert!(
        usable_item_definitions > 0 && throwable_item_definitions > 0,
        "object data contains no usable or throwable item definitions"
    );
    assert!(validate_sound_font(&sound_font), "invalid SoundFont");
    let midi_archive = MkfArchive::new(&midi_mkf).expect("invalid MIDI.MKF archive");
    assert!(
        validate_midi_output(
            midi_archive.read_chunk(31).expect("missing opening music"),
            &sound_font,
        ),
        "SoundFont MIDI synthesis produced no audio"
    );
    let leader = game.party.leader().expect("loaded party has no leader");
    assert!(leader.attributes.hp <= leader.attributes.max_hp);
    assert!(leader.attributes.mp <= leader.attributes.max_mp);
    assert!(
        text.word(usize::from(leader.attributes.name_word_id))
            .is_some_and(|name| !name.is_empty()),
        "party leader references an unavailable name"
    );
    assert!(
        global_objects.get(99).is_some(),
        "item 99 has no object definition"
    );
    assert!(
        initial_event_sprite_numbers
            .iter()
            .all(|&sprite_num| sprite_num == 0
                || role_sprites
                    .character_frame_count(sprite_num as usize)
                    .is_some()),
        "scene event object references an unavailable MGO.MKF sprite"
    );
    assert!(
        dialog_faces.iter().any(Option::is_some),
        "RGM.MKF contains no valid dialog faces"
    );
    assert!(
        dialog_icons.len() >= 3
            && dialog_icons
                .iter()
                .take(3)
                .all(|icon| icon.width > 0 && icon.height > 0),
        "DATA.MKF contains invalid dialog wait icons"
    );
    let (speed_controls, terminal_controls, icon_controls) = text_control_counts(&text);
    assert!(
        speed_controls > 0 && terminal_controls > 0 && icon_controls > 0,
        "M.MSG does not exercise all supported dialog timing and icon controls"
    );
    render_tile_map(
        &mut renderer,
        &game.map,
        Some(&role_sprites),
        &[],
        &[],
        viewport,
    );
    let map_only = renderer.screen().to_vec();
    render_tile_map(
        &mut renderer,
        &game.map,
        Some(&role_sprites),
        std::slice::from_ref(&game.player),
        &game.scene_objects,
        viewport,
    );
    let sprite_pixels = renderer
        .screen()
        .chunks_exact(4)
        .zip(map_only.chunks_exact(4))
        .filter(|(with_sprite, map_pixel)| with_sprite != map_pixel)
        .count();
    let visible_pixels = renderer
        .screen()
        .chunks_exact(4)
        .filter(|pixel| pixel[..3] != [0, 0, 0])
        .count();
    let chromatic_pixels = renderer
        .screen()
        .chunks_exact(4)
        .filter(|pixel| {
            let [r, g, b, _] = pixel else {
                return false;
            };
            r.abs_diff(*g).max(g.abs_diff(*b)).max(b.abs_diff(*r)) >= 8
        })
        .count();

    let visible_object = game
        .scene_objects
        .iter()
        .find(|object| object.is_visible())
        .expect("current scene has no visible event object");
    let visible_object_id = visible_object.id;
    let object_viewport = Viewport::new(
        (visible_object.world_x - SCREEN_WIDTH as i32 / 2).max(0),
        (visible_object.world_y - SCREEN_HEIGHT as i32 / 2).max(0),
        SCREEN_WIDTH,
        SCREEN_HEIGHT,
    );
    render_tile_map(
        &mut renderer,
        &game.map,
        Some(&role_sprites),
        &[],
        &[],
        object_viewport,
    );
    let object_map_only = renderer.screen().to_vec();
    render_tile_map(
        &mut renderer,
        &game.map,
        Some(&role_sprites),
        &[],
        std::slice::from_ref(visible_object),
        object_viewport,
    );
    let event_object_pixels = renderer
        .screen()
        .chunks_exact(4)
        .zip(object_map_only.chunks_exact(4))
        .filter(|(with_object, map_pixel)| with_object != map_pixel)
        .count();
    let text_before = renderer.screen().to_vec();
    let sample_word = (0..text.word_count())
        .filter_map(|index| text.word(index))
        .find(|word| !word.is_empty())
        .expect("WORD.DAT has no displayable sample");
    renderer.draw_big5_text(&font, sample_word, 8, 8, 0x4f);
    let text_pixels = renderer
        .screen()
        .chunks_exact(4)
        .zip(text_before.chunks_exact(4))
        .filter(|(with_text, before)| with_text != before)
        .count();
    let script_object = game
        .scene_objects
        .iter()
        .find(|object| {
            script_table
                .entry(object.trigger_script)
                .is_some_and(|entry| entry.opcode == 0xffff)
        })
        .expect("scene has no directly displayable message script");
    let trigger = pal_core::scene::TriggerRequest {
        object_id: script_object.id,
        script_entry: script_object.trigger_script,
        kind: pal_core::scene::TriggerKind::Search,
    };
    let script_count = script_table.len();
    let mut enemy_attack_items = 0usize;
    for index in 0..battle_data.enemies.len() {
        let enemy_index = u16::try_from(index).expect("enemy table exceeds addressable range");
        let enemy = battle_data
            .enemies
            .get(enemy_index)
            .expect("enemy definition index disappeared");
        if enemy.attack_equivalent_item == 0 || enemy.attack_equivalent_item_rate == 0 {
            continue;
        }
        let item = global_objects
            .get(enemy.attack_equivalent_item)
            .unwrap_or_else(|| {
                panic!(
                    "enemy {index} references unavailable attack-equivalent item {}",
                    enemy.attack_equivalent_item
                )
            });
        let use_script = item.item_use_script();
        assert_ne!(
            use_script, 0,
            "enemy {index} attack-equivalent item {} has no use script",
            enemy.attack_equivalent_item
        );
        assert!(
            script_table.entry(use_script).is_some(),
            "enemy {index} attack-equivalent item {} references unavailable use script {use_script}",
            enemy.attack_equivalent_item
        );
        enemy_attack_items += 1;
    }
    assert!(
        enemy_attack_items > 0,
        "battle data contains no attack-equivalent enemy items"
    );
    let auto_scripts = script_table.clone();
    let mut battle_scripts = ScriptRuntime::new(auto_scripts.clone());
    let mut scripts = ScriptRuntime::new(script_table);
    assert!(scripts.start(trigger));
    let mut script_messages = 0;
    loop {
        match scripts
            .advance()
            .expect("script runtime stopped without an event")
        {
            ScriptEvent::Message { message_id, .. } => {
                assert!(
                    text.message(usize::from(message_id)).is_some(),
                    "script references an unavailable message"
                );
                script_messages += 1;
            }
            ScriptEvent::Completed { .. } => break,
            event => panic!("message script did not complete: {event:?}"),
        }
    }
    assert!(script_messages > 0, "message script yielded no messages");
    let movement_trigger = pal_core::scene::TriggerRequest {
        object_id: 2,
        script_entry: 69,
        kind: pal_core::scene::TriggerKind::Touch,
    };
    assert!(scripts.start(movement_trigger));
    let mut script_ticks = 0;
    loop {
        match scripts
            .advance()
            .expect("movement script stopped without an event")
        {
            ScriptEvent::Action(_)
            | ScriptEvent::Waiting
            | ScriptEvent::Delay
            | ScriptEvent::Confirm { .. }
            | ScriptEvent::OpenBuyMenu { .. }
            | ScriptEvent::OpenSellMenu
            | ScriptEvent::FadeScene { .. }
            | ScriptEvent::Visual(_)
            | ScriptEvent::WaitForKey => script_ticks += 1,
            ScriptEvent::Completed { .. } => break,
            event => panic!("movement script did not complete: {event:?}"),
        }
    }
    assert!(script_ticks > 0, "movement script yielded no timed work");

    let intro_trigger = pal_core::scene::TriggerRequest {
        object_id: 0xffff,
        script_entry: initial_enter_script,
        kind: pal_core::scene::TriggerKind::Touch,
    };
    assert!(scripts.start(intro_trigger));
    let mut intro_messages = 0;
    let mut intro_actions = 0;
    loop {
        match scripts
            .advance()
            .expect("scene enter script stopped without an event")
        {
            ScriptEvent::Message { message_id, .. } => {
                assert!(text.message(usize::from(message_id)).is_some());
                intro_messages += 1;
            }
            ScriptEvent::Action(ScriptAction::WalkPlayerTo {
                tile_x,
                tile_y,
                half,
                speed,
                repeat_entry,
            }) => {
                match game.walk_player_to(tile_x, tile_y, half, speed) {
                    Some(true) => {}
                    Some(false) => assert!(scripts.branch_to(repeat_entry)),
                    None => panic!("scene enter party walk target is invalid"),
                }
                intro_actions += 1;
            }
            ScriptEvent::Action(ScriptAction::RideObjectTo {
                object_id,
                tile_x,
                tile_y,
                half,
                speed,
                repeat_entry,
            }) => {
                match game.ride_object_to(object_id, tile_x, tile_y, half, speed) {
                    Some(true) => {}
                    Some(false) => assert!(scripts.branch_to(repeat_entry)),
                    None => panic!("scene enter ride target is invalid"),
                }
                game.update_auto_scripts(&auto_scripts)
                    .expect("scene 1 auto script failed during party ride");
                intro_actions += 1;
            }
            ScriptEvent::Action(action @ ScriptAction::MoveViewport { x, y, frames }) => {
                assert!(game.apply_script_action(action));
                if (x != 0 || y != 0) && frames != -1 {
                    game.update_auto_scripts(&auto_scripts)
                        .expect("scene 1 auto script failed during viewport movement");
                }
                intro_actions += 1;
            }
            ScriptEvent::Action(action) => {
                assert!(
                    game.apply_script_action(action),
                    "scene enter action could not be applied: {action:?}"
                );
                intro_actions += 1;
            }
            ScriptEvent::Waiting => {
                game.update_auto_scripts(&auto_scripts)
                    .expect("scene 1 auto script failed during trigger wait");
            }
            ScriptEvent::Delay
            | ScriptEvent::Confirm { .. }
            | ScriptEvent::OpenBuyMenu { .. }
            | ScriptEvent::OpenSellMenu
            | ScriptEvent::FadeScene { .. }
            | ScriptEvent::Visual(_)
            | ScriptEvent::WaitForKey => {}
            ScriptEvent::Completed { .. } => break,
            event => panic!("scene enter script did not complete: {event:?}"),
        }
    }
    assert_eq!(intro_messages, 67);
    assert!(intro_actions > 0);
    assert!(
        game.scene_objects
            .iter()
            .find(|object| object.id == 11)
            .is_some_and(|object| object.state == 0),
        "scene 1 NPC did not leave during trigger-script waits"
    );
    assert!(
        game.scene_objects
            .iter()
            .find(|object| object.id == 4)
            .is_some_and(|object| object.state > 0),
        "scene 1 bedroom exit was not enabled after the NPC left"
    );

    for _ in 0..128 {
        game.update_auto_scripts(&auto_scripts)
            .expect("scene 1 auto script failed");
        let door_open = game
            .scene_objects
            .iter()
            .find(|object| object.id == 4)
            .is_some_and(|object| object.state > 0);
        let npc_finished = game
            .scene_objects
            .iter()
            .find(|object| object.id == 11)
            .is_some_and(|object| object.state == 0);
        if door_open && npc_finished {
            break;
        }
    }
    assert!(
        game.scene_objects
            .iter()
            .find(|object| object.id == 4)
            .is_some_and(|object| object.state > 0),
        "scene 1 auto script did not enable the bedroom exit"
    );

    let item_trigger = pal_core::scene::TriggerRequest {
        object_id: 5,
        script_entry: 6318,
        kind: pal_core::scene::TriggerKind::Search,
    };
    assert!(scripts.start(item_trigger));
    loop {
        match scripts
            .advance()
            .expect("item script stopped without an event")
        {
            ScriptEvent::Message { message_id, .. } => {
                assert!(text.message(usize::from(message_id)).is_some());
            }
            ScriptEvent::Action(action) => assert!(game.apply_script_action(action)),
            ScriptEvent::Waiting => {}
            ScriptEvent::Delay
            | ScriptEvent::Confirm { .. }
            | ScriptEvent::OpenBuyMenu { .. }
            | ScriptEvent::OpenSellMenu
            | ScriptEvent::FadeScene { .. }
            | ScriptEvent::Visual(_)
            | ScriptEvent::WaitForKey => {}
            ScriptEvent::Completed { .. } => break,
            event => panic!("item script did not complete: {event:?}"),
        }
    }
    assert_eq!(game.item_count(99), 1);

    assert!(game.apply_script_action(ScriptAction::AdjustPlayerHealth {
        role_id: 0,
        hp: -25,
        mp: 0,
        apply_to_all: false,
    }));
    let damaged_hp = game.player_role(0).unwrap().hp;
    let use_request = game
        .item_use_request(99, Some(0))
        .expect("scene item 99 is not usable by the initial role");
    assert!(scripts.start(use_request));
    loop {
        match scripts
            .advance()
            .expect("item use script stopped without an event")
        {
            ScriptEvent::Action(
                action @ (ScriptAction::AdjustPlayerHealth { .. }
                | ScriptAction::RevivePlayer { .. }),
            ) => {
                let succeeded = game.apply_script_action(action);
                assert!(scripts.set_success(succeeded));
            }
            ScriptEvent::Action(action) => assert!(game.apply_script_action(action)),
            ScriptEvent::Completed {
                next_entry,
                succeeded,
                ..
            } => {
                assert!(succeeded, "item 99 recovery script reported failure");
                assert!(game.finish_item_use(99, next_entry, succeeded));
                break;
            }
            event => panic!("item 99 use did not complete: {event:?}"),
        }
    }
    assert!(game.player_role(0).unwrap().hp > damaged_hp);
    assert_eq!(game.inventory_count(99), 0);

    game.player.world_x = 1152;
    game.player.world_y = 384;
    assert!(game.update(Default::default()));
    let exit_trigger = game
        .take_trigger()
        .expect("walking into the enabled bedroom exit did not trigger it");
    assert_eq!(exit_trigger.object_id, 4);
    assert_eq!(exit_trigger.script_entry, 4475);
    assert!(scripts.start(exit_trigger));
    let mut switched_scene = None;
    loop {
        match scripts
            .advance()
            .expect("exit script stopped without an event")
        {
            ScriptEvent::Action(pal_core::script::ScriptAction::ChangeScene { scene_number }) => {
                let loaded =
                    load_runtime_scene(&data_dir, &scene_data, scene_number, &role_sprites)
                        .expect("exit script target scene could not be loaded");
                game.replace_scene(loaded.number, loaded.map, loaded.objects);
                switched_scene = Some(scene_number);
            }
            ScriptEvent::Action(action) => assert!(game.apply_script_action(action)),
            ScriptEvent::Waiting => {}
            ScriptEvent::Delay
            | ScriptEvent::Confirm { .. }
            | ScriptEvent::OpenBuyMenu { .. }
            | ScriptEvent::OpenSellMenu
            | ScriptEvent::FadeScene { .. }
            | ScriptEvent::Visual(_)
            | ScriptEvent::WaitForKey => {}
            ScriptEvent::Completed { .. } => break,
            event => panic!("exit script did not complete: {event:?}"),
        }
    }
    assert_eq!(switched_scene, Some(3));
    assert_eq!(game.scene_number, 3);

    let inn_trigger = pal_core::scene::TriggerRequest {
        object_id: 57,
        script_entry: 4701,
        kind: pal_core::scene::TriggerKind::Search,
    };
    assert!(scripts.start(inn_trigger));
    let mut inn_messages = 0;
    loop {
        match scripts
            .advance()
            .expect("inn conversation stopped without an event")
        {
            ScriptEvent::Message { message_id, .. } => {
                assert!(text.message(usize::from(message_id)).is_some());
                inn_messages += 1;
            }
            ScriptEvent::Action(action) => assert!(
                game.apply_script_action(action),
                "inn conversation action could not be applied: {action:?}"
            ),
            ScriptEvent::Waiting => {
                game.update_auto_scripts(&auto_scripts)
                    .expect("inn auto script failed during conversation");
            }
            ScriptEvent::Delay
            | ScriptEvent::Confirm { .. }
            | ScriptEvent::OpenBuyMenu { .. }
            | ScriptEvent::OpenSellMenu
            | ScriptEvent::FadeScene { .. }
            | ScriptEvent::Visual(_)
            | ScriptEvent::WaitForKey => {}
            ScriptEvent::Completed { .. } => break,
            event => panic!("inn conversation did not complete: {event:?}"),
        }
    }
    assert!(inn_messages >= 20);
    for _ in 0..256 {
        game.update_auto_scripts(&auto_scripts)
            .expect("inn auto script failed after conversation");
    }
    for (object_id, expected_position) in [
        (57, (1136, 1624)),
        (60, (1312, 1328)),
        (61, (1456, 1416)),
        (62, (1440, 1408)),
    ] {
        let object = game
            .scene_objects
            .iter()
            .find(|object| object.id == object_id)
            .expect("inn actor is missing");
        assert_eq!(
            (object.world_x, object.world_y),
            expected_position,
            "inn actor {object_id} did not finish moving; auto script is {}",
            object.auto_script
        );
    }
    for object_id in [60, 61, 62] {
        assert_eq!(
            game.object_state(object_id),
            Some(0),
            "lobby actor {object_id} was not hidden after reaching the stairs"
        );
    }
    for object_id in [25, 26, 27] {
        assert_eq!(
            game.object_state(object_id),
            Some(2),
            "upstairs actor {object_id} was not enabled"
        );
    }

    game.cash = 1_234;
    let snapshot_bytes = game
        .encode_snapshot()
        .expect("failed to encode real-resource M3 snapshot");
    let snapshot = game
        .decode_snapshot(&snapshot_bytes)
        .expect("failed to decode real-resource M3 snapshot");
    game.cash = 0;
    let snapshot_scene = load_runtime_scene(
        &data_dir,
        &scene_data,
        snapshot.scene_number(),
        &role_sprites,
    )
    .expect("snapshot scene could not be reloaded");
    game.restore_snapshot(snapshot, snapshot_scene.map);
    assert_eq!(game.scene_number, 3);
    assert_eq!(game.cash, 1_234);
    assert_eq!(game.item_count(99), 0);

    game.player.world_x = 1472;
    game.player.world_y = 1520;
    assert!(game.update(Default::default()));
    let stairs_trigger = game
        .take_trigger()
        .expect("scene 3 stairs did not trigger after the actors moved");
    assert_eq!(stairs_trigger.object_id, 46);
    assert_eq!(stairs_trigger.script_entry, 4659);
    assert!(scripts.start(stairs_trigger));
    loop {
        match scripts
            .advance()
            .expect("stairs script stopped without an event")
        {
            ScriptEvent::Action(ScriptAction::ChangeScene { scene_number }) => {
                let loaded =
                    load_runtime_scene(&data_dir, &scene_data, scene_number, &role_sprites)
                        .expect("stairs target scene could not be loaded");
                game.replace_scene(loaded.number, loaded.map, loaded.objects);
            }
            ScriptEvent::Action(action) => assert!(game.apply_script_action(action)),
            ScriptEvent::Delay
            | ScriptEvent::Waiting
            | ScriptEvent::Confirm { .. }
            | ScriptEvent::OpenBuyMenu { .. }
            | ScriptEvent::OpenSellMenu
            | ScriptEvent::FadeScene { .. }
            | ScriptEvent::Visual(_)
            | ScriptEvent::WaitForKey => {}
            ScriptEvent::Completed { .. } => break,
            event => panic!("stairs script did not complete: {event:?}"),
        }
    }
    assert_eq!(game.scene_number, 1);
    for object_id in [25, 26, 27] {
        assert_eq!(game.object_state(object_id), Some(2));
    }
    let first_store = game.store_items(0).expect("store 0 is unavailable");
    assert!(!first_store.is_empty(), "store 0 contains no items");
    assert!(
        first_store
            .iter()
            .all(|item| text.word(usize::from(item.item_id)).is_some()),
        "store 0 references an unavailable item name"
    );

    let first_battle_actor = game
        .scene_objects
        .iter()
        .find(|object| object.id == 28)
        .expect("first battle event object 28 is unavailable");
    assert_eq!(first_battle_actor.trigger_script, 6906);
    let battle_trigger = pal_core::scene::TriggerRequest {
        object_id: first_battle_actor.id,
        script_entry: first_battle_actor.trigger_script,
        kind: pal_core::scene::TriggerKind::Search,
    };
    assert!(scripts.start(battle_trigger));
    let mut first_battle_messages = 0;
    let mut first_battle_actions = 0;
    let battle_request = loop {
        match scripts
            .advance()
            .expect("first battle script stopped before BATTLE")
        {
            ScriptEvent::Message { message_id, .. } => {
                assert!(text.message(usize::from(message_id)).is_some());
                first_battle_messages += 1;
            }
            ScriptEvent::Action(action) => {
                assert!(
                    game.apply_script_action(action),
                    "first battle setup action could not be applied: {action:?}"
                );
                first_battle_actions += 1;
            }
            ScriptEvent::Waiting => {
                game.update_auto_scripts(&auto_scripts)
                    .expect("first battle setup auto script failed");
            }
            ScriptEvent::Delay
            | ScriptEvent::FadeScene { .. }
            | ScriptEvent::Visual(_)
            | ScriptEvent::WaitForKey => {}
            ScriptEvent::StartBattle(request) => break request,
            event => panic!("first battle setup yielded an unexpected event: {event:?}"),
        }
    };
    assert!(
        first_battle_messages >= 20,
        "first battle setup yielded only {first_battle_messages} messages"
    );
    assert!(
        first_battle_actions >= 10,
        "first battle setup yielded only {first_battle_actions} actions"
    );
    assert_eq!(battle_request.enemy_team, 18);
    assert_eq!(battle_request.lost_entry, 40091);
    assert_eq!(battle_request.flee_entry, 0);
    assert!(battle_request.is_boss);
    assert_eq!(game.current_battlefield, 21);
    assert_eq!(
        game.party
            .members()
            .iter()
            .map(|member| member.role_id)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert!(game.apply_script_action(ScriptAction::AdjustPlayerHealth {
        role_id: u16::MAX,
        hp: i16::MAX,
        mp: i16::MAX,
        apply_to_all: true,
    }));
    let base_attack = game
        .player_role(0)
        .expect("first battle leader role is unavailable")
        .attack_strength;
    assert!(game.start_battle(battle_request, &auto_scripts));
    let battle = game.battle().expect("first battle state was not created");
    assert_eq!(battle.enemy_team, 18);
    assert_eq!(battle.battlefield, 21);
    assert_eq!(battle.enemies.len(), 2);
    assert!(battle.enemies.iter().all(|enemy| enemy.object_id == 495));
    assert!(
        battle.players[0].attack_strength > base_attack,
        "initial equipment effects were not applied before battle"
    );
    assert!(scripts.is_waiting_for_battle());
    let battle_background = battle_backgrounds
        .get(usize::from(battle.battlefield))
        .and_then(Option::as_ref)
        .expect("first battle background is unavailable");
    renderer.blit_bitmap(battle_background, 0, 0);
    let background_only = renderer.screen().to_vec();
    for player in &battle.players {
        assert!(
            player_battle_sprites
                .decode_frame(usize::from(player.battle_sprite_num), 0)
                .is_some(),
            "first battle player sprite frame is unavailable"
        );
    }
    render_battle(
        &mut renderer,
        battle,
        BattleRenderResources {
            enemy_sprites: &enemy_battle_sprites,
            player_sprites: &player_battle_sprites,
            backgrounds: &battle_backgrounds,
            text: &text,
            font: &font,
        },
        BattleRenderState {
            selected_enemy: battle.first_living_enemy().unwrap_or(0),
            selected_command: 0,
            ticks: 0,
            event: None,
            event_ticks: 0,
        },
    );
    let battle_idle_frame = renderer.screen().to_vec();
    let battle_sprite_pixels = renderer
        .screen()
        .chunks_exact(4)
        .zip(background_only.chunks_exact(4))
        .filter(|(with_sprites, background)| with_sprites != background)
        .count();
    assert!(battle_sprite_pixels > 0, "battle screen did not render");

    let blow_event = |blow| pal_core::battle::BattleEvent::PlayerMagic {
        player: 0,
        enemy: 0,
        magic_object: 314,
        blow,
        damage: 0,
        defeated: false,
    };
    render_battle(
        &mut renderer,
        battle,
        BattleRenderResources {
            enemy_sprites: &enemy_battle_sprites,
            player_sprites: &player_battle_sprites,
            backgrounds: &battle_backgrounds,
            text: &text,
            font: &font,
        },
        BattleRenderState {
            selected_enemy: 0,
            selected_command: 0,
            ticks: 0,
            event: Some(blow_event(0)),
            event_ticks: 1,
        },
    );
    let magic_without_blow = renderer.screen().to_vec();
    render_battle(
        &mut renderer,
        battle,
        BattleRenderResources {
            enemy_sprites: &enemy_battle_sprites,
            player_sprites: &player_battle_sprites,
            backgrounds: &battle_backgrounds,
            text: &text,
            font: &font,
        },
        BattleRenderState {
            selected_enemy: 0,
            selected_command: 0,
            ticks: 0,
            event: Some(blow_event(-3)),
            event_ticks: 1,
        },
    );
    let battle_blow_pixels = renderer
        .screen()
        .chunks_exact(4)
        .zip(magic_without_blow.chunks_exact(4))
        .filter(|(blown, stationary)| blown != stationary)
        .count();
    assert!(
        battle_blow_pixels > 0,
        "signed magic blow did not displace rendered enemies"
    );
    render_battle(
        &mut renderer,
        battle,
        BattleRenderResources {
            enemy_sprites: &enemy_battle_sprites,
            player_sprites: &player_battle_sprites,
            backgrounds: &battle_backgrounds,
            text: &text,
            font: &font,
        },
        BattleRenderState {
            selected_enemy: 0,
            selected_command: 0,
            ticks: 0,
            event: Some(pal_core::battle::BattleEvent::PlayerMagicAnimation { player: None }),
            event_ticks: 1,
        },
    );
    let magic_color_shift_pixels = renderer
        .screen()
        .chunks_exact(4)
        .zip(battle_idle_frame.chunks_exact(4))
        .filter(|(shifted, idle)| shifted != idle)
        .count();
    assert!(
        magic_color_shift_pixels > 0,
        "scripted magic animation did not color-shift the party"
    );
    render_battle(
        &mut renderer,
        battle,
        BattleRenderResources {
            enemy_sprites: &enemy_battle_sprites,
            player_sprites: &player_battle_sprites,
            backgrounds: &battle_backgrounds,
            text: &text,
            font: &font,
        },
        BattleRenderState {
            selected_enemy: 0,
            selected_command: 0,
            ticks: 0,
            event: Some(pal_core::battle::BattleEvent::PlayerMagicAnimation { player: Some(1) }),
            event_ticks: 5,
        },
    );
    let pre_magic_pixels = renderer
        .screen()
        .chunks_exact(4)
        .zip(battle_idle_frame.chunks_exact(4))
        .filter(|(casting, idle)| casting != idle)
        .count();
    assert!(
        pre_magic_pixels > 0,
        "scripted player magic animation did not change the caster frame"
    );

    let mut battle_feedback_pixels = 0;
    for _ in 0..1024 {
        let events = game.advance_battle_resolution();
        if let Some(request) = game.take_battle_script() {
            run_headless_battle_script(&mut game, &mut battle_scripts, request, &text);
            continue;
        }
        if !matches!(
            game.battle().map(|battle| battle.phase()),
            Some(BattlePhase::AwaitingCommand)
        ) {
            break;
        }
        if battle_feedback_pixels == 0 {
            if let Some(event) = events.iter().copied().find(|event| {
                matches!(
                    event,
                    pal_core::battle::BattleEvent::PlayerAttack { .. }
                        | pal_core::battle::BattleEvent::PlayerMagic { .. }
                        | pal_core::battle::BattleEvent::EnemyAttack { .. }
                        | pal_core::battle::BattleEvent::EnemyMagic { .. }
                        | pal_core::battle::BattleEvent::EnemyConfusedAttack { .. }
                        | pal_core::battle::BattleEvent::PlayerConfusedAttack { .. }
                        | pal_core::battle::BattleEvent::SimulatedMagic { .. }
                )
            }) {
                let battle = game
                    .battle()
                    .expect("first battle disappeared during action feedback");
                render_battle(
                    &mut renderer,
                    battle,
                    BattleRenderResources {
                        enemy_sprites: &enemy_battle_sprites,
                        player_sprites: &player_battle_sprites,
                        backgrounds: &battle_backgrounds,
                        text: &text,
                        font: &font,
                    },
                    BattleRenderState {
                        selected_enemy: battle.first_living_enemy().unwrap_or(0),
                        selected_command: 0,
                        ticks: 4,
                        event: Some(event),
                        event_ticks: 4,
                    },
                );
                battle_feedback_pixels = renderer
                    .screen()
                    .chunks_exact(4)
                    .zip(battle_idle_frame.chunks_exact(4))
                    .filter(|(feedback, idle)| feedback != idle)
                    .count();
            }
        }
        if game
            .battle()
            .and_then(|battle| battle.active_player())
            .is_none()
        {
            continue;
        }
        let (target, magic) = {
            let battle = game.battle().expect("first battle disappeared");
            let target = battle
                .first_living_enemy()
                .expect("active first battle has no living enemy");
            let player = &battle.players[battle.active_player().expect("no active player")];
            let magic = player
                .magics
                .iter()
                .enumerate()
                .filter(|(_, magic)| player.mp >= magic.mp_cost)
                .max_by_key(|(_, magic)| magic.base_damage)
                .map(|(index, _)| index);
            (target, magic)
        };
        let committed = if let Some(magic) = magic {
            game.battle_mut()
                .and_then(|battle| battle.cast_magic(magic, target))
        } else {
            game.battle_mut().and_then(|battle| battle.attack(target))
        }
        .expect("headless first-battle action was rejected");
        assert!(
            committed.is_empty(),
            "commands should resolve through the action queue"
        );
    }
    assert!(
        battle_feedback_pixels > 0,
        "battle action feedback did not change the rendered frame"
    );
    let finished_battle = game
        .battle()
        .expect("finished first battle state disappeared before settlement");
    render_battle(
        &mut renderer,
        finished_battle,
        BattleRenderResources {
            enemy_sprites: &enemy_battle_sprites,
            player_sprites: &player_battle_sprites,
            backgrounds: &battle_backgrounds,
            text: &text,
            font: &font,
        },
        BattleRenderState {
            selected_enemy: 0,
            selected_command: 0,
            ticks: 0,
            event: None,
            event_ticks: 0,
        },
    );
    let settlement_border_pixels = renderer
        .screen()
        .chunks_exact(4)
        .filter(|pixel| *pixel == [255, 236, 80, 255])
        .count();
    assert!(
        settlement_border_pixels > 0,
        "battle settlement overlay did not render"
    );
    let cash_before_battle = game.cash;
    let (battle_result, battle_rewards) = game
        .settle_battle()
        .expect("headless first battle did not finish");
    assert_eq!(battle_result, BattleResult::Won);
    assert_eq!(battle_rewards.experience, 52);
    assert_eq!(battle_rewards.cash, 96);
    assert_eq!(game.cash, cash_before_battle + 96);
    assert!(scripts.resolve_battle(battle_result));
    assert!(!scripts.is_waiting_for_battle());
    match scripts
        .advance()
        .expect("first battle script did not continue after victory")
    {
        ScriptEvent::Action(action @ ScriptAction::PlayMusic { music_id: 24, .. }) => {
            assert!(game.apply_script_action(action));
        }
        event => panic!("first battle victory continued with unexpected event: {event:?}"),
    }
    assert_eq!(game.current_music, Some(24));
    assert!(game.battle().is_none());
    let mut post_battle_actions = 0;
    let post_battle_message = loop {
        match scripts
            .advance()
            .expect("first battle follow-up stopped before the next story message")
        {
            ScriptEvent::Message { message_id, .. } => break message_id,
            ScriptEvent::Action(action) => {
                assert!(
                    game.apply_script_action(action),
                    "first battle follow-up action could not be applied: {action:?}"
                );
                post_battle_actions += 1;
            }
            ScriptEvent::Waiting => {
                game.update_auto_scripts(&auto_scripts)
                    .expect("first battle follow-up auto script failed");
            }
            ScriptEvent::Delay
            | ScriptEvent::FadeScene { .. }
            | ScriptEvent::Visual(_)
            | ScriptEvent::WaitForKey => {}
            event => panic!("first battle follow-up yielded an unexpected event: {event:?}"),
        }
    };
    assert!(post_battle_actions >= 1);
    assert!(text.message(usize::from(post_battle_message)).is_some());
    let mut extended_story_messages = 1usize;
    let mut extended_story_actions = 0usize;
    let extended_story_next_entry = loop {
        match scripts
            .advance()
            .expect("extended post-battle story stopped without an event")
        {
            ScriptEvent::Message { message_id, .. } => {
                assert!(
                    text.message(usize::from(message_id)).is_some(),
                    "extended story references unavailable message {message_id}"
                );
                extended_story_messages += 1;
            }
            ScriptEvent::Action(
                action @ (ScriptAction::AdjustPlayerHealth { .. }
                | ScriptAction::RevivePlayer { .. }),
            ) => {
                let succeeded = game.apply_script_action(action);
                assert!(scripts.set_success(succeeded));
                extended_story_actions += 1;
            }
            ScriptEvent::Action(action) => {
                assert!(
                    game.apply_script_action(action),
                    "extended story action could not be applied: {action:?}"
                );
                extended_story_actions += 1;
            }
            ScriptEvent::Waiting => {
                game.update_auto_scripts(&auto_scripts)
                    .expect("extended story auto script failed");
            }
            ScriptEvent::Delay
            | ScriptEvent::FadeScene { .. }
            | ScriptEvent::Visual(_)
            | ScriptEvent::WaitForKey => {}
            ScriptEvent::Completed { next_entry, .. } => break next_entry,
            event => panic!("extended post-battle story yielded an unexpected event: {event:?}"),
        }
    };
    assert!(extended_story_messages >= 10);
    assert!(extended_story_actions >= 10);
    assert_eq!(game.party.members().len(), 2);

    assert!(visible_pixels > 0, "rendered map is blank");
    assert!(
        chromatic_pixels > 0,
        "rendered map contains no chromatic pixels; check palette selection"
    );
    assert!(
        sprite_pixels > 0,
        "scene sprite did not change the framebuffer"
    );
    assert!(
        event_object_pixels > 0,
        "event object did not change the framebuffer"
    );
    assert!(
        text_pixels > 0,
        "bitmap text did not change the framebuffer"
    );
    println!(
        "scene sprite rendered: {sprite_pixels} pixels, {} slots loaded",
        role_sprites.character_count()
    );
    println!(
        "scene data passed: {} scenes, {} event objects total, {} in scene {}",
        scene_data.scene_count(),
        scene_data.event_object_count(),
        initial_event_sprite_numbers.len(),
        initial_scene_number,
    );
    println!(
        "event object {} rendered: {event_object_pixels} pixels",
        visible_object_id
    );
    println!(
        "text data passed: {} words, {} messages, {} glyphs, {text_pixels} sample pixels, \
         {speed_controls} speed, {terminal_controls} terminal and {icon_controls} icon controls",
        text.word_count(),
        text.message_count(),
        font.glyph_count(),
    );
    println!(
        "script data passed: {} records, {script_messages} messages, {script_ticks} timed actions, \
         {enemy_turn_jumps} enemy-turn branches, {simulated_magic_scripts} simulated, \
         {thrown_weapon_scripts} weapon-throw, {mp_scaled_magic_scripts} MP-scaled and \
         {cash_scaled_magic_scripts} cash-scaled magic, {temporary_stat_scripts} temporary-stat \
         and {temporary_sprite_scripts} temporary-sprite scripts",
        script_count,
    );
    println!("sound data passed: {sound_effect_count} PCM VOC effects");
    println!("music data passed: {music_count} standard MIDI songs");
    println!(
        "cutscene data passed: {} RNG animations, {rng_frame_count} decoded frames, \
         {fbp_frame_count} FBP pictures, {fbp_script_references} script references, {} ending \
         sprites, {fbp_black_fallbacks} black fallbacks",
        rng_archive.animation_count(),
        ending_sprite_references.len(),
    );
    println!("store data passed: {} items in store 0", first_store.len());
    println!(
        "battle data passed: {} enemies, {} teams, {} battlefields, {enemy_attack_items} \
         attack-equivalent item definitions, {player_confusion_scripts} player-confusion and \
         {player_haste_scripts} player-haste scripts",
        battle_data.enemies.len(),
        battle_data.enemy_teams.len(),
        battle_data.battlefields.len(),
    );
    println!(
        "battle item data passed: {usable_item_definitions} usable and \
         {throwable_item_definitions} throwable object definitions"
    );
    println!(
        "battle graphics passed: {} ABC slots, {} F slots, {} screen pixels, {} feedback pixels, \
         {battle_blow_pixels} blow pixels, {magic_color_shift_pixels} color-shift pixels, \
         {pre_magic_pixels} pre-magic pixels, {} settlement pixels",
        enemy_battle_sprites.len(),
        player_battle_sprites.len(),
        battle_sprite_pixels,
        battle_feedback_pixels,
        settlement_border_pixels,
    );
    println!("SoundFont data passed: {} bytes", sound_font.len());
    println!(
        "M3 data passed: {} party member, {} role definitions, {} {:?} object definitions",
        game.party.members().len(),
        player_roles.iter().len(),
        global_objects.len(),
        global_objects.layout(),
    );
    println!(
            "M4 flow passed: {intro_messages} intro messages, {intro_actions} intro actions, item 99 acquired and used, scene 3 loaded, snapshot restored"
        );
    println!(
        "M5 first battle passed: object 28 entry 6906, {first_battle_messages} setup messages, {first_battle_actions} setup actions, script 6965, team 18, 2 enemies, {} EXP, {} cash, {post_battle_actions} follow-up actions, message {post_battle_message}",
        battle_rewards.experience,
        battle_rewards.cash,
    );
    println!(
        "M6 extended story passed: {extended_story_messages} messages, \
         {extended_story_actions} actions, {} party members, next entry {extended_story_next_entry}",
        game.party.members().len(),
    );
    println!(
        "M6 dynamic enemy data passed: {divide_enemy_scripts} divisions, \
         {summon_enemy_scripts} summons, {transform_enemy_scripts} transformations"
    );
    println!(
        "M6 world-state data passed: {pause_chase_scripts} chase pause, \
         {speed_chase_scripts} chase speed-up, {level_up_scripts} scripted level-up, \
         {halve_cash_scripts} cash-halving, {set_object_script_scripts} object-script mutations"
    );
    println!(
        "M6 script fallback data passed: {player_sprite_scripts} player-sprite changes, \
         {outside_zone_scripts} object-zone branches, {cd_music_scripts} CD-to-MIDI fallbacks"
    );
    println!(
        "M6 battle command data passed: {collect_enemy_scripts} collection, \
         {transmute_scripts} transmutation, {hide_battle_scripts} hiding, \
         {steal_enemy_scripts} stealing, {auto_battle_scripts} auto-battle, {} signed magic blows, \
         {} player magic animation",
        battle_blow_amounts.len(),
        player_magic_animation_players.len(),
    );
    println!(
        "asset check passed: {visible_pixels} visible pixels, \
             {chromatic_pixels} chromatic pixels"
    );
}

fn text_control_counts(text: &pal_assets::text::TextLibrary) -> (usize, usize, usize) {
    let mut speed = 0;
    let mut terminal = 0;
    let mut icon = 0;
    for message_id in 0..text.message_count() {
        let Some(message) = text.message(message_id) else {
            continue;
        };
        let mut index = 0;
        while index < message.len() {
            match message[index] {
                byte if byte >= 0x80 => index += (message.len() - index).min(2),
                b'\\' => index += (message.len() - index).min(2),
                b'$' => {
                    speed += 1;
                    index += (message.len() - index).min(3);
                }
                b'~' => {
                    terminal += 1;
                    index += (message.len() - index).min(3);
                }
                b'(' | b')' => {
                    icon += 1;
                    index += 1;
                }
                _ => index += 1,
            }
        }
    }
    (speed, terminal, icon)
}

fn run_headless_battle_script(
    game: &mut pal_core::game::GameState,
    scripts: &mut ScriptRuntime,
    request: pal_core::scene::TriggerRequest,
    text: &pal_assets::text::TextLibrary,
) {
    assert!(scripts.start(request), "battle script runtime was busy");
    for _ in 0..4096 {
        match scripts
            .advance()
            .expect("battle script stopped without completing")
        {
            ScriptEvent::Action(
                action @ (ScriptAction::AdjustPlayerHealth { .. }
                | ScriptAction::RevivePlayer { .. }),
            ) => {
                let succeeded = game.apply_script_action(action);
                assert!(scripts.set_success(succeeded));
            }
            ScriptEvent::Action(action @ ScriptAction::SetEnemyStatus { resisted_entry, .. }) => {
                if !game.apply_script_action(action) {
                    assert!(scripts.branch_to(resisted_entry));
                }
            }
            ScriptEvent::Action(action @ ScriptAction::FleeBattle { failure_entry }) => {
                if !game.apply_script_action(action) {
                    assert!(scripts.branch_to(failure_entry));
                }
            }
            ScriptEvent::Action(
                action @ (ScriptAction::DivideEnemy { failure_entry, .. }
                | ScriptAction::SummonEnemy { failure_entry, .. }),
            ) => {
                if !game.apply_script_action(action) && failure_entry != 0 {
                    assert!(scripts.branch_to(failure_entry));
                }
            }
            ScriptEvent::Action(action @ ScriptAction::CollectEnemy { failure_entry, .. }) => {
                if !game.apply_script_action(action) {
                    assert!(scripts.branch_to(failure_entry));
                }
            }
            ScriptEvent::Action(action) => assert!(
                game.apply_script_action(action),
                "battle script action could not be applied: {action:?}"
            ),
            ScriptEvent::Condition(condition) => {
                let (matches, target_entry) = match condition {
                    ScriptCondition::ItemCountLess {
                        item_id,
                        amount,
                        target_entry,
                    } => (
                        i32::from(game.item_count(item_id)) < i32::from(amount),
                        target_entry,
                    ),
                    ScriptCondition::ObjectStateEquals {
                        object_id,
                        state,
                        target_entry,
                    } => (game.object_state(object_id) == Some(state), target_entry),
                    ScriptCondition::SceneEquals {
                        scene_number,
                        target_entry,
                    } => (game.scene_number == scene_number, target_entry),
                    ScriptCondition::PartyContainsName {
                        name_word_id,
                        target_entry,
                    } => (game.party_contains_name(name_word_id), target_entry),
                    ScriptCondition::PlayerFacesObject {
                        object_id,
                        range,
                        target_entry,
                    } => (!game.player_faces_object(object_id, range), target_entry),
                    ScriptCondition::PartyNotFullHp { target_entry } => {
                        (game.party_not_full_hp(), target_entry)
                    }
                    ScriptCondition::ItemNotEquipped {
                        item_id,
                        amount,
                        target_entry,
                    } => (game.equipped_item_count(item_id) < amount, target_entry),
                    ScriptCondition::PlayerLacksPoison {
                        role_id,
                        poison_id,
                        target_entry,
                    } => (!game.player_has_poison(role_id, poison_id), target_entry),
                    ScriptCondition::EnemyLacksPoison {
                        enemy_index,
                        poison_id,
                        target_entry,
                    } => (!game.enemy_has_poison(enemy_index, poison_id), target_entry),
                    ScriptCondition::PlayerNotPoisoned {
                        role_id,
                        target_entry,
                    } => (
                        game.player_poisons(role_id).is_none_or(|poisons| {
                            poisons.iter().all(|poison| poison.object_id == 0)
                        }),
                        target_entry,
                    ),
                    ScriptCondition::EnemyHpAbove {
                        enemy_index,
                        percentage,
                        target_entry,
                    } => (game.enemy_hp_above(enemy_index, percentage), target_entry),
                    ScriptCondition::EnemyNotFirstKind {
                        enemy_index,
                        target_entry,
                    } => (game.enemy_not_first_kind(enemy_index), target_entry),
                    ScriptCondition::EnemyTurn { target_entry } => {
                        (game.is_enemy_turn(), target_entry)
                    }
                };
                if matches {
                    assert!(scripts.branch_to(target_entry));
                }
            }
            ScriptEvent::Message { message_id, .. } => assert!(
                text.message(usize::from(message_id)).is_some(),
                "battle script references unavailable message {message_id}"
            ),
            ScriptEvent::Waiting
            | ScriptEvent::Delay
            | ScriptEvent::FadeScene { .. }
            | ScriptEvent::Visual(_)
            | ScriptEvent::WaitForKey => {}
            ScriptEvent::Completed {
                next_entry,
                succeeded,
                ..
            } => {
                assert!(game.finish_battle_script(next_entry, succeeded));
                return;
            }
            event => panic!("headless battle script yielded an unexpected event: {event:?}"),
        }
    }
    panic!("headless battle script exceeded the instruction limit");
}
