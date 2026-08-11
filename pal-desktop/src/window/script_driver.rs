use pal_assets::text::TextLibrary;
use pal_core::battle::BattleSteal;
use pal_core::game::{AutoScriptError, GameState};
use pal_core::role::RoleSprites;
use pal_core::scene::{TriggerKind, TriggerRequest};
use pal_core::script::{
    ScriptAction, ScriptCondition, ScriptEvent, ScriptOpcode, ScriptRuntime, ScriptVisual,
};

use super::battle_update::queue_battle_events;
use super::dialog::ActiveDialog;
use super::dialog_text::dialog_body_lines;
use super::menu_state::{
    ConfirmationMenu, FieldMenu, InventoryMenu, InventoryMode, ShopMenu, ShopMode,
};
use super::session::DesktopSession;
use super::LoadedScene;

const MAX_IMMEDIATE_SCRIPT_EVENTS: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScriptAdvanceFlow {
    ContinueImmediately,
    StopForFrame,
}

#[derive(Clone, Copy)]
pub(super) struct ScriptRenderResources<'a> {
    pub(super) text: &'a TextLibrary,
    pub(super) role_sprites: &'a RoleSprites,
}

fn script_event_needs_dialog_confirmation(
    text: &TextLibrary,
    dialog: &ActiveDialog,
    event: ScriptEvent,
) -> bool {
    !matches!(event, ScriptEvent::Message { .. }) && !dialog_body_lines(text, dialog).is_empty()
}

pub(super) fn advance_script<L>(
    scripts: &mut ScriptRuntime,
    game: &mut GameState,
    dialog: &mut Option<ActiveDialog>,
    resources: ScriptRenderResources<'_>,
    load_scene: &mut L,
    services: &mut DesktopSession,
    set_title: &mut impl FnMut(&str),
) where
    L: FnMut(u16, Option<u16>, &RoleSprites) -> Option<LoadedScene>,
{
    // Classic trigger scripts keep interpreting instantaneous host actions until
    // an opcode explicitly presents or waits for a frame. Drain those actions so
    // state setup between a returned battle and a following fade-out cannot expose
    // an intermediate world frame.
    for _ in 0..MAX_IMMEDIATE_SCRIPT_EVENTS {
        if advance_script_once(
            scripts, game, dialog, resources, load_scene, services, set_title,
        ) == ScriptAdvanceFlow::StopForFrame
        {
            return;
        }
    }
    set_title("Rust-PAL [script host event limit]");
}

#[allow(clippy::too_many_arguments)]
fn advance_script_once<L>(
    scripts: &mut ScriptRuntime,
    game: &mut GameState,
    dialog: &mut Option<ActiveDialog>,
    resources: ScriptRenderResources<'_>,
    load_scene: &mut L,
    services: &mut DesktopSession,
    set_title: &mut impl FnMut(&str),
) -> ScriptAdvanceFlow
where
    L: FnMut(u16, Option<u16>, &RoleSprites) -> Option<LoadedScene>,
{
    let event = if let Some(event) = services.scripts.pending_script_event.take() {
        Some(event)
    } else {
        scripts.set_random_state(game.random_state());
        let event = scripts.advance();
        game.set_random_state(scripts.random_state());
        event
    };
    if let (Some(active), Some(event_value)) = (dialog.as_ref(), event) {
        if script_event_needs_dialog_confirmation(resources.text, active, event_value) {
            let active = dialog.as_mut().expect("dialog body was checked above");
            active.wait_after_reveal = true;
            active.awaiting_input = true;
            active.auto_wait_ticks = None;
            services.scripts.pending_script_event = event;
            return ScriptAdvanceFlow::StopForFrame;
        }
        if !matches!(event_value, ScriptEvent::Message { .. }) {
            *dialog = None;
        }
    }

    let mut flow = match event {
        Some(ScriptEvent::Action(_) | ScriptEvent::Condition(_) | ScriptEvent::Teleport { .. }) => {
            ScriptAdvanceFlow::ContinueImmediately
        }
        _ => ScriptAdvanceFlow::StopForFrame,
    };

    match event {
        Some(ScriptEvent::Message {
            message_id,
            position,
            font_color,
            face_index,
            playing_rng,
        }) => {
            let mut next = ActiveDialog::new(
                message_id,
                position,
                font_color,
                face_index,
                playing_rng,
                services.scripts.dialog_delay_ms,
            );
            if let Some(active) = dialog.as_mut() {
                if active.position == position
                    && active.font_color == font_color
                    && active.face_index == face_index
                    && active.playing_rng == playing_rng
                    && !active.awaiting_input
                {
                    active.message_ids.push(message_id);
                    active.wait_after_reveal =
                        dialog_body_lines(resources.text, active).len() >= (active.page + 1) * 4;
                } else if dialog_body_lines(resources.text, active).is_empty() {
                    *dialog = Some(next);
                } else {
                    active.awaiting_input = true;
                    next.wait_after_reveal |= dialog_body_lines(resources.text, &next).len() >= 4;
                    services.scripts.pending_dialog = Some(next);
                }
            } else {
                next.wait_after_reveal |= dialog_body_lines(resources.text, &next).len() >= 4;
                *dialog = Some(next);
            }
            set_title("Rust-PAL [Dialog]");
        }
        Some(ScriptEvent::Waiting {
            process_triggers,
            update_party_gestures,
        }) => {
            if update_party_gestures {
                game.stop_party_walking_animation();
            }
            if process_triggers {
                // A nested touch trigger must persist the callee's returned
                // entry before this waiting script resumes. The current call
                // stack does not expose that completion boundary yet.
                set_title("Rust-PAL [WaitFrames touch update unsupported]");
            }
            update_trigger_world(scripts, game, resources, load_scene, services, set_title);
            if scripts.debug_snapshot().wait_frames == 0 {
                flow = ScriptAdvanceFlow::ContinueImmediately;
            }
        }
        Some(ScriptEvent::Redraw {
            update_party_gestures: true,
        }) => {
            game.stop_party_walking_animation();
        }
        Some(ScriptEvent::Redraw { .. }) => {}
        Some(ScriptEvent::Delay) => {}
        Some(ScriptEvent::Confirm { no_entry }) => {
            services.set_confirmation_menu(ConfirmationMenu {
                no_entry,
                selected_yes: false,
            });
            set_title("Rust-PAL [Confirm]");
        }
        Some(ScriptEvent::OpenBuyMenu { store_number }) => {
            if game.store_items(store_number).is_none() {
                set_title("Rust-PAL [invalid store]");
                return ScriptAdvanceFlow::StopForFrame;
            }
            services.set_shop_menu(ShopMenu {
                mode: ShopMode::Buy { store_number },
                selected: 0,
                confirming: false,
                selected_yes: false,
            });
            set_title("Rust-PAL [Buy]");
        }
        Some(ScriptEvent::OpenSellMenu) => {
            services.set_shop_menu(ShopMenu {
                mode: ShopMode::Sell,
                selected: 0,
                confirming: false,
                selected_yes: false,
            });
            set_title("Rust-PAL [Sell]");
        }
        Some(ScriptEvent::StartBattle(request)) => {
            services.clear_active_menu();
            if game.start_battle(request, &services.scripts.auto_scripts) {
                services.battle.begin_battle();
                services.battle.battle_selected_enemy = game
                    .battle()
                    .and_then(|battle| battle.first_living_enemy())
                    .unwrap_or(0);
                services.battle.battle_command_selected = 0;
                services.battle.battle_targeting_enemy = false;
                services.battle.battle_menu = super::battle_render::BattleMenuState::Main;
                services.battle.battle_auto_attack = false;
                services.battle.battle_force_all = false;
                services.battle.battle_repeat_all = false;
                services.battle.post_battle = None;
                services.battle.battle_events.clear();
                services.battle.battle_event_ticks = 0;
                services.battle.battle_kept_effects.clear();
                services.battle.battle_effect_sound_count = 0;
                services.battle.battle_feedback_sound_played = false;
                services.battle.battle_debug_hit = None;
                services.battle.battle_debug_item_start = None;
                services.battle.battle_settlement_ticks = None;
                services.visual.queue_battle_transition();
                if game.current_battle_music == 0 {
                    services.audio.music.stop();
                } else if !services
                    .audio
                    .music
                    .play(game.current_battle_music, true, 0)
                {
                    set_title("Rust-PAL [battle music unavailable]");
                    return ScriptAdvanceFlow::StopForFrame;
                }
                set_title("Rust-PAL [Battle]");
            } else {
                set_title("Rust-PAL [failed to start battle]");
            }
        }
        Some(ScriptEvent::Teleport { failure_entry }) => {
            let teleport_entry = game.scene_teleport_script(0);
            if teleport_entry == 0 || !scripts.call(teleport_entry, 0xffff) {
                scripts.set_success(false);
                scripts.branch_to(failure_entry);
            }
        }
        Some(ScriptEvent::FadeScene { speed })
            if !services
                .visual
                .queue(ScriptVisual::FadeToCurrentScene { speed }) =>
        {
            set_title("Rust-PAL [visual effect is already active]");
        }
        Some(ScriptEvent::FadeScene { .. }) => {}
        Some(ScriptEvent::Visual(command)) if !services.visual.queue(command) => {
            set_title("Rust-PAL [visual effect is already active]");
        }
        Some(ScriptEvent::Visual(_)) => {}
        Some(ScriptEvent::WaitForKey) => services.scripts.waiting_for_key = true,
        Some(ScriptEvent::LoadLastSave) => services.persistence.load_last_save_requested = true,
        Some(ScriptEvent::QuitGame) => services.persistence.quit_requested = true,
        Some(ScriptEvent::Action(
            action @ pal_core::script::ScriptAction::SetPlayerPosition { .. },
        )) if !game.apply_script_action(action) => {
            set_title("Rust-PAL [script target is unavailable]");
            flow = ScriptAdvanceFlow::StopForFrame;
        }
        Some(ScriptEvent::Action(pal_core::script::ScriptAction::SetPlayerPosition { .. })) => {}
        Some(ScriptEvent::Action(pal_core::script::ScriptAction::ChangeScene { scene_number }))
            if super::session::PendingSceneChange::request(
                &mut services.scripts.pending_scene_change,
                &mut game.scene_number,
                scene_number,
            ) =>
        {
            game.set_party_layer(0);
            set_title(&format!("Rust-PAL [scene {scene_number}]"));
        }
        Some(ScriptEvent::Action(pal_core::script::ScriptAction::ChangeScene { .. })) => {}
        Some(ScriptEvent::Action(
            action @ pal_core::script::ScriptAction::SetSceneMap {
                scene_number,
                map_number: _,
            },
        )) => {
            let target_scene = scene_number.unwrap_or(game.scene_number);
            if !game.apply_script_action(action) {
                set_title("Rust-PAL [invalid scene map]");
                return ScriptAdvanceFlow::StopForFrame;
            }
            if services.scripts.pending_scene_change.is_none() && target_scene == game.scene_number
            {
                let Some(scene) = load_scene(
                    target_scene,
                    game.scene_map_override(target_scene),
                    resources.role_sprites,
                ) else {
                    set_title("Rust-PAL [failed to reload scene map]");
                    return ScriptAdvanceFlow::StopForFrame;
                };
                game.replace_map(scene.map);
            }
        }
        Some(ScriptEvent::Action(pal_core::script::ScriptAction::PlaySound { sound_id }))
            if !services.audio.sound_effects.play(sound_id) =>
        {
            set_title(&format!("Rust-PAL [invalid sound {sound_id}]"));
        }
        Some(ScriptEvent::Action(pal_core::script::ScriptAction::PlaySound { .. })) => {}
        Some(ScriptEvent::Action(pal_core::script::ScriptAction::PlayMusic {
            music_id,
            looped,
            fade_seconds,
        })) => {
            if services.audio.music.play(music_id, looped, fade_seconds) {
                game.apply_script_action(pal_core::script::ScriptAction::PlayMusic {
                    music_id,
                    looped,
                    fade_seconds,
                });
            } else {
                set_title(&format!("Rust-PAL [invalid music {music_id}]"));
            }
        }
        Some(ScriptEvent::Action(pal_core::script::ScriptAction::AdjustCash {
            amount,
            insufficient_entry,
        })) if !game.adjust_cash(amount) => {
            scripts.branch_to(insufficient_entry);
        }
        Some(ScriptEvent::Action(pal_core::script::ScriptAction::AdjustCash { .. })) => {}
        Some(ScriptEvent::Action(pal_core::script::ScriptAction::RemoveItem {
            item_id,
            amount,
            insufficient_entry,
        })) => {
            if let (false, 1..) = (
                game.remove_item(item_id, amount, insufficient_entry),
                insufficient_entry,
            ) {
                scripts.branch_to(insufficient_entry);
            }
        }
        Some(ScriptEvent::Action(
            action @ (pal_core::script::ScriptAction::AdjustPlayerHealth { .. }
            | pal_core::script::ScriptAction::RevivePlayer { .. }
            | pal_core::script::ScriptAction::SetPlayerStatus { .. }),
        )) => {
            let succeeded = game.apply_script_action(action);
            scripts.set_success(succeeded);
        }
        Some(ScriptEvent::Action(
            action @ pal_core::script::ScriptAction::SetEnemyStatus { resisted_entry, .. },
        )) if !game.apply_script_action(action) => {
            scripts.branch_to(resisted_entry);
        }
        Some(ScriptEvent::Action(
            action @ pal_core::script::ScriptAction::FleeBattle { failure_entry },
        )) if !game.apply_script_action(action) => {
            scripts.branch_to(failure_entry);
        }
        Some(ScriptEvent::Action(
            action @ pal_core::script::ScriptAction::DivideEnemy { failure_entry, .. },
        )) => {
            flow = ScriptAdvanceFlow::StopForFrame;
            if game.apply_script_action(action) {
                let events = game.advance_battle_resolution();
                queue_battle_events(game, services, events);
            } else if failure_entry != 0 {
                scripts.branch_to(failure_entry);
            }
        }
        Some(ScriptEvent::Action(
            action @ pal_core::script::ScriptAction::CollectEnemy { failure_entry, .. },
        )) if !game.apply_script_action(action) => {
            scripts.branch_to(failure_entry);
        }
        Some(ScriptEvent::Action(
            action @ pal_core::script::ScriptAction::SummonEnemy { failure_entry, .. },
        )) => {
            flow = ScriptAdvanceFlow::StopForFrame;
            let succeeded = game.apply_script_action(action);
            let events = game.advance_battle_resolution();
            if !events.is_empty() {
                queue_battle_events(game, services, events);
            }
            if !succeeded && failure_entry != 0 {
                scripts.branch_to(failure_entry);
            }
        }
        Some(ScriptEvent::Action(pal_core::script::ScriptAction::TransformEnemy {
            enemy_index,
            object_id,
        })) => {
            flow = ScriptAdvanceFlow::StopForFrame;
            match game.transform_enemy(enemy_index, object_id) {
                Some(true) => {
                    let events = game.advance_battle_resolution();
                    queue_battle_events(game, services, events);
                }
                Some(false) => {}
                None => set_title("Rust-PAL [script transform target is unavailable]"),
            }
        }
        Some(ScriptEvent::Action(
            pal_core::script::ScriptAction::SetEnemyStatus { .. }
            | pal_core::script::ScriptAction::FleeBattle { .. }
            | pal_core::script::ScriptAction::CollectEnemy { .. },
        )) => {}
        Some(ScriptEvent::Action(action @ pal_core::script::ScriptAction::EnemyEscape))
            if game.apply_script_action(action) =>
        {
            flow = ScriptAdvanceFlow::StopForFrame;
            let events = game.advance_battle_resolution();
            queue_battle_events(game, services, events);
        }
        Some(ScriptEvent::Action(pal_core::script::ScriptAction::EnemyEscape)) => {
            flow = ScriptAdvanceFlow::StopForFrame;
        }
        Some(ScriptEvent::Action(
            action @ pal_core::script::ScriptAction::PlaceObjectInFront { blocked_entry, .. },
        )) if !game.apply_script_action(action) => {
            scripts.set_success(false);
            scripts.branch_to(blocked_entry);
        }
        Some(ScriptEvent::Action(pal_core::script::ScriptAction::PlaceObjectInFront {
            ..
        })) => {}
        Some(ScriptEvent::Action(
            action @ pal_core::script::ScriptAction::CheckObjectZone { failure_entry, .. },
        )) if !game.apply_script_action(action) => {
            scripts.set_success(false);
            scripts.branch_to(failure_entry);
        }
        Some(ScriptEvent::Action(pal_core::script::ScriptAction::CheckObjectZone { .. })) => {}
        Some(ScriptEvent::Action(pal_core::script::ScriptAction::WalkObjectTo {
            object_id,
            tile_x,
            tile_y,
            half,
            speed,
            repeat_entry,
        })) => match game.walk_object_to(object_id, tile_x, tile_y, half, speed) {
            Some(true) => {}
            Some(false) => {
                scripts.branch_to(repeat_entry);
                flow = ScriptAdvanceFlow::StopForFrame;
            }
            None => {
                set_title("Rust-PAL [script walk target is unavailable]");
                flow = ScriptAdvanceFlow::StopForFrame;
            }
        },
        Some(ScriptEvent::Action(pal_core::script::ScriptAction::WalkPlayerTo {
            tile_x,
            tile_y,
            half,
            speed,
            repeat_entry,
        })) => {
            let before = (game.player.world_x, game.player.world_y);
            let mut completed = false;
            match game.walk_player_to(tile_x, tile_y, half, speed) {
                Some(true) => {
                    game.stop_party_walking_animation();
                    completed = true;
                }
                Some(false) => {
                    scripts.branch_to(repeat_entry);
                    scripts.delay_next_advance();
                }
                None => set_title("Rust-PAL [script party walk target is unavailable]"),
            }
            if before != (game.player.world_x, game.player.world_y) {
                update_trigger_world(scripts, game, resources, load_scene, services, set_title);
            }
            if !completed {
                flow = ScriptAdvanceFlow::StopForFrame;
            }
        }
        Some(ScriptEvent::Action(pal_core::script::ScriptAction::RideObjectTo {
            object_id,
            tile_x,
            tile_y,
            half,
            speed,
            repeat_entry,
        })) => {
            let before = (game.player.world_x, game.player.world_y);
            let mut completed = false;
            match game.ride_object_to(object_id, tile_x, tile_y, half, speed) {
                Some(true) => completed = true,
                Some(false) => {
                    scripts.branch_to(repeat_entry);
                    scripts.delay_next_advance();
                }
                None => set_title("Rust-PAL [script ride target is unavailable]"),
            }
            if before != (game.player.world_x, game.player.world_y) {
                update_trigger_world(scripts, game, resources, load_scene, services, set_title);
            }
            if !completed {
                flow = ScriptAdvanceFlow::StopForFrame;
            }
        }
        Some(ScriptEvent::Action(
            action @ pal_core::script::ScriptAction::MoveViewport { x, y, frames },
        )) => {
            game.apply_script_action(action);
            if (x != 0 || y != 0) && frames != -1 {
                update_trigger_world(scripts, game, resources, load_scene, services, set_title);
                scripts.delay_next_advance();
                flow = ScriptAdvanceFlow::StopForFrame;
            }
        }
        Some(ScriptEvent::Condition(condition)) => {
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
                    game.player_poisons(role_id)
                        .is_none_or(|poisons| poisons.iter().all(|poison| poison.object_id == 0)),
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
                ScriptCondition::EnemyTurn { target_entry } => (game.is_enemy_turn(), target_entry),
            };
            if matches {
                scripts.branch_to(target_entry);
            }
        }
        Some(ScriptEvent::Action(
            action @ pal_core::script::ScriptAction::PlayerMagicAnimation { .. },
        )) => {
            flow = ScriptAdvanceFlow::StopForFrame;
            if !game.apply_script_action(action) {
                set_title("Rust-PAL [script target is unavailable]");
                return ScriptAdvanceFlow::StopForFrame;
            }
            let events = game.advance_battle_resolution();
            queue_battle_events(game, services, events);
        }
        Some(ScriptEvent::Action(pal_core::script::ScriptAction::StealEnemy {
            enemy_index,
            rate,
        })) => {
            let Some(stolen) = game.steal_enemy(enemy_index, rate) else {
                set_title("Rust-PAL [script target is unavailable]");
                return ScriptAdvanceFlow::StopForFrame;
            };
            if let Some(text) = steal_result_text(resources.text, stolen) {
                *dialog = Some(ActiveDialog::center_window_text(text));
            }
            flow = ScriptAdvanceFlow::StopForFrame;
        }
        Some(ScriptEvent::Action(
            action @ (pal_core::script::ScriptAction::SimulatePlayerMagic { .. }
            | pal_core::script::ScriptAction::ThrowWeapon { .. }),
        )) => {
            flow = ScriptAdvanceFlow::StopForFrame;
            if !game.apply_script_action(action) {
                set_title("Rust-PAL [script target is unavailable]");
                return ScriptAdvanceFlow::StopForFrame;
            }
            let events = game.advance_battle_resolution();
            queue_battle_events(game, services, events);
        }
        Some(ScriptEvent::Action(action @ pal_core::script::ScriptAction::SetParty { .. })) => {
            if !game.apply_script_action(action) {
                set_title("Rust-PAL [script party is unavailable]");
            } else if !game.refresh_equipment_effects(&services.scripts.auto_scripts) {
                set_title("Rust-PAL [failed to refresh party equipment effects]");
            }
        }
        Some(ScriptEvent::Action(action @ ScriptAction::OffsetPlayer { .. }))
            if !game.apply_script_action(action) =>
        {
            set_title("Rust-PAL [script target is unavailable]");
            flow = ScriptAdvanceFlow::StopForFrame;
        }
        Some(ScriptEvent::Action(ScriptAction::OffsetPlayer { .. })) => {}
        Some(ScriptEvent::Action(action)) if !game.apply_script_action(action) => {
            set_title("Rust-PAL [script target is unavailable]");
            flow = ScriptAdvanceFlow::StopForFrame;
        }
        Some(ScriptEvent::Action(_)) => {}
        Some(ScriptEvent::Completed {
            trigger,
            next_entry,
            succeeded,
        }) => {
            if trigger.kind == TriggerKind::Battle {
                if !game.finish_battle_script(next_entry, succeeded) {
                    set_title("Rust-PAL [battle script completion failed]");
                }
                return ScriptAdvanceFlow::StopForFrame;
            } else if trigger.kind == TriggerKind::Item {
                if let Some(item_use) = services.menus.item_use.take() {
                    game.finish_item_use(item_use.item_id, next_entry, succeeded);
                    let selected = item_use
                        .inventory_selected
                        .min(game.inventory().len().saturating_sub(1));
                    services.menus.inventory_selected = selected;
                    if item_use.apply_to_all {
                        services.clear_active_menu();
                        set_title("Rust-PAL");
                    } else if game.usable_item(item_use.item_id).is_some() {
                        let target_selected = game
                            .party
                            .members()
                            .iter()
                            .position(|member| member.role_id == trigger.object_id)
                            .unwrap_or(0);
                        services.menus.item_target_selected = target_selected;
                        services.set_inventory_menu(InventoryMenu {
                            selected,
                            mode: InventoryMode::Target {
                                item_id: item_use.item_id,
                                selected: target_selected,
                            },
                        });
                        set_title("Rust-PAL [Item target]");
                    } else {
                        services.set_inventory_menu(InventoryMenu {
                            selected,
                            mode: InventoryMode::Items,
                        });
                        set_title("Rust-PAL [Use item]");
                    }
                }
                return ScriptAdvanceFlow::StopForFrame;
            } else if trigger.kind == TriggerKind::Equip {
                if let Some(equip) = services.menus.equip.take() {
                    game.finish_item_equip(equip.item_id, next_entry);
                    let selected = equip
                        .inventory_selected
                        .min(game.equippable_inventory().len().saturating_sub(1));
                    services.menus.inventory_selected = selected;
                    services.menus.item_target_selected = equip.role_selected;
                    services.set_inventory_menu(InventoryMenu {
                        selected,
                        mode: InventoryMode::EquipItems,
                    });
                    set_title("Rust-PAL [Equip item]");
                }
                return ScriptAdvanceFlow::StopForFrame;
            } else if trigger.kind == TriggerKind::Magic {
                if let Some(mut magic) = services.menus.magic.take() {
                    game.finish_magic_script(magic.magic_id, next_entry, magic.success_phase);
                    let caster_role = game.party.members()[magic.caster_selected].role_id;
                    if succeeded && !magic.success_phase {
                        let target_role = magic
                            .target_selected
                            .map(|selected| game.party.members()[selected].role_id);
                        if let Some(request) =
                            game.magic_request(caster_role, magic.magic_id, target_role, true)
                        {
                            magic.success_phase = true;
                            services.menus.magic = Some(magic);
                            scripts.start(request);
                            set_title("Rust-PAL [Casting]");
                            return ScriptAdvanceFlow::StopForFrame;
                        }
                    }
                    if succeeded {
                        game.consume_magic_mp(caster_role, magic.magic_id);
                    }
                    let available = game
                        .field_magics(caster_role)
                        .into_iter()
                        .any(|field_magic| {
                            field_magic.magic_id == magic.magic_id && field_magic.enabled
                        });
                    if available {
                        if let Some(selected) = magic.target_selected {
                            services.set_field_menu(FieldMenu::MagicTarget {
                                caster: magic.caster_selected,
                                magic_id: magic.magic_id,
                                selected,
                            });
                            set_title("Rust-PAL [Magic target]");
                        } else {
                            services.set_field_menu(FieldMenu::MagicList {
                                caster: magic.caster_selected,
                                selected: services.menus.magic_selected,
                            });
                            set_title("Rust-PAL [Magic list]");
                        }
                    } else {
                        set_title("Rust-PAL");
                    }
                }
                return ScriptAdvanceFlow::StopForFrame;
            } else if trigger.kind == TriggerKind::Auto {
                // The owning event object already advanced past CALL before
                // this nested trigger runtime started.
            } else if trigger.object_id == 0xffff {
                let completed_scene = services
                    .scripts
                    .pending_scene_change
                    .map_or(game.scene_number, |change| change.source_scene());
                game.update_scene_enter_script_for(completed_scene, next_entry);
            } else if let Some(object) = game
                .scene_objects
                .iter_mut()
                .find(|object| object.id == trigger.object_id)
            {
                object.trigger_script = next_entry;
            }
            if !finish_pending_scene_change(
                scripts,
                game,
                resources.role_sprites,
                load_scene,
                services,
                set_title,
            ) {
                set_title("Rust-PAL");
            }
            if let Some(active) = dialog.as_mut() {
                active.awaiting_input = true;
            }
        }
        Some(ScriptEvent::Unsupported {
            trigger,
            entry,
            opcode,
        }) => {
            cancel_pending_scene_change(game, services);
            if trigger.kind == TriggerKind::Battle {
                game.finish_battle_script(trigger.script_entry, false);
            }
            resume_script_menu_after_error(game, services, trigger.kind);
            set_title(&format!(
                "Rust-PAL [unsupported script {entry} {}]",
                opcode_label(opcode)
            ));
        }
        Some(ScriptEvent::InvalidEntry { trigger, entry }) => {
            cancel_pending_scene_change(game, services);
            if trigger.kind == TriggerKind::Battle {
                game.finish_battle_script(trigger.script_entry, false);
            }
            resume_script_menu_after_error(game, services, trigger.kind);
            set_title(&format!("Rust-PAL [invalid script entry {entry}]"));
        }
        Some(ScriptEvent::InstructionLimit { trigger, entry }) => {
            cancel_pending_scene_change(game, services);
            if trigger.kind == TriggerKind::Battle {
                game.finish_battle_script(trigger.script_entry, false);
            }
            resume_script_menu_after_error(game, services, trigger.kind);
            set_title(&format!("Rust-PAL [script loop at {entry}]"));
        }
        None => {}
    }
    flow
}

/// Build the same centered theft result used by Classic's `PAL_BattleStealFromEnemy`.
fn steal_result_text(text: &TextLibrary, stolen: BattleSteal) -> Option<Vec<u8>> {
    let obtained = text.word(34)?;
    match stolen {
        BattleSteal::Nothing => None,
        BattleSteal::Cash(amount) if amount != 0 => {
            let cash_unit = text.word(10)?;
            let mut message = Vec::new();
            message.push(b'@');
            message.extend_from_slice(obtained);
            message.extend_from_slice(b" @");
            message.extend_from_slice(amount.to_string().as_bytes());
            message.extend_from_slice(b" @");
            message.extend_from_slice(cash_unit);
            message.push(b'@');
            Some(message)
        }
        BattleSteal::Item(item_id) => {
            let item = text.word(usize::from(item_id))?;
            let mut message = Vec::with_capacity(obtained.len() + item.len() + 2);
            message.extend_from_slice(obtained);
            message.push(b'@');
            message.extend_from_slice(item);
            message.push(b'@');
            Some(message)
        }
        BattleSteal::Cash(_) => None,
    }
}

fn finish_pending_scene_change<L>(
    scripts: &mut ScriptRuntime,
    game: &mut GameState,
    role_sprites: &RoleSprites,
    load_scene: &mut L,
    services: &mut DesktopSession,
    set_title: &mut impl FnMut(&str),
) -> bool
where
    L: FnMut(u16, Option<u16>, &RoleSprites) -> Option<LoadedScene>,
{
    let Some(change) = services.scripts.pending_scene_change.take() else {
        return false;
    };
    let target_scene = change.target_scene();
    let Some(scene) = load_scene(
        target_scene,
        game.scene_map_override(target_scene),
        role_sprites,
    ) else {
        game.scene_number = change.source_scene();
        set_title("Rust-PAL [failed to load scene]");
        return true;
    };
    game.replace_scene(scene.number, scene.map, scene.objects);
    let enter_script = game.scene_enter_script(scene.enter_script);
    if enter_script != 0 {
        scripts.start(pal_core::scene::TriggerRequest {
            object_id: 0xffff,
            script_entry: enter_script,
            kind: TriggerKind::Touch,
        });
    }
    set_title(&format!("Rust-PAL [scene {}]", scene.number));
    true
}

fn cancel_pending_scene_change(game: &mut GameState, services: &mut DesktopSession) {
    if let Some(change) = services.scripts.pending_scene_change.take() {
        game.scene_number = change.source_scene();
    }
}

pub(super) fn auto_script_error_title(error: AutoScriptError) -> String {
    match error {
        AutoScriptError::InvalidEntry { object_id, entry } => {
            format!("Rust-PAL [object {object_id} invalid auto script {entry}]")
        }
        AutoScriptError::MissingObject {
            object_id,
            entry,
            target_id,
        } => {
            format!("Rust-PAL [object {object_id} auto script {entry} missing object {target_id}]")
        }
        AutoScriptError::Unsupported {
            object_id,
            entry,
            opcode,
        } => format!(
            "Rust-PAL [object {object_id} auto script {entry} {}]",
            opcode_label(opcode)
        ),
        AutoScriptError::HostRequired {
            object_id,
            entry,
            opcode,
        } => format!(
            "Rust-PAL [object {object_id} auto script {entry} {} requires desktop host]",
            opcode_label(opcode)
        ),
        AutoScriptError::InstructionLimit { object_id, entry } => {
            format!("Rust-PAL [object {object_id} auto script loop at {entry}]")
        }
    }
}

pub(super) fn opcode_label(raw: u16) -> String {
    ScriptOpcode::from_raw(raw).map_or_else(
        || format!("opcode {raw:04X}"),
        |opcode| format!("{} ({raw:04X})", opcode.name()),
    )
}

fn resume_inventory_after_item_error(game: &GameState, services: &mut DesktopSession) {
    let Some(item_use) = services.menus.item_use.take() else {
        return;
    };
    let selected = item_use
        .inventory_selected
        .min(game.inventory().len().saturating_sub(1));
    services.menus.inventory_selected = selected;
    services.set_inventory_menu(InventoryMenu {
        selected,
        mode: InventoryMode::Items,
    });
}

pub(super) fn resume_script_menu_after_error(
    game: &GameState,
    services: &mut DesktopSession,
    kind: TriggerKind,
) {
    match kind {
        TriggerKind::Item => resume_inventory_after_item_error(game, services),
        TriggerKind::Equip => {
            if let Some(equip) = services.menus.equip.take() {
                services.set_inventory_menu(InventoryMenu {
                    selected: equip
                        .inventory_selected
                        .min(game.equippable_inventory().len().saturating_sub(1)),
                    mode: InventoryMode::EquipItems,
                });
            }
        }
        TriggerKind::Magic => {
            if let Some(magic) = services.menus.magic.take() {
                services.set_field_menu(FieldMenu::MagicList {
                    caster: magic.caster_selected,
                    selected: services.menus.magic_selected,
                });
            }
        }
        TriggerKind::Search | TriggerKind::Touch | TriggerKind::Auto | TriggerKind::Battle => {}
    }
}

pub(super) fn update_trigger_world<L>(
    scripts: &mut ScriptRuntime,
    game: &mut GameState,
    resources: ScriptRenderResources<'_>,
    load_scene: &mut L,
    services: &mut DesktopSession,
    set_title: &mut impl FnMut(&str),
) where
    L: FnMut(u16, Option<u16>, &RoleSprites) -> Option<LoadedScene>,
{
    let update = game.update_auto_scripts_report(&services.scripts.auto_scripts);
    if let Some(error) = update.error {
        set_title(&auto_script_error_title(error));
    }
    if game.take_auto_script_failure() {
        let _ = scripts.set_success(false);
    }
    for sound_id in game.take_auto_script_sounds() {
        if !services.audio.sound_effects.play(sound_id) {
            set_title(&format!("Rust-PAL [invalid auto sound {sound_id}]"));
        }
    }
    let _ = apply_auto_script_events(
        game.take_auto_script_events(),
        scripts,
        game,
        resources.role_sprites,
        load_scene,
        services,
        set_title,
    );
    if let Some(trigger) = game.take_trigger() {
        if !enter_auto_trigger(scripts, trigger) {
            set_title("Rust-PAL [auto CALL runtime is unavailable]");
        }
    }
}

pub(super) fn enter_auto_trigger(scripts: &mut ScriptRuntime, trigger: TriggerRequest) -> bool {
    if trigger.kind != TriggerKind::Auto {
        return false;
    }
    if scripts.is_active() {
        scripts.call(trigger.script_entry, trigger.object_id)
    } else {
        scripts.start(trigger)
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn apply_auto_script_events<L>(
    events: Vec<ScriptEvent>,
    scripts: &mut ScriptRuntime,
    game: &mut GameState,
    role_sprites: &RoleSprites,
    load_scene: &mut L,
    services: &mut DesktopSession,
    set_title: &mut impl FnMut(&str),
) -> bool
where
    L: FnMut(u16, Option<u16>, &RoleSprites) -> Option<LoadedScene>,
{
    let mut changed = false;
    for event in events {
        changed = true;
        match event {
            ScriptEvent::Visual(command) => {
                if !services.visual.queue(command) {
                    set_title("Rust-PAL [auto visual effect is already active]");
                }
            }
            ScriptEvent::OpenBuyMenu { store_number } => {
                if game.store_items(store_number).is_none() {
                    set_title("Rust-PAL [invalid auto store]");
                    continue;
                }
                services.set_shop_menu(ShopMenu {
                    mode: ShopMode::Buy { store_number },
                    selected: 0,
                    confirming: false,
                    selected_yes: false,
                });
                set_title("Rust-PAL [Buy]");
            }
            ScriptEvent::OpenSellMenu => {
                services.set_shop_menu(ShopMenu {
                    mode: ShopMode::Sell,
                    selected: 0,
                    confirming: false,
                    selected_yes: false,
                });
                set_title("Rust-PAL [Sell]");
            }
            ScriptEvent::WaitForKey => services.scripts.waiting_for_key = true,
            ScriptEvent::LoadLastSave => services.persistence.load_last_save_requested = true,
            ScriptEvent::QuitGame => services.persistence.quit_requested = true,
            ScriptEvent::Action(pal_core::script::ScriptAction::PlayMusic {
                music_id,
                looped,
                fade_seconds,
            }) => {
                if !services.audio.music.play(music_id, looped, fade_seconds) {
                    set_title(&format!("Rust-PAL [invalid auto music {music_id}]"));
                }
            }
            ScriptEvent::Action(pal_core::script::ScriptAction::ChangeScene { scene_number }) => {
                if super::session::PendingSceneChange::request(
                    &mut services.scripts.pending_scene_change,
                    &mut game.scene_number,
                    scene_number,
                ) {
                    let _ = finish_pending_scene_change(
                        scripts,
                        game,
                        role_sprites,
                        load_scene,
                        services,
                        set_title,
                    );
                }
            }
            ScriptEvent::Action(
                action @ pal_core::script::ScriptAction::SetSceneMap {
                    scene_number,
                    map_number: _,
                },
            ) => {
                let target_scene = scene_number.unwrap_or(game.scene_number);
                if !game.apply_script_action(action) {
                    set_title("Rust-PAL [invalid auto scene map]");
                    continue;
                }
                if target_scene == game.scene_number {
                    let Some(scene) = load_scene(
                        target_scene,
                        game.scene_map_override(target_scene),
                        role_sprites,
                    ) else {
                        set_title("Rust-PAL [failed to reload auto scene map]");
                        continue;
                    };
                    game.replace_map(scene.map);
                }
            }
            unexpected => set_title(&format!("Rust-PAL [unexpected auto event {unexpected:?}]")),
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::MusicBackend;
    use pal_assets::battle::BattleSpriteArchive;
    use pal_assets::script::ScriptTable;
    use pal_core::map::{Map, MAP_COLUMNS, MAP_HALVES, MAP_ROWS};
    use pal_core::role::{Direction, Role};
    use pal_core::scene::SceneObject;

    use super::super::session::MusicResources;

    fn text_library(messages: &[&[u8]]) -> TextLibrary {
        let word_data = [b' '; 10];
        let mut message_data = Vec::new();
        let mut message_index = 0u32.to_le_bytes().to_vec();
        for message in messages {
            message_data.extend_from_slice(message);
            message_index.extend_from_slice(&(message_data.len() as u32).to_le_bytes());
        }
        TextLibrary::parse(&word_data, &message_data, &message_index).unwrap()
    }

    fn theft_text_library() -> TextLibrary {
        let mut words = vec![b' '; 35 * 10];
        for (index, value) in [(10, b"coin".as_slice()), (12, b"item"), (34, b"got")] {
            words[index * 10..index * 10 + value.len()].copy_from_slice(value);
        }
        TextLibrary::parse(&words, b"x", &[0, 0, 0, 0, 1, 0, 0, 0]).unwrap()
    }

    fn mkf(chunks: &[Vec<u8>]) -> Vec<u8> {
        let table_size = (chunks.len() + 1) * 4;
        let mut offset = table_size as u32;
        let mut data = offset.to_le_bytes().to_vec();
        for chunk in chunks {
            offset += chunk.len() as u32;
            data.extend_from_slice(&offset.to_le_bytes());
        }
        for chunk in chunks {
            data.extend_from_slice(chunk);
        }
        data
    }

    fn raw_yj1(data: &[u8]) -> Vec<u8> {
        let blocks = data.chunks(usize::from(u16::MAX)).collect::<Vec<_>>();
        let compressed_len = 16 + blocks.iter().map(|block| 4 + block.len()).sum::<usize>();
        let mut result = Vec::new();
        result.extend_from_slice(b"YJ_1");
        result.extend_from_slice(&(data.len() as u32).to_le_bytes());
        result.extend_from_slice(&(compressed_len as u32).to_le_bytes());
        result.extend_from_slice(&(blocks.len() as u16).to_le_bytes());
        result.extend_from_slice(&[0, 0]);
        for block in blocks {
            result.extend_from_slice(&(block.len() as u16).to_le_bytes());
            result.extend_from_slice(&0u16.to_le_bytes());
            result.extend_from_slice(block);
        }
        result
    }

    fn one_pixel_sprite() -> Vec<u8> {
        let mut sprite = Vec::new();
        sprite.extend_from_slice(&2u16.to_le_bytes());
        sprite.extend_from_slice(&0u16.to_le_bytes());
        sprite.extend_from_slice(&[1, 0, 1, 0, 1, 2]);
        sprite
    }

    fn role_sprites() -> RoleSprites {
        RoleSprites::load(&mkf(&[raw_yj1(&one_pixel_sprite())])).unwrap()
    }

    fn game_with_objects(object_count: u16) -> GameState {
        let map_data = vec![0; MAP_ROWS * MAP_COLUMNS * MAP_HALVES * 4];
        let map = Map::load(
            1,
            &mkf(&[Vec::new(), raw_yj1(&map_data)]),
            &mkf(&[Vec::new(), one_pixel_sprite()]),
        )
        .unwrap();
        let mut game = GameState::new(
            map,
            Role {
                sprite_index: 0,
                world_x: 0,
                world_y: 0,
                direction: Direction::South,
                anim_frame: 0,
                frames_per_direction: 1,
            },
            320,
            200,
        );
        game.scene_objects = (1..=object_count)
            .map(|id| SceneObject {
                id,
                world_x: 0,
                world_y: 0,
                layer: 0,
                trigger_script: 0,
                auto_script: 0,
                state: 1,
                trigger_mode: 0,
                sprite_index: None,
                frames_per_direction: 0,
                sprite_frame_count: 0,
                direction: Direction::South,
                current_frame: 0,
                vanish_time: 0,
                auto_script_idle_frame: 0,
            })
            .collect();
        game
    }

    fn desktop_session(auto_scripts: ScriptTable) -> DesktopSession {
        let sprite_mkf = mkf(&[one_pixel_sprite()]);
        let sprites = BattleSpriteArchive::load(&sprite_mkf).unwrap();
        let empty_mkf = mkf(&[Vec::new()]);
        DesktopSession::new(
            auto_scripts,
            &empty_mkf,
            MusicResources {
                rix_mkf: &empty_mkf,
                midi_mkf: &[],
                sound_font: &[],
                requested_backend: MusicBackend::Rix,
            },
            &sprites,
            &sprites,
            &sprites,
        )
    }

    fn script_table(entries: &[[u16; 4]]) -> ScriptTable {
        let data = entries
            .iter()
            .flat_map(|entry| entry.iter().copied().flat_map(u16::to_le_bytes))
            .collect::<Vec<_>>();
        ScriptTable::parse(&data).unwrap()
    }

    fn ignore_title(_: &str) {}

    #[test]
    fn non_message_events_wait_for_body_confirmation_but_not_title_only_dialogs() {
        let text = text_library(&[b"Name:", b"body"]);
        let title = ActiveDialog::new(
            0,
            pal_core::script::DialogPosition::Upper,
            0x4f,
            None,
            false,
            24,
        );
        let mut body = title.clone();
        body.message_ids.push(1);
        let delay = ScriptEvent::Delay;

        assert!(!script_event_needs_dialog_confirmation(
            &text, &title, delay
        ));
        assert!(script_event_needs_dialog_confirmation(&text, &body, delay));
        assert!(!script_event_needs_dialog_confirmation(
            &text,
            &body,
            ScriptEvent::Message {
                message_id: 1,
                position: pal_core::script::DialogPosition::Upper,
                font_color: 0x4f,
                face_index: None,
                playing_rng: false,
            }
        ));
    }

    #[test]
    fn successful_theft_uses_the_classic_center_window_text_and_timeout() {
        let text = theft_text_library();
        assert_eq!(
            steal_result_text(&text, BattleSteal::Item(12)),
            Some(b"got@item@".to_vec())
        );
        assert_eq!(
            steal_result_text(&text, BattleSteal::Cash(7)),
            Some(b"@got @7 @coin@".to_vec())
        );
        assert_eq!(steal_result_text(&text, BattleSteal::Nothing), None);

        let dialog = ActiveDialog::center_window_text(b"got@item@".to_vec());
        assert_eq!(dialog.auto_wait_ticks, Some(28));
        assert_eq!(
            dialog_body_lines(&text, &dialog)[0].as_ref(),
            b"got@item@".as_slice()
        );
    }

    #[test]
    fn instantaneous_post_battle_actions_reach_fade_in_one_host_frame() {
        let table = script_table(&[
            [ScriptOpcode::Stop.raw(), 0, 0, 0],
            [ScriptOpcode::SetObjectState.raw(), 1, 0, 0],
            [ScriptOpcode::SetObjectState.raw(), 2, 2, 0],
            [ScriptOpcode::SetObjectState.raw(), 3, 0, 0],
            [ScriptOpcode::SetObjectState.raw(), 4, 2, 0],
            [ScriptOpcode::SetObjectState.raw(), 5, 0, 0],
            [ScriptOpcode::FadeOut.raw(), 0, 0, 0],
            [ScriptOpcode::Stop.raw(), 0, 0, 0],
        ]);
        let mut scripts = ScriptRuntime::new(table.clone());
        assert!(scripts.start(TriggerRequest {
            object_id: u16::MAX,
            script_entry: 1,
            kind: TriggerKind::Touch,
        }));
        let mut game = game_with_objects(5);
        let sprites = role_sprites();
        let text = text_library(&[b"x"]);
        let mut services = desktop_session(table);
        let mut dialog = None;
        let mut load_scene = |_, _, _: &RoleSprites| None;
        let mut title = String::new();

        advance_script(
            &mut scripts,
            &mut game,
            &mut dialog,
            ScriptRenderResources {
                text: &text,
                role_sprites: &sprites,
            },
            &mut load_scene,
            &mut services,
            &mut |value| title = value.to_owned(),
        );

        assert_eq!(
            (1..=5)
                .map(|object_id| game.object_state(object_id).unwrap())
                .collect::<Vec<_>>(),
            [0, 2, 0, 2, 0]
        );
        assert!(services.visual.is_blocking());
        assert_eq!(
            scripts.debug_snapshot().last_instruction.unwrap().opcode,
            ScriptOpcode::FadeOut.raw()
        );
        assert_eq!(
            scripts.debug_snapshot().next_instruction.unwrap().opcode,
            ScriptOpcode::Stop.raw()
        );
        assert!(title.is_empty());
    }

    #[test]
    fn explicit_redraw_still_stops_before_a_following_fade() {
        let table = script_table(&[
            [ScriptOpcode::Stop.raw(), 0, 0, 0],
            [ScriptOpcode::SetObjectState.raw(), 1, 2, 0],
            [ScriptOpcode::Redraw.raw(), 0, 0, 0],
            [ScriptOpcode::FadeOut.raw(), 0, 0, 0],
            [ScriptOpcode::Stop.raw(), 0, 0, 0],
        ]);
        let mut scripts = ScriptRuntime::new(table.clone());
        assert!(scripts.start(TriggerRequest {
            object_id: u16::MAX,
            script_entry: 1,
            kind: TriggerKind::Touch,
        }));
        let mut game = game_with_objects(1);
        let sprites = role_sprites();
        let text = text_library(&[b"x"]);
        let mut services = desktop_session(table);
        let mut dialog = None;
        let mut load_scene = |_, _, _: &RoleSprites| None;
        let mut set_title = ignore_title;

        advance_script(
            &mut scripts,
            &mut game,
            &mut dialog,
            ScriptRenderResources {
                text: &text,
                role_sprites: &sprites,
            },
            &mut load_scene,
            &mut services,
            &mut set_title,
        );

        assert_eq!(game.object_state(1), Some(2));
        assert!(!services.visual.is_blocking());
        assert_eq!(
            scripts.debug_snapshot().last_instruction.unwrap().opcode,
            ScriptOpcode::Redraw.raw()
        );

        advance_script(
            &mut scripts,
            &mut game,
            &mut dialog,
            ScriptRenderResources {
                text: &text,
                role_sprites: &sprites,
            },
            &mut load_scene,
            &mut services,
            &mut set_title,
        );
        assert!(!services.visual.is_blocking());

        advance_script(
            &mut scripts,
            &mut game,
            &mut dialog,
            ScriptRenderResources {
                text: &text,
                role_sprites: &sprites,
            },
            &mut load_scene,
            &mut services,
            &mut set_title,
        );
        assert!(services.visual.is_blocking());
        assert_eq!(
            scripts.debug_snapshot().last_instruction.unwrap().opcode,
            ScriptOpcode::FadeOut.raw()
        );
    }

    #[test]
    fn auto_call_nests_inside_a_waiting_trigger_and_restores_its_wait() {
        let data = [
            [0x0000u16, 0, 0, 0],
            [ScriptOpcode::WaitFrames.raw(), 3, 1, 1],
            [0x0000, 0, 0, 0],
            [ScriptOpcode::SetObjectGesture.raw(), 7, 0, 0],
            [0x0000, 0, 0, 0],
        ]
        .into_iter()
        .flat_map(|entry| entry.into_iter().flat_map(u16::to_le_bytes))
        .collect::<Vec<_>>();
        let mut scripts = ScriptRuntime::new(ScriptTable::parse(&data).unwrap());
        let parent = TriggerRequest {
            object_id: 9,
            script_entry: 1,
            kind: TriggerKind::Touch,
        };
        assert!(scripts.start(parent));
        assert_eq!(scripts.advance(), Some(ScriptEvent::Delay));

        assert!(enter_auto_trigger(
            &mut scripts,
            TriggerRequest {
                object_id: 7,
                script_entry: 3,
                kind: TriggerKind::Auto,
            }
        ));
        assert_eq!(
            scripts.advance(),
            Some(ScriptEvent::Action(
                pal_core::script::ScriptAction::SetObjectPose {
                    object_id: 7,
                    direction: Some(pal_core::role::Direction::South),
                    frame: Some(7),
                }
            ))
        );
        let scene_frame = ScriptEvent::Waiting {
            process_triggers: true,
            update_party_gestures: true,
        };
        assert_eq!(scripts.advance(), Some(ScriptEvent::Delay));
        assert_eq!(scripts.advance(), Some(scene_frame));
        assert_eq!(scripts.advance(), Some(ScriptEvent::Delay));
        assert_eq!(scripts.advance(), Some(scene_frame));
        assert_eq!(scripts.advance(), Some(ScriptEvent::Delay));
        assert_eq!(scripts.advance(), Some(scene_frame));
        assert_eq!(
            scripts.advance(),
            Some(ScriptEvent::Completed {
                trigger: parent,
                next_entry: 1,
                succeeded: true,
            })
        );
    }
}
