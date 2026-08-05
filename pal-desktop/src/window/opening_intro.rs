use std::time::{SystemTime, UNIX_EPOCH};

use pal_assets::fbp::{FbpArchive, FBP_HEIGHT, FBP_PIXELS, FBP_WIDTH};
use pal_assets::palette::{Palette, PaletteSet};
use pal_assets::rle::RleBitmap;
use pal_assets::rng::{RngArchive, RNG_FRAME_PIXELS};
use pal_core::game::GameInput;
use pal_core::role::RoleSprites;

use crate::renderer::Renderer;

const TRADEMARK_ANIMATION: usize = 6;
const TRADEMARK_PALETTE: usize = 3;
const TRADEMARK_FRAME_MS: u32 = 40;
const TRADEMARK_HOLD_MS: u32 = 1_000;
const SPLASH_UP_PICTURE: usize = 38;
const SPLASH_DOWN_PICTURE: usize = 39;
const SPLASH_TITLE_SPRITE: usize = 71;
const SPLASH_CRANE_SPRITE: usize = 73;
const SPLASH_PALETTE: usize = 1;
const SPLASH_FRAME_MS: u32 = 85;
const SPLASH_PALETTE_FADE_MS: u32 = 15_000;
const SCREEN_FADE_MS: u32 = 600;
const SPLASH_SKIP_STEP_MS: u32 = 8;
const SPLASH_SKIP_VIRTUAL_MS: u32 = 250;
const SPLASH_SKIP_HOLD_MS: u32 = 500;
const CRANE_COUNT: usize = 9;
const CRANE_ANIMATION_FRAMES: usize = 8;

pub(super) const TITLE_MUSIC: u16 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OpeningIntroAction {
    None,
    PlayTitleMusic,
    StopTitleMusic,
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpeningIntroPhase {
    Trademark {
        next_frame: usize,
        frame_accumulator_ms: u32,
    },
    TrademarkHold {
        remaining_ms: u32,
    },
    TrademarkFadeOut {
        elapsed_ms: u32,
    },
    Splash,
    SplashCompleting {
        elapsed_ms: u32,
        accelerate_ms: u32,
        total_ms: u32,
        start_palette_ms: u32,
    },
    SplashFadeOut {
        elapsed_ms: u32,
    },
    Finished,
}

#[derive(Debug, Clone, Copy)]
struct Crane {
    /// The original loop decrements X after drawing. Stored X is therefore the
    /// next frame's position and the current frame is rendered at `x + 1`.
    x: i32,
    y: i32,
    frame: usize,
}

/// The DOS startup sequence from `PAL_TrademarkScreen` and `PAL_SplashScreen`.
pub(super) struct OpeningIntro {
    phase: OpeningIntroPhase,
    trademark_frame_count: usize,
    trademark_canvas: Vec<u8>,
    splash_up: Vec<u8>,
    splash_down: Vec<u8>,
    title: RleBitmap,
    crane_frames: Vec<RleBitmap>,
    cranes: [Crane; CRANE_COUNT],
    splash_image_position: usize,
    splash_title_height: usize,
    splash_frame: u32,
    splash_frame_accumulator_ms: u32,
    splash_palette_ms: u32,
}

impl OpeningIntro {
    pub(super) fn from_resources(
        fbp: &FbpArchive,
        rng: &RngArchive,
        sprites: &RoleSprites,
    ) -> Result<Self, String> {
        let animation = rng
            .animation(TRADEMARK_ANIMATION)
            .ok_or_else(|| format!("RNG animation {TRADEMARK_ANIMATION} is unavailable"))?;
        let trademark_frame_count = (0..animation.frame_count())
            .take_while(|&frame| {
                animation
                    .compressed_frame(frame)
                    .is_some_and(|data| !data.is_empty())
            })
            .count();
        if trademark_frame_count == 0 {
            return Err(format!("RNG animation {TRADEMARK_ANIMATION} has no frames"));
        }
        let mut trademark_canvas = vec![0; RNG_FRAME_PIXELS];
        animation
            .apply_frame(0, &mut trademark_canvas)
            .ok_or_else(|| format!("RNG animation {TRADEMARK_ANIMATION} frame 0 is invalid"))?;

        let splash_up = fbp
            .frame(SPLASH_UP_PICTURE)
            .ok_or_else(|| format!("FBP picture {SPLASH_UP_PICTURE} is unavailable"))?;
        let splash_down = fbp
            .frame(SPLASH_DOWN_PICTURE)
            .ok_or_else(|| format!("FBP picture {SPLASH_DOWN_PICTURE} is unavailable"))?;
        let title = sprites
            .decode_frame(SPLASH_TITLE_SPRITE, 0)
            .ok_or_else(|| format!("MGO sprite {SPLASH_TITLE_SPRITE} is unavailable"))?;
        let crane_frame_count = sprites
            .character_frame_count(SPLASH_CRANE_SPRITE)
            .ok_or_else(|| format!("MGO sprite {SPLASH_CRANE_SPRITE} is unavailable"))?;
        if crane_frame_count < CRANE_ANIMATION_FRAMES {
            return Err(format!(
                "MGO sprite {SPLASH_CRANE_SPRITE} has only {crane_frame_count} frames"
            ));
        }
        let crane_frames = (0..crane_frame_count.min(CRANE_ANIMATION_FRAMES + 1))
            .map(|frame| {
                sprites
                    .decode_frame(SPLASH_CRANE_SPRITE, frame)
                    .ok_or_else(|| {
                        format!("MGO sprite {SPLASH_CRANE_SPRITE} frame {frame} is invalid")
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(1, |duration| duration.as_secs() as u32)
            .max(1);
        Ok(Self::new(
            trademark_frame_count,
            trademark_canvas,
            splash_up,
            splash_down,
            title,
            crane_frames,
            seed,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn new(
        trademark_frame_count: usize,
        trademark_canvas: Vec<u8>,
        splash_up: Vec<u8>,
        splash_down: Vec<u8>,
        title: RleBitmap,
        crane_frames: Vec<RleBitmap>,
        seed: u32,
    ) -> Self {
        let mut random = PalRandom::new(seed);
        let cranes = std::array::from_fn(|_| Crane {
            x: random.range(300, 600),
            y: random.range(0, 80),
            frame: random.range(0, i32::try_from(crane_frames.len().min(9) - 1).unwrap_or(0))
                as usize,
        });
        let phase = if trademark_frame_count > 1 {
            OpeningIntroPhase::Trademark {
                next_frame: 1,
                frame_accumulator_ms: 0,
            }
        } else {
            OpeningIntroPhase::TrademarkHold {
                remaining_ms: TRADEMARK_HOLD_MS,
            }
        };
        Self {
            phase,
            trademark_frame_count,
            trademark_canvas,
            splash_up,
            splash_down,
            title,
            crane_frames,
            cranes,
            splash_image_position: FBP_HEIGHT,
            splash_title_height: 0,
            splash_frame: 0,
            splash_frame_accumulator_ms: 0,
            splash_palette_ms: 0,
        }
    }

    pub(super) fn update(
        &mut self,
        elapsed_ms: u32,
        input: GameInput,
        rng: &RngArchive,
    ) -> Result<(bool, OpeningIntroAction), String> {
        let phase = std::mem::replace(&mut self.phase, OpeningIntroPhase::Finished);
        let (next_phase, changed, action) = match phase {
            OpeningIntroPhase::Trademark {
                mut next_frame,
                mut frame_accumulator_ms,
            } => {
                frame_accumulator_ms = frame_accumulator_ms.saturating_add(elapsed_ms);
                let animation = rng
                    .animation(TRADEMARK_ANIMATION)
                    .ok_or_else(|| format!("RNG animation {TRADEMARK_ANIMATION} disappeared"))?;
                let mut changed = false;
                while frame_accumulator_ms >= TRADEMARK_FRAME_MS
                    && next_frame < self.trademark_frame_count
                {
                    animation
                        .apply_frame(next_frame, &mut self.trademark_canvas)
                        .ok_or_else(|| {
                            format!(
                                "RNG animation {TRADEMARK_ANIMATION} frame {next_frame} is invalid"
                            )
                        })?;
                    next_frame += 1;
                    frame_accumulator_ms -= TRADEMARK_FRAME_MS;
                    changed = true;
                }
                let next = if next_frame == self.trademark_frame_count {
                    OpeningIntroPhase::TrademarkHold {
                        remaining_ms: TRADEMARK_HOLD_MS,
                    }
                } else {
                    OpeningIntroPhase::Trademark {
                        next_frame,
                        frame_accumulator_ms,
                    }
                };
                (next, changed, OpeningIntroAction::None)
            }
            OpeningIntroPhase::TrademarkHold { remaining_ms } => {
                let remaining_ms = remaining_ms.saturating_sub(elapsed_ms);
                let next = if remaining_ms == 0 {
                    OpeningIntroPhase::TrademarkFadeOut { elapsed_ms: 0 }
                } else {
                    OpeningIntroPhase::TrademarkHold { remaining_ms }
                };
                (next, false, OpeningIntroAction::None)
            }
            OpeningIntroPhase::TrademarkFadeOut {
                elapsed_ms: fade_ms,
            } => {
                let fade_ms = fade_ms.saturating_add(elapsed_ms).min(SCREEN_FADE_MS);
                if fade_ms == SCREEN_FADE_MS {
                    self.advance_splash_frame();
                    (
                        OpeningIntroPhase::Splash,
                        true,
                        OpeningIntroAction::PlayTitleMusic,
                    )
                } else {
                    (
                        OpeningIntroPhase::TrademarkFadeOut {
                            elapsed_ms: fade_ms,
                        },
                        true,
                        OpeningIntroAction::None,
                    )
                }
            }
            OpeningIntroPhase::Splash => {
                if input.confirm || input.cancel {
                    self.splash_title_height = usize::from(self.title.height);
                    let remaining_palette_ms =
                        SPLASH_PALETTE_FADE_MS.saturating_sub(self.splash_palette_ms);
                    if remaining_palette_ms == 0 {
                        (
                            OpeningIntroPhase::SplashFadeOut { elapsed_ms: 0 },
                            true,
                            OpeningIntroAction::StopTitleMusic,
                        )
                    } else {
                        let accelerate_ms = remaining_palette_ms
                            .div_ceil(SPLASH_SKIP_VIRTUAL_MS)
                            .saturating_mul(SPLASH_SKIP_STEP_MS);
                        (
                            OpeningIntroPhase::SplashCompleting {
                                elapsed_ms: 0,
                                accelerate_ms,
                                total_ms: accelerate_ms.saturating_add(SPLASH_SKIP_HOLD_MS),
                                start_palette_ms: self.splash_palette_ms,
                            },
                            true,
                            OpeningIntroAction::None,
                        )
                    }
                } else {
                    let previous_palette_ms = self.splash_palette_ms;
                    self.splash_palette_ms = self
                        .splash_palette_ms
                        .saturating_add(elapsed_ms)
                        .min(SPLASH_PALETTE_FADE_MS);
                    self.splash_frame_accumulator_ms =
                        self.splash_frame_accumulator_ms.saturating_add(elapsed_ms);
                    let mut changed = self.splash_palette_ms != previous_palette_ms;
                    while self.splash_frame_accumulator_ms >= SPLASH_FRAME_MS {
                        self.splash_frame_accumulator_ms -= SPLASH_FRAME_MS;
                        self.advance_splash_frame();
                        changed = true;
                    }
                    (OpeningIntroPhase::Splash, changed, OpeningIntroAction::None)
                }
            }
            OpeningIntroPhase::SplashCompleting {
                elapsed_ms: complete_ms,
                accelerate_ms,
                total_ms,
                start_palette_ms,
            } => {
                let complete_ms = complete_ms.saturating_add(elapsed_ms).min(total_ms);
                if accelerate_ms == 0 || complete_ms >= accelerate_ms {
                    self.splash_palette_ms = SPLASH_PALETTE_FADE_MS;
                } else {
                    let remaining = SPLASH_PALETTE_FADE_MS - start_palette_ms;
                    self.splash_palette_ms = start_palette_ms
                        .saturating_add(remaining.saturating_mul(complete_ms) / accelerate_ms);
                }
                if complete_ms == total_ms {
                    (
                        OpeningIntroPhase::SplashFadeOut { elapsed_ms: 0 },
                        true,
                        OpeningIntroAction::StopTitleMusic,
                    )
                } else {
                    (
                        OpeningIntroPhase::SplashCompleting {
                            elapsed_ms: complete_ms,
                            accelerate_ms,
                            total_ms,
                            start_palette_ms,
                        },
                        true,
                        OpeningIntroAction::None,
                    )
                }
            }
            OpeningIntroPhase::SplashFadeOut {
                elapsed_ms: fade_ms,
            } => {
                let fade_ms = fade_ms.saturating_add(elapsed_ms).min(SCREEN_FADE_MS);
                if fade_ms == SCREEN_FADE_MS {
                    (
                        OpeningIntroPhase::Finished,
                        true,
                        OpeningIntroAction::Finished,
                    )
                } else {
                    (
                        OpeningIntroPhase::SplashFadeOut {
                            elapsed_ms: fade_ms,
                        },
                        true,
                        OpeningIntroAction::None,
                    )
                }
            }
            OpeningIntroPhase::Finished => {
                (OpeningIntroPhase::Finished, false, OpeningIntroAction::None)
            }
        };
        self.phase = next_phase;
        Ok((changed, action))
    }

    pub(super) fn render(
        &self,
        renderer: &mut Renderer,
        palettes: &[PaletteSet],
    ) -> Result<(), String> {
        if self.is_trademark_phase() {
            let palette = palettes
                .get(TRADEMARK_PALETTE)
                .ok_or_else(|| format!("palette {TRADEMARK_PALETTE} is unavailable"))?
                .select(false);
            renderer.set_palette(palette);
            if !renderer.replace_with_indexed(&self.trademark_canvas) {
                return Err("trademark screen has the wrong size".to_owned());
            }
            renderer.apply_brightness(self.brightness());
            return Ok(());
        }

        let palette = palettes
            .get(SPLASH_PALETTE)
            .ok_or_else(|| format!("palette {SPLASH_PALETTE} is unavailable"))?
            .select(false);
        renderer.set_palette(palette);
        let splash = self.compose_splash_background();
        if !renderer.replace_with_indexed(&splash) {
            return Err("splash screen has the wrong size".to_owned());
        }
        for crane in &self.cranes {
            let Some(frame) = self.crane_frames.get(crane.frame) else {
                continue;
            };
            renderer.blit_rle(frame, crane.x + 1, crane.y);
        }
        draw_rle_rows(
            renderer,
            palette,
            &self.title,
            255,
            10,
            self.splash_title_height,
        );
        renderer.apply_brightness(self.brightness());
        Ok(())
    }

    fn is_trademark_phase(&self) -> bool {
        matches!(
            self.phase,
            OpeningIntroPhase::Trademark { .. }
                | OpeningIntroPhase::TrademarkHold { .. }
                | OpeningIntroPhase::TrademarkFadeOut { .. }
        )
    }

    fn brightness(&self) -> u8 {
        match self.phase {
            OpeningIntroPhase::Trademark { .. } | OpeningIntroPhase::TrademarkHold { .. } => 64,
            OpeningIntroPhase::TrademarkFadeOut { elapsed_ms }
            | OpeningIntroPhase::SplashFadeOut { elapsed_ms } => {
                ((SCREEN_FADE_MS - elapsed_ms).saturating_mul(64) / SCREEN_FADE_MS) as u8
            }
            OpeningIntroPhase::Splash | OpeningIntroPhase::SplashCompleting { .. } => {
                (self.splash_palette_ms.saturating_mul(64) / SPLASH_PALETTE_FADE_MS) as u8
            }
            OpeningIntroPhase::Finished => 0,
        }
    }

    fn advance_splash_frame(&mut self) {
        if self.splash_image_position > 1 {
            self.splash_image_position -= 1;
        }
        let odd_animation_frame = self.splash_frame & 1 != 0;
        for crane in &mut self.cranes {
            if odd_animation_frame {
                crane.frame = (crane.frame + 1) % CRANE_ANIMATION_FRAMES;
            }
            if self.splash_image_position > 1 && self.splash_image_position & 1 != 0 {
                crane.y += 1;
            }
            crane.x -= 1;
        }
        self.splash_frame = self.splash_frame.wrapping_add(1);
        self.splash_title_height =
            (self.splash_title_height + 1).min(usize::from(self.title.height));
    }

    fn compose_splash_background(&self) -> Vec<u8> {
        let mut output = vec![0; FBP_PIXELS];
        let position = self.splash_image_position.min(FBP_HEIGHT);
        let upper_rows = FBP_HEIGHT - position;
        let upper_bytes = upper_rows * FBP_WIDTH;
        let upper_start = position * FBP_WIDTH;
        output[..upper_bytes]
            .copy_from_slice(&self.splash_up[upper_start..upper_start + upper_bytes]);
        let lower_bytes = position * FBP_WIDTH;
        output[upper_bytes..upper_bytes + lower_bytes]
            .copy_from_slice(&self.splash_down[..lower_bytes]);
        output
    }
}

fn draw_rle_rows(
    renderer: &mut Renderer,
    palette: &Palette,
    bitmap: &RleBitmap,
    x: i32,
    y: i32,
    visible_rows: usize,
) {
    let width = usize::from(bitmap.width);
    for row in 0..visible_rows.min(usize::from(bitmap.height)) {
        for column in 0..width {
            let source = row * width + column;
            if !bitmap.opaque[source] {
                continue;
            }
            let (r, g, b) = palette.get_rgb(bitmap.pixels[source]);
            renderer.put_rgba(x + column as i32, y + row as i32, [r, g, b, 255]);
        }
    }
}

struct PalRandom {
    seed: u32,
}

impl PalRandom {
    fn new(seed: u32) -> Self {
        Self { seed }
    }

    fn range(&mut self, from: i32, to: i32) -> i32 {
        if to <= from {
            return from;
        }
        self.seed = self
            .seed
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        let random = (self.seed >> 1).wrapping_add(1_073_741_824);
        let width = u32::try_from(to - from + 1).unwrap_or(1);
        from + i32::try_from(random % width).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pal_assets::palette::{Palette, PaletteColor};

    fn bitmap(width: u16, height: u16, index: u8) -> RleBitmap {
        let count = usize::from(width) * usize::from(height);
        RleBitmap {
            width,
            height,
            pixels: vec![index; count],
            opaque: vec![true; count],
        }
    }

    fn intro(phase: OpeningIntroPhase) -> OpeningIntro {
        OpeningIntro {
            phase,
            trademark_frame_count: 1,
            trademark_canvas: vec![1; RNG_FRAME_PIXELS],
            splash_up: vec![1; FBP_PIXELS],
            splash_down: vec![2; FBP_PIXELS],
            title: bitmap(2, 2, 3),
            crane_frames: vec![bitmap(1, 1, 4); CRANE_ANIMATION_FRAMES],
            cranes: [Crane {
                x: 400,
                y: 20,
                frame: 0,
            }; CRANE_COUNT],
            splash_image_position: 100,
            splash_title_height: 1,
            splash_frame: 0,
            splash_frame_accumulator_ms: 0,
            splash_palette_ms: SPLASH_PALETTE_FADE_MS,
        }
    }

    #[test]
    fn splash_composes_upper_and_lower_pictures_at_the_original_split() {
        let intro = intro(OpeningIntroPhase::Splash);
        let screen = intro.compose_splash_background();

        assert!(screen[..100 * FBP_WIDTH].iter().all(|&pixel| pixel == 1));
        assert!(screen[100 * FBP_WIDTH..].iter().all(|&pixel| pixel == 2));
    }

    #[test]
    fn splash_waits_for_input_then_stops_music_and_fades_out() {
        let mut intro = intro(OpeningIntroPhase::Splash);
        let empty_rng = RngArchive::new(&[8, 0, 0, 0, 8, 0, 0, 0]).unwrap();
        let input = GameInput {
            direction: None,
            direction_pressed: None,
            confirm: true,
            cancel: false,
        };

        let (_, action) = intro.update(50, input, &empty_rng).unwrap();
        assert_eq!(action, OpeningIntroAction::StopTitleMusic);
        assert!(matches!(
            intro.phase,
            OpeningIntroPhase::SplashFadeOut { .. }
        ));
        let (_, action) = intro
            .update(SCREEN_FADE_MS, GameInput::default(), &empty_rng)
            .unwrap();
        assert_eq!(action, OpeningIntroAction::Finished);
        assert_eq!(intro.brightness(), 0);
    }

    #[test]
    fn splash_render_applies_palette_and_title_reveal() {
        let intro = intro(OpeningIntroPhase::Splash);
        let mut colors = [PaletteColor { r: 0, g: 0, b: 0 }; 256];
        colors[1] = PaletteColor { r: 63, g: 0, b: 0 };
        colors[2] = PaletteColor { r: 0, g: 63, b: 0 };
        colors[3] = PaletteColor { r: 0, g: 0, b: 63 };
        let set = PaletteSet {
            day: Palette { colors },
            night: None,
        };
        let palettes = vec![set.clone(), set];
        let mut renderer = Renderer::new(Palette::default(), FBP_WIDTH, FBP_HEIGHT);

        intro.render(&mut renderer, &palettes).unwrap();

        assert_eq!(&renderer.screen()[0..4], &[252, 0, 0, 255]);
        let lower = 150 * FBP_WIDTH * 4;
        assert_eq!(&renderer.screen()[lower..lower + 4], &[0, 252, 0, 255]);
        let title = (10 * FBP_WIDTH + 255) * 4;
        assert_eq!(&renderer.screen()[title..title + 4], &[0, 0, 252, 255]);
    }
}
