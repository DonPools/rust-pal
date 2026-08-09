//! Canvas hit testing for event-object markers and sprites.

use pal_core::role::RoleSprites;
use pal_core::scene::SceneObject;

use super::super::Viewport;

pub(super) fn hit_test_object_marker(
    objects: &[SceneObject],
    viewport: Viewport,
    zoom: u8,
    cursor: (i32, i32),
    current: Option<u16>,
) -> Option<u16> {
    let zoom = i32::from(zoom.max(1));
    let mut candidates = objects
        .iter()
        .filter_map(|object| {
            let x = (object.world_x - viewport.x) * zoom;
            let y = (object.world_y - viewport.y) * zoom;
            let dx = x - cursor.0;
            let dy = y - cursor.1;
            let distance = i64::from(dx) * i64::from(dx) + i64::from(dy) * i64::from(dy);
            (distance <= 14 * 14).then_some((
                distance,
                object.world_y + i32::from(object.layer) * 8,
                object.id,
            ))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| right.1.cmp(&left.1))
            .then_with(|| left.2.cmp(&right.2))
    });
    cycle_marker_candidates(&candidates, current)
}

pub(super) fn hit_test_visible_sprite(
    objects: &[SceneObject],
    sprites: &RoleSprites,
    viewport: Viewport,
    zoom: u8,
    cursor: (i32, i32),
    current: Option<u16>,
) -> Option<u16> {
    let zoom = i32::from(zoom.max(1));
    let mut candidates = objects
        .iter()
        .filter(|object| object.is_visible())
        .filter_map(|object| {
            let bitmap = sprites.decode_frame(object.sprite_index?, object.frame_index()?)?;
            let left = (object.world_x - i32::from(bitmap.width) / 2 - viewport.x) * zoom;
            let top = (object.world_y + 7 - i32::from(bitmap.height) - viewport.y) * zoom;
            let right = left + i32::from(bitmap.width) * zoom;
            let bottom = top + i32::from(bitmap.height) * zoom;
            (cursor.0 >= left && cursor.0 < right && cursor.1 >= top && cursor.1 < bottom)
                .then_some((object.world_y + i32::from(object.layer) * 8, object.id))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    cycle_candidate_ids(&candidates, current)
}

fn cycle_marker_candidates(candidates: &[(i64, i32, u16)], current: Option<u16>) -> Option<u16> {
    if candidates.is_empty() {
        return None;
    }
    current
        .and_then(|id| candidates.iter().position(|candidate| candidate.2 == id))
        .map_or_else(
            || Some(candidates[0].2),
            |index| Some(candidates[(index + 1) % candidates.len()].2),
        )
}

fn cycle_candidate_ids(candidates: &[(i32, u16)], current: Option<u16>) -> Option<u16> {
    if candidates.is_empty() {
        return None;
    }
    current
        .and_then(|id| candidates.iter().position(|candidate| candidate.1 == id))
        .map_or_else(
            || Some(candidates[0].1),
            |index| Some(candidates[(index + 1) % candidates.len()].1),
        )
}

#[cfg(test)]
mod tests {
    use pal_core::role::Direction;

    use super::*;

    fn object(id: u16, x: i32, y: i32) -> SceneObject {
        SceneObject {
            id,
            world_x: x,
            world_y: y,
            layer: 0,
            trigger_script: 10,
            auto_script: 0,
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
    fn marker_hit_testing_includes_hidden_objects_and_cycles_overlaps() {
        let mut first = object(5, 100, 80);
        first.state = 0;
        let second = object(6, 100, 80);
        let viewport = Viewport::new(50, 40, 240, 225);
        let cursor = ((100 - 50) * 2, (80 - 40) * 2);

        assert_eq!(
            hit_test_object_marker(&[first.clone(), second.clone()], viewport, 2, cursor, None),
            Some(5)
        );
        assert_eq!(
            hit_test_object_marker(&[first, second], viewport, 2, cursor, Some(5)),
            Some(6)
        );
    }

    #[test]
    fn candidate_cycle_handles_empty_and_selected_lists() {
        assert_eq!(cycle_candidate_ids(&[], None), None);
        assert_eq!(cycle_candidate_ids(&[(10, 5), (9, 6)], Some(5)), Some(6));
    }
}
