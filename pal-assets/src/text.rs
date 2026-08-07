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

/// Optional Big5 item and magic descriptions supplied by SDLPAL `desc.dat`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemDescriptions {
    entries: BTreeMap<u16, Vec<Vec<u8>>>,
}

impl ItemDescriptions {
    /// Parse lines in the form `HEX_ID(name)=line one*line two`.
    ///
    /// Header and blank lines are ignored. `*` separates display lines while
    /// the retained text remains in its original Big5 encoding.
    pub fn parse(data: &[u8]) -> Option<Self> {
        let mut entries = BTreeMap::new();
        for raw_line in data.split(|byte| *byte == b'\n') {
            let line = raw_line.strip_suffix(b"\r").unwrap_or(raw_line);
            let Some(open) = line.iter().position(|byte| *byte == b'(') else {
                continue;
            };
            let Ok(id_text) = std::str::from_utf8(&line[..open]) else {
                continue;
            };
            if id_text.is_empty() || !id_text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                continue;
            }
            let close = line.windows(2).position(|bytes| bytes == b")=")?;
            if close <= open + 1 {
                return None;
            }
            let id = u16::from_str_radix(id_text, 16).ok()?;
            let name = &line[open + 1..close];
            let description = &line[close + 2..];
            if name.is_empty()
                || description.is_empty()
                || !is_valid_big5_text(name)
                || !is_valid_big5_text(description)
            {
                return None;
            }
            let lines = description
                .split(|byte| *byte == b'*')
                .map(|line| (!line.is_empty()).then(|| line.to_vec()))
                .collect::<Option<Vec<_>>>()?;
            if entries.insert(id, lines).is_some() {
                return None;
            }
        }
        (!entries.is_empty()).then_some(Self { entries })
    }

    pub fn lines(&self, object_id: u16) -> Option<&[Vec<u8>]> {
        self.entries.get(&object_id).map(Vec::as_slice)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (u16, &[Vec<u8>])> {
        self.entries
            .iter()
            .map(|(&object_id, lines)| (object_id, lines.as_slice()))
    }
}

fn is_valid_big5_text(text: &[u8]) -> bool {
    let mut index = 0;
    while index < text.len() {
        let lead = text[index];
        if lead < 0x80 {
            index += 1;
            continue;
        }
        let Some(&trail) = text.get(index + 1) else {
            return false;
        };
        if !(0x81..=0xfe).contains(&lead)
            || !((0x40..=0x7e).contains(&trail) || (0xa1..=0xfe).contains(&trail))
        {
            return false;
        }
        index += 2;
    }
    true
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
    fn parses_optional_item_descriptions_and_line_breaks() {
        let data = b"header\r\n3d(\xa4\x40\xa4\x41)=\xa4\x42*HP+150\r\n40(\xa4\x43)=\xa4\x44\r\n";
        let descriptions = ItemDescriptions::parse(data).unwrap();
        assert_eq!(descriptions.len(), 2);
        assert_eq!(
            descriptions.lines(0x3d),
            Some([b"\xa4\x42".to_vec(), b"HP+150".to_vec()].as_slice())
        );
        assert_eq!(
            descriptions.lines(0x40),
            Some([b"\xa4\x44".to_vec()].as_slice())
        );
        assert_eq!(descriptions.lines(0x41), None);
    }

    #[test]
    fn rejects_malformed_or_duplicate_item_descriptions() {
        assert!(ItemDescriptions::parse(b"3d(\xa4\x40)=\xa4\r\n").is_none());
        assert!(
            ItemDescriptions::parse(b"3d(\xa4\x40)=first\r\n3d(\xa4\x40)=second\r\n").is_none()
        );
        assert!(ItemDescriptions::parse(b"3d(\xa4\x40)=valid\r\n40(broken\r\n").is_none());
        assert!(ItemDescriptions::parse(b"header only\r\n").is_none());
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
