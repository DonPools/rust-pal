use std::time::Duration;

use pal_assets::text::TextLibrary;
use pal_core::role::Direction;
use pal_core::scene::SceneObject;
use pal_core::script::DialogPosition;

use super::app::{elapsed_ui_ticks, update_interval_ms};
use super::debug_render::{
    object_debug_color, OBJECT_AUTO_COLOR, OBJECT_BOTH_COLOR, OBJECT_FOCUS_COLOR,
    OBJECT_HIDDEN_COLOR, OBJECT_INERT_COLOR, OBJECT_TRIGGER_COLOR,
};
use super::dialog::{advance_dialog_playback, ActiveDialog, DialogPlayback};
use super::dialog_text::{
    center_window_width_units, dialog_body_lines, dialog_page_count, dialog_text_width,
    dialog_title,
};
use super::menu_state::{update_wrapping_selection, InventoryMenu, ShopMenu, ShopMode};
use super::presentation::cycle_dialog_icon_palette;
use super::scene_render::{cover_tile_candidate, covering_tile_y};
use super::text_render::{dialog_color_after, DialogTextMode};

fn text_library(messages: &[&[u8]]) -> TextLibrary {
    let word_data = [b' '; 10];
    let mut message_data = Vec::new();
    let mut message_index = Vec::new();
    message_index.extend_from_slice(&0u32.to_le_bytes());
    for message in messages {
        message_data.extend_from_slice(message);
        message_index.extend_from_slice(&(message_data.len() as u32).to_le_bytes());
    }
    TextLibrary::parse(&word_data, &message_data, &message_index).unwrap()
}

#[test]
fn update_clock_uses_original_scene_and_battle_rates() {
    assert_eq!(update_interval_ms(false, false, false, false), 100);
    assert_eq!(update_interval_ms(false, false, true, false), 40);
    assert_eq!(update_interval_ms(false, false, true, true), 40);
}

#[test]
fn compatibility_clocks_keep_their_existing_intervals() {
    assert_eq!(update_interval_ms(true, true, true, true), 10);
    assert_eq!(update_interval_ms(false, true, true, true), 50);
    assert_eq!(update_interval_ms(false, false, false, true), 50);
}

#[test]
fn ui_clock_uses_real_ten_millisecond_quanta() {
    assert_eq!(elapsed_ui_ticks(Duration::from_millis(9)), 0);
    assert_eq!(elapsed_ui_ticks(Duration::from_millis(10)), 1);
    assert_eq!(elapsed_ui_ticks(Duration::from_millis(99)), 9);
    assert_eq!(elapsed_ui_ticks(Duration::from_millis(100)), 10);
}

fn debug_object() -> SceneObject {
    SceneObject {
        id: 1,
        world_x: 100,
        world_y: 80,
        layer: 0,
        trigger_script: 10,
        auto_script: 20,
        state: 1,
        trigger_mode: 1,
        sprite_index: None,
        frames_per_direction: 0,
        sprite_frame_count: 0,
        direction: Direction::South,
        current_frame: 0,
        vanish_time: 0,
        auto_script_idle_frame: 0,
    }
}

#[test]
fn object_debug_colors_distinguish_script_roles_and_focus() {
    let mut object = debug_object();
    assert_eq!(object_debug_color(&object, false), OBJECT_BOTH_COLOR);

    object.auto_script = 0;
    assert_eq!(object_debug_color(&object, false), OBJECT_TRIGGER_COLOR);
    object.trigger_script = 0;
    object.auto_script = 20;
    assert_eq!(object_debug_color(&object, false), OBJECT_AUTO_COLOR);
    object.auto_script = 0;
    assert_eq!(object_debug_color(&object, false), OBJECT_INERT_COLOR);
    object.state = 0;
    assert_eq!(object_debug_color(&object, false), OBJECT_HIDDEN_COLOR);
    assert_eq!(object_debug_color(&object, true), OBJECT_FOCUS_COLOR);
}

#[test]
fn covering_tiles_are_bottom_aligned_to_their_logical_tile() {
    assert_eq!(covering_tile_y(10, 0, 15, 0), 152);
    assert_eq!(covering_tile_y(10, 1, 31, 20), 124);
}

#[test]
fn cover_tile_candidates_match_pal_scan_pattern() {
    assert_eq!(cover_tile_candidate(10, 20, 0, 0), (10, 20, 0));
    assert_eq!(cover_tile_candidate(10, 20, 0, 2), (9, 20, 1));
    assert_eq!(cover_tile_candidate(10, 20, 1, 2), (10, 21, 0));
    assert_eq!(cover_tile_candidate(10, 20, 0, 4), (10, 20, 1));
    assert_eq!(cover_tile_candidate(10, 20, 1, 4), (11, 21, 0));
}

#[test]
fn each_original_message_remains_exactly_one_dialog_line() {
    let full_width_line = [0xa4, 0x40].repeat(14);
    let text = text_library(&[b"Name:", &full_width_line, b"last"]);
    let mut dialog = ActiveDialog::new(0, DialogPosition::Upper, 0x4f, None, false, 24);
    dialog.message_ids = vec![0, 1, 2];

    let lines = dialog_body_lines(&text, &dialog);
    assert_eq!(lines, [&full_width_line, b"last".as_slice()]);
    assert_eq!(center_window_width_units(&full_width_line), 28);
}

#[test]
fn dialog_title_does_not_consume_one_of_four_body_lines() {
    let text = text_library(&[b"Name:", b"one", b"two", b"three", b"four"]);
    let mut dialog = ActiveDialog::new(0, DialogPosition::Upper, 0x4f, None, false, 24);
    dialog.message_ids = vec![0, 1, 2, 3, 4];
    dialog.awaiting_input = true;
    assert_eq!(dialog_title(&text, &dialog), Some(b"Name:".as_slice()));
    assert_eq!(dialog_body_lines(&text, &dialog).len(), 4);
    assert_eq!(dialog_page_count(&text, &dialog), 1);
}

#[test]
fn dialog_controls_do_not_consume_layout_width() {
    assert_eq!(dialog_text_width(b"A-$03B"), 16);
    assert_eq!(dialog_text_width(br"A\$B"), 24);
    assert_eq!(
        dialog_color_after(b"-cyan", 0x4f, DialogTextMode::Normal),
        0x8d
    );
    assert_eq!(
        dialog_color_after(b"still cyan-", 0x8d, DialogTextMode::Normal),
        0x4f
    );
    assert_eq!(
        dialog_color_after(br#"\"literal"#, 0x4f, DialogTextMode::Normal),
        0x4f
    );
    assert_eq!(
        dialog_color_after(b"\"quoted\"", 0x4f, DialogTextMode::CenterWindow),
        0x4f
    );
}

#[test]
fn dialog_playback_applies_speed_terminal_delay_and_icon_controls() {
    let text = text_library(&[b"A$07BC", b"A~70ignored", b"A)", b"B"]);
    let mut persistent_delay = 24;
    let mut dialog = ActiveDialog::new(0, DialogPosition::Upper, 0x4f, None, false, 24);
    assert_eq!(
        advance_dialog_playback(&text, &mut dialog, &mut persistent_delay, 24, false),
        DialogPlayback::Revealing
    );
    assert_eq!(dialog.revealed_glyphs, 1);
    assert_eq!(persistent_delay, 80);
    assert_eq!(
        advance_dialog_playback(&text, &mut dialog, &mut persistent_delay, 79, false),
        DialogPlayback::Revealing
    );
    assert_eq!(dialog.revealed_glyphs, 1);
    assert_eq!(
        advance_dialog_playback(&text, &mut dialog, &mut persistent_delay, 1, false),
        DialogPlayback::Revealing
    );
    assert_eq!(dialog.revealed_glyphs, 2);

    let mut terminal = ActiveDialog::new(1, DialogPosition::Upper, 0x4f, None, false, 24);
    assert_eq!(
        advance_dialog_playback(&text, &mut terminal, &mut persistent_delay, 0, true),
        DialogPlayback::Revealing
    );
    assert_eq!(terminal.terminal_wait_ms, Some(800));
    assert_eq!(
        advance_dialog_playback(&text, &mut terminal, &mut persistent_delay, 799, false),
        DialogPlayback::Revealing
    );
    assert_eq!(
        advance_dialog_playback(&text, &mut terminal, &mut persistent_delay, 1, false),
        DialogPlayback::AutoClose
    );

    let mut icon = ActiveDialog::new(2, DialogPosition::Upper, 0x4f, None, false, 24);
    icon.wait_after_reveal = true;
    assert_eq!(
        advance_dialog_playback(&text, &mut icon, &mut persistent_delay, 0, true),
        DialogPlayback::AwaitingInput
    );
    assert_eq!(icon.wait_icon, 1);

    let mut reset_icon = ActiveDialog::new(2, DialogPosition::Upper, 0x4f, None, false, 24);
    reset_icon.message_ids.push(3);
    reset_icon.wait_after_reveal = true;
    assert_eq!(
        advance_dialog_playback(&text, &mut reset_icon, &mut persistent_delay, 0, true),
        DialogPlayback::AwaitingInput
    );
    assert_eq!(reset_icon.wait_icon, 0);
}

#[test]
fn dialog_wait_palette_cycles_original_six_icon_colors() {
    let mut palette = pal_assets::palette::Palette::default();
    for (index, color) in palette.colors[0xf9..=0xfe].iter_mut().enumerate() {
        color.r = index as u8;
    }
    cycle_dialog_icon_palette(&mut palette, 2);
    assert_eq!(
        palette.colors[0xf9..=0xfe]
            .iter()
            .map(|color| color.r)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5, 0]
    );
}

#[test]
fn inventory_menu_uses_original_three_column_navigation_and_scrolling() {
    let mut menu = InventoryMenu::default();
    menu.update(Some(Direction::North), 10);
    assert_eq!(menu.selected, 0);
    menu.update(Some(Direction::South), 10);
    assert_eq!(menu.selected, 3);
    menu.update(Some(Direction::East), 10);
    assert_eq!(menu.selected, 4);
    menu.update(Some(Direction::West), 10);
    assert_eq!(menu.selected, 3);
    menu.update(Some(Direction::North), 10);
    assert_eq!(menu.selected, 0);

    menu.selected = 8;
    menu.update(Some(Direction::South), 10);
    assert_eq!(menu.selected, 9);
    menu.selected = 29;
    assert_eq!(menu.first_visible(30), 15);

    menu.update(None, 0);
    assert_eq!(menu.selected, 0);
    assert_eq!(menu.first_visible(0), 0);
}

#[test]
fn main_and_target_menus_wrap_at_both_ends() {
    let mut selected = 0;
    update_wrapping_selection(&mut selected, Some(Direction::North), 4);
    assert_eq!(selected, 3);
    update_wrapping_selection(&mut selected, Some(Direction::South), 4);
    assert_eq!(selected, 0);
    update_wrapping_selection(&mut selected, Some(Direction::West), 4);
    assert_eq!(selected, 3);
    update_wrapping_selection(&mut selected, Some(Direction::East), 4);
    assert_eq!(selected, 0);

    let mut shop = ShopMenu {
        mode: ShopMode::Sell,
        selected: 0,
        confirming: false,
        selected_yes: false,
    };
    shop.update_selection(Some(Direction::North), 3);
    assert_eq!(shop.selected, 2);
    shop.update_selection(Some(Direction::South), 3);
    assert_eq!(shop.selected, 0);
}
