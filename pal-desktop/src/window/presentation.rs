use pal_assets::battle::BattleSpriteArchive;
use pal_assets::bitmap::Bitmap;
use pal_assets::palette::PaletteSet;
use pal_assets::rle::RleBitmap;
use pal_assets::text::{BitmapFont, TextLibrary};
use pal_core::battle::BattleEvent;
use pal_core::game::GameState;
use pal_core::role::RoleSprites;
use pal_core::script::ScriptDebugSnapshot;

use super::battle_render::{render_battle, BattleRenderResources, BattleRenderState};
use super::debug_render::{focused_debug_object, render_collision_overlay, render_object_overlay};
use super::dialog::{render_dialog, ActiveDialog};
use super::menu_render::{
    render_confirmation_menu, render_field_menu, render_inventory_menu, render_shop_menu,
};
use super::menu_state::{ConfirmationMenu, FieldMenu, InventoryMenu, ShopMenu};
use super::scene_render::render_tile_map;
use super::visual::VisualState;
use super::Viewport;
use crate::renderer::Renderer;

#[derive(Clone, Copy)]
pub(super) struct UiRenderContext<'a> {
    pub(super) dialog: Option<&'a ActiveDialog>,
    pub(super) field_menu: Option<&'a FieldMenu>,
    pub(super) inventory_menu: Option<&'a InventoryMenu>,
    pub(super) confirmation_menu: Option<&'a ConfirmationMenu>,
    pub(super) shop_menu: Option<&'a ShopMenu>,
    pub(super) text: &'a TextLibrary,
    pub(super) font: &'a BitmapFont,
    pub(super) dialog_faces: &'a [Option<RleBitmap>],
    pub(super) ui_sprites: &'a [RleBitmap],
    pub(super) item_sprites: &'a [Option<RleBitmap>],
    pub(super) enemy_battle_sprites: &'a BattleSpriteArchive,
    pub(super) player_battle_sprites: &'a BattleSpriteArchive,
    pub(super) battle_backgrounds: &'a [Option<Bitmap>],
    pub(super) battle_selected_enemy: usize,
    pub(super) battle_command_selected: usize,
    pub(super) battle_event: Option<BattleEvent>,
    pub(super) battle_event_ticks: u16,
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
    if let Ok(palette) = ui.visual.palette(ui.palettes) {
        renderer.set_palette(&palette);
    }
    let override_rendered = ui.visual.render_override(renderer);
    if !override_rendered {
        if let Some(battle) = game.battle() {
            render_battle(
                renderer,
                battle,
                BattleRenderResources {
                    enemy_sprites: ui.enemy_battle_sprites,
                    player_sprites: ui.player_battle_sprites,
                    backgrounds: ui.battle_backgrounds,
                    text: ui.text,
                    font: ui.font,
                },
                BattleRenderState {
                    selected_enemy: ui.battle_selected_enemy,
                    selected_command: ui.battle_command_selected,
                    ticks: ui.ui_ticks,
                    event: ui.battle_event,
                    event_ticks: ui.battle_event_ticks,
                },
            );
            ui.visual.apply_post_effects(renderer, role_sprites);
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
    if let Some(dialog) = ui.dialog {
        render_dialog(renderer, ui.text, ui.font, ui.dialog_faces, dialog);
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
