//! Creative Voice File clips stored in `VOC.MKF`.

const VOC_SIGNATURE: &[u8; 20] = b"Creative Voice File\x1a";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VocClip {
    pub sample_rate: u32,
    /// Unsigned 8-bit mono PCM, centered at 128.
    pub samples: Vec<u8>,
}

impl VocClip {
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.get(..20)? != VOC_SIGNATURE {
            return None;
        }
        let data_offset = usize::from(u16::from_le_bytes(data.get(20..22)?.try_into().ok()?));
        let version = u16::from_le_bytes(data.get(22..24)?.try_into().ok()?);
        let checksum = u16::from_le_bytes(data.get(24..26)?.try_into().ok()?);
        if data_offset < 26
            || data_offset > data.len()
            || checksum != (!version).wrapping_add(0x1234)
        {
            return None;
        }

        let mut cursor = data_offset;
        let mut sample_rate = None;
        let mut samples = Vec::new();
        let mut terminated = false;
        while cursor < data.len() {
            let block_type = *data.get(cursor)?;
            cursor += 1;
            if block_type == 0 {
                terminated = true;
                break;
            }
            let size_bytes = data.get(cursor..cursor.checked_add(3)?)?;
            cursor += 3;
            let size = usize::from(size_bytes[0])
                | (usize::from(size_bytes[1]) << 8)
                | (usize::from(size_bytes[2]) << 16);
            let block = data.get(cursor..cursor.checked_add(size)?)?;
            cursor += size;

            match block_type {
                1 => {
                    let (&time_constant, payload) = block.split_first()?;
                    let (&codec, pcm) = payload.split_first()?;
                    if codec != 0 {
                        return None;
                    }
                    let rate =
                        1_000_000u32.checked_div(256u32.checked_sub(time_constant.into())?)?;
                    set_sample_rate(&mut sample_rate, rate)?;
                    samples.extend_from_slice(pcm);
                }
                2 => {
                    sample_rate?;
                    samples.extend_from_slice(block);
                }
                3 => {
                    let count = u16::from_le_bytes(block.get(..2)?.try_into().ok()?);
                    let time_constant = *block.get(2)?;
                    if block.len() != 3 {
                        return None;
                    }
                    let rate =
                        1_000_000u32.checked_div(256u32.checked_sub(time_constant.into())?)?;
                    set_sample_rate(&mut sample_rate, rate)?;
                    samples.resize(samples.len().checked_add(usize::from(count) + 1)?, 128);
                }
                4 if block.len() == 2 => {}
                5 => {}
                9 => {
                    if block.len() < 12 {
                        return None;
                    }
                    let rate = u32::from_le_bytes(block[0..4].try_into().ok()?);
                    let bits = block[4];
                    let channels = block[5];
                    let codec = u16::from_le_bytes(block[6..8].try_into().ok()?);
                    if rate == 0 || bits != 8 || channels != 1 || codec != 0 {
                        return None;
                    }
                    set_sample_rate(&mut sample_rate, rate)?;
                    samples.extend_from_slice(&block[12..]);
                }
                _ => return None,
            }
        }

        terminated
            .then_some(Self {
                sample_rate: sample_rate?,
                samples,
            })
            .filter(|clip| !clip.samples.is_empty())
    }
}

fn set_sample_rate(current: &mut Option<u32>, rate: u32) -> Option<()> {
    if rate == 0 || current.is_some_and(|current| current != rate) {
        return None;
    }
    *current = Some(rate);
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voc(blocks: &[u8]) -> Vec<u8> {
        let version = 0x010a_u16;
        let mut data = VOC_SIGNATURE.to_vec();
        data.extend_from_slice(&26u16.to_le_bytes());
        data.extend_from_slice(&version.to_le_bytes());
        data.extend_from_slice(&(!version).wrapping_add(0x1234).to_le_bytes());
        data.extend_from_slice(blocks);
        data
    }

    #[test]
    fn parses_pcm_continuation_and_silence() {
        let clip = VocClip::parse(&voc(&[
            1, 4, 0, 0, 156, 0, 0, 255, 2, 2, 0, 0, 64, 192, 3, 3, 0, 0, 1, 0, 156, 0,
        ]))
        .unwrap();
        assert_eq!(clip.sample_rate, 10_000);
        assert_eq!(clip.samples, vec![0, 255, 64, 192, 128, 128]);
    }

    #[test]
    fn parses_new_pcm_markers_and_text_blocks() {
        let clip = VocClip::parse(&voc(&[
            4, 2, 0, 0, 9, 0, // marker
            5, 3, 0, 0, b'P', b'A', b'L', // text
            9, 14, 0, 0, 0x10, 0x27, 0, 0, 8, 1, 0, 0, 0, 0, 0, 0, 12, 34, // PCM
            0,
        ]))
        .unwrap();
        assert_eq!(clip.sample_rate, 10_000);
        assert_eq!(clip.samples, [12, 34]);
    }

    #[test]
    fn rejects_truncation_bad_headers_and_unsupported_codecs() {
        assert!(VocClip::parse(&[]).is_none());
        let mut bad_checksum = voc(&[0]);
        bad_checksum[24] ^= 1;
        assert!(VocClip::parse(&bad_checksum).is_none());
        assert!(VocClip::parse(&voc(&[1, 4, 0, 0, 156, 1, 0, 0, 0])).is_none());
        assert!(VocClip::parse(&voc(&[1, 8, 0])).is_none());
        assert!(VocClip::parse(&voc(&[1, 4, 0, 0, 156, 0, 0, 1])).is_none());
    }
}
