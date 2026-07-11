use super::{
    catalog::{self, GrammarId},
    SyntaxLanguage, SyntaxSpan,
};
use crate::ui::theme::SyntaxRole;
use lst_editor::{BufferDelta, BufferEdit};
use ropey::Rope;
use tree_sitter::{InputEdit, Parser, Point, QueryCursor, StreamingIterator, Tree};

/// Per-tab parse state. Owns its own `Parser` and the most recent `Tree`
/// for the buffer, plus a snapshot of the buffer the tree was parsed
/// against (so the next incremental edit can be expressed against the
/// correct pre-edit coordinates).
///
/// Held on the UI thread — `Parser` is `!Send` and the synchronous design
/// is the whole point of the rewrite. Incremental reparses for typical
/// files take microseconds.
pub(crate) struct TabSyntaxState {
    pub(crate) language: SyntaxLanguage,
    /// Revision of the buffer the tree was last parsed against.
    pub(crate) revision: u64,
    parser: Parser,
    tree: Tree,
    /// Snapshot of the buffer at `revision`. `Rope::clone` is O(1) (it
    /// shares the internal node tree), so storing it is cheap.
    parsed_buffer: Rope,
}

impl TabSyntaxState {
    /// `source` must equal `buffer.to_string()`. The caller passes both so
    /// the same string can be reused for `compute_spans` without a second
    /// allocation.
    pub(crate) fn parse_initial(language: SyntaxLanguage, buffer: &Rope, source: &str, revision: u64) -> Option<Self> {
        let grammar = catalog::grammar(catalog::root_grammar(language));
        let mut parser = Parser::new();
        parser.set_language(&grammar.language).ok()?;
        let tree = parser.parse(source, None)?;
        Some(Self {
            language,
            revision,
            parser,
            tree,
            parsed_buffer: buffer.clone(),
        })
    }

    /// Update the tree to match `new_buffer` at `new_revision`, using
    /// `delta` to choose between incremental and full reparse. `new_source`
    /// must equal `new_buffer.to_string()`.
    pub(crate) fn update(&mut self, new_buffer: &Rope, new_source: &str, delta: BufferDelta, new_revision: u64) {
        match delta {
            BufferDelta::Unchanged => {
                // Buffer claims to be unchanged; refresh the snapshot anyway
                // in case a caller reached us with a stale revision.
            }
            BufferDelta::FullReplace => {
                if let Some(tree) = self.parser.parse(new_source, None) {
                    self.tree = tree;
                }
            }
            BufferDelta::Edits(edits) => {
                // Apply edits in reverse order so each edit's pre-batch
                // coordinates (in `self.parsed_buffer`) stay valid — later
                // edits don't shift earlier positions.
                for edit in edits.iter().rev() {
                    let input_edit = input_edit_for(&self.parsed_buffer, edit);
                    self.tree.edit(&input_edit);
                }
                if let Some(tree) = self.parser.parse(new_source, Some(&self.tree)) {
                    self.tree = tree;
                }
            }
        }
        self.parsed_buffer = new_buffer.clone();
        self.revision = new_revision;
    }

    pub(crate) fn compute_spans(&self, source: &str) -> (Vec<Vec<SyntaxSpan>>, Vec<u32>) {
        // Line topology must match `EditorTab::lines()` (which iterates
        // `Rope::lines()` and trims trailing \n/\r), otherwise the byte-
        // length guard in viewport disables the cache for the wrong line
        // indices on files containing lone CR or other Unicode separators
        // that ropey treats as line breaks.
        let (line_starts, display_ends) = line_bounds_from_rope(&self.parsed_buffer);
        let line_byte_lens: Vec<u32> = line_starts
            .iter()
            .zip(display_ends.iter())
            .map(|(start, end)| (end.saturating_sub(*start)) as u32)
            .collect();
        let mut lines = vec![Vec::new(); line_starts.len()];

        let root_grammar = catalog::root_grammar(self.language);
        let mut captures = Vec::new();
        collect_captures(root_grammar, &self.tree, source.as_bytes(), 0, 0, &mut captures);

        emit_non_overlapping_spans(&captures, &mut lines, &line_starts, &display_ends);
        (lines, line_byte_lens)
    }
}

#[derive(Clone, Copy)]
struct CapturedSpan {
    start: usize,
    end: usize,
    role: SyntaxRole,
    /// Recursion depth — 0 for host grammar, 1 for first-level injection,
    /// 2 for an injection inside an injection, etc. Used as a tie-break in
    /// `innermost_role` so deeper grammars win over the outer's same-range
    /// captures (matches tree-sitter's "innermost grammar wins" intent).
    depth: u16,
    /// Pattern index within the grammar's highlights query. Used as a
    /// secondary tie-break so equal-range captures resolve to the later
    /// pattern, matching tree-sitter-highlight's documented precedence.
    pattern_index: u16,
}

struct InjectionMatch {
    content_start: usize,
    content_end: usize,
    embedded: GrammarId,
    /// Default tree-sitter spec: with `injection.include-children` unset,
    /// host captures fully inside the injection content range are
    /// suppressed and only the embedded grammar paints.
    include_children: bool,
}

/// Walks the highlights query of `grammar` over `tree`, then recurses into
/// each injection region. Byte offsets are shifted by `byte_offset` so
/// injected sub-trees land in outer-buffer coordinates. Host captures
/// fully inside a `!include_children` injection range are suppressed to
/// match the default tree-sitter spec.
fn collect_captures(
    grammar: GrammarId,
    tree: &Tree,
    source: &[u8],
    byte_offset: usize,
    depth: u16,
    out: &mut Vec<CapturedSpan>,
) {
    let config = catalog::grammar(grammar);

    let injections = collect_injection_matches(tree, source, config);
    let suppression: Vec<std::ops::Range<usize>> = injections
        .iter()
        .filter(|inj| !inj.include_children)
        .map(|inj| inj.content_start..inj.content_end)
        .collect();

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&config.highlights, tree.root_node(), source);
    while let Some(m) = matches.next() {
        let pattern_index = m.pattern_index as u16;
        for capture in m.captures {
            let Some(role) = config.capture_roles.get(capture.index as usize).copied().flatten() else {
                continue;
            };
            let node = capture.node;
            let start_local = node.start_byte();
            let end_local = node.end_byte();
            if start_local >= end_local {
                continue;
            }
            if suppression.iter().any(|r| r.start <= start_local && end_local <= r.end) {
                continue;
            }
            out.push(CapturedSpan {
                start: byte_offset + start_local,
                end: byte_offset + end_local,
                role,
                depth,
                pattern_index,
            });
        }
    }

    for inj in injections {
        if inj.content_end <= inj.content_start {
            continue;
        }
        let sub_source = &source[inj.content_start..inj.content_end];
        let Some(sub_tree) = parse_sub_source(inj.embedded, sub_source) else {
            continue;
        };
        collect_captures(
            inj.embedded,
            &sub_tree,
            sub_source,
            byte_offset + inj.content_start,
            depth + 1,
            out,
        );
    }
}

fn collect_injection_matches(tree: &Tree, source: &[u8], config: &catalog::GrammarConfig) -> Vec<InjectionMatch> {
    let Some(injections_query) = config.injections.as_ref() else {
        return Vec::new();
    };
    let content_index = config.injection_content_index;
    let language_index = config.injection_language_index;

    let mut out = Vec::new();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(injections_query, tree.root_node(), source);
    while let Some(m) = matches.next() {
        let mut content_node = None;
        let mut language_text: Option<&str> = None;
        for capture in m.captures {
            if Some(capture.index) == content_index {
                content_node = Some(capture.node);
            } else if Some(capture.index) == language_index {
                let range = capture.node.byte_range();
                language_text = std::str::from_utf8(&source[range]).ok();
            }
        }
        let Some(node) = content_node else {
            continue;
        };
        let settings = injections_query.property_settings(m.pattern_index);
        let language_property = settings
            .iter()
            .find(|p| p.key.as_ref() == "injection.language")
            .and_then(|p| p.value.as_deref());
        let embedded = language_text
            .and_then(catalog::injectable_grammar)
            .or_else(|| language_property.and_then(catalog::injectable_grammar))
            .or(config.implicit_injection_grammar);
        let Some(embedded_grammar) = embedded else {
            continue;
        };
        // `(#set! injection.include-children)` without a value means
        // "include host captures inside the injection range". A
        // `(#set! injection.include-children false)` form would carry a
        // value of "false"; treat any value other than "true" / unset as
        // suppress (matching tree-sitter-highlight's default behavior).
        let include_children = settings.iter().any(|p| {
            p.key.as_ref() == "injection.include-children"
                && match p.value.as_deref() {
                    None => true,
                    Some(value) => value.eq_ignore_ascii_case("true"),
                }
        });
        out.push(InjectionMatch {
            content_start: node.start_byte(),
            content_end: node.end_byte(),
            embedded: embedded_grammar,
            include_children,
        });
    }
    out
}

fn parse_sub_source(grammar: GrammarId, source: &[u8]) -> Option<Tree> {
    let config = catalog::grammar(grammar);
    let mut parser = Parser::new();
    parser.set_language(&config.language).ok()?;
    parser.parse(source, None)
}

/// Converts a flat list of captured spans (which may overlap or nest) into
/// non-overlapping, innermost-wins per-line spans suitable for the renderer.
fn emit_non_overlapping_spans(
    captures: &[CapturedSpan],
    lines: &mut [Vec<SyntaxSpan>],
    line_starts: &[usize],
    display_ends: &[usize],
) {
    if captures.is_empty() {
        return;
    }

    #[derive(Clone, Copy)]
    struct Event {
        byte: usize,
        is_end: bool,
        capture_ix: usize,
    }
    let mut events: Vec<Event> = Vec::with_capacity(captures.len() * 2);
    for (ix, cap) in captures.iter().enumerate() {
        events.push(Event {
            byte: cap.start,
            is_end: false,
            capture_ix: ix,
        });
        events.push(Event {
            byte: cap.end,
            is_end: true,
            capture_ix: ix,
        });
    }
    // At the same byte, closes come before opens so a capture ending here
    // doesn't briefly co-exist with a sibling starting here.
    events.sort_by_key(|e| (e.byte, !e.is_end));

    let mut active: Vec<usize> = Vec::new();
    let mut prev_byte = events[0].byte;
    let mut current_role = innermost_role(&active, captures);

    for event in events {
        if event.byte > prev_byte {
            if let Some(role) = current_role {
                push_highlight_span(lines, line_starts, display_ends, prev_byte, event.byte, role);
            }
            prev_byte = event.byte;
        }
        if event.is_end {
            if let Some(pos) = active.iter().rposition(|&ix| ix == event.capture_ix) {
                active.swap_remove(pos);
            }
        } else {
            active.push(event.capture_ix);
        }
        current_role = innermost_role(&active, captures);
    }
}

fn innermost_role(active: &[usize], captures: &[CapturedSpan]) -> Option<SyntaxRole> {
    // Smallest range wins (nested tree nodes are properly contained, so
    // the narrowest capture is the deepest). Tie-breaks match
    // tree-sitter-highlight's documented precedence: deeper grammar
    // (injected over host), then later pattern in the query.
    active
        .iter()
        .map(|&ix| captures[ix])
        .min_by(|a, b| {
            let len_a = a.end - a.start;
            let len_b = b.end - b.start;
            len_a
                .cmp(&len_b)
                .then_with(|| b.depth.cmp(&a.depth))
                .then_with(|| b.pattern_index.cmp(&a.pattern_index))
        })
        .map(|cap| cap.role)
}

/// Build per-line `(start_byte, display_end_byte)` pairs by walking the
/// rope. Ropey treats LF, CR, CRLF, NEL, VT, FF, LS, PS as line
/// separators by default, so naive `\n`-only scanning over the source
/// string disagrees with `EditorTab::lines()` on files containing lone
/// CR (or other Unicode line terminators). Using `Rope::lines()` keeps
/// the line indices and per-line lengths consistent with the renderer.
fn line_bounds_from_rope(buffer: &Rope) -> (Vec<usize>, Vec<usize>) {
    let len_lines = buffer.len_lines();
    let mut line_starts = Vec::with_capacity(len_lines);
    let mut display_ends = Vec::with_capacity(len_lines);
    let mut cursor = 0usize;
    for line in buffer.lines() {
        line_starts.push(cursor);
        let line_str = line.to_string();
        let bytes = line_str.as_bytes();
        let mut end = bytes.len();
        while end > 0 && matches!(bytes[end - 1], b'\n' | b'\r') {
            end -= 1;
        }
        display_ends.push(cursor + end);
        cursor += bytes.len();
    }
    if line_starts.is_empty() {
        line_starts.push(0);
        display_ends.push(0);
    }
    (line_starts, display_ends)
}

fn push_highlight_span(
    lines: &mut [Vec<SyntaxSpan>],
    line_starts: &[usize],
    display_ends: &[usize],
    mut start: usize,
    end: usize,
    role: SyntaxRole,
) {
    while start < end {
        let line_ix = line_starts.partition_point(|offset| *offset <= start).saturating_sub(1);
        let line_start = line_starts[line_ix];
        let display_end = display_ends[line_ix];
        let next_line_start = line_starts.get(line_ix + 1).copied().unwrap_or(end);
        let visible_end = end.min(display_end);

        if start < visible_end {
            lines[line_ix].push(SyntaxSpan {
                start: start - line_start,
                end: visible_end - line_start,
                role,
            });
        }

        if end <= next_line_start {
            break;
        }
        start = next_line_start;
    }
}

/// Build a tree-sitter `InputEdit` for one `BufferEdit` applied to
/// `old_buffer`. `old_buffer` reflects the buffer state *before* this
/// edit (and before any later edits in the same batch — see
/// `apply_edits_and_reparse` which iterates in reverse to preserve that).
fn input_edit_for(old_buffer: &Rope, edit: &BufferEdit) -> InputEdit {
    let start_char = edit.range.start.min(old_buffer.len_chars());
    let old_end_char = edit.range.end.min(old_buffer.len_chars());
    let start_byte = old_buffer.char_to_byte(start_char);
    let old_end_byte = old_buffer.char_to_byte(old_end_char);
    let new_end_byte = start_byte + edit.replacement.len();

    let start_position = point_at_byte(old_buffer, start_char, start_byte);
    let old_end_position = point_at_byte(old_buffer, old_end_char, old_end_byte);
    let new_end_position = position_after_insertion(start_position, &edit.replacement);

    InputEdit {
        start_byte,
        old_end_byte,
        new_end_byte,
        start_position,
        old_end_position,
        new_end_position,
    }
}

fn point_at_byte(buffer: &Rope, char_offset: usize, byte_offset: usize) -> Point {
    let line = buffer.char_to_line(char_offset);
    let line_start_char = buffer.line_to_char(line);
    let line_start_byte = buffer.char_to_byte(line_start_char);
    Point {
        row: line,
        column: byte_offset - line_start_byte,
    }
}

fn position_after_insertion(start: Point, replacement: &str) -> Point {
    let bytes = replacement.as_bytes();
    let mut row = start.row;
    let mut last_line_start = 0usize;
    for (i, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' {
            row += 1;
            last_line_start = i + 1;
        }
    }
    let column = if row == start.row {
        start.column + bytes.len()
    } else {
        bytes.len() - last_line_start
    };
    Point { row, column }
}
