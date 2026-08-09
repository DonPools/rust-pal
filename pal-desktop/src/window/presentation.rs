use pal_assets::battle::{BattleEffects, BattleSpriteArchive};
use pal_assets::bitmap::Bitmap;
use pal_assets::palette::{Palette, PaletteSet};
use pal_assets::rle::RleBitmap;
use pal_assets::text::{BitmapFont, ItemDescriptions, TextLibrary};
use pal_core::battle::BattleEvent;
use pal_core::game::GameState;
use pal_core::role::RoleSprites;
use pal_core::script::ScriptDebugSnapshot;

use super::battle_render::BattleMenuState;
use super::battle_render::{
    render_battle_frame, render_post_battle_pages, BattleRenderResources, BattleRenderState,
    PostBattlePresentation,
};
use super::debug_render::{focused_debug_object, render_collision_overlay, render_object_overlay};
use super::dialog::{render_dialog, ActiveDialog};
use super::menu_render::{
    render_confirmation_menu, render_field_menu, render_inventory_menu, render_opening_menu,
    render_shop_menu, render_status_menu,
};
use super::menu_state::{ActiveMenu, OpeningMenu};
use super::scene_render::render_tile_map;
use super::state::AppModeView;
use super::visual::VisualState;
use super::Viewport;
use crate::renderer::Renderer;

#[derive(Clone, Copy)]
pub(super) struct UiRenderContext<'a> {
    pub(super) app_mode: AppModeView<'a>,
    pub(super) opening_background: &'a Bitmap,
    pub(super) dialog: Option<&'a ActiveDialog>,
    pub(super) active_menu: Option<&'a ActiveMenu>,
    pub(super) text: &'a TextLibrary,
    pub(super) font: &'a BitmapFont,
    pub(super) item_descriptions: &'a ItemDescriptions,
    pub(super) dialog_faces: &'a [Option<RleBitmap>],
    pub(super) dialog_icons: &'a [RleBitmap],
    pub(super) ui_sprites: &'a [RleBitmap],
    pub(super) item_sprites: &'a [Option<RleBitmap>],
    pub(super) enemy_battle_sprites: &'a BattleSpriteArchive,
    pub(super) player_battle_sprites: &'a BattleSpriteArchive,
    pub(super) magic_effect_sprites: &'a BattleSpriteArchive,
    pub(super) battle_effects: &'a BattleEffects,
    pub(super) battle_backgrounds: &'a [Option<Bitmap>],
    pub(super) battle_selected_enemy: usize,
    pub(super) battle_command_selected: usize,
    pub(super) battle_targeting_enemy: bool,
    pub(super) battle_menu: BattleMenuState,
    pub(super) battle_auto_attack: bool,
    pub(super) battle_event: Option<BattleEvent>,
    pub(super) battle_event_ticks: u16,
    pub(super) battle_kept_effects: &'a [BattleEvent],
    pub(super) post_battle: Option<&'a PostBattlePresentation>,
    pub(super) status_background: &'a Bitmap,
    pub(super) equip_background: &'a Bitmap,
    pub(super) ui_ticks: u64,
    pub(super) battle_render_ticks: u64,
    pub(super) palettes: &'a [PaletteSet],
    pub(super) visual: &'a VisualState,
}

pub(super) fn render_game(
    renderer: &mut Renderer,
    game: &GameState,
    role_sprites: &RoleSprites,
    show_collision: bool,
    show_objects: bool,
    script: ScriptDebugSnapshot,
    ui: UiRenderContext<'_>,
) {
    match ui.app_mode {
        AppModeView::OpeningAnimation(animation) => animation
            .render(renderer, ui.palettes)
            .expect("failed to render original opening animation"),
        AppModeView::OpeningMenu(menu) => {
            render_opening_menu_frame(renderer, role_sprites, menu, &ui)
        }
        AppModeView::Playing => render_playing_frame(
            renderer,
            game,
            role_sprites,
            show_collision,
            show_objects,
            script,
            &ui,
        ),
    }
}

fn render_opening_menu_frame(
    renderer: &mut Renderer,
    role_sprites: &RoleSprites,
    menu: &OpeningMenu,
    ui: &UiRenderContext<'_>,
) {
    prepare_palette(renderer, ui);
    if !ui.visual.render_override(renderer) {
        render_opening_menu(
            renderer,
            ui.opening_background,
            ui.text,
            ui.font,
            ui.ui_sprites,
            *menu,
            ui.ui_ticks,
        );
    }
    ui.visual.apply_post_effects(renderer, role_sprites);
}

fn render_playing_frame(
    renderer: &mut Renderer,
    game: &GameState,
    role_sprites: &RoleSprites,
    show_collision: bool,
    show_objects: bool,
    script: ScriptDebugSnapshot,
    ui: &UiRenderContext<'_>,
) {
    prepare_palette(renderer, ui);
    if !ui.visual.render_override(renderer) {
        render_battle_or_world(
            renderer,
            game,
            role_sprites,
            show_collision,
            show_objects,
            script,
            ui,
        );
    }
    render_dialog_or_menu(renderer, game, ui);
    ui.visual.apply_post_effects(renderer, role_sprites);
}

fn prepare_palette(renderer: &mut Renderer, ui: &UiRenderContext<'_>) {
    if let Ok(mut palette) = ui.visual.palette(ui.palettes) {
        if ui.dialog.is_some_and(|dialog| {
            dialog.awaiting_input
                && !matches!(
                    dialog.position,
                    pal_core::script::DialogPosition::Center
                        | pal_core::script::DialogPosition::CenterWindow
                )
        }) {
            cycle_dialog_icon_palette(
                &mut palette,
                ui.dialog.map_or(0, |dialog| dialog.wait_palette_ticks),
            );
        }
        renderer.set_palette(&palette);
    }
}

#[allow(clippy::too_many_arguments)]
fn render_battle_or_world(
    renderer: &mut Renderer,
    game: &GameState,
    role_sprites: &RoleSprites,
    show_collision: bool,
    show_objects: bool,
    script: ScriptDebugSnapshot,
    ui: &UiRenderContext<'_>,
) {
    if let Some(battle) = ui
        .post_battle
        .map(|presentation| &presentation.battle)
        .or_else(|| game.battle())
    {
        let settlement_visible = ui
            .post_battle
            .is_none_or(|presentation| presentation.visible_pages().0);
        render_battle_frame(
            renderer,
            battle,
            BattleRenderResources {
                enemy_sprites: ui.enemy_battle_sprites,
                player_sprites: ui.player_battle_sprites,
                magic_effect_sprites: ui.magic_effect_sprites,
                battle_effects: ui.battle_effects,
                backgrounds: ui.battle_backgrounds,
                text: ui.text,
                font: ui.font,
                ui_sprites: ui.ui_sprites,
                cash: game.cash,
            },
            BattleRenderState {
                selected_enemy: ui.battle_selected_enemy,
                selected_command: ui.battle_command_selected,
                targeting_enemy: ui.battle_targeting_enemy,
                menu: ui.battle_menu,
                auto_attack: ui.battle_auto_attack,
                ticks: ui.battle_render_ticks,
                event: ui
                    .post_battle
                    .is_none()
                    .then_some(ui.battle_event)
                    .flatten(),
                event_ticks: if ui.post_battle.is_some() {
                    0
                } else {
                    ui.battle_event_ticks
                },
                kept_effects: ui.battle_kept_effects,
            },
            settlement_visible,
        );
        if let Some(presentation) = ui.post_battle {
            render_post_battle_pages(renderer, presentation, ui.ui_sprites, ui.text, ui.font);
        } else if let BattleMenuState::Status { selected } = ui.battle_menu {
            render_status_menu(
                renderer,
                game,
                ui.text,
                ui.font,
                ui.dialog_faces,
                ui.ui_sprites,
                ui.item_sprites,
                ui.status_background,
                selected,
                Some(battle),
            );
        }
        return;
    }

    let viewport = Viewport::from(game.camera);
    let roles = std::iter::once(&game.player)
        .chain(game.party_followers())
        .cloned()
        .collect::<Vec<_>>();
    render_tile_map(
        renderer,
        &game.map,
        Some(role_sprites),
        &roles,
        game.party_layer(),
        &game.scene_objects,
        viewport,
    );
    if show_collision {
        render_collision_overlay(renderer, &game.map, &game.player, viewport);
    }
    if show_objects {
        let focused_object_id = focused_debug_object(game, script).map(|object| object.id);
        render_object_overlay(renderer, &game.scene_objects, viewport, focused_object_id);
    }
}

fn render_dialog_or_menu(renderer: &mut Renderer, game: &GameState, ui: &UiRenderContext<'_>) {
    if let Some(dialog) = ui.dialog {
        render_dialog(
            renderer,
            ui.text,
            ui.font,
            ui.dialog_faces,
            ui.dialog_icons,
            ui.ui_sprites,
            dialog,
        );
    } else if let Some(menu) = ui.active_menu {
        match *menu {
            ActiveMenu::Confirmation(menu) => render_confirmation_menu(
                renderer,
                ui.text,
                ui.font,
                ui.ui_sprites,
                menu,
                ui.ui_ticks,
            ),
            ActiveMenu::Field(menu) => render_field_menu(
                renderer,
                game,
                ui.text,
                ui.font,
                ui.dialog_faces,
                ui.ui_sprites,
                ui.item_sprites,
                ui.status_background,
                menu,
                ui.ui_ticks,
            ),
            ActiveMenu::Shop(menu) => render_shop_menu(
                renderer,
                game,
                ui.text,
                ui.font,
                ui.ui_sprites,
                ui.item_sprites,
                menu,
                ui.ui_ticks,
            ),
            ActiveMenu::Inventory(menu) => render_inventory_menu(
                renderer,
                game,
                ui.text,
                ui.font,
                ui.item_descriptions,
                ui.ui_sprites,
                ui.item_sprites,
                ui.equip_background,
                menu,
                ui.ui_ticks,
            ),
        }
    }
}

pub(super) fn cycle_dialog_icon_palette(palette: &mut Palette, ticks: u64) {
    let phase = usize::try_from((ticks / 2) % 6).unwrap_or(0);
    palette.colors[0xf9..=0xfe].rotate_left(phase);
}
