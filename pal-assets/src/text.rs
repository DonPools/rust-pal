//! Traditional Chinese text and bitmap-font resources used by PAL.

use std::collections::BTreeMap;

const DEFAULT_WORD_LENGTH: usize = 10;
const FONT_DATA_OFFSET: usize = 0x682;
const GLYPH_SIZE: usize = 30;

/// One 16x15 monochrome glyph from `WOR16.FON`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontGlyph {
    pub rows: [[u8; 2]; 15],
}

/// Big5 code-to-glyph mapping assembled from `WOR16.ASC` and `WOR16.FON`.
#[derive(Debug)]
pub struct BitmapFont {
    glyphs: BTreeMap<u16, FontGlyph>,
}

impl BitmapFont {
    /// Parse the Big5 code table and its corresponding 16x15 bitmap records.
    pub fn parse(code_table: &[u8], font_data: &[u8]) -> Option<Self> {
        if code_table.is_empty() || !code_table.len().is_multiple_of(2) {
            return None;
        }
        let glyph_count = code_table.len() / 2;
        let glyph_bytes = glyph_count.checked_mul(GLYPH_SIZE)?;
        let end = FONT_DATA_OFFSET.checked_add(glyph_bytes)?;
        if font_data.len() < end {
            return None;
        }

        let mut glyphs = BTreeMap::new();
        for (code, bytes) in code_table
            .chunks_exact(2)
            .zip(font_data[FONT_DATA_OFFSET..end].chunks_exact(GLYPH_SIZE))
        {
            let code = u16::from_be_bytes([code[0], code[1]]);
            let mut rows = [[0; 2]; 15];
            for (row, source) in rows.iter_mut().zip(bytes.chunks_exact(2)) {
                row.copy_from_slice(source);
            }
            glyphs.insert(code, FontGlyph { rows });
        }
        Some(Self { glyphs })
    }

    pub fn glyph(&self, big5_code: u16) -> Option<&FontGlyph> {
        self.glyphs.get(&big5_code)
    }

    pub fn glyph_count(&self) -> usize {
        self.glyphs.len()
    }
}

/// Fixed-width words and indexed messages retained in their original Big5 encoding.
#[derive(Debug)]
pub struct TextLibrary {
    words: Vec<Vec<u8>>,
    messages: Vec<Vec<u8>>,
}

impl TextLibrary {
    /// Parse `WORD.DAT`, `M.MSG`, and the message-offset chunk from `SSS.MKF`.
    pub fn parse(word_data: &[u8], message_data: &[u8], message_index: &[u8]) -> Option<Self> {
        Self::parse_with_word_length(word_data, message_data, message_index, DEFAULT_WORD_LENGTH)
    }

    fn parse_with_word_length(
        word_data: &[u8],
        message_data: &[u8],
        message_index: &[u8],
        word_length: usize,
    ) -> Option<Self> {
        if word_length == 0
            || word_data.is_empty()
            || !word_data.len().is_multiple_of(word_length)
            || message_index.len() < 8
            || !message_index.len().is_multiple_of(4)
        {
            return None;
        }

        let words = word_data
            .chunks_exact(word_length)
            .map(|word| {
                let end = word
                    .iter()
                    .rposition(|byte| *byte != 0 && *byte != b' ')
                    .map_or(0, |index| index + 1);
                let mut word = word[..end].to_vec();
                if word.last() == Some(&b'1') {
                    word.pop();
                    while word.last() == Some(&b' ') {
                        word.pop();
                    }
                }
                word
            })
            .collect();

        let offsets = message_index
            .chunks_exact(4)
            .map(|bytes| {
                usize::try_from(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])).ok()
            })
            .collect::<Option<Vec<_>>>()?;
        if offsets[0] != 0
            || offsets.windows(2).any(|pair| pair[0] > pair[1])
            || offsets.last().copied()? > message_data.len()
        {
            return None;
        }
        let messages = offsets
            .windows(2)
            .map(|pair| message_data[pair[0]..pair[1]].to_vec())
            .collect();

        Some(Self { words, messages })
    }

    pub fn word(&self, index: usize) -> Option<&[u8]> {
        self.words.get(index).map(Vec::as_slice)
    }

    pub fn message(&self, index: usize) -> Option<&[u8]> {
        self.messages.get(index).map(Vec::as_slice)
    }

    pub fn word_count(&self) -> usize {
        self.words.len()
    }

    pub fn message_count(&self) -> usize {
        self.messages.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_words_and_indexed_messages() {
        let words = b"first     second   1";
        let messages = b"onetwothree";
        let mut index = Vec::new();
        for offset in [0u32, 3, 6, 11] {
            index.extend_from_slice(&offset.to_le_bytes());
        }

        let text = TextLibrary::parse(words, messages, &index).unwrap();
        assert_eq!(text.word_count(), 2);
        assert_eq!(text.word(0), Some(b"first".as_slice()));
        assert_eq!(text.word(1), Some(b"second".as_slice()));
        assert_eq!(text.message_count(), 3);
        assert_eq!(text.message(1), Some(b"two".as_slice()));
        assert_eq!(text.message(2), Some(b"three".as_slice()));
    }

    #[test]
    fn rejects_invalid_text_boundaries() {
        assert!(TextLibrary::parse(b"short", b"message", &[0; 8]).is_none());

        let past_end = [0u32, 8]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        assert!(TextLibrary::parse(&[b' '; 10], b"short", &past_end).is_none());

        let backwards = [0u32, 4, 3]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        assert!(TextLibrary::parse(&[b' '; 10], b"text", &backwards).is_none());
    }

    #[test]
    fn parses_font_mapping_and_rejects_truncation() {
        let mut font_data = vec![0; FONT_DATA_OFFSET + GLYPH_SIZE];
        font_data[FONT_DATA_OFFSET] = 0x80;
        let font = BitmapFont::parse(&[0xb8, 0x67], &font_data).unwrap();
        assert_eq!(font.glyph_count(), 1);
        assert_eq!(font.glyph(0xb867).unwrap().rows[0], [0x80, 0]);
        assert!(BitmapFont::parse(&[0xb8], &font_data).is_none());
        assert!(BitmapFont::parse(&[0xb8, 0x67], &font_data[..font_data.len() - 1]).is_none());
    }
}
