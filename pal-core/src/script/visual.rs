//! Presentation commands yielded by trigger scripts.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptVisual {
    Shake {
        frames: u16,
        level: u16,
    },
    PlayRng {
        animation: u16,
        start_frame: u16,
        end_frame: Option<u16>,
        speed: u16,
    },
    FadeToRed,
    FadeOut {
        speed: u16,
    },
    FadeIn {
        speed: u16,
    },
    SetNightPalette {
        night: bool,
    },
    SetScreenWave {
        level: u16,
        progression: i16,
    },
    ShowFbp {
        index: u16,
        fade: u16,
    },
    ToggleDayNightPalette {
        update_scene: bool,
    },
    SetPalette {
        index: u16,
    },
    FadeColor {
        color: u8,
        from_color: bool,
        delay: u16,
    },
    RestoreScreen,
    FadeSceneWithUpdate {
        step: i16,
    },
    FadeToCurrentScene {
        speed: u16,
    },
    ScrollFbp {
        index: u16,
        speed: u16,
    },
    ShowFbpWithSprite {
        index: u16,
        sprite: Option<u16>,
        fade: u16,
    },
    PlayEndingAnimation,
    BackupScreen,
}
