use std::collections::VecDeque;

use pal_assets::script::ScriptTable;
use pal_core::battle::BattleEvent;

use crate::audio::{BackgroundMusic, SoundEffects};

use super::dialog::ActiveDialog;
use super::menu_state::{
    ConfirmationMenu, EquipSession, FieldMenu, InventoryMenu, ItemUseSession, MagicSession,
    ShopMenu,
};
use super::visual::VisualState;

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

pub(super) struct SessionState {
    pub(super) pending_scene_change: Option<PendingSceneChange>,
    pub(super) pending_dialog: Option<ActiveDialog>,
    pub(super) field_menu: Option<FieldMenu>,
    pub(super) main_menu_selected: usize,
    pub(super) inventory_action_selected: usize,
    pub(super) inventory_selected: usize,
    pub(super) item_target_selected: usize,
    pub(super) magic_caster_selected: usize,
    pub(super) magic_selected: usize,
    pub(super) magic_target_selected: usize,
    pub(super) battle_selected_enemy: usize,
    pub(super) battle_command_selected: usize,
    pub(super) battle_pending_throw_item: Option<u16>,
    pub(super) battle_events: VecDeque<BattleEvent>,
    pub(super) battle_event_ticks: u16,
    pub(super) system_selected: usize,
    pub(super) confirmation_menu: Option<ConfirmationMenu>,
    pub(super) shop_menu: Option<ShopMenu>,
    pub(super) inventory_menu: Option<InventoryMenu>,
    pub(super) item_use: Option<ItemUseSession>,
    pub(super) equip: Option<EquipSession>,
    pub(super) magic: Option<MagicSession>,
    pub(super) auto_scripts: ScriptTable,
    pub(super) sound_effects: SoundEffects,
    pub(super) music: BackgroundMusic,
    pub(super) visual: VisualState,
    pub(super) dialog_delay_ms: u16,
    pub(super) waiting_for_key: bool,
    pub(super) load_last_save_requested: bool,
    pub(super) quit_requested: bool,
    pub(super) current_save_slot: Option<u8>,
}

impl SessionState {
    pub(super) fn new(
        auto_scripts: ScriptTable,
        voc_mkf: &[u8],
        midi_mkf: &[u8],
        sound_font: &[u8],
    ) -> Self {
        Self {
            pending_scene_change: None,
            pending_dialog: None,
            field_menu: None,
            main_menu_selected: 0,
            inventory_action_selected: 0,
            inventory_selected: 0,
            item_target_selected: 0,
            magic_caster_selected: 0,
            magic_selected: 0,
            magic_target_selected: 0,
            battle_selected_enemy: 0,
            battle_command_selected: 0,
            battle_pending_throw_item: None,
            battle_events: VecDeque::new(),
            battle_event_ticks: 0,
            system_selected: 0,
            confirmation_menu: None,
            shop_menu: None,
            inventory_menu: None,
            item_use: None,
            equip: None,
            magic: None,
            auto_scripts,
            sound_effects: SoundEffects::new(voc_mkf).expect("failed to load VOC sound effects"),
            music: BackgroundMusic::new(midi_mkf, sound_font)
                .expect("failed to load MIDI music and SoundFont"),
            visual: VisualState::new(),
            dialog_delay_ms: 24,
            waiting_for_key: false,
            load_last_save_requested: false,
            quit_requested: false,
            current_save_slot: None,
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
