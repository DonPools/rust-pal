use std::collections::VecDeque;

use pal_assets::battle::BattleSpriteArchive;
use pal_assets::script::ScriptTable;
use pal_core::battle::BattleEvent;
use pal_core::script::ScriptEvent;

use crate::audio::{BackgroundMusic, MusicBackend, SoundEffects};

use super::battle_render::{BattleMenuState, PostBattlePresentation};
use super::dialog::ActiveDialog;
use super::menu_state::{
    ActiveMenu, ConfirmationMenu, EquipSession, FieldMenu, InventoryMenu, ItemUseSession,
    MagicSession, ShopMenu,
};
use super::original_save::OriginalSaveEnvironment;
use super::visual::VisualState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BattleDebugTarget {
    Enemy,
    Player,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct BattleDebugHit {
    pub(super) action: &'static str,
    pub(super) source: usize,
    pub(super) object_id: Option<u16>,
    pub(super) target: BattleDebugTarget,
    pub(super) target_index: usize,
    pub(super) damage: u16,
    pub(super) hp_before: Option<u16>,
    pub(super) hp_after: u16,
    pub(super) defeated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BattleDebugItemStart {
    pub(super) player: usize,
    pub(super) item_object: u16,
    pub(super) target: Option<usize>,
    pub(super) enemy_hp: Vec<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PendingSceneChange {
    source_scene: u16,
    target_scene: u16,
}

impl PendingSceneChange {
    /// Update the logical scene immediately while keeping the loaded scene
    /// available until the current trigger script returns to the frame loop.
    pub(super) fn request(
        pending: &mut Option<Self>,
        logical_scene: &mut u16,
        target_scene: u16,
    ) -> bool {
        if target_scene == 0 || target_scene == *logical_scene {
            return false;
        }
        let source_scene = pending.map_or(*logical_scene, |change| change.source_scene);
        *logical_scene = target_scene;
        *pending = Some(Self {
            source_scene,
            target_scene,
        });
        true
    }

    pub(super) fn source_scene(self) -> u16 {
        self.source_scene
    }

    pub(super) fn target_scene(self) -> u16 {
        self.target_scene
    }
}

pub(super) struct ScriptSession {
    pub(super) pending_scene_change: Option<PendingSceneChange>,
    pub(super) pending_dialog: Option<ActiveDialog>,
    pub(super) pending_script_event: Option<ScriptEvent>,
    pub(super) auto_scripts: ScriptTable,
    pub(super) dialog_delay_ms: u16,
    pub(super) waiting_for_key: bool,
}

pub(super) struct MenuSession {
    pub(super) active_menu: Option<ActiveMenu>,
    pub(super) main_menu_selected: usize,
    pub(super) inventory_action_selected: usize,
    pub(super) inventory_selected: usize,
    pub(super) item_target_selected: usize,
    pub(super) magic_caster_selected: usize,
    pub(super) magic_selected: usize,
    pub(super) magic_target_selected: usize,
    pub(super) system_selected: usize,
    pub(super) item_use: Option<ItemUseSession>,
    pub(super) equip: Option<EquipSession>,
    pub(super) magic: Option<MagicSession>,
}

pub(super) struct BattlePresentationState {
    pub(super) battle_selected_enemy: usize,
    pub(super) battle_command_selected: usize,
    pub(super) battle_targeting_enemy: bool,
    pub(super) battle_menu: BattleMenuState,
    pub(super) battle_auto_attack: bool,
    pub(super) battle_force_all: bool,
    pub(super) battle_repeat_all: bool,
    pub(super) battle_events: VecDeque<BattleEvent>,
    pub(super) battle_event_ticks: u16,
    pub(super) battle_kept_effects: Vec<BattleEvent>,
    pub(super) battle_effect_sound_count: u16,
    pub(super) battle_feedback_sound_played: bool,
    pub(super) battle_debug_hit: Option<BattleDebugHit>,
    pub(super) battle_debug_item_start: Option<BattleDebugItemStart>,
    pub(super) battle_settlement_ticks: Option<u16>,
    pub(super) magic_effect_frame_counts: Vec<Option<usize>>,
    pub(super) player_battle_frame_counts: Vec<Option<usize>>,
    pub(super) enemy_battle_frame_widths: Vec<Option<u16>>,
    pub(super) post_battle: Option<PostBattlePresentation>,
}

pub(super) struct AudioSession {
    pub(super) sound_effects: SoundEffects,
    pub(super) music: BackgroundMusic,
}

pub(super) struct MusicResources<'a> {
    pub(super) rix_mkf: &'a [u8],
    pub(super) midi_mkf: &'a [u8],
    pub(super) sound_font: &'a [u8],
    pub(super) requested_backend: MusicBackend,
}

pub(super) struct PersistenceState {
    pub(super) load_last_save_requested: bool,
    pub(super) pending_load_slot: Option<u8>,
    pub(super) quit_requested: bool,
    pub(super) current_save_slot: Option<u8>,
}

pub(super) struct DesktopSession {
    pub(super) scripts: ScriptSession,
    pub(super) menus: MenuSession,
    pub(super) battle: BattlePresentationState,
    pub(super) audio: AudioSession,
    pub(super) visual: VisualState,
    pub(super) persistence: PersistenceState,
}

impl DesktopSession {
    pub(super) fn has_active_menu(&self) -> bool {
        self.menus.active_menu.is_some()
    }

    pub(super) fn set_field_menu(&mut self, menu: FieldMenu) {
        self.menus.active_menu = Some(ActiveMenu::Field(menu));
    }

    pub(super) fn set_inventory_menu(&mut self, menu: InventoryMenu) {
        self.menus.active_menu = Some(ActiveMenu::Inventory(menu));
    }

    pub(super) fn set_confirmation_menu(&mut self, menu: ConfirmationMenu) {
        self.menus.active_menu = Some(ActiveMenu::Confirmation(menu));
    }

    pub(super) fn set_shop_menu(&mut self, menu: ShopMenu) {
        self.menus.active_menu = Some(ActiveMenu::Shop(menu));
    }

    pub(super) fn take_field_menu(&mut self) -> Option<FieldMenu> {
        match self.menus.active_menu.take() {
            Some(ActiveMenu::Field(menu)) => Some(menu),
            other => {
                self.menus.active_menu = other;
                None
            }
        }
    }

    pub(super) fn take_inventory_menu(&mut self) -> Option<InventoryMenu> {
        match self.menus.active_menu.take() {
            Some(ActiveMenu::Inventory(menu)) => Some(menu),
            other => {
                self.menus.active_menu = other;
                None
            }
        }
    }

    pub(super) fn take_confirmation_menu(&mut self) -> Option<ConfirmationMenu> {
        match self.menus.active_menu.take() {
            Some(ActiveMenu::Confirmation(menu)) => Some(menu),
            other => {
                self.menus.active_menu = other;
                None
            }
        }
    }

    pub(super) fn take_shop_menu(&mut self) -> Option<ShopMenu> {
        match self.menus.active_menu.take() {
            Some(ActiveMenu::Shop(menu)) => Some(menu),
            other => {
                self.menus.active_menu = other;
                None
            }
        }
    }

    pub(super) fn clear_active_menu(&mut self) {
        self.menus.active_menu = None;
    }

    pub(super) fn clear_transient_interaction(&mut self) {
        self.scripts.pending_scene_change = None;
        self.scripts.pending_dialog = None;
        self.scripts.pending_script_event = None;
        self.persistence.pending_load_slot = None;
        self.menus.active_menu = None;
        self.scripts.waiting_for_key = false;
        self.menus.item_use = None;
        self.menus.equip = None;
        self.menus.magic = None;
        self.battle.battle_events.clear();
        self.battle.battle_kept_effects.clear();
        self.battle.battle_debug_hit = None;
        self.battle.battle_debug_item_start = None;
        self.battle.post_battle = None;
    }

    pub(super) fn apply_original_restore(
        &mut self,
        environment: OriginalSaveEnvironment,
        prepare_fade_in: bool,
    ) {
        self.persistence.current_save_slot = Some(environment.slot);
        self.visual
            .restore_original_environment(environment.night_palette, environment.screen_wave);
        if prepare_fade_in {
            self.visual.prepare_scene_fade_in();
        }
        self.clear_transient_interaction();
    }

    pub(super) fn sync_music(&mut self, music_id: Option<u16>) -> bool {
        if let Some(music_id) = music_id {
            self.audio.music.play(music_id, true, 0)
        } else {
            self.audio.music.stop();
            true
        }
    }

    pub(super) fn new(
        auto_scripts: ScriptTable,
        voc_mkf: &[u8],
        music_resources: MusicResources<'_>,
        magic_effect_sprites: &BattleSpriteArchive,
        player_battle_sprites: &BattleSpriteArchive,
        enemy_battle_sprites: &BattleSpriteArchive,
    ) -> Self {
        let mut music = BackgroundMusic::new(
            music_resources.rix_mkf,
            music_resources.midi_mkf,
            music_resources.sound_font,
        )
        .expect("failed to load MUS.MKF RIX music");
        if !music.set_backend(music_resources.requested_backend) {
            eprintln!(
                "requested {} music is unavailable; using {}",
                music_resources.requested_backend,
                music.backend()
            );
        }
        println!("music backend: {}", music.backend());
        Self {
            scripts: ScriptSession {
                pending_scene_change: None,
                pending_dialog: None,
                pending_script_event: None,
                auto_scripts,
                dialog_delay_ms: 24,
                waiting_for_key: false,
            },
            menus: MenuSession {
                active_menu: None,
                main_menu_selected: 0,
                inventory_action_selected: 0,
                inventory_selected: 0,
                item_target_selected: 0,
                magic_caster_selected: 0,
                magic_selected: 0,
                magic_target_selected: 0,
                system_selected: 0,
                item_use: None,
                equip: None,
                magic: None,
            },
            battle: BattlePresentationState {
                battle_selected_enemy: 0,
                battle_command_selected: 0,
                battle_targeting_enemy: false,
                battle_menu: BattleMenuState::Main,
                battle_auto_attack: false,
                battle_force_all: false,
                battle_repeat_all: false,
                battle_events: VecDeque::new(),
                battle_event_ticks: 0,
                battle_kept_effects: Vec::new(),
                battle_effect_sound_count: 0,
                battle_feedback_sound_played: false,
                battle_debug_hit: None,
                battle_debug_item_start: None,
                battle_settlement_ticks: None,
                magic_effect_frame_counts: (0..magic_effect_sprites.len())
                    .map(|index| magic_effect_sprites.frame_count(index))
                    .collect(),
                player_battle_frame_counts: (0..player_battle_sprites.len())
                    .map(|index| player_battle_sprites.frame_count(index))
                    .collect(),
                enemy_battle_frame_widths: (0..enemy_battle_sprites.len())
                    .map(|index| {
                        enemy_battle_sprites
                            .decode_frame(index, 0)
                            .map(|frame| frame.width)
                    })
                    .collect(),
                post_battle: None,
            },
            audio: AudioSession {
                sound_effects: SoundEffects::new(voc_mkf)
                    .expect("failed to load VOC sound effects"),
                music,
            },
            visual: VisualState::new(),
            persistence: PersistenceState {
                load_last_save_requested: false,
                pending_load_slot: None,
                quit_requested: false,
                current_save_slot: None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PendingSceneChange;

    #[test]
    fn scene_change_keeps_the_loaded_source_while_updating_the_logical_target() {
        let mut pending = None;
        let mut logical_scene = 1;

        assert!(PendingSceneChange::request(
            &mut pending,
            &mut logical_scene,
            3
        ));
        assert_eq!(logical_scene, 3);
        assert_eq!(pending.unwrap().source_scene(), 1);
        assert_eq!(pending.unwrap().target_scene(), 3);

        assert!(!PendingSceneChange::request(
            &mut pending,
            &mut logical_scene,
            3
        ));
        assert!(PendingSceneChange::request(
            &mut pending,
            &mut logical_scene,
            1
        ));
        assert_eq!(logical_scene, 1);
        assert_eq!(pending.unwrap().source_scene(), 1);
        assert_eq!(pending.unwrap().target_scene(), 1);
    }
}
