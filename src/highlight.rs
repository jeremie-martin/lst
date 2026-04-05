use iced::advanced::text::highlighter::{self, Highlighter};
use iced::{Color, Font, Theme};
use std::ops::Range;
use std::sync::LazyLock;

use syntect::highlighting::{
    HighlightState, Highlighter as SyntectHighlighter, RangedHighlightIterator,
    Theme as SyntectTheme,
};
use syntect::parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet};

// ── Shared syntect state (initialized once) ─────────────────────────────────

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);

static THEME: LazyLock<SyntectTheme> = LazyLock::new(|| {
    let data = include_bytes!("catppuccin-mocha.tmTheme");
    let mut cursor = std::io::Cursor::new(&data[..]);
    syntect::highlighting::ThemeSet::load_from_reader(&mut cursor)
        .expect("embedded Catppuccin Mocha theme should be valid")
});

static SYNTECT_HIGHLIGHTER: LazyLock<SyntectHighlighter<'static>> =
    LazyLock::new(|| SyntectHighlighter::new(&THEME));

// ── Catppuccin Mocha palette (for hand-rolled Markdown highlights) ──────────

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

// ── Highlight types ─────────────────────────────────────────────────────────

#[derive(Clone, PartialEq)]
pub struct Settings {
    pub extension: Option<String>,
}

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
    Syntect(Color),
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
            Self::Syntect(c) => c,
        }
    }
}

pub fn format(highlight: &Highlight, _theme: &Theme) -> highlighter::Format<Font> {
    highlighter::Format {
        color: Some(highlight.color()),
        font: None,
    }
}

// ── Highlight mode ──────────────────────────────────────────────────────────

enum HighlightMode {
    Markdown,
    Syntect(&'static SyntaxReference),
    PlainText,
}

fn determine_mode(ext: &Option<String>) -> HighlightMode {
    match ext.as_deref() {
        Some("md") | Some("markdown") => HighlightMode::Markdown,
        None => HighlightMode::PlainText,
        Some(ext) => match SYNTAX_SET.find_syntax_by_extension(ext) {
            Some(syntax) => HighlightMode::Syntect(syntax),
            None => HighlightMode::PlainText,
        },
    }
}

// ── Per-line state cache ────────────────────────────────────────────────────

#[derive(Clone)]
struct MdLineState {
    in_code_block: bool,
    fence: Option<(char, usize)>,
    code_lang: Option<String>,
    syntect_parse: Option<ParseState>,
    syntect_hl: Option<HighlightState>,
}

#[derive(Clone)]
struct SyntectLineState {
    parse_state: ParseState,
    highlight_state: HighlightState,
}

#[derive(Clone)]
enum LineState {
    Md(MdLineState),
    Syntect(SyntectLineState),
}

// ── Unified highlighter ─────────────────────────────────────────────────────

pub struct LstHighlighter {
    mode: HighlightMode,
    states: Vec<LineState>,
    current_line: usize,
    line_buffer: String,
    // Markdown working state
    in_code_block: bool,
    fence: Option<(char, usize)>,
    code_lang: Option<String>,
    syntect_parse: Option<ParseState>,
    syntect_hl: Option<HighlightState>,
    // Full-file syntect working state
    full_file_parse: Option<ParseState>,
    full_file_hl: Option<HighlightState>,
}

impl LstHighlighter {
    fn init_mode(&mut self) {
        if let HighlightMode::Syntect(syntax) = &self.mode {
            self.full_file_parse = Some(ParseState::new(syntax));
            self.full_file_hl = Some(HighlightState::new(&SYNTECT_HIGHLIGHTER, ScopeStack::new()));
        }
    }

    fn reset(&mut self) {
        self.states.clear();
        self.current_line = 0;
        self.line_buffer.clear();
        self.in_code_block = false;
        self.fence = None;
        self.code_lang = None;
        self.syntect_parse = None;
        self.syntect_hl = None;
        self.full_file_parse = None;
        self.full_file_hl = None;
    }

    // ── Markdown mode ───────────────────────────────────────────────────

    fn highlight_md_line(&mut self, line: &str) -> Vec<(Range<usize>, Highlight)> {
        let mut spans = Vec::new();
        let trimmed = line.trim_start();

        if self.in_code_block {
            if let Some((fc, fl)) = self.fence {
                if is_closing_fence(trimmed, fc, fl) {
                    self.in_code_block = false;
                    self.fence = None;
                    self.code_lang = None;
                    self.syntect_parse = None;
                    self.syntect_hl = None;
                    if !line.is_empty() {
                        spans.push((0..line.len(), Highlight::CodeBlock));
                    }
                    self.push_md_state();
                    return spans;
                }
            }

            if self.syntect_parse.is_some() {
                spans = self.highlight_code_line(line);
            } else if !line.is_empty() {
                spans.push((0..line.len(), Highlight::CodeBlock));
            }
            self.push_md_state();
            return spans;
        }

        // Opening code fence
        if let Some((fence, lang)) = parse_code_fence_with_lang(trimmed) {
            self.in_code_block = true;
            self.fence = Some(fence);
            if let Some(ref lang) = lang {
                if let Some(syntax) = SYNTAX_SET.find_syntax_by_token(lang) {
                    self.syntect_parse = Some(ParseState::new(syntax));
                    self.syntect_hl =
                        Some(HighlightState::new(&SYNTECT_HIGHLIGHTER, ScopeStack::new()));
                }
            }
            self.code_lang = lang;
            if !line.is_empty() {
                spans.push((0..line.len(), Highlight::CodeBlock));
            }
            self.push_md_state();
            return spans;
        }

        highlight_block(line, trimmed, &mut spans);
        self.push_md_state();
        spans
    }

    fn push_md_state(&mut self) {
        self.states.push(LineState::Md(MdLineState {
            in_code_block: self.in_code_block,
            fence: self.fence,
            code_lang: self.code_lang.clone(),
            syntect_parse: self.syntect_parse.clone(),
            syntect_hl: self.syntect_hl.clone(),
        }));
    }

    // ── Syntect line highlighting (shared by MD code blocks and full-file) ──

    fn highlight_code_line(&mut self, line: &str) -> Vec<(Range<usize>, Highlight)> {
        let parse = self.syntect_parse.as_mut().unwrap();
        let hl_state = self.syntect_hl.as_mut().unwrap();
        run_syntect_line(parse, hl_state, &mut self.line_buffer, line)
    }

    // ── Full-file syntect mode ──────────────────────────────────────────

    fn highlight_syntect_line(&mut self, line: &str) -> Vec<(Range<usize>, Highlight)> {
        let parse = self.full_file_parse.as_mut().unwrap();
        let hl_state = self.full_file_hl.as_mut().unwrap();
        let spans = run_syntect_line(parse, hl_state, &mut self.line_buffer, line);
        self.states.push(LineState::Syntect(SyntectLineState {
            parse_state: parse.clone(),
            highlight_state: hl_state.clone(),
        }));
        spans
    }
}

fn run_syntect_line(
    parse: &mut ParseState,
    hl_state: &mut HighlightState,
    line_buffer: &mut String,
    line: &str,
) -> Vec<(Range<usize>, Highlight)> {
    line_buffer.clear();
    line_buffer.push_str(line);
    line_buffer.push('\n');

    let ops = match parse.parse_line(line_buffer, &SYNTAX_SET) {
        Ok(ops) => ops,
        Err(_) => return Vec::new(),
    };

    let iter = RangedHighlightIterator::new(hl_state, &ops, line_buffer, &SYNTECT_HIGHLIGHTER);

    let mut spans = Vec::new();
    for (style, _text, range) in iter {
        let start = range.start.min(line.len());
        let end = range.end.min(line.len());
        if start < end {
            spans.push((
                start..end,
                Highlight::Syntect(Color::from_rgb8(
                    style.foreground.r,
                    style.foreground.g,
                    style.foreground.b,
                )),
            ));
        }
    }
    spans
}

// ── Highlighter trait implementation ────────────────────────────────────────

impl Highlighter for LstHighlighter {
    type Settings = Settings;
    type Highlight = Highlight;
    type Iterator<'a> = std::vec::IntoIter<(Range<usize>, Highlight)>;

    fn new(settings: &Settings) -> Self {
        let mode = determine_mode(&settings.extension);
        let mut h = Self {
            mode,
            states: Vec::new(),
            current_line: 0,
            line_buffer: String::new(),
            in_code_block: false,
            fence: None,
            code_lang: None,
            syntect_parse: None,
            syntect_hl: None,
            full_file_parse: None,
            full_file_hl: None,
        };
        h.init_mode();
        h
    }

    fn update(&mut self, settings: &Settings) {
        self.mode = determine_mode(&settings.extension);
        self.reset();
        self.init_mode();
    }

    fn change_line(&mut self, line: usize) {
        self.states.truncate(line);
        if line == 0 {
            self.reset();
            self.init_mode();
        } else if let Some(state) = self.states.last() {
            match state.clone() {
                LineState::Md(md) => {
                    self.in_code_block = md.in_code_block;
                    self.fence = md.fence;
                    self.code_lang = md.code_lang;
                    self.syntect_parse = md.syntect_parse;
                    self.syntect_hl = md.syntect_hl;
                }
                LineState::Syntect(s) => {
                    self.full_file_parse = Some(s.parse_state);
                    self.full_file_hl = Some(s.highlight_state);
                }
            }
        }
        self.current_line = line;
    }

    fn highlight_line(&mut self, line: &str) -> Self::Iterator<'_> {
        let spans = match &self.mode {
            HighlightMode::Markdown => self.highlight_md_line(line),
            HighlightMode::Syntect(_) => self.highlight_syntect_line(line),
            HighlightMode::PlainText => Vec::new(),
        };
        self.current_line += 1;
        spans.into_iter()
    }

    fn current_line(&self) -> usize {
        self.current_line
    }
}

// ── Block-level Markdown highlighting ───────────────────────────────────────

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

// ── Inline Markdown highlighting ────────────────────────────────────────────

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

// ── Helpers ─────────────────────────────────────────────────────────────────

fn parse_code_fence_with_lang(trimmed: &str) -> Option<((char, usize), Option<String>)> {
    let first = *trimmed.as_bytes().first()?;
    if first != b'`' && first != b'~' {
        return None;
    }
    let count = trimmed.bytes().take_while(|&b| b == first).count();
    if count < 3 {
        return None;
    }
    let info = trimmed[count..].trim();
    let lang = if info.is_empty() {
        None
    } else {
        info.split_whitespace()
            .next()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_lowercase())
    };
    Some(((first as char, count), lang))
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
    (start..bytes.len()).find(|&i| bytes[i] == marker && (i == start || bytes[i - 1] != b'\\'))
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
