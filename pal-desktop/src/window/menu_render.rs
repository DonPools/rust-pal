//! Rendering for field, inventory, shop, and target menus.

use pal_assets::text::{BitmapFont, TextLibrary};
use pal_core::game::GameState;

use super::draw::{draw_number, fill_rect, stroke_rect};
use super::menu_state::{
    ConfirmationMenu, FieldMenu, InventoryMenu, InventoryMode, ShopMenu, INVENTORY_COLUMNS,
    INVENTORY_VISIBLE_ROWS,
};
use super::text_render::draw_dialog_ascii;
use crate::renderer::Renderer;

pub(super) fn render_confirmation_menu(
    renderer: &mut Renderer,
    text: &TextLibrary,
    font: &BitmapFont,
    menu: ConfirmationMenu,
) {
    const NO_WORD: usize = 19;
    const YES_WORD: usize = 20;
    const X: i32 = 120;
    const Y: i32 = 92;
    fill_rect(renderer, X, Y, 80, 32, [8, 8, 12, 255]);
    stroke_rect(renderer, X, Y, 80, 32, [224, 224, 208, 255]);
    for (index, (word_id, selected)) in
        [(NO_WORD, !menu.selected_yes), (YES_WORD, menu.selected_yes)]
            .into_iter()
            .enumerate()
    {
        let Some(label) = text.word(word_id) else {
            continue;
        };
        let x = X + 10 + index as i32 * 38;
        if selected {
            fill_rect(renderer, x - 4, Y + 6, 34, 20, [48, 48, 56, 255]);
        }
        renderer.draw_big5_text(font, label, x, Y + 8, if selected { 0x2d } else { 0x4f });
    }
}

pub(super) fn render_field_menu(
    renderer: &mut Renderer,
    game: &GameState,
    text: &TextLibrary,
    font: &BitmapFont,
    menu: FieldMenu,
) {
    match menu {
        FieldMenu::Main { selected } => {
            const LABELS: [usize; 4] = [3, 4, 5, 6];
            fill_rect(renderer, 3, 37, 90, 82, [8, 8, 12, 255]);
            stroke_rect(renderer, 3, 37, 90, 82, [224, 224, 208, 255]);
            for (index, word_id) in LABELS.into_iter().enumerate() {
                let Some(label) = text.word(word_id) else {
                    continue;
                };
                let color = if index == selected { 0xf9 } else { 0x4f };
                renderer.draw_big5_text(font, label, 16, 50 + index as i32 * 18, color);
            }

            fill_rect(renderer, 3, 4, 112, 28, [8, 8, 12, 255]);
            stroke_rect(renderer, 3, 4, 112, 28, [224, 224, 208, 255]);
            if let Some(label) = text.word(21) {
                renderer.draw_big5_text(font, label, 12, 11, 0x4f);
            }
            draw_number(renderer, game.cash, 104, 16, [240, 224, 96, 255]);
        }
        FieldMenu::InventoryAction { selected } => {
            const LABELS: [usize; 2] = [22, 23];
            fill_rect(renderer, 30, 60, 82, 48, [8, 8, 12, 255]);
            stroke_rect(renderer, 30, 60, 82, 48, [224, 224, 208, 255]);
            for (index, word_id) in LABELS.into_iter().enumerate() {
                let Some(label) = text.word(word_id) else {
                    continue;
                };
                let color = if index == selected { 0xf9 } else { 0x4f };
                renderer.draw_big5_text(font, label, 43, 73 + index as i32 * 18, color);
            }
        }
        FieldMenu::Status { selected } => render_status_menu(renderer, game, text, font, selected),
        FieldMenu::MagicCaster { selected } => {
            render_role_selection(renderer, game, text, font, selected, "Magic")
        }
        FieldMenu::MagicList { caster, selected } => {
            render_magic_list(renderer, game, text, font, caster, selected)
        }
        FieldMenu::MagicTarget {
            caster,
            magic_id,
            selected,
        } => {
            render_magic_list(renderer, game, text, font, caster, usize::MAX);
            render_role_selection(renderer, game, text, font, selected, "Target");
            if let Some(name) = text.word(usize::from(magic_id)) {
                renderer.draw_big5_text(font, name, 12, 176, 0xf9);
            }
        }
        FieldMenu::System { selected } => {
            const LABELS: [usize; 5] = [11, 12, 13, 14, 15];
            fill_rect(renderer, 40, 60, 108, 102, [8, 8, 12, 255]);
            stroke_rect(renderer, 40, 60, 108, 102, [224, 224, 208, 255]);
            for (index, word_id) in LABELS.into_iter().enumerate() {
                if let Some(label) = text.word(word_id) {
                    renderer.draw_big5_text(
                        font,
                        label,
                        53,
                        72 + index as i32 * 18,
                        if index == selected { 0xf9 } else { 0x4f },
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
    selected: usize,
    _title: &str,
) {
    let height = 20 + game.party.members().len() as i32 * 22;
    fill_rect(renderer, 35, 62, 120, height, [8, 8, 12, 255]);
    stroke_rect(renderer, 35, 62, 120, height, [224, 224, 208, 255]);
    for (index, member) in game.party.members().iter().enumerate() {
        let Some(role) = game.player_role(member.role_id) else {
            continue;
        };
        if let Some(name) = text.word(usize::from(role.name_word_id)) {
            let color = match (index == selected, role.hp > 0) {
                (true, true) => 0xf9,
                (true, false) => 0x1c,
                (false, true) => 0x4f,
                (false, false) => 0x18,
            };
            renderer.draw_big5_text(font, name, 48, 75 + index as i32 * 22, color);
        }
        draw_number(
            renderer,
            u32::from(role.hp),
            140,
            80 + index as i32 * 22,
            [144, 224, 176, 255],
        );
    }
}

pub(super) fn render_magic_list(
    renderer: &mut Renderer,
    game: &GameState,
    text: &TextLibrary,
    font: &BitmapFont,
    caster: usize,
    selected: usize,
) {
    let Some(member) = game.party.members().get(caster) else {
        return;
    };
    let magics = game.field_magics(member.role_id);
    fill_rect(renderer, 2, 0, 316, 148, [8, 8, 12, 255]);
    stroke_rect(renderer, 2, 0, 316, 148, [224, 224, 208, 255]);
    for (index, magic) in magics.iter().enumerate() {
        let column = index % INVENTORY_COLUMNS;
        let row = index / INVENTORY_COLUMNS;
        let x = 15 + column as i32 * 100;
        let y = 12 + row as i32 * 18;
        if let Some(name) = text.word(usize::from(magic.magic_id)) {
            let color = match (index == selected, magic.enabled) {
                (true, true) => 0xf9,
                (true, false) => 0x1c,
                (false, true) => 0x4f,
                (false, false) => 0x18,
            };
            renderer.draw_big5_text(font, name, x, y, color);
        }
        draw_number(
            renderer,
            u32::from(magic.mp_cost),
            x + 88,
            y + 5,
            [96, 224, 240, 255],
        );
    }
}

pub(super) fn render_status_menu(
    renderer: &mut Renderer,
    game: &GameState,
    text: &TextLibrary,
    font: &BitmapFont,
    selected: usize,
) {
    let Some(member) = game.party.members().get(selected) else {
        return;
    };
    let Some(role) = game.effective_player_role(member.role_id) else {
        return;
    };
    fill_rect(renderer, 8, 6, 304, 188, [8, 8, 12, 255]);
    stroke_rect(renderer, 8, 6, 304, 188, [224, 224, 208, 255]);
    if let Some(name) = text.word(usize::from(role.name_word_id)) {
        renderer.draw_big5_text(font, name, 24, 18, 0xf9);
    }
    let stats = [
        (48usize, u32::from(role.level)),
        (49, u32::from(role.hp)),
        (50, u32::from(role.mp)),
        (51, u32::from(role.attack_strength)),
        (52, u32::from(role.magic_strength)),
        (53, u32::from(role.defense)),
        (54, u32::from(role.dexterity)),
        (55, u32::from(role.flee_rate)),
    ];
    for (row, (word_id, value)) in stats.into_iter().enumerate() {
        let y = 16 + row as i32 * 20;
        if let Some(label) = text.word(word_id) {
            renderer.draw_big5_text(font, label, 180, y, 0xbb);
        }
        draw_number(renderer, value, 292, y + 5, [240, 224, 96, 255]);
    }
    draw_number(
        renderer,
        u32::from(role.max_hp),
        150,
        41,
        [144, 184, 240, 255],
    );
    draw_number(
        renderer,
        u32::from(role.max_mp),
        150,
        61,
        [144, 184, 240, 255],
    );

    for (slot, &item_id) in role.equipment.iter().enumerate() {
        if item_id == 0 {
            continue;
        }
        if let Some(name) = text.word(usize::from(item_id)) {
            renderer.draw_big5_text(font, name, 24, 56 + slot as i32 * 20, 0xbe);
        }
    }
}

pub(super) fn render_shop_menu(
    renderer: &mut Renderer,
    game: &GameState,
    text: &TextLibrary,
    font: &BitmapFont,
    menu: ShopMenu,
) {
    const PANEL_X: i32 = 112;
    const PANEL_Y: i32 = 8;
    const PANEL_WIDTH: i32 = 200;
    const ROW_HEIGHT: i32 = 18;
    const CASH_WORD: usize = 21;

    let items = menu.items(game);
    fill_rect(
        renderer,
        PANEL_X,
        PANEL_Y,
        PANEL_WIDTH,
        184,
        [8, 8, 12, 255],
    );
    stroke_rect(
        renderer,
        PANEL_X,
        PANEL_Y,
        PANEL_WIDTH,
        184,
        [224, 224, 208, 255],
    );
    for (row, item) in items.iter().take(9).enumerate() {
        let y = PANEL_Y + 10 + row as i32 * ROW_HEIGHT;
        if row == menu.selected {
            stroke_rect(
                renderer,
                PANEL_X + 5,
                y - 3,
                PANEL_WIDTH - 10,
                17,
                [224, 192, 64, 255],
            );
        }
        if let Some(name) = text.word(usize::from(item.item_id)) {
            renderer.draw_big5_text(font, name, PANEL_X + 12, y, 0x4f);
        }
        draw_number(
            renderer,
            u32::from(item.price),
            PANEL_X + PANEL_WIDTH - 12,
            y + 4,
            [240, 224, 96, 255],
        );
    }
    if let Some(cash_label) = text.word(CASH_WORD) {
        renderer.draw_big5_text(font, cash_label, PANEL_X + 12, PANEL_Y + 164, 0x4f);
    }
    draw_number(
        renderer,
        game.cash,
        PANEL_X + PANEL_WIDTH - 12,
        PANEL_Y + 168,
        [240, 224, 96, 255],
    );
    if menu.confirming {
        render_confirmation_menu(
            renderer,
            text,
            font,
            ConfirmationMenu {
                no_entry: 0,
                selected_yes: menu.selected_yes,
            },
        );
    }
}

pub(super) fn render_inventory_menu(
    renderer: &mut Renderer,
    game: &GameState,
    text: &TextLibrary,
    font: &BitmapFont,
    menu: InventoryMenu,
) {
    match menu.mode {
        InventoryMode::Target { selected, .. } => {
            render_item_target_menu(renderer, game, text, font, selected, None);
            return;
        }
        InventoryMode::EquipTarget { item_id, selected } => {
            render_item_target_menu(renderer, game, text, font, selected, Some(item_id));
            return;
        }
        InventoryMode::Items | InventoryMode::EquipItems => {}
    }

    const PANEL_X: i32 = 2;
    const PANEL_Y: i32 = 0;
    const PANEL_WIDTH: i32 = 316;
    const ROW_HEIGHT: i32 = 18;
    const COLUMN_WIDTH: i32 = 100;

    let inventory = if menu.mode == InventoryMode::EquipItems {
        game.equippable_inventory()
    } else {
        game.inventory().collect::<Vec<_>>()
    };
    let first = menu.first_visible(inventory.len());
    fill_rect(
        renderer,
        PANEL_X,
        PANEL_Y,
        PANEL_WIDTH,
        136,
        [8, 8, 12, 255],
    );
    stroke_rect(
        renderer,
        PANEL_X,
        PANEL_Y,
        PANEL_WIDTH,
        136,
        [224, 224, 208, 255],
    );

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
            let usable =
                menu.mode == InventoryMode::EquipItems || game.usable_item(item_id).is_some();
            let color = match (selected, usable) {
                (true, true) => 0xf9,
                (true, false) => 0x1c,
                (false, true) => 0x4f,
                (false, false) => 0x18,
            };
            renderer.draw_big5_text(font, name, x, y, color);
        }
        if amount > 1 {
            draw_number(
                renderer,
                u32::from(amount),
                x + COLUMN_WIDTH - 10,
                y + 5,
                [96, 224, 240, 255],
            );
        }
    }
}

fn render_item_target_menu(
    renderer: &mut Renderer,
    game: &GameState,
    text: &TextLibrary,
    font: &BitmapFont,
    selected: usize,
    equip_item: Option<u16>,
) {
    const PANEL_X: i32 = 108;
    const PANEL_Y: i32 = 10;
    const PANEL_WIDTH: i32 = 204;
    const ROW_HEIGHT: i32 = 34;

    fill_rect(
        renderer,
        PANEL_X,
        PANEL_Y,
        PANEL_WIDTH,
        180,
        [8, 8, 12, 255],
    );
    stroke_rect(
        renderer,
        PANEL_X,
        PANEL_Y,
        PANEL_WIDTH,
        180,
        [224, 224, 208, 255],
    );
    for (index, member) in game.party.members().iter().enumerate() {
        let y = PANEL_Y + 8 + index as i32 * ROW_HEIGHT;
        if index == selected {
            stroke_rect(
                renderer,
                PANEL_X + 5,
                y - 3,
                PANEL_WIDTH - 10,
                ROW_HEIGHT - 2,
                [224, 192, 64, 255],
            );
        }
        if let Some(name) = text.word(usize::from(member.attributes.name_word_id)) {
            let enabled = equip_item
                .is_none_or(|item_id| game.equippable_item(item_id, member.role_id).is_some());
            let color = match (index == selected, enabled) {
                (true, true) => 0xf9,
                (true, false) => 0x1c,
                (false, true) => 0x4f,
                (false, false) => 0x18,
            };
            renderer.draw_big5_text(font, name, PANEL_X + 12, y, color);
        }
        for (offset, byte) in b"HP".iter().enumerate() {
            draw_dialog_ascii(renderer, *byte, PANEL_X + 92 + offset as i32 * 8, y, 0x4f);
        }
        draw_number(
            renderer,
            u32::from(member.attributes.hp),
            PANEL_X + 144,
            y + 4,
            [240, 224, 96, 255],
        );
        draw_number(
            renderer,
            u32::from(member.attributes.max_hp),
            PANEL_X + 188,
            y + 4,
            [144, 184, 240, 255],
        );
        for (offset, byte) in b"MP".iter().enumerate() {
            draw_dialog_ascii(
                renderer,
                *byte,
                PANEL_X + 92 + offset as i32 * 8,
                y + 16,
                0x4f,
            );
        }
        draw_number(
            renderer,
            u32::from(member.attributes.mp),
            PANEL_X + 144,
            y + 20,
            [240, 224, 96, 255],
        );
        draw_number(
            renderer,
            u32::from(member.attributes.max_mp),
            PANEL_X + 188,
            y + 20,
            [144, 184, 240, 255],
        );
    }
}
