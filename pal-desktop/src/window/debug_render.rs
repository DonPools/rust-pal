use pal_core::game::GameState;
use pal_core::map::{Map, MAP_COLUMNS, MAP_ROWS};
use pal_core::role::Role;
use pal_core::scene::SceneObject;
use pal_core::script::ScriptDebugSnapshot;

use super::draw::{draw_debug_text, draw_diamond, draw_line, RenderBounds};
use super::Viewport;
use crate::debug_overlay::DebugObjectSnapshot;
use crate::renderer::Renderer;

const BLOCKED_COLOR: [u8; 4] = [255, 48, 48, 255];
const WALKABLE_COLOR: [u8; 4] = [32, 224, 96, 255];
const PLAYER_COLLISION_COLOR: [u8; 4] = [255, 224, 32, 255];
pub(super) const OBJECT_TRIGGER_COLOR: [u8; 4] = [255, 160, 32, 255];
pub(super) const OBJECT_AUTO_COLOR: [u8; 4] = [32, 208, 255, 255];
pub(super) const OBJECT_BOTH_COLOR: [u8; 4] = [255, 96, 224, 255];
pub(super) const OBJECT_INERT_COLOR: [u8; 4] = [176, 184, 192, 255];
pub(super) const OBJECT_HIDDEN_COLOR: [u8; 4] = [96, 104, 112, 255];
pub(super) const OBJECT_FOCUS_COLOR: [u8; 4] = [255, 255, 255, 255];

pub(super) fn render_collision_overlay(
    renderer: &mut Renderer,
    map: &Map,
    player: &Role,
    viewport: Viewport,
) {
    let bounds = RenderBounds::for_viewport(viewport);
    for y in bounds.start_y..bounds.end_y {
        for h in 0..2i32 {
            for x in bounds.start_x..bounds.end_x {
                let (Ok(x_index), Ok(y_index)) = (usize::try_from(x), usize::try_from(y)) else {
                    continue;
                };
                if x_index >= MAP_COLUMNS || y_index >= MAP_ROWS {
                    continue;
                }

                let center_x = x * 32 + h * 16 - viewport.x;
                let center_y = y * 16 + h * 8 - viewport.y;
                let color = if map.is_tile_blocked(x_index, y_index, h as usize) {
                    BLOCKED_COLOR
                } else {
                    WALKABLE_COLOR
                };
                draw_diamond(renderer, center_x, center_y, color);
            }
        }
    }

    let player_x = player.world_x - viewport.x;
    let player_y = player.world_y - viewport.y;
    draw_line(
        renderer,
        player_x - 3,
        player_y,
        player_x + 3,
        player_y,
        PLAYER_COLLISION_COLOR,
    );
    draw_line(
        renderer,
        player_x,
        player_y - 3,
        player_x,
        player_y + 3,
        PLAYER_COLLISION_COLOR,
    );
}

pub(super) fn focused_debug_object(
    game: &GameState,
    script: ScriptDebugSnapshot,
) -> Option<&SceneObject> {
    script
        .next_instruction
        .or(script.last_instruction)
        .map(|instruction| instruction.object_id)
        .or_else(|| script.trigger.map(|trigger| trigger.object_id))
        .filter(|object_id| *object_id != 0xffff)
        .and_then(|object_id| {
            game.scene_objects
                .iter()
                .find(|object| object.id == object_id)
        })
}

pub(super) fn debug_object_snapshot(object: &SceneObject) -> DebugObjectSnapshot {
    DebugObjectSnapshot {
        id: object.id,
        world_x: object.world_x,
        world_y: object.world_y,
        state: object.state,
        layer: object.layer,
        trigger_mode: object.trigger_mode,
        trigger_script: object.trigger_script,
        auto_script: object.auto_script,
        sprite_index: object.sprite_index,
        frames_per_direction: object.frames_per_direction,
        sprite_frame_count: object.sprite_frame_count,
        direction: object.direction as u16,
        current_frame: object.current_frame,
        vanish_time: object.vanish_time,
        visible: object.is_visible(),
        blocker: object.is_blocker(),
        can_search: object.can_search(),
        can_touch: object.can_touch(),
    }
}

pub(super) fn render_object_overlay(
    renderer: &mut Renderer,
    objects: &[SceneObject],
    viewport: Viewport,
    focused_object_id: Option<u16>,
    label_all_objects: bool,
) {
    for object in objects {
        let x = object.world_x - viewport.x;
        let y = object.world_y - viewport.y;
        if x < -32 || y < -16 || x >= viewport.width as i32 + 32 || y >= viewport.height as i32 + 16
        {
            continue;
        }
        let focused = focused_object_id == Some(object.id);
        let color = object_debug_color(object, focused);
        let radius = if focused { 5 } else { 3 };
        draw_line(renderer, x - radius, y, x + radius, y, color);
        draw_line(renderer, x, y - radius, x, y + radius, color);
        if label_all_objects || focused {
            draw_debug_text(
                renderer,
                x + 4,
                y - 9,
                &format!("#{:04X}", object.id),
                color,
            );
        }
    }
}

pub(super) fn object_debug_color(object: &SceneObject, focused: bool) -> [u8; 4] {
    if focused {
        return OBJECT_FOCUS_COLOR;
    }
    if object.state <= 0 || object.vanish_time > 0 {
        return OBJECT_HIDDEN_COLOR;
    }
    match (object.trigger_script != 0, object.auto_script != 0) {
        (true, true) => OBJECT_BOTH_COLOR,
        (true, false) => OBJECT_TRIGGER_COLOR,
        (false, true) => OBJECT_AUTO_COLOR,
        (false, false) => OBJECT_INERT_COLOR,
    }
}
