//! `RNG.MKF` animation archive and incremental frame decoder.
//!
//! Each outer MKF chunk is itself an offset table whose sub-chunks are YJ_1
//! compressed command streams. A command stream updates a persistent 320x200
//! indexed-color canvas two pixels at a time.

use crate::{mkf::MkfArchive, yj1};

pub const RNG_WIDTH: usize = 320;
pub const RNG_HEIGHT: usize = 200;
pub const RNG_FRAME_PIXELS: usize = RNG_WIDTH * RNG_HEIGHT;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RngDecodeError {
    InvalidCanvasLength { actual: usize },
    TruncatedInput,
    DestinationOverflow,
    UnknownOpcode(u8),
}

/// Parsed `RNG.MKF` archive.
#[derive(Debug)]
pub struct RngArchive {
    archive: MkfArchive,
}

impl RngArchive {
    /// Parse the outer MKF archive and validate its chunk boundaries.
    pub fn new(data: &[u8]) -> Option<Self> {
        Some(Self {
            archive: MkfArchive::new(data)?,
        })
    }

    pub fn animation_count(&self) -> usize {
        self.archive.chunk_count()
    }

    /// Parse one zero-based animation's inner frame table.
    pub fn animation(&self, index: usize) -> Option<RngAnimation<'_>> {
        RngAnimation::new(self.archive.read_chunk(index)?)
    }
}

/// One RNG animation borrowed from an archive.
#[derive(Debug)]
pub struct RngAnimation<'a> {
    data: &'a [u8],
    offsets: Vec<usize>,
}

impl<'a> RngAnimation<'a> {
    fn new(data: &'a [u8]) -> Option<Self> {
        if data.is_empty() {
            return Some(Self {
                data,
                offsets: vec![0],
            });
        }
        let table_size = read_u32(data, 0)? as usize;
        if table_size < 8 || table_size > data.len() || !table_size.is_multiple_of(4) {
            return None;
        }

        let frame_count = table_size.checked_div(4)?.checked_sub(1)?;
        let offsets = (0..=frame_count)
            .map(|index| Some(read_u32(data, index.checked_mul(4)?)? as usize))
            .collect::<Option<Vec<_>>>()?;
        if offsets.first().copied()? != table_size
            || offsets
                .windows(2)
                .any(|pair| pair[0] > pair[1] || pair[1] > data.len())
        {
            return None;
        }

        Some(Self { data, offsets })
    }

    pub fn frame_count(&self) -> usize {
        self.offsets.len() - 1
    }

    /// Borrow one frame's compressed YJ_1 stream.
    pub fn compressed_frame(&self, index: usize) -> Option<&'a [u8]> {
        let start = *self.offsets.get(index)?;
        let end = *self.offsets.get(index.checked_add(1)?)?;
        self.data.get(start..end)
    }

    /// Decompress one frame without applying its incremental commands.
    pub fn decompress_frame(&self, index: usize) -> Option<Vec<u8>> {
        yj1::decompress(self.compressed_frame(index)?)
    }

    /// Decompress and apply one frame to the previous indexed-color canvas.
    pub fn apply_frame(&self, index: usize, canvas: &mut [u8]) -> Option<()> {
        apply_frame_delta_checked(&self.decompress_frame(index)?, canvas).ok()
    }
}

/// Apply one decompressed RNG command stream to a persistent 320x200 canvas.
///
/// Skipped pixels retain their value from the previous frame. Invalid commands,
/// truncated operands, and writes or skips past the canvas are rejected.
pub fn apply_frame_delta(commands: &[u8], canvas: &mut [u8]) -> Option<()> {
    apply_frame_delta_checked(commands, canvas).ok()
}

/// Checked variant of [`apply_frame_delta`] that reports the failure class.
pub fn apply_frame_delta_checked(commands: &[u8], canvas: &mut [u8]) -> Result<(), RngDecodeError> {
    if canvas.len() != RNG_FRAME_PIXELS {
        return Err(RngDecodeError::InvalidCanvasLength {
            actual: canvas.len(),
        });
    }

    let mut source = 0usize;
    let mut destination = 0usize;
    while source < commands.len() {
        let opcode = take_u8(commands, &mut source)?;
        match opcode {
            0x00 | 0x13 => return Ok(()),
            0x02 => skip_pairs(&mut destination, 1)?,
            0x03 => {
                let pairs = usize::from(take_u8(commands, &mut source)?) + 1;
                skip_pairs(&mut destination, pairs)?;
            }
            0x04 => {
                let pairs = usize::from(take_u16(commands, &mut source)?) + 1;
                skip_pairs(&mut destination, pairs)?;
            }
            0x06..=0x0a => {
                let pairs = usize::from(opcode - 0x05);
                copy_pairs(commands, &mut source, canvas, &mut destination, pairs)?;
            }
            0x0b => {
                let pairs = usize::from(take_u8(commands, &mut source)?) + 1;
                copy_pairs(commands, &mut source, canvas, &mut destination, pairs)?;
            }
            0x0c => {
                let pairs = usize::from(take_u16(commands, &mut source)?) + 1;
                copy_pairs(commands, &mut source, canvas, &mut destination, pairs)?;
            }
            0x0d..=0x10 => {
                let pairs = usize::from(opcode - 0x0b);
                repeat_pair(commands, &mut source, canvas, &mut destination, pairs)?;
            }
            0x11 => {
                let pairs = usize::from(take_u8(commands, &mut source)?) + 1;
                repeat_pair(commands, &mut source, canvas, &mut destination, pairs)?;
            }
            0x12 => {
                let pairs = usize::from(take_u16(commands, &mut source)?) + 1;
                repeat_pair(commands, &mut source, canvas, &mut destination, pairs)?;
            }
            _ => return Err(RngDecodeError::UnknownOpcode(opcode)),
        }
    }

    // The original decoder also accepts a stream that ends exactly after its
    // last command, even when it has no explicit 0x00/0x13 terminator.
    Ok(())
}

fn skip_pairs(destination: &mut usize, pairs: usize) -> Result<(), RngDecodeError> {
    let bytes = pairs
        .checked_mul(2)
        .ok_or(RngDecodeError::DestinationOverflow)?;
    *destination = destination
        .checked_add(bytes)
        .ok_or(RngDecodeError::DestinationOverflow)?;
    (*destination <= RNG_FRAME_PIXELS)
        .then_some(())
        .ok_or(RngDecodeError::DestinationOverflow)
}

fn copy_pairs(
    commands: &[u8],
    source: &mut usize,
    canvas: &mut [u8],
    destination: &mut usize,
    pairs: usize,
) -> Result<(), RngDecodeError> {
    let bytes = pairs
        .checked_mul(2)
        .ok_or(RngDecodeError::DestinationOverflow)?;
    let source_end = source
        .checked_add(bytes)
        .ok_or(RngDecodeError::TruncatedInput)?;
    let destination_end = destination
        .checked_add(bytes)
        .ok_or(RngDecodeError::DestinationOverflow)?;
    let input = commands
        .get(*source..source_end)
        .ok_or(RngDecodeError::TruncatedInput)?;
    canvas
        .get_mut(*destination..destination_end)
        .ok_or(RngDecodeError::DestinationOverflow)?
        .copy_from_slice(input);
    *source = source_end;
    *destination = destination_end;
    Ok(())
}

fn repeat_pair(
    commands: &[u8],
    source: &mut usize,
    canvas: &mut [u8],
    destination: &mut usize,
    pairs: usize,
) -> Result<(), RngDecodeError> {
    let first = take_u8(commands, source)?;
    let second = take_u8(commands, source)?;
    let bytes = pairs
        .checked_mul(2)
        .ok_or(RngDecodeError::DestinationOverflow)?;
    let destination_end = destination
        .checked_add(bytes)
        .ok_or(RngDecodeError::DestinationOverflow)?;
    let output = canvas
        .get_mut(*destination..destination_end)
        .ok_or(RngDecodeError::DestinationOverflow)?;
    for pair in output.chunks_exact_mut(2) {
        pair.copy_from_slice(&[first, second]);
    }
    *destination = destination_end;
    Ok(())
}

fn take_u8(data: &[u8], cursor: &mut usize) -> Result<u8, RngDecodeError> {
    let value = *data.get(*cursor).ok_or(RngDecodeError::TruncatedInput)?;
    *cursor = cursor
        .checked_add(1)
        .ok_or(RngDecodeError::TruncatedInput)?;
    Ok(value)
}

fn take_u16(data: &[u8], cursor: &mut usize) -> Result<u16, RngDecodeError> {
    let end = cursor
        .checked_add(2)
        .ok_or(RngDecodeError::TruncatedInput)?;
    let value = u16::from_le_bytes(
        data.get(*cursor..end)
            .ok_or(RngDecodeError::TruncatedInput)?
            .try_into()
            .map_err(|_| RngDecodeError::TruncatedInput)?,
    );
    *cursor = end;
    Ok(value)
}

fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    Some(u32::from_le_bytes(data.get(offset..end)?.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_yj1(data: &[u8]) -> Vec<u8> {
        let mut result = Vec::new();
        result.extend_from_slice(b"YJ_1");
        result.extend_from_slice(&(data.len() as u32).to_le_bytes());
        result.extend_from_slice(&(20u32 + data.len() as u32).to_le_bytes());
        result.extend_from_slice(&1u16.to_le_bytes());
        result.extend_from_slice(&[0, 0]);
        result.extend_from_slice(&(data.len() as u16).to_le_bytes());
        result.extend_from_slice(&0u16.to_le_bytes());
        result.extend_from_slice(data);
        result
    }

    fn inner_archive(frames: &[Vec<u8>]) -> Vec<u8> {
        let table_size = (frames.len() + 1) * 4;
        let mut offset = table_size as u32;
        let mut data = Vec::new();
        data.extend_from_slice(&offset.to_le_bytes());
        for frame in frames {
            offset += frame.len() as u32;
            data.extend_from_slice(&offset.to_le_bytes());
        }
        for frame in frames {
            data.extend_from_slice(frame);
        }
        data
    }

    fn outer_archive(animations: &[Vec<u8>]) -> Vec<u8> {
        inner_archive(animations)
    }

    #[test]
    fn reads_nested_archive_and_applies_yj1_frame() {
        let commands = [0x06, 1, 2, 0x02, 0x0d, 3, 4, 0x00];
        let animation = inner_archive(&[raw_yj1(&commands)]);
        let archive = RngArchive::new(&outer_archive(&[animation])).unwrap();
        assert_eq!(archive.animation_count(), 1);
        let animation = archive.animation(0).unwrap();
        assert_eq!(animation.frame_count(), 1);

        let mut canvas = vec![9; RNG_FRAME_PIXELS];
        animation.apply_frame(0, &mut canvas).unwrap();
        assert_eq!(&canvas[..8], &[1, 2, 9, 9, 3, 4, 3, 4]);
        assert!(animation.apply_frame(1, &mut canvas).is_none());
    }

    #[test]
    fn decodes_literal_skip_and_repeat_command_variants() {
        let commands = [
            0x03, 0x00, // skip one pair
            0x07, 1, 2, 3, 4, // copy two pairs
            0x0b, 0x00, 5, 6, // byte-counted literal, one pair
            0x0c, 0x00, 0x00, 7, 8, // word-counted literal, one pair
            0x0e, 9, 10, // repeat three pairs
            0x11, 0x00, 11, 12, // byte-counted repeat, one pair
            0x12, 0x00, 0x00, 13, 14, // word-counted repeat, one pair
            0x13,
        ];
        let mut canvas = vec![0xaa; RNG_FRAME_PIXELS];
        apply_frame_delta(&commands, &mut canvas).unwrap();
        assert_eq!(
            &canvas[..22],
            &[
                0xaa, 0xaa, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 9, 10, 9, 10, 11, 12, 13, 14, 0xaa,
                0xaa,
            ]
        );
    }

    #[test]
    fn rejects_truncated_commands_invalid_opcodes_and_bad_canvas() {
        for commands in [
            &[0x03][..],
            &[0x04, 0][..],
            &[0x06, 1][..],
            &[0x0b, 1, 1, 2][..],
            &[0x11, 0, 1][..],
            &[0x12, 0][..],
            &[0x01][..],
        ] {
            assert!(apply_frame_delta(commands, &mut vec![0; RNG_FRAME_PIXELS]).is_none());
        }
        assert!(apply_frame_delta(&[0], &mut vec![0; RNG_FRAME_PIXELS - 1]).is_none());
    }

    #[test]
    fn rejects_destination_overflow_for_skips_writes_and_repeats() {
        let mut skip = vec![0x04];
        skip.extend_from_slice(&u16::MAX.to_le_bytes());
        assert!(apply_frame_delta(&skip, &mut vec![0; RNG_FRAME_PIXELS]).is_none());

        let pairs_to_end = (RNG_FRAME_PIXELS / 2 - 1) as u16;
        let mut write = vec![0x04];
        write.extend_from_slice(&pairs_to_end.to_le_bytes());
        write.extend_from_slice(&[0x06, 1, 2]);
        assert!(apply_frame_delta(&write, &mut vec![0; RNG_FRAME_PIXELS]).is_none());

        let mut repeat = vec![0x04];
        repeat.extend_from_slice(&pairs_to_end.to_le_bytes());
        repeat.extend_from_slice(&[0x0d, 1, 2]);
        assert!(apply_frame_delta(&repeat, &mut vec![0; RNG_FRAME_PIXELS]).is_none());
    }

    #[test]
    fn rejects_invalid_inner_tables_and_empty_frames() {
        let valid = inner_archive(&[raw_yj1(&[0])]);
        assert!(RngArchive::new(&outer_archive(&[valid]))
            .unwrap()
            .animation(0)
            .is_some());

        let empty = RngArchive::new(&outer_archive(&[Vec::new()])).unwrap();
        assert_eq!(empty.animation(0).unwrap().frame_count(), 0);

        for invalid in [
            vec![0; 8],
            vec![12, 0, 0, 0, 11, 0, 0, 0, 12, 0, 0, 0],
            vec![8, 0, 0, 0, 20, 0, 0, 0],
        ] {
            let archive = RngArchive::new(&outer_archive(&[invalid])).unwrap();
            assert!(archive.animation(0).is_none());
        }

        let empty_frame = inner_archive(&[Vec::new()]);
        let archive = RngArchive::new(&outer_archive(&[empty_frame])).unwrap();
        let animation = archive.animation(0).unwrap();
        assert_eq!(animation.frame_count(), 1);
        assert!(animation
            .apply_frame(0, &mut vec![0; RNG_FRAME_PIXELS])
            .is_none());
    }
}
