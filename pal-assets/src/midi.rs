//! Standard MIDI files stored in `MIDI.MKF` chunks.

const DEFAULT_TEMPO_MICROS: u32 = 500_000;
const MAX_EVENTS: usize = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiEventKind {
    NoteOn { channel: u8, key: u8, velocity: u8 },
    NoteOff { channel: u8, key: u8 },
    ProgramChange { channel: u8, program: u8 },
    ChannelVolume { channel: u8, volume: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiEvent {
    pub time_micros: u64,
    pub kind: MidiEventKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MidiSong {
    pub events: Vec<MidiEvent>,
    pub duration_micros: u64,
}

#[derive(Debug, Clone, Copy)]
enum RawKind {
    Tempo(u32),
    Event(MidiEventKind),
    End,
}

#[derive(Debug, Clone, Copy)]
struct RawEvent {
    tick: u64,
    order: usize,
    kind: RawKind,
}

impl MidiSong {
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.get(..4)? != b"MThd" {
            return None;
        }
        let header_len = usize::try_from(read_u32_be(data, 4)?).ok()?;
        if header_len < 6 {
            return None;
        }
        let header_end = 8usize.checked_add(header_len)?;
        let format = read_u16_be(data, 8)?;
        let track_count = usize::from(read_u16_be(data, 10)?);
        let division = read_u16_be(data, 12)?;
        if format > 1 || track_count == 0 || division == 0 || division & 0x8000 != 0 {
            return None;
        }

        let mut offset = header_end;
        let mut raw = Vec::new();
        let mut order = 0;
        for _ in 0..track_count {
            if data.get(offset..offset.checked_add(4)?)? != b"MTrk" {
                return None;
            }
            let length = usize::try_from(read_u32_be(data, offset + 4)?).ok()?;
            let start = offset.checked_add(8)?;
            let end = start.checked_add(length)?;
            parse_track(data.get(start..end)?, &mut raw, &mut order)?;
            offset = end;
        }
        if raw.len() > MAX_EVENTS {
            return None;
        }
        raw.sort_by_key(|event| (event.tick, event.order));

        let mut tempo = DEFAULT_TEMPO_MICROS;
        let mut tick = 0u64;
        let mut micros = 0u64;
        let mut events = Vec::new();
        for event in raw {
            let delta = event.tick.checked_sub(tick)?;
            let elapsed = u128::from(delta)
                .checked_mul(u128::from(tempo))?
                .checked_div(u128::from(division))?;
            micros = micros.checked_add(u64::try_from(elapsed).ok()?)?;
            tick = event.tick;
            match event.kind {
                RawKind::Tempo(value) => tempo = value,
                RawKind::Event(kind) => events.push(MidiEvent {
                    time_micros: micros,
                    kind,
                }),
                RawKind::End => {}
            }
        }
        Some(Self {
            events,
            duration_micros: micros,
        })
    }
}

fn parse_track(data: &[u8], output: &mut Vec<RawEvent>, order: &mut usize) -> Option<()> {
    let mut offset = 0usize;
    let mut tick = 0u64;
    let mut running_status = None;
    while offset < data.len() {
        tick = tick.checked_add(u64::from(read_vlq(data, &mut offset)?))?;
        let first = *data.get(offset)?;
        let status = if first & 0x80 != 0 {
            offset += 1;
            first
        } else {
            running_status?
        };
        if status < 0xf0 {
            running_status = Some(status);
            let channel = status & 0x0f;
            let data_len = match status >> 4 {
                0xc | 0xd => 1,
                0x8..=0xb | 0xe => 2,
                _ => return None,
            };
            let first_data = *data.get(offset)?;
            if first_data & 0x80 != 0 {
                return None;
            }
            let second_data = if data_len == 2 {
                let value = *data.get(offset + 1)?;
                if value & 0x80 != 0 {
                    return None;
                }
                value
            } else {
                0
            };
            offset = offset.checked_add(data_len)?;
            let kind = match status >> 4 {
                0x8 => Some(MidiEventKind::NoteOff {
                    channel,
                    key: first_data,
                }),
                0x9 if second_data == 0 => Some(MidiEventKind::NoteOff {
                    channel,
                    key: first_data,
                }),
                0x9 => Some(MidiEventKind::NoteOn {
                    channel,
                    key: first_data,
                    velocity: second_data,
                }),
                0xb if first_data == 7 => Some(MidiEventKind::ChannelVolume {
                    channel,
                    volume: second_data,
                }),
                0xc => Some(MidiEventKind::ProgramChange {
                    channel,
                    program: first_data,
                }),
                _ => None,
            };
            if let Some(kind) = kind {
                push_raw(output, order, tick, RawKind::Event(kind))?;
            }
        } else if status == 0xff {
            running_status = None;
            let meta_type = *data.get(offset)?;
            offset += 1;
            let length = usize::try_from(read_vlq(data, &mut offset)?).ok()?;
            let payload = data.get(offset..offset.checked_add(length)?)?;
            offset += length;
            match meta_type {
                0x2f if payload.is_empty() => {
                    push_raw(output, order, tick, RawKind::End)?;
                    return (offset == data.len()).then_some(());
                }
                0x51 if payload.len() == 3 => {
                    let tempo = u32::from_be_bytes([0, payload[0], payload[1], payload[2]]);
                    if tempo == 0 {
                        return None;
                    }
                    push_raw(output, order, tick, RawKind::Tempo(tempo))?;
                }
                _ => {}
            }
        } else if status == 0xf0 || status == 0xf7 {
            running_status = None;
            let length = usize::try_from(read_vlq(data, &mut offset)?).ok()?;
            offset = offset.checked_add(length)?;
            data.get(..offset)?;
        } else {
            return None;
        }
    }
    push_raw(output, order, tick, RawKind::End)
}

fn push_raw(output: &mut Vec<RawEvent>, order: &mut usize, tick: u64, kind: RawKind) -> Option<()> {
    if output.len() >= MAX_EVENTS {
        return None;
    }
    output.push(RawEvent {
        tick,
        order: *order,
        kind,
    });
    *order = order.checked_add(1)?;
    Some(())
}

fn read_vlq(data: &[u8], offset: &mut usize) -> Option<u32> {
    let mut value = 0u32;
    for _ in 0..4 {
        let byte = *data.get(*offset)?;
        *offset = offset.checked_add(1)?;
        value = value.checked_shl(7)?.checked_add(u32::from(byte & 0x7f))?;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

fn read_u16_be(data: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        data.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn read_u32_be(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        data.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn midi(track: &[u8]) -> Vec<u8> {
        let mut data = b"MThd\0\0\0\x06\0\0\0\x01\0\x60MTrk".to_vec();
        data.extend_from_slice(&(track.len() as u32).to_be_bytes());
        data.extend_from_slice(track);
        data
    }

    #[test]
    fn parses_tempo_running_status_and_channel_events() {
        let song = MidiSong::parse(&midi(&[
            0, 0xff, 0x51, 3, 0x07, 0xa1, 0x20, 0, 0xc0, 8, 0, 0xb2, 7, 90, 0, 0x99, 35, 127, 0,
            0x90, 60, 100, 0x60, 64, 80, 0x60, 0x80, 60, 0, 0, 0xff, 0x2f, 0,
        ]))
        .unwrap();
        assert_eq!(song.duration_micros, 1_000_000);
        assert_eq!(song.events.len(), 6);
        assert_eq!(
            song.events[1].kind,
            MidiEventKind::ChannelVolume {
                channel: 2,
                volume: 90
            }
        );
        assert_eq!(
            song.events[2].kind,
            MidiEventKind::NoteOn {
                channel: 9,
                key: 35,
                velocity: 127
            }
        );
        assert_eq!(song.events[4].time_micros, 500_000);
    }

    #[test]
    fn rejects_invalid_headers_tracks_and_events() {
        assert!(MidiSong::parse(&[]).is_none());
        assert!(MidiSong::parse(&midi(&[0, 0x90, 60])).is_none());
        let mut bad_division = midi(&[0, 0xff, 0x2f, 0]);
        bad_division[12..14].copy_from_slice(&0u16.to_be_bytes());
        assert!(MidiSong::parse(&bad_division).is_none());
        assert!(MidiSong::parse(&midi(&[0x81, 0x80, 0x80, 0x80, 0, 0xff, 0x2f, 0])).is_none());
    }
}
