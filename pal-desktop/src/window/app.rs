//! Desktop application state, fixed-step scheduling, and mode transitions.

use std::time::{Duration, Instant};

use crate::debug_overlay::DebugSnapshot;
use crate::renderer::Renderer;
use pal_core::game::GameState;
use pal_core::role::RoleSprites;
use pal_core::script::{ScriptRuntime, ScriptVisual};
use winit::keyboard::KeyCode;

use super::battle_debug_overlay::BattleDebugSnapshot;
use super::battle_update::{advance_post_battle, update_battle};
use super::clock::FrameClock;
use super::debug_render::{debug_object_snapshot, focused_debug_object};
use super::dialog::{advance_dialog_playback, ActiveDialog, DialogPlayback};
use super::dialog_text::dialog_page_count;
use super::input::HeldInput;
use super::menu_state::{FieldMenu, OpeningMenu, OpeningMenuAction};
use super::menu_update::{update_active_menu, MenuUpdateContext};
use super::minimap::{MiniMapCache, MiniMapFrame};
use super::opening_animation::{OpeningAnimation, OpeningAnimationAction, TITLE_MUSIC};
use super::original_save::{
    latest_original_save_slot, original_save_slots, restore_original_save, RestoreOriginalSaveError,
};
use super::presentation::{render_game, UiRenderContext};
use super::script_driver::{advance_script, auto_script_error_title, ScriptRenderResources};
use super::session::{DesktopSession, MusicResources};
use super::snapshot::{restore_snapshot, save_snapshot, RestoreSnapshotError};
use super::state::{
    AppMode, DebugState, OpeningMenuState, PlayingConditions, PlayingTarget, PlayingTimingState,
    TickTarget, TimingMode,
};
use super::types::{GameResources, LoadedScene};
use super::UI_TIME_QUANTUM_MS;

const OPENING_MENU_MUSIC: u16 = 4;
const DIALOG_POLL_INTERVAL_MS: u64 = 8;

pub(super) struct AdvanceResult {
    pub(super) exit: bool,
    pub(super) wait_until: Instant,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct TickOutcome {
    changed: bool,
    exit: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VisualFinalization {
    Run,
    Defer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AccumulatorAction {
    ConsumeTick,
    Reset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TargetOutcome {
    changed: bool,
    visual_finalization: VisualFinalization,
    accumulator_action: AccumulatorAction,
}

impl TargetOutcome {
    const fn run(changed: bool) -> Self {
        Self {
            changed,
            visual_finalization: VisualFinalization::Run,
            accumulator_action: AccumulatorAction::ConsumeTick,
        }
    }

    const fn defer(changed: bool) -> Self {
        Self {
            changed,
            visual_finalization: VisualFinalization::Defer,
            accumulator_action: AccumulatorAction::ConsumeTick,
        }
    }
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
    app_mode: AppMode,
    input: HeldInput,
    clock: FrameClock,
    minimap_enabled: bool,
    minimap_cache: MiniMapCache,
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
            MusicResources {
                rix_mkf: &resources.mus_mkf,
                midi_mkf: &resources.midi_mkf,
                sound_font: &resources.sound_font,
                requested_backend: resources.music_backend,
            },
            &resources.magic_effect_sprites,
            &resources.player_battle_sprites,
            &resources.enemy_battle_sprites,
        );
        let opening_animation = OpeningAnimation::from_resources(
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
            app_mode: AppMode::OpeningAnimation(Box::new(opening_animation)),
            input: HeldInput::default(),
            clock: FrameClock::new(now),
            minimap_enabled: true,
            minimap_cache: MiniMapCache::default(),
        }
    }

    pub(super) fn render_frame(&mut self, ui_ticks: u64) {
        let battle_active = self.game.battle().is_some();
        let freeze_battle_for_dialog = self.dialog.is_some() && battle_active;
        let battle_render_ticks =
            self.session
                .battle
                .render_ticks(ui_ticks, battle_active, freeze_battle_for_dialog);
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
                app_mode: self.app_mode.view(),
                opening_background: &resources.opening_background,
                dialog: self.dialog.as_ref(),
                active_menu: session.menus.active_menu.as_ref(),
                text: &resources.text,
                font: &resources.font,
                item_descriptions: &resources.item_descriptions,
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
                battle_render_ticks,
                palettes: &resources.palettes,
                visual: &session.visual,
            },
        );
        if let Some(presentation) = self.session.battle.post_battle.as_mut() {
            presentation.mark_current_page_presented();
        }
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
                KeyCode::F7 => {
                    self.debug.show_battle = !self.debug.show_battle;
                    set_title(self.debug.title());
                    true
                }
                KeyCode::KeyM
                    if self.app_mode.is_playing()
                        && self.game.battle().is_none()
                        && self.session.battle.post_battle.is_none()
                        && !self.scripts.is_active()
                        && self.dialog.is_none()
                        && !self.session.has_active_menu()
                        && !self.session.visual.is_blocking() =>
                {
                    self.minimap_enabled = !self.minimap_enabled;
                    true
                }
                KeyCode::F5
                    if !self.app_mode.is_opening_menu()
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
                    if !self.app_mode.is_opening_menu()
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
            let stopped_direction_input = self.input.set_key(code, pressed, repeat);
            if stopped_direction_input && self.stop_exploration_walking_animation() {
                self.render_frame(elapsed_ui_ticks(self.clock.ui_elapsed(Instant::now())));
            }
        }
    }

    fn stop_exploration_walking_animation(&mut self) -> bool {
        (self.select_tick_target(false) == TickTarget::Playing(PlayingTarget::Exploration))
            && self.game.stop_party_walking_animation()
    }

    fn timing_mode(&self) -> TimingMode {
        match &self.app_mode {
            AppMode::OpeningAnimation(_) => TimingMode::OpeningAnimation,
            AppMode::OpeningMenu(_) => TimingMode::Ui,
            AppMode::Playing => PlayingTimingState {
                dialog_or_visual: self.session.visual.is_blocking()
                    || self.dialog.is_some()
                    || self.session.scripts.waiting_for_key,
                battle: self.game.battle().is_some() || self.session.battle.post_battle.is_some(),
                scripted_or_menu: self.scripts.is_active()
                    || self.battle_scripts.is_active()
                    || self.session.has_active_menu(),
            }
            .mode(),
        }
    }

    fn select_playing_target(&self) -> PlayingTarget {
        PlayingConditions {
            waiting_for_key: self.session.scripts.waiting_for_key,
            dialog: self.dialog.is_some(),
            post_battle: self.session.battle.post_battle.is_some(),
            battle: self.game.battle().is_some(),
            battle_script_ready: self.battle_scripts.is_active()
                && self.session.battle.battle_events.is_empty(),
            menu: self.session.has_active_menu(),
            scene_script: self.scripts.is_active(),
        }
        .target()
    }

    fn select_tick_target(&self, visual_or_deferred_action: bool) -> TickTarget {
        match &self.app_mode {
            AppMode::OpeningAnimation(_) => TickTarget::OpeningAnimation,
            AppMode::OpeningMenu(_) if visual_or_deferred_action => {
                TickTarget::VisualOrDeferredAction
            }
            AppMode::OpeningMenu(_) => TickTarget::OpeningMenu,
            AppMode::Playing if visual_or_deferred_action => TickTarget::VisualOrDeferredAction,
            AppMode::Playing => TickTarget::Playing(self.select_playing_target()),
        }
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

    fn advance_opening_animation(
        &mut self,
        update_tick: Duration,
        input: pal_core::game::GameInput,
        set_title: &mut impl FnMut(&str),
    ) -> (bool, bool) {
        let AppMode::OpeningAnimation(animation) = &mut self.app_mode else {
            return (false, false);
        };
        let (changed, action) = animation
            .update(
                update_tick.as_millis() as u32,
                input,
                &self.resources.rng_archive,
            )
            .unwrap_or_else(|error| panic!("failed to play original opening animation: {error}"));
        let finished = action == OpeningAnimationAction::Finished;
        match action {
            OpeningAnimationAction::None => {}
            OpeningAnimationAction::PlayTitleMusic => {
                if !self.session.audio.music.play(TITLE_MUSIC, true, 2) {
                    set_title("Rust-PAL [title music unavailable]");
                }
            }
            OpeningAnimationAction::StopTitleMusic => self.session.audio.music.stop(),
            OpeningAnimationAction::Finished => {
                self.app_mode = AppMode::OpeningMenu(OpeningMenuState::new(OpeningMenu::new(
                    original_save_slots(&self.resources.original_save_dir),
                )));
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
        let pending = match &mut self.app_mode {
            AppMode::OpeningMenu(state) => state.pending_action.take(),
            AppMode::OpeningAnimation(_) | AppMode::Playing => return false,
        };
        if let Some(action) = pending {
            match action {
                OpeningMenuAction::StartNewGame => {
                    self.app_mode = AppMode::Playing;
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
                            self.app_mode = AppMode::Playing;
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

        let action = match &mut self.app_mode {
            AppMode::OpeningMenu(state) => state.menu.update(input),
            AppMode::OpeningAnimation(_) | AppMode::Playing => return false,
        };
        if action != OpeningMenuAction::None {
            if self
                .session
                .visual
                .queue(ScriptVisual::FadeOut { speed: 1 })
            {
                if let AppMode::OpeningMenu(state) = &mut self.app_mode {
                    state.pending_action = Some(action);
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
        .expect("active menu update target must contain a menu")
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

    fn current_update_tick(&self) -> Duration {
        Duration::from_millis(self.timing_mode().interval_ms())
    }

    /// Advance elapsed-time-driven state independently of the fixed simulation
    /// tick used by scripts, battles, and exploration.
    fn advance_realtime(&mut self, elapsed: Duration) -> bool {
        if !self.app_mode.is_playing() {
            return false;
        }
        self.advance_dialog_reveal(elapsed)
    }

    fn advance_dialog_reveal(&mut self, elapsed: Duration) -> bool {
        if self.session.visual.is_blocking() || self.session.scripts.waiting_for_key {
            return false;
        }
        let Some(dialog) = self.dialog.as_mut().filter(|dialog| !dialog.awaiting_input) else {
            return false;
        };
        let before = (dialog.revealed_glyphs, dialog.awaiting_input);
        let _ = advance_dialog_playback(
            &self.resources.text,
            dialog,
            &mut self.session.scripts.dialog_delay_ms,
            u32::try_from(elapsed.as_millis()).unwrap_or(u32::MAX),
            false,
        );
        let after = (dialog.revealed_glyphs, dialog.awaiting_input);
        before != after
    }

    fn advance_visual(&mut self, set_title: &mut impl FnMut(&str)) -> bool {
        if !self.session.visual.needs_update() {
            return false;
        }
        match self.session.visual.update(
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

    fn restore_selected_slot_if_ready(&mut self, set_title: &mut impl FnMut(&str)) -> bool {
        let Some(slot) = (!self.session.visual.is_blocking())
            .then(|| self.session.persistence.pending_load_slot.take())
            .flatten()
        else {
            return false;
        };

        match self.restore_original_slot(slot, true, true) {
            Ok(()) => set_title(&format!("Rust-PAL [save slot {slot} loaded]")),
            Err(RestoreOriginalSaveError::SceneUnavailable) => {
                self.session.visual.queue(ScriptVisual::FadeIn { speed: 1 });
                self.session.sync_music(self.game.current_music);
                set_title("Rust-PAL [save scene unavailable]");
            }
            Err(RestoreOriginalSaveError::Unavailable | RestoreOriginalSaveError::Invalid) => {
                self.session.visual.queue(ScriptVisual::FadeIn { speed: 1 });
                self.session.sync_music(self.game.current_music);
                set_title("Rust-PAL [invalid save slot]");
            }
        }
        true
    }

    fn restore_last_save_if_requested(&mut self, set_title: &mut impl FnMut(&str)) -> bool {
        if !std::mem::take(&mut self.session.persistence.load_last_save_requested) {
            return false;
        }

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
        true
    }

    fn advance_tick_target(
        &mut self,
        target: TickTarget,
        update_tick: Duration,
        input: pal_core::game::GameInput,
        any_pressed: bool,
        visual_scene_update_due: bool,
        set_title: &mut impl FnMut(&str),
    ) -> TargetOutcome {
        match target {
            TickTarget::OpeningAnimation => {
                let (changed, finished) =
                    self.advance_opening_animation(update_tick, input, set_title);
                TargetOutcome {
                    changed,
                    visual_finalization: VisualFinalization::Defer,
                    accumulator_action: if finished {
                        AccumulatorAction::Reset
                    } else {
                        AccumulatorAction::ConsumeTick
                    },
                }
            }
            TickTarget::VisualOrDeferredAction => {
                TargetOutcome::run(visual_scene_update_due && self.update_auto_scripts(set_title))
            }
            TickTarget::OpeningMenu => {
                TargetOutcome::run(self.advance_opening_menu(input, set_title))
            }
            TickTarget::Playing(target) => {
                self.advance_playing_target(target, input, any_pressed, set_title)
            }
        }
    }

    fn advance_playing_target(
        &mut self,
        target: PlayingTarget,
        input: pal_core::game::GameInput,
        any_pressed: bool,
        set_title: &mut impl FnMut(&str),
    ) -> TargetOutcome {
        match target {
            PlayingTarget::WaitingForKey => {
                if any_pressed {
                    self.session.scripts.waiting_for_key = false;
                    self.advance_active_script(set_title);
                    TargetOutcome::run(true)
                } else {
                    TargetOutcome::run(false)
                }
            }
            PlayingTarget::Dialog => {
                TargetOutcome::run(self.advance_dialog(input, any_pressed, set_title))
            }
            PlayingTarget::PostBattle => {
                advance_post_battle(any_pressed, &mut self.game, &mut self.session);
                TargetOutcome::run(true)
            }
            PlayingTarget::BattleScript => {
                let changed = self.advance_battle(input, any_pressed, set_title);
                TargetOutcome::defer(changed)
            }
            PlayingTarget::Battle => {
                TargetOutcome::run(self.advance_battle(input, any_pressed, set_title))
            }
            PlayingTarget::Menu => TargetOutcome::run(self.advance_menu(input, set_title)),
            PlayingTarget::SceneScript => {
                self.advance_scene_script(set_title);
                TargetOutcome::run(true)
            }
            PlayingTarget::Exploration => {
                let (changed, opened_menu) = self.advance_exploration(input, set_title);
                if opened_menu {
                    TargetOutcome::defer(changed)
                } else {
                    TargetOutcome::run(changed)
                }
            }
        }
    }

    fn should_queue_automatic_scene_fade_in(&self) -> bool {
        let script_execution_paused = (!self.scripts.is_active()
            && !self.battle_scripts.is_active())
            || self.dialog.is_some()
            || self.session.scripts.waiting_for_key
            || self.session.has_active_menu();
        !self.app_mode.is_opening_menu() && script_execution_paused
    }

    fn finish_tick_visuals(&mut self, set_title: &mut impl FnMut(&str)) -> bool {
        let mut changed = self.should_queue_automatic_scene_fade_in()
            && self.session.visual.queue_automatic_scene_fade_in();
        changed |= self.start_pending_visual(set_title);
        changed
    }

    fn advance_fixed_tick(
        &mut self,
        update_tick: Duration,
        set_title: &mut impl FnMut(&str),
    ) -> TickOutcome {
        let (input, any_pressed) = self.input.sample();
        let mut outcome = TickOutcome::default();

        let (target, visual_scene_update_due) = if self.app_mode.is_opening_animation() {
            (self.select_tick_target(false), false)
        } else {
            // Target selection intentionally uses the visual state from before
            // update(), so a visual finishing now cannot also advance gameplay.
            let visual_was_blocking = self.session.visual.is_blocking();
            let visual_scene_update_due = self.session.visual.scene_update_due();
            outcome.changed |= self.advance_visual(set_title);
            outcome.exit =
                self.session.persistence.quit_requested && !self.session.visual.is_blocking();

            let selected_load_handled = self.restore_selected_slot_if_ready(set_title);
            let last_save_load_handled = self.restore_last_save_if_requested(set_title);
            outcome.changed |= selected_load_handled || last_save_load_handled;

            let deferred_action_handled = selected_load_handled || last_save_load_handled;
            (
                self.select_tick_target(visual_was_blocking || deferred_action_handled),
                visual_scene_update_due,
            )
        };
        let target_outcome = self.advance_tick_target(
            target,
            update_tick,
            input,
            any_pressed,
            visual_scene_update_due,
            set_title,
        );
        outcome.changed |= target_outcome.changed;

        if target_outcome.visual_finalization == VisualFinalization::Run {
            outcome.changed |= self.finish_tick_visuals(set_title);
        }
        match target_outcome.accumulator_action {
            AccumulatorAction::ConsumeTick => self.clock.consume(update_tick),
            AccumulatorAction::Reset => self.clock.reset_accumulator(),
        }
        outcome
    }

    fn needs_continuous_ui_redraw(&self) -> bool {
        self.app_mode.is_opening_menu() || (self.dialog.is_none() && self.session.has_active_menu())
    }

    fn next_wakeup(&self, now: Instant) -> Instant {
        let simulation_wait = self
            .current_update_tick()
            .saturating_sub(self.clock.accumulator());
        let dialog_wait = self
            .dialog
            .as_ref()
            .is_some_and(|dialog| !dialog.awaiting_input)
            .then_some(Duration::from_millis(DIALOG_POLL_INTERVAL_MS));
        now + dialog_wait.map_or(simulation_wait, |wait| simulation_wait.min(wait))
    }

    pub(super) fn advance(
        &mut self,
        now: Instant,
        set_title: &mut impl FnMut(&str),
    ) -> AdvanceResult {
        self.session.audio.music.poll();
        let frame_elapsed = self.clock.begin_frame(now);
        let mut changed = self.advance_realtime(frame_elapsed);
        let mut exit = false;

        // Keep one timing mode for this catch-up cycle. State transitions affect
        // the wake-up interval calculated after the loop.
        let update_tick = self.current_update_tick();
        while self.clock.accumulator() >= update_tick {
            let outcome = self.advance_fixed_tick(update_tick, set_title);
            changed |= outcome.changed;
            exit |= outcome.exit;
        }

        changed |= self.needs_continuous_ui_redraw();
        if changed {
            self.render_frame(elapsed_ui_ticks(self.clock.ui_elapsed(now)));
        }
        AdvanceResult {
            exit,
            wait_until: self.next_wakeup(now),
        }
    }

    pub(super) fn reset_input(&mut self) {
        self.input = HeldInput::default();
        if self.stop_exploration_walking_animation() {
            self.render_frame(elapsed_ui_ticks(self.clock.ui_elapsed(Instant::now())));
        }
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

    pub(super) fn battle_assist_requested(&self) -> bool {
        self.debug.show_battle && self.game.battle().is_some()
    }

    pub(super) fn battle_debug_snapshot(
        &self,
        surface_width: u32,
        surface_height: u32,
        scale_factor: f64,
    ) -> Option<BattleDebugSnapshot> {
        if !self.debug.show_battle {
            return None;
        }
        let battle = self.game.battle()?;
        Some(BattleDebugSnapshot::capture(
            battle,
            &self.resources.text,
            self.session
                .battle
                .battle_targeting_enemy
                .then_some(self.session.battle.battle_selected_enemy),
            scale_factor,
            surface_width,
            surface_height,
        ))
    }

    pub(super) fn minimap_frame(
        &mut self,
        scale_factor: f64,
        surface_width: u32,
        surface_height: u32,
    ) -> Option<&MiniMapFrame> {
        let visible = self.minimap_enabled
            && self.app_mode.is_playing()
            && !self.debug.show_script
            && self.game.battle().is_none()
            && self.session.battle.post_battle.is_none()
            && !self.scripts.is_active()
            && self.dialog.is_none()
            && !self.session.has_active_menu()
            && !self.session.visual.is_blocking();
        visible.then(|| {
            self.minimap_cache.frame(
                self.game.scene_number,
                &self.game.map,
                &self.game.player,
                &self.game.scene_objects,
                scale_factor,
                surface_width,
                surface_height,
            )
        })
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
    opening_animation: bool,
    dialog_or_visual: bool,
    battle: bool,
    scripted_or_menu: bool,
) -> u64 {
    if opening_animation {
        TimingMode::OpeningAnimation
    } else {
        PlayingTimingState {
            dialog_or_visual,
            battle,
            scripted_or_menu,
        }
        .mode()
    }
    .interval_ms()
}

pub(super) fn elapsed_ui_ticks(elapsed: Duration) -> u64 {
    u64::try_from(elapsed.as_millis() / u128::from(UI_TIME_QUANTUM_MS)).unwrap_or(u64::MAX)
}
