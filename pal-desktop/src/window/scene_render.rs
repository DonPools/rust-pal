use pal_assets::rle::RleBitmap;
use pal_core::map::{Map, MAP_COLUMNS, MAP_ROWS};
use pal_core::role::{Role, RoleSprites};
use pal_core::scene::SceneObject;

use super::draw::RenderBounds;
use super::{Renderer, Viewport};

pub(super) struct DepthSprite {
    pub(super) bitmap: RleBitmap,
    pub(super) x: i32,
    pub(super) y: i32,
    pub(super) depth: i32,
}

/// Render the map and depth-sort characters with elevated covering tiles.
pub fn render_tile_map(
    renderer: &mut Renderer,
    map: &Map,
    role_sprites: Option<&RoleSprites>,
    roles: &[Role],
    party_layer: u16,
    scene_objects: &[SceneObject],
    viewport: Viewport,
) {
    render_tile_map_with_wave(
        renderer,
        map,
        role_sprites,
        roles,
        party_layer,
        scene_objects,
        viewport,
        None,
    );
}

/// Render a scene with Classic's map-only wave between the map and sprite passes.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_tile_map_with_wave(
    renderer: &mut Renderer,
    map: &Map,
    role_sprites: Option<&RoleSprites>,
    roles: &[Role],
    party_layer: u16,
    scene_objects: &[SceneObject],
    viewport: Viewport,
    wave: Option<(u16, i16)>,
) {
    renderer.clear_black();
    let bounds = RenderBounds::for_viewport(viewport);
    render_tile_layer(renderer, map, viewport, bounds, false);
    render_tile_layer(renderer, map, viewport, bounds, true);
    if let Some((level, phase)) = wave {
        renderer.apply_wave(level, phase);
    }
    render_depth_sorted_sprites(
        renderer,
        map,
        role_sprites,
        roles,
        party_layer,
        scene_objects,
        viewport,
        bounds,
    );
}

#[allow(clippy::too_many_arguments)]
fn render_depth_sorted_sprites(
    renderer: &mut Renderer,
    map: &Map,
    role_sprites: Option<&RoleSprites>,
    roles: &[Role],
    party_layer: u16,
    scene_objects: &[SceneObject],
    viewport: Viewport,
    bounds: RenderBounds,
) {
    let mut sprites = Vec::new();

    if let Some(role_sprites) = role_sprites {
        let party_layer = i32::from(party_layer);
        for role in roles {
            let (anchor_x, anchor_y) = role.screen_anchor();
            if anchor_y < bounds.start_y * 16 || anchor_y >= bounds.end_y * 16 {
                continue;
            }
            if let Some(bitmap) = role_sprites.decode_role_frame(role) {
                let (cover_x, cover_y) =
                    party_cover_anchor(role.world_x, role.world_y, bitmap.width, party_layer);
                add_covering_tiles(
                    &mut sprites,
                    map,
                    viewport,
                    cover_x,
                    cover_y,
                    bitmap.width,
                    bitmap.height,
                );
                sprites.push(DepthSprite {
                    x: anchor_x - bitmap.width as i32 / 2 - viewport.x,
                    y: anchor_y - bitmap.height as i32 - viewport.y,
                    // PAL sorts party sprites six logical pixels below the
                    // rendered foot anchor.
                    depth: party_sprite_depth(anchor_y, party_layer),
                    bitmap,
                });
            }
        }

        for object in scene_objects.iter().filter(|object| object.is_visible()) {
            let (Some(sprite_index), Some(frame_index)) =
                (object.sprite_index, object.frame_index())
            else {
                continue;
            };
            let Some(bitmap) = role_sprites.decode_frame(sprite_index, frame_index) else {
                continue;
            };
            let layer = i32::from(object.layer) * 8 + 2;
            let draw_x = object.world_x - i32::from(bitmap.width) / 2;
            let draw_y = object.world_y + 7 - i32::from(bitmap.height);
            add_covering_tiles(
                &mut sprites,
                map,
                viewport,
                draw_x - layer / 2,
                object.world_y + 7,
                bitmap.width,
                bitmap.height,
            );
            sprites.push(DepthSprite {
                bitmap,
                x: draw_x - viewport.x,
                y: draw_y - viewport.y,
                depth: object.world_y + i32::from(object.layer) * 8 + 9,
            });
        }
    }

    sprites.sort_by_key(|sprite| sprite.depth);
    for sprite in sprites {
        renderer.blit_rle(&sprite.bitmap, sprite.x, sprite.y);
    }
}

fn party_sprite_depth(anchor_y: i32, party_layer: i32) -> i32 {
    anchor_y + party_layer + 6
}

fn party_cover_anchor(
    world_x: i32,
    world_y: i32,
    bitmap_width: u16,
    party_layer: i32,
) -> (i32, i32) {
    (
        world_x - i32::from(bitmap_width) / 2 - (party_layer + 6) / 2,
        world_y + 4,
    )
}

fn add_covering_tiles(
    sprites: &mut Vec<DepthSprite>,
    map: &Map,
    viewport: Viewport,
    sx: i32,
    sy: i32,
    sprite_width: u16,
    sprite_height: u16,
) {
    let width = i32::from(sprite_width);
    let height = i32::from(sprite_height);
    let half = i32::from(sx % 32 != 0);

    for scan_y in (sy - height - 15) / 16..=sy / 16 {
        let first_x = (sx - width / 2) / 32;
        let last_x = (sx + width / 2) / 32;
        for scan_x in first_x..=last_x {
            let first_candidate = if scan_x == first_x { 0 } else { 3 };
            for candidate in first_candidate..5 {
                let (tile_x, tile_y, tile_half) =
                    cover_tile_candidate(scan_x, scan_y, half, candidate);
                let (Ok(x_index), Ok(y_index), Ok(half_index)) = (
                    usize::try_from(tile_x),
                    usize::try_from(tile_y),
                    usize::try_from(tile_half),
                ) else {
                    continue;
                };
                if x_index >= MAP_COLUMNS || y_index >= MAP_ROWS || half_index >= 2 {
                    continue;
                }

                for top in [false, true] {
                    let Some(tile_height) = map.tile_height(x_index, y_index, half_index, top)
                    else {
                        continue;
                    };
                    if tile_height == 0
                        || (tile_y + i32::from(tile_height)) * 16 + tile_half * 8 < sy
                    {
                        continue;
                    }
                    let bitmap = if top {
                        map.decode_top_tile(x_index, y_index, half_index)
                    } else {
                        map.decode_bottom_tile(x_index, y_index, half_index)
                    };
                    if let Some(bitmap) = bitmap {
                        let layer = i32::from(top);
                        sprites.push(DepthSprite {
                            x: tile_x * 32 + tile_half * 16 - 16 - viewport.x,
                            y: covering_tile_y(tile_y, tile_half, bitmap.height, viewport.y),
                            depth: tile_y * 16
                                + tile_half * 8
                                + 7
                                + layer
                                + i32::from(tile_height) * 8,
                            bitmap,
                        });
                    }
                }
            }
        }
    }
}

pub(super) fn cover_tile_candidate(x: i32, y: i32, half: i32, candidate: i32) -> (i32, i32, i32) {
    match candidate {
        0 => (x, y, half),
        1 => (x - 1, y, half),
        2 if half != 0 => (x, y + 1, 0),
        2 => (x - 1, y, 1),
        3 => (x + 1, y, half),
        4 if half != 0 => (x + 1, y + 1, 0),
        4 => (x, y, 1),
        _ => unreachable!("cover tile candidate must be in 0..5"),
    }
}

pub(super) fn covering_tile_y(tile_y: i32, half: i32, bitmap_height: u16, viewport_y: i32) -> i32 {
    tile_y * 16 + half * 8 + 7 - i32::from(bitmap_height) - viewport_y
}

fn render_tile_layer(
    renderer: &mut Renderer,
    map: &Map,
    viewport: Viewport,
    bounds: RenderBounds,
    top: bool,
) {
    for y in bounds.start_y..bounds.end_y {
        for h in 0..2i32 {
            render_tile_row(renderer, map, viewport, bounds, y, h, top);
        }
    }
}

fn render_tile_row(
    renderer: &mut Renderer,
    map: &Map,
    viewport: Viewport,
    bounds: RenderBounds,
    y: i32,
    h: i32,
    top: bool,
) {
    for x in bounds.start_x..bounds.end_x {
        let (Ok(x_index), Ok(y_index)) = (usize::try_from(x), usize::try_from(y)) else {
            continue;
        };
        if x_index >= MAP_COLUMNS || y_index >= MAP_ROWS {
            continue;
        }
        let bitmap = if top {
            map.decode_top_tile(x_index, y_index, h as usize)
        } else {
            map.decode_bottom_tile(x_index, y_index, h as usize)
        };
        if let Some(bitmap) = bitmap {
            renderer.blit_rle(
                &bitmap,
                x * 32 + h * 16 - 16 - viewport.x,
                y * 16 + h * 8 - 8 - viewport.y,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{party_cover_anchor, party_sprite_depth};

    #[test]
    fn party_layer_changes_depth_in_eight_pixel_units() {
        assert_eq!(party_sprite_depth(120, 0), 126);
        assert_eq!(party_sprite_depth(120, 24), 150);
    }

    #[test]
    fn party_layer_shifts_cover_scan_horizontally_but_not_vertically() {
        assert_eq!(party_cover_anchor(160, 120, 20, 0), (147, 124));
        assert_eq!(party_cover_anchor(160, 120, 20, 24), (135, 124));
    }
}
