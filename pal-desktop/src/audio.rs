//! Desktop audio output for decoded PAL sound effects and SoundFont MIDI music.

use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use pal_assets::mkf::MkfArchive;
use pal_assets::voc::VocClip;
use rodio::buffer::SamplesBuffer;
use rodio::{OutputStream, OutputStreamHandle, Sink, Source};
use rustysynth::{MidiFile, MidiFileSequencer, SoundFont, Synthesizer, SynthesizerSettings};

const MUSIC_SAMPLE_RATE: u32 = 44_100;
const MAX_MUSIC_SECONDS: f64 = 15.0 * 60.0;
const RELEASE_TAIL_SECONDS: f64 = 2.0;

pub struct SoundEffects {
    archive: MkfArchive,
    output: Option<(OutputStream, OutputStreamHandle)>,
}

pub struct BackgroundMusic {
    archive: MkfArchive,
    sound_font: Arc<SoundFont>,
    output: Option<(OutputStream, OutputStreamHandle)>,
    sink: Option<Sink>,
    current: Option<u16>,
}

impl BackgroundMusic {
    pub fn new(midi_mkf: &[u8], sound_font: &[u8]) -> Option<Self> {
        Some(Self {
            archive: MkfArchive::new(midi_mkf)?,
            sound_font: parse_sound_font(sound_font)?,
            output: OutputStream::try_default().ok(),
            sink: None,
            current: None,
        })
    }

    /// Decode and play one `MIDI.MKF` song with the configured General MIDI SoundFont.
    pub fn play(&mut self, music_id: u16, looped: bool, fade_seconds: u8) -> bool {
        if music_id == 0 {
            self.stop();
            return true;
        }
        if self.current == Some(music_id) && self.sink.as_ref().is_some_and(|sink| !sink.empty()) {
            return true;
        }
        let Some(chunk) = self.archive.read_chunk(usize::from(music_id)) else {
            return false;
        };
        let Some(samples) = synthesize(chunk, &self.sound_font, looped) else {
            return false;
        };
        self.stop();
        let Some((_, handle)) = &self.output else {
            self.current = Some(music_id);
            return true;
        };
        let Ok(sink) = Sink::try_new(handle) else {
            return false;
        };
        sink.set_volume(0.7);
        let fade = Duration::from_secs(u64::from(fade_seconds));
        let source = SamplesBuffer::new(2, MUSIC_SAMPLE_RATE, samples);
        if looped {
            sink.append(source.repeat_infinite().fade_in(fade));
        } else {
            sink.append(source.fade_in(fade));
        }
        self.sink = Some(sink);
        self.current = Some(music_id);
        true
    }

    pub fn stop(&mut self) {
        if let Some(sink) = self.sink.take() {
            sink.stop();
        }
        self.current = None;
    }
}

pub fn validate_sound_font(data: &[u8]) -> bool {
    parse_sound_font(data).is_some()
}

pub fn validate_midi_output(midi: &[u8], sound_font: &[u8]) -> bool {
    let Some(sound_font) = parse_sound_font(sound_font) else {
        return false;
    };
    let mut reader = Cursor::new(midi);
    let Ok(midi_file) = MidiFile::new(&mut reader) else {
        return false;
    };
    let settings = SynthesizerSettings::new(MUSIC_SAMPLE_RATE as i32);
    let Ok(synthesizer) = Synthesizer::new(&sound_font, &settings) else {
        return false;
    };
    let mut sequencer = MidiFileSequencer::new(synthesizer);
    sequencer.play(&Arc::new(midi_file), false);
    let mut left = vec![0.0f32; MUSIC_SAMPLE_RATE as usize];
    let mut right = vec![0.0f32; MUSIC_SAMPLE_RATE as usize];
    sequencer.render(&mut left, &mut right);
    left.iter()
        .chain(&right)
        .any(|sample| sample.abs() > 0.0001)
}

fn parse_sound_font(data: &[u8]) -> Option<Arc<SoundFont>> {
    let mut reader = Cursor::new(data);
    Some(Arc::new(SoundFont::new(&mut reader).ok()?))
}

fn synthesize(midi: &[u8], sound_font: &Arc<SoundFont>, looped: bool) -> Option<Vec<i16>> {
    let mut reader = Cursor::new(midi);
    let midi_file = Arc::new(MidiFile::new(&mut reader).ok()?);
    let length = midi_file.get_length();
    if !length.is_finite() || length <= 0.0 || length > MAX_MUSIC_SECONDS {
        return None;
    }

    let settings = SynthesizerSettings::new(MUSIC_SAMPLE_RATE as i32);
    let synthesizer = Synthesizer::new(sound_font, &settings).ok()?;
    let mut sequencer = MidiFileSequencer::new(synthesizer);
    sequencer.play(&midi_file, looped);

    let tail = if looped { 0.0 } else { RELEASE_TAIL_SECONDS };
    let sample_count = ((length + tail) * f64::from(MUSIC_SAMPLE_RATE)) as usize;
    let mut left = vec![0.0f32; sample_count];
    let mut right = vec![0.0f32; sample_count];
    sequencer.render(&mut left, &mut right);

    Some(interleave_pcm(&left, &right))
}

fn interleave_pcm(left: &[f32], right: &[f32]) -> Vec<i16> {
    left.iter()
        .zip(right)
        .flat_map(|(&left, &right)| [float_to_pcm(left), float_to_pcm(right)])
        .collect()
}

fn float_to_pcm(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16
}

impl SoundEffects {
    pub fn new(voc_mkf: &[u8]) -> Option<Self> {
        Some(Self {
            archive: MkfArchive::new(voc_mkf)?,
            output: OutputStream::try_default().ok(),
        })
    }

    /// Decode and play one zero-based `VOC.MKF` chunk.
    ///
    /// A missing output device is treated as a successful silent fallback so
    /// gameplay remains usable on headless systems.
    pub fn play(&self, sound_id: u16) -> bool {
        let Some(chunk) = self.archive.read_chunk(usize::from(sound_id)) else {
            return false;
        };
        if chunk.is_empty() {
            return true;
        }
        let Some(clip) = VocClip::parse(chunk) else {
            return false;
        };
        let Some((_, handle)) = &self.output else {
            return true;
        };
        let Ok(sink) = Sink::try_new(handle) else {
            return false;
        };
        let samples = clip
            .samples
            .into_iter()
            .map(|sample| (i16::from(sample) - 128) << 8)
            .collect::<Vec<_>>();
        sink.append(SamplesBuffer::new(1, clip.sample_rate, samples));
        sink.detach();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_archives_and_sound_fonts_without_opening_audio() {
        assert!(SoundEffects::new(&[]).is_none());
        assert!(BackgroundMusic::new(&[], &[]).is_none());
        assert!(!validate_sound_font(b"not a soundfont"));
    }

    #[test]
    fn interleaves_and_clamps_stereo_pcm() {
        assert_eq!(
            interleave_pcm(&[0.0, 1.5], &[-1.5, 0.5]),
            vec![0, -32767, 32767, 16383]
        );
    }
}
