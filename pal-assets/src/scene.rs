//! Scene and event-object records stored in `SSS.MKF`.

use crate::mkf::MkfArchive;

const EVENT_OBJECT_RECORD_SIZE: usize = 32;
const SCENE_RECORD_SIZE: usize = 8;

/// Static definition of one scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scene {
    pub map_num: u16,
    pub script_on_enter: u16,
    pub script_on_teleport: u16,
    /// Zero-based boundary into the global event-object table.
    pub event_object_index: u16,
}

/// Mutable initial state of one event object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventObject {
    pub vanish_time: i16,
    pub x: u16,
    pub y: u16,
    pub layer: i16,
    pub trigger_script: u16,
    pub auto_script: u16,
    pub state: i16,
    pub trigger_mode: u16,
    /// `MGO.MKF` chunk index; zero means that the object has no sprite.
    pub sprite_num: u16,
    pub sprite_frames: u16,
    pub direction: u16,
    pub current_frame: u16,
    pub script_idle_frame: u16,
    pub sprite_ptr_offset: u16,
    pub auto_sprite_frames: u16,
    pub auto_script_idle_frame: u16,
}

/// Parsed scene tables from the first two chunks of `SSS.MKF`.
#[derive(Debug)]
pub struct SceneData {
    scenes: Vec<Scene>,
    event_objects: Vec<EventObject>,
}

/// One-based scene definition and its slice of the global event-object table.
#[derive(Debug, Clone, Copy)]
pub struct SceneView<'a> {
    pub number: usize,
    pub scene: &'a Scene,
    pub event_objects: &'a [EventObject],
}

impl SceneData {
    /// Parse and validate scene data from a complete `SSS.MKF` archive.
    pub fn parse(data: &[u8]) -> Option<Self> {
        let archive = MkfArchive::new(data)?;
        let event_chunk = archive.read_chunk(0)?;
        let scene_chunk = archive.read_chunk(1)?;
        if event_chunk.len() % EVENT_OBJECT_RECORD_SIZE != 0
            || scene_chunk.len() < SCENE_RECORD_SIZE * 2
            || scene_chunk.len() % SCENE_RECORD_SIZE != 0
        {
            return None;
        }

        let event_objects = event_chunk
            .chunks_exact(EVENT_OBJECT_RECORD_SIZE)
            .map(parse_event_object)
            .collect::<Option<Vec<_>>>()?;
        let scenes = scene_chunk
            .chunks_exact(SCENE_RECORD_SIZE)
            .map(parse_scene)
            .collect::<Option<Vec<_>>>()?;

        if scenes.windows(2).any(|pair| {
            pair[0].event_object_index > pair[1].event_object_index
                || pair[1].event_object_index as usize > event_objects.len()
        }) {
            return None;
        }

        Some(Self {
            scenes,
            event_objects,
        })
    }

    /// Number of addressable scenes. The final record supplies the last boundary.
    pub fn scene_count(&self) -> usize {
        self.scenes.len() - 1
    }

    /// Select a one-based scene and the event objects belonging to it.
    pub fn scene(&self, number: usize) -> Option<SceneView<'_>> {
        let index = number.checked_sub(1)?;
        let scene = self.scenes.get(index)?;
        let next = self.scenes.get(index + 1)?;
        let start = scene.event_object_index as usize;
        let end = next.event_object_index as usize;
        Some(SceneView {
            number,
            scene,
            event_objects: self.event_objects.get(start..end)?,
        })
    }

    pub fn event_object_count(&self) -> usize {
        self.event_objects.len()
    }

    /// Complete global event-object table in one-based ID order.
    pub fn event_objects(&self) -> &[EventObject] {
        &self.event_objects
    }
}

fn parse_scene(record: &[u8]) -> Option<Scene> {
    Some(Scene {
        map_num: read_u16(record, 0)?,
        script_on_enter: read_u16(record, 2)?,
        script_on_teleport: read_u16(record, 4)?,
        event_object_index: read_u16(record, 6)?,
    })
}

fn parse_event_object(record: &[u8]) -> Option<EventObject> {
    Some(EventObject {
        vanish_time: read_i16(record, 0)?,
        x: read_u16(record, 2)?,
        y: read_u16(record, 4)?,
        layer: read_i16(record, 6)?,
        trigger_script: read_u16(record, 8)?,
        auto_script: read_u16(record, 10)?,
        state: read_i16(record, 12)?,
        trigger_mode: read_u16(record, 14)?,
        sprite_num: read_u16(record, 16)?,
        sprite_frames: read_u16(record, 18)?,
        direction: read_u16(record, 20)?,
        current_frame: read_u16(record, 22)?,
        script_idle_frame: read_u16(record, 24)?,
        sprite_ptr_offset: read_u16(record, 26)?,
        auto_sprite_frames: read_u16(record, 28)?,
        auto_script_idle_frame: read_u16(record, 30)?,
    })
}

fn read_u16(data: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        data.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn read_i16(data: &[u8], offset: usize) -> Option<i16> {
    Some(i16::from_le_bytes(
        data.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_mkf(chunks: &[&[u8]]) -> Vec<u8> {
        let table_size = (chunks.len() + 1) * 4;
        let mut offset = table_size as u32;
        let mut data = Vec::new();
        data.extend_from_slice(&offset.to_le_bytes());
        for chunk in chunks {
            offset += chunk.len() as u32;
            data.extend_from_slice(&offset.to_le_bytes());
        }
        for chunk in chunks {
            data.extend_from_slice(chunk);
        }
        data
    }

    fn scene_record(map: u16, enter: u16, teleport: u16, boundary: u16) -> [u8; 8] {
        let mut record = [0; 8];
        for (offset, value) in [(0, map), (2, enter), (4, teleport), (6, boundary)] {
            record[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        record
    }

    #[test]
    fn parses_scenes_and_selects_their_event_objects() {
        let mut events = [0; EVENT_OBJECT_RECORD_SIZE * 2];
        events[2..4].copy_from_slice(&320u16.to_le_bytes());
        events[4..6].copy_from_slice(&200u16.to_le_bytes());
        events[12..14].copy_from_slice(&2i16.to_le_bytes());
        events[16..18].copy_from_slice(&7u16.to_le_bytes());

        let mut scenes = Vec::new();
        scenes.extend_from_slice(&scene_record(12, 99, 100, 0));
        scenes.extend_from_slice(&scene_record(8, 0, 0, 2));
        scenes.extend_from_slice(&scene_record(0, 0, 0, 2));
        let archive = make_mkf(&[&events, &scenes]);

        let data = SceneData::parse(&archive).unwrap();
        assert_eq!(data.scene_count(), 2);
        assert_eq!(data.event_object_count(), 2);
        let scene = data.scene(1).unwrap();
        assert_eq!(scene.scene.map_num, 12);
        assert_eq!(scene.scene.script_on_enter, 99);
        assert_eq!(scene.event_objects.len(), 2);
        assert_eq!(scene.event_objects[0].x, 320);
        assert_eq!(scene.event_objects[0].y, 200);
        assert_eq!(scene.event_objects[0].state, 2);
        assert_eq!(scene.event_objects[0].sprite_num, 7);
        assert!(data.scene(0).is_none());
        assert!(data.scene(3).is_none());
    }

    #[test]
    fn rejects_truncated_records_and_invalid_boundaries() {
        let events = [0; EVENT_OBJECT_RECORD_SIZE];
        let valid_scenes = [scene_record(1, 0, 0, 0), scene_record(0, 0, 0, 1)].concat();
        assert!(SceneData::parse(&make_mkf(&[&events[..31], &valid_scenes])).is_none());
        assert!(SceneData::parse(&make_mkf(&[&events, &valid_scenes[..15]])).is_none());

        let past_end = [scene_record(1, 0, 0, 0), scene_record(0, 0, 0, 2)].concat();
        assert!(SceneData::parse(&make_mkf(&[&events, &past_end])).is_none());

        let backwards = [
            scene_record(1, 0, 0, 1),
            scene_record(2, 0, 0, 0),
            scene_record(0, 0, 0, 1),
        ]
        .concat();
        assert!(SceneData::parse(&make_mkf(&[&events, &backwards])).is_none());
    }

    #[test]
    fn rejects_empty_and_missing_chunks() {
        assert!(SceneData::parse(&make_mkf(&[])).is_none());
        assert!(SceneData::parse(&make_mkf(&[&[]])).is_none());
        assert!(SceneData::parse(&make_mkf(&[&[], &[0; 8]])).is_none());
    }
}
