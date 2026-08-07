//! Explicit desktop update and timing state classification.

use pal_core::game::{BATTLE_FRAME_MS, EXPLORATION_FRAME_MS, UPDATE_INTERVAL_MS};

use super::menu_state::{OpeningMenu, OpeningMenuAction};
use super::opening_intro::OpeningIntro;

pub(super) const OPENING_INTRO_UPDATE_MS: u64 = 10;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct DebugState {
    pub(super) show_collision: bool,
    pub(super) show_objects: bool,
    pub(super) show_script: bool,
}

impl DebugState {
    pub(super) fn title(self) -> &'static str {
        match (self.show_collision, self.show_objects, self.show_script) {
            (false, false, false) => "Rust-PAL",
            (true, false, false) => "Rust-PAL [Collision]",
            (false, true, false) => "Rust-PAL [Objects]",
            (false, false, true) => "Rust-PAL [Script]",
            _ => "Rust-PAL [Debug]",
        }
    }
}

pub(super) enum FrontendState {
    Intro(Box<OpeningIntro>),
    OpeningMenu {
        menu: OpeningMenu,
        pending_action: Option<OpeningMenuAction>,
    },
    Playing,
}

impl FrontendState {
    pub(super) fn is_intro(&self) -> bool {
        matches!(self, Self::Intro(_))
    }

    pub(super) fn is_opening_menu(&self) -> bool {
        matches!(self, Self::OpeningMenu { .. })
    }

    pub(super) fn intro(&self) -> Option<&OpeningIntro> {
        match self {
            Self::Intro(intro) => Some(intro),
            Self::OpeningMenu { .. } | Self::Playing => None,
        }
    }

    pub(super) fn opening_menu(&self) -> Option<&OpeningMenu> {
        match self {
            Self::OpeningMenu { menu, .. } => Some(menu),
            Self::Intro(_) | Self::Playing => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UpdateLane {
    OpeningIntro,
    VisualOrDeferredAction,
    OpeningMenu,
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
pub(super) struct UpdateLaneState {
    pub(super) opening_intro: bool,
    pub(super) visual_or_deferred_action: bool,
    pub(super) opening_menu: bool,
    pub(super) waiting_for_key: bool,
    pub(super) dialog: bool,
    pub(super) post_battle: bool,
    pub(super) battle: bool,
    pub(super) battle_script_ready: bool,
    pub(super) menu: bool,
    pub(super) scene_script: bool,
}

impl UpdateLaneState {
    pub(super) fn lane(self) -> UpdateLane {
        if self.opening_intro {
            UpdateLane::OpeningIntro
        } else if self.visual_or_deferred_action {
            UpdateLane::VisualOrDeferredAction
        } else if self.opening_menu {
            UpdateLane::OpeningMenu
        } else if self.waiting_for_key {
            UpdateLane::WaitingForKey
        } else if self.dialog {
            UpdateLane::Dialog
        } else if self.post_battle {
            UpdateLane::PostBattle
        } else if self.battle && self.battle_script_ready {
            UpdateLane::BattleScript
        } else if self.battle {
            UpdateLane::Battle
        } else if self.menu {
            UpdateLane::Menu
        } else if self.scene_script {
            UpdateLane::SceneScript
        } else {
            UpdateLane::Exploration
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TimingMode {
    Intro,
    Ui,
    Battle,
    Exploration,
}

impl TimingMode {
    pub(super) const fn interval_ms(self) -> u64 {
        match self {
            Self::Intro => OPENING_INTRO_UPDATE_MS,
            Self::Ui => UPDATE_INTERVAL_MS,
            Self::Battle => BATTLE_FRAME_MS,
            Self::Exploration => EXPLORATION_FRAME_MS,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct TimingState {
    pub(super) opening_intro: bool,
    pub(super) dialog_or_visual: bool,
    pub(super) battle: bool,
    pub(super) scripted_or_menu: bool,
}

impl TimingState {
    pub(super) fn mode(self) -> TimingMode {
        if self.opening_intro {
            TimingMode::Intro
        } else if self.dialog_or_visual {
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
    fn update_lanes_preserve_desktop_priority() {
        let all = UpdateLaneState {
            opening_intro: true,
            visual_or_deferred_action: true,
            opening_menu: true,
            waiting_for_key: true,
            dialog: true,
            post_battle: true,
            battle: true,
            battle_script_ready: true,
            menu: true,
            scene_script: true,
        };
        assert_eq!(all.lane(), UpdateLane::OpeningIntro);
        assert_eq!(
            UpdateLaneState {
                opening_intro: false,
                ..all
            }
            .lane(),
            UpdateLane::VisualOrDeferredAction
        );
        assert_eq!(
            UpdateLaneState {
                opening_intro: false,
                visual_or_deferred_action: false,
                ..all
            }
            .lane(),
            UpdateLane::OpeningMenu
        );
        assert_eq!(
            UpdateLaneState {
                opening_intro: false,
                visual_or_deferred_action: false,
                opening_menu: false,
                ..all
            }
            .lane(),
            UpdateLane::WaitingForKey
        );
        assert_eq!(
            UpdateLaneState {
                opening_intro: false,
                visual_or_deferred_action: false,
                opening_menu: false,
                waiting_for_key: false,
                ..all
            }
            .lane(),
            UpdateLane::Dialog
        );
        assert_eq!(
            UpdateLaneState {
                opening_intro: false,
                visual_or_deferred_action: false,
                opening_menu: false,
                waiting_for_key: false,
                dialog: false,
                ..all
            }
            .lane(),
            UpdateLane::PostBattle
        );
        assert_eq!(
            UpdateLaneState {
                opening_intro: false,
                visual_or_deferred_action: false,
                opening_menu: false,
                waiting_for_key: false,
                dialog: false,
                post_battle: false,
                ..all
            }
            .lane(),
            UpdateLane::BattleScript
        );
        assert_eq!(
            UpdateLaneState {
                opening_intro: false,
                visual_or_deferred_action: false,
                opening_menu: false,
                waiting_for_key: false,
                dialog: false,
                post_battle: false,
                battle: false,
                battle_script_ready: false,
                ..all
            }
            .lane(),
            UpdateLane::Menu
        );
        assert_eq!(
            UpdateLaneState {
                opening_intro: false,
                visual_or_deferred_action: false,
                opening_menu: false,
                waiting_for_key: false,
                dialog: false,
                post_battle: false,
                battle: false,
                battle_script_ready: false,
                menu: false,
                ..all
            }
            .lane(),
            UpdateLane::SceneScript
        );
        assert_eq!(UpdateLaneState::default().lane(), UpdateLane::Exploration);
    }

    #[test]
    fn battle_script_only_preempts_a_running_battle_when_ready() {
        let battle = UpdateLaneState {
            battle: true,
            battle_script_ready: true,
            ..UpdateLaneState::default()
        };
        assert_eq!(battle.lane(), UpdateLane::BattleScript);
        assert_eq!(
            UpdateLaneState {
                battle_script_ready: false,
                ..battle
            }
            .lane(),
            UpdateLane::Battle
        );
    }

    #[test]
    fn timing_priority_matches_intro_ui_battle_and_exploration() {
        assert_eq!(
            TimingState {
                opening_intro: true,
                dialog_or_visual: true,
                battle: true,
                scripted_or_menu: true,
            }
            .mode(),
            TimingMode::Intro
        );
        assert_eq!(
            TimingState {
                dialog_or_visual: true,
                battle: true,
                ..TimingState::default()
            }
            .mode(),
            TimingMode::Ui
        );
        assert_eq!(
            TimingState {
                battle: true,
                scripted_or_menu: true,
                ..TimingState::default()
            }
            .mode(),
            TimingMode::Battle
        );
        assert_eq!(TimingState::default().mode(), TimingMode::Exploration);
    }
}
