use pal_assets::rle::RleBitmap;
use pal_assets::text::{BitmapFont, TextLibrary};
use pal_core::script::DialogPosition;

use super::dialog_text::{dialog_body_lines, dialog_layout, dialog_text_width, dialog_title};
use super::draw::{fill_rect, stroke_rect};
use super::text_render::{draw_dialog_text, draw_dialog_wait_icon};
use crate::renderer::Renderer;

#[derive(Debug, Clone)]
pub(super) struct ActiveDialog {
    pub(super) message_ids: Vec<u16>,
    pub(super) position: DialogPosition,
    pub(super) font_color: u8,
    pub(super) face_index: Option<u16>,
    pub(super) page: usize,
    pub(super) awaiting_input: bool,
    pub(super) auto_wait_ticks: Option<u16>,
}

pub(super) fn render_dialog(
    renderer: &mut Renderer,
    text: &TextLibrary,
    font: &BitmapFont,
    faces: &[Option<RleBitmap>],
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
        draw_dialog_text(renderer, font, title, layout.title_x, layout.title_y, 0x8c);
    }

    let lines = dialog_body_lines(text, dialog);
    let visible = lines
        .iter()
        .skip(dialog.page * 4)
        .take(4)
        .copied()
        .collect::<Vec<_>>();
    if dialog.position == DialogPosition::CenterWindow {
        render_center_dialog_window(renderer, font, dialog, &visible);
        return;
    }

    let mut last_end = None;
    for (line, bytes) in visible.iter().enumerate() {
        let y = layout.text_y + line as i32 * 18;
        let end = draw_dialog_text(renderer, font, bytes, layout.text_x, y, dialog.font_color);
        last_end = Some((end, y));
    }
    if dialog.awaiting_input {
        if let Some((x, y)) = last_end {
            draw_dialog_wait_icon(renderer, x + 2, y + 5);
        }
    }
}

fn render_center_dialog_window(
    renderer: &mut Renderer,
    font: &BitmapFont,
    dialog: &ActiveDialog,
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
    let mut last_end = None;
    for (line, bytes) in lines.iter().enumerate() {
        let text_x = x + (width - dialog_text_width(bytes) as i32) / 2;
        let text_y = y + 10 + line as i32 * 18;
        let end = draw_dialog_text(renderer, font, bytes, text_x, text_y, dialog.font_color);
        last_end = Some((end, text_y));
    }
    if dialog.awaiting_input {
        if let Some((end, text_y)) = last_end {
            draw_dialog_wait_icon(renderer, end + 2, text_y + 5);
        }
    }
}
