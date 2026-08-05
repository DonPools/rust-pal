use pal_assets::rle::RleBitmap;
use pal_assets::text::{BitmapFont, TextLibrary};
use pal_core::script::DialogPosition;

use super::dialog_text::{
    dialog_body_lines, dialog_glyph_count, dialog_layout, dialog_page_glyph_target,
    dialog_text_width, dialog_title, dialog_token, DialogTokenKind,
};
use super::draw::{fill_rect, stroke_rect};
use super::text_render::{dialog_color_after, draw_dialog_text, draw_dialog_wait_icon};
use crate::renderer::Renderer;

#[derive(Debug, Clone)]
pub(super) struct ActiveDialog {
    pub(super) message_ids: Vec<u16>,
    pub(super) position: DialogPosition,
    pub(super) font_color: u8,
    pub(super) face_index: Option<u16>,
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
        initial_delay_ms: u16,
    ) -> Self {
        let center_window = position == DialogPosition::CenterWindow;
        Self {
            message_ids: vec![message_id],
            position,
            font_color,
            face_index,
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
        let mut index = 0;
        while index < line.len() {
            let token = dialog_token(line, index);
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
                renderer.blit_rle(
                    face,
                    center_x - i32::from(face.width) / 2,
                    center_y - i32::from(face.height) / 2,
                );
            }
        }
    }

    if let Some(title) = dialog_title(text, dialog) {
        let _ = draw_dialog_text(
            renderer,
            font,
            title,
            layout.title_x,
            layout.title_y,
            0x8c,
            usize::MAX,
        );
    }

    let lines = dialog_body_lines(text, dialog);
    let visible = lines
        .iter()
        .skip(dialog.page * 4)
        .take(4)
        .copied()
        .collect::<Vec<_>>();
    if dialog.position == DialogPosition::CenterWindow {
        render_center_dialog_window(renderer, font, dialog, &lines, &visible);
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
            dialog_color_after(line, color)
        });
    let mut last_end = None;
    for (line, bytes) in visible.iter().enumerate() {
        let y = layout.text_y + line as i32 * 18;
        let (end, drawn, next_color) = draw_dialog_text(
            renderer,
            font,
            bytes,
            layout.text_x,
            y,
            color,
            remaining_glyphs,
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
    dialog: &ActiveDialog,
    all_lines: &[&[u8]],
    lines: &[&[u8]],
) {
    let content_width = lines
        .iter()
        .map(|line| dialog_text_width(line))
        .max()
        .unwrap_or(0)
        .clamp(16, 280) as i32;
    let width = content_width + 24;
    let height = lines.len().max(1) as i32 * 18 + 18;
    let x = (320 - width) / 2;
    let y = 40;
    fill_rect(renderer, x + 6, y + 6, width, height, [0, 0, 0, 160]);
    fill_rect(renderer, x, y, width, height, [16, 20, 24, 255]);
    stroke_rect(renderer, x, y, width, height, [232, 224, 192, 255]);
    stroke_rect(
        renderer,
        x + 2,
        y + 2,
        width - 4,
        height - 4,
        [72, 88, 96, 255],
    );
    let mut remaining_glyphs = dialog.revealed_glyphs.saturating_sub(
        all_lines
            .iter()
            .take(dialog.page * 4)
            .map(|line| dialog_glyph_count(line))
            .sum::<usize>(),
    );
    let mut color = all_lines
        .iter()
        .take(dialog.page * 4)
        .fold(dialog.font_color, |color, line| {
            dialog_color_after(line, color)
        });
    for (line, bytes) in lines.iter().enumerate() {
        let text_x = x + (width - dialog_text_width(bytes) as i32) / 2;
        let text_y = y + 10 + line as i32 * 18;
        let (_, drawn, next_color) = draw_dialog_text(
            renderer,
            font,
            bytes,
            text_x,
            text_y,
            color,
            remaining_glyphs,
        );
        color = next_color;
        remaining_glyphs = remaining_glyphs.saturating_sub(drawn);
    }
}
