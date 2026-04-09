use iced::advanced::text::highlighter::{self, Highlighter};
use iced::{Color, Font, Theme};
use std::collections::{HashMap, VecDeque};
use std::ops::Range;
use std::sync::LazyLock;

use syntect::highlighting::{
    HighlightState, Highlighter as SyntectHighlighter, RangedHighlightIterator,
    Theme as SyntectTheme,
};
use syntect::parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet};
use tree_sitter_highlight::{
    Highlight as TreeSitterHighlight, HighlightConfiguration,
    HighlightEvent as TreeSitterHighlightEvent, Highlighter as TreeSitterHighlighter,
};

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
static TREE_SITTER_CAPTURE_NAMES: &[&str] = &[
    "attribute",
    "comment",
    "constant",
    "constructor",
    "escape",
    "function",
    "keyword",
    "module",
    "number",
    "operator",
    "property",
    "punctuation",
    "string",
    "type",
    "variable",
];
const TREE_SITTER_TEXT_CACHE_LIMIT: usize = 4_096;
static TREE_SITTER_RUST_CONFIG: LazyLock<HighlightConfiguration> = LazyLock::new(|| {
    let mut config = HighlightConfiguration::new(
        tree_sitter_rust::LANGUAGE.into(),
        "rust",
        tree_sitter_rust::HIGHLIGHTS_QUERY,
        tree_sitter_rust::INJECTIONS_QUERY,
        "",
    )
    .expect("embedded tree-sitter Rust highlight query should be valid");
    config.configure(TREE_SITTER_CAPTURE_NAMES);
    config
});

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
    TreeSitterRust,
    PlainText,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RustHighlightBackend {
    TreeSitter,
    Syntect,
}

fn determine_mode(ext: &Option<String>) -> HighlightMode {
    match ext.as_deref() {
        Some("md") | Some("markdown") => HighlightMode::Markdown,
        Some("rs") => match rust_highlight_backend() {
            RustHighlightBackend::TreeSitter => HighlightMode::TreeSitterRust,
            RustHighlightBackend::Syntect => SYNTAX_SET
                .find_syntax_by_extension("rs")
                .map(HighlightMode::Syntect)
                .unwrap_or(HighlightMode::PlainText),
        },
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
    tree_sitter: Option<TreeSitterHighlighter>,
    tree_sitter_text_cache: HashMap<String, Vec<(Range<usize>, Highlight)>>,
    tree_sitter_text_cache_order: VecDeque<String>,
}

impl LstHighlighter {
    fn init_mode(&mut self) {
        if let HighlightMode::Syntect(syntax) = &self.mode {
            self.full_file_parse = Some(ParseState::new(syntax));
            self.full_file_hl = Some(HighlightState::new(&SYNTECT_HIGHLIGHTER, ScopeStack::new()));
        } else if matches!(self.mode, HighlightMode::TreeSitterRust) {
            self.tree_sitter = Some(TreeSitterHighlighter::new());
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
        // Keep tree_sitter instance and text cache alive across resets.
        // The cache is keyed by line text (not position), so entries
        // remain valid even when lines move or the file is re-highlighted.
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

    fn highlight_tree_sitter_rust_line(&mut self, line: &str) -> Vec<(Range<usize>, Highlight)> {
        if let Some(cached) = self.tree_sitter_text_cache.get(line) {
            return cached.clone();
        }

        let Some(highlighter) = self.tree_sitter.as_mut() else {
            return Vec::new();
        };

        let Ok(events) =
            highlighter.highlight(&TREE_SITTER_RUST_CONFIG, line.as_bytes(), None, |_| None)
        else {
            return Vec::new();
        };

        let mut spans = Vec::new();
        let mut stack: Vec<TreeSitterHighlight> = Vec::new();

        for event in events {
            match event {
                Ok(TreeSitterHighlightEvent::HighlightStart(highlight)) => stack.push(highlight),
                Ok(TreeSitterHighlightEvent::HighlightEnd) => {
                    let _ = stack.pop();
                }
                Ok(TreeSitterHighlightEvent::Source { start, end }) if start < end => {
                    let Some(color) = stack.last().and_then(|highlight| {
                        tree_sitter_color_for_capture(highlight.0)
                    }) else {
                        continue;
                    };
                    spans.push((start..end, Highlight::Syntect(color)));
                }
                Ok(TreeSitterHighlightEvent::Source { .. }) => {}
                Err(_) => return Vec::new(),
            }
        }

        if self.tree_sitter_text_cache.len() >= TREE_SITTER_TEXT_CACHE_LIMIT {
            if let Some(oldest) = self.tree_sitter_text_cache_order.pop_front() {
                self.tree_sitter_text_cache.remove(&oldest);
            }
        }

        let cache_key = line.to_string();
        self.tree_sitter_text_cache_order.push_back(cache_key.clone());
        self.tree_sitter_text_cache
            .insert(cache_key, spans.clone());

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
            tree_sitter: None,
            tree_sitter_text_cache: HashMap::new(),
            tree_sitter_text_cache_order: VecDeque::new(),
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
            HighlightMode::TreeSitterRust => self.highlight_tree_sitter_rust_line(line),
            HighlightMode::PlainText => Vec::new(),
        };
        self.current_line += 1;
        spans.into_iter()
    }

    fn current_line(&self) -> usize {
        self.current_line
    }
}

fn tree_sitter_color_for_capture(index: usize) -> Option<Color> {
    match TREE_SITTER_CAPTURE_NAMES.get(index).copied() {
        Some("attribute") => Some(YELLOW),
        Some("comment") => Some(OVERLAY0),
        Some("constant") => Some(PEACH),
        Some("constructor") => Some(SAPPHIRE),
        Some("escape") => Some(PINK),
        Some("function") => Some(BLUE),
        Some("keyword") => Some(MAUVE),
        Some("module") => Some(LAVENDER),
        Some("number") => Some(PEACH),
        Some("operator") => Some(SAPPHIRE),
        Some("property") => Some(LAVENDER),
        Some("punctuation") => Some(SURFACE1),
        Some("string") => Some(GREEN),
        Some("type") => Some(YELLOW),
        Some("variable") => None,
        _ => None,
    }
}

fn rust_highlight_backend() -> RustHighlightBackend {
    rust_highlight_backend_from_env(std::env::var("LST_HIGHLIGHT_BACKEND").ok().as_deref())
}

fn rust_highlight_backend_from_env(value: Option<&str>) -> RustHighlightBackend {
    match value {
        Some("syntect") => RustHighlightBackend::Syntect,
        _ => RustHighlightBackend::TreeSitter,
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

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(all(test, feature = "internal-invariants"))]
mod tests {
    use super::*;
    use iced::advanced::text::highlighter::Highlighter as _H;

    fn block_spans(line: &str) -> Vec<(Range<usize>, Highlight)> {
        let mut spans = Vec::new();
        highlight_block(line, line.trim_start(), &mut spans);
        spans
    }

    fn inline_spans(line: &str) -> Vec<(Range<usize>, Highlight)> {
        let mut spans = Vec::new();
        highlight_inline(line, 0, &mut spans);
        spans
    }

    fn md_highlighter() -> LstHighlighter {
        LstHighlighter::new(&Settings {
            extension: Some("md".to_string()),
        })
    }

    fn highlight_lines(
        h: &mut LstHighlighter,
        lines: &[&str],
    ) -> Vec<Vec<(Range<usize>, Highlight)>> {
        lines
            .iter()
            .map(|line| h.highlight_line(line).collect())
            .collect()
    }

    // ── parse_code_fence_with_lang ─────────────────────────────────────

    #[test]
    fn fence_backtick_no_lang() {
        let result = parse_code_fence_with_lang("```");
        assert_eq!(result.as_ref().map(|r| r.0), Some(('`', 3)));
        assert_eq!(result.as_ref().and_then(|r| r.1.as_deref()), None);
    }

    #[test]
    fn fence_tilde_no_lang() {
        let result = parse_code_fence_with_lang("~~~");
        assert_eq!(result.as_ref().map(|r| r.0), Some(('~', 3)));
    }

    #[test]
    fn fence_with_language() {
        let result = parse_code_fence_with_lang("```rust");
        assert_eq!(result.as_ref().and_then(|r| r.1.as_deref()), Some("rust"));
    }

    #[test]
    fn fence_language_lowercased() {
        let result = parse_code_fence_with_lang("```Rust");
        assert_eq!(result.as_ref().and_then(|r| r.1.as_deref()), Some("rust"));
    }

    #[test]
    fn fence_four_backticks() {
        let result = parse_code_fence_with_lang("````");
        assert_eq!(result.as_ref().map(|r| r.0), Some(('`', 4)));
    }

    #[test]
    fn fence_two_backticks_rejected() {
        assert!(parse_code_fence_with_lang("``").is_none());
    }

    #[test]
    fn fence_non_fence_line() {
        assert!(parse_code_fence_with_lang("hello world").is_none());
    }

    // ── is_closing_fence ──────────────────────────────────────────────

    #[test]
    fn closing_fence_exact_match() {
        assert!(is_closing_fence("```", '`', 3));
    }

    #[test]
    fn closing_fence_longer_is_ok() {
        assert!(is_closing_fence("````", '`', 3));
    }

    #[test]
    fn closing_fence_shorter_rejected() {
        assert!(!is_closing_fence("``", '`', 3));
    }

    #[test]
    fn closing_fence_trailing_whitespace() {
        assert!(is_closing_fence("```  ", '`', 3));
    }

    #[test]
    fn closing_fence_trailing_text_rejected() {
        assert!(!is_closing_fence("``` foo", '`', 3));
    }

    // ── is_horizontal_rule ────────────────────────────────────────────

    #[test]
    fn hr_dashes() {
        assert!(is_horizontal_rule("---"));
    }

    #[test]
    fn hr_asterisks() {
        assert!(is_horizontal_rule("***"));
    }

    #[test]
    fn hr_underscores() {
        assert!(is_horizontal_rule("___"));
    }

    #[test]
    fn hr_with_spaces() {
        assert!(is_horizontal_rule("- - -"));
    }

    #[test]
    fn hr_two_chars_rejected() {
        assert!(!is_horizontal_rule("--"));
    }

    #[test]
    fn hr_mixed_chars_rejected() {
        assert!(!is_horizontal_rule("-*-"));
    }

    // ── find_closing / find_double_closing ──────────────────────────────

    #[test]
    fn find_closing_found() {
        assert_eq!(find_closing(b"hello`world", 0, b'`'), Some(5));
    }

    #[test]
    fn find_closing_not_found() {
        assert_eq!(find_closing(b"hello world", 0, b'`'), None);
    }

    #[test]
    fn find_closing_escaped_skipped() {
        // \` should not match, but the next ` should
        assert_eq!(find_closing(b"a\\`b`", 1, b'`'), Some(4));
    }

    #[test]
    fn find_double_closing_found() {
        assert_eq!(find_double_closing(b"ab**cd", 0, b'*'), Some(2));
    }

    #[test]
    fn find_double_closing_not_found() {
        assert_eq!(find_double_closing(b"ab*cd", 0, b'*'), None);
    }

    #[test]
    fn find_double_closing_escaped() {
        // The \** at index 2 is escaped, so the match is the ** at index 5
        assert_eq!(find_double_closing(b"a\\**b**", 0, b'*'), Some(5));
    }

    // ── parse_link ───────────────────────────────────────────────────

    #[test]
    fn link_valid() {
        let bytes = b"[text](url)";
        assert_eq!(parse_link(bytes, 0), Some((5, 10)));
    }

    #[test]
    fn link_missing_paren() {
        assert_eq!(parse_link(b"[text]url", 0), None);
    }

    #[test]
    fn link_no_close_bracket() {
        assert_eq!(parse_link(b"[text(url)", 0), None);
    }

    // ── Block highlighting ─────────────────────────────────────────────

    #[test]
    fn block_heading_h1() {
        let spans = block_spans("# Hello");
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].0, 0..1);
        assert!(matches!(spans[0].1, Highlight::HeadingMarker));
        assert_eq!(spans[1].0, 1..7);
        assert!(matches!(spans[1].1, Highlight::Heading));
    }

    #[test]
    fn block_heading_h3() {
        let spans = block_spans("### Title");
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].0, 0..3);
        assert!(matches!(spans[0].1, Highlight::HeadingMarker));
    }

    #[test]
    fn block_heading_h7_not_heading() {
        let spans = block_spans("####### nope");
        // Should not be a heading — falls through to inline
        assert!(spans.iter().all(|(_, h)| !matches!(h, Highlight::Heading)));
    }

    #[test]
    fn block_heading_no_space_not_heading() {
        let spans = block_spans("#no_space");
        assert!(spans.iter().all(|(_, h)| !matches!(h, Highlight::Heading)));
    }

    #[test]
    fn block_blockquote() {
        let spans = block_spans("> quoted text");
        assert_eq!(spans.len(), 1);
        assert!(matches!(spans[0].1, Highlight::BlockQuote));
        assert_eq!(spans[0].0, 0..13);
    }

    #[test]
    fn block_unordered_list_dash() {
        let spans = block_spans("- item");
        assert_eq!(spans[0].0, 0..1);
        assert!(matches!(spans[0].1, Highlight::ListMarker));
    }

    #[test]
    fn block_unordered_list_asterisk() {
        let spans = block_spans("* item");
        assert!(matches!(spans[0].1, Highlight::ListMarker));
    }

    #[test]
    fn block_ordered_list() {
        let spans = block_spans("1. item");
        assert_eq!(spans[0].0, 0..2);
        assert!(matches!(spans[0].1, Highlight::ListMarker));
    }

    #[test]
    fn block_horizontal_rule() {
        let spans = block_spans("---");
        assert_eq!(spans.len(), 1);
        assert!(matches!(spans[0].1, Highlight::HorizontalRule));
    }

    #[test]
    fn block_empty_line() {
        assert!(block_spans("").is_empty());
        assert!(block_spans("   ").is_empty());
    }

    #[test]
    fn block_indented_heading() {
        let spans = block_spans("  ## Indented");
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].0, 2..4);
        assert!(matches!(spans[0].1, Highlight::HeadingMarker));
        assert_eq!(spans[1].0, 4..13);
        assert!(matches!(spans[1].1, Highlight::Heading));
    }

    // ── Inline highlighting ───────────────────────────────────────────

    #[test]
    fn inline_code() {
        let spans = inline_spans("hello `code` world");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].0, 6..12);
        assert!(matches!(spans[0].1, Highlight::Code));
    }

    #[test]
    fn inline_bold_asterisks() {
        let spans = inline_spans("**bold**");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].0, 0..8);
        assert!(matches!(spans[0].1, Highlight::Bold));
    }

    #[test]
    fn inline_bold_underscores() {
        let spans = inline_spans("__bold__");
        assert_eq!(spans.len(), 1);
        assert!(matches!(spans[0].1, Highlight::Bold));
    }

    #[test]
    fn inline_italic_asterisk() {
        let spans = inline_spans("*italic*");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].0, 0..8);
        assert!(matches!(spans[0].1, Highlight::Italic));
    }

    #[test]
    fn inline_italic_underscore() {
        let spans = inline_spans("_italic_");
        assert_eq!(spans.len(), 1);
        assert!(matches!(spans[0].1, Highlight::Italic));
    }

    #[test]
    fn inline_link() {
        let spans = inline_spans("[text](url)");
        assert_eq!(spans.len(), 2);
        assert!(matches!(spans[0].1, Highlight::Link));
        assert_eq!(spans[0].0, 0..6);
        assert!(matches!(spans[1].1, Highlight::Url));
        assert_eq!(spans[1].0, 6..11);
    }

    #[test]
    fn inline_image() {
        let spans = inline_spans("![alt](img.png)");
        assert_eq!(spans.len(), 2);
        assert!(matches!(spans[0].1, Highlight::Link));
        assert!(matches!(spans[1].1, Highlight::Url));
    }

    #[test]
    fn inline_escaped_closing_backtick() {
        // Escaped closing backtick doesn't end the span — no match found
        let spans = inline_spans("`code \\` more`");
        assert_eq!(spans.len(), 1);
        assert!(matches!(spans[0].1, Highlight::Code));
        // The span runs from the first ` to the final `, skipping the escaped one
        assert_eq!(spans[0].0, 0..14);
    }

    #[test]
    fn inline_mid_word_underscore_suppressed() {
        let spans = inline_spans("foo_bar_baz");
        // Mid-word underscores should not create italic
        assert!(spans.iter().all(|(_, h)| !matches!(h, Highlight::Italic)));
    }

    #[test]
    fn inline_multiple_elements() {
        let spans = inline_spans("`code` and **bold**");
        assert_eq!(spans.len(), 2);
        assert!(matches!(spans[0].1, Highlight::Code));
        assert!(matches!(spans[1].1, Highlight::Bold));
    }

    // ── Multi-line state machine ──────────────────────────────────────

    #[test]
    fn md_code_block_lifecycle() {
        let mut h = md_highlighter();
        let results = highlight_lines(&mut h, &["# Title", "```", "some code", "```", "paragraph"]);

        // Line 0: heading
        assert!(results[0]
            .iter()
            .any(|(_, hl)| matches!(hl, Highlight::Heading)));

        // Line 1: fence open → CodeBlock
        assert!(results[1]
            .iter()
            .all(|(_, hl)| matches!(hl, Highlight::CodeBlock)));

        // Line 2: inside fence → CodeBlock
        assert!(results[2]
            .iter()
            .all(|(_, hl)| matches!(hl, Highlight::CodeBlock)));

        // Line 3: fence close → CodeBlock
        assert!(results[3]
            .iter()
            .all(|(_, hl)| matches!(hl, Highlight::CodeBlock)));

        // Line 4: back to normal markdown (no CodeBlock)
        assert!(results[4]
            .iter()
            .all(|(_, hl)| !matches!(hl, Highlight::CodeBlock)));
    }

    #[test]
    fn md_change_line_restores_code_block_state() {
        let mut h = md_highlighter();
        // Build up state through 3 lines
        highlight_lines(&mut h, &["```", "inside", "```"]);

        // Jump back to line 1 (inside code block)
        h.change_line(1);
        let spans: Vec<_> = h.highlight_line("different content").collect();
        assert!(spans
            .iter()
            .all(|(_, hl)| matches!(hl, Highlight::CodeBlock)));
    }

    #[test]
    fn md_change_line_zero_resets() {
        let mut h = md_highlighter();
        highlight_lines(&mut h, &["```", "code"]);

        // Reset to start
        h.change_line(0);
        let spans: Vec<_> = h.highlight_line("# Heading").collect();
        assert!(spans.iter().any(|(_, hl)| matches!(hl, Highlight::Heading)));
    }

    #[test]
    fn plain_text_returns_no_spans() {
        let mut h = LstHighlighter::new(&Settings { extension: None });
        let spans: Vec<_> = h.highlight_line("# not highlighted").collect();
        assert!(spans.is_empty());
    }

    #[test]
    fn rust_mode_returns_colored_spans() {
        let mut h = LstHighlighter::new(&Settings {
            extension: Some("rs".to_string()),
        });
        let spans: Vec<_> = h.highlight_line("fn main() {}").collect();
        assert!(!spans.is_empty());
        assert!(spans
            .iter()
            .all(|(_, hl)| matches!(hl, Highlight::Syntect(_))));
    }

    #[test]
    fn rust_tree_sitter_cache_reuses_same_text_on_a_different_line() {
        let mut h = LstHighlighter::new(&Settings {
            extension: Some("rs".to_string()),
        });
        let _first: Vec<_> = h.highlight_line("fn alpha() {}").collect();
        let expected: Vec<_> = h.highlight_line("let beta = 1;").collect();

        h.change_line(50);
        h.tree_sitter = None;

        let reused: Vec<_> = h.highlight_line("let beta = 1;").collect();
        assert_eq!(
            reused.iter().map(|(range, _)| range.clone()).collect::<Vec<_>>(),
            expected
                .iter()
                .map(|(range, _)| range.clone())
                .collect::<Vec<_>>()
        );
        assert!(reused
            .iter()
            .all(|(_, hl)| matches!(hl, Highlight::Syntect(_))));
    }

    #[test]
    fn rust_tree_sitter_cache_does_not_reuse_stale_line_text() {
        let mut h = LstHighlighter::new(&Settings {
            extension: Some("rs".to_string()),
        });
        let _first: Vec<_> = h.highlight_line("fn alpha() {}").collect();
        let _second: Vec<_> = h.highlight_line("let beta = 1;").collect();

        h.change_line(50);
        h.tree_sitter = None;

        let changed: Vec<_> = h.highlight_line("let gamma = 2;").collect();
        assert!(changed.is_empty());
    }

    #[test]
    fn rust_backend_defaults_to_tree_sitter() {
        assert_eq!(
            rust_highlight_backend_from_env(None),
            RustHighlightBackend::TreeSitter
        );
        assert_eq!(
            rust_highlight_backend_from_env(Some("tree-sitter")),
            RustHighlightBackend::TreeSitter
        );
        assert_eq!(
            rust_highlight_backend_from_env(Some("unexpected")),
            RustHighlightBackend::TreeSitter
        );
    }

    #[test]
    fn rust_backend_accepts_syntect_fallback() {
        assert_eq!(
            rust_highlight_backend_from_env(Some("syntect")),
            RustHighlightBackend::Syntect
        );
    }

    #[test]
    fn md_fenced_code_with_language_uses_syntect() {
        let mut h = md_highlighter();
        let results = highlight_lines(&mut h, &["```rust", "fn main() {}", "```"]);

        // Line 0: fence → CodeBlock
        assert!(results[0]
            .iter()
            .all(|(_, hl)| matches!(hl, Highlight::CodeBlock)));

        // Line 1: inside rust fence → should get Syntect colors, not plain CodeBlock
        assert!(!results[1].is_empty());
        assert!(results[1]
            .iter()
            .all(|(_, hl)| matches!(hl, Highlight::Syntect(_))));
    }
}
