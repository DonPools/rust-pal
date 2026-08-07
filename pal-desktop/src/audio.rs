//! Desktop audio output for decoded PAL sound effects and selectable RIX or MIDI music.

use std::io::Cursor;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use nuked_opl3::Opl3Chip;
use pal_assets::mkf::MkfArchive;
use pal_assets::rix::{RixSequencer, RixTrack};
use pal_assets::voc::VocClip;
use rodio::buffer::SamplesBuffer;
use rodio::{OutputStream, OutputStreamHandle, Sink, Source};
use rustysynth::{MidiFile, MidiFileSequencer, SoundFont, Synthesizer, SynthesizerSettings};

const MUSIC_SAMPLE_RATE: u32 = 44_100;
const MAX_MUSIC_SECONDS: f64 = 15.0 * 60.0;
const RELEASE_TAIL_SECONDS: f64 = 2.0;
const MUSIC_BUFFER_FRAMES: usize = 2048;
const MUSIC_BUFFER_COUNT: usize = 8;
const MIDI_PREAMP_GAIN: f32 = 0.75;
const MIDI_LIMITER_THRESHOLD: f32 = 0.85;
const MIDI_LIMITER_CEILING: f32 = 0.98;
const RIX_TICKS_PER_SECOND: usize = 70;
const RIX_SAMPLES_PER_TICK: usize = MUSIC_SAMPLE_RATE as usize / RIX_TICKS_PER_SECOND;
const MAX_RIX_TICKS: usize = MAX_MUSIC_SECONDS as usize * RIX_TICKS_PER_SECOND;
pub const AUDIO_VOLUME_MAX: u8 = 100;
const DEFAULT_MUSIC_VOLUME: u8 = 100;
const DEFAULT_SOUND_VOLUME: u8 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MusicBackend {
    Midi,
    Rix,
}

impl std::fmt::Display for MusicBackend {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Midi => "MIDI",
            Self::Rix => "RIX/OPL2",
        })
    }
}

pub struct SoundEffects {
    archive: MkfArchive,
    output: Option<(OutputStream, OutputStreamHandle)>,
    volume: Arc<AtomicU8>,
    enabled: bool,
}

pub struct BackgroundMusic {
    rix_archive: MkfArchive,
    midi_archive: Option<MkfArchive>,
    sound_font: Option<Arc<SoundFont>>,
    output: Option<(OutputStream, OutputStreamHandle)>,
    sink: Option<Sink>,
    pending: Option<Receiver<Option<PreparedMusicSource>>>,
    pending_fade: Option<Duration>,
    loop_control: Option<Arc<AtomicBool>>,
    current: Option<u16>,
    backend: MusicBackend,
    volume: u8,
    enabled: bool,
}

impl BackgroundMusic {
    pub fn new(rix_mkf: &[u8], midi_mkf: &[u8], sound_font: &[u8]) -> Option<Self> {
        Some(Self {
            rix_archive: MkfArchive::new(rix_mkf)?,
            midi_archive: MkfArchive::new(midi_mkf),
            sound_font: parse_sound_font(sound_font),
            output: OutputStream::try_default().ok(),
            sink: None,
            pending: None,
            pending_fade: None,
            loop_control: None,
            current: None,
            backend: MusicBackend::Rix,
            volume: DEFAULT_MUSIC_VOLUME,
            enabled: true,
        })
    }

    /// Play one song with the selected backend without changing its timbre mid-session.
    pub fn play(&mut self, music_id: u16, looped: bool, fade_seconds: u8) -> bool {
        if music_id == 0 {
            self.stop();
            return true;
        }
        if !self.enabled {
            self.current = Some(music_id);
            return true;
        }
        if self.current == Some(music_id)
            && (self.pending.is_some() || self.sink.as_ref().is_some_and(|sink| !sink.empty()))
        {
            if let Some(control) = &self.loop_control {
                control.store(looped, Ordering::Release);
            }
            return true;
        }
        let data = match self.backend {
            MusicBackend::Midi => {
                let Some(midi) = self
                    .midi_archive
                    .as_ref()
                    .and_then(|archive| archive.read_chunk(usize::from(music_id)))
                    .filter(|chunk| !chunk.is_empty())
                else {
                    return false;
                };
                let Some(sound_font) = &self.sound_font else {
                    return false;
                };
                MusicSourceData::Midi {
                    midi: midi.to_vec(),
                    sound_font: Arc::clone(sound_font),
                }
            }
            MusicBackend::Rix => {
                let Some(rix) = self
                    .rix_archive
                    .read_chunk(usize::from(music_id))
                    .filter(|chunk| !chunk.is_empty())
                else {
                    return false;
                };
                MusicSourceData::Rix(rix.to_vec())
            }
        };
        self.stop();
        let Some((_, handle)) = &self.output else {
            self.current = Some(music_id);
            return true;
        };
        let Ok(sink) = Sink::try_new(handle) else {
            return false;
        };
        sink.set_volume(volume_gain(self.volume));
        let fade = Duration::from_secs(u64::from(fade_seconds));
        let (sender, receiver) = mpsc::channel();
        let loop_control = Arc::new(AtomicBool::new(looped));
        let producer_loop_control = Arc::clone(&loop_control);
        thread::spawn(move || {
            let source = match data {
                MusicSourceData::Rix(rix) => {
                    StreamingRixSource::new(&rix, Arc::clone(&producer_loop_control))
                        .map(|source| PreparedMusicSource::Rix(Box::new(source)))
                }
                MusicSourceData::Midi { midi, sound_font } => {
                    StreamingMidiSource::new(&midi, &sound_font, producer_loop_control)
                        .map(PreparedMusicSource::Midi)
                }
            };
            let _ = sender.send(source);
        });
        self.pending = Some(receiver);
        self.pending_fade = Some(fade);
        self.loop_control = Some(loop_control);
        self.sink = Some(sink);
        self.current = Some(music_id);
        true
    }

    /// Attach a prepared MIDI source to the output sink without blocking the game loop.
    pub fn poll(&mut self) -> bool {
        let Some(receiver) = self.pending.take() else {
            return false;
        };
        match receiver.try_recv() {
            Ok(Some(source)) => {
                if let Some(sink) = &self.sink {
                    let fade = self.pending_fade.take().unwrap_or(Duration::ZERO);
                    sink.append(source.fade_in(fade));
                }
                true
            }
            Ok(None) => {
                self.pending_fade = None;
                self.loop_control = None;
                if let Some(sink) = self.sink.take() {
                    sink.stop();
                }
                self.current = None;
                false
            }
            Err(TryRecvError::Empty) => {
                self.pending = Some(receiver);
                false
            }
            Err(TryRecvError::Disconnected) => {
                self.pending_fade = None;
                self.loop_control = None;
                false
            }
        }
    }

    pub fn stop(&mut self) {
        self.pending = None;
        self.pending_fade = None;
        self.loop_control = None;
        if let Some(sink) = self.sink.take() {
            sink.stop();
        }
        self.current = None;
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn backend(&self) -> MusicBackend {
        self.backend
    }

    pub fn backend_available(&self, backend: MusicBackend) -> bool {
        match backend {
            MusicBackend::Midi => self.midi_archive.is_some() && self.sound_font.is_some(),
            MusicBackend::Rix => true,
        }
    }

    pub fn set_backend(&mut self, backend: MusicBackend) -> bool {
        if !self.backend_available(backend) {
            return false;
        }
        if self.backend != backend {
            self.stop();
            self.backend = backend;
        }
        true
    }

    pub fn volume(&self) -> u8 {
        self.volume
    }

    pub fn set_volume(&mut self, volume: u8) {
        self.volume = volume.min(AUDIO_VOLUME_MAX);
        if let Some(sink) = &self.sink {
            sink.set_volume(volume_gain(self.volume));
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        if self.enabled == enabled {
            return;
        }
        self.enabled = enabled;
        if !enabled {
            self.pending = None;
            self.pending_fade = None;
            self.loop_control = None;
            if let Some(sink) = self.sink.take() {
                sink.stop();
            }
        }
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
    let settings = midi_synthesizer_settings();
    let Ok(synthesizer) = Synthesizer::new(&sound_font, &settings) else {
        return false;
    };
    let mut sequencer = MidiFileSequencer::new(synthesizer);
    sequencer.play(&Arc::new(midi_file), false);
    let mut left = vec![0.0f32; MUSIC_SAMPLE_RATE as usize];
    let mut right = vec![0.0f32; MUSIC_SAMPLE_RATE as usize];
    sequencer.render(&mut left, &mut right);
    let mut audible = false;
    for sample in left.iter().chain(&right) {
        let pcm = midi_sample_to_pcm(*sample);
        audible |= pcm != 0;
        if pcm == i16::MIN || pcm == i16::MAX {
            return false;
        }
    }
    audible
}

/// Render a bounded prefix of one RIX track and verify that OPL2 produces sound.
pub fn validate_rix_output(rix: &[u8]) -> bool {
    let loop_control = Arc::new(AtomicBool::new(false));
    let Some(mut source) = StreamingRixSource::new(rix, loop_control) else {
        return false;
    };
    source
        .by_ref()
        .take(MUSIC_SAMPLE_RATE as usize * 2)
        .any(|sample| sample != 0)
}

fn parse_sound_font(data: &[u8]) -> Option<Arc<SoundFont>> {
    let mut reader = Cursor::new(data);
    Some(Arc::new(SoundFont::new(&mut reader).ok()?))
}

enum MusicSourceData {
    Rix(Vec<u8>),
    Midi {
        midi: Vec<u8>,
        sound_font: Arc<SoundFont>,
    },
}

enum PreparedMusicSource {
    Rix(Box<StreamingRixSource>),
    Midi(StreamingMidiSource),
}

impl Iterator for PreparedMusicSource {
    type Item = i16;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Rix(source) => source.next(),
            Self::Midi(source) => source.next(),
        }
    }
}

impl Source for PreparedMusicSource {
    fn current_frame_len(&self) -> Option<usize> {
        match self {
            Self::Rix(source) => source.current_frame_len(),
            Self::Midi(source) => source.current_frame_len(),
        }
    }

    fn channels(&self) -> u16 {
        2
    }

    fn sample_rate(&self) -> u32 {
        MUSIC_SAMPLE_RATE
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

struct StreamingRixSource {
    sequencer: RixSequencer,
    chip: Opl3Chip,
    loop_control: Arc<AtomicBool>,
    buffer: Vec<i16>,
    position: usize,
    pass_ticks: usize,
    finished: bool,
}

impl StreamingRixSource {
    fn new(rix: &[u8], loop_control: Arc<AtomicBool>) -> Option<Self> {
        let track = RixTrack::parse(rix)?;
        let chip = Opl3Chip::new(MUSIC_SAMPLE_RATE);
        Some(Self {
            sequencer: RixSequencer::new(track),
            chip,
            loop_control,
            buffer: Vec::new(),
            position: 0,
            pass_ticks: 0,
            finished: false,
        })
    }

    fn refill(&mut self) -> bool {
        if self.finished {
            return false;
        }
        let writes = loop {
            if self.pass_ticks >= MAX_RIX_TICKS {
                self.finished = true;
                return false;
            }
            if let Some(writes) = self.sequencer.advance() {
                self.pass_ticks += 1;
                break writes;
            }
            if !self.sequencer.ended_cleanly()
                || !self.loop_control.load(Ordering::Acquire)
                || self.pass_ticks == 0
            {
                self.finished = true;
                return false;
            }
            self.sequencer.rewind(true);
            self.pass_ticks = 0;
        };
        for write in writes {
            self.chip.write_register(write.register, write.value);
        }
        self.buffer.resize(RIX_SAMPLES_PER_TICK * 2, 0);
        if self.chip.generate_stream(&mut self.buffer).is_err() {
            self.finished = true;
            return false;
        }
        self.position = 0;
        true
    }
}

impl Iterator for StreamingRixSource {
    type Item = i16;

    fn next(&mut self) -> Option<Self::Item> {
        if self.position >= self.buffer.len() && !self.refill() {
            return None;
        }
        let sample = self.buffer[self.position];
        self.position += 1;
        Some(sample)
    }
}

impl Source for StreamingRixSource {
    fn current_frame_len(&self) -> Option<usize> {
        Some(self.buffer.len().saturating_sub(self.position))
    }

    fn channels(&self) -> u16 {
        2
    }

    fn sample_rate(&self) -> u32 {
        MUSIC_SAMPLE_RATE
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

struct StreamingMidiSource {
    chunks: Receiver<Option<Vec<i16>>>,
    buffer: Vec<i16>,
    position: usize,
}

impl StreamingMidiSource {
    fn new(
        midi: &[u8],
        sound_font: &Arc<SoundFont>,
        loop_control: Arc<AtomicBool>,
    ) -> Option<Self> {
        let mut reader = Cursor::new(midi);
        let midi_file = Arc::new(MidiFile::new(&mut reader).ok()?);
        let length = midi_file.get_length();
        if !length.is_finite() || length <= 0.0 || length > MAX_MUSIC_SECONDS {
            return None;
        }

        let settings = midi_synthesizer_settings();
        let synthesizer = Synthesizer::new(sound_font, &settings).ok()?;
        let mut sequencer = MidiFileSequencer::new(synthesizer);
        // Pass boundaries are controlled by `loop_control` so a repeated
        // PLAY_MUSIC command can change looping without restarting the song.
        sequencer.play(&midi_file, false);

        let sequence_frames = (length * f64::from(MUSIC_SAMPLE_RATE)).ceil().max(1.0) as usize;
        let release_frames = (RELEASE_TAIL_SECONDS * f64::from(MUSIC_SAMPLE_RATE)).ceil() as usize;
        let (sender, receiver) = mpsc::sync_channel(MUSIC_BUFFER_COUNT);
        thread::spawn(move || {
            produce_midi_chunks(
                sequencer,
                midi_file,
                sender,
                sequence_frames,
                release_frames,
                loop_control,
            )
        });
        Some(Self {
            chunks: receiver,
            buffer: Vec::new(),
            position: 0,
        })
    }

    fn refill(&mut self) -> bool {
        let Ok(Some(chunk)) = self.chunks.recv() else {
            return false;
        };
        self.buffer = chunk;
        self.position = 0;
        true
    }
}

fn produce_midi_chunks(
    mut sequencer: MidiFileSequencer,
    midi_file: Arc<MidiFile>,
    sender: SyncSender<Option<Vec<i16>>>,
    sequence_frames: usize,
    release_frames: usize,
    loop_control: Arc<AtomicBool>,
) {
    let mut pass_frames = 0usize;
    let mut left = vec![0.0f32; MUSIC_BUFFER_FRAMES];
    let mut right = vec![0.0f32; MUSIC_BUFFER_FRAMES];
    loop {
        let looped = loop_control.load(Ordering::Acquire);
        let frames = match midi_pass_action(pass_frames, sequence_frames, release_frames, looped) {
            MidiPassAction::Restart => {
                sequencer.play(&midi_file, false);
                pass_frames = 0;
                continue;
            }
            MidiPassAction::Finish => {
                let _ = sender.send(None);
                return;
            }
            MidiPassAction::Render(frames) => frames,
        };
        sequencer.render(&mut left[..frames], &mut right[..frames]);
        let chunk = left[..frames]
            .iter()
            .zip(&right[..frames])
            .flat_map(|(&left, &right)| [midi_sample_to_pcm(left), midi_sample_to_pcm(right)])
            .collect();
        if sender.send(Some(chunk)).is_err() {
            return;
        }
        pass_frames += frames;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MidiPassAction {
    Render(usize),
    Restart,
    Finish,
}

fn midi_pass_action(
    pass_frames: usize,
    sequence_frames: usize,
    release_frames: usize,
    looped: bool,
) -> MidiPassAction {
    if pass_frames >= sequence_frames && looped {
        return MidiPassAction::Restart;
    }
    let boundary = sequence_frames.saturating_add(if looped { 0 } else { release_frames });
    if pass_frames >= boundary {
        return MidiPassAction::Finish;
    }
    MidiPassAction::Render(
        boundary
            .saturating_sub(pass_frames)
            .min(MUSIC_BUFFER_FRAMES),
    )
}

impl Iterator for StreamingMidiSource {
    type Item = i16;

    fn next(&mut self) -> Option<Self::Item> {
        if self.position >= self.buffer.len() && !self.refill() {
            return None;
        }
        let sample = self.buffer[self.position];
        self.position += 1;
        Some(sample)
    }
}

impl Source for StreamingMidiSource {
    fn current_frame_len(&self) -> Option<usize> {
        Some(self.buffer.len().saturating_sub(self.position))
    }

    fn channels(&self) -> u16 {
        2
    }

    fn sample_rate(&self) -> u32 {
        MUSIC_SAMPLE_RATE
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

#[cfg(test)]
fn interleave_pcm(left: &[f32], right: &[f32]) -> Vec<i16> {
    left.iter()
        .zip(right)
        .flat_map(|(&left, &right)| [midi_sample_to_pcm(left), midi_sample_to_pcm(right)])
        .collect()
}

fn midi_synthesizer_settings() -> SynthesizerSettings {
    let mut settings = SynthesizerSettings::new(MUSIC_SAMPLE_RATE as i32);
    settings.enable_reverb_and_chorus = false;
    settings
}

fn midi_sample_to_pcm(sample: f32) -> i16 {
    if !sample.is_finite() {
        return 0;
    }
    let sample = sample * MIDI_PREAMP_GAIN;
    let magnitude = sample.abs();
    let limited = if magnitude <= MIDI_LIMITER_THRESHOLD {
        sample
    } else {
        let knee = MIDI_LIMITER_CEILING - MIDI_LIMITER_THRESHOLD;
        sample.signum()
            * (MIDI_LIMITER_THRESHOLD
                + knee * (1.0 - (-(magnitude - MIDI_LIMITER_THRESHOLD) / knee).exp()))
    };
    (limited * f32::from(i16::MAX)) as i16
}

fn volume_gain(volume: u8) -> f32 {
    f32::from(volume.min(AUDIO_VOLUME_MAX)) / f32::from(AUDIO_VOLUME_MAX)
}

struct DynamicVolume<S> {
    source: S,
    volume: Arc<AtomicU8>,
}

impl<S> DynamicVolume<S> {
    fn new(source: S, volume: Arc<AtomicU8>) -> Self {
        Self { source, volume }
    }
}

impl<S> Iterator for DynamicVolume<S>
where
    S: Source<Item = i16>,
{
    type Item = i16;

    fn next(&mut self) -> Option<Self::Item> {
        let sample = i32::from(self.source.next()?);
        let volume = i32::from(self.volume.load(Ordering::Acquire).min(AUDIO_VOLUME_MAX));
        Some((sample * volume / i32::from(AUDIO_VOLUME_MAX)) as i16)
    }
}

impl<S> Source for DynamicVolume<S>
where
    S: Source<Item = i16>,
{
    fn current_frame_len(&self) -> Option<usize> {
        self.source.current_frame_len()
    }

    fn channels(&self) -> u16 {
        self.source.channels()
    }

    fn sample_rate(&self) -> u32 {
        self.source.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.source.total_duration()
    }
}

impl SoundEffects {
    pub fn new(voc_mkf: &[u8]) -> Option<Self> {
        Some(Self {
            archive: MkfArchive::new(voc_mkf)?,
            output: OutputStream::try_default().ok(),
            volume: Arc::new(AtomicU8::new(DEFAULT_SOUND_VOLUME)),
            enabled: true,
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
        if !self.enabled {
            return true;
        }
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
        sink.append(DynamicVolume::new(
            SamplesBuffer::new(1, clip.sample_rate, samples),
            Arc::clone(&self.volume),
        ));
        sink.detach();
        true
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn volume(&self) -> u8 {
        self.volume.load(Ordering::Acquire)
    }

    pub fn set_volume(&self, volume: u8) {
        self.volume
            .store(volume.min(AUDIO_VOLUME_MAX), Ordering::Release);
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_mkf(chunks: &[&[u8]]) -> Vec<u8> {
        let table_size = (chunks.len() + 1) * 4;
        let mut offset = table_size as u32;
        let mut data = offset.to_le_bytes().to_vec();
        for chunk in chunks {
            offset += chunk.len() as u32;
            data.extend_from_slice(&offset.to_le_bytes());
        }
        for chunk in chunks {
            data.extend_from_slice(chunk);
        }
        data
    }

    fn synthetic_rix() -> Vec<u8> {
        let mut data = vec![0; 96];
        data[0..2].copy_from_slice(&0x55aau16.to_le_bytes());
        data[8..10].copy_from_slice(&16u16.to_le_bytes());
        data[12..14].copy_from_slice(&72u16.to_le_bytes());
        for operator in [0usize, 13] {
            for (field, value) in [(1, 1u16), (2, 2), (3, 15), (4, 4), (6, 2), (7, 3), (12, 1)] {
                let offset = 16 + (operator + field) * 2;
                data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
            }
        }
        data[72..86]
            .copy_from_slice(&[0, 0x90, 127, 0xb0, 60, 0xc0, 14, 0, 0, 0xc0, 14, 0, 0, 0x80]);
        data
    }

    #[test]
    fn rejects_invalid_archives_and_sound_fonts_without_opening_audio() {
        assert!(SoundEffects::new(&[]).is_none());
        assert!(BackgroundMusic::new(&[], &[], &[]).is_none());
        assert!(!validate_sound_font(b"not a soundfont"));
        assert!(!validate_rix_output(b"not a RIX track"));
    }

    #[test]
    fn rix_opl2_rendering_produces_audio_and_obeys_live_loop_control() {
        let rix = synthetic_rix();
        assert!(validate_rix_output(&rix));

        let loop_control = Arc::new(AtomicBool::new(true));
        let mut source = StreamingRixSource::new(&rix, Arc::clone(&loop_control)).unwrap();
        let repeated_samples = RIX_SAMPLES_PER_TICK * 2 * 12;
        assert_eq!(
            source.by_ref().take(repeated_samples).count(),
            repeated_samples
        );
        loop_control.store(false, Ordering::Release);
        assert!(source.count() < RIX_SAMPLES_PER_TICK * 2 * 12);
    }

    #[test]
    fn unavailable_backend_does_not_change_the_selected_timbre() {
        let rix = synthetic_rix();
        let rix_mkf = make_mkf(&[&rix]);
        let mut music = BackgroundMusic::new(&rix_mkf, &[], &[]).unwrap();
        assert_eq!(music.backend(), MusicBackend::Rix);
        assert!(!music.backend_available(MusicBackend::Midi));
        assert!(!music.set_backend(MusicBackend::Midi));
        assert_eq!(music.backend(), MusicBackend::Rix);
    }

    #[test]
    fn interleaves_midi_with_headroom_and_smooth_limiting() {
        let pcm = interleave_pcm(&[0.0, 1.5], &[-1.5, 0.5]);
        assert_eq!(pcm[0], 0);
        assert_eq!(pcm[1], -pcm[2]);
        assert_eq!(
            pcm[3],
            (0.5 * MIDI_PREAMP_GAIN * f32::from(i16::MAX)) as i16
        );
        assert!(pcm[1].unsigned_abs() < i16::MAX as u16);
        assert!(pcm[2].unsigned_abs() < i16::MAX as u16);
        assert_eq!(midi_sample_to_pcm(f32::NAN), 0);
    }

    #[test]
    fn midi_synthesis_disables_brightening_effects_by_default() {
        assert!(!midi_synthesizer_settings().enable_reverb_and_chorus);
        assert_eq!(midi_sample_to_pcm(0.5), -midi_sample_to_pcm(-0.5));
        assert!(midi_sample_to_pcm(10.0).unsigned_abs() < i16::MAX as u16);
    }

    #[test]
    fn dynamic_volume_changes_an_active_source() {
        let volume = Arc::new(AtomicU8::new(50));
        let samples = SamplesBuffer::new(1, 8_000, vec![1_000i16, -1_000, i16::MAX]);
        let mut source = DynamicVolume::new(samples, Arc::clone(&volume));
        assert_eq!(source.next(), Some(500));
        volume.store(0, Ordering::Release);
        assert_eq!(source.next(), Some(0));
        volume.store(AUDIO_VOLUME_MAX, Ordering::Release);
        assert_eq!(source.next(), Some(i16::MAX));
    }

    #[test]
    fn volume_gain_clamps_to_the_public_range() {
        assert_eq!(volume_gain(0), 0.0);
        assert_eq!(volume_gain(50), 0.5);
        assert_eq!(volume_gain(u8::MAX), 1.0);
    }

    #[test]
    fn midi_loop_flag_is_rechecked_at_pass_and_release_boundaries() {
        assert_eq!(
            midi_pass_action(0, 10_000, 2_000, true),
            MidiPassAction::Render(MUSIC_BUFFER_FRAMES)
        );
        assert_eq!(
            midi_pass_action(10_000, 10_000, 2_000, true),
            MidiPassAction::Restart
        );
        assert_eq!(
            midi_pass_action(10_000, 10_000, 2_000, false),
            MidiPassAction::Render(2_000)
        );
        assert_eq!(
            midi_pass_action(11_000, 10_000, 2_000, true),
            MidiPassAction::Restart
        );
        assert_eq!(
            midi_pass_action(12_000, 10_000, 2_000, false),
            MidiPassAction::Finish
        );
    }
}
