use pal_assets::battle::{BattleEffects, BattleSpriteArchive};
use pal_assets::bitmap::Bitmap;
use pal_assets::palette::{Palette, PaletteSet};
use pal_assets::rle::RleBitmap;
use pal_assets::text::{BitmapFont, TextLibrary};
use pal_core::battle::BattleEvent;
use pal_core::game::GameState;
use pal_core::role::RoleSprites;
use pal_core::script::ScriptDebugSnapshot;

use super::battle_render::BattleMenuState;
use super::battle_render::{
    render_battle, render_post_battle_page, BattleRenderResources, BattleRenderState,
    PostBattlePresentation,
};
use super::debug_render::{focused_debug_object, render_collision_overlay, render_object_overlay};
use super::dialog::{render_dialog, ActiveDialog};
use super::menu_render::{
    render_confirmation_menu, render_field_menu, render_inventory_menu, render_opening_menu,
    render_shop_menu, render_status_menu,
};
use super::menu_state::{ConfirmationMenu, FieldMenu, InventoryMenu, OpeningMenu, ShopMenu};
use super::opening_intro::OpeningIntro;
use super::scene_render::render_tile_map;
use super::visual::VisualState;
use super::Viewport;
use crate::renderer::Renderer;

#[derive(Clone, Copy)]
pub(super) struct UiRenderContext<'a> {
    pub(super) opening_intro: Option<&'a OpeningIntro>,
    pub(super) opening_menu: Option<&'a OpeningMenu>,
    pub(super) opening_background: &'a Bitmap,
    pub(super) dialog: Option<&'a ActiveDialog>,
    pub(super) field_menu: Option<&'a FieldMenu>,
    pub(super) inventory_menu: Option<&'a InventoryMenu>,
    pub(super) confirmation_menu: Option<&'a ConfirmationMenu>,
    pub(super) shop_menu: Option<&'a ShopMenu>,
    pub(super) music_enabled: bool,
    pub(super) music_volume: u8,
    pub(super) sound_enabled: bool,
    pub(super) sound_volume: u8,
    pub(super) text: &'a TextLibrary,
    pub(super) font: &'a BitmapFont,
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
    if let Some(opening_intro) = ui.opening_intro {
        opening_intro
            .render(renderer, ui.palettes)
            .expect("failed to render original opening animation");
        return;
    }
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
    let override_rendered = ui.visual.render_override(renderer);
    if let Some(opening_menu) = ui.opening_menu {
        if !override_rendered {
            render_opening_menu(
                renderer,
                ui.opening_background,
                ui.text,
                ui.font,
                ui.ui_sprites,
                *opening_menu,
                ui.ui_ticks,
            );
        }
        ui.visual.apply_post_effects(renderer, role_sprites);
        return;
    }
    if !override_rendered {
        if let Some(battle) = ui
            .post_battle
            .map(|presentation| &presentation.battle)
            .or_else(|| game.battle())
        {
            render_battle(
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
                    ticks: ui.ui_ticks,
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
            );
            if let Some(presentation) = ui.post_battle {
                render_post_battle_page(renderer, presentation, ui.ui_sprites, ui.text, ui.font);
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
        } else {
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
    }
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
    } else if let Some(menu) = ui.confirmation_menu {
        render_confirmation_menu(
            renderer,
            ui.text,
            ui.font,
            ui.ui_sprites,
            *menu,
            ui.ui_ticks,
        );
    } else if let Some(menu) = ui.field_menu {
        render_field_menu(
            renderer,
            game,
            ui.text,
            ui.font,
            ui.dialog_faces,
            ui.ui_sprites,
            ui.item_sprites,
            ui.status_background,
            *menu,
            ui.ui_ticks,
            ui.music_enabled,
            ui.music_volume,
            ui.sound_enabled,
            ui.sound_volume,
        );
    } else if let Some(menu) = ui.shop_menu {
        render_shop_menu(
            renderer,
            game,
            ui.text,
            ui.font,
            ui.ui_sprites,
            ui.item_sprites,
            *menu,
            ui.ui_ticks,
        );
    } else if let Some(menu) = ui.inventory_menu {
        render_inventory_menu(
            renderer,
            game,
            ui.text,
            ui.font,
            ui.ui_sprites,
            ui.item_sprites,
            ui.equip_background,
            *menu,
            ui.ui_ticks,
        );
    }
    ui.visual.apply_post_effects(renderer, role_sprites);
}

pub(super) fn cycle_dialog_icon_palette(palette: &mut Palette, ticks: u64) {
    let phase = usize::try_from((ticks / 2) % 6).unwrap_or(0);
    palette.colors[0xf9..=0xfe].rotate_left(phase);
}
