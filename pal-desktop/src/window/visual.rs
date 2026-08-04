use pal_assets::fbp::{FbpArchive, FBP_HEIGHT};
use pal_assets::palette::{Palette, PaletteColor, PaletteSet};
use pal_assets::rng::{RngArchive, RNG_FRAME_PIXELS};
use pal_core::script::ScriptVisual;
use std::collections::HashMap;

use crate::renderer::Renderer;

pub(super) struct VisualState {
    pending: Option<ScriptVisual>,
    effect: Option<VisualEffect>,
    palette_index: usize,
    night_palette: bool,
    indexed_screen: Option<Vec<u8>>,
    rgba_screen: Option<Vec<u8>>,
    backup_screen: Option<Vec<u8>>,
    brightness: u8,
    tint_color: (u8, u8, u8),
    tint_amount: u8,
    screen_wave: i32,
    wave_progression: i16,
    wave_phase: i16,
    shake_remaining: u16,
    shake_level: u16,
}

enum VisualEffect {
    Fade {
        start: u8,
        end: u8,
        progress: u32,
        total: u32,
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
    },
    Scroll {
        previous: Vec<u8>,
        progress: u32,
        total: u32,
    },
    Rng(RngPlayback),
}

struct RngPlayback {
    animation: usize,
    next_frame: usize,
    end_frame: Option<usize>,
    ticks_per_frame: u16,
    ticks_until_frame: u16,
}

impl VisualState {
    pub(super) fn new() -> Self {
        Self {
            pending: None,
            effect: None,
            palette_index: 0,
            night_palette: false,
            indexed_screen: None,
            rgba_screen: None,
            backup_screen: None,
            brightness: 64,
            tint_color: (0, 0, 0),
            tint_amount: 0,
            screen_wave: 0,
            wave_progression: 0,
            wave_phase: 0,
            shake_remaining: 0,
            shake_level: 0,
        }
    }

    pub(super) fn queue(&mut self, command: ScriptVisual) -> bool {
        if self.pending.is_some() || self.effect.is_some() || self.shake_remaining != 0 {
            return false;
        }
        self.pending = Some(command);
        true
    }

    pub(super) fn is_blocking(&self) -> bool {
        self.pending.is_some() || self.effect.is_some() || self.shake_remaining != 0
    }

    pub(super) fn needs_update(&self) -> bool {
        self.is_blocking() || self.screen_wave != 0
    }

    pub(super) fn restore_original_environment(&mut self, night: bool, screen_wave: u16) {
        self.pending = None;
        self.effect = None;
        self.palette_index = 0;
        self.night_palette = night;
        self.indexed_screen = None;
        self.rgba_screen = None;
        self.backup_screen = None;
        self.brightness = 64;
        self.tint_amount = 0;
        self.screen_wave = i32::from(screen_wave);
        self.wave_progression = 0;
        self.wave_phase = 0;
        self.shake_remaining = 0;
        self.shake_level = 0;
    }

    pub(super) fn update(
        &mut self,
        current_screen: &[u8],
        palettes: &[PaletteSet],
        fbp: &FbpArchive,
        rng: &RngArchive,
    ) -> Result<bool, String> {
        let mut changed = false;
        if let Some(command) = self.pending.take() {
            self.start(command, current_screen, palettes, fbp, rng)?;
            changed = true;
        }

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

    fn start(
        &mut self,
        command: ScriptVisual,
        current_screen: &[u8],
        palettes: &[PaletteSet],
        fbp: &FbpArchive,
        rng: &RngArchive,
    ) -> Result<(), String> {
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
                }));
            }
            ScriptVisual::FadeToRed => {
                self.effect = Some(VisualEffect::Tint {
                    color: (252, 0, 0),
                    start: self.tint_amount,
                    end: 64,
                    progress: 0,
                    total: 32,
                });
            }
            ScriptVisual::FadeOut { speed } => {
                self.effect = Some(VisualEffect::Fade {
                    start: self.brightness,
                    end: 0,
                    progress: 0,
                    total: u32::from(speed).saturating_mul(12).max(1),
                });
            }
            ScriptVisual::FadeIn { speed } => {
                self.brightness = 0;
                self.effect = Some(VisualEffect::Fade {
                    start: 0,
                    end: 64,
                    progress: 0,
                    total: u32::from(speed).saturating_mul(12).max(1),
                });
            }
            ScriptVisual::SetNightPalette { night } => self.night_palette = night,
            ScriptVisual::SetScreenWave { level, progression } => {
                self.screen_wave = i32::from(level);
                self.wave_progression = progression;
            }
            ScriptVisual::ShowFbp { index, fade } => {
                let target = fbp
                    .frame(usize::from(index))
                    .ok_or_else(|| format!("FBP picture {index} is unavailable"))?;
                self.rgba_screen = None;
                self.indexed_screen = Some(target);
                if fade != 0 {
                    self.effect = Some(VisualEffect::CrossFade {
                        previous: current_screen.to_vec(),
                        progress: 0,
                        total: u32::from(fade).saturating_mul(8).max(1),
                    });
                }
            }
            ScriptVisual::ToggleDayNightPalette { update_scene } => {
                let from = self.palette(palettes)?;
                self.night_palette = !self.night_palette;
                let to = self.palette(palettes)?;
                self.effect = Some(VisualEffect::PaletteBlend {
                    from: Box::new(from),
                    to: Box::new(to),
                    progress: 0,
                    total: if update_scene { 32 } else { 16 },
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
                let color = self.palette(palettes)?.get_rgb(color);
                let (start, end) = if from_color { (64, 0) } else { (0, 64) };
                self.tint_color = color;
                self.tint_amount = start;
                self.effect = Some(VisualEffect::Tint {
                    color,
                    start,
                    end,
                    progress: 0,
                    total: u32::from(delay.max(1)).saturating_mul(8),
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
                self.indexed_screen = None;
                self.rgba_screen = None;
                let magnitude = u32::from(step.unsigned_abs().max(1));
                let (start, end) = if step < 0 { (64, 0) } else { (0, 64) };
                self.brightness = start;
                self.effect = Some(VisualEffect::Fade {
                    start,
                    end,
                    progress: 0,
                    total: 64u32.div_ceil(magnitude).max(1),
                });
            }
            ScriptVisual::FadeToCurrentScene => {
                self.indexed_screen = None;
                self.rgba_screen = None;
                self.effect = Some(VisualEffect::CrossFade {
                    previous: current_screen.to_vec(),
                    progress: 0,
                    total: 24,
                });
            }
            ScriptVisual::ScrollFbp { index, speed } => {
                let target = fbp
                    .frame(usize::from(index))
                    .ok_or_else(|| format!("FBP picture {index} is unavailable"))?;
                self.rgba_screen = None;
                self.indexed_screen = Some(target);
                self.effect = Some(VisualEffect::Scroll {
                    previous: current_screen.to_vec(),
                    progress: 0,
                    total: u32::from(speed.max(1)).saturating_mul(20),
                });
            }
            ScriptVisual::ShowFbpWithSprite {
                index,
                sprite: _,
                fade,
            } => {
                self.start(
                    ScriptVisual::ShowFbp { index, fade },
                    current_screen,
                    palettes,
                    fbp,
                    rng,
                )?;
            }
            ScriptVisual::BackupScreen => self.backup_screen = Some(current_screen.to_vec()),
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
            } => {
                *progress = progress.saturating_add(1).min(*total);
                self.brightness = interpolate(*start, *end, *progress, *total);
                if progress == total {
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
            VisualEffect::Rng(playback) => {
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
                if playback
                    .end_frame
                    .is_some_and(|end| playback.next_frame > end)
                    || playback.next_frame >= animation.frame_count()
                {
                    return Ok(None);
                }
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

    pub(super) fn apply_post_effects(&self, renderer: &mut Renderer) {
        match &self.effect {
            Some(VisualEffect::CrossFade {
                previous,
                progress,
                total,
            }) => {
                renderer.blend_from(previous, progress_64(*progress, *total));
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
                renderer.reveal_from_top(previous, rows);
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
    }
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

    fn resources() -> (Vec<PaletteSet>, FbpArchive, RngArchive) {
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
        (palettes, fbp, rng)
    }

    #[test]
    fn fbp_and_rng_commands_replace_and_increment_the_indexed_screen() {
        let (palettes, fbp, rng) = resources();
        let current = vec![252; RNG_FRAME_PIXELS * 4];
        let mut visual = VisualState::new();
        assert!(visual.queue(ScriptVisual::ShowFbp { index: 0, fade: 0 }));
        assert!(visual.update(&current, &palettes, &fbp, &rng).unwrap());
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
            .update(renderer.screen(), &palettes, &fbp, &rng)
            .unwrap();
        assert!(!visual.is_blocking());
        assert!(visual.render_override(&mut renderer));
        assert_eq!(&renderer.screen()[..8], &[0, 252, 0, 255, 0, 252, 0, 255]);
        assert_eq!(&renderer.screen()[8..12], &[252, 0, 0, 255]);
    }

    #[test]
    fn fades_block_until_their_final_brightness() {
        let (palettes, fbp, rng) = resources();
        let current = vec![0; RNG_FRAME_PIXELS * 4];
        let mut visual = VisualState::new();
        assert!(visual.queue(ScriptVisual::FadeOut { speed: 1 }));
        let mut ticks = 0;
        while visual.is_blocking() {
            visual.update(&current, &palettes, &fbp, &rng).unwrap();
            ticks += 1;
        }
        assert_eq!(ticks, 12);
        assert_eq!(visual.brightness, 0);

        assert!(visual.queue(ScriptVisual::FadeIn { speed: 1 }));
        while visual.is_blocking() {
            visual.update(&current, &palettes, &fbp, &rng).unwrap();
        }
        assert_eq!(visual.brightness, 64);
    }
}
