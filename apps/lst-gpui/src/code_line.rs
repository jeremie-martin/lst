//! Painted code segments.
//!
//! GPUI shapes text through cosmic-text, which builds a rustybuzz shape plan
//! for every word. A freshly visible code line therefore costs a few hundred
//! microseconds to shape, which dominates scrolling, page navigation, and
//! first paint. Plain monospace ASCII text (the vast majority of code) has a
//! trivially known layout: glyph `n` sits at column `n`. This module paints
//! such segments from a per-token glyph cache at fixed column positions, and
//! keeps GPUI's shaped line for anything else (non-ASCII text, tabs, fonts
//! whose advances are not uniform).

use gpui::{
    point, px, size, App, Bounds, Font, FontId, GlyphId, Hsla, Pixels, ShapedLine, SharedString, TextRun, Window,
};
use std::{collections::HashMap, ops::Range, rc::Rc};

/// Tokens longer than this are shaped as a whole line instead of cached;
/// a pathological run of punctuation should not fill the cache.
const MAX_TOKEN_LEN: usize = 64;
/// Bound on cached tokens per viewport; the cache is cleared when reached.
const MAX_CACHED_TOKENS: usize = 16 * 1024;
/// Tolerance when checking that a token's shaped width is a whole number of
/// cells, in pixels.
const CELL_WIDTH_TOLERANCE: f32 = 0.05;

/// One painted code segment.
#[derive(Clone)]
pub(crate) enum CodeLine {
    /// Shaped by GPUI; glyph positions come from the shaper.
    Shaped(Rc<ShapedLine>),
    /// Plain monospace ASCII; glyph positions are column multiples.
    Cells(Rc<CellLine>),
}

impl CodeLine {
    pub(crate) fn text(&self) -> &str {
        match self {
            Self::Shaped(line) => line.text.as_ref(),
            Self::Cells(line) => line.text.as_ref(),
        }
    }

    /// X offset of the boundary before `local_char`, relative to the line origin.
    pub(crate) fn x_for_char(&self, local_char: usize) -> Pixels {
        match self {
            Self::Shaped(line) => line.x_for_index(char_to_byte(line.text.as_ref(), local_char)),
            Self::Cells(line) => line.char_width * local_char.min(line.text.len()) as f32,
        }
    }

    /// Character boundary closest to `x`, relative to the line origin.
    pub(crate) fn closest_char_for_x(&self, x: Pixels) -> usize {
        match self {
            Self::Shaped(line) => byte_index_to_char(line.text.as_ref(), line.closest_index_for_x(x)),
            Self::Cells(line) => {
                let width = f32::from(line.char_width);
                if width <= 0.0 {
                    return 0;
                }
                ((f32::from(x) / width).round().max(0.0) as usize).min(line.text.len())
            }
        }
    }

    /// Paints the segment with its top-left corner at `origin`. `visible` is
    /// the x range, relative to `origin`, that can appear on screen.
    pub(crate) fn paint(
        &self,
        origin: gpui::Point<Pixels>,
        line_height: Pixels,
        visible: Range<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) {
        match self {
            Self::Shaped(line) => {
                let _ = line.paint(origin, line_height, window, cx);
            }
            Self::Cells(line) => line.paint(origin, line_height, visible, window),
        }
    }
}

/// A plain monospace ASCII segment resolved to glyphs at column positions.
pub(crate) struct CellLine {
    text: SharedString,
    char_width: Pixels,
    font_size: Pixels,
    ascent: Pixels,
    descent: Pixels,
    glyphs: Vec<CellGlyph>,
    /// Exclusive byte end and color of each style run, in text order.
    colors: Vec<(usize, Hsla)>,
}

#[derive(Clone, Copy)]
struct CellGlyph {
    font_id: FontId,
    id: GlyphId,
    /// X offset relative to the line origin.
    x: Pixels,
    /// Byte index of the glyph's cluster within the segment text.
    index: usize,
}

impl CellLine {
    fn paint(&self, origin: gpui::Point<Pixels>, line_height: Pixels, visible: Range<Pixels>, window: &mut Window) {
        let padding_top = (line_height - self.ascent - self.descent) / 2.0;
        let baseline_y = origin.y + padding_top + self.ascent;
        // Ligatures span a few cells; keep a small margin so a glyph that
        // starts left of the visible range still paints its visible part.
        let left = visible.start - self.char_width * f32::from(MAX_TOKEN_LEN as u16);
        let line_bounds = Bounds::new(origin, size(self.char_width * self.text.len() as f32, line_height));
        // One layer per line, as GPUI's own line painting does: primitives
        // inside a layer share its draw order instead of each taking a
        // bounds-tree insertion, and the caret painted afterwards stays on top.
        window.paint_layer(line_bounds, |window| {
            let mut colors = self.colors.iter().peekable();
            for glyph in &self.glyphs {
                if glyph.x >= visible.end {
                    break;
                }
                while colors.peek().is_some_and(|(end, _)| glyph.index >= *end) {
                    colors.next();
                }
                if glyph.x < left {
                    continue;
                }
                let color = colors.peek().map_or(gpui::black(), |(_, color)| *color);
                let _ = window.paint_glyph(
                    point(origin.x + glyph.x, baseline_y),
                    glyph.font_id,
                    glyph.id,
                    self.font_size,
                    color,
                );
            }
        });
    }
}

/// Glyphs of one shaped token, positioned relative to the token start.
struct TokenGlyphs {
    glyphs: Vec<CellGlyph>,
    ascent: Pixels,
    descent: Pixels,
}

/// Shaped tokens for one font and size. Tokens are maximal runs of
/// identifier characters or of punctuation, mirroring the word boundaries
/// cosmic-text shapes within, so ligatures inside punctuation survive.
#[derive(Default)]
pub(crate) struct GlyphTokenCache {
    key: Option<(Font, Pixels)>,
    tokens: HashMap<Box<str>, Option<Rc<TokenGlyphs>>>,
}

impl GlyphTokenCache {
    fn prepare(&mut self, font: &Font, font_size: Pixels) {
        if self
            .key
            .as_ref()
            .is_none_or(|(key_font, key_size)| key_font != font || *key_size != font_size)
        {
            self.key = Some((font.clone(), font_size));
            self.tokens.clear();
        }
        if self.tokens.len() >= MAX_CACHED_TOKENS {
            self.tokens.clear();
        }
    }

    /// Returns `None` when the token is not a whole number of uniform cells
    /// in this font, in which case the whole segment must be shaped.
    fn token(
        &mut self,
        text: &str,
        font: &Font,
        font_size: Pixels,
        char_width: Pixels,
        window: &Window,
    ) -> Option<Rc<TokenGlyphs>> {
        if let Some(cached) = self.tokens.get(text) {
            return cached.clone();
        }
        let layout = window.text_system().layout_line(
            text,
            font_size,
            &[TextRun {
                len: text.len(),
                font: font.clone(),
                color: gpui::black(),
                background_color: None,
                underline: None,
                strikethrough: None,
            }],
            None,
        );
        let expected_width = char_width * text.len() as f32;
        let mut glyphs = Vec::new();
        let mut uniform = (f32::from(layout.width) - f32::from(expected_width)).abs() <= CELL_WIDTH_TOLERANCE;
        for run in &layout.runs {
            for glyph in &run.glyphs {
                let expected_x = char_width * glyph.index as f32;
                uniform &= !glyph.is_emoji
                    && (f32::from(glyph.position.x) - f32::from(expected_x)).abs() <= CELL_WIDTH_TOLERANCE;
                glyphs.push(CellGlyph {
                    font_id: run.font_id,
                    id: glyph.id,
                    x: glyph.position.x,
                    index: glyph.index,
                });
            }
        }
        let token = uniform.then(|| {
            Rc::new(TokenGlyphs {
                glyphs,
                ascent: layout.ascent,
                descent: layout.descent,
            })
        });
        self.tokens.insert(text.into(), token.clone());
        token
    }
}

/// Whether `text` can be painted from cells: ASCII without tabs or control
/// characters other than what the display text already carries.
pub(crate) fn is_cell_text(text: &str) -> bool {
    text.bytes().all(|byte| byte.is_ascii() && byte != b'\t')
}

/// Builds a cell line for plain monospace ASCII `text` colored by `runs`.
/// Returns `None` when a token does not shape to uniform cells, so the caller
/// falls back to GPUI shaping.
pub(crate) fn build_cell_line(
    cache: &mut GlyphTokenCache,
    text: SharedString,
    runs: &[TextRun],
    font: &Font,
    font_size: Pixels,
    char_width: Pixels,
    window: &Window,
) -> Option<Rc<CellLine>> {
    debug_assert!(is_cell_text(text.as_ref()));
    cache.prepare(font, font_size);
    let bytes = text.as_bytes();
    let mut glyphs = Vec::with_capacity(bytes.len());
    let mut ascent = px(0.0);
    let mut descent = px(0.0);
    let mut start = 0;
    while start < bytes.len() {
        let class = token_class(bytes[start]);
        let mut end = start + 1;
        while end < bytes.len() && token_class(bytes[end]) == class && end - start < MAX_TOKEN_LEN {
            end += 1;
        }
        if class != TokenClass::Space {
            let token = cache.token(&text[start..end], font, font_size, char_width, window)?;
            let token_x = char_width * start as f32;
            glyphs.extend(token.glyphs.iter().map(|glyph| CellGlyph {
                x: token_x + glyph.x,
                index: start + glyph.index,
                ..*glyph
            }));
            ascent = token.ascent;
            descent = token.descent;
        }
        start = end;
    }
    let mut colors = Vec::with_capacity(runs.len());
    let mut cursor = 0;
    for run in runs {
        cursor += run.len;
        colors.push((cursor, run.color));
    }
    Some(Rc::new(CellLine {
        text,
        char_width,
        font_size,
        ascent,
        descent,
        glyphs,
        colors,
    }))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TokenClass {
    Space,
    Word,
    Punctuation,
}

fn token_class(byte: u8) -> TokenClass {
    match byte {
        b' ' => TokenClass::Space,
        b'0'..=b'9' | b'A'..=b'Z' | b'a'..=b'z' | b'_' => TokenClass::Word,
        _ => TokenClass::Punctuation,
    }
}

fn char_to_byte(text: &str, char_offset: usize) -> usize {
    text.char_indices()
        .nth(char_offset)
        .map(|(i, _)| i)
        .unwrap_or(text.len())
}

pub(crate) fn byte_index_to_char(text: &str, byte_index: usize) -> usize {
    text[..byte_index.min(text.len())].chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell_line(text: &str, char_width: f32) -> CodeLine {
        CodeLine::Cells(Rc::new(CellLine {
            text: SharedString::from(text.to_string()),
            char_width: px(char_width),
            font_size: px(14.0),
            ascent: px(10.0),
            descent: px(3.0),
            glyphs: Vec::new(),
            colors: Vec::new(),
        }))
    }

    #[test]
    fn cell_positions_are_column_multiples_clamped_to_the_text() {
        let line = cell_line("fn main()", 8.0);
        assert_eq!(line.x_for_char(0), px(0.0));
        assert_eq!(line.x_for_char(3), px(24.0));
        assert_eq!(line.x_for_char(50), px(72.0));
    }

    #[test]
    fn closest_cell_rounds_to_the_nearest_boundary() {
        let line = cell_line("abcd", 8.0);
        assert_eq!(line.closest_char_for_x(px(-5.0)), 0);
        assert_eq!(line.closest_char_for_x(px(3.9)), 0);
        assert_eq!(line.closest_char_for_x(px(4.1)), 1);
        assert_eq!(line.closest_char_for_x(px(19.0)), 2);
        assert_eq!(line.closest_char_for_x(px(500.0)), 4);
    }

    #[test]
    fn token_classes_split_words_spaces_and_punctuation() {
        let text = b"let x = foo(&bar)->baz;";
        let mut tokens = Vec::new();
        let mut start = 0;
        while start < text.len() {
            let class = token_class(text[start]);
            let mut end = start + 1;
            while end < text.len() && token_class(text[end]) == class {
                end += 1;
            }
            if class != TokenClass::Space {
                tokens.push(std::str::from_utf8(&text[start..end]).unwrap());
            }
            start = end;
        }
        assert_eq!(tokens, ["let", "x", "=", "foo", "(&", "bar", ")->", "baz", ";"]);
    }

    #[test]
    fn cell_text_excludes_tabs_and_non_ascii() {
        assert!(is_cell_text("plain ascii; punctuation -> ok"));
        assert!(!is_cell_text("tab\there"));
        assert!(!is_cell_text("caf\u{e9}"));
    }
}
