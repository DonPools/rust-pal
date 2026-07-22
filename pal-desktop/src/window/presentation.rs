use pal_assets::bitmap::Bitmap;
use pal_assets::rle::RleBitmap;
use pal_assets::text::{BitmapFont, TextLibrary};
use pal_core::game::GameState;
use pal_core::role::RoleSprites;
use pal_core::script::ScriptDebugSnapshot;

use super::debug_render::{focused_debug_object, render_collision_overlay, render_object_overlay};
use super::dialog::{render_dialog, ActiveDialog};
use super::menu_render::{
    render_confirmation_menu, render_field_menu, render_inventory_menu, render_shop_menu,
};
use super::menu_state::{ConfirmationMenu, FieldMenu, InventoryMenu, ShopMenu};
use super::scene_render::render_tile_map;
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
    pub(super) status_background: &'a Bitmap,
    pub(super) equip_background: &'a Bitmap,
    pub(super) ui_ticks: u64,
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
}
