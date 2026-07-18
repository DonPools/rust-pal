use pal_assets::text::TextLibrary;
use pal_core::script::DialogPosition;

use super::ActiveDialog;

#[derive(Clone, Copy)]
pub(super) struct DialogLayout {
    pub(super) title_x: i32,
    pub(super) title_y: i32,
    pub(super) text_x: i32,
    pub(super) text_y: i32,
    pub(super) max_width: usize,
}

pub(super) fn dialog_layout(dialog: &ActiveDialog) -> DialogLayout {
    let has_face = dialog.face_index.is_some();
    match dialog.position {
        DialogPosition::Upper => DialogLayout {
            title_x: if has_face { 80 } else { 12 },
            title_y: 8,
            text_x: if has_face { 96 } else { 44 },
            text_y: 26,
            max_width: if has_face { 216 } else { 268 },
        },
        DialogPosition::Lower => DialogLayout {
            title_x: if has_face { 4 } else { 12 },
            title_y: 108,
            text_x: if has_face { 20 } else { 44 },
            text_y: 126,
            max_width: if has_face { 220 } else { 268 },
        },
        DialogPosition::Center => DialogLayout {
            title_x: 12,
            title_y: 8,
            text_x: 80,
            text_y: 40,
            max_width: 232,
        },
        DialogPosition::CenterWindow => DialogLayout {
            title_x: 12,
            title_y: 8,
            text_x: 160,
            text_y: 40,
            max_width: 280,
        },
    }
}

pub(super) fn dialog_page_count(text: &TextLibrary, dialog: &ActiveDialog) -> usize {
    dialog_body_lines(text, dialog).len().max(1).div_ceil(4)
}

pub(super) fn dialog_title<'a>(text: &'a TextLibrary, dialog: &ActiveDialog) -> Option<&'a [u8]> {
    if dialog.position == DialogPosition::Center {
        return None;
    }
    text.message(usize::from(*dialog.message_ids.first()?))
        .filter(|message| is_dialog_title(message))
}

pub(super) fn dialog_body_lines<'a>(text: &'a TextLibrary, dialog: &ActiveDialog) -> Vec<&'a [u8]> {
    let layout = dialog_layout(dialog);
    dialog
        .message_ids
        .iter()
        .enumerate()
        .filter_map(|(index, message_id)| {
            text.message(usize::from(*message_id)).filter(|message| {
                index != 0 || dialog.position == DialogPosition::Center || !is_dialog_title(message)
            })
        })
        .flat_map(|message| wrap_big5_lines(message, layout.max_width))
        .collect()
}

fn is_dialog_title(text: &[u8]) -> bool {
    text.ends_with(b":") || text.ends_with(&[0xa1, 0x47])
}

pub(super) fn dialog_text_width(text: &[u8]) -> usize {
    let mut width = 0;
    let mut index = 0;
    while index < text.len() {
        if text[index] == b'~' || matches!(text[index], b'\r' | b'\n') {
            break;
        }
        let (bytes, token_width) = dialog_token(text, index);
        width += token_width;
        index += bytes;
    }
    width
}

pub(super) fn wrap_big5_lines(text: &[u8], max_width: usize) -> Vec<&[u8]> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut index = 0;
    let mut width = 0;
    while index < text.len() {
        if matches!(text[index], b'\r' | b'\n') {
            lines.push(&text[start..index]);
            index += 1;
            start = index;
            width = 0;
            continue;
        }
        let (bytes, character_width) = dialog_token(text, index);
        if width + character_width > max_width && index > start {
            lines.push(&text[start..index]);
            start = index;
            width = 0;
        }
        index += bytes;
        width += character_width;
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

fn dialog_token(text: &[u8], index: usize) -> (usize, usize) {
    match text[index] {
        b'-' | b'\'' | b'@' | b'"' | b'(' | b')' | b'\\' => (1, 0),
        b'$' | b'~' => ((text.len() - index).min(3), 0),
        byte if byte >= 0x80 && index + 1 < text.len() => (2, 16),
        _ => (1, 8),
    }
}
