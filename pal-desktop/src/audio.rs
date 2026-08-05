//! Desktop audio output for decoded PAL sound effects and SoundFont MIDI music.

use std::io::Cursor;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use pal_assets::mkf::MkfArchive;
use pal_assets::voc::VocClip;
use rodio::buffer::SamplesBuffer;
use rodio::{OutputStream, OutputStreamHandle, Sink, Source};
use rustysynth::{MidiFile, MidiFileSequencer, SoundFont, Synthesizer, SynthesizerSettings};

const MUSIC_SAMPLE_RATE: u32 = 44_100;
const MAX_MUSIC_SECONDS: f64 = 15.0 * 60.0;
const RELEASE_TAIL_SECONDS: f64 = 2.0;
const MUSIC_BUFFER_FRAMES: usize = 2048;
const MUSIC_BUFFER_COUNT: usize = 8;
pub const AUDIO_VOLUME_MAX: u8 = 100;
pub const AUDIO_VOLUME_STEP: u8 = 10;
const DEFAULT_MUSIC_VOLUME: u8 = 70;
const DEFAULT_SOUND_VOLUME: u8 = 100;

pub struct SoundEffects {
    archive: MkfArchive,
    output: Option<(OutputStream, OutputStreamHandle)>,
    volume: Arc<AtomicU8>,
    enabled: bool,
}

pub struct BackgroundMusic {
    archive: MkfArchive,
    sound_font: Arc<SoundFont>,
    output: Option<(OutputStream, OutputStreamHandle)>,
    sink: Option<Sink>,
    pending: Option<Receiver<Option<StreamingMidiSource>>>,
    pending_fade: Option<Duration>,
    loop_control: Option<Arc<AtomicBool>>,
    current: Option<u16>,
    volume: u8,
    enabled: bool,
}

impl BackgroundMusic {
    pub fn new(midi_mkf: &[u8], sound_font: &[u8]) -> Option<Self> {
        Some(Self {
            archive: MkfArchive::new(midi_mkf)?,
            sound_font: parse_sound_font(sound_font)?,
            output: OutputStream::try_default().ok(),
            sink: None,
            pending: None,
            pending_fade: None,
            loop_control: None,
            current: None,
            volume: DEFAULT_MUSIC_VOLUME,
            enabled: true,
        })
    }

    /// Decode and play one `MIDI.MKF` song with the configured General MIDI SoundFont.
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
        let Some(midi) = self
            .archive
            .read_chunk(usize::from(music_id))
            .map(<[u8]>::to_vec)
        else {
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
        sink.set_volume(volume_gain(self.volume));
        let fade = Duration::from_secs(u64::from(fade_seconds));
        let (sender, receiver) = mpsc::channel();
        let sound_font = Arc::clone(&self.sound_font);
        let loop_control = Arc::new(AtomicBool::new(looped));
        let producer_loop_control = Arc::clone(&loop_control);
        thread::spawn(move || {
            let source = StreamingMidiSource::new(&midi, &sound_font, producer_loop_control);
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

        let settings = SynthesizerSettings::new(MUSIC_SAMPLE_RATE as i32);
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
            .flat_map(|(&left, &right)| [float_to_pcm(left), float_to_pcm(right)])
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
        .flat_map(|(&left, &right)| [float_to_pcm(left), float_to_pcm(right)])
        .collect()
}

fn float_to_pcm(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16
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
