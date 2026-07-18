use super::Viewport;
use crate::debug_overlay::glyph;
use crate::renderer::Renderer;

#[derive(Clone, Copy)]
pub(super) struct RenderBounds {
    pub(super) start_x: i32,
    pub(super) end_x: i32,
    pub(super) start_y: i32,
    pub(super) end_y: i32,
}

impl RenderBounds {
    pub(super) fn for_viewport(viewport: Viewport) -> Self {
        Self {
            start_x: viewport.x.div_euclid(32) - 1,
            end_x: (viewport.x + viewport.width as i32).div_euclid(32) + 2,
            start_y: viewport.y.div_euclid(16) - 1,
            end_y: (viewport.y + viewport.height as i32).div_euclid(16) + 2,
        }
    }
}

pub(super) fn fill_rect(
    renderer: &mut Renderer,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    color: [u8; 4],
) {
    for row in y..y + height {
        for column in x..x + width {
            renderer.put_rgba(column, row, color);
        }
    }
}

pub(super) fn stroke_rect(
    renderer: &mut Renderer,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    color: [u8; 4],
) {
    fill_rect(renderer, x, y, width, 1, color);
    fill_rect(renderer, x, y + height - 1, width, 1, color);
    fill_rect(renderer, x, y, 1, height, color);
    fill_rect(renderer, x + width - 1, y, 1, height, color);
}

pub(super) fn draw_number(
    renderer: &mut Renderer,
    value: u32,
    right_x: i32,
    y: i32,
    color: [u8; 4],
) {
    const DIGITS: [[u8; 5]; 10] = [
        [0b111, 0b101, 0b101, 0b101, 0b111],
        [0b010, 0b110, 0b010, 0b010, 0b111],
        [0b111, 0b001, 0b111, 0b100, 0b111],
        [0b111, 0b001, 0b111, 0b001, 0b111],
        [0b101, 0b101, 0b111, 0b001, 0b001],
        [0b111, 0b100, 0b111, 0b001, 0b111],
        [0b111, 0b100, 0b111, 0b101, 0b111],
        [0b111, 0b001, 0b010, 0b010, 0b010],
        [0b111, 0b101, 0b111, 0b101, 0b111],
        [0b111, 0b101, 0b111, 0b001, 0b111],
    ];

    let mut digits = [0u8; 10];
    let mut count = 0;
    let mut remaining = value;
    loop {
        digits[digits.len() - 1 - count] = (remaining % 10) as u8;
        count += 1;
        remaining /= 10;
        if remaining == 0 {
            break;
        }
    }
    let start_x = right_x - count as i32 * 5 + 1;
    for (position, &digit) in digits[digits.len() - count..].iter().enumerate() {
        for (row, bits) in DIGITS[usize::from(digit)].iter().enumerate() {
            for column in 0..3 {
                if bits & (0b100 >> column) != 0 {
                    renderer.put_rgba(
                        start_x + position as i32 * 5 + column,
                        y + row as i32,
                        color,
                    );
                }
            }
        }
    }
}

pub(super) fn draw_debug_text(renderer: &mut Renderer, x: i32, y: i32, text: &str, color: [u8; 4]) {
    draw_debug_text_pixels(renderer, x + 1, y + 1, text, [0, 0, 0, 255]);
    draw_debug_text_pixels(renderer, x, y, text, color);
}

fn draw_debug_text_pixels(renderer: &mut Renderer, x: i32, y: i32, text: &str, color: [u8; 4]) {
    for (character_index, character) in text.chars().enumerate() {
        for (row, bits) in glyph(character).iter().enumerate() {
            for column in 0..5 {
                if bits & (0b1_0000 >> column) != 0 {
                    renderer.put_rgba(
                        x + character_index as i32 * 6 + column,
                        y + row as i32,
                        color,
                    );
                }
            }
        }
    }
}

pub(super) fn draw_diamond(renderer: &mut Renderer, center_x: i32, center_y: i32, color: [u8; 4]) {
    let left = (center_x - 16, center_y);
    let top = (center_x, center_y - 8);
    let right = (center_x + 16, center_y);
    let bottom = (center_x, center_y + 8);
    for (start, end) in [(left, top), (top, right), (right, bottom), (bottom, left)] {
        draw_line(renderer, start.0, start.1, end.0, end.1, color);
    }
}

pub(super) fn draw_line(
    renderer: &mut Renderer,
    mut x0: i32,
    mut y0: i32,
    x1: i32,
    y1: i32,
    color: [u8; 4],
) {
    let dx = (x1 - x0).abs();
    let step_x = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let step_y = if y0 < y1 { 1 } else { -1 };
    let mut error = dx + dy;

    loop {
        renderer.put_rgba(x0, y0, color);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let doubled_error = error * 2;
        if doubled_error >= dy {
            error += dy;
            x0 += step_x;
        }
        if doubled_error <= dx {
            error += dx;
            y0 += step_y;
        }
    }
}
