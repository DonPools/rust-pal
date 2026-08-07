//! Softstar RIX music command stream decoding.

const CHANNELS: usize = 11;
const OPERATORS: usize = 18;
const FREQUENCIES: usize = 25 * 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RixRegisterWrite {
    pub register: u16,
    pub value: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RixTrack {
    data: Vec<u8>,
    rhythm: bool,
    instrument_offset: usize,
    music_offset: usize,
}

impl RixTrack {
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 16 || u16::from_le_bytes(data[0..2].try_into().ok()?) != 0x55aa {
            return None;
        }
        let instrument_offset = usize::from(u16::from_le_bytes(data[8..10].try_into().ok()?));
        let music_offset = usize::from(u16::from_le_bytes(data[12..14].try_into().ok()?));
        if instrument_offset >= data.len()
            || music_offset >= data.len()
            || music_offset.checked_add(1)? >= data.len()
        {
            return None;
        }
        Some(Self {
            data: data.to_vec(),
            rhythm: data[2] != 0,
            instrument_offset,
            music_offset,
        })
    }
}

#[derive(Debug, Clone)]
pub struct RixSequencer {
    track: RixTrack,
    frequency: [u16; FREQUENCIES],
    pitch_base: [i16; CHANNELS],
    note: [u8; OPERATORS],
    keyed: [bool; OPERATORS],
    octave: [u8; 96],
    semitone: [u8; 96],
    instrument: [u16; 28],
    displacement: [u16; CHANNELS],
    registers: [[u16; 14]; OPERATORS],
    volume: [u8; OPERATORS],
    percussion: u8,
    sustain: i32,
    position: usize,
    ended: bool,
    writes: Vec<RixRegisterWrite>,
}

impl RixSequencer {
    pub fn new(track: RixTrack) -> Self {
        let mut sequencer = Self {
            track,
            frequency: [0; FREQUENCIES],
            pitch_base: [0; CHANNELS],
            note: [0; OPERATORS],
            keyed: [false; OPERATORS],
            octave: [0; 96],
            semitone: [0; 96],
            instrument: [0; 28],
            displacement: [0; CHANNELS],
            registers: [[0; 14]; OPERATORS],
            volume: [0x7f; OPERATORS],
            percussion: 0,
            sustain: 0,
            position: 0,
            ended: false,
            writes: Vec::new(),
        };
        sequencer.initialize(true);
        sequencer
    }

    pub fn rewind(&mut self, reinitialize: bool) {
        self.position = self.track.music_offset + 1;
        self.sustain = 0;
        self.ended = false;
        self.writes.clear();
        if reinitialize {
            self.initialize(true);
        }
    }

    /// Whether the command stream reached its explicit `0x80` end marker.
    pub fn ended_cleanly(&self) -> bool {
        self.ended
    }

    /// Advance one 70 Hz music tick and return the OPL2 writes for that tick.
    pub fn advance(&mut self) -> Option<Vec<RixRegisterWrite>> {
        if self.ended {
            return None;
        }
        loop {
            if self.sustain > 0 {
                self.sustain -= 14;
                break;
            }
            let delay = self.process_commands()?;
            if delay == 0 {
                self.ended = true;
                return (!self.writes.is_empty()).then(|| std::mem::take(&mut self.writes));
            }
            self.sustain += i32::from(delay);
        }
        Some(std::mem::take(&mut self.writes))
    }

    fn initialize(&mut self, reset_state: bool) {
        if reset_state {
            self.pitch_base.fill(0);
            self.note.fill(0);
            self.keyed.fill(false);
            self.instrument.fill(0);
            self.displacement.fill(0);
            self.registers.fill([0; 14]);
            self.volume.fill(0x7f);
            self.percussion = 0;
        }
        self.build_frequency_table();
        for note in 0..96 {
            self.octave[note] = (note / 12) as u8;
            self.semitone[note] = (note % 12) as u8;
        }
        self.write(1, 0x20);
        self.write(0xbd, 0);
        self.write(0x08, 0);
        for channel in 0..9 {
            self.write(0xa0 + channel as u16, 0);
            self.write(0xb0 + channel as u16, 0);
        }
        for operator in 0..OPERATORS {
            self.write(0xe0 + operator_register_offset(operator), 0);
        }
        if self.track.rhythm {
            self.write_frequency(6, 0, false);
            self.write_frequency(7, 0, false);
            self.write_frequency(8, 0, false);
            self.note[8] = 0x18;
            self.note[7] = 0x1f;
            self.write(0xa8, 87);
            self.write(0xb8, 9);
            self.write(0xa7, 3);
            self.write(0xb7, 15);
        }
        self.write_percussion();
        self.position = self.track.music_offset + 1;
        self.sustain = 0;
        self.ended = false;
    }

    fn build_frequency_table(&mut self) {
        for octave in 0..25usize {
            let mut value =
                ((octave as u32 * 24 + 10_000) * 52_088 / 250_000) * 0x24_000 / 0x1_b503;
            for semitone in 0..12usize {
                self.frequency[octave * 12 + semitone] = (value as u16).wrapping_add(4) >> 3;
                value = (f64::from(value) * 1.06) as u32;
            }
        }
    }

    fn process_commands(&mut self) -> Option<u16> {
        loop {
            let control = *self.track.data.get(self.position)?;
            if control == 0x80 {
                self.stop_all();
                self.position = self.track.music_offset + 1;
                return Some(0);
            }
            let low = *self.track.data.get(self.position.checked_sub(1)?)?;
            self.position = self.position.checked_add(2)?;
            let channel = usize::from(control & 0x0f);
            match control & 0xf0 {
                0x90 => {
                    if channel >= CHANNELS {
                        return None;
                    }
                    self.load_instrument(low)?;
                    self.apply_instrument(channel)?;
                }
                0xa0 => {
                    if channel >= CHANNELS {
                        return None;
                    }
                    if !self.track.rhythm && channel >= 9 {
                        return None;
                    }
                    if !self.track.rhythm || channel <= 6 {
                        self.prepare_pitch(channel, u16::from(low) << 6)?;
                        self.write_frequency(channel, self.note[channel], self.keyed[channel]);
                    }
                }
                0xb0 => {
                    if channel >= CHANNELS {
                        return None;
                    }
                    self.set_volume(channel, low)?;
                }
                0xc0 => {
                    if channel >= CHANNELS {
                        return None;
                    }
                    if !self.track.rhythm && channel >= 9 {
                        return None;
                    }
                    self.key_off(channel)?;
                    if low != 0 {
                        self.key_on(channel, low)?;
                    }
                }
                _ => return Some((u16::from(control) << 8) | u16::from(low)),
            }
        }
    }

    fn load_instrument(&mut self, instrument: u8) -> Option<()> {
        let start = self
            .track
            .instrument_offset
            .checked_add(usize::from(instrument).checked_mul(64)?)?;
        let bytes = self.track.data.get(start..start.checked_add(56)?)?;
        for (index, word) in bytes.chunks_exact(2).enumerate() {
            self.instrument[index] = u16::from_le_bytes(word.try_into().ok()?);
        }
        Some(())
    }

    fn apply_instrument(&mut self, channel: usize) -> Option<()> {
        if !self.track.rhythm || channel < 6 {
            self.install_operator(melodic_operator(channel, false)?, 0, self.instrument[26]);
            self.install_operator(melodic_operator(channel, true)?, 13, self.instrument[27]);
        } else if channel == 6 {
            self.install_operator(12, 0, self.instrument[26]);
            self.install_operator(15, 13, self.instrument[27]);
        } else {
            self.install_operator(percussion_operator(channel)?, 0, self.instrument[26]);
        }
        Some(())
    }

    fn install_operator(&mut self, operator: usize, source: usize, wave: u16) {
        for field in 0..13 {
            self.registers[operator][field] = self.instrument[source + field];
        }
        self.registers[operator][13] = wave & 3;
        self.write_percussion();
        self.write(0x08, 0);
        self.write_operator_volume(operator);
        self.write_operator_connection(operator);
        let data60 =
            ((self.registers[operator][3] & 0x0f) << 4) | (self.registers[operator][6] & 0x0f);
        self.write(0x60 + operator_register_offset(operator), data60 as u8);
        let data80 =
            ((self.registers[operator][4] & 0x0f) << 4) | (self.registers[operator][7] & 0x0f);
        self.write(0x80 + operator_register_offset(operator), data80 as u8);
        let data20 = (u16::from(self.registers[operator][9] != 0) << 7)
            | (u16::from(self.registers[operator][10] != 0) << 6)
            | (u16::from(self.registers[operator][5] != 0) << 5)
            | (u16::from(self.registers[operator][11] != 0) << 4)
            | (self.registers[operator][1] & 0x0f);
        self.write(0x20 + operator_register_offset(operator), data20 as u8);
        self.write(
            0xe0 + operator_register_offset(operator),
            (self.registers[operator][13] & 3) as u8,
        );
    }

    fn write_operator_connection(&mut self, operator: usize) {
        if is_carrier(operator) {
            return;
        }
        let data = self.registers[operator][2].wrapping_mul(2)
            | u16::from(self.registers[operator][12] == 0);
        self.write(0xc0 + channel_register_offset(operator), data as u8);
    }

    fn set_volume(&mut self, channel: usize, value: u8) -> Option<()> {
        let operator = if !self.track.rhythm || channel < 6 {
            melodic_operator(channel, true)?
        } else {
            percussion_operator(channel)?
        };
        self.volume[operator] = value.min(0x7f);
        self.write_operator_volume(operator);
        Some(())
    }

    fn write_operator_volume(&mut self, operator: usize) {
        let source = 0x3f_u16.wrapping_sub(self.registers[operator][8] & 0x3f);
        let scaled = source
            .wrapping_mul(u16::from(self.volume[operator]))
            .wrapping_mul(2)
            .wrapping_add(0x7f)
            / 0xfe;
        let level = 0u16.wrapping_sub(scaled.wrapping_sub(0x3f));
        let value = level | self.registers[operator][0].wrapping_shl(6);
        self.write(0x40 + operator_register_offset(operator), value as u8);
    }

    fn prepare_pitch(&mut self, channel: usize, value: u16) -> Option<()> {
        let base = (i32::from(value) - 0x2000) * 0x19 / 0x2000;
        if base < 0 {
            let adjusted = 0x18 - base;
            self.pitch_base[channel] = (adjusted / -0x19) as i16;
            let shifted = adjusted - 0x18;
            let remainder = shifted % 0x19;
            let displacement = if remainder == 0 {
                shifted / 0x19
            } else {
                0x19 - remainder
            };
            self.displacement[channel] = (displacement * 0x18) as u16;
        } else {
            self.pitch_base[channel] = (base / 0x19) as i16;
            self.displacement[channel] = ((base % 0x19) * 0x18) as u16;
        }
        Some(())
    }

    fn key_on(&mut self, channel: usize, value: u8) -> Option<()> {
        let note = value.saturating_sub(12);
        if !self.track.rhythm || channel < 6 {
            self.write_frequency(channel, note, true);
        } else if channel == 6 {
            self.write_frequency(channel, note, false);
            self.percussion |= percussion_bit(channel)?;
            self.write_percussion();
        } else if channel == 8 {
            self.write_frequency(channel, note, false);
            self.write_frequency(7, note.saturating_add(7), false);
            self.percussion |= percussion_bit(channel)?;
            self.write_percussion();
        } else {
            self.percussion |= percussion_bit(channel)?;
            self.write_percussion();
        }
        Some(())
    }

    fn key_off(&mut self, channel: usize) -> Option<()> {
        if !self.track.rhythm || channel < 6 {
            self.write_frequency(channel, self.note[channel], false);
        } else {
            self.percussion &= !percussion_bit(channel)?;
            self.write_percussion();
        }
        Some(())
    }

    fn write_frequency(&mut self, channel: usize, note: u8, keyed: bool) {
        if channel >= CHANNELS {
            return;
        }
        self.note[channel] = note;
        self.keyed[channel] = keyed;
        let index = (i16::from(note) + self.pitch_base[channel]).clamp(0, 95) as usize;
        let frequency_index = usize::from(self.semitone[index])
            .saturating_add(usize::from(self.displacement[channel] / 2))
            .min(FREQUENCIES - 1);
        let frequency = self.frequency[frequency_index];
        self.write(0xa0 + channel as u16, frequency as u8);
        let high = u16::from(self.octave[index]).wrapping_mul(4)
            | u16::from(keyed).wrapping_mul(0x20)
            | ((frequency >> 8) & 3);
        self.write(0xb0 + channel as u16, high as u8);
    }

    fn write_percussion(&mut self) {
        self.write(
            0xbd,
            (if self.track.rhythm { 0x20 } else { 0 }) | self.percussion,
        );
    }

    fn stop_all(&mut self) {
        for channel in 0..CHANNELS {
            let _ = self.key_off(channel);
        }
    }

    fn write(&mut self, register: u16, value: u8) {
        self.writes.push(RixRegisterWrite { register, value });
    }
}

fn melodic_operator(channel: usize, carrier: bool) -> Option<usize> {
    const MODULATORS: [usize; 9] = [0, 1, 2, 6, 7, 8, 12, 13, 14];
    const CARRIERS: [usize; 9] = [3, 4, 5, 9, 10, 11, 15, 16, 17];
    (if carrier { CARRIERS } else { MODULATORS })
        .get(channel)
        .copied()
}

fn percussion_operator(channel: usize) -> Option<usize> {
    match channel {
        6 => Some(15),
        7 => Some(16),
        8 => Some(14),
        9 => Some(17),
        10 => Some(13),
        _ => None,
    }
}

fn percussion_bit(channel: usize) -> Option<u8> {
    match channel {
        6 => Some(0x10),
        7 => Some(0x08),
        8 => Some(0x04),
        9 => Some(0x02),
        10 => Some(0x01),
        _ => None,
    }
}

fn operator_register_offset(operator: usize) -> u16 {
    const OFFSETS: [u16; OPERATORS] = [
        0, 1, 2, 3, 4, 5, 8, 9, 10, 11, 12, 13, 16, 17, 18, 19, 20, 21,
    ];
    OFFSETS[operator]
}

fn channel_register_offset(operator: usize) -> u16 {
    const OFFSETS: [u16; OPERATORS] = [0, 1, 2, 0, 1, 2, 3, 4, 5, 3, 4, 5, 6, 7, 8, 6, 7, 8];
    OFFSETS[operator]
}

fn is_carrier(operator: usize) -> bool {
    matches!(operator, 3..=5 | 9..=11 | 15..=17)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_truncated_and_invalid_rix_tracks() {
        assert!(RixTrack::parse(&[]).is_none());
        assert!(RixTrack::parse(&[0; 16]).is_none());
        let mut data = vec![0; 16];
        data[0..2].copy_from_slice(&0x55aau16.to_le_bytes());
        data[8..10].copy_from_slice(&8u16.to_le_bytes());
        data[12..14].copy_from_slice(&15u16.to_le_bytes());
        assert!(RixTrack::parse(&data).is_none());

        let mut data = vec![0; 18];
        data[0..2].copy_from_slice(&0x55aau16.to_le_bytes());
        data[8..10].copy_from_slice(&16u16.to_le_bytes());
        data[12..14].copy_from_slice(&16u16.to_le_bytes());
        data[16] = 14;
        let mut sequence = RixSequencer::new(RixTrack::parse(&data).unwrap());
        assert!(sequence.advance().is_some());
        assert!(sequence.advance().is_none());
        assert!(!sequence.ended_cleanly());

        let mut data = vec![0; 20];
        data[0..2].copy_from_slice(&0x55aau16.to_le_bytes());
        data[8..10].copy_from_slice(&16u16.to_le_bytes());
        data[12..14].copy_from_slice(&16u16.to_le_bytes());
        data[16] = 0;
        data[17] = 0xaf;
        let mut sequence = RixSequencer::new(RixTrack::parse(&data).unwrap());
        assert!(sequence.advance().is_none());
        assert!(!sequence.ended_cleanly());
    }

    #[test]
    fn minimal_track_emits_delay_then_stops_channels() {
        let mut data = vec![0; 80];
        data[0..2].copy_from_slice(&0x55aau16.to_le_bytes());
        data[8..10].copy_from_slice(&16u16.to_le_bytes());
        data[12..14].copy_from_slice(&72u16.to_le_bytes());
        data[72] = 14;
        data[73] = 0;
        data[74] = 0;
        data[75] = 0x80;
        let track = RixTrack::parse(&data).unwrap();
        let mut sequence = RixSequencer::new(track);
        assert!(sequence.advance().is_some());
        assert!(sequence.advance().is_some());
        assert!(sequence.advance().is_none());
    }

    #[test]
    fn melodic_channels_map_to_all_nine_opl2_operator_pairs() {
        assert_eq!(melodic_operator(0, false), Some(0));
        assert_eq!(melodic_operator(0, true), Some(3));
        assert_eq!(melodic_operator(5, false), Some(8));
        assert_eq!(melodic_operator(5, true), Some(11));
        assert_eq!(melodic_operator(8, false), Some(14));
        assert_eq!(melodic_operator(8, true), Some(17));
        assert_eq!(melodic_operator(9, false), None);
    }
}
