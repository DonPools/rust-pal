//! Rendering for field, inventory, shop, and target menus.

#![allow(clippy::too_many_arguments)]

use pal_assets::bitmap::Bitmap;
use pal_assets::rle::RleBitmap;
use pal_assets::text::{BitmapFont, TextLibrary};
use pal_core::game::GameState;

use super::draw::{draw_number, fill_rect, stroke_rect};
use super::menu_state::{
    ConfirmationMenu, FieldMenu, InventoryMenu, InventoryMode, ShopMenu, ShopMode,
    INVENTORY_COLUMNS, INVENTORY_VISIBLE_ROWS,
};
use crate::renderer::Renderer;

fn draw_ui_box(
    renderer: &mut Renderer,
    sprites: &[RleBitmap],
    x: i32,
    y: i32,
    rows: usize,
    columns: usize,
    style: usize,
) {
    draw_ui_box_with_shadow(renderer, sprites, x, y, rows, columns, style, 6);
}

fn draw_ui_box_with_shadow(
    renderer: &mut Renderer,
    sprites: &[RleBitmap],
    x: i32,
    y: i32,
    rows: usize,
    columns: usize,
    style: usize,
    shadow_offset: i32,
) {
    let base = style.saturating_mul(9);
    let Some(corner) = sprites.get(base) else {
        fill_rect(
            renderer,
            x,
            y,
            (columns + 2) as i32 * 16,
            (rows + 2) as i32 * 16,
            [8, 8, 12, 255],
        );
        stroke_rect(
            renderer,
            x,
            y,
            (columns + 2) as i32 * 16,
            (rows + 2) as i32 * 16,
            [224, 224, 208, 255],
        );
        return;
    };
    let total_rows = rows + 2;
    let total_columns = columns + 2;
    let mut y_offset = 0;
    for row in 0..total_rows {
        let edge_row = if row == 0 {
            0
        } else if row + 1 == total_rows {
            2
        } else {
            1
        };
        let mut x_offset = 0;
        for column in 0..total_columns {
            let edge_column = if column == 0 {
                0
            } else if column + 1 == total_columns {
                2
            } else {
                1
            };
            if let Some(sprite) = sprites.get(base + edge_row * 3 + edge_column) {
                if shadow_offset > 0 {
                    renderer.blit_rle_shadow(
                        sprite,
                        x + x_offset + shadow_offset,
                        y + y_offset + shadow_offset,
                    );
                }
                renderer.blit_rle(sprite, x + x_offset, y + y_offset);
                x_offset += i32::from(sprite.width);
            }
        }
        y_offset += i32::from(
            sprites
                .get(base + edge_row * 3)
                .map_or(corner.height, |sprite| sprite.height),
        );
    }
}

fn draw_single_line_box(
    renderer: &mut Renderer,
    sprites: &[RleBitmap],
    x: i32,
    y: i32,
    length: usize,
) {
    draw_single_line_box_with_shadow(renderer, sprites, x, y, length, 6);
}

fn draw_single_line_box_with_shadow(
    renderer: &mut Renderer,
    sprites: &[RleBitmap],
    x: i32,
    y: i32,
    length: usize,
    shadow_offset: i32,
) {
    let (Some(left), Some(middle), Some(right)) =
        (sprites.get(44), sprites.get(45), sprites.get(46))
    else {
        fill_rect(
            renderer,
            x,
            y,
            (length + 2) as i32 * 16,
            24,
            [8, 8, 12, 255],
        );
        stroke_rect(
            renderer,
            x,
            y,
            (length + 2) as i32 * 16,
            24,
            [224, 224, 208, 255],
        );
        return;
    };

    if shadow_offset > 0 {
        renderer.blit_rle_shadow(left, x + shadow_offset, y + shadow_offset);
    }
    renderer.blit_rle(left, x, y);
    let mut draw_x = x + i32::from(left.width);
    for _ in 0..length {
        if shadow_offset > 0 {
            renderer.blit_rle_shadow(middle, draw_x + shadow_offset, y + shadow_offset);
        }
        renderer.blit_rle(middle, draw_x, y);
        draw_x += i32::from(middle.width);
    }
    if shadow_offset > 0 {
        renderer.blit_rle_shadow(right, draw_x + shadow_offset, y + shadow_offset);
    }
    renderer.blit_rle(right, draw_x, y);
}

fn selected_color(ui_ticks: u64) -> u8 {
    0xf9 + ((ui_ticks / 10) % 6) as u8
}

#[derive(Clone, Copy)]
enum NumberColor {
    Yellow,
    Blue,
    Cyan,
}

fn draw_ui_number(
    renderer: &mut Renderer,
    sprites: &[RleBitmap],
    value: u32,
    length: usize,
    x: i32,
    y: i32,
    color: NumberColor,
) {
    let base = match color {
        NumberColor::Yellow => 19,
        NumberColor::Blue => 29,
        NumberColor::Cyan => 56,
    };
    if sprites.get(base + 9).is_none() {
        let fallback = match color {
            NumberColor::Yellow => [240, 224, 96, 255],
            NumberColor::Blue => [144, 184, 240, 255],
            NumberColor::Cyan => [96, 224, 240, 255],
        };
        draw_number(renderer, value, x + length as i32 * 6, y, fallback);
        return;
    }

    let digits = value.to_string();
    let visible = &digits[digits.len().saturating_sub(length)..];
    let mut draw_x = x + (length.saturating_sub(visible.len())) as i32 * 6;
    for digit in visible.bytes() {
        renderer.blit_rle(&sprites[base + usize::from(digit - b'0')], draw_x, y);
        draw_x += 6;
    }
}

fn draw_slash(renderer: &mut Renderer, sprites: &[RleBitmap], x: i32, y: i32) {
    if let Some(slash) = sprites.get(39) {
        renderer.blit_rle(slash, x, y);
    }
}

fn draw_item_bitmap(
    renderer: &mut Renderer,
    game: &GameState,
    item_sprites: &[Option<RleBitmap>],
    item_id: u16,
    x: i32,
    y: i32,
) {
    let Some(bitmap) = game
        .item_bitmap(item_id)
        .and_then(|index| item_sprites.get(usize::from(index)))
        .and_then(Option::as_ref)
    else {
        return;
    };
    renderer.blit_rle(bitmap, x, y);
}

fn draw_cursor(renderer: &mut Renderer, sprites: &[RleBitmap], x: i32, y: i32) {
    if let Some(cursor) = sprites.get(69) {
        renderer.blit_rle(cursor, x, y);
    }
}

fn draw_player_info_boxes(renderer: &mut Renderer, game: &GameState, sprites: &[RleBitmap]) {
    for (index, member) in game.party.members().iter().enumerate() {
        let x = 45 + index as i32 * 78;
        let y = 165;
        if let Some(box_sprite) = sprites.get(18) {
            renderer.blit_rle(box_sprite, x, y);
        }
        if let Some(face) = sprites.get(48 + usize::from(member.role_id)) {
            renderer.blit_rle(face, x - 2, y - 4);
        }
        let Some(role) = game.effective_player_role(member.role_id) else {
            continue;
        };
        draw_slash(renderer, sprites, x + 49, y + 14);
        draw_ui_number(
            renderer,
            sprites,
            u32::from(role.hp),
            4,
            x + 26,
            y + 13,
            NumberColor::Yellow,
        );
        draw_ui_number(
            renderer,
            sprites,
            u32::from(role.max_hp),
            4,
            x + 47,
            y + 16,
            NumberColor::Yellow,
        );
        draw_slash(renderer, sprites, x + 49, y + 24);
        draw_ui_number(
            renderer,
            sprites,
            u32::from(role.mp),
            4,
            x + 26,
            y + 23,
            NumberColor::Cyan,
        );
        draw_ui_number(
            renderer,
            sprites,
            u32::from(role.max_mp),
            4,
            x + 47,
            y + 26,
            NumberColor::Cyan,
        );
    }
}

pub(super) fn render_confirmation_menu(
    renderer: &mut Renderer,
    text: &TextLibrary,
    font: &BitmapFont,
    sprites: &[RleBitmap],
    menu: ConfirmationMenu,
    ui_ticks: u64,
) {
    const NO_WORD: usize = 19;
    const YES_WORD: usize = 20;
    const Y: i32 = 100;
    for (index, (word_id, selected)) in
        [(NO_WORD, !menu.selected_yes), (YES_WORD, menu.selected_yes)]
            .into_iter()
            .enumerate()
    {
        let Some(label) = text.word(word_id) else {
            continue;
        };
        let box_x = 130 + index as i32 * 75;
        draw_single_line_box(renderer, sprites, box_x, Y, 2);
        renderer.draw_big5_text_shadowed(
            font,
            label,
            145 + index as i32 * 75,
            110,
            if selected {
                selected_color(ui_ticks)
            } else {
                0x4f
            },
        );
    }
}

pub(super) fn render_field_menu(
    renderer: &mut Renderer,
    game: &GameState,
    text: &TextLibrary,
    font: &BitmapFont,
    faces: &[Option<RleBitmap>],
    sprites: &[RleBitmap],
    item_sprites: &[Option<RleBitmap>],
    status_background: &Bitmap,
    menu: FieldMenu,
    ui_ticks: u64,
    music_enabled: bool,
    music_volume: u8,
    sound_enabled: bool,
    sound_volume: u8,
) {
    match menu {
        FieldMenu::Main { selected } => {
            const LABELS: [usize; 4] = [3, 4, 5, 6];
            draw_ui_box(renderer, sprites, 3, 37, 3, 3, 0);
            for (index, word_id) in LABELS.into_iter().enumerate() {
                let Some(label) = text.word(word_id) else {
                    continue;
                };
                let color = if index == selected {
                    selected_color(ui_ticks)
                } else {
                    0x4f
                };
                renderer.draw_big5_text_shadowed(font, label, 16, 50 + index as i32 * 18, color);
            }

            draw_single_line_box(renderer, sprites, 0, 0, 5);
            if let Some(label) = text.word(21) {
                renderer.draw_big5_text(font, label, 12, 11, 0);
            }
            draw_ui_number(renderer, sprites, game.cash, 6, 49, 14, NumberColor::Yellow);
        }
        FieldMenu::InventoryAction { selected } => {
            const LABELS: [usize; 2] = [22, 23];
            draw_ui_box(renderer, sprites, 30, 60, 1, 3, 0);
            for (index, word_id) in LABELS.into_iter().enumerate() {
                let Some(label) = text.word(word_id) else {
                    continue;
                };
                let color = if index == selected {
                    selected_color(ui_ticks)
                } else {
                    0x4f
                };
                renderer.draw_big5_text_shadowed(font, label, 43, 73 + index as i32 * 18, color);
            }
        }
        FieldMenu::Status { selected } => render_status_menu(
            renderer,
            game,
            text,
            font,
            faces,
            sprites,
            item_sprites,
            status_background,
            selected,
        ),
        FieldMenu::MagicCaster { selected } => render_role_selection(
            renderer, game, text, font, sprites, selected, "Magic", ui_ticks,
        ),
        FieldMenu::MagicList { caster, selected } => render_magic_list(
            renderer, game, text, font, sprites, caster, selected, ui_ticks,
        ),
        FieldMenu::MagicTarget {
            caster,
            magic_id,
            selected,
        } => {
            render_magic_list(
                renderer,
                game,
                text,
                font,
                sprites,
                caster,
                usize::MAX,
                ui_ticks,
            );
            if let Some(cursor) = sprites.get(67) {
                renderer.blit_rle(cursor, 75 + selected as i32 * 78, 158);
            }
            if let Some(name) = text.word(usize::from(magic_id)) {
                renderer.draw_big5_text_shadowed(font, name, 12, 176, 0xf9);
            }
        }
        FieldMenu::System { selected } => {
            const LABELS: [usize; 5] = [11, 12, 13, 14, 15];
            draw_ui_box(renderer, sprites, 40, 60, 4, 3, 0);
            for (index, word_id) in LABELS.into_iter().enumerate() {
                let color = if index == selected {
                    selected_color(ui_ticks)
                } else {
                    0x4f
                };
                if let Some(label) = text.word(word_id) {
                    renderer.draw_big5_text_shadowed(
                        font,
                        label,
                        53,
                        72 + index as i32 * 18,
                        color,
                    );
                }
                let audio = match index {
                    2 => Some((music_enabled, music_volume)),
                    3 => Some((sound_enabled, sound_volume)),
                    _ => None,
                };
                if let Some((enabled, volume)) = audio {
                    let value = if enabled {
                        format!("{:>3}%", volume)
                    } else {
                        " OFF".to_owned()
                    };
                    renderer.draw_big5_text_shadowed(
                        font,
                        value.as_bytes(),
                        101,
                        72 + index as i32 * 18,
                        color,
                    );
                }
            }
        }
    }
}

pub(super) fn render_role_selection(
    renderer: &mut Renderer,
    game: &GameState,
    text: &TextLibrary,
    font: &BitmapFont,
    sprites: &[RleBitmap],
    selected: usize,
    _title: &str,
    ui_ticks: u64,
) {
    draw_player_info_boxes(renderer, game, sprites);
    draw_ui_box(
        renderer,
        sprites,
        35,
        62,
        game.party.members().len().saturating_sub(1),
        6,
        0,
    );
    for (index, member) in game.party.members().iter().enumerate() {
        let Some(role) = game.player_role(member.role_id) else {
            continue;
        };
        if let Some(name) = text.word(usize::from(role.name_word_id)) {
            let color = match (index == selected, role.hp > 0) {
                (true, true) => selected_color(ui_ticks),
                (true, false) => 0x1c,
                (false, true) => 0x4f,
                (false, false) => 0x18,
            };
            renderer.draw_big5_text_shadowed(font, name, 48, 75 + index as i32 * 18, color);
        }
    }
}

pub(super) fn render_magic_list(
    renderer: &mut Renderer,
    game: &GameState,
    text: &TextLibrary,
    font: &BitmapFont,
    sprites: &[RleBitmap],
    caster: usize,
    selected: usize,
    ui_ticks: u64,
) {
    let Some(member) = game.party.members().get(caster) else {
        return;
    };
    let magics = game.field_magics(member.role_id);
    draw_player_info_boxes(renderer, game, sprites);
    draw_ui_box_with_shadow(renderer, sprites, 10, 42, 4, 16, 1, 0);
    draw_single_line_box(renderer, sprites, 0, 0, 5);
    if let Some(label) = text.word(21) {
        renderer.draw_big5_text(font, label, 10, 10, 0);
    }
    draw_ui_number(renderer, sprites, game.cash, 6, 49, 14, NumberColor::Yellow);

    draw_single_line_box(renderer, sprites, 215, 0, 5);
    if let Some(magic) = magics.get(selected.min(magics.len().saturating_sub(1))) {
        draw_ui_number(
            renderer,
            sprites,
            u32::from(magic.mp_cost),
            4,
            230,
            14,
            NumberColor::Yellow,
        );
    }
    draw_slash(renderer, sprites, 260, 14);
    draw_ui_number(
        renderer,
        sprites,
        u32::from(member.attributes.mp),
        4,
        265,
        14,
        NumberColor::Cyan,
    );

    let first = selected / (INVENTORY_COLUMNS * 5) * (INVENTORY_COLUMNS * 5);
    for (visible_index, magic) in magics.iter().skip(first).take(15).enumerate() {
        let index = first + visible_index;
        let column = index % INVENTORY_COLUMNS;
        let row = visible_index / INVENTORY_COLUMNS;
        let x = 35 + column as i32 * 100;
        let y = 54 + row as i32 * 18;
        if let Some(name) = text.word(usize::from(magic.magic_id)) {
            let color = match (index == selected, magic.enabled) {
                (true, true) => selected_color(ui_ticks),
                (true, false) => 0x1c,
                (false, true) => 0x4f,
                (false, false) => 0x18,
            };
            renderer.draw_big5_text_shadowed(font, name, x, y, color);
        }
        if index == selected {
            draw_cursor(renderer, sprites, x + 25, y + 10);
        }
    }
}

pub(super) fn render_status_menu(
    renderer: &mut Renderer,
    game: &GameState,
    text: &TextLibrary,
    font: &BitmapFont,
    faces: &[Option<RleBitmap>],
    sprites: &[RleBitmap],
    item_sprites: &[Option<RleBitmap>],
    background: &Bitmap,
    selected: usize,
) {
    let Some(member) = game.party.members().get(selected) else {
        return;
    };
    let Some(role) = game.effective_player_role(member.role_id) else {
        return;
    };
    renderer.blit_bitmap(background, 0, 0);
    if let Some(avatar) = faces.get(usize::from(role.avatar)).and_then(Option::as_ref) {
        renderer.blit_rle(avatar, 110, 30);
    }
    if let Some(name) = text.word(usize::from(role.name_word_id)) {
        renderer.draw_big5_text_shadowed(font, name, 110, 8, 0x4f);
    }
    let labels = [
        (2usize, 6, 6),
        (48, 6, 32),
        (49, 6, 54),
        (50, 6, 76),
        (51, 6, 98),
        (52, 6, 118),
        (53, 6, 138),
        (54, 6, 158),
        (55, 6, 178),
    ];
    for (word_id, x, y) in labels {
        if let Some(label) = text.word(word_id) {
            renderer.draw_big5_text_shadowed(font, label, x, y, 0x4f);
        }
    }
    draw_ui_number(
        renderer,
        sprites,
        u32::from(role.level),
        2,
        54,
        35,
        NumberColor::Yellow,
    );
    draw_ui_number(
        renderer,
        sprites,
        u32::from(role.hp),
        4,
        42,
        56,
        NumberColor::Yellow,
    );
    draw_ui_number(
        renderer,
        sprites,
        u32::from(role.max_hp),
        4,
        63,
        61,
        NumberColor::Blue,
    );
    draw_slash(renderer, sprites, 65, 58);
    draw_ui_number(
        renderer,
        sprites,
        u32::from(role.mp),
        4,
        42,
        78,
        NumberColor::Yellow,
    );
    draw_ui_number(
        renderer,
        sprites,
        u32::from(role.max_mp),
        4,
        63,
        83,
        NumberColor::Blue,
    );
    draw_slash(renderer, sprites, 65, 80);

    let values = [
        role.attack_strength,
        role.magic_strength,
        role.defense,
        role.dexterity,
        role.flee_rate,
    ];
    for (index, value) in values.into_iter().enumerate() {
        draw_ui_number(
            renderer,
            sprites,
            u32::from(value),
            4,
            42,
            102 + index as i32 * 20,
            NumberColor::Yellow,
        );
    }

    const EQUIP_BOXES: [(i32, i32); 6] = [
        (189, -1),
        (247, 39),
        (251, 101),
        (201, 133),
        (141, 141),
        (81, 125),
    ];
    const EQUIP_NAMES: [(i32, i32); 6] = [
        (195, 38),
        (253, 78),
        (257, 140),
        (207, 172),
        (147, 180),
        (87, 164),
    ];
    for (slot, &item_id) in role.equipment.iter().enumerate() {
        if item_id == 0 {
            continue;
        }
        draw_item_bitmap(
            renderer,
            game,
            item_sprites,
            item_id,
            EQUIP_BOXES[slot].0 + 1,
            EQUIP_BOXES[slot].1 + 1,
        );
        if let Some(name) = text.word(usize::from(item_id)) {
            renderer.draw_big5_text_shadowed(
                font,
                name,
                EQUIP_NAMES[slot].0,
                EQUIP_NAMES[slot].1,
                0xbe,
            );
        }
    }
}

pub(super) fn render_shop_menu(
    renderer: &mut Renderer,
    game: &GameState,
    text: &TextLibrary,
    font: &BitmapFont,
    sprites: &[RleBitmap],
    item_sprites: &[Option<RleBitmap>],
    menu: ShopMenu,
    ui_ticks: u64,
) {
    let items = menu.items(game);
    if menu.mode == ShopMode::Sell {
        render_shop_sell_menu(
            renderer,
            game,
            text,
            font,
            sprites,
            item_sprites,
            menu,
            ui_ticks,
        );
        return;
    }

    draw_ui_box(renderer, sprites, 122, 8, 8, 8, 1);
    for (row, item) in items.iter().take(9).enumerate() {
        let y = 21 + row as i32 * 18;
        if let Some(name) = text.word(usize::from(item.item_id)) {
            renderer.draw_big5_text_shadowed(
                font,
                name,
                150,
                y,
                if row == menu.selected {
                    selected_color(ui_ticks)
                } else {
                    0x4f
                },
            );
        }
        draw_ui_number(
            renderer,
            sprites,
            u32::from(item.price),
            6,
            238,
            26 + row as i32 * 18,
            NumberColor::Yellow,
        );
    }

    if let Some(item) = items.get(menu.selected) {
        if let Some(item_box) = sprites.get(70) {
            renderer.blit_rle(item_box, 40, 8);
        }
        draw_item_bitmap(renderer, game, item_sprites, item.item_id, 48, 15);

        draw_single_line_box(renderer, sprites, 20, 100, 5);
        if let Some(label) = text.word(35) {
            renderer.draw_big5_text(font, label, 30, 110, 0);
        }
        draw_ui_number(
            renderer,
            sprites,
            u32::from(game.item_count(item.item_id)),
            6,
            69,
            115,
            NumberColor::Yellow,
        );
    }
    draw_single_line_box(renderer, sprites, 20, 141, 5);
    if let Some(cash_label) = text.word(21) {
        renderer.draw_big5_text(font, cash_label, 30, 151, 0);
    }
    draw_ui_number(
        renderer,
        sprites,
        game.cash,
        6,
        69,
        156,
        NumberColor::Yellow,
    );
    if menu.confirming {
        render_confirmation_menu(
            renderer,
            text,
            font,
            sprites,
            ConfirmationMenu {
                no_entry: 0,
                selected_yes: menu.selected_yes,
            },
            ui_ticks,
        );
    }
}

fn render_shop_sell_menu(
    renderer: &mut Renderer,
    game: &GameState,
    text: &TextLibrary,
    font: &BitmapFont,
    sprites: &[RleBitmap],
    item_sprites: &[Option<RleBitmap>],
    menu: ShopMenu,
    ui_ticks: u64,
) {
    let items = menu.items(game);
    draw_ui_box_with_shadow(renderer, sprites, 2, 0, 6, 17, 1, 0);
    for (index, item) in items.iter().take(21).enumerate() {
        let column = index % 3;
        let row = index / 3;
        let x = 15 + column as i32 * 100;
        let y = 12 + row as i32 * 18;
        if let Some(name) = text.word(usize::from(item.item_id)) {
            renderer.draw_big5_text_shadowed(
                font,
                name,
                x,
                y,
                if index == menu.selected {
                    selected_color(ui_ticks)
                } else {
                    0x4f
                },
            );
        }
        let amount = game.inventory_count(item.item_id);
        if amount > 1 {
            draw_ui_number(
                renderer,
                sprites,
                u32::from(amount),
                2,
                x + 81,
                y + 5,
                NumberColor::Cyan,
            );
        }
        if index == menu.selected {
            draw_cursor(renderer, sprites, x + 25, y + 10);
            if let Some(item_box) = sprites.get(70) {
                renderer.blit_rle(item_box, 0, 140);
            }
            draw_item_bitmap(renderer, game, item_sprites, item.item_id, 8, 147);
        }
    }

    draw_single_line_box_with_shadow(renderer, sprites, 100, 150, 5, 0);
    if let Some(label) = text.word(21) {
        renderer.draw_big5_text(font, label, 110, 160, 0);
    }
    draw_ui_number(
        renderer,
        sprites,
        game.cash,
        6,
        148,
        165,
        NumberColor::Yellow,
    );
    draw_single_line_box_with_shadow(renderer, sprites, 224, 150, 5, 0);
    if let Some(item) = items.get(menu.selected) {
        if let Some(label) = text.word(25) {
            renderer.draw_big5_text(font, label, 234, 160, 0);
        }
        draw_ui_number(
            renderer,
            sprites,
            u32::from(item.price),
            6,
            272,
            165,
            NumberColor::Yellow,
        );
    }
    if menu.confirming {
        render_confirmation_menu(
            renderer,
            text,
            font,
            sprites,
            ConfirmationMenu {
                no_entry: 0,
                selected_yes: menu.selected_yes,
            },
            ui_ticks,
        );
    }
}

pub(super) fn render_inventory_menu(
    renderer: &mut Renderer,
    game: &GameState,
    text: &TextLibrary,
    font: &BitmapFont,
    sprites: &[RleBitmap],
    item_sprites: &[Option<RleBitmap>],
    equip_background: &Bitmap,
    menu: InventoryMenu,
    ui_ticks: u64,
) {
    match menu.mode {
        InventoryMode::Target { item_id, selected }
        | InventoryMode::BattleUseTarget { item_id, selected } => {
            render_item_target_menu(
                renderer,
                game,
                text,
                font,
                sprites,
                item_sprites,
                selected,
                item_id,
                false,
                equip_background,
                ui_ticks,
            );
            return;
        }
        InventoryMode::EquipTarget { item_id, selected } => {
            render_item_target_menu(
                renderer,
                game,
                text,
                font,
                sprites,
                item_sprites,
                selected,
                item_id,
                true,
                equip_background,
                ui_ticks,
            );
            return;
        }
        InventoryMode::Items
        | InventoryMode::EquipItems
        | InventoryMode::BattleUseItems
        | InventoryMode::BattleThrowItems => {}
    }

    const PANEL_X: i32 = 2;
    const PANEL_Y: i32 = 0;
    const ROW_HEIGHT: i32 = 18;
    const COLUMN_WIDTH: i32 = 100;

    let inventory = match menu.mode {
        InventoryMode::EquipItems => game.equippable_inventory(),
        InventoryMode::BattleUseItems => game
            .battle_usable_inventory()
            .into_iter()
            .map(|item| (item.item_id, item.amount))
            .collect(),
        InventoryMode::BattleThrowItems => game
            .throwable_inventory()
            .into_iter()
            .map(|item| (item.item_id, item.amount))
            .collect(),
        InventoryMode::Items => game.inventory().collect(),
        InventoryMode::EquipTarget { .. }
        | InventoryMode::Target { .. }
        | InventoryMode::BattleUseTarget { .. } => unreachable!("target menus returned above"),
    };
    let first = menu.first_visible(inventory.len());
    draw_ui_box_with_shadow(renderer, sprites, PANEL_X, PANEL_Y, 6, 17, 1, 0);
    if let Some(item_box) = sprites.get(70) {
        renderer.blit_rle(item_box, 0, 140);
    }

    for (row, &(item_id, amount)) in inventory
        .iter()
        .skip(first)
        .take(INVENTORY_VISIBLE_ROWS * INVENTORY_COLUMNS)
        .enumerate()
    {
        let column = row % INVENTORY_COLUMNS;
        let line = row / INVENTORY_COLUMNS;
        let x = PANEL_X + 13 + column as i32 * COLUMN_WIDTH;
        let y = PANEL_Y + 12 + line as i32 * ROW_HEIGHT;
        let selected = first + row == menu.selected;
        if let Some(name) = text.word(usize::from(item_id)) {
            let usable = matches!(
                menu.mode,
                InventoryMode::EquipItems
                    | InventoryMode::BattleUseItems
                    | InventoryMode::BattleThrowItems
            ) || game.usable_item(item_id).is_some();
            let color = match (selected, usable) {
                (true, true) => selected_color(ui_ticks),
                (true, false) => 0x1c,
                (false, true) => 0x4f,
                (false, false) => 0x18,
            };
            renderer.draw_big5_text_shadowed(font, name, x, y, color);
        }
        if amount > 1 {
            draw_ui_number(
                renderer,
                sprites,
                u32::from(amount),
                2,
                x + 81,
                y + 5,
                NumberColor::Cyan,
            );
        }
        if selected {
            draw_cursor(renderer, sprites, x + 25, y + 10);
            draw_item_bitmap(renderer, game, item_sprites, item_id, 8, 147);
        }
    }
}

fn render_item_target_menu(
    renderer: &mut Renderer,
    game: &GameState,
    text: &TextLibrary,
    font: &BitmapFont,
    sprites: &[RleBitmap],
    item_sprites: &[Option<RleBitmap>],
    selected: usize,
    item_id: u16,
    equipping: bool,
    equip_background: &Bitmap,
    ui_ticks: u64,
) {
    if equipping {
        render_equip_target_menu(
            renderer,
            game,
            text,
            font,
            sprites,
            item_sprites,
            equip_background,
            selected,
            item_id,
            ui_ticks,
        );
        return;
    }
    draw_ui_box(renderer, sprites, 110, 2, 7, 9, 0);
    for (index, member) in game.party.members().iter().enumerate() {
        let y = 16 + index as i32 * 20;
        if let Some(name) = text.word(usize::from(member.attributes.name_word_id)) {
            let enabled = !equipping || game.equippable_item(item_id, member.role_id).is_some();
            let color = match (index == selected, enabled) {
                (true, true) => selected_color(ui_ticks),
                (true, false) => 0x1c,
                (false, true) => 0x4f,
                (false, false) => 0x18,
            };
            renderer.draw_big5_text_shadowed(font, name, 125, y, color);
        }
    }

    let Some(member) = game.party.members().get(selected) else {
        return;
    };
    let Some(role) = game.effective_player_role(member.role_id) else {
        return;
    };
    let labels = [48usize, 49, 50, 51, 52, 53, 54, 55];
    for (index, word_id) in labels.into_iter().enumerate() {
        if let Some(label) = text.word(word_id) {
            renderer.draw_big5_text_shadowed(font, label, 200, 16 + index as i32 * 18, 0xbb);
        }
    }
    draw_ui_number(
        renderer,
        sprites,
        u32::from(role.level),
        4,
        240,
        20,
        NumberColor::Yellow,
    );
    draw_ui_number(
        renderer,
        sprites,
        u32::from(role.hp),
        4,
        240,
        37,
        NumberColor::Yellow,
    );
    draw_ui_number(
        renderer,
        sprites,
        u32::from(role.max_hp),
        4,
        261,
        40,
        NumberColor::Blue,
    );
    draw_slash(renderer, sprites, 263, 38);
    draw_ui_number(
        renderer,
        sprites,
        u32::from(role.mp),
        4,
        240,
        55,
        NumberColor::Yellow,
    );
    draw_ui_number(
        renderer,
        sprites,
        u32::from(role.max_mp),
        4,
        261,
        58,
        NumberColor::Blue,
    );
    draw_slash(renderer, sprites, 263, 56);
    for (index, value) in [
        role.attack_strength,
        role.magic_strength,
        role.defense,
        role.dexterity,
        role.flee_rate,
    ]
    .into_iter()
    .enumerate()
    {
        draw_ui_number(
            renderer,
            sprites,
            u32::from(value),
            4,
            240,
            74 + index as i32 * 18,
            NumberColor::Yellow,
        );
    }

    if let Some(item_box) = sprites.get(70) {
        renderer.blit_rle(item_box, 120, 80);
    }
    draw_item_bitmap(renderer, game, item_sprites, item_id, 127, 88);
    if let Some(name) = text.word(usize::from(item_id)) {
        renderer.draw_big5_text_shadowed(font, name, 116, 143, 0xbe);
    }
    draw_ui_number(
        renderer,
        sprites,
        u32::from(game.inventory_count(item_id)),
        2,
        170,
        133,
        NumberColor::Cyan,
    );
}

fn render_equip_target_menu(
    renderer: &mut Renderer,
    game: &GameState,
    text: &TextLibrary,
    font: &BitmapFont,
    sprites: &[RleBitmap],
    item_sprites: &[Option<RleBitmap>],
    background: &Bitmap,
    selected: usize,
    item_id: u16,
    ui_ticks: u64,
) {
    renderer.blit_bitmap(background, 0, 0);
    draw_item_bitmap(renderer, game, item_sprites, item_id, 16, 16);
    if let Some(name) = text.word(usize::from(item_id)) {
        renderer.draw_big5_text_shadowed(font, name, 5, 70, 0x2c);
    }
    draw_ui_number(
        renderer,
        sprites,
        u32::from(game.inventory_count(item_id)),
        2,
        51,
        57,
        NumberColor::Cyan,
    );

    let Some(member) = game.party.members().get(selected) else {
        return;
    };
    let Some(role) = game.effective_player_role(member.role_id) else {
        return;
    };
    for (index, value) in [
        role.attack_strength,
        role.magic_strength,
        role.defense,
        role.dexterity,
        role.flee_rate,
    ]
    .into_iter()
    .enumerate()
    {
        draw_ui_number(
            renderer,
            sprites,
            u32::from(value),
            4,
            260,
            14 + index as i32 * 22,
            NumberColor::Cyan,
        );
    }
    for slot in 0..6 {
        if let Some(current) = role.equipment.get(slot).copied().filter(|&id| id != 0) {
            if let Some(label) = text.word(usize::from(current)) {
                renderer.draw_big5_text_shadowed(font, label, 130, 11 + slot as i32 * 22, 0x4f);
            }
        }
    }
    draw_ui_box(
        renderer,
        sprites,
        2,
        95,
        game.party.members().len().saturating_sub(1),
        4,
        0,
    );
    for (index, party_member) in game.party.members().iter().enumerate() {
        if let Some(label) = text.word(usize::from(party_member.attributes.name_word_id)) {
            let enabled = game
                .equippable_item(item_id, party_member.role_id)
                .is_some();
            let color = if index == selected {
                if enabled {
                    selected_color(ui_ticks)
                } else {
                    0x1c
                }
            } else if enabled {
                0x4f
            } else {
                0x18
            };
            renderer.draw_big5_text_shadowed(font, label, 15, 108 + index as i32 * 18, color);
        }
    }
}
