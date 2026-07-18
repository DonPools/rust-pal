use pal_assets::mkf::MkfArchive;
use pal_core::script::{ScriptAction, ScriptEvent, ScriptRuntime};
use pal_desktop::audio::{validate_midi_output, validate_sound_font};
use pal_desktop::window::{render_tile_map, Viewport};

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
        role_sprites,
        dialog_faces,
        mut game,
        mut renderer,
        ..
    } = boot;
    let viewport = Viewport::from(game.camera);
    let sound_effect_count = validate_sound_effects(&voc_mkf)
        .expect("VOC.MKF contains an invalid or unsupported sound effect");
    let music_count = validate_music(&midi_mkf).expect("MIDI.MKF contains invalid MIDI music");
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
    let auto_scripts = script_table.clone();
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
            | ScriptEvent::FadeScene { .. } => script_ticks += 1,
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
            | ScriptEvent::FadeScene { .. } => {}
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
            | ScriptEvent::FadeScene { .. } => {}
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
            | ScriptEvent::FadeScene { .. } => {}
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
            | ScriptEvent::FadeScene { .. } => {}
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
            | ScriptEvent::FadeScene { .. } => {}
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
        "text data passed: {} words, {} messages, {} glyphs, {text_pixels} sample pixels",
        text.word_count(),
        text.message_count(),
        font.glyph_count(),
    );
    println!(
        "script data passed: {} records, {script_messages} messages, {script_ticks} timed actions",
        script_count,
    );
    println!("sound data passed: {sound_effect_count} PCM VOC effects");
    println!("music data passed: {music_count} standard MIDI songs");
    println!("store data passed: {} items in store 0", first_store.len());
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
        "asset check passed: {visible_pixels} visible pixels, \
             {chromatic_pixels} chromatic pixels"
    );
}
