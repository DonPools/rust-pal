use pal_assets::rle::RleBitmap;
use pal_assets::text::{BitmapFont, TextLibrary};
use pal_core::script::DialogPosition;

use super::dialog_text::{
    center_window_width_units, dialog_body_lines, dialog_glyph_count, dialog_layout,
    dialog_page_glyph_target, dialog_title, dialog_token, DialogTokenKind,
};
use super::menu_render::draw_single_line_box_with_shadow;
use super::text_render::{
    dialog_color_after, draw_dialog_text, draw_dialog_wait_icon, DialogTextMode, DialogTextStyle,
};
use crate::renderer::Renderer;

#[derive(Debug, Clone)]
pub(super) struct ActiveDialog {
    pub(super) message_ids: Vec<u16>,
    pub(super) inline_lines: Option<Vec<Vec<u8>>>,
    pub(super) position: DialogPosition,
    pub(super) font_color: u8,
    pub(super) face_index: Option<u16>,
    pub(super) playing_rng: bool,
    pub(super) page: usize,
    pub(super) awaiting_input: bool,
    pub(super) wait_after_reveal: bool,
    pub(super) auto_wait_ticks: Option<u16>,
    pub(super) revealed_glyphs: usize,
    pub(super) reveal_credit_ms: u32,
    pub(super) initial_delay_ms: u16,
    pub(super) terminal_wait_ms: Option<u32>,
    pub(super) wait_icon: u8,
    pub(super) wait_palette_ticks: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DialogPlayback {
    Revealing,
    ContinueScript,
    AwaitingInput,
    AutoClose,
}

impl ActiveDialog {
    pub(super) fn new(
        message_id: u16,
        position: DialogPosition,
        font_color: u8,
        face_index: Option<u16>,
        playing_rng: bool,
        initial_delay_ms: u16,
    ) -> Self {
        let center_window = position == DialogPosition::CenterWindow;
        Self {
            message_ids: vec![message_id],
            inline_lines: None,
            position,
            font_color,
            face_index,
            playing_rng,
            page: 0,
            awaiting_input: false,
            wait_after_reveal: center_window,
            auto_wait_ticks: center_window.then_some(28),
            revealed_glyphs: 0,
            reveal_credit_ms: 0,
            initial_delay_ms,
            terminal_wait_ms: None,
            wait_icon: 0,
            wait_palette_ticks: 0,
        }
    }

    /// Create Classic's centered, single-line transient dialog for text that is
    /// assembled at runtime rather than stored in `M.MSG`.
    pub(super) fn center_window_text(text: Vec<u8>) -> Self {
        Self {
            message_ids: Vec::new(),
            inline_lines: Some(vec![text]),
            position: DialogPosition::CenterWindow,
            font_color: 0x4f,
            face_index: None,
            playing_rng: false,
            page: 0,
            awaiting_input: false,
            wait_after_reveal: true,
            // Classic's center-window dialog waits at most 1.4 seconds.
            auto_wait_ticks: Some(28),
            revealed_glyphs: 0,
            reveal_credit_ms: 0,
            initial_delay_ms: 0,
            terminal_wait_ms: None,
            wait_icon: 0,
            wait_palette_ticks: 0,
        }
    }
}

pub(super) fn advance_dialog_playback(
    text: &TextLibrary,
    dialog: &mut ActiveDialog,
    persistent_delay_ms: &mut u16,
    elapsed_ms: u32,
    skip_reveal: bool,
) -> DialogPlayback {
    if dialog.awaiting_input {
        return DialogPlayback::AwaitingInput;
    }
    if let Some(remaining) = dialog.terminal_wait_ms.as_mut() {
        *remaining = remaining.saturating_sub(elapsed_ms);
        return if *remaining == 0 {
            DialogPlayback::AutoClose
        } else {
            DialogPlayback::Revealing
        };
    }

    dialog.reveal_credit_ms = dialog.reveal_credit_ms.saturating_add(elapsed_ms);
    let target = dialog_page_glyph_target(text, dialog);
    let mut glyph_index = 0usize;
    let mut delay_ms = dialog.initial_delay_ms;
    let mut icon = 0u8;
    let mut terminal_delay = None;
    let instant = dialog.position == DialogPosition::CenterWindow;

    'lines: for line in dialog_body_lines(text, dialog)
        .into_iter()
        .take((dialog.page + 1) * 4)
    {
        // PAL resets the waiting icon at the beginning of every MESSAGE.
        icon = 0;
        let mut index = 0;
        while index < line.len() {
            let token = dialog_token(line.as_ref(), index);
            match token.kind {
                DialogTokenKind::Glyph => {
                    if glyph_index >= dialog.revealed_glyphs && glyph_index < target {
                        if !skip_reveal
                            && !instant
                            && delay_ms != 0
                            && dialog.reveal_credit_ms < u32::from(delay_ms)
                        {
                            break 'lines;
                        }
                        if !skip_reveal && !instant && delay_ms != 0 {
                            dialog.reveal_credit_ms -= u32::from(delay_ms);
                        }
                        dialog.revealed_glyphs += 1;
                    }
                    glyph_index += 1;
                }
                DialogTokenKind::Delay(value) => {
                    delay_ms = original_character_delay_ms(value);
                    *persistent_delay_ms = delay_ms;
                }
                DialogTokenKind::Terminate(value) => {
                    if dialog.revealed_glyphs >= glyph_index {
                        terminal_delay = Some(original_terminal_delay_ms(value));
                    }
                    break 'lines;
                }
                DialogTokenKind::Icon(value) => icon = value,
                DialogTokenKind::Color | DialogTokenKind::LineBreak => {}
            }
            index += token.bytes;
        }
    }
    dialog.wait_icon = icon;

    if let Some(delay) = terminal_delay {
        if delay == 0 {
            return DialogPlayback::AutoClose;
        }
        dialog.terminal_wait_ms = Some(delay);
        return DialogPlayback::Revealing;
    }
    if dialog.revealed_glyphs < target {
        return DialogPlayback::Revealing;
    }
    if dialog.wait_after_reveal {
        dialog.awaiting_input = true;
        dialog.wait_palette_ticks = 0;
        DialogPlayback::AwaitingInput
    } else {
        DialogPlayback::ContinueScript
    }
}

fn original_character_delay_ms(value: u16) -> u16 {
    value.saturating_mul(10) / 7 * 8
}

fn original_terminal_delay_ms(value: u16) -> u32 {
    u32::from(value).saturating_mul(80) / 7
}

pub(super) fn render_dialog(
    renderer: &mut Renderer,
    text: &TextLibrary,
    font: &BitmapFont,
    faces: &[Option<RleBitmap>],
    icons: &[RleBitmap],
    ui_sprites: &[RleBitmap],
    dialog: &ActiveDialog,
) {
    let layout = dialog_layout(dialog);
    if let Some(face_index) = dialog.face_index {
        if let Some(Some(face)) = faces.get(usize::from(face_index)) {
            let (center_x, center_y) = match dialog.position {
                DialogPosition::Upper => (48, 55),
                DialogPosition::Lower => (270, 144),
                _ => (0, 0),
            };
            if center_x != 0 {
                let mut left = center_x - i32::from(face.width) / 2;
                let mut top = center_y - i32::from(face.height) / 2;
                if dialog.position == DialogPosition::Upper {
                    left = left.max(0);
                    top = top.max(0);
                }
                renderer.blit_rle(face, left, top);
            }
        }
    }

    if let Some(title) = dialog_title(text, dialog) {
        let _ = draw_dialog_text(
            renderer,
            font,
            ui_sprites,
            title,
            layout.title_x,
            layout.title_y,
            DialogTextStyle {
                color: 0x8c,
                glyph_limit: usize::MAX,
                mode: DialogTextMode::Normal,
            },
        );
    }

    let lines = dialog_body_lines(text, dialog);
    let visible = lines
        .iter()
        .skip(dialog.page * 4)
        .take(4)
        .map(|line| line.as_ref())
        .collect::<Vec<_>>();
    if dialog.position == DialogPosition::CenterWindow {
        render_center_dialog_window(renderer, font, ui_sprites, dialog, &visible);
        return;
    }

    let mut remaining_glyphs = dialog.revealed_glyphs.saturating_sub(
        lines
            .iter()
            .take(dialog.page * 4)
            .map(|line| dialog_glyph_count(line))
            .sum::<usize>(),
    );
    let mut color = lines
        .iter()
        .take(dialog.page * 4)
        .fold(dialog.font_color, |color, line| {
            dialog_color_after(line, color, DialogTextMode::Normal)
        });
    let mut last_end = None;
    for (line, bytes) in visible.iter().enumerate() {
        let y = layout.text_y + line as i32 * 18;
        let (end, drawn, next_color) = draw_dialog_text(
            renderer,
            font,
            ui_sprites,
            bytes,
            layout.text_x,
            y,
            DialogTextStyle {
                color,
                glyph_limit: remaining_glyphs,
                mode: DialogTextMode::Normal,
            },
        );
        color = next_color;
        remaining_glyphs = remaining_glyphs.saturating_sub(drawn);
        last_end = Some((end, y));
    }
    if dialog.awaiting_input && dialog.position != DialogPosition::Center {
        if let Some((x, y)) = last_end {
            draw_dialog_wait_icon(renderer, icons, dialog.wait_icon, x, y);
        }
    }
}

fn render_center_dialog_window(
    renderer: &mut Renderer,
    font: &BitmapFont,
    ui_sprites: &[RleBitmap],
    dialog: &ActiveDialog,
    lines: &[&[u8]],
) {
    let Some(&line) = lines.first() else {
        return;
    };
    let units = center_window_width_units(line);
    let x = 160 - i32::try_from(units).unwrap_or(0) * 4;
    let y = 40;
    draw_single_line_box_with_shadow(renderer, ui_sprites, x, y, units.div_ceil(2), 0);
    let text_x = x + 8 + i32::try_from(units % 2).unwrap_or(0) * 4;
    let _ = draw_dialog_text(
        renderer,
        font,
        ui_sprites,
        line,
        text_x,
        y + 10,
        DialogTextStyle {
            color: dialog.font_color,
            glyph_limit: dialog.revealed_glyphs,
            mode: DialogTextMode::CenterWindow,
        },
    );
}
