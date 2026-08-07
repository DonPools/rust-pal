//! Desktop application state, fixed-step scheduling, and mode transitions.

use std::time::{Duration, Instant};

use crate::debug_overlay::DebugSnapshot;
use crate::renderer::Renderer;
use pal_core::game::GameState;
use pal_core::role::RoleSprites;
use pal_core::script::{ScriptRuntime, ScriptVisual};
use winit::keyboard::KeyCode;

use super::battle_update::{advance_post_battle, update_battle};
use super::clock::FrameClock;
use super::debug_render::{debug_object_snapshot, focused_debug_object};
use super::dialog::{advance_dialog_playback, ActiveDialog, DialogPlayback};
use super::dialog_text::dialog_page_count;
use super::input::HeldInput;
use super::menu_state::{FieldMenu, OpeningMenu, OpeningMenuAction};
use super::menu_update::{update_active_menu, MenuUpdateContext};
use super::opening_intro::{OpeningIntro, OpeningIntroAction, TITLE_MUSIC};
use super::original_save::{
    latest_original_save_slot, original_save_slots, restore_original_save, RestoreOriginalSaveError,
};
use super::presentation::{render_game, UiRenderContext};
use super::script_driver::{advance_script, auto_script_error_title, ScriptRenderResources};
use super::session::DesktopSession;
use super::snapshot::{restore_snapshot, save_snapshot, RestoreSnapshotError};
use super::state::{DebugState, FrontendState, TimingState, UpdateLane, UpdateLaneState};
use super::types::{GameResources, LoadedScene};
use super::UI_TIME_QUANTUM_MS;

const OPENING_MENU_MUSIC: u16 = 4;
const DIALOG_POLL_INTERVAL_MS: u64 = 8;

pub(super) struct AdvanceResult {
    pub(super) exit: bool,
    pub(super) wait_until: Instant,
}

pub(super) struct DesktopApp<L> {
    renderer: Renderer,
    game: GameState,
    resources: GameResources,
    load_scene: L,
    debug: DebugState,
    battle_scripts: ScriptRuntime,
    scripts: ScriptRuntime,
    dialog: Option<ActiveDialog>,
    session: DesktopSession,
    frontend: FrontendState,
    input: HeldInput,
    clock: FrameClock,
}

impl<L> DesktopApp<L>
where
    L: FnMut(u16, Option<u16>, &RoleSprites) -> Option<LoadedScene>,
{
    pub(super) fn new(
        renderer: Renderer,
        game: GameState,
        resources: GameResources,
        load_scene: L,
    ) -> Self {
        let auto_scripts = resources.script_table.clone();
        let battle_scripts = ScriptRuntime::new(resources.script_table.clone());
        let scripts = ScriptRuntime::new(resources.script_table.clone());
        let session = DesktopSession::new(
            auto_scripts,
            &resources.voc_mkf,
            &resources.mus_mkf,
            &resources.midi_mkf,
            &resources.sound_font,
            &resources.magic_effect_sprites,
            &resources.player_battle_sprites,
        );
        let opening_intro = OpeningIntro::from_resources(
            &resources.fbp_archive,
            &resources.rng_archive,
            &resources.role_sprites,
        )
        .expect("failed to load original opening animation resources");
        let now = Instant::now();
        Self {
            renderer,
            game,
            resources,
            load_scene,
            debug: DebugState::default(),
            battle_scripts,
            scripts,
            dialog: None,
            session,
            frontend: FrontendState::Intro(Box::new(opening_intro)),
            input: HeldInput::default(),
            clock: FrameClock::new(now),
        }
    }

    pub(super) fn render_frame(&mut self, ui_ticks: u64) {
        let resources = &self.resources;
        let session = &self.session;
        render_game(
            &mut self.renderer,
            &self.game,
            &resources.role_sprites,
            self.debug.show_collision,
            self.debug.show_objects,
            self.scripts.debug_snapshot(),
            UiRenderContext {
                opening_intro: self.frontend.intro(),
                opening_menu: self.frontend.opening_menu(),
                opening_background: &resources.opening_background,
                dialog: self.dialog.as_ref(),
                active_menu: session.menus.active_menu.as_ref(),
                text: &resources.text,
                font: &resources.font,
                dialog_faces: &resources.dialog_faces,
                dialog_icons: &resources.dialog_icons,
                ui_sprites: &resources.ui_sprites,
                item_sprites: &resources.item_sprites,
                enemy_battle_sprites: &resources.enemy_battle_sprites,
                player_battle_sprites: &resources.player_battle_sprites,
                magic_effect_sprites: &resources.magic_effect_sprites,
                battle_effects: &resources.battle_effects,
                battle_backgrounds: &resources.battle_backgrounds,
                battle_selected_enemy: session.battle.battle_selected_enemy,
                battle_command_selected: session.battle.battle_command_selected,
                battle_targeting_enemy: session.battle.battle_targeting_enemy,
                battle_menu: session.battle.battle_menu,
                battle_auto_attack: session.battle.battle_auto_attack,
                battle_event: session.battle.battle_events.front().copied(),
                battle_event_ticks: session.battle.battle_event_ticks,
                battle_kept_effects: &session.battle.battle_kept_effects,
                post_battle: session.battle.post_battle.as_ref(),
                status_background: &resources.status_background,
                equip_background: &resources.equip_background,
                ui_ticks,
                palettes: &resources.palettes,
                visual: &session.visual,
            },
        );
    }

    pub(super) fn handle_key_event(
        &mut self,
        code: KeyCode,
        pressed: bool,
        repeat: bool,
        set_title: &mut impl FnMut(&str),
    ) {
        let handled = pressed
            && !repeat
            && match code {
                KeyCode::F3 => {
                    self.debug.show_collision = !self.debug.show_collision;
                    set_title(self.debug.title());
                    true
                }
                KeyCode::F4 => {
                    self.debug.show_objects = !self.debug.show_objects;
                    set_title(self.debug.title());
                    true
                }
                KeyCode::F6 => {
                    self.debug.show_script = !self.debug.show_script;
                    set_title(self.debug.title());
                    true
                }
                KeyCode::F5
                    if !self.frontend.is_opening_menu()
                        && !self.scripts.is_active()
                        && self.dialog.is_none() =>
                {
                    if save_snapshot(&self.resources.snapshot_path, &self.game).is_ok() {
                        set_title("Rust-PAL [Snapshot saved]");
                    } else {
                        set_title("Rust-PAL [Snapshot save failed]");
                    }
                    true
                }
                KeyCode::F9
                    if !self.frontend.is_opening_menu()
                        && !self.scripts.is_active()
                        && self.dialog.is_none() =>
                {
                    match restore_snapshot(
                        &self.resources.snapshot_path,
                        &mut self.game,
                        &self.resources.role_sprites,
                        &mut self.load_scene,
                    ) {
                        Ok(()) => {
                            if !self.session.sync_music(self.game.current_music) {
                                set_title("Rust-PAL [snapshot music unavailable]");
                            }
                            self.session.clear_transient_interaction();
                            self.input = HeldInput::default();
                            set_title("Rust-PAL [Snapshot restored]");
                        }
                        Err(RestoreSnapshotError::SceneUnavailable) => {
                            set_title("Rust-PAL [Snapshot scene unavailable]");
                        }
                        Err(RestoreSnapshotError::Unavailable) => {
                            set_title("Rust-PAL [No snapshot]");
                        }
                    }
                    true
                }
                _ => false,
            };
        if handled {
            self.render_frame(elapsed_ui_ticks(self.clock.ui_elapsed(Instant::now())));
        } else {
            self.input.set_key(code, pressed, repeat);
        }
    }

    fn timing_state(&self) -> TimingState {
        TimingState {
            opening_intro: self.frontend.is_intro(),
            dialog_or_visual: self.session.visual.is_blocking()
                || self.dialog.is_some()
                || self.session.scripts.waiting_for_key,
            battle: self.game.battle().is_some() || self.session.battle.post_battle.is_some(),
            scripted_or_menu: self.frontend.is_opening_menu()
                || self.scripts.is_active()
                || self.battle_scripts.is_active()
                || self.session.has_active_menu(),
        }
    }

    fn update_lane(&self, visual_or_deferred_action: bool) -> UpdateLane {
        UpdateLaneState {
            opening_intro: self.frontend.is_intro(),
            visual_or_deferred_action,
            opening_menu: self.frontend.is_opening_menu(),
            waiting_for_key: self.session.scripts.waiting_for_key,
            dialog: self.dialog.is_some(),
            post_battle: self.session.battle.post_battle.is_some(),
            battle: self.game.battle().is_some(),
            battle_script_ready: self.battle_scripts.is_active()
                && self.session.battle.battle_events.is_empty(),
            menu: self.session.has_active_menu(),
            scene_script: self.scripts.is_active(),
        }
        .lane()
    }

    fn advance_active_script(&mut self, set_title: &mut impl FnMut(&str)) {
        let Self {
            game,
            resources,
            load_scene,
            battle_scripts,
            scripts,
            dialog,
            session,
            ..
        } = self;
        let active_scripts = if battle_scripts.is_active() {
            battle_scripts
        } else {
            scripts
        };
        advance_script(
            active_scripts,
            game,
            dialog,
            ScriptRenderResources {
                text: &resources.text,
                role_sprites: &resources.role_sprites,
            },
            load_scene,
            session,
            set_title,
        );
    }

    fn advance_scene_script(&mut self, set_title: &mut impl FnMut(&str)) {
        let Self {
            game,
            resources,
            load_scene,
            scripts,
            dialog,
            session,
            ..
        } = self;
        advance_script(
            scripts,
            game,
            dialog,
            ScriptRenderResources {
                text: &resources.text,
                role_sprites: &resources.role_sprites,
            },
            load_scene,
            session,
            set_title,
        );
    }

    fn update_auto_scripts(&mut self, set_title: &mut impl FnMut(&str)) -> bool {
        let update = self
            .game
            .update_auto_scripts_report(&self.session.scripts.auto_scripts);
        if let Some(error) = update.error {
            set_title(&auto_script_error_title(error));
        }
        for sound_id in self.game.take_auto_script_sounds() {
            if !self.session.audio.sound_effects.play(sound_id) {
                set_title(&format!("Rust-PAL [invalid auto sound {sound_id}]"));
            }
        }
        update.changed
    }

    fn restore_original_slot(
        &mut self,
        slot: u8,
        prepare_fade_in: bool,
        reset_input: bool,
    ) -> Result<(), RestoreOriginalSaveError> {
        let environment = restore_original_save(
            &self.resources.original_save_dir,
            slot,
            &mut self.game,
            &self.resources.role_sprites,
            &mut self.load_scene,
        )?;
        self.session
            .apply_original_restore(environment, prepare_fade_in);
        self.dialog = None;
        self.session.sync_music(self.game.current_music);
        if reset_input {
            self.input = HeldInput::default();
        }
        Ok(())
    }

    fn advance_intro(
        &mut self,
        update_tick: Duration,
        input: pal_core::game::GameInput,
        set_title: &mut impl FnMut(&str),
    ) -> (bool, bool) {
        let FrontendState::Intro(intro) = &mut self.frontend else {
            return (false, false);
        };
        let (changed, action) = intro
            .update(
                update_tick.as_millis() as u32,
                input,
                &self.resources.rng_archive,
            )
            .unwrap_or_else(|error| panic!("failed to play original opening animation: {error}"));
        let finished = action == OpeningIntroAction::Finished;
        match action {
            OpeningIntroAction::None => {}
            OpeningIntroAction::PlayTitleMusic => {
                if !self.session.audio.music.play(TITLE_MUSIC, true, 2) {
                    set_title("Rust-PAL [title music unavailable]");
                }
            }
            OpeningIntroAction::StopTitleMusic => self.session.audio.music.stop(),
            OpeningIntroAction::Finished => {
                self.frontend = FrontendState::OpeningMenu {
                    menu: OpeningMenu::new(original_save_slots(&self.resources.original_save_dir)),
                    pending_action: None,
                };
                self.session.visual.restore_original_environment(false, 0);
                if !self.session.audio.music.play(OPENING_MENU_MUSIC, true, 1) {
                    set_title("Rust-PAL [opening music unavailable]");
                }
                self.session.visual.queue(ScriptVisual::FadeIn { speed: 1 });
                self.session
                    .visual
                    .start_pending(
                        self.renderer.screen(),
                        &self.resources.palettes,
                        &self.resources.fbp_archive,
                        &self.resources.rng_archive,
                        &self.resources.role_sprites,
                    )
                    .unwrap_or_else(|error| {
                        panic!("failed to start opening menu fade-in: {error}")
                    });
            }
        }
        (changed || finished, finished)
    }

    fn advance_opening_menu(
        &mut self,
        input: pal_core::game::GameInput,
        set_title: &mut impl FnMut(&str),
    ) -> bool {
        let pending = match &mut self.frontend {
            FrontendState::OpeningMenu { pending_action, .. } => pending_action.take(),
            FrontendState::Intro(_) | FrontendState::Playing => return false,
        };
        if let Some(action) = pending {
            match action {
                OpeningMenuAction::StartNewGame => {
                    self.frontend = FrontendState::Playing;
                    self.session.clear_transient_interaction();
                    self.session.audio.music.stop();
                    self.session.visual.prepare_scene_fade_in();
                    let enter_script = self
                        .game
                        .scene_enter_script(self.resources.initial_enter_script);
                    if enter_script != 0 {
                        self.scripts.start(pal_core::scene::TriggerRequest {
                            object_id: 0xffff,
                            script_entry: enter_script,
                            kind: pal_core::scene::TriggerKind::Touch,
                        });
                    }
                    set_title("Rust-PAL");
                }
                OpeningMenuAction::LoadSlot(slot) => {
                    match self.restore_original_slot(slot, true, false) {
                        Ok(()) => {
                            self.frontend = FrontendState::Playing;
                            set_title(&format!("Rust-PAL [save slot {slot} loaded]"));
                        }
                        Err(RestoreOriginalSaveError::SceneUnavailable) => {
                            self.session.visual.queue(ScriptVisual::FadeIn { speed: 1 });
                            set_title("Rust-PAL [save scene unavailable]");
                        }
                        Err(RestoreOriginalSaveError::Unavailable) => {
                            self.session.visual.queue(ScriptVisual::FadeIn { speed: 1 });
                            set_title("Rust-PAL [empty save slot]");
                        }
                        Err(RestoreOriginalSaveError::Invalid) => {
                            self.session.visual.queue(ScriptVisual::FadeIn { speed: 1 });
                            set_title("Rust-PAL [invalid save slot]");
                        }
                    }
                }
                OpeningMenuAction::None => {}
            }
            return true;
        }

        let action = match &mut self.frontend {
            FrontendState::OpeningMenu { menu, .. } => menu.update(input),
            FrontendState::Intro(_) | FrontendState::Playing => return false,
        };
        if action != OpeningMenuAction::None {
            if self
                .session
                .visual
                .queue(ScriptVisual::FadeOut { speed: 1 })
            {
                if let FrontendState::OpeningMenu { pending_action, .. } = &mut self.frontend {
                    *pending_action = Some(action);
                }
            } else {
                set_title("Rust-PAL [opening transition is already active]");
            }
        }
        true
    }

    fn advance_dialog(
        &mut self,
        input: pal_core::game::GameInput,
        any_pressed: bool,
        set_title: &mut impl FnMut(&str),
    ) -> bool {
        let awaiting_input = self
            .dialog
            .as_ref()
            .is_some_and(|dialog| dialog.awaiting_input);
        if awaiting_input {
            let dialog = self.dialog.as_mut().expect("dialog was checked above");
            dialog.wait_palette_ticks = dialog.wait_palette_ticks.wrapping_add(1);
        }
        let timed_out = if awaiting_input {
            self.dialog
                .as_mut()
                .and_then(|dialog| dialog.auto_wait_ticks.as_mut())
                .is_some_and(|ticks| {
                    *ticks = ticks.saturating_sub(1);
                    *ticks == 0
                })
        } else {
            false
        };
        if awaiting_input && (any_pressed || timed_out) {
            let dialog = self.dialog.as_mut().expect("dialog was checked above");
            let page_count = dialog_page_count(&self.resources.text, dialog);
            if dialog.page + 1 < page_count {
                dialog.page += 1;
                dialog.awaiting_input = false;
                dialog.auto_wait_ticks = None;
                dialog.wait_palette_ticks = 0;
            } else if let Some(pending) = self.session.scripts.pending_dialog.take() {
                self.dialog = Some(pending);
            } else {
                self.dialog = None;
                self.advance_active_script(set_title);
            }
        } else if !awaiting_input {
            let playback = advance_dialog_playback(
                &self.resources.text,
                self.dialog.as_mut().expect("dialog was checked above"),
                &mut self.session.scripts.dialog_delay_ms,
                0,
                input.confirm || input.cancel,
            );
            if matches!(
                playback,
                DialogPlayback::ContinueScript | DialogPlayback::AutoClose
            ) {
                if playback == DialogPlayback::AutoClose {
                    self.dialog = None;
                }
                self.advance_active_script(set_title);
            }
        }
        true
    }

    fn advance_battle(
        &mut self,
        input: pal_core::game::GameInput,
        any_pressed: bool,
        set_title: &mut impl FnMut(&str),
    ) -> bool {
        if self.battle_scripts.is_active() && self.session.battle.battle_events.is_empty() {
            let Self {
                game,
                resources,
                load_scene,
                battle_scripts,
                dialog,
                session,
                ..
            } = self;
            advance_script(
                battle_scripts,
                game,
                dialog,
                ScriptRenderResources {
                    text: &resources.text,
                    role_sprites: &resources.role_sprites,
                },
                load_scene,
                session,
                set_title,
            );
            return true;
        }
        let outcome = update_battle(
            input,
            any_pressed,
            &mut self.game,
            &mut self.session,
            &mut self.battle_scripts,
        );
        if let Some(outcome) = outcome {
            if !self.scripts.resolve_battle(outcome.result) {
                set_title("Rust-PAL [battle script resume failed]");
            } else {
                self.session.sync_music(self.game.current_music);
                set_title("Rust-PAL");
                self.advance_scene_script(set_title);
            }
        }
        true
    }

    fn advance_menu(
        &mut self,
        input: pal_core::game::GameInput,
        set_title: &mut impl FnMut(&str),
    ) -> bool {
        update_active_menu(&mut MenuUpdateContext {
            input,
            scripts: &mut self.scripts,
            game: &mut self.game,
            dialog: &mut self.dialog,
            text: &self.resources.text,
            role_sprites: &self.resources.role_sprites,
            load_scene: &mut self.load_scene,
            services: &mut self.session,
            original_save_dir: &self.resources.original_save_dir,
            set_title,
        })
        .expect("active menu update lane must contain a menu")
    }

    fn advance_exploration(
        &mut self,
        input: pal_core::game::GameInput,
        set_title: &mut impl FnMut(&str),
    ) -> (bool, bool) {
        if input.cancel {
            self.session.set_field_menu(FieldMenu::Main {
                selected: self.session.menus.main_menu_selected,
            });
            set_title("Rust-PAL [Menu]");
            return (true, true);
        }
        let mut changed = self.game.update(input);
        if changed {
            if let Some(trigger) = self.game.take_trigger() {
                if self.scripts.start(trigger) {
                    self.advance_scene_script(set_title);
                }
            }
        }
        if !self.scripts.is_active() {
            changed |= self.update_auto_scripts(set_title);
        }
        (changed, false)
    }

    fn start_pending_visual(&mut self, set_title: &mut impl FnMut(&str)) -> bool {
        match self.session.visual.start_pending(
            self.renderer.screen(),
            &self.resources.palettes,
            &self.resources.fbp_archive,
            &self.resources.rng_archive,
            &self.resources.role_sprites,
        ) {
            Ok(changed) => changed,
            Err(error) => {
                set_title(&format!("Rust-PAL [visual error: {error}]"));
                false
            }
        }
    }

    pub(super) fn advance(
        &mut self,
        now: Instant,
        set_title: &mut impl FnMut(&str),
    ) -> AdvanceResult {
        self.session.audio.music.poll();
        let frame_elapsed = self.clock.begin_frame(now);
        let mut changed = false;
        let mut exit = false;

        // Dialog glyphs use the original 8 ms timing quantum rather than the
        // slower simulation step used by scripts and exploration.
        if !self.frontend.is_intro()
            && !self.frontend.is_opening_menu()
            && !self.session.visual.is_blocking()
            && !self.session.scripts.waiting_for_key
            && self
                .dialog
                .as_ref()
                .is_some_and(|dialog| !dialog.awaiting_input)
        {
            let before = self
                .dialog
                .as_ref()
                .map(|dialog| (dialog.revealed_glyphs, dialog.awaiting_input));
            let _ = advance_dialog_playback(
                &self.resources.text,
                self.dialog.as_mut().expect("dialog was checked above"),
                &mut self.session.scripts.dialog_delay_ms,
                u32::try_from(frame_elapsed.as_millis()).unwrap_or(u32::MAX),
                false,
            );
            let after = self
                .dialog
                .as_ref()
                .map(|dialog| (dialog.revealed_glyphs, dialog.awaiting_input));
            changed |= before != after;
        }

        let update_tick = Duration::from_millis(self.timing_state().mode().interval_ms());
        while self.clock.accumulator() >= update_tick {
            let (input, any_pressed) = self.input.sample();
            if self.frontend.is_intro() {
                let (intro_changed, finished) = self.advance_intro(update_tick, input, set_title);
                changed |= intro_changed;
                if finished {
                    self.clock.reset_accumulator();
                } else {
                    self.clock.consume(update_tick);
                }
                continue;
            }

            let visual_was_blocking = self.session.visual.is_blocking();
            let visual_scene_update_due = self.session.visual.scene_update_due();
            if self.session.visual.needs_update() {
                match self.session.visual.update(
                    self.renderer.screen(),
                    &self.resources.palettes,
                    &self.resources.fbp_archive,
                    &self.resources.rng_archive,
                    &self.resources.role_sprites,
                ) {
                    Ok(visual_changed) => changed |= visual_changed,
                    Err(error) => set_title(&format!("Rust-PAL [visual error: {error}]")),
                }
            }
            if self.session.persistence.quit_requested && !self.session.visual.is_blocking() {
                exit = true;
            }

            let selected_load_slot = (!self.session.visual.is_blocking())
                .then(|| self.session.persistence.pending_load_slot.take())
                .flatten();
            if let Some(slot) = selected_load_slot {
                match self.restore_original_slot(slot, true, true) {
                    Ok(()) => set_title(&format!("Rust-PAL [save slot {slot} loaded]")),
                    Err(RestoreOriginalSaveError::SceneUnavailable) => {
                        self.session.visual.queue(ScriptVisual::FadeIn { speed: 1 });
                        self.session.sync_music(self.game.current_music);
                        set_title("Rust-PAL [save scene unavailable]");
                    }
                    Err(
                        RestoreOriginalSaveError::Unavailable | RestoreOriginalSaveError::Invalid,
                    ) => {
                        self.session.visual.queue(ScriptVisual::FadeIn { speed: 1 });
                        self.session.sync_music(self.game.current_music);
                        set_title("Rust-PAL [invalid save slot]");
                    }
                }
                changed = true;
            }

            let load_last_save_requested =
                std::mem::take(&mut self.session.persistence.load_last_save_requested);
            if load_last_save_requested {
                let slot = self
                    .session
                    .persistence
                    .current_save_slot
                    .or_else(|| latest_original_save_slot(&self.resources.original_save_dir));
                match slot.map(|slot| self.restore_original_slot(slot, false, false)) {
                    Some(Ok(())) => set_title("Rust-PAL [Original save loaded]"),
                    Some(Err(RestoreOriginalSaveError::SceneUnavailable)) => {
                        set_title("Rust-PAL [Original save scene unavailable]")
                    }
                    Some(Err(
                        RestoreOriginalSaveError::Unavailable | RestoreOriginalSaveError::Invalid,
                    )) => set_title("Rust-PAL [Invalid original save]"),
                    None => set_title("Rust-PAL [No original save]"),
                }
                changed = true;
            }

            let visual_or_deferred =
                visual_was_blocking || selected_load_slot.is_some() || load_last_save_requested;
            let lane = self.update_lane(visual_or_deferred);
            let mut skip_post_tick = false;
            match lane {
                UpdateLane::OpeningIntro => {
                    unreachable!("opening intro is handled before desktop update lanes")
                }
                UpdateLane::VisualOrDeferredAction => {
                    if visual_scene_update_due {
                        changed |= self.update_auto_scripts(set_title);
                    }
                }
                UpdateLane::OpeningMenu => {
                    changed |= self.advance_opening_menu(input, set_title);
                }
                UpdateLane::WaitingForKey => {
                    if any_pressed {
                        self.session.scripts.waiting_for_key = false;
                        self.advance_active_script(set_title);
                        changed = true;
                    }
                }
                UpdateLane::Dialog => {
                    changed |= self.advance_dialog(input, any_pressed, set_title);
                }
                UpdateLane::PostBattle => {
                    advance_post_battle(any_pressed, &mut self.game, &mut self.session);
                    changed = true;
                }
                UpdateLane::BattleScript => {
                    changed |= self.advance_battle(input, any_pressed, set_title);
                    skip_post_tick = true;
                }
                UpdateLane::Battle => {
                    changed |= self.advance_battle(input, any_pressed, set_title);
                }
                UpdateLane::Menu => {
                    changed = self.advance_menu(input, set_title);
                }
                UpdateLane::SceneScript => {
                    self.advance_scene_script(set_title);
                    changed = true;
                }
                UpdateLane::Exploration => {
                    let (exploration_changed, opened_menu) =
                        self.advance_exploration(input, set_title);
                    changed |= exploration_changed;
                    skip_post_tick = opened_menu;
                }
            }
            if skip_post_tick {
                self.clock.consume(update_tick);
                continue;
            }

            let script_is_paused = (!self.scripts.is_active() && !self.battle_scripts.is_active())
                || self.dialog.is_some()
                || self.session.scripts.waiting_for_key
                || self.session.has_active_menu();
            if !self.frontend.is_opening_menu()
                && script_is_paused
                && self.session.visual.queue_automatic_scene_fade_in()
            {
                changed = true;
            }
            changed |= self.start_pending_visual(set_title);
            self.clock.consume(update_tick);
        }

        if self.frontend.is_opening_menu()
            || (self.dialog.is_none() && self.session.has_active_menu())
        {
            changed = true;
        }
        let next_update_tick = Duration::from_millis(self.timing_state().mode().interval_ms());
        let simulation_wait = next_update_tick.saturating_sub(self.clock.accumulator());
        let dialog_wait = self
            .dialog
            .as_ref()
            .is_some_and(|dialog| !dialog.awaiting_input)
            .then_some(Duration::from_millis(DIALOG_POLL_INTERVAL_MS));
        if changed {
            self.render_frame(elapsed_ui_ticks(self.clock.ui_elapsed(now)));
        }
        AdvanceResult {
            exit,
            wait_until: now + dialog_wait.map_or(simulation_wait, |wait| simulation_wait.min(wait)),
        }
    }

    pub(super) fn reset_input(&mut self) {
        self.input = HeldInput::default();
    }

    pub(super) fn screen(&self) -> &[u8] {
        self.renderer.screen()
    }

    pub(super) fn is_dirty(&self) -> bool {
        self.renderer.is_dirty()
    }

    pub(super) fn mark_presented(&mut self) {
        self.renderer.mark_cleaned();
    }

    pub(super) fn show_script_debug(&self) -> bool {
        self.debug.show_script
    }

    pub(super) fn debug_snapshot(
        &self,
        surface_width: u32,
        surface_height: u32,
        scale_factor: f64,
    ) -> DebugSnapshot {
        let script = self.scripts.debug_snapshot();
        DebugSnapshot {
            scene_number: self.game.scene_number,
            object_count: self.game.scene_objects.len(),
            player_x: self.game.player.world_x,
            player_y: self.game.player.world_y,
            camera_x: self.game.camera.x,
            camera_y: self.game.camera.y,
            virtual_width: self.renderer.width as u32,
            virtual_height: self.renderer.height as u32,
            surface_width,
            surface_height,
            scale_factor,
            focused_object: focused_debug_object(&self.game, script).map(debug_object_snapshot),
            script,
        }
    }
}

#[cfg(test)]
pub(super) fn update_interval_ms(
    opening_intro: bool,
    dialog_or_visual: bool,
    battle: bool,
    scripted_or_menu: bool,
) -> u64 {
    TimingState {
        opening_intro,
        dialog_or_visual,
        battle,
        scripted_or_menu,
    }
    .mode()
    .interval_ms()
}

pub(super) fn elapsed_ui_ticks(elapsed: Duration) -> u64 {
    u64::try_from(elapsed.as_millis() / u128::from(UI_TIME_QUANTUM_MS)).unwrap_or(u64::MAX)
}
