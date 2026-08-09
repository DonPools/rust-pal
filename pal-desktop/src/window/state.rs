//! Explicit desktop update and timing state classification.

use pal_core::game::{BATTLE_FRAME_MS, EXPLORATION_FRAME_MS, UPDATE_INTERVAL_MS};

use super::menu_state::{OpeningMenu, OpeningMenuAction};
use super::opening_animation::OpeningAnimation;

pub(super) const OPENING_ANIMATION_UPDATE_MS: u64 = 10;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct DebugState {
    pub(super) show_collision: bool,
    pub(super) show_objects: bool,
    pub(super) show_script: bool,
    pub(super) show_battle: bool,
}

impl DebugState {
    pub(super) fn title(self) -> &'static str {
        match (
            self.show_collision,
            self.show_objects,
            self.show_script,
            self.show_battle,
        ) {
            (false, false, false, false) => "Rust-PAL",
            (true, false, false, false) => "Rust-PAL [Collision]",
            (false, true, false, false) => "Rust-PAL [Objects]",
            (false, false, true, false) => "Rust-PAL [Script]",
            (false, false, false, true) => "Rust-PAL [战斗助手]",
            _ => "Rust-PAL [Debug]",
        }
    }
}

pub(super) struct OpeningMenuState {
    pub(super) menu: OpeningMenu,
    pub(super) pending_action: Option<OpeningMenuAction>,
}

impl OpeningMenuState {
    pub(super) fn new(menu: OpeningMenu) -> Self {
        Self {
            menu,
            pending_action: None,
        }
    }
}

pub(super) enum AppMode {
    OpeningAnimation(Box<OpeningAnimation>),
    OpeningMenu(OpeningMenuState),
    Playing,
}

#[derive(Clone, Copy)]
pub(super) enum AppModeView<'a> {
    OpeningAnimation(&'a OpeningAnimation),
    OpeningMenu(&'a OpeningMenu),
    Playing,
}

impl AppMode {
    pub(super) fn is_opening_menu(&self) -> bool {
        matches!(self, Self::OpeningMenu(_))
    }

    pub(super) fn is_playing(&self) -> bool {
        matches!(self, Self::Playing)
    }

    pub(super) fn view(&self) -> AppModeView<'_> {
        match self {
            Self::OpeningAnimation(animation) => AppModeView::OpeningAnimation(animation),
            Self::OpeningMenu(state) => AppModeView::OpeningMenu(&state.menu),
            Self::Playing => AppModeView::Playing,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TickTarget {
    OpeningAnimation,
    OpeningMenu,
    Playing(PlayingTarget),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PlayingTarget {
    WaitingForKey,
    Dialog,
    PostBattle,
    BattleScript,
    Battle,
    Menu,
    SceneScript,
    Exploration,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct PlayingConditions {
    pub(super) waiting_for_key: bool,
    pub(super) dialog: bool,
    pub(super) post_battle: bool,
    pub(super) battle: bool,
    pub(super) battle_script_ready: bool,
    pub(super) menu: bool,
    pub(super) scene_script: bool,
}

impl PlayingConditions {
    pub(super) fn target(self) -> PlayingTarget {
        if self.waiting_for_key {
            PlayingTarget::WaitingForKey
        } else if self.dialog {
            PlayingTarget::Dialog
        } else if self.post_battle {
            PlayingTarget::PostBattle
        } else if self.battle && self.battle_script_ready {
            PlayingTarget::BattleScript
        } else if self.battle {
            PlayingTarget::Battle
        } else if self.menu {
            PlayingTarget::Menu
        } else if self.scene_script {
            PlayingTarget::SceneScript
        } else {
            PlayingTarget::Exploration
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TimingMode {
    OpeningAnimation,
    Ui,
    Battle,
    Exploration,
}

impl TimingMode {
    pub(super) const fn interval_ms(self) -> u64 {
        match self {
            Self::OpeningAnimation => OPENING_ANIMATION_UPDATE_MS,
            Self::Ui => UPDATE_INTERVAL_MS,
            Self::Battle => BATTLE_FRAME_MS,
            Self::Exploration => EXPLORATION_FRAME_MS,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct PlayingTimingState {
    pub(super) dialog_or_visual: bool,
    pub(super) battle: bool,
    pub(super) scripted_or_menu: bool,
}

impl PlayingTimingState {
    pub(super) fn mode(self) -> TimingMode {
        if self.dialog_or_visual {
            TimingMode::Ui
        } else if self.battle {
            TimingMode::Battle
        } else if self.scripted_or_menu {
            TimingMode::Ui
        } else {
            TimingMode::Exploration
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playing_targets_preserve_gameplay_priority() {
        let all = PlayingConditions {
            waiting_for_key: true,
            dialog: true,
            post_battle: true,
            battle: true,
            battle_script_ready: true,
            menu: true,
            scene_script: true,
        };
        assert_eq!(all.target(), PlayingTarget::WaitingForKey);
        assert_eq!(
            PlayingConditions {
                waiting_for_key: false,
                ..all
            }
            .target(),
            PlayingTarget::Dialog
        );
        assert_eq!(
            PlayingConditions {
                waiting_for_key: false,
                dialog: false,
                ..all
            }
            .target(),
            PlayingTarget::PostBattle
        );
        assert_eq!(
            PlayingConditions {
                waiting_for_key: false,
                dialog: false,
                post_battle: false,
                ..all
            }
            .target(),
            PlayingTarget::BattleScript
        );
        assert_eq!(
            PlayingConditions {
                waiting_for_key: false,
                dialog: false,
                post_battle: false,
                battle: false,
                battle_script_ready: false,
                ..all
            }
            .target(),
            PlayingTarget::Menu
        );
        assert_eq!(
            PlayingConditions {
                waiting_for_key: false,
                dialog: false,
                post_battle: false,
                battle: false,
                battle_script_ready: false,
                menu: false,
                ..all
            }
            .target(),
            PlayingTarget::SceneScript
        );
        assert_eq!(
            PlayingConditions::default().target(),
            PlayingTarget::Exploration
        );
    }

    #[test]
    fn battle_script_only_preempts_a_running_battle_when_ready() {
        let battle = PlayingConditions {
            battle: true,
            battle_script_ready: true,
            ..PlayingConditions::default()
        };
        assert_eq!(battle.target(), PlayingTarget::BattleScript);
        assert_eq!(
            PlayingConditions {
                battle_script_ready: false,
                ..battle
            }
            .target(),
            PlayingTarget::Battle
        );
    }

    #[test]
    fn playing_timing_priority_matches_ui_battle_and_exploration() {
        assert_eq!(
            PlayingTimingState {
                dialog_or_visual: true,
                battle: true,
                ..PlayingTimingState::default()
            }
            .mode(),
            TimingMode::Ui
        );
        assert_eq!(
            PlayingTimingState {
                battle: true,
                scripted_or_menu: true,
                ..PlayingTimingState::default()
            }
            .mode(),
            TimingMode::Battle
        );
        assert_eq!(
            PlayingTimingState::default().mode(),
            TimingMode::Exploration
        );
    }

    #[test]
    fn battle_debug_has_a_distinct_window_title() {
        assert_eq!(
            DebugState {
                show_battle: true,
                ..DebugState::default()
            }
            .title(),
            "Rust-PAL [战斗助手]"
        );
    }
}
