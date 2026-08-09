use pal_assets::fbp::{FbpArchive, FBP_HEIGHT, FBP_PIXELS, FBP_WIDTH};
use pal_assets::palette::{Palette, PaletteColor, PaletteSet};
use pal_assets::rle::RleBitmap;
use pal_assets::rng::{RngArchive, RNG_FRAME_PIXELS};
use pal_core::game::UPDATE_INTERVAL_MS;
use pal_core::role::RoleSprites;
use pal_core::script::ScriptVisual;
use std::collections::HashMap;

use crate::renderer::Renderer;

const BATTLE_SCREEN_SWITCH_GROUPS: usize = 6;
const BATTLE_SCREEN_SWITCH_STEP_MS: u64 = 60;
const BATTLE_SCREEN_SWITCH_TOTAL_MS: u64 =
    BATTLE_SCREEN_SWITCH_STEP_MS * BATTLE_SCREEN_SWITCH_GROUPS as u64;
const BATTLE_SCREEN_SWITCH_ORDER: [usize; BATTLE_SCREEN_SWITCH_GROUPS] = [0, 3, 1, 5, 2, 4];

pub(super) struct VisualState {
    pending: Option<ScriptVisual>,
    battle_transition_pending: bool,
    effect: Option<VisualEffect>,
    palette_index: usize,
    night_palette: bool,
    indexed_screen: Option<Vec<u8>>,
    rgba_screen: Option<Vec<u8>>,
    backup_screen: Option<Vec<u8>>,
    brightness: u8,
    tint_color: (u8, u8, u8),
    tint_amount: u8,
    needs_scene_fade_in: bool,
    fade_screen_frozen: bool,
    screen_wave: i32,
    wave_progression: i16,
    wave_phase: i16,
    shake_remaining: u16,
    shake_level: u16,
    ending_effect_sprite: u16,
}

enum VisualEffect {
    Fade {
        start: u8,
        end: u8,
        progress: u32,
        total: u32,
        update_scene: bool,
    },
    Tint {
        color: (u8, u8, u8),
        start: u8,
        end: u8,
        progress: u32,
        total: u32,
    },
    PaletteBlend {
        from: Box<Palette>,
        to: Box<Palette>,
        progress: u32,
        total: u32,
    },
    CrossFade {
        previous: Vec<u8>,
        progress: u32,
        total: u32,
        draw_ending_sprite: bool,
    },
    ScreenSwitch {
        previous: Vec<u8>,
        progress: u32,
        total: u32,
    },
    Scroll {
        previous: Vec<u8>,
        progress: u32,
        total: u32,
    },
    Rng(RngPlayback),
    Ending(EndingAnimation),
}

struct RngPlayback {
    animation: usize,
    next_frame: usize,
    end_frame: Option<usize>,
    ticks_per_frame: u16,
    ticks_until_frame: u16,
    fade_in_progress: Option<u32>,
}

struct EndingAnimation {
    upper: Vec<u8>,
    lower: Vec<u8>,
    beast: Vec<RleBitmap>,
    girl: Vec<RleBitmap>,
    frame: u16,
}

impl VisualState {
    pub(super) fn new() -> Self {
        Self {
            pending: None,
            battle_transition_pending: false,
            effect: None,
            palette_index: 0,
            night_palette: false,
            indexed_screen: None,
            rgba_screen: None,
            backup_screen: None,
            brightness: 64,
            tint_color: (0, 0, 0),
            tint_amount: 0,
            needs_scene_fade_in: false,
            fade_screen_frozen: false,
            screen_wave: 0,
            wave_progression: 0,
            wave_phase: 0,
            shake_remaining: 0,
            shake_level: 0,
            ending_effect_sprite: 0,
        }
    }

    pub(super) fn queue(&mut self, command: ScriptVisual) -> bool {
        if self.pending.is_some()
            || self.battle_transition_pending
            || self.effect.is_some()
            || self.shake_remaining != 0
        {
            return false;
        }
        self.pending = Some(command);
        true
    }

    pub(super) fn is_blocking(&self) -> bool {
        self.pending.is_some()
            || self.battle_transition_pending
            || self.effect.is_some()
            || self.shake_remaining != 0
    }

    pub(super) fn queue_battle_transition(&mut self) -> bool {
        if self.is_blocking() {
            return false;
        }
        self.battle_transition_pending = true;
        true
    }

    /// Whether Classic's fixed battle-entry screen switch still owns the frame.
    pub(super) fn battle_transition_active(&self) -> bool {
        self.battle_transition_pending
            || matches!(self.effect, Some(VisualEffect::ScreenSwitch { .. }))
    }

    pub(super) fn needs_update(&self) -> bool {
        self.is_blocking() || self.screen_wave != 0
    }

    pub(super) fn scene_update_due(&self) -> bool {
        matches!(self.pending, Some(ScriptVisual::FadeSceneWithUpdate { .. }))
            || matches!(
                &self.effect,
                Some(VisualEffect::Fade {
                    update_scene: true,
                    progress,
                    ..
                }) if *progress % duration_ticks(100) == 0
            )
    }

    /// Keep the next scene black until its enter script has finished, then let
    /// the normal implicit scene fade-in reveal it.
    pub(super) fn prepare_scene_fade_in(&mut self) {
        self.pending = None;
        self.battle_transition_pending = false;
        self.effect = None;
        self.indexed_screen = None;
        self.rgba_screen = None;
        self.brightness = 0;
        self.needs_scene_fade_in = true;
        self.fade_screen_frozen = false;
        self.ending_effect_sprite = 0;
    }

    /// Start the implicit fade performed by the original scene renderer after
    /// opcode 0x0050. Explicit script effects get the first chance to consume
    /// the pending fade; this path runs only once script execution pauses.
    pub(super) fn queue_automatic_scene_fade_in(&mut self) -> bool {
        if !self.needs_scene_fade_in || self.is_blocking() {
            return false;
        }
        self.needs_scene_fade_in = false;
        self.indexed_screen = None;
        self.rgba_screen = None;
        self.ending_effect_sprite = 0;
        self.fade_screen_frozen = false;
        self.pending = Some(ScriptVisual::FadeIn { speed: 1 });
        true
    }

    pub(super) fn restore_original_environment(&mut self, night: bool, screen_wave: u16) {
        self.pending = None;
        self.battle_transition_pending = false;
        self.effect = None;
        self.palette_index = 0;
        self.night_palette = night;
        self.indexed_screen = None;
        self.rgba_screen = None;
        self.backup_screen = None;
        self.brightness = 64;
        self.tint_amount = 0;
        self.needs_scene_fade_in = false;
        self.fade_screen_frozen = false;
        self.screen_wave = i32::from(screen_wave);
        self.wave_progression = 0;
        self.wave_phase = 0;
        self.shake_remaining = 0;
        self.shake_level = 0;
        self.ending_effect_sprite = 0;
    }

    pub(super) fn night_palette(&self) -> bool {
        self.night_palette
    }

    pub(super) fn screen_wave(&self) -> u16 {
        u16::try_from(self.screen_wave.max(0)).unwrap_or(u16::MAX)
    }

    pub(super) fn update(
        &mut self,
        current_screen: &[u8],
        palettes: &[PaletteSet],
        fbp: &FbpArchive,
        rng: &RngArchive,
        role_sprites: &RoleSprites,
    ) -> Result<bool, String> {
        let mut changed = self.start_pending(current_screen, palettes, fbp, rng, role_sprites)?;

        if let Some(effect) = self.effect.take() {
            self.effect = self.advance_effect(effect, rng)?;
            changed = true;
        }
        if self.shake_remaining > 0 {
            self.shake_remaining -= 1;
            changed = true;
        }
        if self.screen_wave != 0 {
            self.screen_wave += i32::from(self.wave_progression);
            if !(1..256).contains(&self.screen_wave) {
                self.screen_wave = 0;
                self.wave_progression = 0;
            }
            self.wave_phase = self.wave_phase.wrapping_add(1);
            changed = true;
        }
        Ok(changed)
    }

    /// Start a command queued during the current script tick before world state
    /// is rendered again. Fade-out can therefore capture the frame that was
    /// actually visible when the trigger began.
    pub(super) fn start_pending(
        &mut self,
        current_screen: &[u8],
        palettes: &[PaletteSet],
        fbp: &FbpArchive,
        rng: &RngArchive,
        role_sprites: &RoleSprites,
    ) -> Result<bool, String> {
        if self.battle_transition_pending {
            self.battle_transition_pending = false;
            self.effect = Some(VisualEffect::ScreenSwitch {
                previous: current_screen.to_vec(),
                progress: 0,
                total: duration_ticks(BATTLE_SCREEN_SWITCH_TOTAL_MS),
            });
            return Ok(true);
        }
        let Some(command) = self.pending.take() else {
            return Ok(false);
        };
        self.start(command, current_screen, palettes, fbp, rng, role_sprites)?;
        Ok(true)
    }

    fn start(
        &mut self,
        command: ScriptVisual,
        current_screen: &[u8],
        palettes: &[PaletteSet],
        fbp: &FbpArchive,
        rng: &RngArchive,
        role_sprites: &RoleSprites,
    ) -> Result<(), String> {
        if !matches!(
            command,
            ScriptVisual::FadeOut { .. } | ScriptVisual::FadeIn { .. }
        ) {
            self.fade_screen_frozen = false;
        }
        match command {
            ScriptVisual::Shake { frames, level } => {
                self.shake_remaining = frames;
                self.shake_level = level;
            }
            ScriptVisual::PlayRng {
                animation,
                start_frame,
                end_frame,
                speed,
            } => {
                let fade_in_after_first_frame = self.needs_scene_fade_in;
                self.needs_scene_fade_in = false;
                let animation_index = usize::from(animation);
                let animation_data = rng
                    .animation(animation_index)
                    .ok_or_else(|| format!("RNG animation {animation} is unavailable"))?;
                if usize::from(start_frame) >= animation_data.frame_count() {
                    return Err(format!(
                        "RNG animation {animation} has no frame {start_frame}"
                    ));
                }
                self.rgba_screen = None;
                self.indexed_screen =
                    Some(rgba_to_indices(current_screen, &self.palette(palettes)?)?);
                let ticks_per_frame = 20u16.div_ceil(speed.max(1)).max(1);
                self.effect = Some(VisualEffect::Rng(RngPlayback {
                    animation: animation_index,
                    next_frame: usize::from(start_frame),
                    end_frame: end_frame.map(usize::from),
                    ticks_per_frame,
                    ticks_until_frame: 0,
                    fade_in_progress: fade_in_after_first_frame.then_some(0),
                }));
            }
            ScriptVisual::FadeToRed => {
                self.effect = Some(VisualEffect::Tint {
                    color: (252, 0, 0),
                    start: self.tint_amount,
                    end: 64,
                    progress: 0,
                    total: duration_ticks(32 * 75),
                });
            }
            ScriptVisual::FadeOut { speed } => {
                self.needs_scene_fade_in = true;
                self.indexed_screen = None;
                self.rgba_screen = Some(current_screen.to_vec());
                self.fade_screen_frozen = true;
                self.effect = Some(VisualEffect::Fade {
                    start: self.brightness,
                    end: 0,
                    progress: 0,
                    total: standard_fade_ticks(speed),
                    update_scene: false,
                });
            }
            ScriptVisual::FadeIn { speed } => {
                self.needs_scene_fade_in = false;
                self.brightness = 0;
                self.effect = Some(VisualEffect::Fade {
                    start: 0,
                    end: 64,
                    progress: 0,
                    total: standard_fade_ticks(speed),
                    update_scene: false,
                });
            }
            ScriptVisual::SetNightPalette { night } => self.night_palette = night,
            ScriptVisual::SetScreenWave { level, progression } => {
                self.screen_wave = i32::from(level);
                self.wave_progression = progression;
            }
            ScriptVisual::ShowFbp { index, fade } => {
                self.ending_effect_sprite = 0;
                self.start_fbp(index, fade, current_screen, fbp)?;
            }
            ScriptVisual::ToggleDayNightPalette { update_scene } => {
                let from = self.palette(palettes)?;
                self.night_palette = !self.night_palette;
                let to = self.palette(palettes)?;
                self.effect = Some(VisualEffect::PaletteBlend {
                    from: Box::new(from),
                    to: Box::new(to),
                    progress: 0,
                    total: palette_fade_ticks(update_scene),
                });
            }
            ScriptVisual::SetPalette { index } => {
                let index = usize::from(index);
                if palettes.get(index).is_none() {
                    return Err(format!("palette {index} is unavailable"));
                }
                self.palette_index = index;
                self.night_palette = false;
            }
            ScriptVisual::FadeColor {
                color,
                from_color,
                delay,
            } => {
                self.needs_scene_fade_in = false;
                let color = self.palette(palettes)?.get_rgb(color);
                let (start, end) = if from_color { (64, 0) } else { (0, 64) };
                self.tint_color = color;
                self.tint_amount = start;
                self.effect = Some(VisualEffect::Tint {
                    color,
                    start,
                    end,
                    progress: 0,
                    total: color_fade_ticks(delay),
                });
            }
            ScriptVisual::RestoreScreen => {
                let backup = self
                    .backup_screen
                    .clone()
                    .ok_or_else(|| "screen backup is unavailable".to_owned())?;
                self.indexed_screen = None;
                self.rgba_screen = Some(backup);
            }
            ScriptVisual::FadeSceneWithUpdate { step } => {
                self.needs_scene_fade_in = step < 0;
                self.indexed_screen = None;
                self.rgba_screen = None;
                let magnitude = u32::from(step.unsigned_abs().max(1));
                let (start, end) = if step < 0 { (64, 0) } else { (0, 64) };
                self.brightness = start;
                self.effect = Some(VisualEffect::Fade {
                    start,
                    end,
                    progress: 0,
                    total: 64u32
                        .div_ceil(magnitude)
                        .saturating_mul(duration_ticks(100)),
                    update_scene: true,
                });
            }
            ScriptVisual::FadeToCurrentScene { speed } => {
                self.needs_scene_fade_in = false;
                self.indexed_screen = None;
                self.rgba_screen = None;
                self.effect = Some(VisualEffect::CrossFade {
                    previous: current_screen.to_vec(),
                    progress: 0,
                    total: screen_fade_ticks(speed),
                    draw_ending_sprite: false,
                });
            }
            ScriptVisual::ScrollFbp { index, speed } => {
                self.needs_scene_fade_in = false;
                let target = fbp
                    .frame(usize::from(index))
                    .ok_or_else(|| format!("FBP picture {index} is unavailable"))?;
                self.rgba_screen = None;
                self.indexed_screen = Some(target);
                self.effect = Some(VisualEffect::Scroll {
                    previous: current_screen.to_vec(),
                    progress: 0,
                    total: fbp_scroll_ticks(speed),
                });
            }
            ScriptVisual::ShowFbpWithSprite {
                index,
                sprite,
                fade,
            } => {
                if let Some(sprite) = sprite {
                    if sprite != 0
                        && role_sprites
                            .character_frame_count(usize::from(sprite))
                            .is_none()
                    {
                        return Err(format!("ending effect sprite {sprite} is unavailable"));
                    }
                    self.ending_effect_sprite = sprite;
                }
                self.start_fbp(index, fade, current_screen, fbp)?;
            }
            ScriptVisual::PlayEndingAnimation => {
                self.needs_scene_fade_in = false;
                let upper = fbp
                    .frame(61)
                    .ok_or_else(|| "ending FBP picture 61 is unavailable".to_owned())?;
                let lower = fbp
                    .frame(62)
                    .ok_or_else(|| "ending FBP picture 62 is unavailable".to_owned())?;
                let beast = load_ending_frames(role_sprites, 571, 2, "beast")?;
                let girl = load_ending_frames(role_sprites, 572, 4, "girl")?;
                self.rgba_screen = None;
                self.indexed_screen = Some(vec![0; FBP_PIXELS]);
                self.ending_effect_sprite = 0;
                self.screen_wave = 0;
                self.wave_progression = 0;
                self.wave_phase = 0;
                self.effect = Some(VisualEffect::Ending(EndingAnimation {
                    upper,
                    lower,
                    beast,
                    girl,
                    frame: 0,
                }));
            }
            ScriptVisual::BackupScreen => self.backup_screen = Some(current_screen.to_vec()),
        }
        Ok(())
    }

    fn start_fbp(
        &mut self,
        index: u16,
        fade: u16,
        current_screen: &[u8],
        fbp: &FbpArchive,
    ) -> Result<(), String> {
        let target = match fbp.frame(usize::from(index)) {
            Some(target) => target,
            None if index == u16::MAX => vec![0; FBP_PIXELS],
            None => return Err(format!("FBP picture {index} is unavailable")),
        };
        self.rgba_screen = None;
        self.indexed_screen = Some(target);
        if fade != 0 {
            self.effect = Some(VisualEffect::CrossFade {
                previous: current_screen.to_vec(),
                progress: 0,
                total: fbp_fade_ticks(fade),
                draw_ending_sprite: true,
            });
        }
        Ok(())
    }

    fn advance_effect(
        &mut self,
        mut effect: VisualEffect,
        rng: &RngArchive,
    ) -> Result<Option<VisualEffect>, String> {
        match &mut effect {
            VisualEffect::Fade {
                start,
                end,
                progress,
                total,
                ..
            } => {
                *progress = progress.saturating_add(1).min(*total);
                self.brightness = interpolate(*start, *end, *progress, *total);
                if progress == total {
                    if *end == 64 && self.fade_screen_frozen {
                        self.rgba_screen = None;
                        self.fade_screen_frozen = false;
                    }
                    return Ok(None);
                }
            }
            VisualEffect::Tint {
                color,
                start,
                end,
                progress,
                total,
            } => {
                *progress = progress.saturating_add(1).min(*total);
                self.tint_color = *color;
                self.tint_amount = interpolate(*start, *end, *progress, *total);
                if progress == total {
                    return Ok(None);
                }
            }
            VisualEffect::PaletteBlend {
                progress, total, ..
            }
            | VisualEffect::CrossFade {
                progress, total, ..
            }
            | VisualEffect::Scroll {
                progress, total, ..
            } => {
                *progress = progress.saturating_add(1).min(*total);
                if progress == total {
                    return Ok(None);
                }
            }
            VisualEffect::ScreenSwitch {
                progress, total, ..
            } => {
                *progress = progress.saturating_add(1).min(*total);
                if *progress == *total {
                    return Ok(None);
                }
            }
            VisualEffect::Rng(playback) => {
                if playback
                    .fade_in_progress
                    .is_some_and(|progress| progress > 0)
                {
                    let total = standard_fade_ticks(1);
                    let progress = playback
                        .fade_in_progress
                        .unwrap_or_default()
                        .saturating_add(1)
                        .min(total);
                    self.brightness = interpolate(0, 64, progress, total);
                    if progress == total {
                        playback.fade_in_progress = None;
                        playback.ticks_until_frame = 0;
                    } else {
                        playback.fade_in_progress = Some(progress);
                    }
                    return Ok(Some(effect));
                }
                if playback.ticks_until_frame > 0 {
                    playback.ticks_until_frame -= 1;
                    return Ok(Some(effect));
                }
                let animation = rng
                    .animation(playback.animation)
                    .ok_or_else(|| format!("RNG animation {} disappeared", playback.animation))?;
                if playback.next_frame >= animation.frame_count()
                    || playback
                        .end_frame
                        .is_some_and(|end| playback.next_frame > end)
                {
                    return Ok(None);
                }
                if animation
                    .compressed_frame(playback.next_frame)
                    .is_some_and(<[u8]>::is_empty)
                {
                    return Ok(None);
                }
                animation
                    .apply_frame(
                        playback.next_frame,
                        self.indexed_screen
                            .as_mut()
                            .ok_or_else(|| "RNG canvas is unavailable".to_owned())?,
                    )
                    .ok_or_else(|| {
                        format!(
                            "RNG animation {} frame {} is invalid",
                            playback.animation, playback.next_frame
                        )
                    })?;
                playback.next_frame += 1;
                playback.ticks_until_frame = playback.ticks_per_frame.saturating_sub(1);
                if playback.fade_in_progress == Some(0) {
                    let total = standard_fade_ticks(1);
                    let progress = 1.min(total);
                    self.brightness = interpolate(0, 64, progress, total);
                    playback.fade_in_progress = (progress < total).then_some(progress);
                    return Ok(Some(effect));
                }
                if playback
                    .end_frame
                    .is_some_and(|end| playback.next_frame > end)
                    || playback.next_frame >= animation.frame_count()
                {
                    return Ok(None);
                }
            }
            VisualEffect::Ending(ending) => {
                let screen = self
                    .indexed_screen
                    .as_mut()
                    .ok_or_else(|| "ending canvas is unavailable".to_owned())?;
                compose_ending_frame(ending, screen)?;
                if ending.frame == 399 {
                    self.screen_wave = 0;
                    self.wave_progression = 0;
                    self.wave_phase = 0;
                    return Ok(None);
                }
                ending.frame += 1;
            }
        }
        Ok(Some(effect))
    }

    pub(super) fn palette(&self, palettes: &[PaletteSet]) -> Result<Palette, String> {
        if let Some(VisualEffect::PaletteBlend {
            from,
            to,
            progress,
            total,
        }) = &self.effect
        {
            return Ok(blend_palette(from, to, *progress, *total));
        }
        palettes
            .get(self.palette_index)
            .map(|set| set.select(self.night_palette).clone())
            .ok_or_else(|| format!("palette {} is unavailable", self.palette_index))
    }

    /// Returns true when a saved full-screen image replaced normal world rendering.
    pub(super) fn render_override(&self, renderer: &mut Renderer) -> bool {
        if let Some(indices) = &self.indexed_screen {
            return renderer.replace_with_indexed(indices);
        }
        if let Some(rgba) = &self.rgba_screen {
            return renderer.replace_screen(rgba);
        }
        false
    }

    pub(super) fn apply_post_effects(&self, renderer: &mut Renderer, role_sprites: &RoleSprites) {
        let mut ending_sprite_tick = None;
        match &self.effect {
            Some(VisualEffect::CrossFade {
                previous,
                progress,
                total,
                draw_ending_sprite,
            }) => {
                renderer.blend_from(previous, progress_64(*progress, *total));
                if *draw_ending_sprite {
                    ending_sprite_tick = Some(*progress);
                }
            }
            Some(VisualEffect::ScreenSwitch {
                previous,
                progress,
                total,
            }) => {
                let revealed_groups = screen_switch_revealed_groups(*progress, *total);
                apply_screen_switch(renderer.screen_mut(), previous, revealed_groups);
            }
            Some(VisualEffect::Scroll {
                previous,
                progress,
                total,
            }) => {
                let rows = (*progress as usize)
                    .saturating_mul(FBP_HEIGHT)
                    .checked_div(*total as usize)
                    .unwrap_or(FBP_HEIGHT);
                renderer.scroll_down_from(previous, rows);
                ending_sprite_tick = Some(*progress);
            }
            _ => {}
        }
        if self.brightness < 64 {
            renderer.apply_brightness(self.brightness);
        }
        if self.tint_amount != 0 {
            renderer.apply_color_tint(self.tint_color, self.tint_amount);
        }
        if self.screen_wave != 0 {
            renderer.apply_wave(self.screen_wave as u16, self.wave_phase);
        }
        if self.shake_remaining != 0 {
            let level = i32::from(self.shake_level.min(i16::MAX as u16));
            let (x, y) = match self.shake_remaining % 4 {
                0 => (level, 0),
                1 => (0, level),
                2 => (-level, 0),
                _ => (0, -level),
            };
            renderer.apply_shake(x, y);
        }
        if let Some(tick) = ending_sprite_tick {
            let sprite_index = usize::from(self.ending_effect_sprite);
            if sprite_index != 0 {
                let frame_count = role_sprites
                    .character_frame_count(sprite_index)
                    .unwrap_or(0);
                if frame_count != 0 {
                    let frame = (tick as usize / ending_sprite_frame_ticks()) % frame_count;
                    if let Some(bitmap) = role_sprites.decode_frame(sprite_index, frame) {
                        renderer.blit_rle(&bitmap, 0, 0);
                    }
                }
            }
        }
    }
}

/// Apply Classic's `VIDEO_SwitchScreen` order to an RGBA framebuffer.
///
/// The original copies every sixth indexed pixel in the order 0, 3, 1, 5, 2, 4.
/// RGBA output must group channels by pixel before applying the same ordering.
fn apply_screen_switch(current: &mut [u8], previous: &[u8], revealed_groups: usize) -> bool {
    if current.len() != previous.len()
        || !current.len().is_multiple_of(4)
        || revealed_groups > BATTLE_SCREEN_SWITCH_GROUPS
    {
        return false;
    }
    let mut revealed = [false; BATTLE_SCREEN_SWITCH_GROUPS];
    for &group in BATTLE_SCREEN_SWITCH_ORDER.iter().take(revealed_groups) {
        revealed[group] = true;
    }
    for (index, (pixel, old_pixel)) in current
        .chunks_exact_mut(4)
        .zip(previous.chunks_exact(4))
        .enumerate()
    {
        if !revealed[index % BATTLE_SCREEN_SWITCH_GROUPS] {
            pixel.copy_from_slice(old_pixel);
        }
    }
    true
}

fn screen_switch_revealed_groups(progress: u32, total: u32) -> usize {
    if progress == 0 {
        return 0;
    }
    usize::try_from(
        progress
            .saturating_mul(BATTLE_SCREEN_SWITCH_GROUPS as u32)
            .div_ceil(total.max(1))
            .min(BATTLE_SCREEN_SWITCH_GROUPS as u32),
    )
    .unwrap_or(BATTLE_SCREEN_SWITCH_GROUPS)
}

fn load_ending_frames(
    role_sprites: &RoleSprites,
    sprite: usize,
    required: usize,
    name: &str,
) -> Result<Vec<RleBitmap>, String> {
    (0..required)
        .map(|frame| {
            role_sprites.decode_frame(sprite, frame).ok_or_else(|| {
                format!("ending {name} MGO sprite {sprite} frame {frame} is unavailable")
            })
        })
        .collect()
}

fn compose_ending_frame(ending: &EndingAnimation, screen: &mut [u8]) -> Result<(), String> {
    if ending.frame >= 400
        || ending.upper.len() != FBP_PIXELS
        || ending.lower.len() != FBP_PIXELS
        || ending.beast.len() < 2
        || ending.girl.len() < 4
        || screen.len() != FBP_PIXELS
    {
        return Err("ending animation has invalid frame data".to_owned());
    }

    let scroll = usize::from(ending.frame / 2);
    let upper_rows = scroll * FBP_WIDTH;
    let lower_rows = FBP_PIXELS - upper_rows;
    screen[..upper_rows].copy_from_slice(&ending.upper[lower_rows..]);
    screen[upper_rows..].copy_from_slice(&ending.lower[..lower_rows]);
    if !apply_indexed_wave(screen, 2, ending.frame) {
        return Err("ending animation could not apply its background wave".to_owned());
    }

    let frame = i32::from(ending.frame);
    if !blit_indexed_rle(
        screen,
        FBP_WIDTH,
        FBP_HEIGHT,
        &ending.beast[0],
        0,
        -400 + frame,
    ) || !blit_indexed_rle(
        screen,
        FBP_WIDTH,
        FBP_HEIGHT,
        &ending.beast[1],
        0,
        -200 + frame,
    ) || !blit_indexed_rle(
        screen,
        FBP_WIDTH,
        FBP_HEIGHT,
        &ending.girl[usize::from(ending.frame % 4)],
        220,
        ending_girl_y(ending.frame),
    ) {
        return Err("ending animation contains invalid sprite data".to_owned());
    }
    Ok(())
}

fn ending_girl_y(frame: u16) -> i32 {
    (180 - i32::from(frame.div_ceil(2))).max(80)
}

/// Apply the original 32-row PAL wave directly to an indexed framebuffer.
/// Ending sprites are drawn afterwards so only the two FBP backgrounds move.
fn apply_indexed_wave(screen: &mut [u8], level: u16, phase: u16) -> bool {
    if screen.len() != FBP_PIXELS || level == 0 {
        return screen.len() == FBP_PIXELS;
    }
    let mut offsets = [0usize; 32];
    let mut accumulated = 0i32;
    let mut step = 68i32;
    for index in 0..16 {
        step -= 8;
        accumulated += step;
        let offset = usize::try_from(accumulated * i32::from(level) / 256).unwrap_or(0);
        offsets[index] = offset % FBP_WIDTH;
        offsets[index + 16] = (FBP_WIDTH - offset) % FBP_WIDTH;
    }
    for (row, pixels) in screen.chunks_exact_mut(FBP_WIDTH).enumerate() {
        let offset = offsets[(usize::from(phase) + row) % offsets.len()];
        pixels.rotate_left(offset);
    }
    true
}

/// Blit an indexed RLE bitmap with clipping while preserving literal index 0.
fn blit_indexed_rle(
    target: &mut [u8],
    target_width: usize,
    target_height: usize,
    rle: &RleBitmap,
    dx: i32,
    dy: i32,
) -> bool {
    if target_width.checked_mul(target_height) != Some(target.len()) {
        return false;
    }
    let source_width = usize::from(rle.width);
    let source_height = usize::from(rle.height);
    let Some(source_pixels) = source_width.checked_mul(source_height) else {
        return false;
    };
    if rle.pixels.len() != source_pixels || rle.opaque.len() != source_pixels {
        return false;
    }

    for source_y in 0..source_height {
        let Some(target_y) = dy.checked_add(source_y as i32) else {
            continue;
        };
        let Ok(target_y) = usize::try_from(target_y) else {
            continue;
        };
        if target_y >= target_height {
            continue;
        }
        for source_x in 0..source_width {
            let source = source_y * source_width + source_x;
            if !rle.opaque[source] {
                continue;
            }
            let Some(target_x) = dx.checked_add(source_x as i32) else {
                continue;
            };
            let Ok(target_x) = usize::try_from(target_x) else {
                continue;
            };
            if target_x < target_width {
                target[target_y * target_width + target_x] = rle.pixels[source];
            }
        }
    }
    true
}

fn duration_ticks(milliseconds: u64) -> u32 {
    u32::try_from(milliseconds.div_ceil(UPDATE_INTERVAL_MS))
        .unwrap_or(u32::MAX)
        .max(1)
}

fn standard_fade_ticks(speed: u16) -> u32 {
    duration_ticks(u64::from(speed) * 10 * 60)
}

fn screen_fade_ticks(speed: u16) -> u32 {
    duration_ticks(72 * (u64::from(speed) + 1) * 10)
}

fn fbp_fade_ticks(fade: u16) -> u32 {
    duration_ticks(96 * (u64::from(fade) + 1) * 10)
}

fn fbp_scroll_ticks(speed: u16) -> u32 {
    duration_ticks(220 * (800 / u64::from(speed.max(1))))
}

fn palette_fade_ticks(update_scene: bool) -> u32 {
    duration_ticks(32 * if update_scene { 100 } else { 25 })
}

fn color_fade_ticks(delay: u16) -> u32 {
    duration_ticks(64 * u64::from(delay.max(1)) * 10)
}

fn ending_sprite_frame_ticks() -> usize {
    usize::try_from(150u64.div_ceil(UPDATE_INTERVAL_MS)).unwrap_or(usize::MAX)
}

fn interpolate(start: u8, end: u8, progress: u32, total: u32) -> u8 {
    let start = i64::from(start);
    let distance = i64::from(end) - start;
    (start + distance * i64::from(progress) / i64::from(total.max(1))).clamp(0, 64) as u8
}

fn progress_64(progress: u32, total: u32) -> u8 {
    progress
        .saturating_mul(64)
        .checked_div(total.max(1))
        .unwrap_or(64)
        .min(64) as u8
}

fn blend_palette(from: &Palette, to: &Palette, progress: u32, total: u32) -> Palette {
    let mut result = Palette::default();
    for (output, (from, to)) in result
        .colors
        .iter_mut()
        .zip(from.colors.iter().zip(&to.colors))
    {
        *output = PaletteColor {
            r: blend_channel(from.r, to.r, progress, total),
            g: blend_channel(from.g, to.g, progress, total),
            b: blend_channel(from.b, to.b, progress, total),
        };
    }
    result
}

fn blend_channel(from: u8, to: u8, progress: u32, total: u32) -> u8 {
    let from = i64::from(from);
    let distance = i64::from(to) - from;
    (from + distance * i64::from(progress) / i64::from(total.max(1))).clamp(0, 63) as u8
}

fn rgba_to_indices(screen: &[u8], palette: &Palette) -> Result<Vec<u8>, String> {
    if screen.len() != RNG_FRAME_PIXELS * 4 {
        return Err("RNG source screen has the wrong size".to_owned());
    }
    let mut lookup = HashMap::with_capacity(256);
    for index in 0u8..=u8::MAX {
        lookup.entry(palette.get_rgb(index)).or_insert(index);
    }
    Ok(screen
        .chunks_exact(4)
        .map(|pixel| {
            lookup
                .get(&(pixel[0], pixel[1], pixel[2]))
                .copied()
                .unwrap_or(0)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolation_reaches_both_ends() {
        assert_eq!(interpolate(64, 0, 0, 8), 64);
        assert_eq!(interpolate(64, 0, 4, 8), 32);
        assert_eq!(interpolate(64, 0, 8, 8), 0);
        assert_eq!(progress_64(3, 4), 48);
    }

    #[test]
    fn battle_screen_switch_reveals_rgba_pixels_in_classic_order() {
        let previous = (101u8..=106)
            .flat_map(|value| [value, 0, 0, 255])
            .collect::<Vec<_>>();
        let target = (1u8..=6)
            .flat_map(|value| [value, 0, 0, 255])
            .collect::<Vec<_>>();

        let switched = |groups| {
            let mut current = target.clone();
            assert!(apply_screen_switch(&mut current, &previous, groups));
            current
                .chunks_exact(4)
                .map(|pixel| pixel[0])
                .collect::<Vec<_>>()
        };
        assert_eq!(switched(0), [101, 102, 103, 104, 105, 106]);
        assert_eq!(switched(1), [1, 102, 103, 104, 105, 106]);
        assert_eq!(switched(2), [1, 102, 103, 4, 105, 106]);
        assert_eq!(switched(3), [1, 2, 103, 4, 105, 106]);
        assert_eq!(switched(6), [1, 2, 3, 4, 5, 6]);
        assert_eq!(screen_switch_revealed_groups(0, 8), 0);
        assert_eq!(screen_switch_revealed_groups(1, 8), 1);
        assert_eq!(screen_switch_revealed_groups(4, 8), 3);
        assert_eq!(screen_switch_revealed_groups(7, 8), 6);
    }

    #[test]
    fn battle_transition_keeps_the_classic_switch_active_for_its_full_duration() {
        let (palettes, fbp, rng, role_sprites) = resources();
        let current = vec![0; RNG_FRAME_PIXELS * 4];
        let mut visual = VisualState::new();

        assert!(visual.queue_battle_transition());
        assert!(visual
            .start_pending(&current, &palettes, &fbp, &rng, &role_sprites)
            .unwrap());
        assert!(visual.battle_transition_active());
        assert!(matches!(
            &visual.effect,
            Some(VisualEffect::ScreenSwitch {
                progress: 0,
                total: 8,
                ..
            })
        ));
        for _ in 0..7 {
            assert!(visual
                .update(&current, &palettes, &fbp, &rng, &role_sprites)
                .unwrap());
            assert!(visual.battle_transition_active());
        }
        assert!(visual
            .update(&current, &palettes, &fbp, &rng, &role_sprites)
            .unwrap());
        assert!(!visual.battle_transition_active());
    }

    #[test]
    fn palette_blend_interpolates_six_bit_channels() {
        let mut from = Palette::default();
        let mut to = Palette::default();
        from.colors[1] = PaletteColor { r: 0, g: 20, b: 40 };
        to.colors[1] = PaletteColor {
            r: 60,
            g: 40,
            b: 20,
        };
        assert_eq!(
            blend_palette(&from, &to, 1, 2).colors[1],
            PaletteColor {
                r: 30,
                g: 30,
                b: 30
            }
        );
    }

    fn mkf(chunks: &[Vec<u8>]) -> Vec<u8> {
        let table_size = (chunks.len() + 1) * 4;
        let mut offset = table_size as u32;
        let mut data = Vec::new();
        data.extend_from_slice(&offset.to_le_bytes());
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
        let mut result = Vec::new();
        result.extend_from_slice(b"YJ_1");
        result.extend_from_slice(&(data.len() as u32).to_le_bytes());
        result.extend_from_slice(&(20u32 + data.len() as u32).to_le_bytes());
        result.extend_from_slice(&1u16.to_le_bytes());
        result.extend_from_slice(&[0, 0]);
        result.extend_from_slice(&(data.len() as u16).to_le_bytes());
        result.extend_from_slice(&0u16.to_le_bytes());
        result.extend_from_slice(data);
        result
    }

    fn indexed_sprite(colors: &[u8]) -> Vec<u8> {
        let first_offset_words = u16::try_from(colors.len() + 1).unwrap();
        let mut sprite = Vec::new();
        for frame in 0..colors.len() {
            let offset = usize::from(first_offset_words) + frame * 3;
            sprite.extend_from_slice(&u16::try_from(offset).unwrap().to_le_bytes());
        }
        sprite.extend_from_slice(&0u16.to_le_bytes());
        for &color in colors {
            sprite.extend_from_slice(&[1, 0, 1, 0, 1, color]);
        }
        sprite
    }

    fn resources() -> (Vec<PaletteSet>, FbpArchive, RngArchive, RoleSprites) {
        let mut palette = Palette::default();
        palette.colors[1] = PaletteColor { r: 63, g: 0, b: 0 };
        palette.colors[2] = PaletteColor { r: 0, g: 63, b: 0 };
        let palettes = vec![PaletteSet {
            day: palette,
            night: None,
        }];
        let fbp = FbpArchive::new(&mkf(&[vec![1; RNG_FRAME_PIXELS]])).unwrap();
        let animation = mkf(&[raw_yj1(&[0x06, 2, 2, 0x00])]);
        let rng = RngArchive::new(&mkf(&[animation])).unwrap();
        let mut sprite = Vec::new();
        sprite.extend_from_slice(&2u16.to_le_bytes());
        sprite.extend_from_slice(&0u16.to_le_bytes());
        sprite.extend_from_slice(&[1, 0, 1, 0, 1, 2]);
        let role_sprites = RoleSprites::load(&mkf(&[Vec::new(), raw_yj1(&sprite)])).unwrap();
        (palettes, fbp, rng, role_sprites)
    }

    fn ending_resources() -> (Vec<PaletteSet>, FbpArchive, RngArchive, RoleSprites) {
        let mut palette = Palette::default();
        for index in 1u8..=8 {
            palette.colors[usize::from(index)] = PaletteColor {
                r: index,
                g: 0,
                b: 0,
            };
        }
        let palettes = vec![PaletteSet {
            day: palette,
            night: None,
        }];
        let mut pictures = vec![Vec::new(); 63];
        pictures[61] = vec![1; FBP_PIXELS];
        pictures[62] = vec![2; FBP_PIXELS];
        let fbp = FbpArchive::new(&mkf(&pictures)).unwrap();
        let animation = mkf(&[raw_yj1(&[0x00])]);
        let rng = RngArchive::new(&mkf(&[animation])).unwrap();
        let mut sprites = vec![Vec::new(); 573];
        sprites[571] = raw_yj1(&indexed_sprite(&[3, 4]));
        sprites[572] = raw_yj1(&indexed_sprite(&[5, 6, 7, 8]));
        let role_sprites = RoleSprites::load(&mkf(&sprites)).unwrap();
        (palettes, fbp, rng, role_sprites)
    }

    #[test]
    fn ending_background_splices_upper_and_lower_pictures() {
        let transparent = RleBitmap {
            width: 1,
            height: 1,
            pixels: vec![0],
            opaque: vec![false],
        };
        let ending = EndingAnimation {
            upper: vec![1; FBP_PIXELS],
            lower: vec![2; FBP_PIXELS],
            beast: vec![transparent.clone(), transparent.clone()],
            girl: vec![transparent; 4],
            frame: 200,
        };
        let mut screen = vec![0; FBP_PIXELS];
        compose_ending_frame(&ending, &mut screen).unwrap();
        assert!(screen[..100 * FBP_WIDTH].iter().all(|&pixel| pixel == 1));
        assert!(screen[100 * FBP_WIDTH..].iter().all(|&pixel| pixel == 2));
    }

    #[test]
    fn ending_girl_rises_to_the_original_floor() {
        assert_eq!(ending_girl_y(0), 180);
        assert_eq!(ending_girl_y(199), 80);
        assert_eq!(ending_girl_y(200), 80);
        assert_eq!(ending_girl_y(399), 80);
    }

    #[test]
    fn indexed_rle_blit_clips_and_preserves_opaque_zero() {
        let bitmap = RleBitmap {
            width: 2,
            height: 2,
            pixels: vec![9, 0, 0, 7],
            opaque: vec![true, true, false, true],
        };
        let mut target = vec![5; 6];
        assert!(blit_indexed_rle(&mut target, 3, 2, &bitmap, -1, 0));
        assert_eq!(target, [0, 5, 5, 7, 5, 5]);
    }

    #[test]
    fn ending_animation_composes_400_frames_and_keeps_the_last() {
        let (palettes, fbp, rng, role_sprites) = ending_resources();
        let current = vec![0; FBP_PIXELS * 4];
        let mut visual = VisualState::new();
        assert!(visual.queue(ScriptVisual::PlayEndingAnimation));
        for tick in 0..400 {
            assert!(visual
                .update(&current, &palettes, &fbp, &rng, &role_sprites)
                .unwrap());
            let screen = visual.indexed_screen.as_ref().unwrap();
            match tick {
                0 => assert_eq!(screen[180 * FBP_WIDTH + 220], 5),
                199 => assert_eq!(screen[80 * FBP_WIDTH + 220], 8),
                200 => {
                    assert_eq!(screen[0], 4);
                    assert_eq!(screen[80 * FBP_WIDTH + 220], 5);
                }
                399 => {
                    assert_eq!(screen[199 * FBP_WIDTH], 4);
                    assert_eq!(screen[80 * FBP_WIDTH + 220], 8);
                }
                _ => {}
            }
            assert_eq!(visual.is_blocking(), tick < 399);
        }
        assert_eq!(visual.screen_wave, 0);
        assert_eq!(visual.indexed_screen.as_ref().unwrap()[199 * FBP_WIDTH], 4);
    }

    #[test]
    fn fbp_and_rng_commands_replace_and_increment_the_indexed_screen() {
        let (palettes, fbp, rng, role_sprites) = resources();
        let current = vec![252; RNG_FRAME_PIXELS * 4];
        let mut visual = VisualState::new();
        assert!(visual.queue(ScriptVisual::ShowFbp { index: 0, fade: 0 }));
        assert!(visual
            .update(&current, &palettes, &fbp, &rng, &role_sprites)
            .unwrap());
        assert!(!visual.is_blocking());

        let mut renderer = Renderer::new(palettes[0].day.clone(), 320, 200);
        assert!(visual.render_override(&mut renderer));
        assert_eq!(&renderer.screen()[..4], &[252, 0, 0, 255]);

        assert!(visual.queue(ScriptVisual::PlayRng {
            animation: 0,
            start_frame: 0,
            end_frame: None,
            speed: 16,
        }));
        visual
            .update(renderer.screen(), &palettes, &fbp, &rng, &role_sprites)
            .unwrap();
        assert!(!visual.is_blocking());
        assert!(visual.render_override(&mut renderer));
        assert_eq!(&renderer.screen()[..8], &[0, 252, 0, 255, 0, 252, 0, 255]);
        assert_eq!(&renderer.screen()[8..12], &[252, 0, 0, 255]);
    }

    #[test]
    fn multi_frame_rng_fades_in_after_a_pending_scene_fade() {
        let (palettes, fbp, _, role_sprites) = resources();
        let animation = mkf(&[
            raw_yj1(&[0x06, 1, 1, 0x00]),
            raw_yj1(&[0x06, 2, 2, 0x00]),
            raw_yj1(&[0x06, 1, 1, 0x00]),
        ]);
        let rng = RngArchive::new(&mkf(&[animation])).unwrap();
        let current = vec![252; RNG_FRAME_PIXELS * 4];
        let mut visual = VisualState::new();

        assert!(visual.queue(ScriptVisual::FadeOut { speed: 1 }));
        while visual.is_blocking() {
            visual
                .update(&current, &palettes, &fbp, &rng, &role_sprites)
                .unwrap();
        }
        assert_eq!(visual.brightness, 0);
        assert!(visual.needs_scene_fade_in);

        assert!(visual.queue(ScriptVisual::PlayRng {
            animation: 0,
            start_frame: 0,
            end_frame: None,
            speed: 20,
        }));
        visual
            .update(&current, &palettes, &fbp, &rng, &role_sprites)
            .unwrap();
        assert!(!visual.needs_scene_fade_in);
        assert!(visual.brightness > 0);
        assert_eq!(&visual.indexed_screen.as_ref().unwrap()[..2], &[1, 1]);

        while visual.brightness < 64 {
            visual
                .update(&current, &palettes, &fbp, &rng, &role_sprites)
                .unwrap();
            assert_eq!(&visual.indexed_screen.as_ref().unwrap()[..2], &[1, 1]);
        }

        let mut saw_second_frame = false;
        while visual.is_blocking() {
            visual
                .update(&current, &palettes, &fbp, &rng, &role_sprites)
                .unwrap();
            saw_second_frame |= visual.indexed_screen.as_ref().unwrap()[0] == 2;
        }
        assert!(saw_second_frame);
        assert_eq!(&visual.indexed_screen.as_ref().unwrap()[..2], &[1, 1]);
    }

    #[test]
    fn fades_block_until_their_final_brightness() {
        let (palettes, fbp, rng, role_sprites) = resources();
        let current = vec![0; RNG_FRAME_PIXELS * 4];
        let mut visual = VisualState::new();
        assert!(visual.queue(ScriptVisual::FadeOut { speed: 1 }));
        let mut ticks = 0;
        while visual.is_blocking() {
            visual
                .update(&current, &palettes, &fbp, &rng, &role_sprites)
                .unwrap();
            ticks += 1;
        }
        assert_eq!(ticks, 12);
        assert_eq!(visual.brightness, 0);
        assert!(visual.needs_scene_fade_in);
        assert_eq!(visual.rgba_screen.as_deref(), Some(current.as_slice()));

        assert!(visual.queue(ScriptVisual::FadeIn { speed: 1 }));
        while visual.is_blocking() {
            visual
                .update(&current, &palettes, &fbp, &rng, &role_sprites)
                .unwrap();
        }
        assert_eq!(visual.brightness, 64);
        assert!(!visual.needs_scene_fade_in);
        assert!(visual.rgba_screen.is_none());
        assert!(!visual.fade_screen_frozen);
    }

    #[test]
    fn fade_out_freezes_the_frame_visible_when_the_command_starts() {
        let (palettes, fbp, rng, role_sprites) = resources();
        let current = vec![17; RNG_FRAME_PIXELS * 4];
        let mut visual = VisualState::new();
        assert!(visual.queue(ScriptVisual::FadeOut { speed: 1 }));
        assert!(visual
            .start_pending(&current, &palettes, &fbp, &rng, &role_sprites)
            .unwrap());

        assert_eq!(visual.rgba_screen.as_deref(), Some(current.as_slice()));
        assert_eq!(visual.brightness, 64);
        assert!(visual.fade_screen_frozen);
    }

    #[test]
    fn prepared_scene_stays_black_until_automatic_fade_in_is_queued() {
        let mut visual = VisualState::new();
        visual.rgba_screen = Some(vec![17; RNG_FRAME_PIXELS * 4]);
        visual.brightness = 64;

        visual.prepare_scene_fade_in();

        assert_eq!(visual.brightness, 0);
        assert!(visual.needs_scene_fade_in);
        assert!(visual.rgba_screen.is_none());
        assert!(visual.queue_automatic_scene_fade_in());
        assert!(visual.is_blocking());
    }

    #[test]
    fn completed_fade_out_automatically_reveals_the_next_scene() {
        let (palettes, fbp, rng, role_sprites) = resources();
        let current = vec![0; RNG_FRAME_PIXELS * 4];
        let mut visual = VisualState::new();
        assert!(visual.queue(ScriptVisual::ShowFbp { index: 0, fade: 0 }));
        visual
            .update(&current, &palettes, &fbp, &rng, &role_sprites)
            .unwrap();
        assert!(visual.indexed_screen.is_some());
        assert!(visual.queue(ScriptVisual::FadeOut { speed: 1 }));
        while visual.is_blocking() {
            visual
                .update(&current, &palettes, &fbp, &rng, &role_sprites)
                .unwrap();
        }
        assert_eq!(visual.brightness, 0);
        assert!(visual.rgba_screen.is_some());

        assert!(visual.queue_automatic_scene_fade_in());
        assert!(visual.indexed_screen.is_none());
        assert!(visual.rgba_screen.is_none());
        while visual.is_blocking() {
            visual
                .update(&current, &palettes, &fbp, &rng, &role_sprites)
                .unwrap();
        }
        assert_eq!(visual.brightness, 64);
        assert!(!visual.needs_scene_fade_in);
        assert!(!visual.queue_automatic_scene_fade_in());
    }

    #[test]
    fn original_visual_delays_scale_with_their_speed_operands() {
        assert_eq!(screen_fade_ticks(0), 15);
        assert_eq!(screen_fade_ticks(4), 72);
        assert_eq!(fbp_fade_ticks(1), 39);
        assert_eq!(fbp_scroll_ticks(4), 880);
        assert_eq!(palette_fade_ticks(false), 16);
        assert_eq!(palette_fade_ticks(true), 64);
        assert_eq!(color_fade_ticks(1), 13);
        assert_eq!(duration_ticks(32 * 75), 48);
        assert_eq!(ending_sprite_frame_ticks(), 3);
    }

    #[test]
    fn scene_fade_schedules_original_ten_fps_world_updates() {
        let (palettes, fbp, rng, role_sprites) = resources();
        let current = vec![0; RNG_FRAME_PIXELS * 4];
        let mut visual = VisualState::new();
        assert!(visual.queue(ScriptVisual::FadeSceneWithUpdate { step: 1 }));
        let mut ticks = 0;
        let mut scene_updates = 0;
        while visual.is_blocking() {
            scene_updates += usize::from(visual.scene_update_due());
            visual
                .update(&current, &palettes, &fbp, &rng, &role_sprites)
                .unwrap();
            ticks += 1;
        }
        assert_eq!(ticks, 128);
        assert_eq!(scene_updates, 64);
    }

    #[test]
    fn fbp_effect_sprite_is_persistent_and_drawn_during_transitions() {
        let (palettes, fbp, rng, role_sprites) = resources();
        let current = vec![0; RNG_FRAME_PIXELS * 4];
        let mut visual = VisualState::new();
        assert!(visual.queue(ScriptVisual::ShowFbpWithSprite {
            index: 0,
            sprite: Some(1),
            fade: 1,
        }));
        visual
            .update(&current, &palettes, &fbp, &rng, &role_sprites)
            .unwrap();

        let mut renderer = Renderer::new(palettes[0].day.clone(), 320, 200);
        assert!(visual.render_override(&mut renderer));
        visual.apply_post_effects(&mut renderer, &role_sprites);
        assert_eq!(&renderer.screen()[..4], &[0, 252, 0, 255]);

        while visual.is_blocking() {
            visual
                .update(&current, &palettes, &fbp, &rng, &role_sprites)
                .unwrap();
        }
        assert!(visual.queue(ScriptVisual::ShowFbpWithSprite {
            index: 0,
            sprite: None,
            fade: 1,
        }));
        visual
            .update(&current, &palettes, &fbp, &rng, &role_sprites)
            .unwrap();
        assert!(visual.render_override(&mut renderer));
        visual.apply_post_effects(&mut renderer, &role_sprites);
        assert_eq!(&renderer.screen()[..4], &[0, 252, 0, 255]);

        while visual.is_blocking() {
            visual
                .update(&current, &palettes, &fbp, &rng, &role_sprites)
                .unwrap();
        }
        assert!(visual.queue(ScriptVisual::ShowFbp { index: 0, fade: 1 }));
        visual
            .update(&current, &palettes, &fbp, &rng, &role_sprites)
            .unwrap();
        assert!(visual.render_override(&mut renderer));
        visual.apply_post_effects(&mut renderer, &role_sprites);
        assert_ne!(&renderer.screen()[..4], &[0, 252, 0, 255]);
    }

    #[test]
    fn show_fbp_max_index_preserves_the_original_black_screen_fallback() {
        let (palettes, fbp, rng, role_sprites) = resources();
        let current = vec![252; RNG_FRAME_PIXELS * 4];
        let mut visual = VisualState::new();
        assert!(visual.queue(ScriptVisual::ShowFbp {
            index: u16::MAX,
            fade: 0,
        }));
        visual
            .update(&current, &palettes, &fbp, &rng, &role_sprites)
            .unwrap();
        let mut renderer = Renderer::new(palettes[0].day.clone(), 320, 200);
        assert!(visual.render_override(&mut renderer));
        assert_eq!(&renderer.screen()[..4], &[0, 0, 0, 255]);
    }
}
