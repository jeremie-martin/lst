use iced::advanced::text::highlighter::{self, Highlighter};
use iced::{Color, Font, Theme};
use std::ops::Range;

// ── Catppuccin Mocha palette ─────────────────────────────────────────────────

const BLUE: Color = Color::from_rgb(0.537, 0.706, 0.980); // #89b4fa
const PEACH: Color = Color::from_rgb(0.980, 0.702, 0.529); // #fab387
const PINK: Color = Color::from_rgb(0.961, 0.761, 0.906); // #f5c2e7
const GREEN: Color = Color::from_rgb(0.651, 0.890, 0.631); // #a6e3a1
const SAPPHIRE: Color = Color::from_rgb(0.455, 0.780, 0.925); // #74c7ec
const OVERLAY0: Color = Color::from_rgb(0.424, 0.439, 0.525); // #6c7086
const MAUVE: Color = Color::from_rgb(0.796, 0.651, 0.969); // #cba6f7
const LAVENDER: Color = Color::from_rgb(0.706, 0.745, 0.996); // #b4befe
const YELLOW: Color = Color::from_rgb(0.976, 0.886, 0.659); // #f9e2af
const SURFACE1: Color = Color::from_rgb(0.271, 0.278, 0.353); // #45475a

// ── Highlight types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct Settings;

#[derive(Debug, Clone, Copy)]
pub enum Highlight {
    Heading,
    HeadingMarker,
    Bold,
    Italic,
    Code,
    CodeBlock,
    Link,
    Url,
    ListMarker,
    BlockQuote,
    HorizontalRule,
}

impl Highlight {
    fn color(self) -> Color {
        match self {
            Self::Heading => BLUE,
            Self::HeadingMarker => YELLOW,
            Self::Bold => PEACH,
            Self::Italic => PINK,
            Self::Code | Self::CodeBlock => GREEN,
            Self::Link => SAPPHIRE,
            Self::Url => OVERLAY0,
            Self::ListMarker => MAUVE,
            Self::BlockQuote => LAVENDER,
            Self::HorizontalRule => SURFACE1,
        }
    }
}

pub fn format(highlight: &Highlight, _theme: &Theme) -> highlighter::Format<Font> {
    highlighter::Format {
        color: Some(highlight.color()),
        font: None,
    }
}

// ── State cached per line (for multi-line code blocks) ───────────────────────

#[derive(Clone, Copy)]
struct LineState {
    in_code_block: bool,
    fence: Option<(char, usize)>,
}

// ── Highlighter implementation ───────────────────────────────────────────────

pub struct MdHighlighter {
    /// `states[i]` = state AFTER processing line i (entering line i+1).
    states: Vec<LineState>,
    in_code_block: bool,
    fence: Option<(char, usize)>,
    current_line: usize,
}

impl Highlighter for MdHighlighter {
    type Settings = Settings;
    type Highlight = Highlight;
    type Iterator<'a> = std::vec::IntoIter<(Range<usize>, Highlight)>;

    fn new(_settings: &Settings) -> Self {
        Self {
            states: Vec::new(),
            in_code_block: false,
            fence: None,
            current_line: 0,
        }
    }

    fn update(&mut self, _settings: &Settings) {}

    fn change_line(&mut self, line: usize) {
        self.states.truncate(line);
        if line == 0 {
            self.in_code_block = false;
            self.fence = None;
        } else if let Some(s) = self.states.last() {
            self.in_code_block = s.in_code_block;
            self.fence = s.fence;
        }
        self.current_line = line;
    }

    fn current_line(&self) -> usize {
        self.current_line
    }

    fn highlight_line(&mut self, line: &str) -> Self::Iterator<'_> {
        let spans = self.process_line(line);

        self.states.push(LineState {
            in_code_block: self.in_code_block,
            fence: self.fence,
        });
        self.current_line += 1;

        spans.into_iter()
    }
}

impl MdHighlighter {
    fn process_line(&mut self, line: &str) -> Vec<(Range<usize>, Highlight)> {
        let mut spans = Vec::new();
        let trimmed = line.trim_start();

        // Inside a fenced code block?
        if self.in_code_block {
            if let Some((fc, fl)) = self.fence {
                // Check for closing fence
                if is_closing_fence(trimmed, fc, fl) {
                    self.in_code_block = false;
                    self.fence = None;
                }
            }
            if !line.is_empty() {
                spans.push((0..line.len(), Highlight::CodeBlock));
            }
            return spans;
        }

        // Opening code fence: ``` or ~~~
        if let Some(fence) = parse_code_fence(trimmed) {
            self.in_code_block = true;
            self.fence = Some(fence);
            if !line.is_empty() {
                spans.push((0..line.len(), Highlight::CodeBlock));
            }
            return spans;
        }

        // Block-level patterns
        highlight_block(line, trimmed, &mut spans);
        spans
    }
}

// ── Block-level highlighting ─────────────────────────────────────────────────

fn highlight_block(line: &str, trimmed: &str, spans: &mut Vec<(Range<usize>, Highlight)>) {
    if trimmed.is_empty() {
        return;
    }

    let indent = line.len() - trimmed.len();

    // Horizontal rule: ---, ***, ___
    if is_horizontal_rule(trimmed) {
        spans.push((0..line.len(), Highlight::HorizontalRule));
        return;
    }

    // ATX heading: # through ######
    if trimmed.starts_with('#') {
        let level = trimmed.bytes().take_while(|&b| b == b'#').count();
        if level <= 6 && trimmed.as_bytes().get(level) == Some(&b' ') {
            spans.push((indent..indent + level, Highlight::HeadingMarker));
            spans.push((indent + level..line.len(), Highlight::Heading));
            return;
        }
    }

    // Block quote
    if trimmed.starts_with('>') {
        spans.push((0..line.len(), Highlight::BlockQuote));
        return;
    }

    // Unordered list: - item, * item, + item
    if trimmed.len() >= 2
        && matches!(trimmed.as_bytes()[0], b'-' | b'*' | b'+')
        && trimmed.as_bytes()[1] == b' '
    {
        spans.push((indent..indent + 1, Highlight::ListMarker));
        highlight_inline(line, indent + 2, spans);
        return;
    }

    // Ordered list: 1. item, 2. item, etc.
    if let Some(dot) = trimmed.find(". ") {
        if dot <= 9
            && !trimmed[..dot].is_empty()
            && trimmed[..dot].bytes().all(|b| b.is_ascii_digit())
        {
            spans.push((indent..indent + dot + 1, Highlight::ListMarker));
            highlight_inline(line, indent + dot + 2, spans);
            return;
        }
    }

    // Plain paragraph — inline highlights only
    highlight_inline(line, indent, spans);
}

// ── Inline highlighting ──────────────────────────────────────────────────────

fn highlight_inline(line: &str, start: usize, spans: &mut Vec<(Range<usize>, Highlight)>) {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut i = start;

    while i < len {
        match bytes[i] {
            // Inline code
            b'`' => {
                if let Some(end) = find_closing(bytes, i + 1, b'`') {
                    spans.push((i..end + 1, Highlight::Code));
                    i = end + 1;
                    continue;
                }
            }

            // Bold (**text** or __text__)
            b'*' | b'_' if i + 2 < len && bytes[i + 1] == bytes[i] => {
                let marker = bytes[i];
                // _ requires word boundary (CommonMark flanking rules)
                if marker == b'_' && i > 0 && bytes[i - 1].is_ascii_alphanumeric() {
                    i += 1;
                    continue;
                }
                if let Some(end) = find_double_closing(bytes, i + 2, marker) {
                    if marker == b'_' && end + 2 < len && bytes[end + 2].is_ascii_alphanumeric() {
                        i += 1;
                        continue;
                    }
                    spans.push((i..end + 2, Highlight::Bold));
                    i = end + 2;
                    continue;
                }
            }

            // Italic (*text* or _text_), only if not start of bold
            b'*' | b'_' if i + 1 < len && bytes[i + 1] != bytes[i] => {
                let marker = bytes[i];
                if marker == b'_' && i > 0 && bytes[i - 1].is_ascii_alphanumeric() {
                    i += 1;
                    continue;
                }
                if let Some(end) = find_closing(bytes, i + 1, marker) {
                    if end + 1 >= len || bytes[end + 1] != marker {
                        if marker == b'_' && end + 1 < len && bytes[end + 1].is_ascii_alphanumeric()
                        {
                            i += 1;
                            continue;
                        }
                        spans.push((i..end + 1, Highlight::Italic));
                        i = end + 1;
                        continue;
                    }
                }
            }

            // Image ![alt](url) — check before link
            b'!' if i + 1 < len && bytes[i + 1] == b'[' => {
                if let Some((text_end, url_end)) = parse_link(bytes, i + 1) {
                    spans.push((i..text_end + 1, Highlight::Link));
                    spans.push((text_end + 1..url_end + 1, Highlight::Url));
                    i = url_end + 1;
                    continue;
                }
            }

            // Link [text](url)
            b'[' => {
                if let Some((text_end, url_end)) = parse_link(bytes, i) {
                    spans.push((i..text_end + 1, Highlight::Link));
                    spans.push((text_end + 1..url_end + 1, Highlight::Url));
                    i = url_end + 1;
                    continue;
                }
            }

            _ => {}
        }

        i += 1;
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn parse_code_fence(trimmed: &str) -> Option<(char, usize)> {
    let first = *trimmed.as_bytes().first()?;
    if first != b'`' && first != b'~' {
        return None;
    }
    let count = trimmed.bytes().take_while(|&b| b == first).count();
    if count >= 3 {
        Some((first as char, count))
    } else {
        None
    }
}

fn is_closing_fence(trimmed: &str, fence_char: char, fence_len: usize) -> bool {
    let count = trimmed
        .bytes()
        .take_while(|&b| b == fence_char as u8)
        .count();
    count >= fence_len && trimmed[count..].trim().is_empty()
}

fn is_horizontal_rule(trimmed: &str) -> bool {
    if trimmed.len() < 3 {
        return false;
    }
    let first = trimmed.as_bytes()[0];
    matches!(first, b'-' | b'*' | b'_')
        && trimmed.bytes().all(|b| b == first || b == b' ')
        && trimmed.bytes().filter(|&b| b == first).count() >= 3
}

fn find_closing(bytes: &[u8], start: usize, marker: u8) -> Option<usize> {
    for i in start..bytes.len() {
        if bytes[i] == marker && (i == start || bytes[i - 1] != b'\\') {
            return Some(i);
        }
    }
    None
}

fn find_double_closing(bytes: &[u8], start: usize, marker: u8) -> Option<usize> {
    let mut i = start;
    while i + 1 < bytes.len() {
        if bytes[i] == marker && bytes[i + 1] == marker && (i == start || bytes[i - 1] != b'\\') {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Parse `[text](url)` starting at the `[`. Returns (close_bracket_pos, close_paren_pos).
fn parse_link(bytes: &[u8], open: usize) -> Option<(usize, usize)> {
    let close_bracket = find_closing(bytes, open + 1, b']')?;
    if close_bracket + 1 >= bytes.len() || bytes[close_bracket + 1] != b'(' {
        return None;
    }
    let close_paren = find_closing(bytes, close_bracket + 2, b')')?;
    Some((close_bracket, close_paren))
}
