use std::borrow::Cow;

use pal_assets::text::TextLibrary;
use pal_core::script::DialogPosition;

use super::dialog::ActiveDialog;

#[derive(Clone, Copy)]
pub(super) struct DialogLayout {
    pub(super) title_x: i32,
    pub(super) title_y: i32,
    pub(super) text_x: i32,
    pub(super) text_y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DialogTokenKind {
    Glyph,
    Color,
    Delay(u16),
    Terminate(u16),
    Icon(u8),
    LineBreak,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DialogToken {
    pub(super) bytes: usize,
    pub(super) width: usize,
    pub(super) kind: DialogTokenKind,
}

pub(super) fn dialog_layout(dialog: &ActiveDialog) -> DialogLayout {
    let has_face = dialog.face_index.is_some();
    match dialog.position {
        DialogPosition::Upper => DialogLayout {
            title_x: if has_face { 80 } else { 12 },
            title_y: 8,
            text_x: if has_face { 96 } else { 44 },
            text_y: 26,
        },
        DialogPosition::Lower => DialogLayout {
            title_x: if has_face { 4 } else { 12 },
            title_y: 108,
            text_x: if has_face { 20 } else { 44 },
            text_y: 126,
        },
        DialogPosition::Center => DialogLayout {
            title_x: 12,
            title_y: 8,
            text_x: 80,
            text_y: 40,
        },
        DialogPosition::CenterWindow => DialogLayout {
            title_x: 12,
            title_y: 8,
            text_x: 160,
            text_y: 40,
        },
    }
}

pub(super) fn dialog_page_count(text: &TextLibrary, dialog: &ActiveDialog) -> usize {
    dialog_body_lines(text, dialog).len().max(1).div_ceil(4)
}

pub(super) fn dialog_title<'a>(text: &'a TextLibrary, dialog: &ActiveDialog) -> Option<&'a [u8]> {
    if dialog.position == DialogPosition::Center || dialog.inline_lines.is_some() {
        return None;
    }
    text.message(usize::from(*dialog.message_ids.first()?))
        .filter(|message| is_dialog_title(message))
}

pub(super) fn dialog_body_lines<'a>(
    text: &'a TextLibrary,
    dialog: &ActiveDialog,
) -> Vec<Cow<'a, [u8]>> {
    if let Some(lines) = dialog.inline_lines.as_ref() {
        return lines.iter().cloned().map(Cow::Owned).collect();
    }
    dialog
        .message_ids
        .iter()
        .enumerate()
        .filter_map(|(index, message_id)| {
            text.message(usize::from(*message_id))
                .filter(|message| {
                    index != 0
                        || dialog.position == DialogPosition::Center
                        || !is_dialog_title(message)
                })
                .map(Cow::Borrowed)
        })
        .collect()
}

fn is_dialog_title(text: &[u8]) -> bool {
    text.ends_with(b":") || text.ends_with(&[0xa1, 0x47])
}

#[cfg(test)]
pub(super) fn dialog_text_width(text: &[u8]) -> usize {
    let mut width = 0;
    let mut index = 0;
    while index < text.len() {
        let token = dialog_token(text, index);
        if matches!(
            token.kind,
            DialogTokenKind::Terminate(_) | DialogTokenKind::LineBreak
        ) {
            break;
        }
        width += token.width;
        index += token.bytes;
    }
    width
}

/// Return the original center-window width in eight-pixel units. Unlike
/// normal drawing, PAL measures control characters when sizing this box.
pub(super) fn center_window_width_units(text: &[u8]) -> usize {
    let mut units = 0;
    let mut index = 0;
    while index < text.len() {
        if text[index] >= 0x80 && index + 1 < text.len() {
            units += 2;
            index += 2;
        } else {
            units += 1;
            index += 1;
        }
    }
    units
}

pub(super) fn dialog_glyph_count(text: &[u8]) -> usize {
    let mut count = 0;
    let mut index = 0;
    while index < text.len() {
        let token = dialog_token(text, index);
        if token.kind == DialogTokenKind::Glyph {
            count += 1;
        }
        index += token.bytes;
        if matches!(token.kind, DialogTokenKind::Terminate(_)) {
            break;
        }
    }
    count
}

pub(super) fn dialog_page_glyph_target(text: &TextLibrary, dialog: &ActiveDialog) -> usize {
    dialog_body_lines(text, dialog)
        .iter()
        .take((dialog.page + 1) * 4)
        .map(|line| dialog_glyph_count(line.as_ref()))
        .sum()
}

pub(super) fn dialog_token(text: &[u8], index: usize) -> DialogToken {
    let remaining = text.len() - index;
    let numeric_control = |kind: fn(u16) -> DialogTokenKind| DialogToken {
        bytes: remaining.min(3),
        width: 0,
        kind: kind(parse_two_digits(text.get(index + 1..index + 3))),
    };
    match text[index] {
        b'\\' if remaining >= 2 => DialogToken {
            bytes: 2,
            width: 8,
            kind: DialogTokenKind::Glyph,
        },
        b'-' | b'\'' | b'@' | b'"' => DialogToken {
            bytes: 1,
            width: 0,
            kind: DialogTokenKind::Color,
        },
        b'(' => DialogToken {
            bytes: 1,
            width: 0,
            kind: DialogTokenKind::Icon(2),
        },
        b')' => DialogToken {
            bytes: 1,
            width: 0,
            kind: DialogTokenKind::Icon(1),
        },
        b'$' => numeric_control(DialogTokenKind::Delay),
        b'~' => numeric_control(DialogTokenKind::Terminate),
        b'\r' | b'\n' => DialogToken {
            bytes: 1,
            width: 0,
            kind: DialogTokenKind::LineBreak,
        },
        byte if byte >= 0x80 && remaining >= 2 => DialogToken {
            bytes: 2,
            width: 16,
            kind: DialogTokenKind::Glyph,
        },
        _ => DialogToken {
            bytes: 1,
            width: 8,
            kind: DialogTokenKind::Glyph,
        },
    }
}

fn parse_two_digits(bytes: Option<&[u8]>) -> u16 {
    let Some([tens, ones]) = bytes else {
        return 0;
    };
    if !tens.is_ascii_digit() || !ones.is_ascii_digit() {
        return 0;
    }
    u16::from(tens - b'0') * 10 + u16::from(ones - b'0')
}
