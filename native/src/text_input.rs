//! Text editing helpers for single-line fields. Offsets are UTF-8 byte offsets into the field's
//! value unless a name says otherwise. UTF-16 offsets appear only at the platform input boundary,
//! because IME and the text services speak UTF-16.

use std::ops::Range;

/// Moves a byte offset back to the nearest character boundary, and clamps it to the text.
pub fn clamp_to_boundary(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

pub fn previous_boundary(text: &str, index: usize) -> usize {
    text[..index].char_indices().next_back().map_or(0, |(start, _)| start)
}

pub fn next_boundary(text: &str, index: usize) -> usize {
    text[index..].chars().next().map_or(text.len(), |ch| index + ch.len_utf8())
}

fn word_class(ch: char) -> u8 {
    if ch.is_alphanumeric() || ch == '_' { 1 }
    else if ch.is_whitespace() { 2 }
    else { 3 }
}

/// Moves left over adjacent whitespace and the preceding word or punctuation run.
pub fn previous_word_boundary(text: &str, index: usize) -> usize {
    let mut at = clamp_to_boundary(text, index);
    while at > 0 {
        let previous = previous_boundary(text, at);
        if !text[previous..at].chars().next().is_some_and(char::is_whitespace) { break; }
        at = previous;
    }
    let Some(ch) = text[..at].chars().next_back() else { return at; };
    let class = word_class(ch);
    while at > 0 {
        let previous = previous_boundary(text, at);
        if word_class(text[previous..at].chars().next().unwrap()) != class { break; }
        at = previous;
    }
    at
}

/// Moves right over the current word or punctuation run and following whitespace.
pub fn next_word_boundary(text: &str, index: usize) -> usize {
    let mut at = clamp_to_boundary(text, index);
    if at == text.len() { return at; }
    let class = word_class(text[at..].chars().next().unwrap());
    while at < text.len() {
        let next = next_boundary(text, at);
        if word_class(text[at..next].chars().next().unwrap()) != class { break; }
        at = next;
    }
    while at < text.len() {
        let next = next_boundary(text, at);
        if !text[at..next].chars().next().is_some_and(char::is_whitespace) { break; }
        at = next;
    }
    at
}

/// Converts a UTF-16 offset to a byte offset. An offset inside a surrogate pair rounds up to the
/// end of that character.
pub fn utf16_to_byte(text: &str, offset: usize) -> usize {
    let mut units = 0;
    for (byte, ch) in text.char_indices() {
        if units >= offset {
            return byte;
        }
        units += ch.len_utf16();
    }
    text.len()
}

pub fn byte_to_utf16(text: &str, index: usize) -> usize {
    text[..clamp_to_boundary(text, index)].encode_utf16().count()
}

/// Removes line breaks from text inserted into a single-line field, such as a paste.
pub fn single_line(text: &str) -> String {
    text.chars().filter(|ch| *ch != '\n' && *ch != '\r').collect()
}

/// Line breaks in text inserted into a textarea become "\n", whatever the platform sent.
pub fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Byte offset of the start of the logical line that contains `index`.
pub fn line_start(text: &str, index: usize) -> usize {
    text[..clamp_to_boundary(text, index)].rfind('\n').map_or(0, |line_break| line_break + 1)
}

/// Byte offset of the end of the logical line that contains `index`, before its line break.
pub fn line_end(text: &str, index: usize) -> usize {
    let index = clamp_to_boundary(text, index);
    text[index..].find('\n').map_or(text.len(), |offset| index + offset)
}

/// The run of word characters, spaces, line breaks, or punctuation around `index`. A double-click
/// selects this range. Ahead of the caret wins; at the end of the text the character before it is used.
pub fn word_range(text: &str, index: usize) -> Range<usize> {
    fn class(ch: char) -> u8 { if ch == '\n' { 4 } else { word_class(ch) } }
    let index = clamp_to_boundary(text, index);
    let ahead = text[index..].chars().next();
    let behind = text[..index].chars().next_back();
    let Some(pivot) = ahead.or(behind) else { return index..index };
    let wanted = class(pivot);
    let mut start = if ahead.is_some() { index } else { index - pivot.len_utf8() };
    while let Some(ch) = text[..start].chars().next_back() {
        if class(ch) != wanted { break; }
        start -= ch.len_utf8();
    }
    let mut end = start;
    while let Some(ch) = text[end..].chars().next() {
        if class(ch) != wanted { break; }
        end += ch.len_utf8();
    }
    start..end
}

/// Replaces `range` with `insert`. Returns the new text and the offset just after the insertion.
pub fn replace_range(text: &str, range: Range<usize>, insert: &str) -> (String, usize) {
    let start = clamp_to_boundary(text, range.start);
    let end = clamp_to_boundary(text, range.end).max(start);
    let mut out = String::with_capacity(text.len() + insert.len());
    out.push_str(&text[..start]);
    out.push_str(insert);
    out.push_str(&text[end..]);
    (out, start + insert.len())
}

/// Byte offset in the displayed text for a byte offset in the value. A masked (password) field
/// shows one `*` per character, so the displayed offset is the character count before `index`.
pub fn display_offset(value: &str, index: usize, masked: bool) -> usize {
    if masked {
        value[..clamp_to_boundary(value, index)].chars().count()
    } else {
        index
    }
}

/// Inverse of [`display_offset`]: the value offset for a byte offset in the displayed text.
pub fn value_offset(value: &str, display: usize, masked: bool) -> usize {
    if masked {
        value.char_indices().nth(display).map_or(value.len(), |(start, _)| start)
    } else {
        clamp_to_boundary(value, display)
    }
}

/// A caret (when `anchor == head`) or a selection. `head` is the end that moves with the keyboard.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}

impl Selection {
    pub fn caret(index: usize) -> Self {
        Self { anchor: index, head: index }
    }

    pub fn range(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    pub fn reversed(&self) -> bool {
        self.head < self.anchor
    }

    /// Keeps both ends on character boundaries inside `text`, for when the value changed under a
    /// stored selection.
    pub fn clamped(self, text: &str) -> Self {
        Self {
            anchor: clamp_to_boundary(text, self.anchor),
            head: clamp_to_boundary(text, self.head),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundaries_step_over_whole_characters() {
        let text = "aé😀b";
        assert_eq!(next_boundary(text, 0), 1);
        assert_eq!(next_boundary(text, 1), 3);
        assert_eq!(next_boundary(text, 3), 7);
        assert_eq!(next_boundary(text, 7), 8);
        assert_eq!(next_boundary(text, 8), 8);
        assert_eq!(previous_boundary(text, 8), 7);
        assert_eq!(previous_boundary(text, 3), 1);
        assert_eq!(previous_boundary(text, 0), 0);
    }

    #[test]
    fn clamp_moves_inside_multibyte_characters_back() {
        assert_eq!(clamp_to_boundary("é", 1), 0);
        assert_eq!(clamp_to_boundary("é", 9), 2);
    }

    #[test]
    fn utf16_offsets_convert_both_ways() {
        let text = "a😀b";
        assert_eq!(byte_to_utf16(text, 0), 0);
        assert_eq!(byte_to_utf16(text, 1), 1);
        assert_eq!(byte_to_utf16(text, 5), 3);
        assert_eq!(utf16_to_byte(text, 0), 0);
        assert_eq!(utf16_to_byte(text, 1), 1);
        assert_eq!(utf16_to_byte(text, 3), 5);
        assert_eq!(utf16_to_byte(text, 99), text.len());
    }

    #[test]
    fn utf16_offset_inside_a_surrogate_pair_rounds_up() {
        assert_eq!(utf16_to_byte("😀b", 1), 4);
    }

    #[test]
    fn replace_range_inserts_and_reports_the_caret() {
        assert_eq!(replace_range("hello", 1..4, "EY"), ("hEYo".to_string(), 3));
        assert_eq!(replace_range("hello", 5..5, "!"), ("hello!".to_string(), 6));
        assert_eq!(replace_range("hello", 0..5, ""), (String::new(), 0));
        assert_eq!(replace_range("aé", 2..2, "x"), ("axé".to_string(), 2));
    }

    #[test]
    fn single_line_drops_line_breaks() {
        assert_eq!(single_line("a\r\nb\nc"), "abc");
    }

    #[test]
    fn masked_offsets_count_characters_not_bytes() {
        // "é" is 2 bytes, "😀" is 4 bytes, so the "x" starts at byte 6.
        let value = "é😀x";
        assert_eq!(display_offset(value, 6, true), 2);
        assert_eq!(display_offset(value, 6, false), 6);
        assert_eq!(display_offset(value, 3, true), 1);
        assert_eq!(value_offset(value, 2, true), 6);
        assert_eq!(value_offset(value, 9, true), value.len());
        assert_eq!(value_offset(value, 2, false), 2);
        assert_eq!(value_offset("é", 1, false), 0);
    }

    #[test]
    fn line_bounds_stop_at_line_breaks() {
        let text = "ab\ncdé\n";
        assert_eq!(line_start(text, 0), 0);
        assert_eq!(line_start(text, 2), 0, "the caret before a line break is still on that line");
        assert_eq!(line_start(text, 3), 3);
        assert_eq!(line_end(text, 3), 7);
        assert_eq!(line_end(text, 7), 7);
        assert_eq!(line_start(text, 99), 8);
    }

    #[test]
    fn newlines_are_normalized_for_textareas_only() {
        assert_eq!(normalize_newlines("a\r\nb\rc\nd"), "a\nb\nc\nd");
        assert_eq!(single_line("a\r\nb"), "ab");
    }

    #[test]
    fn word_selection_follows_character_classes() {
        let text = "hello world, ok";
        assert_eq!(word_range(text, 2), 0..5);
        assert_eq!(word_range(text, 5), 5..6, "at the space after a word, only the spaces");
        assert_eq!(word_range(text, 6), 6..11);
        assert_eq!(word_range(text, 11), 11..12, "punctuation is its own run");
        assert_eq!(word_range(text, 99), 13..15, "past the end uses the character before");
        assert_eq!(word_range("", 0), 0..0);
        assert_eq!(word_range("a\nb", 1), 1..2, "a line break is its own run");
    }

    #[test]
    fn word_selection_handles_multibyte_text() {
        assert_eq!(word_range("café ok", 1), 0..5);
    }

    #[test]
    fn selection_range_is_ordered_and_clamps() {
        let forward = Selection { anchor: 1, head: 3 };
        let backward = Selection { anchor: 3, head: 1 };
        assert_eq!(forward.range(), 1..3);
        assert_eq!(backward.range(), 1..3);
        assert!(backward.reversed());
        assert!(!forward.reversed());
        assert!(Selection::caret(2).is_empty());
        assert_eq!(Selection { anchor: 1, head: 99 }.clamped("abc"), Selection { anchor: 1, head: 3 });
    }
}
