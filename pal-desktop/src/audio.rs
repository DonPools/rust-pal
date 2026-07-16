//! Desktop audio output for decoded PAL sound effects.

use pal_assets::mkf::MkfArchive;
use pal_assets::voc::VocClip;
use rodio::buffer::SamplesBuffer;
use rodio::{OutputStream, OutputStreamHandle, Sink};

pub struct SoundEffects {
    archive: MkfArchive,
    output: Option<(OutputStream, OutputStreamHandle)>,
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
    fn rejects_invalid_archives_without_opening_audio() {
        assert!(SoundEffects::new(&[]).is_none());
    }
}
