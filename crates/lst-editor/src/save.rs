use crate::transaction::{TextChange, TextChangeSet};

#[derive(Clone, Copy, Debug, Default)]
pub struct SaveTextOptions {
    pub trim_trailing_whitespace: bool,
    pub ensure_final_newline: bool,
}

/// The bytes to write and the exact edits that produced them. Keep these
/// together until the write succeeds so the editor can map its positions
/// without reconstructing changes from a whole-document replacement.
#[derive(Debug, PartialEq, Eq)]
pub struct PreparedSave {
    text: String,
    changes: Option<TextChangeSet>,
}

impl PreparedSave {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn changes(&self) -> Option<&TextChangeSet> {
        self.changes.as_ref()
    }
}

impl SaveTextOptions {
    pub fn prepare(self, text: String) -> PreparedSave {
        if !self.trim_trailing_whitespace && !self.ensure_final_newline {
            return PreparedSave { text, changes: None };
        }

        let mut output = String::with_capacity(text.len());
        let mut changes = Vec::new();
        let mut offset = 0;
        let mut remaining = text.as_str();
        let mut preferred_ending = None;
        while !remaining.is_empty() {
            let end = ropey::str_utils::line_to_byte_idx(remaining, 1);
            let (line, rest) = remaining.split_at(end);
            let content = crate::line_content_str(line);
            let ending = &line[content.len()..];
            if preferred_ending.is_none() {
                preferred_ending = crate::document::line_ending_kind(ending);
            }
            let mut trimmed = if self.trim_trailing_whitespace {
                content.trim_end_matches([' ', '\t'])
            } else {
                content
            };
            if trimmed.is_empty() && output.ends_with('\r') && ending.starts_with('\n') {
                // Emptying a blank line between a lone CR and an LF would join
                // them into one CRLF break and delete the line.
                trimmed = content;
            }
            let content_len = content.chars().count();
            if trimmed.len() != content.len() {
                // Only ASCII spaces/tabs were removed, so their byte and
                // character lengths agree even in a Unicode document.
                let removed = content.len() - trimmed.len();
                changes.push(TextChange::delete(offset + content_len - removed..offset + content_len));
            }
            output.push_str(trimmed);
            output.push_str(ending);
            offset += content_len + ending.chars().count();
            remaining = rest;
        }
        if self.ensure_final_newline && !output.is_empty() && !output.ends_with(crate::document::is_line_break) {
            // Preserve mixed endings. An added terminator follows the editor's
            // convention (`line_ending_style`): the first CR or LF style, else LF.
            let ending = preferred_ending.unwrap_or("\n");
            output.push_str(ending);
            changes.push(TextChange::insert(offset, ending));
        }
        PreparedSave {
            text: output,
            changes: (!changes.is_empty()).then(|| TextChangeSet::new(changes, 0)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transaction::apply_change_to_buffer;
    use ropey::Rope;

    #[test]
    fn normalization_preserves_terminators_and_its_edits_reproduce_the_written_text() {
        for ending in ["\n", "\r\n", "\r", "\u{b}", "\u{c}", "\u{85}", "\u{2028}", "\u{2029}"] {
            for trailing in ["", ending] {
                let original = format!("é🦀 \t{ending}second\t  {trailing}");
                let prepared = SaveTextOptions {
                    trim_trailing_whitespace: true,
                    ensure_final_newline: true,
                }
                .prepare(original.clone());
                // Only CR and LF terminators set the style of an added one.
                let added = crate::document::line_ending_kind(ending).unwrap_or("\n");
                let expected_end = if trailing.is_empty() { added } else { ending };
                assert_eq!(prepared.text(), format!("é🦀{ending}second{expected_end}"));
                let mut buffer = Rope::from_str(&original);
                for change in prepared.changes().unwrap().as_slice().iter().rev() {
                    apply_change_to_buffer(&mut buffer, change);
                }
                assert_eq!(buffer.to_string(), prepared.text());
                let again = SaveTextOptions {
                    trim_trailing_whitespace: true,
                    ensure_final_newline: true,
                }
                .prepare(prepared.text().to_string());
                assert!(again.changes().is_none());
            }
        }
    }

    #[test]
    fn normalization_handles_empty_documents_mixed_endings_and_independent_options() {
        for (input, trim, newline, expected) in [
            ("", true, true, ""),
            (" \t", true, true, ""),
            (" \t", false, true, " \t\n"),
            ("x  ", false, false, "x  "),
            ("x  ", true, false, "x"),
            ("x  ", false, true, "x  \n"),
            ("x \r\ny \nz\t", true, true, "x\r\ny\nz\r\n"),
            // Trimming the blank line would turn CR + LF into one CRLF break.
            ("x\r \ny", true, true, "x\r \ny\r"),
            ("x\r \t\ny\r\t\r", true, false, "x\r \t\ny\r\r"),
        ] {
            let saved = SaveTextOptions {
                trim_trailing_whitespace: trim,
                ensure_final_newline: newline,
            }
            .prepare(input.to_string());
            assert_eq!(saved.text(), expected, "{input:?}");
        }
    }
}
